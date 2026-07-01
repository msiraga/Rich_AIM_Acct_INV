//! Edge Local Database
//!
//! Embedded SQLite database for offline-first operation.
//! Schema mirrors the SurrealDB tables for seamless sync.
//!
//! # Design
//!
//! - Uses `rusqlite` for a synchronous, embedded SQLite connection
//! - `std::sync::Mutex` wraps the connection (SQLite operations are fast and
//!   blocking; `tokio::task::spawn_blocking` can be used for long queries)
//! - Decimal values stored as TEXT to preserve precision
//! - DateTime values stored as TEXT (RFC 3339)
//! - UUID values stored as TEXT
//! - JSON fields (entries, metadata, document_ids, bank_details) stored as TEXT
//! - Every data table includes `_dirty` and `_modified_at` columns for
//!   change tracking, plus `created_at` and `updated_at` timestamps
//! - Versioned SQL migrations applied on first connect
//! - Infrastructure tables (`schema_version`, `sync_state`, `changes`) do
//!   not carry `_dirty` / `_modified_at` since they are not synced entities

use std::sync::Mutex;
use rusqlite::{Connection, ToSql};
use rusqlite::types::{Value, ValueRef, FromSql};
use thiserror::Error;
use tracing::{info, debug, error, warn};
use chrono::Utc;

// ═══════════════════════════════════════════════════════════
// Error Type
// ═══════════════════════════════════════════════════════════

/// Errors that can occur during local database operations.
#[derive(Error, Debug)]
pub enum LocalDbError {
    /// SQLite database error.
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// Migration failed.
    #[error("Migration error: {0}")]
    Migration(String),

    /// Database has not been initialized.
    #[error("Database not initialized")]
    NotInitialized,

    /// Query returned no rows when one was expected.
    #[error("Record not found")]
    NotFound,

    /// Data could not be converted to or from a SQLite value.
    #[error("Invalid data: {0}")]
    InvalidData(String),

    /// Filesystem I/O error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// The mutex guarding the connection was poisoned.
    #[error("Database lock poisoned")]
    PoisonedLock,
}

// ═══════════════════════════════════════════════════════════
// Owned Row Type
// ═══════════════════════════════════════════════════════════

/// An owned row from a database query.
///
/// Unlike `rusqlite::Row`, this type does not borrow from the connection
/// or statement, allowing it to be returned from query methods and stored
/// across lock boundaries.
#[derive(Debug, Clone)]
pub struct Row {
    /// Column names in the order they appear in the result set.
    columns: Vec<String>,
    /// Column values, parallel to `columns`.
    values: Vec<Value>,
}

impl Row {
    /// Create a new owned row from column names and values.
    pub fn new(columns: Vec<String>, values: Vec<Value>) -> Self {
        Self { columns, values }
    }

    /// Get a typed value by column name.
    ///
    /// # Type parameters
    /// - `T`: The target type, which must implement `rusqlite::types::FromSql`.
    ///
    /// # Errors
    /// Returns `LocalDbError::InvalidData` if the column is not found or the
    /// value cannot be converted to `T`.
    pub fn get<T: FromSql>(&self, column: &str) -> Result<T, LocalDbError> {
        let idx = self
            .columns
            .iter()
            .position(|c| c == column)
            .ok_or_else(|| LocalDbError::InvalidData(format!("Column '{}' not found", column)))?;
        let val_ref = value_to_value_ref(&self.values[idx]);
        T::column_result(val_ref)
            .map_err(|e| LocalDbError::InvalidData(format!("Failed to convert column '{}': {}", column, e)))
    }

    /// Get a typed value by column index (0-based).
    pub fn get_by_index<T: FromSql>(&self, idx: usize) -> Result<T, LocalDbError> {
        if idx >= self.values.len() {
            return Err(LocalDbError::InvalidData(format!(
                "Column index {} out of bounds (have {} columns)",
                idx,
                self.values.len()
            )));
        }
        let val_ref = value_to_value_ref(&self.values[idx]);
        T::column_result(val_ref)
            .map_err(|e| LocalDbError::InvalidData(format!("Failed to convert column at index {}: {}", idx, e)))
    }

    /// Get the raw `Value` for a column by name.
    pub fn get_value(&self, column: &str) -> Result<&Value, LocalDbError> {
        let idx = self
            .columns
            .iter()
            .position(|c| c == column)
            .ok_or_else(|| LocalDbError::InvalidData(format!("Column '{}' not found", column)))?;
        Ok(&self.values[idx])
    }

    /// Get the raw `Value` for a column by index.
    pub fn get_value_by_index(&self, idx: usize) -> Result<&Value, LocalDbError> {
        if idx >= self.values.len() {
            return Err(LocalDbError::InvalidData(format!(
                "Column index {} out of bounds",
                idx
            )));
        }
        Ok(&self.values[idx])
    }

