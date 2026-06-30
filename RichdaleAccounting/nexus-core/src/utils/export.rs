//! Export Module — CSV and OFX export of transactions.

use crate::database::financial::{Transaction, Account, EntryType};
use crate::accounting::ledger::Ledger;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;
use uuid::Uuid;

/// Export transactions as CSV string.
///
/// Columns: date, number, description, account_number, account_name, debit, credit, amount, status
///
/// The `accounts` slice is used to resolve each entry's `account_id` to a
/// human-readable account number and name.
pub fn export_transactions_csv(transactions: &[Transaction], accounts: &[Account]) -> String {
    let mut csv = String::from("date,number,description,account_number,account_name,debit,credit,amount,status\n");

    // Build a lookup map: account_id → (number, name)
    let account_map: HashMap<Uuid, (&str, &str)> = accounts.iter()
        .map(|a| (a.id, (a.number.as_str(), a.name.as_str())))
        .collect();

    for txn in transactions {
        for entry in &txn.entries {
            let (account_number, account_name) = account_map
                .get(&entry.account_id)
                .copied()
                .unwrap_or(("UNKNOWN", "Unknown Account"));

            let debit = if entry.entry_type == EntryType::Debit {
                entry.amount.to_string()
            } else {
                "0".to_string()
            };
            let credit = if entry.entry_type == EntryType::Credit {
                entry.amount.to_string()
            } else {
                "0".to_string()
            };

            csv.push_str(&format!(
                "{},{},{},{},{},{},{},{},{:?}\n",
                txn.date.format("%Y-%m-%d"),
                escape_csv(&txn.number),
                escape_csv(&txn.description),
                escape_csv(account_number),
                escape_csv(account_name),
                debit,
                credit,
                entry.amount,
                txn.status,
            ));
        }
    }

    csv
}

/// Export ledger transactions as CSV, optionally filtered by date range.
///
/// If `start_date` / `end_date` are `Some`, only transactions within the range
/// (inclusive) are exported.
pub async fn export_ledger_csv(
    ledger: &Ledger,
    start_date: Option<DateTime<Utc>>,
    end_date: Option<DateTime<Utc>>,
) -> Result<String, String> {
    let accounts = ledger
        .list_accounts()
        .await
        .map_err(|e| e.to_string())?;

    let transactions = match (start_date, end_date) {
        (Some(start), Some(end)) => {
            ledger.list_transactions_by_date(start, end).await
        }
        (Some(start), None) => {
            let all = ledger.list_transactions().await.map_err(|e| e.to_string())?;
            Ok(all.into_iter().filter(|t| t.date >= start).collect())
        }
        (None, Some(end)) => {
            let all = ledger.list_transactions().await.map_err(|e| e.to_string())?;
            Ok(all.into_iter().filter(|t| t.date <= end).collect())
        }
        (None, None) => ledger.list_transactions().await,
    }
    .map_err(|e| e.to_string())?;

    Ok(export_transactions_csv(&transactions, &accounts))
}

