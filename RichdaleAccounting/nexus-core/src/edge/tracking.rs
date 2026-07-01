//! Edge Change Tracking
//!
//! Tracks local changes for synchronization with the remote database.
//! Every write through `LocalStore` sets `_dirty = 1` and records a change
//! in the `changes` table.  This module provides the higher-level query API
//! that the sync engine uses to determine:
//!
//! - Which records are dirty (need to be pushed to the server)
//! - What operations occurred (for audit trail and incremental sync)
//! - Batch operations for marking records as synced
//! - Statistics and sync-state management
//!
//! # Schema Evolution
//!
//! The base `changes` table (created by `local_db.rs` migration v1) has
//! columns: `id, entity_type, entity_id, operation, timestamp`.  On first
//! use, `ChangeTracker::new()` adds a `synced INTEGER DEFAULT 0` column
//! via `ALTER TABLE` so individual change entries can be flagged as
//! processed by the sync engine.  This is a common SQLite schema-evolution pattern
//! and is idempotent — calling `new()` on a database that already has the
//! column is a no-op.
//!
//! # Entity-Type ↔ Table Mapping
//!
//! The `changes` table stores singular entity types (`account`,
//! `transaction`, …) while the data tables use plural names (`accounts`,
//! `transactions`, …).  The mapping is:
//!
//! | `entity_type`    | Table               |
//! |------------------|----------------------|
//! | `account`        | `accounts`           |
//! | `transaction`    | `transactions`       |
//! | `journal_entry`  | `journal_entries`    |
//! | `transaction_entry` | `transaction_entries` |
//! | `invoice`        | `invoices`           |
//! | `bill`           | `bills`              |
//! | `asset`          | `assets`             |

use std::sync::Arc;
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use thiserror::Error;
use tracing::{info, debug, warn, error};

use super::local_db::{LocalDb, LocalDbError, Row};

// ═══════════════════════════════════════════════════════════
// Error Type
// ═══════════════════════════════════════════════════════════

/// Errors that can occur during change-tracking operations.
#[derive(Error, Debug)]
pub enum TrackingError {
    /// Underlying SQLite / `LocalDb` error.
    #[error("Local DB error: {0}")]
    Db(#[from] LocalDbError),

    /// The requested entity or change was not found.
    #[error("Not found: {0}")]
    NotFound(String),

    /// An invalid entity type or operation was supplied.
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
}

// ═══════════════════════════════════════════════════════════
// Data Structures
// ═══════════════════════════════════════════════════════════

/// A dirty record that needs to be synced to the remote database.
///
/// Each `DirtyRecord` corresponds to one row in a data table where
/// `_dirty = 1`.  The `operation` field reflects the most recent
/// operation recorded for this entity in the `changes` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirtyRecord {
    /// Singular entity type (`"account"`, `"transaction"`, …).
    pub entity_type: String,
    /// UUID of the entity (as stored in the data table `id` column).
    pub entity_id: String,
    /// Most recent operation: `"insert"`, `"update"`, or `"delete"`.
    pub operation: String,
    /// When the record was last modified (from `_modified_at`).
    pub modified_at: DateTime<Utc>,
}

/// A single entry in the `changes` audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeEntry {
    /// Auto-incremented primary key from the `changes` table.
    pub id: i64,
    /// Singular entity type.
    pub entity_type: String,
    /// UUID of the entity.
    pub entity_id: String,
    /// Operation performed: `"insert"`, `"update"`, or `"delete"`.
    pub operation: String,
    /// When the change was recorded (RFC 3339).
    pub timestamp: DateTime<Utc>,
    /// Whether the sync engine has processed this entry.
    pub synced: bool,
}

/// Summary of the current synchronization state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    /// Timestamp of the last successful full sync, if any.
    pub last_sync: Option<DateTime<Utc>>,
    /// Total number of dirty (unsynced) records across all data tables.
    pub pending_changes: usize,
    /// Dirty-record count broken down by entity type.
    pub pending_by_type: std::collections::HashMap<String, usize>,
}

// ═══════════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════════

/// Mapping from singular entity type (used in the `changes` table) to
/// the plural data-table name.
const ENTITY_TABLES: &[(&str, &str)] = &[
    ("account", "accounts"),
    ("transaction", "transactions"),
    ("journal_entry", "journal_entries"),
    ("transaction_entry", "transaction_entries"),
    ("invoice", "invoices"),
    ("bill", "bills"),
    ("asset", "assets"),
];

// ═══════════════════════════════════════════════════════════
// ChangeTracker
// ═══════════════════════════════════════════════════════════

/// High-level API for querying and managing local change-tracking state.
///
/// `ChangeTracker` wraps an `Arc<LocalDb>` and provides methods that the
/// sync engine calls to determine what needs to be pushed to the remote
/// database, mark records as synced after a successful push, and manage
/// the `sync_state` metadata.
///
/// # Construction
///
/// ```no_run
/// use std::sync::Arc;
/// use nexus_core::edge::local_db::LocalDb;
/// use nexus_core::edge::tracking::ChangeTracker;
///
/// let db = Arc::new(LocalDb::open_in_memory().unwrap());
/// let tracker = ChangeTracker::new(db);
/// ```
///
/// On construction, `ChangeTracker` ensures the `synced` column exists on
/// the `changes` table (adding it via `ALTER TABLE` if necessary).
#[derive(Debug, Clone)]
pub struct ChangeTracker {
    db: Arc<LocalDb>,
}

impl ChangeTracker {
    /// Create a new `ChangeTracker` and ensure the `synced` column
    /// exists on the `changes` table.
    ///
    /// This is idempotent — calling it on a database that already has the
    /// column is a no-op.
    pub fn new(db: Arc<LocalDb>) -> Self {
        let tracker = Self { db };
        tracker.ensure_synced_column();
        tracker
    }

    // ── Dirty Record Queries ─────────────────────────────

    /// Get all dirty records across every data table.
    ///
    /// Returns one `DirtyRecord` per row where `_dirty = 1`.  The
    /// `operation` field is resolved from the most recent entry in the
    /// `changes` table for that entity; if no change entry exists it
    /// defaults to `"update"`.
    pub fn get_dirty_records(&self) -> Result<Vec<DirtyRecord>, TrackingError> {
        debug!("get_dirty_records: querying all data tables");
        let mut records = Vec::new();

        for (entity_type, table) in ENTITY_TABLES {
            let sql = Self::build_dirty_record_sql(entity_type, table);
            let rows = self.db.query_all(&sql, &[])?;

            for row in &rows {
                records.push(Self::row_to_dirty_record(row, entity_type)?);
            }
        }

        debug!("get_dirty_records: found {} dirty records", records.len());
        Ok(records)
    }