    /// Return the column names in result-set order.
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// Number of columns in this row.
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// Whether the row has no columns (e.g. from a query that selected nothing).
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl Default for Row {
    fn default() -> Self {
        Self {
            columns: Vec::new(),
            values: Vec::new(),
        }
    }
}

// ═══════════════════════════════════════════════════════════
// Migration Definitions
// ═══════════════════════════════════════════════════════════

/// A single versioned SQL migration.
struct Migration {
    /// Monotonically increasing version number.
    version: i32,
    /// Human-readable description.
    description: &'static str,
    /// SQL statements to execute (separated by semicolons).
    sql: &'static str,
}

/// SQL for migration v1 — the initial schema.
///
/// Creates all tables for the local SQLite database, mirroring the
/// SurrealDB schema for seamless synchronization.  Data tables (accounts,
/// transactions, etc.) carry `_dirty` / `_modified_at` columns for change
/// tracking; infrastructure tables (schema_version, sync_state, changes)
/// do not, since they are not synced entities.
const MIGRATION_001_SQL: &str = r#"
-- ─── Infrastructure Tables ───

CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS sync_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    last_sync TEXT,
    last_sync_attempt TEXT,
    sync_in_progress INTEGER NOT NULL DEFAULT 0,
    pending_changes INTEGER NOT NULL DEFAULT 0
);

INSERT OR IGNORE INTO sync_state (id, last_sync, last_sync_attempt, sync_in_progress, pending_changes)
VALUES (1, NULL, NULL, 0, 0);