/// Export transactions as OFX (Open Financial Exchange) format for bank import.
///
/// Generates a valid OFX 2.x SGML document with:
/// - Signed `<TRNAMT>`: positive for credits (money in), negative for debits (money out)
/// - `<BANKACCTFROM>` element with bank routing and account numbers
/// - `<LEDGERBAL>` element with the ending balance
pub fn export_transactions_ofx(
    transactions: &[Transaction],
    accounts: &[Account],
) -> String {
    // Resolve the cash/bank account (account 1000) for BANKACCTFROM and LEDGERBAL
    let cash_account = accounts.iter().find(|a| a.number == "1000");

    let bank_id = cash_account
        .and_then(|a| a.bank_details.as_ref())
        .map(|bd| bd.routing_number.as_str())
        .unwrap_or("000000000");
    let acct_id = cash_account
        .and_then(|a| a.bank_details.as_ref())
        .map(|bd| bd.account_number.as_str())
        .unwrap_or("0000000000");
    let acct_type = cash_account
        .and_then(|a| a.bank_details.as_ref())
        .map(|bd| bd.account_type.as_str())
        .unwrap_or("CHECKING");
    let ledger_balance = cash_account
        .map(|a| a.balance)
        .unwrap_or(dec!(0));

    let mut ofx = String::from(
        "OFXHEADER:100\n\
         DATA:OFXSGML\n\
         VERSION:102\n\
         SECURITY:NONE\n\
         ENCODING:USASCII\n\
         CHARSET:1252\n\
         COMPRESSION:NONE\n\
         OLDFILEUID:NONE\n\
         NEWFILEUID:NONE\n\
         <OFX>\n\
         <BANKMSGSRSV1>\n\
         <STMTTRNRS>\n\
         <STMTRS>\n\
         <CURDEF>USD\n\
         <BANKACCTFROM>\n\
         <BANKID>");
    ofx.push_str(bank_id);
    ofx.push_str("</BANKID>\n<ACCTID>");
    ofx.push_str(acct_id);
    ofx.push_str("</ACCTID>\n<ACCTTYPE>");
    ofx.push_str(acct_type);
    ofx.push_str("</ACCTTYPE>\n</BANKACCTFROM>\n");
    ofx.push_str("<BANKTRANLIST>\n");

    for txn in transactions {
        // Determine if this is a credit (money in) or debit (money out).
        // A transaction is a credit to cash if cash has a credit entry,
        // and a debit if cash has a debit entry.
        // For OFX, TRNAMT should be signed: positive = credit, negative = debit.
        let signed_amount = compute_signed_amount_for_ofx(txn, accounts);
        let trn_type = if signed_amount >= Decimal::ZERO { "CREDIT" } else { "DEBIT" };

        ofx.push_str(&format!(
            "  <STMTTRN>\n\
               <TRNTYPE>{}</TRNTYPE>\n\
               <DTPOSTED>{}</DTPOSTED>\n\
               <TRNAMT>{}</TRNAMT>\n\
               <FITID>{}</FITID>\n\
               <NAME>{}</NAME>\n\
               <MEMO>{}</MEMO>\n\
             </STMTTRN>\n",
            trn_type,
            txn.date.format("%Y%m%d"),
            signed_amount,
            txn.id,
            escape_xml(&txn.number),
            escape_xml(&txn.description),
        ));
    }

    ofx.push_str("  </BANKTRANLIST>\n");

    // LEDGERBAL: ending balance as of the last transaction date (or now)
    let bal_date = transactions.last()
        .map(|t| t.date.format("%Y%m%d").to_string())
        .unwrap_or_else(|| Utc::now().format("%Y%m%d").to_string());
    ofx.push_str(&format!(
        "<LEDGERBAL>\n\
         <BALAMT>{}</BALAMT>\n\
         <DTASOF>{}</DTASOF>\n\
         </LEDGERBAL>\n",
        ledger_balance, bal_date
    ));

    ofx.push_str(
        "</STMTRS>\n\
         </STMTTRNRS>\n\
         </BANKMSGSRSV1>\n\
         </OFX>\n"
    );

    ofx
}

/// Compute the signed amount for OFX export.
///
/// Returns a positive value for credits (money into the cash account) and
/// negative for debits (money out of the cash account).
fn compute_signed_amount_for_ofx(txn: &Transaction, accounts: &[Account]) -> Decimal {
    let cash_account_ids: Vec<Uuid> = accounts.iter()
        .filter(|a| a.number == "1000" || a.is_bank_account)
        .map(|a| a.id)
        .collect();

    let mut signed = dec!(0);
    for entry in &txn.entries {
        if cash_account_ids.contains(&entry.account_id) {
            // Cash is an asset: debit increases (money in), credit decreases (money out)
            signed += match entry.entry_type {
                EntryType::Debit => entry.amount,
                EntryType::Credit => -entry.amount,
            };
        }
    }

    // If no cash account entries found, fall back to total_amount with sign
    // based on whether the transaction is a credit or debit to cash overall
    if signed == dec!(0) {
        let total = txn.total_amount();
        // Heuristic: if there are more debit entries, it's likely a debit (money out)
        let debits = txn.entries.iter().filter(|e| e.entry_type == EntryType::Debit).count();
        let credits = txn.entries.iter().filter(|e| e.entry_type == EntryType::Credit).count();
        if debits > credits {
            -total
        } else {
            total
        }
    } else {
        signed
    }
}

