//! Performance Benchmarks for NexusLedger
//!
//! Criterion benchmarks measuring throughput and latency for core ledger
//! operations at scale.
//!
//! ## Benchmarks
//!
//! | Benchmark                     | Description                              | Target  |
//! |-------------------------------|------------------------------------------|---------|
//! | `create_10k_transactions`     | Create 10K balanced transactions         | < 2s    |
//! | `list_10k_transactions`       | List 10K transactions                    | < 500ms |
//! | `generate_balance_sheet_10k`  | Generate balance sheet (10K txns)        | < 2s    |
//! | `generate_trial_balance_10k`  | Generate trial balance (10K txns)        | < 1s    |
//! | `csv_import_10k_rows`         | Import 10K CSV rows (5K txn pairs)       | < 5s    |
//! | `sync_push_1k_dirty`          | Record 1K transactions (sync throughput) | < 10s   |
//!
//! ## Reproducibility
//!
//! All benchmark data is deterministic: every transaction uses the same
//! amount (100), the same accounts (Cash 1000 / Sales Revenue 4000), and
//! sequential descriptions based on the loop index. No RNG is involved,
//! ensuring reproducible results across runs.
//!
//! Run with: `cargo bench -p nexus-core`

use chrono::Utc;
use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use nexus_core::accounting::ledger::Ledger;
use nexus_core::database::financial::{EntryType, Transaction, TransactionEntry};
use nexus_core::utils::import::import_csv;
use rust_decimal_macros::dec;
use std::time::Duration;
use uuid::Uuid;

// ── Constants ──────────────────────────────────────────────────────────────

/// Number of transactions for the 10K benchmarks.
const TRANSACTION_COUNT_10K: usize = 10_000;

/// Number of transactions for the 1K sync benchmark.
const TRANSACTION_COUNT_1K: usize = 1_000;

/// Number of CSV row pairs (each pair = 1 balanced transaction).
const CSV_PAIR_COUNT: usize = 5_000;

// ── Helper Functions ───────────────────────────────────────────────────────

/// Create a new `Ledger` and initialize it with the default 20-account chart.
fn create_initialized_ledger(rt: &tokio::runtime::Runtime) -> Ledger {
    let mut ledger = Ledger::new();
    rt.block_on(async {
        ledger.initialize().await.expect("ledger init failed");
    });
    ledger
}

/// Look up account IDs for Cash (1000) and Sales Revenue (4000).
fn get_account_ids(ledger: &Ledger, rt: &tokio::runtime::Runtime) -> (Uuid, Uuid) {
    rt.block_on(async {
        let cash = ledger
            .get_account_by_number("1000")
            .await
            .expect("lookup failed")
            .expect("Cash account 1000 not found");
        let revenue = ledger
            .get_account_by_number("4000")
            .await
            .expect("lookup failed")
            .expect("Revenue account 4000 not found");
        (cash.id, revenue.id)
    })
}

/// Populate a ledger with `count` balanced transactions (Dr Cash 100 / Cr Revenue 100).
fn populate_transactions(
    ledger: &Ledger,
    rt: &tokio::runtime::Runtime,
    count: usize,
    cash_id: Uuid,
    revenue_id: Uuid,
) {
    rt.block_on(async {
        for i in 0..count {
            let txn = make_transaction(cash_id, revenue_id, format!("Setup transaction {i}"));
            ledger
                .record_transaction(txn)
                .await
                .expect("record_transaction failed");
        }
    });
}

/// Build a balanced transaction: Dr Cash 100, Cr Revenue 100.
fn make_transaction(cash_id: Uuid, revenue_id: Uuid, description: String) -> Transaction {
    let entries = vec![
        TransactionEntry::new(cash_id, EntryType::Debit, dec!(100), "Dr Cash"),
        TransactionEntry::new(revenue_id, EntryType::Credit, dec!(100), "Cr Revenue"),
    ];
    Transaction::new(description, Utc::now(), entries)
}

/// Generate CSV content with `pairs` balanced transaction pairs (2 data rows per pair).
///
/// Header: `date,description,amount,account,entry_type`
fn generate_csv_content(pairs: usize) -> String {
    let mut csv = String::from("date,description,amount,account,entry_type\n");
    for i in 0..pairs {
        let desc = format!("CSV Transaction {i}");
        csv.push_str(&format!("2026-07-01,{desc},100.00,1000,debit\n"));
        csv.push_str(&format!("2026-07-01,{desc},100.00,4000,credit\n"));
    }
    csv
}

// ── Benchmarks ─────────────────────────────────────────────────────────────

/// Benchmark 1: Create 10,000 balanced transactions via `record_transaction()`.
///
/// Each transaction debits Cash (1000) and credits Sales Revenue (4000) for 100.
/// Fresh ledger per iteration via `iter_batched` to avoid accumulation effects.
/// Target: < 2s
fn bench_create_10k_transactions(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");

    c.bench_function("create_10k_transactions", |b| {
        b.iter_batched(
            || {
                // Setup: fresh initialized ledger + account IDs
                let ledger = create_initialized_ledger(&rt);
                let (cash_id, revenue_id) = get_account_ids(&ledger, &rt);
                (ledger, cash_id, revenue_id)
            },
            |(ledger, cash_id, revenue_id)| {
                // Routine: create 10K transactions
                rt.block_on(async move {
                    for i in 0..TRANSACTION_COUNT_10K {
                        let txn = make_transaction(
                            cash_id,
                            revenue_id,
                            format!("Benchmark transaction {i}"),
                        );
                        black_box(ledger.record_transaction(txn).await.unwrap());
                    }
                });
            },
            BatchSize::PerIteration,
        );
    });
}