CREATE TABLE IF NOT EXISTS changes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    operation TEXT NOT NULL CHECK (operation IN ('insert', 'update', 'delete')),
    timestamp TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_changes_entity ON changes(entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_changes_timestamp ON changes(timestamp);

-- ─── Data Tables ───

-- Accounts — mirrors SurrealDB 'account' table
CREATE TABLE IF NOT EXISTS accounts (
    id TEXT PRIMARY KEY,
    number TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    account_type TEXT NOT NULL,
    parent_id TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    balance TEXT NOT NULL DEFAULT '0',
    currency TEXT NOT NULL DEFAULT 'USD',
    is_bank_account INTEGER NOT NULL DEFAULT 0,
    bank_details TEXT,
    is_reconciled INTEGER NOT NULL DEFAULT 0,
    last_reconciled TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    _dirty INTEGER NOT NULL DEFAULT 0,
    _modified_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_accounts_number ON accounts(number);
CREATE INDEX IF NOT EXISTS idx_accounts_type ON accounts(account_type);
CREATE INDEX IF NOT EXISTS idx_accounts_parent ON accounts(parent_id);
CREATE INDEX IF NOT EXISTS idx_accounts_status ON accounts(status);
CREATE INDEX IF NOT EXISTS idx_accounts_dirty ON accounts(_dirty);

-- Transaction entries — mirrors SurrealDB 'transaction_entry' table
CREATE TABLE IF NOT EXISTS transaction_entries (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    transaction_id TEXT,
    journal_entry_id TEXT,
    entry_type TEXT NOT NULL,
    amount TEXT NOT NULL DEFAULT '0',
    description TEXT NOT NULL DEFAULT '',
    reference TEXT,
    currency TEXT NOT NULL DEFAULT 'USD',
    exchange_rate TEXT,
    base_currency_amount TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    _dirty INTEGER NOT NULL DEFAULT 0,
    _modified_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_te_account ON transaction_entries(account_id);
CREATE INDEX IF NOT EXISTS idx_te_transaction ON transaction_entries(transaction_id);
CREATE INDEX IF NOT EXISTS idx_te_journal ON transaction_entries(journal_entry_id);
CREATE INDEX IF NOT EXISTS idx_te_dirty ON transaction_entries(_dirty);

-- Journal entries — mirrors SurrealDB 'journal_entry' table
CREATE TABLE IF NOT EXISTS journal_entries (
    id TEXT PRIMARY KEY,
    number TEXT NOT NULL,
    date TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    reference TEXT,
    entries TEXT NOT NULL DEFAULT '[]',
    is_posted INTEGER NOT NULL DEFAULT 0,
    posted_at TEXT,
    is_reconciled INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    _dirty INTEGER NOT NULL DEFAULT 0,
    _modified_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_je_number ON journal_entries(number);
CREATE INDEX IF NOT EXISTS idx_je_date ON journal_entries(date);
CREATE INDEX IF NOT EXISTS idx_je_posted ON journal_entries(is_posted);
CREATE INDEX IF NOT EXISTS idx_je_dirty ON journal_entries(_dirty);

-- Transactions — mirrors SurrealDB 'transaction' table
CREATE TABLE IF NOT EXISTS transactions (
    id TEXT PRIMARY KEY,
    number TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    date TEXT NOT NULL,
    transaction_type TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft',
    entries TEXT NOT NULL DEFAULT '[]',
    journal_entry_id TEXT,
    document_ids TEXT NOT NULL DEFAULT '[]',
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    _dirty INTEGER NOT NULL DEFAULT 0,
    _modified_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_txn_number ON transactions(number);
CREATE INDEX IF NOT EXISTS idx_txn_date ON transactions(date);
CREATE INDEX IF NOT EXISTS idx_txn_status ON transactions(status);
CREATE INDEX IF NOT EXISTS idx_txn_type ON transactions(transaction_type);
CREATE INDEX IF NOT EXISTS idx_txn_dirty ON transactions(_dirty);

-- Invoices
CREATE TABLE IF NOT EXISTS invoices (
    id TEXT PRIMARY KEY,
    number TEXT NOT NULL,
    customer_id TEXT,
    customer_name TEXT NOT NULL DEFAULT '',
    customer_email TEXT NOT NULL DEFAULT '',
    issue_date TEXT NOT NULL,
    due_date TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft',
    subtotal TEXT NOT NULL DEFAULT '0',
    tax_total TEXT NOT NULL DEFAULT '0',
    total TEXT NOT NULL DEFAULT '0',
    amount_paid TEXT NOT NULL DEFAULT '0',
    balance_due TEXT NOT NULL DEFAULT '0',
    currency TEXT NOT NULL DEFAULT 'USD',
    notes TEXT NOT NULL DEFAULT '',
    terms TEXT NOT NULL DEFAULT '',
    line_items TEXT NOT NULL DEFAULT '[]',
    transaction_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    _dirty INTEGER NOT NULL DEFAULT 0,
    _modified_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_inv_number ON invoices(number);
CREATE INDEX IF NOT EXISTS idx_inv_customer ON invoices(customer_id);
CREATE INDEX IF NOT EXISTS idx_inv_status ON invoices(status);
CREATE INDEX IF NOT EXISTS idx_inv_due_date ON invoices(due_date);
CREATE INDEX IF NOT EXISTS idx_inv_dirty ON invoices(_dirty);

-- Bills
CREATE TABLE IF NOT EXISTS bills (
    id TEXT PRIMARY KEY,
    number TEXT NOT NULL,
    vendor_id TEXT,
    vendor_name TEXT NOT NULL DEFAULT '',
    issue_date TEXT NOT NULL,
    due_date TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft',
    subtotal TEXT NOT NULL DEFAULT '0',
    tax_total TEXT NOT NULL DEFAULT '0',
    total TEXT NOT NULL DEFAULT '0',
    amount_paid TEXT NOT NULL DEFAULT '0',
    balance_due TEXT NOT NULL DEFAULT '0',
    currency TEXT NOT NULL DEFAULT 'USD',
    notes TEXT NOT NULL DEFAULT '',
    line_items TEXT NOT NULL DEFAULT '[]',
    transaction_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    _dirty INTEGER NOT NULL DEFAULT 0,
    _modified_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_bill_number ON bills(number);
CREATE INDEX IF NOT EXISTS idx_bill_vendor ON bills(vendor_id);
CREATE INDEX IF NOT EXISTS idx_bill_status ON bills(status);
CREATE INDEX IF NOT EXISTS idx_bill_due_date ON bills(due_date);
CREATE INDEX IF NOT EXISTS idx_bill_dirty ON bills(_dirty);

-- Fixed assets
CREATE TABLE IF NOT EXISTS assets (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    asset_type TEXT NOT NULL,
    account_id TEXT,
    purchase_date TEXT NOT NULL,
    purchase_cost TEXT NOT NULL DEFAULT '0',
    salvage_value TEXT NOT NULL DEFAULT '0',
    useful_life_months INTEGER NOT NULL DEFAULT 0,
    depreciation_method TEXT NOT NULL DEFAULT 'straight_line',
    accumulated_depreciation TEXT NOT NULL DEFAULT '0',
    current_value TEXT NOT NULL DEFAULT '0',
    currency TEXT NOT NULL DEFAULT 'USD',
    is_disposed INTEGER NOT NULL DEFAULT 0,
    disposal_date TEXT,
    disposal_value TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    _dirty INTEGER NOT NULL DEFAULT 0,
    _modified_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_asset_type ON assets(asset_type);
CREATE INDEX IF NOT EXISTS idx_asset_account ON assets(account_id);
CREATE INDEX IF NOT EXISTS idx_asset_disposed ON assets(is_disposed);
CREATE INDEX IF NOT EXISTS idx_asset_dirty ON assets(_dirty);
"#;

/// All registered migrations, in version order.
const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        description: "Initial schema — accounts, transactions, journal_entries, \
                      transaction_entries, invoices, bills, assets, sync_state, changes",
        sql: MIGRATION_001_SQL,
    },
];

/// The highest migration version known to this build.
const LATEST_SCHEMA_VERSION: i32 = 1;

// ═══════════════════════════════════════════════════════════
// LocalDb
// ═══════════════════════════════════════════════════════════

/// Embedded SQLite database for offline-first edge operation.
///
/// Wraps a single `rusqlite::Connection` behind a `std::sync::Mutex`.
/// The schema mirrors the remote SurrealDB tables so that the sync
/// engine can push and pull records with minimal transformation.
///
/// # Concurrency
///
/// `LocalDb` is `Send + Sync`. For async contexts, wrap long-running
/// queries in `tokio::task::spawn_blocking` to avoid blocking the
/// runtime. For shared ownership, wrap in `Arc<LocalDb>`.
///
/// # Why `std::sync::Mutex` (not `tokio::sync::Mutex`)
///
/// SQLite operations are inherently synchronous. Using `std::sync::Mutex`
/// keeps the API synchronous, avoids async overhead, and aligns with the
/// tokio team's guidance for guarding short-lived, synchronous resources.
#[derive(Debug)]
pub struct LocalDb {
    conn: Mutex<Connection>,
    path: String,
}

impl LocalDb {
    // ── Construction ──────────────────────────────────────

    /// Open (or create) a SQLite database file at `path` and run migrations.
    ///
    /// Enables WAL journal mode, foreign-key enforcement, and `synchronous = NORMAL`
    /// for a good balance of durability and performance on local storage.
    pub fn open(path: &str) -> Result<Self, LocalDbError> {
        // Ensure the parent directory exists
        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent)?;
                debug!("Created parent directory for database: {:?}", parent);
            }
        }

        let conn = Connection::open(path)?;

        // Pragmas — WAL mode, foreign keys, normal sync
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             PRAGMA synchronous = NORMAL;",
        )?;

        info!("Opened local database at: {}", path);

        let db = Self {
            conn: Mutex::new(conn),
            path: path.to_string(),
        };

        // Apply any pending migrations
        db.run_migrations()?;

        Ok(db)
    }

    /// Create an in-memory SQLite database (for testing).
    ///
    /// Runs migrations so the schema is ready for immediate use.
    pub fn open_in_memory() -> Result<Self, LocalDbError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;

        debug!("Opened in-memory local database");

        let db = Self {
            conn: Mutex::new(conn),
            path: ":memory:".to_string(),
        };

        db.run_migrations()?;

        Ok(db)
    }

    // ── Migrations ────────────────────────────────────────

    /// Apply all pending schema migrations.
    ///
    /// Idempotent: calling this on an up-to-date database is a no-op.
    /// Each migration is wrapped in a transaction for atomicity.
    pub fn run_migrations(&self) -> Result<(), LocalDbError> {
        let mut conn = self.conn.lock().map_err(|_| LocalDbError::PoisonedLock)?;

        // Ensure the schema_version tracking table exists
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT ''
            );",
        )?;

        // Read the highest applied version
        let current_version: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        debug!(
            "Current schema version: {}, latest: {}",
            current_version, LATEST_SCHEMA_VERSION
        );

        if current_version > LATEST_SCHEMA_VERSION {
            warn!(
                "Database schema version {} is higher than latest known version {} — \
                 possible downgrade",
                current_version, LATEST_SCHEMA_VERSION
            );
        }

        if current_version >= LATEST_SCHEMA_VERSION {
            debug!("Database schema is up to date");
            return Ok(());
        }

        // Apply each pending migration in order
        for migration in MIGRATIONS {
            if migration.version > current_version {
                info!(
                    "Applying migration v{}: {}",
                    migration.version, migration.description
                );

                let tx = conn.transaction()?;
                tx.execute_batch(migration.sql).map_err(|e| {
                    LocalDbError::Migration(format!(
                        "Migration v{} failed: {}",
                        migration.version, e
                    ))
                })?;

                let now = Utc::now().to_rfc3339();
                tx.execute(
                    "INSERT OR IGNORE INTO schema_version \
                     (version, applied_at, description) VALUES (?1, ?2, ?3)",
                    rusqlite::params![migration.version, now, migration.description],
                )?;
                tx.commit()?;

                info!("Migration v{} applied successfully", migration.version);
            }
        }

        debug!("All migrations complete");
        Ok(())
    }

    /// Return the highest applied schema version, or 0 if none.
    pub fn get_schema_version(&self) -> i32 {
        let conn = match self.conn.lock() {
            Ok(guard) => guard,
            Err(_) => {
                error!("Failed to acquire database lock for schema version check");
                return 0;
            }
        };

        conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0)
    }

    /// Record that schema version `version` has been applied.
    ///
    /// Uses `INSERT OR IGNORE` so re-applying the same version is a no-op.
    pub fn set_schema_version(&self, version: i32) -> Result<(), LocalDbError> {
        let conn = self.conn.lock().map_err(|_| LocalDbError::PoisonedLock)?;

        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT OR IGNORE INTO schema_version \
             (version, applied_at, description) VALUES (?1, ?2, ?3)",
            rusqlite::params![version, now, format!("Schema version {}", version)],
        )?;

        debug!("Set schema version to {}", version);
        Ok(())
    }

    // ── Query Execution ───────────────────────────────────

    /// Execute a SQL statement with parameters and return rows affected.
    pub fn execute(&self, sql: &str, params: &[&dyn ToSql]) -> Result<usize, LocalDbError> {
        let conn = self.conn.lock().map_err(|_| LocalDbError::PoisonedLock)?;
        let count = conn.execute(sql, params)?;
        Ok(count)
    }

    /// Execute a batch of SQL statements (no parameters).
    ///
    /// Convenience for DDL or multi-statement scripts.
    pub fn execute_batch(&self, sql: &str) -> Result<(), LocalDbError> {
        let conn = self.conn.lock().map_err(|_| LocalDbError::PoisonedLock)?;
        conn.execute_batch(sql)?;
        Ok(())
    }

    /// Query for a single row.
    ///
    /// Returns `LocalDbError::NotFound` if no rows match.
    pub fn query_one(&self, sql: &str, params: &[&dyn ToSql]) -> Result<Row, LocalDbError> {
        let conn = self.conn.lock().map_err(|_| LocalDbError::PoisonedLock)?;

        match conn.query_row(sql, params, |row| {
            let count = row.column_count();
            let mut columns = Vec::with_capacity(count);
            let mut values = Vec::with_capacity(count);
            for i in 0..count {
                columns.push(row.column_name(i)?.to_string());
                let value: Value = row.get(i)?;
                values.push(value);
            }
            Ok(Row { columns, values })
        }) {
            Ok(row) => Ok(row),
            Err(rusqlite::Error::QueryReturnedNoRows) => Err(LocalDbError::NotFound),
            Err(e) => Err(LocalDbError::Sqlite(e)),
        }
    }

    /// Query for all matching rows.
    ///
    /// Returns an empty `Vec` if no rows match.
    pub fn query_all(&self, sql: &str, params: &[&dyn ToSql]) -> Result<Vec<Row>, LocalDbError> {
        let conn = self.conn.lock().map_err(|_| LocalDbError::PoisonedLock)?;
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params, |row| {
            let count = row.column_count();
            let mut columns = Vec::with_capacity(count);
            let mut values = Vec::with_capacity(count);
            for i in 0..count {
                columns.push(row.column_name(i)?.to_string());
                let value: Value = row.get(i)?;
                values.push(value);
            }
            Ok(Row { columns, values })
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    // ── Sync Helpers ──────────────────────────────────────

    /// Return the last successful sync timestamp (RFC 3339), if any.
    pub fn get_last_sync(&self) -> Option<String> {
        let conn = self.conn.lock().ok()?;
        conn.query_row(
            "SELECT last_sync FROM sync_state WHERE id = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
    }

    /// Update the last successful sync timestamp.
    pub fn set_last_sync(&self, timestamp: &str) -> Result<(), LocalDbError> {
        let conn = self.conn.lock().map_err(|_| LocalDbError::PoisonedLock)?;
        conn.execute(
            "UPDATE sync_state SET last_sync = ?1 WHERE id = 1",
            rusqlite::params![timestamp],
        )?;
        debug!("Set last sync to {}", timestamp);
        Ok(())
    }

    /// Record a change in the `changes` table for the sync engine to process.
    pub fn record_change(
        &self,
        entity_type: &str,
        entity_id: &str,
        operation: &str,
    ) -> Result<(), LocalDbError> {
        let conn = self.conn.lock().map_err(|_| LocalDbError::PoisonedLock)?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO changes (entity_type, entity_id, operation, timestamp) \
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![entity_type, entity_id, operation, now],
        )?;
        debug!("Recorded change: {} {} {}", operation, entity_type, entity_id);
        Ok(())
    }

    // ── Accessors ─────────────────────────────────────────

    /// The filesystem path (or `:memory:`) this database was opened with.
    pub fn path(&self) -> &str {
        &self.path
    }
}

// ═══════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════

/// Convert an owned `Value` to a borrowed `ValueRef` for `FromSql` calls.
///
/// This lets `Row::get` / `Row::get_by_index` invoke `T::column_result`
/// without holding a borrow on the underlying SQLite statement.
fn value_to_value_ref(val: &Value) -> ValueRef<'_> {
    match val {
        Value::Null => ValueRef::Null,
        Value::Integer(i) => ValueRef::Integer(*i),
        Value::Real(f) => ValueRef::Real(*f),
        Value::Text(s) => ValueRef::Text(s.as_bytes()),
        Value::Blob(b) => ValueRef::Blob(b.as_slice()),
    }
}