    /// Get dirty records for a specific entity type only.
    ///
    /// Returns `TrackingError::InvalidOperation` if `entity_type` is not
    /// one of the recognised singular types.
    pub fn get_dirty_records_by_type(
        &self,
        entity_type: &str,
    ) -> Result<Vec<DirtyRecord>, TrackingError> {
        let table = entity_type_to_table(entity_type).ok_or_else(|| {
            TrackingError::InvalidOperation(format!("Unknown entity type: '{}'", entity_type))
        })?;

        debug!(
            "get_dirty_records_by_type: querying {} for dirty records",
            table
        );

        let sql = Self::build_dirty_record_sql(entity_type, table);
        let rows = self.db.query_all(&sql, &[])?;

        let records: Result<Vec<_>, _> = rows
            .iter()
            .map(|row| Self::row_to_dirty_record(row, entity_type))
            .collect();

        let records = records?;
        debug!(
            "get_dirty_records_by_type: found {} dirty {} records",
            records.len(),
            entity_type
        );
        Ok(records)
    }

    // ── Mark Synced ──────────────────────────────────────

    /// Mark a single entity as synced.
    ///
    /// Sets `_dirty = 0` on the corresponding data-table row **and**
    /// marks all `changes` entries for that entity as `synced = 1`.
    /// Both updates are wrapped in a transaction so they succeed or
    /// fail atomically.
    pub fn mark_synced(
        &self,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<(), TrackingError> {
        let table = entity_type_to_table(entity_type).ok_or_else(|| {
            TrackingError::InvalidOperation(format!("Unknown entity type: '{}'", entity_type))
        })?;

        info!("mark_synced: syncing {} {}", entity_type, entity_id);

        // Begin transaction for atomicity.
        self.db.execute_batch("BEGIN")?;

        let result = self.mark_synced_inner(table, entity_type, entity_id);

        match result {
            Ok(affected) => {
                self.db.execute_batch("COMMIT")?;
                if affected == 0 {
                    warn!(
                        "mark_synced: no dirty record found for {} {}",
                        entity_type, entity_id
                    );
                } else {
                    debug!(
                        "mark_synced: cleared dirty flag and marked changes as synced for {} {}",
                        entity_type, entity_id
                    );
                }
                Ok(())
            }
            Err(e) => {
                error!(
                    "mark_synced failed for {} {}: {}",
                    entity_type, entity_id, e
                );
                // Best-effort rollback; ignore errors since we're already
                // in an error path.
                let _ = self.db.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    /// Mark **all** records as synced.
    ///
    /// Clears every `_dirty` flag across all seven data tables, truncates
    /// the `changes` table, and resets `sync_state.pending_changes` to 0.
    /// This is the "full sync succeeded" nuclear option — use
    /// [`mark_synced`](Self::mark_synced) for incremental syncs.
    ///
    /// Everything runs inside a single `execute_batch` call so the entire
    /// operation is atomic.
    pub fn mark_all_synced(&self) -> Result<(), TrackingError> {
        info!("mark_all_synced: clearing all dirty flags and truncating changes table");

        let sql = r#"
            BEGIN;
            UPDATE accounts SET _dirty = 0;
            UPDATE transactions SET _dirty = 0;
            UPDATE journal_entries SET _dirty = 0;
            UPDATE transaction_entries SET _dirty = 0;
            UPDATE invoices SET _dirty = 0;
            UPDATE bills SET _dirty = 0;
            UPDATE assets SET _dirty = 0;
            DELETE FROM changes;
            UPDATE sync_state SET pending_changes = 0;
            COMMIT;
        "#;

        self.db.execute_batch(sql)?;
        info!("mark_all_synced: all dirty flags cleared, changes table truncated");
        Ok(())
    }

    // ── Pending Changes (Audit Log) ──────────────────────

    /// Get all entries from the `changes` table, ordered by timestamp
    /// then id (oldest first).
    pub fn get_pending_changes(&self) -> Result<Vec<ChangeEntry>, TrackingError> {
        debug!("get_pending_changes: querying all change entries");

        let rows = self.db.query_all(
            "SELECT id, entity_type, entity_id, operation, timestamp, \
             COALESCE(synced, 0) AS synced \
             FROM changes ORDER BY timestamp ASC, id ASC",
            &[],
        )?;

        let entries: Result<Vec<_>, _> = rows.iter().map(row_to_change_entry).collect();
        let entries = entries?;
        debug!("get_pending_changes: found {} entries", entries.len());
        Ok(entries)
    }

    /// Get the number of entries in the `changes` table without
    /// fetching any rows.
    pub fn get_pending_changes_count(&self) -> Result<usize, TrackingError> {
        let row = self
            .db
            .query_one("SELECT COUNT(*) AS count FROM changes", &[])?;
        let count: i64 = row.get("count")?;
        debug!("get_pending_changes_count: {}", count);
        Ok(count as usize)
    }

    /// Get change entries recorded after the given timestamp.
    ///
    /// Used by the sync engine for incremental sync: after a successful
    /// sync, call `update_sync_state(now)` and on the next cycle use
    /// `get_changes_since(last_sync)` to fetch only the new changes.
    pub fn get_changes_since(
        &self,
        since: DateTime<Utc>,
    ) -> Result<Vec<ChangeEntry>, TrackingError> {
        let since_str = since.to_rfc3339();
        debug!("get_changes_since: querying changes after {}", since_str);

        let rows = self.db.query_all(
            "SELECT id, entity_type, entity_id, operation, timestamp, \
             COALESCE(synced, 0) AS synced \
             FROM changes WHERE timestamp > ?1 ORDER BY timestamp ASC, id ASC",
            rusqlite::params![since_str],
        )?;

        let entries: Result<Vec<_>, _> = rows.iter().map(row_to_change_entry).collect();
        let entries = entries?;
        debug!("get_changes_since: found {} entries after {}", entries.len(), since_str);
        Ok(entries)
    }

    /// Remove all change entries that have been marked as `synced = 1`.
    ///
    /// This is the incremental-cleanup counterpart to
    /// [`mark_synced`](Self::mark_synced).  After marking individual
    /// entities as synced, call this to reclaim space in the `changes`
    /// table.
    pub fn clear_synced_changes(&self) -> Result<(), TrackingError> {
        let affected = self.db.execute("DELETE FROM changes WHERE synced = 1", &[])?;
        debug!("clear_synced_changes: removed {} synced entries", affected);
        Ok(())
    }

    // ── Sync State ───────────────────────────────────────

    /// Get a summary of the current sync state.
    ///
    /// Returns the `last_sync` timestamp from the `sync_state` table,
    /// the total number of dirty records, and a per-entity-type
    /// breakdown.
    pub fn get_sync_state(&self) -> Result<SyncState, TrackingError> {
        debug!("get_sync_state: computing sync state summary");

        // Last sync timestamp from the sync_state singleton row.
        let last_sync = self
            .db
            .get_last_sync()
            .map(|s| parse_dt(&s))
            .transpose()?;

        // Count dirty records per entity type.
        let mut pending_by_type = std::collections::HashMap::new();
        let mut total_pending = 0usize;

        for (entity_type, table) in ENTITY_TABLES {
            let sql = format!("SELECT COUNT(*) AS count FROM {} WHERE _dirty = 1", table);
            let row = self.db.query_one(&sql, &[])?;
            let count: i64 = row.get("count")?;
            let count = count as usize;
            if count > 0 {
                pending_by_type.insert(entity_type.to_string(), count);
                total_pending += count;
            }
        }

        debug!(
            "get_sync_state: last_sync={:?}, pending={}, by_type={:?}",
            last_sync, total_pending, pending_by_type
        );

        Ok(SyncState {
            last_sync,
            pending_changes: total_pending,
            pending_by_type,
        })
    }

    /// Update the `last_sync` timestamp in the `sync_state` table.
    ///
    /// Also refreshes the `pending_changes` counter so the `sync_state`
    /// row stays consistent with the actual number of dirty records.
    pub fn update_sync_state(&self, last_sync: DateTime<Utc>) -> Result<(), TrackingError> {
        let ts = last_sync.to_rfc3339();
        let pending = self.get_dirty_record_count()?;

        self.db.execute(
            "UPDATE sync_state SET last_sync = ?1, pending_changes = ?2 WHERE id = 1",
            rusqlite::params![ts, pending as i64],
        )?;

        info!(
            "update_sync_state: last_sync = {}, pending_changes = {}",
            ts, pending
        );
        Ok(())
    }

    // ── Manual Change Recording ─────────────────────────

    /// Record a change entry manually.
    ///
    /// This is a convenience wrapper around `LocalDb::record_change` for
    /// cases where data was written directly to the database (bypassing
    /// `LocalStore`).  The `operation` must be one of `"insert"`,
    /// `"update"`, or `"delete"`.
    pub fn record_change(
        &self,
        entity_type: &str,
        entity_id: &str,
        operation: &str,
    ) -> Result<(), TrackingError> {
        // Validate the operation early for a clear error message.
        match operation {
            "insert" | "update" | "delete" => {}
            _ => {
                return Err(TrackingError::InvalidOperation(format!(
                    "Invalid operation: '{}'. Must be 'insert', 'update', or 'delete'.",
                    operation
                )));
            }
        }

        self.db
            .record_change(entity_type, entity_id, operation)?;
        debug!(
            "record_change: {} {} {}",
            operation, entity_type, entity_id
        );
        Ok(())
    }

    // ── Private Helpers ──────────────────────────────────

    /// Ensure the `synced` column exists on the `changes` table.
    ///
    /// Queries `PRAGMA table_info(changes)` and, if no column named
    /// `synced` is found, runs `ALTER TABLE changes ADD COLUMN synced
    /// INTEGER DEFAULT 0`.
    ///
    /// Errors are logged but do not prevent construction — the caller
    /// will discover the missing column when queries that reference it
    /// fail.
    fn ensure_synced_column(&self) {
        let has_synced = self
            .db
            .query_all("PRAGMA table_info(changes)", &[])
            .map(|rows| {
                rows.iter().any(|row| {
                    row.get::<String>("name")
                        .map(|n| n == "synced")
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);

        if has_synced {
            debug!("ensure_synced_column: 'synced' column already exists on changes table");
        } else {
            debug!("ensure_synced_column: adding 'synced' column to changes table");
            if let Err(e) = self
                .db
                .execute_batch("ALTER TABLE changes ADD COLUMN synced INTEGER DEFAULT 0")
            {
                error!(
                    "ensure_synced_column: failed to add 'synced' column: {}",
                    e
                );
            } else {
                info!("ensure_synced_column: 'synced' column added to changes table");
            }
        }
    }

    /// Inner logic for [`mark_synced`](Self::mark_synced) — runs inside
    /// an already-open transaction.
    fn mark_synced_inner(
        &self,
        table: &str,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<usize, TrackingError> {
        // Clear the dirty flag on the data record.
        let sql = format!("UPDATE {} SET _dirty = 0 WHERE id = ?1", table);
        let affected = self.db.execute(&sql, rusqlite::params![entity_id])?;

        // Mark all change-log entries for this entity as synced.
        self.db.execute(
            "UPDATE changes SET synced = 1 WHERE entity_type = ?1 AND entity_id = ?2",
            rusqlite::params![entity_type, entity_id],
        )?;

        Ok(affected)
    }

    /// Count the total number of dirty records across all data tables
    /// using a single SQL query.
    fn get_dirty_record_count(&self) -> Result<usize, TrackingError> {
        let row = self.db.query_one(
            "SELECT \
                (SELECT COUNT(*) FROM accounts WHERE _dirty = 1) + \
                (SELECT COUNT(*) FROM transactions WHERE _dirty = 1) + \
                (SELECT COUNT(*) FROM journal_entries WHERE _dirty = 1) + \
                (SELECT COUNT(*) FROM transaction_entries WHERE _dirty = 1) + \
                (SELECT COUNT(*) FROM invoices WHERE _dirty = 1) + \
                (SELECT COUNT(*) FROM bills WHERE _dirty = 1) + \
                (SELECT COUNT(*) FROM assets WHERE _dirty = 1) \
                AS total",
            &[],
        )?;
        let total: i64 = row.get("total")?;
        Ok(total as usize)
    }

    /// Build the SQL for querying dirty records from a data table.
    ///
    /// Uses a correlated subquery to resolve the most recent operation
    /// from the `changes` table, defaulting to `"update"` when no change
    /// entry exists.
    fn build_dirty_record_sql(entity_type: &str, table: &str) -> String {
        format!(
            "SELECT \
                '{}' AS entity_type, \
                id AS entity_id, \
                COALESCE( \
                    (SELECT c.operation FROM changes c \
                     WHERE c.entity_type = '{}' AND c.entity_id = {}.id \
                     ORDER BY c.id DESC LIMIT 1), \
                    'update' \
                ) AS operation, \
                _modified_at AS modified_at \
             FROM {} WHERE _dirty = 1",
            entity_type, entity_type, table, table
        )
    }

    /// Parse a `Row` from a dirty-record query into a `DirtyRecord`.
    fn row_to_dirty_record(row: &Row, entity_type: &str) -> Result<DirtyRecord, TrackingError> {
        let entity_id: String = row.get("entity_id")?;
        let operation: String = row.get("operation")?;

        // _modified_at may be NULL if the record was inserted via direct
        // SQL rather than through LocalStore.
        let modified_at_str: Option<String> = row.get("modified_at")?;
        let modified_at = modified_at_str
            .filter(|s| !s.is_empty())
            .map(|s| parse_dt(&s))
            .transpose()?
            .unwrap_or_else(Utc::now);

        Ok(DirtyRecord {
            entity_type: entity_type.to_string(),
            entity_id,
            operation,
            modified_at,
        })
    }
}

// ═══════════════════════════════════════════════════════════
// Private Free Functions
// ═══════════════════════════════════════════════════════════

/// Resolve a singular entity type to its plural data-table name.
///
/// Returns `None` for unrecognised types.
fn entity_type_to_table(entity_type: &str) -> Option<&'static str> {
    match entity_type {
        "account" => Some("accounts"),
        "transaction" => Some("transactions"),
        "journal_entry" => Some("journal_entries"),
        "transaction_entry" => Some("transaction_entries"),
        "invoice" => Some("invoices"),
        "bill" => Some("bills"),
        "asset" => Some("assets"),
        _ => None,
    }
}

/// Parse an RFC 3339 datetime string into a `DateTime<Utc>`.
fn parse_dt(s: &str) -> Result<DateTime<Utc>, LocalDbError> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            LocalDbError::InvalidData(format!("Failed to parse datetime '{}': {}", s, e))
        })
}

/// Parse a `Row` from a `changes`-table query into a `ChangeEntry`.
fn row_to_change_entry(row: &Row) -> Result<ChangeEntry, TrackingError> {
    let id: i64 = row.get("id")?;
    let entity_type: String = row.get("entity_type")?;
    let entity_id: String = row.get("entity_id")?;
    let operation: String = row.get("operation")?;
    let timestamp_str: String = row.get("timestamp")?;
    let synced_val: i64 = row.get("synced")?;

    Ok(ChangeEntry {
        id,
        entity_type,
        entity_id,
        operation,
        timestamp: parse_dt(&timestamp_str)?,
        synced: synced_val != 0,
    })
}

// ═══════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::local_db::LocalDb;
    use super::super::store::LocalStore;
    use crate::database::financial::{
        Account, AccountType,
        Transaction, TransactionType, TransactionStatus,
        TransactionEntry, EntryType,
        JournalEntry,
    };
    use chrono::{Utc, NaiveDate, DateTime};
    use rust_decimal_macros::dec;
    use uuid::Uuid;
    use std::sync::Arc;

    // ── Test Helpers ──────────────────────────────────────

    /// Create a `ChangeTracker` and `LocalStore` sharing the same
    /// in-memory `LocalDb`.
    fn make_tracker_and_store() -> (ChangeTracker, LocalStore) {
        let db = Arc::new(
            LocalDb::open_in_memory().expect("Failed to open in-memory database"),
        );
        let tracker = ChangeTracker::new(db.clone());
        let store = LocalStore::new(db);
        (tracker, store)
    }

    /// Create a `ChangeTracker` backed by an in-memory database.
    fn make_tracker() -> ChangeTracker {
        let db = Arc::new(
            LocalDb::open_in_memory().expect("Failed to open in-memory database"),
        );
        ChangeTracker::new(db)
    }

    /// Create a simple asset account with the given number.
    fn make_account(number: &str) -> Account {
        Account::new(number, &format!("Account {}", number), AccountType::Asset)
    }

    /// Create a balanced transaction with the given number.
    fn make_transaction(number: &str) -> Transaction {
        let acct = Uuid::new_v4();
        let entries = vec![
            TransactionEntry::new(acct, EntryType::Debit, dec!(100), "Debit entry"),
            TransactionEntry::new(acct, EntryType::Credit, dec!(100), "Credit entry"),
        ];
        let mut txn = Transaction::new("Test transaction".to_string(), Utc::now(), entries);
        txn.id = Uuid::new_v4();
        txn.number = number.to_string();
        txn.transaction_type = TransactionType::Payment;
        txn.status = TransactionStatus::Posted;
        txn
    }

    /// Create a balanced journal entry with the given number.
    fn make_journal_entry(number: &str) -> JournalEntry {
        let mut je = JournalEntry::new(
            "Test journal entry",
            NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(),
        );
        je.id = Uuid::new_v4();
        je.number = number.to_string();
        je.add_entry(TransactionEntry::new(
            Uuid::new_v4(),
            EntryType::Debit,
            dec!(500),
            "Debit entry",
        ));
        je.add_entry(TransactionEntry::new(
            Uuid::new_v4(),
            EntryType::Credit,
            dec!(500),
            "Credit entry",
        ));
        je
    }

    /// Create a standalone transaction entry.
    fn make_transaction_entry() -> TransactionEntry {
        TransactionEntry::new(
            Uuid::new_v4(),
            EntryType::Debit,
            dec!(1000),
            "Test entry",
        )
    }

    // ── Empty Database Tests ─────────────────────────────

    #[test]
    fn test_empty_database_returns_no_dirty_records() {
        let tracker = make_tracker();

        let dirty = tracker.get_dirty_records().expect("get_dirty_records failed");
        assert!(dirty.is_empty(), "Empty database should have no dirty records");
    }

    #[test]
    fn test_empty_database_returns_no_pending_changes() {
        let tracker = make_tracker();

        let changes = tracker.get_pending_changes().expect("get_pending_changes failed");
        assert!(changes.is_empty());

        let count = tracker
            .get_pending_changes_count()
            .expect("get_pending_changes_count failed");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_empty_database_sync_state() {
        let tracker = make_tracker();

        let state = tracker.get_sync_state().expect("get_sync_state failed");
        assert!(state.last_sync.is_none());
        assert_eq!(state.pending_changes, 0);
        assert!(state.pending_by_type.is_empty());
    }

    // ── get_dirty_records Tests ──────────────────────────

    #[test]
    fn test_get_dirty_records_after_account_save() {
        let (tracker, store) = make_tracker_and_store();
        let account = make_account("1000");
        store.save_account(&account).expect("save account");

        let dirty = tracker.get_dirty_records().expect("get_dirty_records failed");
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].entity_type, "account");
        assert_eq!(dirty[0].entity_id, account.id.to_string());
        assert_eq!(dirty[0].operation, "insert");
    }

    #[test]
    fn test_get_dirty_records_after_multiple_saves() {
        let (tracker, store) = make_tracker_and_store();

        let acct = make_account("1000");
        store.save_account(&acct).expect("save account");

        let txn = make_transaction("TXN-001");
        store.save_transaction(&txn).expect("save transaction");

        let je = make_journal_entry("JE-001");
        store.save_journal_entry(&je).expect("save journal entry");

        let dirty = tracker.get_dirty_records().expect("get_dirty_records failed");
        assert_eq!(dirty.len(), 3);

        // Verify each entity type is present.
        let types: Vec<&str> = dirty.iter().map(|d| d.entity_type.as_str()).collect();
        assert!(types.contains(&"account"));
        assert!(types.contains(&"transaction"));
        assert!(types.contains(&"journal_entry"));
    }

    #[test]
    fn test_get_dirty_records_operation_reflects_latest() {
        let (tracker, store) = make_tracker_and_store();
        let account = make_account("1000");

        // Insert
        store.save_account(&account).expect("save 1");

        // Update
        let mut updated = account.clone();
        updated.name = "Updated Account".to_string();
        store.save_account(&updated).expect("save 2");

        let dirty = tracker.get_dirty_records().expect("get_dirty_records failed");
        assert_eq!(dirty.len(), 1);
        // Latest operation should be "update".
        assert_eq!(dirty[0].operation, "update");
    }

    #[test]
    fn test_get_dirty_records_includes_modified_at() {
        let (tracker, store) = make_tracker_and_store();
        let account = make_account("1000");
        store.save_account(&account).expect("save account");

        let dirty = tracker.get_dirty_records().expect("get_dirty_records failed");
        assert_eq!(dirty.len(), 1);
        // modified_at should be a recent timestamp.
        let now = Utc::now();
        let diff = now.signed_duration_since(dirty[0].modified_at);
        assert!(
            diff.num_seconds().abs() < 10,
            "modified_at should be close to now: diff = {:?}s",
            diff.num_seconds()
        );
    }

    // ── get_dirty_records_by_type Tests ──────────────────

    #[test]
    fn test_get_dirty_records_by_type_filters_correctly() {
        let (tracker, store) = make_tracker_and_store();

        store.save_account(&make_account("1000")).expect("save account");
        store
            .save_transaction(&make_transaction("TXN-001"))
            .expect("save transaction");
        store
            .save_journal_entry(&make_journal_entry("JE-001"))
            .expect("save journal entry");

        // Only accounts should be returned.
        let accounts = tracker
            .get_dirty_records_by_type("account")
            .expect("get_dirty_records_by_type failed");
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].entity_type, "account");

        // Only transactions.
        let txns = tracker
            .get_dirty_records_by_type("transaction")
            .expect("get_dirty_records_by_type failed");
        assert_eq!(txns.len(), 1);
        assert_eq!(txns[0].entity_type, "transaction");

        // Only journal entries.
        let jes = tracker
            .get_dirty_records_by_type("journal_entry")
            .expect("get_dirty_records_by_type failed");
        assert_eq!(jes.len(), 1);
        assert_eq!(jes[0].entity_type, "journal_entry");
    }