/// Benchmark 2: List 10,000 transactions via `list_transactions()`.
///
/// Ledger is pre-populated once (not measured); each iteration calls
/// `list_transactions()` which clones all 10K transactions.
/// Target: < 500ms
fn bench_list_10k_transactions(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");

    // Pre-populate ledger with 10K transactions (not measured)
    let ledger = create_initialized_ledger(&rt);
    let (cash_id, revenue_id) = get_account_ids(&ledger, &rt);
    populate_transactions(&ledger, &rt, TRANSACTION_COUNT_10K, cash_id, revenue_id);

    c.bench_function("list_10k_transactions", |b| {
        b.iter(|| {
            black_box(rt.block_on(ledger.list_transactions()).unwrap());
        });
    });
}

/// Benchmark 3: Generate balance sheet with 10K transactions.
///
/// Ledger is pre-populated once (not measured); each iteration calls
/// `get_balance_sheet()` which iterates all accounts.
/// Target: < 2s
fn bench_generate_balance_sheet_10k(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");

    // Pre-populate ledger with 10K transactions (not measured)
    let ledger = create_initialized_ledger(&rt);
    let (cash_id, revenue_id) = get_account_ids(&ledger, &rt);
    populate_transactions(&ledger, &rt, TRANSACTION_COUNT_10K, cash_id, revenue_id);

    c.bench_function("generate_balance_sheet_10k", |b| {
        b.iter(|| {
            black_box(rt.block_on(ledger.get_balance_sheet()).unwrap());
        });
    });
}

/// Benchmark 4: Generate trial balance with 10K transactions.
///
/// Ledger is pre-populated once (not measured); each iteration calls
/// `get_trial_balance()` which collects all account balances into a HashMap.
/// Target: < 1s
fn bench_generate_trial_balance_10k(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");

    // Pre-populate ledger with 10K transactions (not measured)
    let ledger = create_initialized_ledger(&rt);
    let (cash_id, revenue_id) = get_account_ids(&ledger, &rt);
    populate_transactions(&ledger, &rt, TRANSACTION_COUNT_10K, cash_id, revenue_id);

    c.bench_function("generate_trial_balance_10k", |b| {
        b.iter(|| {
            black_box(rt.block_on(ledger.get_trial_balance()).unwrap());
        });
    });
}

/// Benchmark 5: Import 10K CSV rows (5,000 balanced transaction pairs).
///
/// Generates 10K CSV data rows once; each iteration imports into a fresh
/// initialized ledger via `import_csv()`.
/// Target: < 5s
fn bench_csv_import_10k_rows(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let csv_content = generate_csv_content(CSV_PAIR_COUNT);

    c.bench_function("csv_import_10k_rows", |b| {
        b.iter_batched(
            || {
                // Setup: fresh initialized ledger + cloned CSV (avoids borrow issues)
                let ledger = create_initialized_ledger(&rt);
                let csv = csv_content.clone();
                (ledger, csv)
            },
            |(ledger, csv)| {
                // Routine: import 10K CSV rows
                black_box(rt.block_on(async move {
                    import_csv(&ledger, &csv).await.unwrap()
                }));
            },
            BatchSize::PerIteration,
        );
    });
}

/// Benchmark 6: Simulate sync push of 1K dirty transactions.
///
/// Measures `record_transaction()` throughput for 1K transactions on a
/// fresh ledger, simulating a sync push of dirty (uncommitted) records.
/// Target: < 10s
fn bench_sync_push_1k_dirty(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");

    c.bench_function("sync_push_1k_dirty", |b| {
        b.iter_batched(
            || {
                // Setup: fresh initialized ledger + account IDs
                let ledger = create_initialized_ledger(&rt);
                let (cash_id, revenue_id) = get_account_ids(&ledger, &rt);
                (ledger, cash_id, revenue_id)
            },
            |(ledger, cash_id, revenue_id)| {
                // Routine: create 1K transactions (simulating sync push)
                rt.block_on(async move {
                    for i in 0..TRANSACTION_COUNT_1K {
                        let txn = make_transaction(
                            cash_id,
                            revenue_id,
                            format!("Sync transaction {i}"),
                        );
                        black_box(ledger.record_transaction(txn).await.unwrap());
                    }
                });
            },
            BatchSize::PerIteration,
        );
    });
}

// ── Criterion Entry Point ──────────────────────────────────────────────────

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(5));
    targets =
        bench_create_10k_transactions,
        bench_list_10k_transactions,
        bench_generate_balance_sheet_10k,
        bench_generate_trial_balance_10k,
        bench_csv_import_10k_rows,
        bench_sync_push_1k_dirty;
}

criterion_main!(benches);