// ═══════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // ── Test Helpers ──────────────────────────────────────

    /// Create an in-memory database with migrations already applied.
    fn make_db() -> LocalDb {
        LocalDb::open_in_memory().expect("Failed to open in-memory database")
    }

    /// Collect all table names from `sqlite_master`.
    fn get_table_names(db: &LocalDb) -> HashSet<String> {
        let rows = db
            .query_all(
                "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
                &[],
            )
            .expect("Failed to query table names");
        rows.iter()
            .filter_map(|row| row.get::<String, _>("name").ok())
            .collect()
    }

    /// Get the column names for a table via `PRAGMA table_info`.
    fn get_table_columns(db: &LocalDb, table: &str) -> HashSet<String> {
        let sql = format!("PRAGMA table_info({})", table);
        let rows = db
            .query_all(&sql, &[])
            .unwrap_or_else(|_| panic!("Failed to query columns for table '{}'", table));
        rows.iter()
            .filter_map(|row| row.get::<String, _>("name").ok())
            .collect()
    }

    // ── Open Tests ────────────────────────────────────────

    #[test]
    fn test_open_creates_database_file() {
        let dir = tempfile::tempdir().expect("Failed to create temp dir");
        let db_path = dir.path().join("test_edge.db");
        let path_str = db_path.to_str().expect("Path not valid UTF-8");

        // File should not exist yet
        assert!(!db_path.exists(), "Database file should not exist before open");

        let db = LocalDb::open(path_str).expect("Failed to open database");

        // File should now exist
        assert!(db_path.exists(), "Database file should exist after open");
        // Migrations should have been applied
        assert!(
            db.get_schema_version() >= 1,
            "Schema version should be >= 1 after open"
        );
    }

    #[test]
    fn test_open_in_memory() {
        let db = LocalDb::open_in_memory().expect("Failed to open in-memory database");
        assert_eq!(db.path(), ":memory:");
        assert!(db.get_schema_version() >= 1);
    }

    #[test]
    fn test_open_creates_parent_directory() {
        let dir = tempfile::tempdir().expect("Failed to create temp dir");
        let nested = dir.path().join("nested").join("sub").join("dir");
        let db_path = nested.join("edge.db");
        let path_str = db_path.to_str().expect("Path not valid UTF-8");

        assert!(!nested.exists(), "Nested directory should not exist yet");

        let db = LocalDb::open(path_str).expect("Failed to open database with nested path");

        assert!(nested.exists(), "Parent directories should have been created");
        assert!(db_path.exists());
        assert!(db.get_schema_version() >= 1);
    }

    // ── Migration Tests ───────────────────────────────────

    #[test]
    fn test_migrations_create_all_tables() {
        let db = make_db();
        let tables = get_table_names(&db);

        // Infrastructure tables
        for table in &["schema_version", "sync_state", "changes"] {
            assert!(
                tables.contains(*table),
                "Infrastructure table '{}' should exist",
                table
            );
        }

        // Data tables
        for table in &[
            "accounts",
            "transactions",
            "journal_entries",
            "transaction_entries",
            "invoices",
            "bills",
            "assets",
        ] {
            assert!(
                tables.contains(*table),
                "Data table '{}' should exist",
                table
            );
        }
    }

    #[test]
    fn test_schema_version_tracking() {
        let db = make_db();

        // After open + migrations, version should be LATEST_SCHEMA_VERSION
        assert_eq!(db.get_schema_version(), LATEST_SCHEMA_VERSION);

        // set_schema_version should insert a new version record
        db.set_schema_version(99).expect("Failed to set schema version");
        assert_eq!(
            db.get_schema_version(),
            99,
            "get_schema_version should return max version"
        );

        // Re-setting the same version is a no-op (INSERT OR IGNORE)
        db.set_schema_version(99).expect("Failed to re-set schema version");
        assert_eq!(db.get_schema_version(), 99);
    }

    #[test]
    fn test_rerunning_migrations_is_idempotent() {
        let db = make_db();
        let version_before = db.get_schema_version();

        // Running migrations again should not error and should not change the version
        db.run_migrations().expect("Re-running migrations should succeed");

        let version_after = db.get_schema_version();
        assert_eq!(
            version_before, version_after,
            "Schema version should not change"
        );

        // Table count should not change
        let tables_before = get_table_names(&db);
        db.run_migrations().expect("Third migration run should succeed");
        let tables_after = get_table_names(&db);
        assert_eq!(tables_before.len(), tables_after.len());
    }

    // ── Column Verification Tests ─────────────────────────

    #[test]
    fn test_accounts_table_columns() {
        let db = make_db();
        let cols = get_table_columns(&db, "accounts");

        // Core financial fields
        for col in &[
            "id",
            "number",
            "name",
            "description",
            "account_type",
            "parent_id",
            "status",
            "balance",
            "currency",
            "is_bank_account",
            "bank_details",
            "is_reconciled",
            "last_reconciled",
        ] {
            assert!(cols.contains(*col), "accounts should have column '{}'", col);
        }

        // Sync tracking columns
        for col in &["created_at", "updated_at", "_dirty", "_modified_at"] {
            assert!(cols.contains(*col), "accounts should have sync column '{}'", col);
        }
    }

    #[test]
    fn test_transactions_table_columns() {
        let db = make_db();
        let cols = get_table_columns(&db, "transactions");

        for col in &[
            "id",
            "number",
            "description",
            "date",
            "transaction_type",
            "status",
            "entries",
            "journal_entry_id",
            "document_ids",
            "metadata",
            "created_at",
            "updated_at",
            "_dirty",
            "_modified_at",
        ] {
            assert!(
                cols.contains(*col),
                "transactions should have column '{}'",
                col
            );
        }
    }

    #[test]
    fn test_journal_entries_table_columns() {
        let db = make_db();
        let cols = get_table_columns(&db, "journal_entries");

        for col in &[
            "id",
            "number",
            "date",
            "description",
            "reference",
            "entries",
            "is_posted",
            "posted_at",
            "is_reconciled",
            "created_at",
            "updated_at",
            "_dirty",
            "_modified_at",
        ] {
            assert!(
                cols.contains(*col),
                "journal_entries should have column '{}'",
                col
            );
        }
    }

    #[test]
    fn test_transaction_entries_table_columns() {
        let db = make_db();
        let cols = get_table_columns(&db, "transaction_entries");

        for col in &[
            "id",
            "account_id",
            "transaction_id",
            "journal_entry_id",
            "entry_type",
            "amount",
            "description",
            "reference",
            "currency",
            "exchange_rate",
            "base_currency_amount",
            "created_at",
            "updated_at",
            "_dirty",
            "_modified_at",
        ] {
            assert!(
                cols.contains(*col),
                "transaction_entries should have column '{}'",
                col
            );
        }
    }

    #[test]
    fn test_invoices_table_columns() {
        let db = make_db();
        let cols = get_table_columns(&db, "invoices");

        for col in &[
            "id",
            "number",
            "customer_id",
            "customer_name",
            "issue_date",
            "due_date",
            "status",
            "subtotal",
            "tax_total",
            "total",
            "amount_paid",
            "balance_due",
            "currency",
            "line_items",
            "transaction_id",
            "created_at",
            "updated_at",
            "_dirty",
            "_modified_at",
        ] {
            assert!(cols.contains(*col), "invoices should have column '{}'", col);
        }
    }

    #[test]
    fn test_bills_table_columns() {
        let db = make_db();
        let cols = get_table_columns(&db, "bills");

        for col in &[
            "id",
            "number",
            "vendor_id",
            "vendor_name",
            "issue_date",
            "due_date",
            "status",
            "subtotal",
            "tax_total",
            "total",
            "amount_paid",
            "balance_due",
            "currency",
            "line_items",
            "transaction_id",
            "created_at",
            "updated_at",
            "_dirty",
            "_modified_at",
        ] {
            assert!(cols.contains(*col), "bills should have column '{}'", col);
        }
    }

    #[test]
    fn test_assets_table_columns() {
        let db = make_db();
        let cols = get_table_columns(&db, "assets");

        for col in &[
            "id",
            "name",
            "description",
            "asset_type",
            "account_id",
            "purchase_date",
            "purchase_cost",
            "salvage_value",
            "useful_life_months",
            "depreciation_method",
            "accumulated_depreciation",
            "current_value",
            "currency",
            "is_disposed",
            "disposal_date",
            "disposal_value",
            "created_at",
            "updated_at",
            "_dirty",
            "_modified_at",
        ] {
            assert!(cols.contains(*col), "assets should have column '{}'", col);
        }
    }

    #[test]
    fn test_dirty_and_modified_at_on_all_data_tables() {
        let db = make_db();

        let data_tables = [
            "accounts",
            "transactions",
            "journal_entries",
            "transaction_entries",
            "invoices",
            "bills",
            "assets",
        ];

        for table in &data_tables {
            let cols = get_table_columns(&db, table);
            assert!(
                cols.contains("_dirty"),
                "Table '{}' should have _dirty column",
                table
            );
            assert!(
                cols.contains("_modified_at"),
                "Table '{}' should have _modified_at column",
                table
            );
            assert!(
                cols.contains("created_at"),
                "Table '{}' should have created_at column",
                table
            );
            assert!(
                cols.contains("updated_at"),
                "Table '{}' should have updated_at column",
                table
            );
        }
    }

    #[test]
    fn test_infrastructure_tables_have_no_dirty_columns() {
        let db = make_db();

        let infra_tables = ["schema_version", "sync_state", "changes"];

        for table in &infra_tables {
            let cols = get_table_columns(&db, table);
            assert!(
                !cols.contains("_dirty"),
                "Infrastructure table '{}' should NOT have _dirty column",
                table
            );
            assert!(
                !cols.contains("_modified_at"),
                "Infrastructure table '{}' should NOT have _modified_at column",
                table
            );
        }
    }

    // ── CRUD / Query Tests ────────────────────────────────

    #[test]
    fn test_execute_insert_and_query() {
        let db = make_db();

        // Insert an account
        let account_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(
            "INSERT INTO accounts \
             (id, number, name, description, account_type, status, balance, currency, \
              is_bank_account, is_reconciled, created_at, updated_at, _dirty) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            rusqlite::params![
                account_id,
                "1000",
                "Cash",
                "Operating cash account",
                "Asset",
                "active",
                "0",
                "USD",
                0,
                0,
                now,
                now,
                1,
            ],
        )
        .expect("Failed to insert account");

        // Query it back
        let row = db
            .query_one(
                "SELECT id, number, name, account_type, _dirty FROM accounts WHERE number = ?1",
                rusqlite::params!["1000"],
            )
            .expect("Failed to query account");

        assert_eq!(row.get::<String, _>("id").unwrap(), account_id);
        assert_eq!(row.get::<String, _>("number").unwrap(), "1000");
        assert_eq!(row.get::<String, _>("name").unwrap(), "Cash");
        assert_eq!(row.get::<String, _>("account_type").unwrap(), "Asset");
        assert_eq!(row.get::<i64, _>("_dirty").unwrap(), 1);
    }

    #[test]
    fn test_query_one_returns_not_found() {
        let db = make_db();

        let result = db.query_one(
            "SELECT * FROM accounts WHERE number = ?1",
            rusqlite::params!["nonexistent"],
        );

        assert!(matches!(result, Err(LocalDbError::NotFound)));
    }

    #[test]
    fn test_query_all_returns_empty_vec() {
        let db = make_db();

        let rows = db
            .query_all(
                "SELECT * FROM accounts WHERE number = ?1",
                rusqlite::params!["nonexistent"],
            )
            .expect("query_all should not error on empty result");

        assert!(rows.is_empty());
    }

    #[test]
    fn test_query_all_multiple_rows() {
        let db = make_db();
        let now = chrono::Utc::now().to_rfc3339();

        // Insert multiple accounts
        for i in 1..=5 {
            db.execute(
                "INSERT INTO accounts \
                 (id, number, name, description, account_type, status, balance, currency, \
                  is_bank_account, is_reconciled, created_at, updated_at, _dirty) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                rusqlite::params![
                    uuid::Uuid::new_v4().to_string(),
                    format!("{}{}", i, "00"),
                    format!("Account {}", i),
                    "",
                    "Asset",
                    "active",
                    "0",
                    "USD",
                    0,
                    0,
                    now,
                    now,
                    0,
                ],
            )
            .expect("Failed to insert account");
        }

        let rows = db
            .query_all("SELECT * FROM accounts ORDER BY number", &[])
            .expect("Failed to query all accounts");

        assert_eq!(rows.len(), 5);
    }

    // ── Sync Helper Tests ─────────────────────────────────

    #[test]
    fn test_sync_state_singleton_exists() {
        let db = make_db();

        let row = db
            .query_one("SELECT * FROM sync_state WHERE id = 1", &[])
            .expect("sync_state singleton should exist");

        assert_eq!(row.get::<i64, _>("id").unwrap(), 1);
        assert!(row.get::<Option<String>, _>("last_sync").unwrap().is_none());
        assert_eq!(row.get::<i64, _>("sync_in_progress").unwrap(), 0);
    }

    #[test]
    fn test_get_and_set_last_sync() {
        let db = make_db();

        // Initially no sync has occurred
        assert!(db.get_last_sync().is_none());

        // Set a sync timestamp
        let ts = "2026-07-01T12:00:00Z";
        db.set_last_sync(ts).expect("Failed to set last sync");

        assert_eq!(db.get_last_sync().as_deref(), Some(ts));
    }

    #[test]
    fn test_record_change() {
        let db = make_db();

        db.record_change("account", "uuid-123", "insert")
            .expect("Failed to record change");
        db.record_change("transaction", "uuid-456", "update")
            .expect("Failed to record change");
        db.record_change("invoice", "uuid-789", "delete")
            .expect("Failed to record change");

        let rows = db
            .query_all("SELECT * FROM changes ORDER BY id", &[])
            .expect("Failed to query changes");

        assert_eq!(rows.len(), 3);

        let first = &rows[0];
        assert_eq!(first.get::<String, _>("entity_type").unwrap(), "account");
        assert_eq!(first.get::<String, _>("entity_id").unwrap(), "uuid-123");
        assert_eq!(first.get::<String, _>("operation").unwrap(), "insert");

        let third = &rows[2];
        assert_eq!(third.get::<String, _>("entity_type").unwrap(), "invoice");
        assert_eq!(third.get::<String, _>("operation").unwrap(), "delete");
    }

    // ── Row Type Tests ────────────────────────────────────

    #[test]
    fn test_row_get_by_name_and_index() {
        let db = make_db();
        let now = chrono::Utc::now().to_rfc3339();

        db.execute(
            "INSERT INTO accounts \
             (id, number, name, description, account_type, status, balance, currency, \
              is_bank_account, is_reconciled, created_at, updated_at, _dirty) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            rusqlite::params![
                "test-id",
                "5000",
                "Test Account",
                "",
                "Liability",
                "active",
                "100.50",
                "EUR",
                1,
                0,
                now,
                now,
                0,
            ],
        )
        .expect("Failed to insert");

        let row = db
            .query_one(
                "SELECT id, number, name, is_bank_account FROM accounts WHERE id = ?1",
                rusqlite::params!["test-id"],
            )
            .expect("Failed to query");

        // By name
        assert_eq!(row.get::<String, _>("id").unwrap(), "test-id");
        assert_eq!(row.get::<String, _>("number").unwrap(), "5000");
        assert_eq!(row.get::<i64, _>("is_bank_account").unwrap(), 1);

        // By index
        assert_eq!(row.get_by_index::<String>(0).unwrap(), "test-id");
        assert_eq!(row.get_by_index::<String>(1).unwrap(), "5000");
        assert_eq!(row.get_by_index::<i64>(3).unwrap(), 1);
    }

    #[test]
    fn test_row_column_not_found() {
        let row = Row::new(
            vec!["a".to_string(), "b".to_string()],
            vec![Value::Integer(1), Value::Integer(2)],
        );

        let result: Result<i64, _> = row.get("nonexistent");
        assert!(matches!(result, Err(LocalDbError::InvalidData(_))));
    }

    #[test]
    fn test_row_index_out_of_bounds() {
        let row = Row::new(
            vec!["a".to_string()],
            vec![Value::Integer(1)],
        );

        let result: Result<i64, _> = row.get_by_index(5);
        assert!(matches!(result, Err(LocalDbError::InvalidData(_))));
    }

    // ── Error Display Tests ───────────────────────────────

    #[test]
    fn test_error_display() {
        let e = LocalDbError::NotInitialized;
        assert_eq!(format!("{}", e), "Database not initialized");

        let e = LocalDbError::NotFound;
        assert_eq!(format!("{}", e), "Record not found");

        let e = LocalDbError::Migration("test migration error".to_string());
        assert_eq!(format!("{}", e), "Migration error: test migration error");

        let e = LocalDbError::InvalidData("bad value".to_string());
        assert_eq!(format!("{}", e), "Invalid data: bad value");

        let e = LocalDbError::PoisonedLock;
        assert_eq!(format!("{}", e), "Database lock poisoned");
    }
}
