//! Performance benchmarks for NexusLedger ledger operations.
//!
//! These benchmarks measure the performance of core ledger operations:
//! - Creating transactions (single and bulk 10K)
//! - Listing transactions
//! - Generating balance sheets
//! - Concurrent write performance under lock contention
//!
//! Run with: `cargo bench --bench performance`

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::time::Duration;

use nexus_core::accounting::ledger::Ledger;
use nexus_core::database::financial::{EntryType, Transaction, TransactionEntry};

use rust_decimal_macros::dec;
use uuid::Uuid;

// ── Helpers ──────────────────────────────────────────────────────────────

/// Set up a fresh in-memory ledger with default accounts.
///
/// Returns the ledger plus the UUIDs of the Cash (1000) and Sales Revenue
/// (4000) accounts, which are used as the debit/credit pair in benchmark
/// transactions.
///
/// The ledger operates purely in-memory (no SurrealDB persistence, `db`
/// field is `None`) so that benchmarks measure ledger logic, not database
/// I/O. This aligns with the < 2-second performance targets.
async fn setup_ledger() -> (Ledger, Uuid, Uuid) {
    let mut ledger = Ledger::new();
    ledger.initialize().await.unwrap();

    let cash_id = ledger
        .get_account_by_number("1000")
        .await
        .unwrap()
        .unwrap()
        .id;
    let revenue_id = ledger
        .get_account_by_number("4000")
        .await
        .unwrap()
        .unwrap()
        .id;

    (ledger, cash_id, revenue_id)
}

/// Create a balanced double-entry transaction with two entries (debit + credit).
///
/// The `record_transaction` method overrides `number` with its own sequential
/// counter, so the number set here is purely for identification.
fn make_transaction(n: usize, debit_account_id: Uuid, credit_account_id: Uuid) -> Transaction {
    Transaction {
        number: format!("TRX-{:06}", n),
        description: format!("Benchmark transaction {}", n),
        entries: vec![
            TransactionEntry {
                account_id: debit_account_id,
                entry_type: EntryType::Debit,
                amount: dec!(100.00),
                description: "Debit entry".to_string(),
                ..Default::default()
            },
            TransactionEntry {
                account_id: credit_account_id,
                entry_type: EntryType::Credit,
                amount: dec!(100.00),
                description: "Credit entry".to_string(),
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}

// ── Benchmarks ───────────────────────────────────────────────────────────

/// Benchmark: Create 10,000 transactions in the ledger.
///
/// Each transaction has 2 entries (debit/credit). A fresh ledger is set up
/// for each sample to avoid unbounded growth across iterations.
///
/// Target: < 2 seconds.
fn bench_create_10k_transactions(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("create_10k_transactions", |b| {
        b.iter(|| {
            rt.block_on(async {
                let (ledger, cash_id, revenue_id) = setup_ledger().await;
                for i in 0..10_000 {
                    let txn = make_transaction(i, cash_id, revenue_id);
                    ledger.record_transaction(txn).await.unwrap();
                }
                black_box(());
            });
        });
    });
}

/// Benchmark: List all transactions after creating 10K.
///
/// The 10K transactions are pre-populated once (outside the measurement
/// loop) so that only the listing operation is measured.
///
/// Target: < 2 seconds.
fn bench_list_10k_transactions(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    // Pre-populate the ledger with 10K transactions (not measured)
    let ledger = rt.block_on(async {
        let (ledger, cash_id, revenue_id) = setup_ledger().await;
        for i in 0..10_000 {
            let txn = make_transaction(i, cash_id, revenue_id);
            ledger.record_transaction(txn).await.unwrap();
        }
        ledger
    });

    c.bench_function("list_10k_transactions", |b| {
        b.iter(|| {
            rt.block_on(async {
                let txns = ledger.list_transactions().await.unwrap();
                black_box(txns);
            });
        });
    });
}

/// Benchmark: Generate a balance sheet after creating 10K transactions.
///
/// The 10K transactions are pre-populated once (outside the measurement
/// loop) so that only the balance sheet generation is measured.
///
/// Target: < 2 seconds.
fn bench_generate_balance_sheet_10k(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    // Pre-populate the ledger with 10K transactions (not measured)
    let ledger = rt.block_on(async {
        let (ledger, cash_id, revenue_id) = setup_ledger().await;
        for i in 0..10_000 {
            let txn = make_transaction(i, cash_id, revenue_id);
            ledger.record_transaction(txn).await.unwrap();
        }
        ledger
    });

    c.bench_function("generate_balance_sheet_10k", |b| {
        b.iter(|| {
            rt.block_on(async {
                let bs = ledger.get_balance_sheet().await.unwrap();
                black_box(bs);
            });
        });
    });
}

/// Benchmark: Create a single transaction (comparison baseline).
///
/// A fresh ledger is set up for each sample. The setup cost (account
/// initialization) is included, making this directly comparable to the
/// 10K benchmark where the same setup is amortized across 10K transactions.
fn bench_create_single_transaction(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("create_single_transaction", |b| {
        b.iter(|| {
            rt.block_on(async {
                let (ledger, cash_id, revenue_id) = setup_ledger().await;
                let txn = make_transaction(0, cash_id, revenue_id);
                let recorded = ledger.record_transaction(txn).await.unwrap();
                black_box(recorded);
            });
        });
    });
}

/// Benchmark: 10 concurrent writers, each creating 100 transactions.
///
/// Measures lock contention under concurrent writes. A fresh ledger is
/// set up for each sample. The `Ledger` is `Clone` (all fields are
/// `Arc`-backed), so each writer gets a cheap handle to the same
/// underlying state.
///
/// Contention points include:
/// - `current_transaction_number` Mutex (sequential counter)
/// - `accounts` RwLock write (balance updates on shared accounts)
/// - `transactions` RwLock write (BTreeMap insert)
/// - `journal_entries` RwLock write (journal entry insert)
fn bench_concurrent_10_writers(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("concurrent_10_writers", |b| {
        b.iter(|| {
            rt.block_on(async {
                let (ledger, cash_id, revenue_id) = setup_ledger().await;

                let mut handles = Vec::new();
                for _ in 0..10 {
                    let ledger_clone = ledger.clone();
                    let handle = tokio::spawn(async move {
                        for i in 0..100 {
                            let txn = make_transaction(i, cash_id, revenue_id);
                            ledger_clone.record_transaction(txn).await.unwrap();
                        }
                    });
                    handles.push(handle);
                }

                for handle in handles {
                    handle.await.unwrap();
                }

                black_box(());
            });
        });
    });
}

// ── Criterion setup ──────────────────────────────────────────────────────

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .measurement_time(Duration::from_secs(10));
    targets = bench_create_10k_transactions,
              bench_list_10k_transactions,
              bench_generate_balance_sheet_10k,
              bench_create_single_transaction,
              bench_concurrent_10_writers
}

criterion_main!(benches);
