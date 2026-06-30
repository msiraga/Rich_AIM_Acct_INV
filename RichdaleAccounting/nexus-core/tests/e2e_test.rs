//! End-to-End Test: Transaction → Ledger
//!
//! Simulates the UI flow: user creates a transaction, it processes through
//! the accounting engine, and appears in the ledger listing.

use nexus_core::NexusLedger;
use nexus_core::database::financial::*;
use rust_decimal_macros::dec;
use chrono::Utc;
use uuid::Uuid;

#[tokio::test]
async fn test_e2e_create_transaction_appears_in_ledger() {
    // 1. Initialize NexusLedger (simulates app startup)
    let mut nexus = NexusLedger::new();
    nexus.initialize().await.expect("NexusLedger should initialize");

    // 2. Get initial state — should have seed accounts
    let accounts = nexus.ledger.list_accounts().await.expect("should list accounts");
    assert!(accounts.len() >= 18, "Expected >= 18 seeded accounts, got {}", accounts.len());

    let cash_account = accounts.iter().find(|a| a.number == "1000")
        .expect("Cash account (1000) should exist");
    let revenue_account = accounts.iter().find(|a| a.number == "4000")
        .expect("Sales Revenue account (4000) should exist");

    let initial_cash_balance = cash_account.balance;
    let initial_revenue_balance = revenue_account.balance;

    // 3. Create a transaction (simulates UI form submit → POST /api/v1/transactions)
    let txn = Transaction {
        id: Uuid::new_v4(),
        number: "E2E-TEST-001".into(),
        description: "E2E Test: Consulting services revenue".into(),
        date: Utc::now(),
        transaction_type: TransactionType::JournalEntry,
        status: TransactionStatus::Pending,
        entries: vec![
            TransactionEntry {
                id: Uuid::new_v4(),
                account_id: cash_account.id,
                entry_type: EntryType::Debit,
                amount: dec!(1500.00),
                description: "Cash received".into(),
                reference: None,
            },
            TransactionEntry {
                id: Uuid::new_v4(),
                account_id: revenue_account.id,
                entry_type: EntryType::Credit,
                amount: dec!(1500.00),
                description: "Consulting revenue".into(),
                reference: None,
            },
        ],
        journal_entry_id: None,
        document_ids: vec![],
        metadata: serde_json::json!({}),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    // 4. Process through the ledger engine
    let processed = nexus.ledger.record_transaction(txn.clone())
        .await
        .expect("Transaction should be recorded");

    assert_eq!(processed.number, "E2E-TEST-001");

    // 5. Verify the transaction appears in the ledger listing
    let transactions = nexus.ledger.list_transactions().await
        .expect("should list transactions");

    let found = transactions.iter().find(|t| t.id == processed.id)
        .expect("Created transaction should appear in listing");

    assert_eq!(found.description, "E2E Test: Consulting services revenue");
    assert_eq!(found.total_amount(), dec!(1500.00));
    assert_eq!(found.entries.len(), 2);

    // 6. Verify account balances updated correctly (double-entry)
    let updated_accounts = nexus.ledger.list_accounts().await.unwrap();
    let updated_cash = updated_accounts.iter().find(|a| a.id == cash_account.id).unwrap();
    let updated_revenue = updated_accounts.iter().find(|a| a.id == revenue_account.id).unwrap();

    // Cash (Asset): Debit increases → balance should go UP by $1500
    assert_eq!(updated_cash.balance, initial_cash_balance + dec!(1500.00));

    // Revenue (Revenue): Credit increases → balance should go UP by $1500
    assert_eq!(updated_revenue.balance, initial_revenue_balance + dec!(1500.00));
}

#[tokio::test]
async fn test_e2e_multiple_transactions_balance() {
    let mut nexus = NexusLedger::new();
    nexus.initialize().await.unwrap();

    let accounts = nexus.ledger.list_accounts().await.unwrap();
    let cash = accounts.iter().find(|a| a.number == "1000").unwrap();
    let expense = accounts.iter().find(|a| a.number == "5020").unwrap(); // Rent

    // Transaction 1: Record rent expense ($2,000)
    let txn1 = Transaction {
        id: Uuid::new_v4(),
        number: "E2E-001".into(),
        description: "Monthly rent".into(),
        date: Utc::now(),
        transaction_type: TransactionType::JournalEntry,
        status: TransactionStatus::Pending,
        entries: vec![
            TransactionEntry {
                id: Uuid::new_v4(),
                account_id: expense.id,
                entry_type: EntryType::Debit,
                amount: dec!(2000.00),
                description: "Office rent".into(),
                reference: None,
            },
            TransactionEntry {
                id: Uuid::new_v4(),
                account_id: cash.id,
                entry_type: EntryType::Credit,
                amount: dec!(2000.00),
                description: "Cash paid for rent".into(),
                reference: None,
            },
        ],
        journal_entry_id: None,
        document_ids: vec![],
        metadata: serde_json::json!({}),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    // Transaction 2: Record service income ($5,000)
    let txn2 = Transaction {
        id: Uuid::new_v4(),
        number: "E2E-002".into(),
        description: "Client payment".into(),
        date: Utc::now(),
        transaction_type: TransactionType::JournalEntry,
        status: TransactionStatus::Pending,
        entries: vec![
            TransactionEntry {
                id: Uuid::new_v4(),
                account_id: cash.id,
                entry_type: EntryType::Debit,
                amount: dec!(5000.00),
                description: "Cash received".into(),
                reference: None,
            },
            TransactionEntry {
                id: Uuid::new_v4(),
                account_id: accounts.iter().find(|a| a.number == "4000").unwrap().id,
                entry_type: EntryType::Credit,
                amount: dec!(5000.00),
                description: "Service revenue".into(),
                reference: None,
            },
        ],
        journal_entry_id: None,
        document_ids: vec![],
        metadata: serde_json::json!({}),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    // Record both transactions
    nexus.ledger.record_transaction(txn1).await.unwrap();
    nexus.ledger.record_transaction(txn2).await.unwrap();

    // Verify: 2 transactions in ledger
    let txns = nexus.ledger.list_transactions().await.unwrap();
    assert_eq!(txns.len(), 2, "Expected 2 transactions in ledger");

    // Verify trial balance is balanced (total debits = total credits)
    let trial_balance = nexus.ledger.get_trial_balance().await.unwrap();
    let total_balance: rust_decimal::Decimal = trial_balance.values().sum();
    // Trial balance of all accounts should net to zero in double-entry
    // (each transaction is balanced, so sum of all balances should always be 0
    //  since every debit has a corresponding credit)
    // Actually, the trial balance total is not necessarily 0 — it's the sum of
    // all individual account balances. Since Assets + Expenses = Liabilities +
    // Equity + Revenue, the total of debits-minus-credits across all accounts
    // should be 0.
    assert_eq!(total_balance, dec!(0), "Trial balance should net to zero");

    // Verify income statement shows $5,000 revenue, $2,000 expense, $3,000 net
    let income = nexus.ledger.get_income_statement(
        Utc::now() - chrono::Duration::days(365),
        Utc::now(),
    ).await.unwrap();

    assert_eq!(income.revenue, dec!(5000.00));
    assert_eq!(income.expenses, dec!(2000.00));
    assert_eq!(income.net_income, dec!(3000.00));
}

#[tokio::test]
async fn test_e2e_reject_unbalanced_transaction() {
    let mut nexus = NexusLedger::new();
    nexus.initialize().await.unwrap();

    let accounts = nexus.ledger.list_accounts().await.unwrap();
    let cash = accounts.iter().find(|a| a.number == "1000").unwrap();
    let revenue = accounts.iter().find(|a| a.number == "4000").unwrap();

    // Unbalanced: debit 1000, credit 500
    let unbalanced = Transaction {
        id: Uuid::new_v4(),
        number: "UNBAL-001".into(),
        description: "Unbalanced test".into(),
        date: Utc::now(),
        transaction_type: TransactionType::JournalEntry,
        status: TransactionStatus::Pending,
        entries: vec![
            TransactionEntry {
                id: Uuid::new_v4(),
                account_id: cash.id,
                entry_type: EntryType::Debit,
                amount: dec!(1000.00),
                description: "Cash".into(),
                reference: None,
            },
            TransactionEntry {
                id: Uuid::new_v4(),
                account_id: revenue.id,
                entry_type: EntryType::Credit,
                amount: dec!(500.00),
                description: "Revenue".into(),
                reference: None,
            },
        ],
        journal_entry_id: None,
        document_ids: vec![],
        metadata: serde_json::json!({}),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let result = nexus.ledger.record_transaction(unbalanced).await;
    assert!(result.is_err(), "Unbalanced transaction should be rejected");
}
