//! CSV Import Module
//!
//! Parses CSV files with transaction data and imports them into the ledger.
//! Expected format: date,description,amount,account_id_or_number,entry_type

use uuid::Uuid;
use chrono::{Utc, NaiveDate};
use rust_decimal::Decimal;
use std::collections::HashMap;

use crate::database::financial::{EntryType, Transaction, TransactionEntry, TransactionType, TransactionStatus};
use crate::accounting::ledger::Ledger;

/// Errors during CSV import.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("CSV parse error: {0}")]
    ParseError(String),
    #[error("Invalid date: {0}")]
    InvalidDate(String),
    #[error("Invalid amount: {0}")]
    InvalidAmount(String),
    #[error("Unknown account: {0}")]
    UnknownAccount(String),
    #[error("Ledger error: {0}")]
    LedgerError(String),
    #[error("Empty file")]
    EmptyFile,
}

pub type ImportResult<T> = Result<T, ImportError>;

/// A parsed CSV row representing a transaction entry.
#[derive(Debug, Clone)]
pub struct CsvRow {
    pub date: NaiveDate,
    pub description: String,
    pub amount: Decimal,
    pub account_number: String,
    pub entry_type: EntryType,
    pub reference: Option<String>,
}

/// Parse a single CSV line into fields, handling quoted fields with embedded
/// commas, escaped double-quotes, and leading/trailing whitespace.
///
/// Rules:
/// - Fields containing commas are wrapped in double quotes: `"field, with comma"`
/// - Double quotes inside quoted fields are escaped by doubling: `"say ""hi"""`
/// - Quotes only start a quoted field at the beginning of a field
pub fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                // Check for escaped quote ("")
                if chars.peek() == Some(&'"') {
                    current.push('"');
                    chars.next(); // consume the second quote
                } else {
                    // End of quoted field
                    in_quotes = false;
                }
            } else {
                current.push(c);
            }
        } else {
            match c {
                '"' if current.is_empty() || current.chars().all(|ch| ch.is_whitespace()) => {
                    // Start of a quoted field (only if at the beginning of a field)
                    in_quotes = true;
                    // Clear any leading whitespace we may have accumulated
                    current.clear();
                }
                '"' => {
                    // Quote in the middle of an unquoted field — include literally
                    current.push(c);
                }
                ',' => {
                    fields.push(current.trim().to_string());
                    current = String::new();
                }
                _ => {
                    current.push(c);
                }
            }
        }
    }
    fields.push(current.trim().to_string());
    fields
}

/// Parse CSV content into rows.
///
/// Expected headers: date,description,amount,account,entry_type
/// Additional column: reference (optional)
///
/// Handles quoted fields with embedded commas per RFC 4180.
pub fn parse_csv(content: &str) -> ImportResult<Vec<CsvRow>> {
    let mut rows = Vec::new();
    let mut lines = content.lines();

    // Skip header
    let header = lines.next().ok_or(ImportError::EmptyFile)?;
    let headers: Vec<String> = parse_csv_line(header).into_iter()
        .map(|s| s.trim().to_lowercase()).collect();

    let date_idx = headers.iter().position(|h| h == "date").unwrap_or(0);
    let desc_idx = headers.iter().position(|h| h == "description").unwrap_or(1);
    let amount_idx = headers.iter().position(|h| h == "amount").unwrap_or(2);
    let account_idx = headers.iter().position(|h| h == "account").unwrap_or(3);
    let entry_idx = headers.iter().position(|h| h == "entry_type").unwrap_or(4);
    let ref_idx = headers.iter().position(|h| h == "reference");

    for (line_num, line) in lines.enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let fields: Vec<String> = parse_csv_line(line);
        if fields.len() < 5 {
            return Err(ImportError::ParseError(format!(
                "Line {} has {} fields, expected at least 5",
                line_num + 2,
                fields.len()
            )));
        }

        let date_str = fields.get(date_idx).map(|s| s.as_str()).unwrap_or("");
        let date = parse_date(date_str)
            .map_err(|e| ImportError::InvalidDate(format!("Line {}: {}", line_num + 2, e)))?;

        let description = fields.get(desc_idx).map(|s| s.as_str()).unwrap_or("").to_string();
        if description.is_empty() {
            return Err(ImportError::ParseError(format!(
                "Line {}: description is empty",
                line_num + 2
            )));
        }

        let amount_str = fields.get(amount_idx).map(|s| s.as_str()).unwrap_or("0");
        let amount: Decimal = amount_str
            .parse()
            .map_err(|_| ImportError::InvalidAmount(format!("Line {}: '{}'", line_num + 2, amount_str)))?;

        let account_number = fields.get(account_idx).map(|s| s.as_str()).unwrap_or("").to_string();

        let entry_type_str = fields.get(entry_idx).map(|s| s.as_str()).unwrap_or("debit").to_lowercase();
        let entry_type = match entry_type_str.as_str() {
            "debit" | "dr" => EntryType::Debit,
            "credit" | "cr" => EntryType::Credit,
            other => return Err(ImportError::ParseError(format!(
                "Line {}: unknown entry_type '{}', expected debit/credit",
                line_num + 2, other
            ))),
        };

        let reference = ref_idx
            .and_then(|i| fields.get(i))
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        rows.push(CsvRow {
            date,
            description,
            amount,
            account_number,
            entry_type,
            reference,
        });
    }

    Ok(rows)
}

