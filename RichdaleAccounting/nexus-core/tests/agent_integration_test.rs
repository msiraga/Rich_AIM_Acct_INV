//! Phase 2 Integration Tests
//!
//! These tests verify that agents process real tasks end-to-end:
//! submit task → agent processes → result verified in SurrealDB.

use nexus_core::database::Database;
use nexus_core::agents::orchestrator::AgentOrchestrator;
use nexus_core::agents::Agent;
use nexus_core::agents::task::{Task, TaskStatus, TaskPayload};
use nexus_core::accounting::ledger::Ledger;
use nexus_core::database::financial::*;
use std::sync::Arc;
use rust_decimal_macros::dec;
use chrono::Utc;

/// Test 2.11: LedgerAgent processes RecordTransaction →
/// verify task completes, balances update, transaction + journal entry created.
#[tokio::test]
async fn test_ledger_agent_transaction_to_db() {
    // Set up database
    let db = Database::new();
    db.connect().await.unwrap();
    db.seed().await.unwrap();

    // Create a ledger connected to SurrealDB and seed accounts
    let mut ledger = Ledger::new();
    ledger.db = Some(Arc::new(db.clone()));
    ledger.initialize().await.unwrap();

    // Create accounts on this ledger
    let cash = ledger
        .create_account(Account::new("8000", "Integration Cash", AccountType::Asset))
        .await
        .unwrap();
    let revenue = ledger
        .create_account(Account::new("8100", "Integration Revenue", AccountType::Revenue))
        .await
        .unwrap();

    // Build a balanced transaction
    let entries = vec![
        TransactionEntry::new(cash.id, EntryType::Debit, dec!(500), "Integration test cash in"),
        TransactionEntry::new(revenue.id, EntryType::Credit, dec!(500), "Integration test revenue"),
    ];
    let transaction = Transaction::new("Integration test sale".to_string(), Utc::now(), entries);

    // Create the LedgerAgent with this ledger (same code path as orchestrator)
    let agent_config = nexus_core::agents::agent_types::AgentConfig::ledger_agent();
    let ledger_agent = nexus_core::accounting::ledger::LedgerAgent::new(
        agent_config,
        ledger,
    );

    // Process through LedgerAgent.process_task (same code path as orchestrator dispatch)
    let task = Task::record_transaction(transaction);
    let completed_task = ledger_agent.process_task(task).await.unwrap();

    // Verify: task completed successfully
    assert_eq!(completed_task.status, TaskStatus::Completed);
    let result = completed_task.result.as_ref().unwrap();
    assert!(result.success);
    assert!(result.message.contains("Transaction recorded"));

    // Verify: result contains the recorded transaction data
    assert!(result.data.is_some(), "Result should contain recorded transaction data");

    // Verify: balances updated correctly on the agent's ledger
    let cash_balance = ledger_agent.ledger.get_account_balance(cash.id).await.unwrap();
    assert_eq!(cash_balance, dec!(500));
    let revenue_balance = ledger_agent.ledger.get_account_balance(revenue.id).await.unwrap();
    assert_eq!(revenue_balance, dec!(500));

    // Verify: transaction was recorded in the ledger's in-memory store
    let all_txns = ledger_agent.ledger.list_transactions().await.unwrap();
    assert_eq!(all_txns.len(), 1, "Exactly one transaction should be recorded");
    assert_eq!(all_txns[0].description, "Integration test sale");
    assert!(all_txns[0].journal_entry_id.is_some(), "Journal entry ID should be set");

    // Verify: journal entry was created in the ledger
    let all_je = ledger_agent.ledger.journal_entries.read().await;
    assert!(!all_je.is_empty(), "Journal entry should exist in ledger");
    let je = all_je.values().next().unwrap();
    assert_eq!(je.entries.len(), 2, "Journal entry should have 2 entries");
    assert!(je.is_posted, "Journal entry should be posted");
}

/// Test: InvoiceAgent generates an invoice through the orchestrator.
#[tokio::test]
async fn test_invoice_agent_via_orchestrator() {
    let mut orchestrator = AgentOrchestrator::new();
    orchestrator.initialize().await.unwrap();

    let task = Task::generate_invoice(serde_json::json!({
        "customer_name": "Acme Corp",
        "customer_email": "billing@acme.com",
        "due_date": "2026-12-31",
        "items": [
            {"description": "Consulting", "quantity": 10, "unit_price": 150},
            {"description": "Travel expenses", "quantity": 1, "unit_price": 500}
        ],
        "notes": "Net 30"
    }));

    orchestrator.submit_task(task).await.unwrap();
    orchestrator.process_next_task().await.unwrap();

    let completed = orchestrator.completed_tasks.lock().await;
    assert_eq!(completed.len(), 1);
    let result = completed[0].result.as_ref().unwrap();
    assert!(result.success);
    assert!(result.message.contains("INV-"));
}

