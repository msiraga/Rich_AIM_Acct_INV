use nexus_core::database::Database;
use nexus_core::database::schema;
use nexus_core::database::seed;
use nexus_core::database::migrations;
use nexus_core::accounting::ledger::Ledger;
use nexus_core::database::financial::*;
use std::sync::Arc;
use rust_decimal_macros::dec;
use chrono::Utc;

#[tokio::test]
async fn test_database_connect_and_schema() {
    let db = Database::new();
    db.connect().await.unwrap();
    assert!(db.is_connected().await);
}

#[tokio::test]
async fn test_seed_accounts() {
    let db = Database::new();
    db.connect().await.unwrap();
    db.seed().await.unwrap();

    // Verify accounts exist by querying SurrealDB
    let client = db.db().await.unwrap();
    let mut response = client.query("SELECT * FROM account").await.unwrap();
    let accounts: Vec<serde_json::Value> = response.take(0).unwrap();
    assert!(accounts.len() >= 18, "Expected at least 18 seeded accounts, got {}", accounts.len());
}

#[tokio::test]
async fn test_ledger_with_database() {
    let db = Database::new();
    db.connect().await.unwrap();
    db.seed().await.unwrap();

    let mut ledger = Ledger::new();
    ledger.db = Some(Arc::new(db));
    ledger.initialize().await.unwrap();

    // Create accounts
    let cash = ledger.create_account(Account::new("6000", "Test Cash", AccountType::Asset)).await.unwrap();
    let revenue = ledger.create_account(Account::new("7000", "Test Revenue", AccountType::Revenue)).await.unwrap();

    // Record a balanced transaction
    let entries = vec![
        TransactionEntry::new(cash.id, EntryType::Debit, dec!(100), "Cash received"),
        TransactionEntry::new(revenue.id, EntryType::Credit, dec!(100), "Revenue earned"),
    ];
    let txn = Transaction::new("Test transaction".to_string(), Utc::now(), entries);
    let recorded = ledger.record_transaction(txn).await.unwrap();

    assert!(recorded.is_balanced());
    assert!(!recorded.number.is_empty());

    // Verify balances
    let cash_balance = ledger.get_account_balance(cash.id).await.unwrap();
    assert_eq!(cash_balance, dec!(100));

    let revenue_balance = ledger.get_account_balance(revenue.id).await.unwrap();
    assert_eq!(revenue_balance, dec!(100));
}

#[tokio::test]
async fn test_migrations_idempotent() {
    let db = Database::new();
    db.connect().await.unwrap();

    // Run migrations twice — should not fail
    let client = db.db().await.unwrap();
    migrations::run_migrations(&client).await.unwrap();
    migrations::run_migrations(&client).await.unwrap();
}