    #[test]
    fn test_get_dirty_records_by_type_unknown_returns_error() {
        let tracker = make_tracker();
        let result = tracker.get_dirty_records_by_type("unknown_type");
        assert!(result.is_err());
        match result {
            Err(TrackingError::InvalidOperation(msg)) => {
                assert!(msg.contains("unknown_type"));
            }
            Err(e) => panic!("Expected InvalidOperation, got: {:?}", e),
            Ok(_) => panic!("Expected error for unknown entity type"),
        }
    }

    #[test]
    fn test_get_dirty_records_by_type_empty_result() {
        let (tracker, store) = make_tracker_and_store();
        store.save_account(&make_account("1000")).expect("save account");

        // No dirty transactions exist.
        let txns = tracker
            .get_dirty_records_by_type("transaction")
            .expect("get_dirty_records_by_type failed");
        assert!(txns.is_empty());
    }

    // ── mark_synced Tests ────────────────────────────────

    #[test]
    fn test_mark_synced_clears_dirty_flag() {
        let (tracker, store) = make_tracker_and_store();
        let account = make_account("1000");
        store.save_account(&account).expect("save account");

        // Verify it's dirty.
        let dirty = tracker.get_dirty_records().expect("get_dirty_records");
        assert_eq!(dirty.len(), 1);

        // Mark synced.
        tracker
            .mark_synced("account", &account.id.to_string())
            .expect("mark_synced failed");

        // Verify it's no longer dirty.
        let dirty = tracker.get_dirty_records().expect("get_dirty_records");
        assert!(dirty.is_empty());
    }