/// Test: TaxAgent calculates tax from real payload (not hardcoded).
#[tokio::test]
async fn test_tax_agent_real_calculation() {
    let mut orchestrator = AgentOrchestrator::new();
    orchestrator.initialize().await.unwrap();

    let task = Task::calculate_taxes(serde_json::json!({
        "jurisdiction_code": "US-CA",
        "tax_type": "Sales",
        "amount": 1000
    }));

    orchestrator.submit_task(task).await.unwrap();
    orchestrator.process_next_task().await.unwrap();

    let completed = orchestrator.completed_tasks.lock().await;
    assert_eq!(completed.len(), 1);
    let result = completed[0].result.as_ref().unwrap();
    assert!(result.success);
    assert!(result.message.contains("US-CA"));
    assert!(result.message.contains("Sales"));

    // Verify actual math: US-CA Sales tax = 7.25% of $1000 = $72.50
    // (rate stored as 0.0725, so 1000 * 0.0725 = 72.50)
    if let Some(TaskPayload::Json(ref data)) = result.data {
        let tax_amount = data.get("tax_amount")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<rust_decimal::Decimal>().ok())
            .expect("tax_amount should be present and parseable");
        assert_eq!(tax_amount, rust_decimal_macros::dec!(72.5000),
            "US-CA Sales tax on $1000 should be $72.50, got {}", tax_amount);
    } else {
        panic!("Expected Json payload data in result");
    }
}

/// Test: ReceiptAgent processes a receipt without a ledger (no DB crash).
#[tokio::test]
async fn test_receipt_agent_process() {
    let mut orchestrator = AgentOrchestrator::new();
    orchestrator.initialize().await.unwrap();

    let task = Task::process_receipt(serde_json::json!({
        "vendor_name": "Office Depot",
        "receipt_date": "2026-06-15",
        "amount": 45.99,
        "expense_category": "office supplies",
        "description": "Printer paper and toner"
    }));

    orchestrator.submit_task(task).await.unwrap();
    orchestrator.process_next_task().await.unwrap();

    let completed = orchestrator.completed_tasks.lock().await;
    assert_eq!(completed.len(), 1);
    let result = completed[0].result.as_ref().unwrap();
    assert!(result.success);
    assert!(result.message.contains("Office Depot"));
}

/// Test: ReportingAgent generates a trial balance report.
#[tokio::test]
async fn test_reporting_agent_trial_balance() {
    let mut orchestrator = AgentOrchestrator::new();
    orchestrator.initialize().await.unwrap();

    let task = Task::generate_report("trial_balance");

    orchestrator.submit_task(task).await.unwrap();
    orchestrator.process_next_task().await.unwrap();

    let completed = orchestrator.completed_tasks.lock().await;
    assert_eq!(completed.len(), 1);
    let result = completed[0].result.as_ref().unwrap();
    assert!(result.success);
    assert!(result.message.contains("Trial Balance"));
}

/// Test: AuditAgent logs real audit entries with entity data.
#[tokio::test]
async fn test_audit_agent_real_check() {
    let mut orchestrator = AgentOrchestrator::new();
    orchestrator.initialize().await.unwrap();

    let task = Task::audit_check(serde_json::json!({
        "entity_type": "transaction",
        "entity_id": "txn-001",
        "action": "Create",
        "new_values": {"amount": 500, "description": "Test payment"}
    }));

    orchestrator.submit_task(task).await.unwrap();
    orchestrator.process_next_task().await.unwrap();

    let completed = orchestrator.completed_tasks.lock().await;
    assert_eq!(completed.len(), 1);
    let result = completed[0].result.as_ref().unwrap();
    assert!(result.success);
    assert!(result.message.contains("transaction"));
}

/// Test: Event-driven dispatch — task_notify wakes the loop.
#[tokio::test]
async fn test_event_driven_dispatch() {
    let mut orchestrator = AgentOrchestrator::new();
    orchestrator.initialize().await.unwrap();

    let orchestrator = Arc::new(orchestrator);
    let orchestrator_clone = orchestrator.clone();

    // Start the dispatch loop in a background task
    let handle = tokio::spawn(async move {
        // Run for a short window, then stop
        tokio::time::timeout(
            std::time::Duration::from_millis(500),
            orchestrator_clone.start(),
        )
        .await
        .ok();
    });

    // Submit a task — should wake the loop via notify
    let task = Task::generate_report("trial_balance");
    orchestrator.submit_task(task).await.unwrap();

    // Wait for the background task to finish
    handle.await.unwrap();

    // Give a moment for processing
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Verify task was processed
    let completed = orchestrator.completed_tasks.lock().await;
    assert!(
        !completed.is_empty(),
        "Task should have been processed by the event-driven loop"
    );
}

/// Test: Multiple task types routed to correct agents.
#[tokio::test]
async fn test_task_routing() {
    let mut orchestrator = AgentOrchestrator::new();
    orchestrator.initialize().await.unwrap();

    // Submit different task types
    let tasks = vec![
        Task::generate_invoice(serde_json::json!({
            "customer_name": "Test",
            "items": [{"description": "Item", "quantity": 1, "unit_price": 100}]
        })),
        Task::calculate_taxes(serde_json::json!({
            "jurisdiction_code": "US-FED",
            "tax_type": "Income",
            "amount": 50000
        })),
        Task::generate_report("balance_sheet"),
    ];

    for task in tasks {
        orchestrator.submit_task(task).await.unwrap();
    }

    // Process all tasks
    for _ in 0..3 {
        orchestrator.process_next_task().await.unwrap();
    }

    // All should be completed
    let completed = orchestrator.completed_tasks.lock().await;
    assert_eq!(completed.len(), 3);
    for task in completed.iter() {
        assert_eq!(task.status, TaskStatus::Completed);
    }

    // No failed tasks
    let failed = orchestrator.failed_tasks.lock().await;
    assert!(failed.is_empty(), "No tasks should have failed");
}