/// Import parsed CSV rows into the ledger as transactions.
///
/// Rows are grouped by date+description to form multi-entry transactions.
pub async fn import_csv_rows(ledger: &Ledger, rows: &[CsvRow]) -> ImportResult<Vec<Transaction>> {
    if rows.is_empty() {
        return Err(ImportError::EmptyFile);
    }

    let accounts = ledger
        .list_accounts()
        .await
        .map_err(|e| ImportError::LedgerError(e.to_string()))?;

    // Build a lookup: account_number → account
    let account_map: HashMap<&str, &crate::database::financial::Account> =
        accounts.iter().map(|a| (a.number.as_str(), a)).collect();

    let mut transactions = Vec::new();

    // Group rows by date+description (for multi-entry transactions)
    let mut groups: Vec<Vec<&CsvRow>> = Vec::new();
    for row in rows {
        // Try to add to the last group if it matches date+description
        if let Some(last) = groups.last_mut() {
            if last[0].date == row.date && last[0].description == row.description {
                last.push(row);
                continue;
            }
        }
        groups.push(vec![row]);
    }

    for group in &groups {
        let mut entries = Vec::new();

        for row in group {
            let account = account_map
                .get(row.account_number.as_str())
                .ok_or_else(|| ImportError::UnknownAccount(format!(
                    "Account number '{}' not found",
                    row.account_number
                )))?;

            entries.push(TransactionEntry {
                id: Uuid::new_v4(),
                account_id: account.id,
                amount: row.amount,
                entry_type: row.entry_type.clone(),
                description: row.description.clone(),
                reference: row.reference.clone(),
                ..Default::default()
            });
        }

        // Balance validation: verify debits == credits before posting
        let total_debits: Decimal = entries.iter()
            .filter(|e| e.entry_type == EntryType::Debit)
            .map(|e| e.amount)
            .sum();
        let total_credits: Decimal = entries.iter()
            .filter(|e| e.entry_type == EntryType::Credit)
            .map(|e| e.amount)
            .sum();
        if total_debits != total_credits {
            // Build a detailed error listing which rows are unbalanced
            let row_details: Vec<String> = group.iter().enumerate()
                .map(|(i, r)| format!(
                    "  row {}: date={}, desc={}, amount={}, type={}",
                    i + 1, r.date, r.description, r.amount,
                    if r.entry_type == EntryType::Debit { "debit" } else { "credit" }
                ))
                .collect();
            return Err(ImportError::ParseError(format!(
                "Unbalanced transaction group '{}': debits={}, credits={}\n{}",
                group[0].description, total_debits, total_credits,
                row_details.join("\n")
            )));
        }

        let date_utc = group[0]
            .date
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc();

        let txn = Transaction {
            id: Uuid::new_v4(),
            number: format!("IMP-{}", &Uuid::new_v4().to_string()[..8]),
            description: group[0].description.clone(),
            date: date_utc,
            transaction_type: TransactionType::JournalEntry,
            status: TransactionStatus::Pending,
            entries,
            journal_entry_id: None,
            document_ids: vec![],
            metadata: serde_json::json!({ "imported": true }),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let recorded = ledger
            .record_transaction(txn)
            .await
            .map_err(|e| ImportError::LedgerError(e.to_string()))?;

        transactions.push(recorded);
    }

    Ok(transactions)
}

/// Convenience: parse CSV content and import directly into the ledger.
pub async fn import_csv(ledger: &Ledger, content: &str) -> ImportResult<Vec<Transaction>> {
    let rows = parse_csv(content)?;
    import_csv_rows(ledger, &rows).await
}

fn parse_date(s: &str) -> Result<NaiveDate, String> {
    // Try multiple formats
    let formats = ["%Y-%m-%d", "%m/%d/%Y", "%d/%m/%Y", "%Y/%m/%d"];
    for fmt in &formats {
        if let Ok(d) = NaiveDate::parse_from_str(s.trim(), fmt) {
            return Ok(d);
        }
    }
    Err(format!("Cannot parse date '{}'", s))
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_CSV: &str = "\
date,description,amount,account,entry_type
2026-06-15,Office supplies,45.99,5000,debit
2026-06-15,Office supplies,45.99,1000,credit
2026-06-20,Consulting revenue,1500.00,1000,debit
2026-06-20,Consulting revenue,1500.00,4000,credit
";

    #[test]
    fn test_parse_csv_valid() {
        let rows = parse_csv(SAMPLE_CSV).unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].account_number, "5000");
        assert_eq!(rows[0].amount.to_string(), "45.99");
        assert_eq!(rows[1].account_number, "1000");
    }

    #[test]
    fn test_parse_csv_entry_types() {
        let rows = parse_csv(SAMPLE_CSV).unwrap();
        assert_eq!(rows[0].entry_type, EntryType::Debit);
        assert_eq!(rows[1].entry_type, EntryType::Credit);
    }

    #[test]
    fn test_parse_csv_empty() {
        assert!(parse_csv("").is_err());
    }

    #[test]
    fn test_parse_csv_skips_comments() {
        let csv = "\
date,description,amount,account,entry_type
# This is a comment
2026-07-01,Test,100.00,5000,debit
";
        let rows = parse_csv(csv).unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn test_parse_date_formats() {
        assert!(parse_date("2026-06-15").is_ok());
        assert!(parse_date("06/15/2026").is_ok());
        assert!(parse_date("not-a-date").is_err());
    }

    #[tokio::test]
    async fn test_import_csv_to_ledger() {
        let mut ledger = Ledger::new();
        ledger.initialize().await.unwrap();

        let result = import_csv(&ledger, SAMPLE_CSV).await.unwrap();
        assert_eq!(result.len(), 2); // 2 transactions (supplies + revenue)

        let txns = ledger.list_transactions().await.unwrap();
        assert!(txns.len() >= 2);
    }

    #[test]
    fn test_parse_csv_line_simple() {
        let fields = parse_csv_line("a,b,c");
        assert_eq!(fields, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_parse_csv_line_quoted_comma() {
        let fields = parse_csv_line(r#""hello, world",b,c"#);
        assert_eq!(fields, vec!["hello, world", "b", "c"]);
    }

    #[test]
    fn test_parse_csv_line_escaped_quotes() {
        let fields = parse_csv_line(r#""say ""hi""",b"#);
        assert_eq!(fields, vec!["say \"hi\"", "b"]);
    }

    #[test]
    fn test_parse_csv_line_multiple_quoted() {
        let fields = parse_csv_line(r#""field one","field, two","field three""#);
        assert_eq!(fields, vec!["field one", "field, two", "field three"]);
    }

    #[test]
    fn test_parse_csv_line_trailing_comma() {
        let fields = parse_csv_line("a,b,");
        assert_eq!(fields, vec!["a", "b", ""]);
    }

    #[test]
    fn test_parse_csv_with_quoted_description() {
        let csv = "\
date,description,amount,account,entry_type
2026-06-15,\"Office supplies, pens and paper\",45.99,5000,debit
2026-06-15,\"Office supplies, pens and paper\",45.99,1000,credit
";
        let rows = parse_csv(csv).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].description, "Office supplies, pens and paper");
        assert_eq!(rows[0].amount.to_string(), "45.99");
    }

    #[tokio::test]
    async fn test_import_unbalanced_returns_error() {
        let mut ledger = Ledger::new();
        ledger.initialize().await.unwrap();

        let csv = "\
date,description,amount,account,entry_type
2026-06-15,Unbalanced entry,50.00,5000,debit
2026-06-15,Unbalanced entry,30.00,1000,credit
";
        let result = import_csv(&ledger, csv).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            ImportError::ParseError(msg) => {
                assert!(msg.contains("Unbalanced"));
                assert!(msg.contains("debits=50"));
                assert!(msg.contains("credits=30"));
            }
            other => panic!("Expected ParseError, got {:?}", other),
        }
    }
}