    #[test]
    fn test_mark_synced_marks_changes_as_synced() {
        let (tracker, store) = make_tracker_and_store();
        let account = make_account("1000");
        store.save_account(&account).expect("save account");

        // Before mark_synced, the change entry should have synced = false.
        let changes = tracker.get_pending_changes().expect("get_pending_changes");
        assert_eq!(changes.len(), 1);
        assert!(!changes[0].synced);

        // Mark synced.
        tracker
            .mark_synced("account", &account.id.to_string())
            .expect("mark_synced failed");

        // After mark_synced, the change entry should have synced = true.
        let changes = tracker.get_pending_changes().expect("get_pending_changes");
        assert_eq!(changes.len(), 1);
        assert!(changes[0].synced);
    }

    #[test]
    fn test_mark_synced_unknown_entity_type() {
        let tracker = make_tracker();
        let result = tracker.mark_synced("unknown_type", "some-id");
        assert!(result.is_err());
        match result {
            Err(TrackingError::InvalidOperation(msg)) => {
                assert!(msg.contains("unknown_type"));
            }
            Err(e) => panic!("Expected InvalidOperation, got: {:?}", e),
            Ok(_) => panic!("Expected error for unknown entity type"),
        }
    }

    #[test]
    fn test_mark_synced_individual_records() {
        let (tracker, store) = make_tracker_and_store();

        let a1 = make_account("1000");
        let a2 = make_account("2000");
        store.save_account(&a1).expect("save 1");
        store.save_account(&a2).expect("save 2");

        // Mark only a1 as synced.
        tracker
            .mark_synced("account", &a1.id.to_string())
            .expect("mark_synced failed");

        // a2 should still be dirty.
        let dirty = tracker.get_dirty_records().expect("get_dirty_records");
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].entity_id, a2.id.to_string());
    }

    // ── mark_all_synced Tests ────────────────────────────

    #[test]
    fn test_mark_all_synced_clears_all_dirty_flags() {
        let (tracker, store) = make_tracker_and_store();

        store.save_account(&make_account("1000")).expect("save account");
        store
            .save_transaction(&make_transaction("TXN-001"))
            .expect("save transaction");
        store
            .save_journal_entry(&make_journal_entry("JE-001"))
            .expect("save journal entry");

        // Verify we have dirty records.
        assert_eq!(
            tracker
                .get_dirty_records()
                .expect("get_dirty_records")
                .len(),
            3
        );

        // Mark all synced.
        tracker.mark_all_synced().expect("mark_all_synced failed");

        // Verify no dirty records remain.
        let dirty = tracker.get_dirty_records().expect("get_dirty_records");
        assert!(dirty.is_empty());
    }

    #[test]
    fn test_mark_all_synced_truncates_changes_table() {
        let (tracker, store) = make_tracker_and_store();
        store.save_account(&make_account("1000")).expect("save account");
        store
            .save_transaction(&make_transaction("TXN-001"))
            .expect("save transaction");

        // Verify changes exist.
        assert_eq!(
            tracker
                .get_pending_changes_count()
                .expect("get_pending_changes_count"),
            2
        );

        // Mark all synced.
        tracker.mark_all_synced().expect("mark_all_synced failed");

        // Verify changes table is empty.
        assert_eq!(
            tracker
                .get_pending_changes_count()
                .expect("get_pending_changes_count"),
            0
        );
    }

    #[test]
    fn test_mark_all_synced_resets_pending_count() {
        let (tracker, store) = make_tracker_and_store();
        store.save_account(&make_account("1000")).expect("save account");

        tracker.mark_all_synced().expect("mark_all_synced failed");

        let state = tracker.get_sync_state().expect("get_sync_state");
        assert_eq!(state.pending_changes, 0);
    }

    // ── get_pending_changes Tests ────────────────────────

    #[test]
    fn test_get_pending_changes_returns_entries() {
        let (tracker, store) = make_tracker_and_store();
        let account = make_account("1000");
        store.save_account(&account).expect("save account");

        let changes = tracker.get_pending_changes().expect("get_pending_changes");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].entity_type, "account");
        assert_eq!(changes[0].entity_id, account.id.to_string());
        assert_eq!(changes[0].operation, "insert");
        assert!(!changes[0].synced);
    }

    #[test]
    fn test_get_pending_changes_ordered_by_timestamp() {
        let (tracker, store) = make_tracker_and_store();

        store.save_account(&make_account("1000")).expect("save 1");
        store
            .save_transaction(&make_transaction("TXN-001"))
            .expect("save 2");
        store
            .save_journal_entry(&make_journal_entry("JE-001"))
            .expect("save 3");

        let changes = tracker.get_pending_changes().expect("get_pending_changes");
        assert_eq!(changes.len(), 3);
        // Should be ordered by timestamp ascending (oldest first).
        assert!(changes[0].timestamp <= changes[1].timestamp);
        assert!(changes[1].timestamp <= changes[2].timestamp);
    }

    // ── get_pending_changes_count Tests ──────────────────

    #[test]
    fn test_get_pending_changes_count() {
        let (tracker, store) = make_tracker_and_store();

        assert_eq!(
            tracker
                .get_pending_changes_count()
                .expect("count failed"),
            0
        );

        store.save_account(&make_account("1000")).expect("save 1");
        assert_eq!(
            tracker
                .get_pending_changes_count()
                .expect("count failed"),
            1
        );

        store
            .save_transaction(&make_transaction("TXN-001"))
            .expect("save 2");
        assert_eq!(
            tracker
                .get_pending_changes_count()
                .expect("count failed"),
            2
        );
    }

    // ── get_changes_since Tests ──────────────────────────

    #[test]
    fn test_get_changes_since_filters_by_timestamp() {
        let tracker = make_tracker();

        // Insert changes with specific timestamps directly.
        tracker
            .db
            .execute(
                "INSERT INTO changes (entity_type, entity_id, operation, timestamp) \
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    "account",
                    "id-old",
                    "insert",
                    "2026-07-01T10:00:00+00:00"
                ],
            )
            .expect("insert old change");

        tracker
            .db
            .execute(
                "INSERT INTO changes (entity_type, entity_id, operation, timestamp) \
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    "account",
                    "id-new",
                    "insert",
                    "2026-07-01T12:00:00+00:00"
                ],
            )
            .expect("insert new change");

        let since = DateTime::parse_from_rfc3339("2026-07-01T11:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let changes = tracker
            .get_changes_since(since)
            .expect("get_changes_since failed");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].entity_id, "id-new");
    }

    #[test]
    fn test_get_changes_since_returns_all_after_cutoff() {
        let tracker = make_tracker();

        for i in 1..=5 {
            let ts = format!("2026-07-01T{:02}:00:00+00:00", 10 + i);
            tracker
                .db
                .execute(
                    "INSERT INTO changes (entity_type, entity_id, operation, timestamp) \
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params!["account", format!("id-{}", i), "insert", ts],
                )
                .expect("insert change");
        }

        let since = DateTime::parse_from_rfc3339("2026-07-01T12:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let changes = tracker
            .get_changes_since(since)
            .expect("get_changes_since failed");
        // Changes at 13:00, 14:00, 15:00 — 3 entries after 12:00.
        assert_eq!(changes.len(), 3);
    }

    #[test]
    fn test_get_changes_since_empty_result() {
        let tracker = make_tracker();
        let since = Utc::now();
        let changes = tracker
            .get_changes_since(since)
            .expect("get_changes_since failed");
        assert!(changes.is_empty());
    }

    // ── clear_synced_changes Tests ───────────────────────

    #[test]
    fn test_clear_synced_changes_removes_synced_entries() {
        let (tracker, store) = make_tracker_and_store();
        let account = make_account("1000");
        store.save_account(&account).expect("save account");

        // Mark as synced — this sets synced = 1 on the change entry.
        tracker
            .mark_synced("account", &account.id.to_string())
            .expect("mark_synced failed");

        // Verify the change entry exists and is synced.
        let changes = tracker.get_pending_changes().expect("get_pending_changes");
        assert_eq!(changes.len(), 1);
        assert!(changes[0].synced);

        // Clear synced changes.
        tracker
            .clear_synced_changes()
            .expect("clear_synced_changes failed");

        // Verify the synced entry was removed.
        let changes = tracker.get_pending_changes().expect("get_pending_changes");
        assert!(changes.is_empty());
    }

    #[test]
    fn test_clear_synced_changes_preserves_unsynced() {
        let (tracker, store) = make_tracker_and_store();

        let a1 = make_account("1000");
        let a2 = make_account("2000");
        store.save_account(&a1).expect("save 1");
        store.save_account(&a2).expect("save 2");

        // Mark only a1 as synced.
        tracker
            .mark_synced("account", &a1.id.to_string())
            .expect("mark_synced failed");

        // Clear synced changes.
        tracker
            .clear_synced_changes()
            .expect("clear_synced_changes failed");

        // a2's change entry should still be there.
        let changes = tracker.get_pending_changes().expect("get_pending_changes");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].entity_id, a2.id.to_string());
        assert!(!changes[0].synced);
    }

    #[test]
    fn test_clear_synced_changes_empty_database() {
        let tracker = make_tracker();
        // Should be a no-op, not an error.
        tracker
            .clear_synced_changes()
            .expect("clear_synced_changes failed");
    }

    // ── get_sync_state Tests ─────────────────────────────

    #[test]
    fn test_get_sync_state_returns_correct_summary() {
        let (tracker, store) = make_tracker_and_store();

        store.save_account(&make_account("1000")).expect("save 1");
        store
            .save_transaction(&make_transaction("TXN-001"))
            .expect("save 2");
        store
            .save_journal_entry(&make_journal_entry("JE-001"))
            .expect("save 3");

        let state = tracker.get_sync_state().expect("get_sync_state failed");
        assert!(state.last_sync.is_none());
        assert_eq!(state.pending_changes, 3);
    }

    #[test]
    fn test_get_sync_state_pending_by_type_breakdown() {
        let (tracker, store) = make_tracker_and_store();

        // 2 accounts, 1 transaction, 1 journal entry.
        store.save_account(&make_account("1000")).expect("save 1");
        store.save_account(&make_account("2000")).expect("save 2");
        store
            .save_transaction(&make_transaction("TXN-001"))
            .expect("save 3");
        store
            .save_journal_entry(&make_journal_entry("JE-001"))
            .expect("save 4");

        let state = tracker.get_sync_state().expect("get_sync_state failed");

        assert_eq!(state.pending_changes, 4);
        assert_eq!(*state.pending_by_type.get("account").unwrap(), 2);
        assert_eq!(*state.pending_by_type.get("transaction").unwrap(), 1);
        assert_eq!(*state.pending_by_type.get("journal_entry").unwrap(), 1);
        assert!(!state.pending_by_type.contains_key("invoice"));
    }

    #[test]
    fn test_get_sync_state_after_mark_synced() {
        let (tracker, store) = make_tracker_and_store();

        store.save_account(&make_account("1000")).expect("save 1");
        store
            .save_transaction(&make_transaction("TXN-001"))
            .expect("save 2");

        // Mark account as synced.
        let dirty = tracker.get_dirty_records().expect("get_dirty_records");
        let account_id = dirty
            .iter()
            .find(|d| d.entity_type == "account")
            .map(|d| d.entity_id.clone())
            .expect("should have a dirty account");
        tracker
            .mark_synced("account", &account_id)
            .expect("mark_synced failed");

        let state = tracker.get_sync_state().expect("get_sync_state");
        assert_eq!(state.pending_changes, 1);
        assert!(!state.pending_by_type.contains_key("account"));
        assert_eq!(*state.pending_by_type.get("transaction").unwrap(), 1);
    }

    // ── update_sync_state Tests ──────────────────────────

    #[test]
    fn test_update_sync_state_persists_timestamp() {
        let tracker = make_tracker();
        let ts = Utc::now();

        tracker
            .update_sync_state(ts)
            .expect("update_sync_state failed");

        let state = tracker.get_sync_state().expect("get_sync_state");
        assert!(state.last_sync.is_some());
        let stored = state.last_sync.unwrap();
        let diff = ts.signed_duration_since(stored);
        assert!(
            diff.num_milliseconds().abs() < 1000,
            "Stored timestamp should be close to the one set"
        );
    }

    #[test]
    fn test_update_sync_state_updates_pending_count() {
        let (tracker, store) = make_tracker_and_store();
        store.save_account(&make_account("1000")).expect("save account");

        tracker
            .update_sync_state(Utc::now())
            .expect("update_sync_state failed");

        // The sync_state table should now have pending_changes = 1.
        let row = tracker
            .db
            .query_one("SELECT pending_changes FROM sync_state WHERE id = 1", &[])
            .expect("query sync_state");
        assert_eq!(row.get::<i64>("pending_changes").unwrap(), 1);
    }

    // ── record_change Tests ──────────────────────────────

    #[test]
    fn test_record_change_adds_entry() {
        let tracker = make_tracker();

        tracker
            .record_change("account", "test-id-123", "insert")
            .expect("record_change failed");

        let changes = tracker.get_pending_changes().expect("get_pending_changes");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].entity_type, "account");
        assert_eq!(changes[0].entity_id, "test-id-123");
        assert_eq!(changes[0].operation, "insert");
    }

    #[test]
    fn test_record_change_invalid_operation() {
        let tracker = make_tracker();
        let result = tracker.record_change("account", "id", "invalid_op");
        assert!(result.is_err());
        match result {
            Err(TrackingError::InvalidOperation(msg)) => {
                assert!(msg.contains("invalid_op"));
            }
            Err(e) => panic!("Expected InvalidOperation, got: {:?}", e),
            Ok(_) => panic!("Expected error for invalid operation"),
        }
    }

    // ── Multiple Operations Tracking Tests ───────────────

    #[test]
    fn test_multiple_operations_on_same_entity_tracked_individually() {
        let (tracker, store) = make_tracker_and_store();
        let account = make_account("1000");

        // Insert
        store.save_account(&account).expect("save 1");

        // Update
        let mut updated = account.clone();
        updated.name = "Updated".to_string();
        store.save_account(&updated).expect("save 2");

        // Delete (soft)
        store.delete_account(account.id).expect("delete");

        let changes = tracker.get_pending_changes().expect("get_pending_changes");
        assert_eq!(changes.len(), 3);

        // Each operation should be tracked individually, in order.
        assert_eq!(changes[0].operation, "insert");
        assert_eq!(changes[1].operation, "update");
        assert_eq!(changes[2].operation, "delete");

        // All should reference the same entity.
        for c in &changes {
            assert_eq!(c.entity_type, "account");
            assert_eq!(c.entity_id, account.id.to_string());
        }
    }

    // ── Synced Column Schema Evolution Tests ─────────────

    #[test]
    fn test_synced_column_exists_after_new() {
        let tracker = make_tracker();

        // Verify the 'synced' column exists on the changes table.
        let cols = tracker
            .db
            .query_all("PRAGMA table_info(changes)", &[])
            .expect("PRAGMA table_info failed");
        let has_synced = cols
            .iter()
            .any(|row| {
                row.get::<String>("name")
                    .map(|n| n == "synced")
                    .unwrap_or(false)
            });
        assert!(has_synced, "changes table should have a 'synced' column");
    }

    #[test]
    fn test_new_is_idempotent_for_synced_column() {
        let db = Arc::new(
            LocalDb::open_in_memory().expect("Failed to open in-memory database"),
        );

        // First construction adds the column.
        let _t1 = ChangeTracker::new(db.clone());

        // Second construction should be a no-op (no error).
        let _t2 = ChangeTracker::new(db.clone());

        // Verify the column exists exactly once.
        let cols = db
            .query_all("PRAGMA table_info(changes)", &[])
            .expect("PRAGMA failed");
        let synced_count = cols
            .iter()
            .filter(|row| {
                row.get::<String>("name")
                    .map(|n| n == "synced")
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(synced_count, 1, "synced column should exist exactly once");
    }

    // ── Transaction Entry Dirty Tracking Tests ───────────

    #[test]
    fn test_transaction_entry_dirty_tracking() {
        let (tracker, store) = make_tracker_and_store();
        let entry = make_transaction_entry();
        store
            .save_transaction_entry(&entry)
            .expect("save entry");

        let dirty = tracker.get_dirty_records().expect("get_dirty_records");
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].entity_type, "transaction_entry");
        assert_eq!(dirty[0].entity_id, entry.id.to_string());
        assert_eq!(dirty[0].operation, "insert");
    }

    // ── Full Sync Flow Integration Test ──────────────────

    #[test]
    fn test_full_sync_flow() {
        let (tracker, store) = make_tracker_and_store();

        // Create some dirty records.
        store.save_account(&make_account("1000")).expect("save 1");
        store
            .save_transaction(&make_transaction("TXN-001"))
            .expect("save 2");
        store
            .save_journal_entry(&make_journal_entry("JE-001"))
            .expect("save 3");

        // Verify dirty state.
        let dirty = tracker.get_dirty_records().expect("get_dirty_records");
        assert_eq!(dirty.len(), 3);

        let count = tracker
            .get_pending_changes_count()
            .expect("get_pending_changes_count");
        assert_eq!(count, 3);

        // Simulate a full sync: mark all synced.
        tracker.mark_all_synced().expect("mark_all_synced failed");

        // Verify everything is clean.
        let dirty = tracker.get_dirty_records().expect("get_dirty_records");
        assert!(dirty.is_empty());

        let count = tracker
            .get_pending_changes_count()
            .expect("get_pending_changes_count");
        assert_eq!(count, 0);

        // Update sync state.
        let sync_time = Utc::now();
        tracker
            .update_sync_state(sync_time)
            .expect("update_sync_state failed");

        let state = tracker.get_sync_state().expect("get_sync_state");
        assert!(state.last_sync.is_some());
        assert_eq!(state.pending_changes, 0);
    }

    // ── Incremental Sync Flow Integration Test ───────────

    #[test]
    fn test_incremental_sync_flow() {
        let (tracker, store) = make_tracker_and_store();

        // Create two accounts.
        let a1 = make_account("1000");
        let a2 = make_account("2000");
        store.save_account(&a1).expect("save 1");
        store.save_account(&a2).expect("save 2");

        // Sync only a1.
        tracker
            .mark_synced("account", &a1.id.to_string())
            .expect("mark_synced a1");

        // a1 should be clean, a2 still dirty.
        let dirty = tracker.get_dirty_records().expect("get_dirty_records");
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].entity_id, a2.id.to_string());

        // Clear synced changes (removes a1's change entry).
        tracker
            .clear_synced_changes()
            .expect("clear_synced_changes");

        // Only a2's change entry should remain.
        let changes = tracker.get_pending_changes().expect("get_pending_changes");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].entity_id, a2.id.to_string());
        assert!(!changes[0].synced);

        // Now sync a2.
        tracker
            .mark_synced("account", &a2.id.to_string())
            .expect("mark_synced a2");

        // Everything should be clean.
        let dirty = tracker.get_dirty_records().expect("get_dirty_records");
        assert!(dirty.is_empty());

        // Update sync state.
        tracker
            .update_sync_state(Utc::now())
            .expect("update_sync_state");

        let state = tracker.get_sync_state().expect("get_sync_state");
        assert_eq!(state.pending_changes, 0);
    }

    // ── Error Display Tests ──────────────────────────────

    #[test]
    fn test_tracking_error_display() {
        let e = TrackingError::NotFound("Account 123".to_string());
        assert_eq!(format!("{}", e), "Not found: Account 123");

        let e = TrackingError::InvalidOperation("bad op".to_string());
        assert_eq!(format!("{}", e), "Invalid operation: bad op");

        let e = TrackingError::Db(LocalDbError::NotFound);
        assert_eq!(format!("{}", e), "Local DB error: Record not found");
    }
}
