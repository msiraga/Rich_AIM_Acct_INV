//! Database Seed Module
//!
//! Seeds the database with default chart of accounts matching the accounts
//! defined in `accounting/ledger.rs::create_default_accounts()`.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use surrealdb::engine::local::Db;
use surrealdb::sql::Datetime;
use surrealdb::Surreal;

use crate::database::error::DatabaseError;

/// Represents a seed account for insertion into the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SeedAccount {
    number: String,
    name: String,
    description: String,
    account_type: String,
    status: String,
    balance: f64,
    currency: String,
    is_bank_account: bool,
    is_reconciled: bool,
    created_at: Datetime,
    updated_at: Datetime,
}

/// Returns the 20 default chart of accounts, matching `accounting/ledger.rs`.
fn default_accounts() -> Vec<SeedAccount> {
    let now = Datetime::from(Utc::now());

    let entries: Vec<(&str, &str, &str)> = vec![
        // Assets
        ("1000", "Cash", "asset"),
        ("1010", "Bank Account", "asset"),
        ("1020", "Accounts Receivable", "asset"),
        ("1030", "Inventory", "asset"),
        ("1040", "Fixed Assets", "asset"),
        ("1050", "Accumulated Depreciation", "asset"),
        // Liabilities
        ("2000", "Accounts Payable", "liability"),
        ("2010", "Loans Payable", "liability"),
        ("2020", "Accrued Expenses", "liability"),
        // Equity
        ("3000", "Owner's Equity", "equity"),
        ("3010", "Retained Earnings", "equity"),
        // Revenue
        ("4000", "Sales Revenue", "revenue"),
        ("4010", "Service Revenue", "revenue"),
        ("4020", "Interest Revenue", "revenue"),
        // Expenses
        ("5000", "Cost of Goods Sold", "expense"),
        ("5010", "Salaries Expense", "expense"),
        ("5020", "Rent Expense", "expense"),
        ("5030", "Utilities Expense", "expense"),
        ("5040", "Office Supplies Expense", "expense"),
        ("5050", "Depreciation Expense", "expense"),
    ];

    entries
        .into_iter()
        .map(|(number, name, account_type)| SeedAccount {
            number: number.to_string(),
            name: name.to_string(),
            description: String::new(),
            account_type: account_type.to_string(),
            status: "active".to_string(),
            balance: 0.0,
            currency: "USD".to_string(),
            is_bank_account: false,
            is_reconciled: false,
            created_at: now.clone(),
            updated_at: now.clone(),
        })
        .collect()
}

/// Seed the default chart of accounts into the database.
///
/// Only inserts accounts that do not already exist (checked by account number).
/// Returns the number of accounts inserted.
pub async fn seed_default_accounts(
    db: &Surreal<Db>,
) -> Result<usize, DatabaseError> {
    let accounts = default_accounts();
    let mut inserted = 0usize;

    for account in &accounts {
        // Check if account already exists by number
        let query = "SELECT * FROM account WHERE number = $number";
        let mut response = db
            .query(query)
            .bind(("number", account.number.clone()))
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let existing: Vec<serde_json::Value> = response
            .take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        if existing.is_empty() {
            let _: Vec<serde_json::Value> = db
                .create("account")
                .content(account.clone())
                .await
                .map_err(|e| DatabaseError::QueryError(e.to_string()))?;
            inserted += 1;
        }
    }

    Ok(inserted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_accounts_count() {
        let accounts = default_accounts();
        assert_eq!(accounts.len(), 20);
    }

    #[test]
    fn test_default_accounts_have_unique_numbers() {
        let accounts = default_accounts();
        let mut numbers: Vec<String> = accounts.iter().map(|a| a.number.clone()).collect();
        numbers.sort();
        numbers.dedup();
        assert_eq!(numbers.len(), accounts.len());
    }

    #[test]
    fn test_default_accounts_cover_all_types() {
        let accounts = default_accounts();
        let types: Vec<&str> = accounts.iter().map(|a| a.account_type.as_str()).collect();
        assert!(types.contains(&"asset"));
        assert!(types.contains(&"liability"));
        assert!(types.contains(&"equity"));
        assert!(types.contains(&"revenue"));
        assert!(types.contains(&"expense"));
    }
}