fn escape_csv(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::financial::{TransactionType, TransactionStatus, TransactionEntry};
    use chrono::Utc;

    fn make_test_accounts() -> Vec<Account> {
        vec![
            Account {
                id: Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
                number: "1000".into(),
                name: "Cash".into(),
                ..Default::default()
            },
            Account {
                id: Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap(),
                number: "5000".into(),
                name: "Cost of Goods Sold".into(),
                ..Default::default()
            },
        ]
    }

    fn make_test_transaction() -> Transaction {
        let cash_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let expense_id = Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();
        Transaction {
            id: Uuid::new_v4(),
            number: "TXN-001".into(),
            description: "Test transaction".into(),
            date: Utc::now(),
            transaction_type: TransactionType::JournalEntry,
            status: TransactionStatus::Posted,
            entries: vec![
                TransactionEntry {
                    id: Uuid::new_v4(),
                    account_id: expense_id,
                    amount: rust_decimal::Decimal::new(10000, 2),
                    entry_type: EntryType::Debit,
                    description: "Entry 1".into(),
                    reference: None,
                    ..Default::default()
                },
                TransactionEntry {
                    id: Uuid::new_v4(),
                    account_id: cash_id,
                    amount: rust_decimal::Decimal::new(10000, 2),
                    entry_type: EntryType::Credit,
                    description: "Entry 2".into(),
                    reference: None,
                    ..Default::default()
                },
            ],
            journal_entry_id: None,
            document_ids: vec![],
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_export_csv_has_header() {
        let csv = export_transactions_csv(&[], &[]);
        assert!(csv.starts_with("date,number,description"));
    }

    #[test]
    fn test_export_csv_includes_transactions() {
        let txn = make_test_transaction();
        let accounts = make_test_accounts();
        let csv = export_transactions_csv(&[txn], &accounts);
        assert!(csv.contains("TXN-001"));
        assert!(csv.contains("Test transaction"));
        // Verify account resolution
        assert!(csv.contains("1000"));
        assert!(csv.contains("Cash"));
        assert!(csv.contains("5000"));
        assert!(csv.contains("Cost of Goods Sold"));
    }

    #[test]
    fn test_export_ofx_valid() {
        let txn = make_test_transaction();
        let accounts = make_test_accounts();
        let ofx = export_transactions_ofx(&[txn], &accounts);
        assert!(ofx.starts_with("OFXHEADER:100"));
        assert!(ofx.contains("<OFX>"));
        assert!(ofx.contains("<STMTTRN>"));
    }

    #[test]
    fn test_export_ofx_has_bankacctfrom() {
        let txn = make_test_transaction();
        let accounts = make_test_accounts();
        let ofx = export_transactions_ofx(&[txn], &accounts);
        assert!(ofx.contains("<BANKACCTFROM>"));
        assert!(ofx.contains("<BANKID>"));
        assert!(ofx.contains("<ACCTID>"));
        assert!(ofx.contains("<ACCTTYPE>"));
    }

    #[test]
    fn test_export_ofx_has_ledgerbal() {
        let txn = make_test_transaction();
        let accounts = make_test_accounts();
        let ofx = export_transactions_ofx(&[txn], &accounts);
        assert!(ofx.contains("<LEDGERBAL>"));
        assert!(ofx.contains("<BALAMT>"));
        assert!(ofx.contains("<DTASOF>"));
    }

    #[test]
    fn test_export_ofx_signed_trnamt() {
        // The test transaction debits expense and credits cash (money out)
        let txn = make_test_transaction();
        let accounts = make_test_accounts();
        let ofx = export_transactions_ofx(&[txn], &accounts);
        assert!(ofx.contains("<TRNAMT>-"));
        assert!(ofx.contains("DEBIT"));
    }

    #[test]
    fn test_escape_csv_quotes_fields_with_comma() {
        assert_eq!(escape_csv("hello,world"), r#""hello,world""#);
        assert_eq!(escape_csv("simple"), "simple");
    }
}
