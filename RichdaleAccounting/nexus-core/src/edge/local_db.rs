//! Edge Local Database
//!
//! Embedded SQLite database for offline-first operation.
//! Schema mirrors the SurrealDB tables for seamless sync.
//!
//! # Design
//!
//! - Uses `rusqlite` (bundled feature) for a synchronous, embedded SQLite connection
//! - `Arc<std::sync::Mutex<Connection>>` wraps the connection (SQLite operations
//!   are fast and blocking; `tokio::task::spawn_blocking` can be used for long queries)
//! - `LocalDb` derives `Clone` — cloning increments the `Arc` refcount, so all
//!   clones share the same underlying connection and mutex
//! - Decimal values stored as TEXT to preserve precision
//! - DateTime values stored as TEXT (RFC 3339)
//! - UUID values stored as TEXT
//! - JSON fields (entries, metadata, document_ids, bank_details) stored as TEXT
//! - Every data table includes `_dirty`, `_deleted`, and `_modified_at` columns
//!   for change tracking, plus `created_at` and `updated_at` timestamps
//! - Infrastructure tables (`schema_version`, `sync_state`, `changes`) do
//!   not carry `_dirty` / `_modified_at` since they are not synced entities
//! - Versioned SQL migrations applied on first connect
//! - `PRAGMA user_version` tracks schema version alongside the `schema_version` table
//!
//! # Schema Overview
//!
//! | Table                 | Purpose                                           |
//! |-----------------------|---------------------------------------------------|
//! | `accounts`            | Chart of accounts (mirrors SurrealDB `account`)   |
//! | `transactions`        | Financial transactions (mirrors `transaction`)    |
//! | `transaction_entries` | Double-entry legs (mirrors `transaction_entry`)   |
//! | `journal_entries`     | Journal entries (mirrors `journal_entry`)          |
//! | `invoices`            | Customer invoices                                 |
//! | `bills`               | Vendor bills                                      |
//! | `assets`              | Fixed assets                                      |
//! | `sync_state`          | Singleton row tracking last sync                  |
//! | `changes`             | Audit log of local changes for sync               |
//! | `encryption_keys`     | Wrapped DEK + salt for field-level encryption     |
//! | `conflicts`           | Sync conflict records for resolution              |

use std::sync::{Arc, Mutex};
use rusqlite::{Connection, OpenFlags, ToSql};
use rusqlite::types::{Value, ValueRef, FromSql};
use thiserror::Error;
use tracing::{info, debug, error, warn};
use chrono::Utc;

// ═══════════════════════════════════════════════════════════
// Error Type
// ═══════════════════════════════════════════════════════════

/// Errors that can occur during local database operations.
///
/// The four primary variants (`ConnectionError`, `MigrationError`,
/// `QueryError`, `NotFound`) cover the main failure modes.  Additional
/// variants exist for backward compatibility with modules that reference
/// `InvalidData` and `PoisonedLock`.
#[derive(Error, Debug)]
pub enum LocalDbError {
    /// Failed to open or connect to the SQLite database.
    #[error("Connection error: {0}")]
    ConnectionError(String),

    /// A schema migration failed to apply.
    #[error("Migration error: {0}")]
    MigrationError(String),

    /// A SQL query failed to execute or returned unexpected data.
    #[error("Query error: {0}")]
    QueryError(String),

    /// A query expected exactly one row but found none.
    #[error("Record not found")]
    NotFound,

    // ── Backward-compatible variants ─────────────────────

    /// Raw SQLite error (auto-converted via `?`).
    ///
    /// Kept so that `From<rusqlite::Error>` is available for `?` syntax
    /// in methods that call `rusqlite` directly.
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// Data could not be converted to or from a SQLite value.
    ///
    /// Used by `Row::get` and by `tracking::parse_dt`.
    #[error("Invalid data: {0}")]
    InvalidData(String),

    /// Filesystem I/O error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// The mutex guarding the connection was poisoned.
    #[error("Database lock poisoned")]
    PoisonedLock,

    /// Database has not been initialized.
    #[error("Database not initialized")]
    NotInitialized,
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
/// transactions, etc.) carry `_dirty` / `_deleted` / `_modified_at` /
/// `_remote_updated_at` columns for change tracking; infrastructure tables
/// (schema_version, sync_state, changes) do not, since they are not synced
/// entities.
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
    last_successful_sync TEXT,
    last_sync_attempt TEXT,
    sync_in_progress INTEGER NOT NULL DEFAULT 0,
    pending_changes INTEGER NOT NULL DEFAULT 0
);

INSERT OR IGNORE INTO sync_state (id, last_sync, last_successful_sync, last_sync_attempt, sync_in_progress, pending_changes)
VALUES (1, NULL, NULL, NULL, 0, 0);

CREATE TABLE IF NOT EXISTS changes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    operation TEXT NOT NULL CHECK (operation IN ('insert', 'update', 'delete')),
    timestamp TEXT NOT NULL,
    dirty INTEGER NOT NULL DEFAULT 0,
    synced INTEGER NOT NULL DEFAULT 0,
    synced_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_changes_entity ON changes(entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_changes_timestamp ON changes(timestamp);
CREATE INDEX IF NOT EXISTS idx_changes_dirty ON changes(dirty);
CREATE INDEX IF NOT EXISTS idx_changes_synced ON changes(synced);

-- ─── Encryption Keys ───

CREATE TABLE IF NOT EXISTS encryption_keys (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    wrapped_dek BLOB,
    salt BLOB,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

INSERT OR IGNORE INTO encryption_keys (id, wrapped_dek, salt, created_at, updated_at)
VALUES (1, NULL, NULL, '', '');

-- ─── Conflicts ───

CREATE TABLE IF NOT EXISTS conflicts (
    id TEXT PRIMARY KEY,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    local_version TEXT,
    remote_version TEXT,
    diff_fields TEXT,
    local_modified_at TEXT,
    remote_modified_at TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    resolution TEXT,
    resolved_by TEXT,
    resolved_at TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_conflicts_entity ON conflicts(entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_conflicts_status ON conflicts(status);

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
    _deleted INTEGER NOT NULL DEFAULT 0,
    _modified_at TEXT,
    _remote_updated_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_accounts_number ON accounts(number);
CREATE INDEX IF NOT EXISTS idx_accounts_type ON accounts(account_type);
CREATE INDEX IF NOT EXISTS idx_accounts_parent ON accounts(parent_id);
CREATE INDEX IF NOT EXISTS idx_accounts_status ON accounts(status);
CREATE INDEX IF NOT EXISTS idx_accounts_dirty ON accounts(_dirty);
CREATE INDEX IF NOT EXISTS idx_accounts_deleted ON accounts(_deleted);

-- Transaction entries — mirrors SurrealDB 'transaction_entry' table
CREATE TABLE IF NOT EXISTS transaction_entries (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id),
    transaction_id TEXT REFERENCES transactions(id),
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
    _deleted INTEGER NOT NULL DEFAULT 0,
    _modified_at TEXT,
    _remote_updated_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_txn_number ON transactions(number);
CREATE INDEX IF NOT EXISTS idx_txn_date ON transactions(date);
CREATE INDEX IF NOT EXISTS idx_txn_status ON transactions(status);
CREATE INDEX IF NOT EXISTS idx_txn_type ON transactions(transaction_type);
CREATE INDEX IF NOT EXISTS idx_txn_dirty ON transactions(_dirty);
CREATE INDEX IF NOT EXISTS idx_txn_deleted ON transactions(_deleted);

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
                      transaction_entries, invoices, bills, assets, sync_state, changes, \
                      encryption_keys, conflicts",
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
/// Wraps a single `rusqlite::Connection` behind an
/// `Arc<std::sync::Mutex<Connection>>`.  The schema mirrors the remote
/// SurrealDB tables so that the sync engine can push and pull records with
/// minimal transformation.
///
/// # Concurrency
///
/// `LocalDb` is `Send + Sync + Clone`.  Cloning is cheap — it increments
/// the `Arc` refcount, so all clones share the same underlying connection
/// and mutex.  For async contexts, wrap long-running queries in
/// `tokio::task::spawn_blocking` to avoid blocking the runtime.
///
/// # Why `std::sync::Mutex` (not `tokio::sync::Mutex`)
///
/// SQLite operations are inherently synchronous.  Using `std::sync::Mutex`
/// keeps the API synchronous, avoids async overhead, and aligns with the
/// tokio team's guidance for guarding short-lived, synchronous resources.
#[derive(Debug, Clone)]
pub struct LocalDb {
    conn: Arc<Mutex<Connection>>,
    path: String,
}

impl LocalDb {
    // ── Construction ──────────────────────────────────────

    /// Open (or create) a SQLite database file at `path` and run migrations.
    ///
    /// Enables WAL journal mode, foreign-key enforcement, and
    /// `synchronous = NORMAL` for a good balance of durability and
    /// performance on local storage.
    ///
    /// # Errors
    /// - [`LocalDbError::ConnectionError`] if the file cannot be opened.
    /// - [`LocalDbError::MigrationError`] if a migration fails.
    pub fn open(path: &str) -> Result<Self, LocalDbError> {
        // Ensure the parent directory exists
        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent)?;
                debug!("Created parent directory for database: {:?}", parent);
            }
        }

        let conn = Connection::open(path)
            .map_err(|e| LocalDbError::ConnectionError(e.to_string()))?;

        // Pragmas — WAL mode, foreign keys, normal sync
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             PRAGMA synchronous = NORMAL;",
        )?;

        info!("Opened local database at: {}", path);

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
            path: path.to_string(),
        };

        // Apply any pending migrations
        db.run_migrations()?;

        Ok(db)
    }

    /// Create an in-memory SQLite database (for testing).
    ///
    /// Uses a shared-cache URI (`file:edge_mem_<uuid>?mode=memory&cache=shared`)
    /// so that [`get_connection`](Self::get_connection) can open additional
    /// connections to the same in-memory database.  Runs migrations so the
    /// schema is ready for immediate use.
    pub fn open_in_memory() -> Result<Self, LocalDbError> {
        let mem_uri = format!(
            "file:edge_mem_{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4()
        );

        let conn = Connection::open_with_flags(
            &mem_uri,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_URI
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| LocalDbError::ConnectionError(e.to_string()))?;

        conn.execute_batch("PRAGMA foreign_keys = ON;")?;

        debug!("Opened in-memory local database");

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
            path: mem_uri,
        };

        db.run_migrations()?;

        Ok(db)
    }

    // ── Migrations ────────────────────────────────────────

    /// Apply all pending schema migrations.
    ///
    /// Idempotent: calling this on an up-to-date database is a no-op.
    /// Each migration is wrapped in a transaction for atomicity.
    /// After applying, `PRAGMA user_version` is updated to match.
    ///
    /// This method is called automatically by [`open`](Self::open) and
    /// [`open_in_memory`](Self::open_in_memory).
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

        // Read the highest applied version from the schema_version table
        let current_version: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        // Also check PRAGMA user_version for consistency
        let pragma_version: i32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap_or(0);

        // Use the higher of the two as the effective version
        let effective_version = current_version.max(pragma_version);

        debug!(
            "Current schema version: {} (table={}, pragma={}), latest: {}",
            effective_version, current_version, pragma_version, LATEST_SCHEMA_VERSION
        );

        if effective_version > LATEST_SCHEMA_VERSION {
            warn!(
                "Database schema version {} is higher than latest known version {} — \
                 possible downgrade",
                effective_version, LATEST_SCHEMA_VERSION
            );
        }

        if effective_version >= LATEST_SCHEMA_VERSION {
            debug!("Database schema is up to date");
            // Ensure PRAGMA user_version is in sync
            conn.execute_batch(&format!(
                "PRAGMA user_version = {}",
                LATEST_SCHEMA_VERSION
            ))
            .map_err(|e| {
                LocalDbError::MigrationError(format!(
                    "Failed to set PRAGMA user_version: {}",
                    e
                ))
            })?;
            return Ok(());
        }

        // Apply each pending migration in order
        for migration in MIGRATIONS {
            if migration.version > effective_version {
                info!(
                    "Applying migration v{}: {}",
                    migration.version, migration.description
                );

                let tx = conn.transaction()?;
                tx.execute_batch(migration.sql).map_err(|e| {
                    LocalDbError::MigrationError(format!(
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

                // Update PRAGMA user_version within the same transaction
                tx.execute_batch(&format!(
                    "PRAGMA user_version = {}",
                    migration.version
                ))
                .map_err(|e| {
                    LocalDbError::MigrationError(format!(
                        "Failed to set PRAGMA user_version to {}: {}",
                        migration.version, e
                    ))
                })?;

                tx.commit()?;

                info!("Migration v{} applied successfully", migration.version);
            }
        }

        debug!("All migrations complete");
        Ok(())
    }

    /// Run pending migrations (alias for [`run_migrations`](Self::run_migrations)).
    ///
    /// Provided for API symmetry with the task specification.  Internally
    /// delegates to `run_migrations`, which uses both the `schema_version`
    /// table and `PRAGMA user_version` for version tracking.
    pub fn migrate(&self) -> Result<(), LocalDbError> {
        self.run_migrations()
    }

    /// Return the highest applied schema version, or 0 if none.
    ///
    /// Reads from `PRAGMA user_version`, which is kept in sync with the
    /// `schema_version` table during migration.
    pub fn get_schema_version(&self) -> i32 {
        let conn = match self.conn.lock() {
            Ok(guard) => guard,
            Err(_) => {
                error!("Failed to acquire database lock for schema version check");
                return 0;
            }
        };

        conn.query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap_or(0)
    }

    /// Record that schema version `version` has been applied.
    ///
    /// Uses `INSERT OR IGNORE` so re-applying the same version is a no-op.
    /// Also updates `PRAGMA user_version` to stay in sync.
    pub fn set_schema_version(&self, version: i32) -> Result<(), LocalDbError> {
        let conn = self.conn.lock().map_err(|_| LocalDbError::PoisonedLock)?;

        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT OR IGNORE INTO schema_version \
             (version, applied_at, description) VALUES (?1, ?2, ?3)",
            rusqlite::params![version, now, format!("Schema version {}", version)],
        )?;

        // Keep PRAGMA user_version in sync
        conn.execute_batch(&format!("PRAGMA user_version = {}", version))
            .map_err(|e| {
                LocalDbError::MigrationError(format!(
                    "Failed to set PRAGMA user_version: {}",
                    e
                ))
            })?;

        debug!("Set schema version to {}", version);
        Ok(())
    }

    // ── Connection Access ─────────────────────────────────

    /// Open a **new** `Connection` to the same underlying database.
    ///
    /// For file-based databases, this opens a second connection to the same
    /// file with WAL and foreign keys enabled.  For in-memory databases
    /// (opened via [`open_in_memory`](Self::open_in_memory)), this opens a
    /// new connection to the same shared-cache in-memory database.
    ///
    /// The returned connection is independent of the internal
    /// `Arc<Mutex<Connection>>` — the caller owns it and is responsible for
    /// running any necessary pragmas or migrations on it (the base pragmas
    /// are already applied).
    ///
    /// # Errors
    /// - [`LocalDbError::ConnectionError`] if the connection cannot be opened.
    pub fn get_connection(&self) -> Result<Connection, LocalDbError> {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;

        // For in-memory databases (shared-cache URI), include SQLITE_OPEN_URI
        let conn = if self.path.starts_with("file:") {
            Connection::open_with_flags(
                &self.path,
                flags | OpenFlags::SQLITE_OPEN_URI,
            )
        } else {
            Connection::open_with_flags(&self.path, flags)
        }
        .map_err(|e| LocalDbError::ConnectionError(e.to_string()))?;

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             PRAGMA synchronous = NORMAL;",
        )
        .map_err(|e| LocalDbError::ConnectionError(e.to_string()))?;

        Ok(conn)
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
    /// Returns [`LocalDbError::NotFound`] if no rows match.
    pub fn query_one(&self, sql: &str, params: &[&dyn ToSql]) -> Result<Row, LocalDbError> {
        let conn = self.conn.lock().map_err(|_| LocalDbError::PoisonedLock)?;

        let mut stmt = conn.prepare(sql)?;
        let col_count = stmt.column_count();
        let col_names: Vec<String> = (0..col_count)
            .map(|i| stmt.column_name(i).map(|s| s.to_string()).unwrap_or_else(|_| format!("col_{}", i)))
            .collect();

        let mut rows = stmt.query(params)?;
        match rows.next()? {
            Some(row) => {
                let mut values = Vec::with_capacity(col_count);
                for i in 0..col_count {
                    let value: Value = row.get(i)?;
                    values.push(value);
                }
                Ok(Row { columns: col_names, values })
            }
            None => Err(LocalDbError::NotFound),
        }
    }

    /// Query for all matching rows.
    ///
    /// Returns an empty `Vec` if no rows match.
    pub fn query_all(&self, sql: &str, params: &[&dyn ToSql]) -> Result<Vec<Row>, LocalDbError> {
        let conn = self.conn.lock().map_err(|_| LocalDbError::PoisonedLock)?;
        let mut stmt = conn.prepare(sql)?;
        let col_count = stmt.column_count();
        let col_names: Vec<String> = (0..col_count)
            .map(|i| stmt.column_name(i).map(|s| s.to_string()).unwrap_or_else(|_| format!("col_{}", i)))
            .collect();

        let mut rows = stmt.query(params)?;
        let mut result = Vec::new();
        while let Some(row) = rows.next()? {
            let mut values = Vec::with_capacity(col_count);
            for i in 0..col_count {
                let value: Value = row.get(i)?;
                values.push(value);
            }
            result.push(Row { columns: col_names.clone(), values });
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
            "UPDATE sync_state SET last_sync = ?1, last_successful_sync = ?1 WHERE id = 1",
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
            "INSERT INTO changes (entity_type, entity_id, operation, timestamp, dirty) \
             VALUES (?1, ?2, ?3, ?4, 1)",
            rusqlite::params![entity_type, entity_id, operation, now],
        )?;
        debug!("Recorded change: {} {} {}", operation, entity_type, entity_id);
        Ok(())
    }

    // ── Accessors ─────────────────────────────────────────

    /// The filesystem path (or shared-cache URI) this database was opened with.
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
            .filter_map(|row| row.get::<String>("name").ok())
            .collect()
    }

    /// Get the column names for a table via `PRAGMA table_info`.
    fn get_table_columns(db: &LocalDb, table: &str) -> HashSet<String> {
        let sql = format!("PRAGMA table_info({})", table);
        let rows = db
            .query_all(&sql, &[])
            .unwrap_or_else(|_| panic!("Failed to query columns for table '{}'", table));
        rows.iter()
            .filter_map(|row| row.get::<String>("name").ok())
            .collect()
    }

    /// Collect all index names from `sqlite_master`.
    fn get_index_names(db: &LocalDb) -> HashSet<String> {
        let rows = db
            .query_all(
                "SELECT name FROM sqlite_master WHERE type = 'index' ORDER BY name",
                &[],
            )
            .expect("Failed to query index names");
        rows.iter()
            .filter_map(|row| row.get::<String>("name").ok())
            .collect()
    }

    /// Get the value of a PRAGMA setting as a string.
    fn get_pragma(db: &LocalDb, pragma: &str) -> String {
        let sql = format!("PRAGMA {}", pragma);
        let row = db
            .query_one(&sql, &[])
            .unwrap_or_else(|_| panic!("Failed to query PRAGMA {}", pragma));
        // PRAGMA results have a single unnamed column; use index 0.
        row.get_by_index::<String>(0)
            .unwrap_or_else(|_| {
                // Some PRAGMAs return integers
                row.get_by_index::<i64>(0)
                    .map(|i| i.to_string())
                    .unwrap_or_else(|_| "unknown".to_string())
            })
    }

    // ── Open Tests ────────────────────────────────────────

    #[test]
    fn test_open_creates_database_file() {
        let dir = tempfile::tempdir().expect("Failed to create temp dir");
        let db_path = dir.path().join("test_edge.db");
        let path_str = db_path.to_str().expect("Path not valid UTF-8");

        assert!(!db_path.exists(), "Database file should not exist before open");

        let db = LocalDb::open(path_str).expect("Failed to open database");

        assert!(db_path.exists(), "Database file should exist after open");
        assert!(
            db.get_schema_version() >= 1,
            "Schema version should be >= 1 after open"
        );
    }

    #[test]
    fn test_open_in_memory() {
        let db = LocalDb::open_in_memory().expect("Failed to open in-memory database");
        assert!(
            db.path().starts_with("file:edge_mem_"),
            "In-memory path should be a shared-cache URI, got: {}",
            db.path()
        );
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
        for table in &[
            "schema_version",
            "sync_state",
            "changes",
            "encryption_keys",
            "conflicts",
        ] {
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

        // set_schema_version should insert a new version record and update PRAGMA
        db.set_schema_version(99).expect("Failed to set schema version");
        assert_eq!(
            db.get_schema_version(),
            99,
            "get_schema_version should return max version from PRAGMA user_version"
        );

        // Re-setting the same version is a no-op (INSERT OR IGNORE)
        db.set_schema_version(99).expect("Failed to re-set schema version");
        assert_eq!(db.get_schema_version(), 99);
    }

    #[test]
    fn test_pragma_user_version_set() {
        let db = make_db();

        // PRAGMA user_version should match LATEST_SCHEMA_VERSION after migrations
        let user_version = get_pragma(&db, "user_version");
        assert_eq!(
            user_version.parse::<i32>().unwrap_or(0),
            LATEST_SCHEMA_VERSION,
            "PRAGMA user_version should be {} after migrations",
            LATEST_SCHEMA_VERSION
        );
    }

    #[test]
    fn test_rerunning_migrations_is_idempotent() {
        let db = make_db();
        let version_before = db.get_schema_version();

        db.run_migrations().expect("Re-running migrations should succeed");

        let version_after = db.get_schema_version();
        assert_eq!(
            version_before, version_after,
            "Schema version should not change"
        );

        let tables_before = get_table_names(&db);
        db.run_migrations().expect("Third migration run should succeed");
        let tables_after = get_table_names(&db);
        assert_eq!(tables_before.len(), tables_after.len());
    }

    #[test]
    fn test_migrate_alias_works() {
        let db = make_db();
        let version_before = db.get_schema_version();

        // migrate() should be equivalent to run_migrations()
        db.migrate().expect("migrate() should succeed");

        assert_eq!(
            db.get_schema_version(),
            version_before,
            "Schema version should not change after re-running migrate()"
        );
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

        // Sync tracking columns including new _deleted and _remote_updated_at
        for col in &[
            "created_at",
            "updated_at",
            "_dirty",
            "_deleted",
            "_modified_at",
            "_remote_updated_at",
        ] {
            assert!(
                cols.contains(*col),
                "accounts should have sync column '{}'",
                col
            );
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
            "_deleted",
            "_modified_at",
            "_remote_updated_at",
        ] {
            assert!(
                cols.contains(*col),
                "transactions should have column '{}'",
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
    fn test_encryption_keys_table_columns() {
        let db = make_db();
        let cols = get_table_columns(&db, "encryption_keys");

        for col in &["id", "wrapped_dek", "salt", "created_at", "updated_at"] {
            assert!(
                cols.contains(*col),
                "encryption_keys should have column '{}'",
                col
            );
        }
    }

    #[test]
    fn test_conflicts_table_columns() {
        let db = make_db();
        let cols = get_table_columns(&db, "conflicts");

        for col in &[
            "id",
            "entity_type",
            "entity_id",
            "local_version",
            "remote_version",
            "diff_fields",
            "local_modified_at",
            "remote_modified_at",
            "status",
            "resolution",
            "resolved_by",
            "resolved_at",
            "created_at",
        ] {
            assert!(
                cols.contains(*col),
                "conflicts should have column '{}'",
                col
            );
        }
    }

    #[test]
    fn test_sync_state_table_columns() {
        let db = make_db();
        let cols = get_table_columns(&db, "sync_state");

        for col in &[
            "id",
            "last_sync",
            "last_successful_sync",
            "last_sync_attempt",
            "sync_in_progress",
            "pending_changes",
        ] {
            assert!(
                cols.contains(*col),
                "sync_state should have column '{}'",
                col
            );
        }
    }

    #[test]
    fn test_changes_table_columns() {
        let db = make_db();
        let cols = get_table_columns(&db, "changes");

        for col in &[
            "id",
            "entity_type",
            "entity_id",
            "operation",
            "timestamp",
            "dirty",
            "synced",
            "synced_at",
        ] {
            assert!(
                cols.contains(*col),
                "changes should have column '{}'",
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
    fn test_deleted_and_remote_updated_at_on_synced_tables() {
        let db = make_db();

        // accounts and transactions should have _deleted and _remote_updated_at
        for table in &["accounts", "transactions"] {
            let cols = get_table_columns(&db, table);
            assert!(
                cols.contains("_deleted"),
                "Table '{}' should have _deleted column",
                table
            );
            assert!(
                cols.contains("_remote_updated_at"),
                "Table '{}' should have _remote_updated_at column",
                table
            );
        }
    }

    #[test]
    fn test_infrastructure_tables_have_no_dirty_columns() {
        let db = make_db();

        let infra_tables = [
            "schema_version",
            "sync_state",
            "changes",
            "encryption_keys",
            "conflicts",
        ];

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

    // ── Index Verification Tests ──────────────────────────

    #[test]
    fn test_indices_exist() {
        let db = make_db();
        let indices = get_index_names(&db);

        // Required indices from the task specification
        let required_indices = [
            // _dirty indices
            "idx_accounts_dirty",
            "idx_txn_dirty",
            "idx_je_dirty",
            "idx_te_dirty",
            "idx_inv_dirty",
            "idx_bill_dirty",
            "idx_asset_dirty",
            // date indices
            "idx_txn_date",
            "idx_je_date",
            "idx_inv_due_date",
            "idx_bill_due_date",
            // account_id index
            "idx_te_account",
            // transaction_id index
            "idx_te_transaction",
            // entity_type + entity_id index
            "idx_changes_entity",
            "idx_conflicts_entity",
            // status indices
            "idx_accounts_status",
            "idx_txn_status",
            "idx_inv_status",
            "idx_bill_status",
            "idx_conflicts_status",
        ];

        for idx in &required_indices {
            assert!(
                indices.contains(*idx),
                "Index '{}' should exist",
                idx
            );
        }
    }

    // ── PRAGMA Verification Tests ─────────────────────────

    #[test]
    fn test_wal_journal_mode() {
        let dir = tempfile::tempdir().expect("Failed to create temp dir");
        let db_path = dir.path().join("test_wal.db");
        let path_str = db_path.to_str().expect("Path not valid UTF-8");

        let db = LocalDb::open(path_str).expect("Failed to open database");

        let journal_mode = get_pragma(&db, "journal_mode");
        assert_eq!(
            journal_mode.to_lowercase(),
            "wal",
            "journal_mode should be WAL, got: {}",
            journal_mode
        );
    }

    #[test]
    fn test_foreign_keys_enabled() {
        let db = make_db();
        let fk = get_pragma(&db, "foreign_keys");
        assert_eq!(
            fk, "1",
            "foreign_keys should be ON (1), got: {}",
            fk
        );
    }

    // ── FK Enforcement Tests ──────────────────────────────

    #[test]
    fn test_fk_enforcement_insert_with_nonexistent_account_fails() {
        let db = make_db();

        // Attempt to insert a transaction_entry with an account_id that
        // does not exist in the accounts table.  With foreign_keys = ON,
        // this should fail.
        let now = Utc::now().to_rfc3339();
        let result = db.execute(
            "INSERT INTO transaction_entries \
             (id, account_id, transaction_id, journal_entry_id, entry_type, amount, \
              description, reference, currency, exchange_rate, base_currency_amount, \
              created_at, updated_at, _dirty, _modified_at) \
             VALUES (?1, ?2, NULL, NULL, ?3, ?4, ?5, NULL, ?6, NULL, NULL, ?7, ?8, 0, ?9)",
            rusqlite::params![
                "te-fk-test",
                "nonexistent-account-id",
                "Debit",
                "100",
                "Test FK",
                "USD",
                now,
                now,
                now,
            ],
        );

        assert!(
            result.is_err(),
            "Inserting a transaction_entry with a non-existent account_id should fail with FK enforcement"
        );
    }

    #[test]
    fn test_fk_enforcement_insert_with_valid_account_succeeds() {
        let db = make_db();
        let now = Utc::now().to_rfc3339();

        // First, create an account
        db.execute(
            "INSERT INTO accounts \
             (id, number, name, description, account_type, status, balance, currency, \
              is_bank_account, is_reconciled, created_at, updated_at, _dirty, _deleted) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, 0, ?9, ?10, 0, 0)",
            rusqlite::params![
                "acc-fk-valid",
                "9999",
                "FK Test Account",
                "",
                "Asset",
                "active",
                "0",
                "USD",
                now,
                now,
            ],
        )
        .expect("Failed to insert account");

        // Now insert a transaction_entry referencing that account
        let result = db.execute(
            "INSERT INTO transaction_entries \
             (id, account_id, transaction_id, journal_entry_id, entry_type, amount, \
              description, reference, currency, exchange_rate, base_currency_amount, \
              created_at, updated_at, _dirty, _modified_at) \
             VALUES (?1, ?2, NULL, NULL, ?3, ?4, ?5, NULL, ?6, NULL, NULL, ?7, ?8, 0, ?9)",
            rusqlite::params![
                "te-fk-valid",
                "acc-fk-valid",
                "Debit",
                "100",
                "Test FK valid",
                "USD",
                now,
                now,
                now,
            ],
        );

        assert!(
            result.is_ok(),
            "Inserting a transaction_entry with a valid account_id should succeed"
        );
    }

    #[test]
    fn test_fk_allows_null_transaction_id() {
        let db = make_db();
        let now = Utc::now().to_rfc3339();

        // Create an account first
        db.execute(
            "INSERT INTO accounts \
             (id, number, name, description, account_type, status, balance, currency, \
              is_bank_account, is_reconciled, created_at, updated_at, _dirty, _deleted) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, 0, ?9, ?10, 0, 0)",
            rusqlite::params![
                "acc-null-fk",
                "8888",
                "Null FK Test",
                "",
                "Asset",
                "active",
                "0",
                "USD",
                now,
                now,
            ],
        )
        .expect("Failed to insert account");

        // Insert a transaction_entry with NULL transaction_id (should succeed)
        let result = db.execute(
            "INSERT INTO transaction_entries \
             (id, account_id, transaction_id, journal_entry_id, entry_type, amount, \
              description, reference, currency, exchange_rate, base_currency_amount, \
              created_at, updated_at, _dirty, _modified_at) \
             VALUES (?1, ?2, NULL, NULL, ?3, ?4, ?5, NULL, ?6, NULL, NULL, ?7, ?8, 0, ?9)",
            rusqlite::params![
                "te-null-fk",
                "acc-null-fk",
                "Debit",
                "50",
                "Null FK",
                "USD",
                now,
                now,
                now,
            ],
        );

        assert!(
            result.is_ok(),
            "Inserting a transaction_entry with NULL transaction_id should succeed (FK allows NULL)"
        );
    }

    // ── CRUD / Query Tests ────────────────────────────────

    #[test]
    fn test_execute_insert_and_query() {
        let db = make_db();

        let account_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        db.execute(
            "INSERT INTO accounts \
             (id, number, name, description, account_type, status, balance, currency, \
              is_bank_account, is_reconciled, created_at, updated_at, _dirty, _deleted) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
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
                0,
            ],
        )
        .expect("Failed to insert account");

        let row = db
            .query_one(
                "SELECT id, number, name, account_type, _dirty, _deleted FROM accounts WHERE number = ?1",
                rusqlite::params!["1000"],
            )
            .expect("Failed to query account");

        assert_eq!(row.get::<String>("id").unwrap(), account_id);
        assert_eq!(row.get::<String>("number").unwrap(), "1000");
        assert_eq!(row.get::<String>("name").unwrap(), "Cash");
        assert_eq!(row.get::<String>("account_type").unwrap(), "Asset");
        assert_eq!(row.get::<i64>("_dirty").unwrap(), 1);
        assert_eq!(row.get::<i64>("_deleted").unwrap(), 0);
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
        let now = Utc::now().to_rfc3339();

        for i in 1..=5 {
            db.execute(
                "INSERT INTO accounts \
                 (id, number, name, description, account_type, status, balance, currency, \
                  is_bank_account, is_reconciled, created_at, updated_at, _dirty, _deleted) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
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

        assert_eq!(row.get::<i64>("id").unwrap(), 1);
        assert!(row.get::<Option<String>>("last_sync").unwrap().is_none());
        assert!(row
            .get::<Option<String>>("last_successful_sync")
            .unwrap()
            .is_none());
        assert_eq!(row.get::<i64>("sync_in_progress").unwrap(), 0);
    }

    #[test]
    fn test_encryption_keys_singleton_exists() {
        let db = make_db();

        let row = db
            .query_one("SELECT * FROM encryption_keys WHERE id = 1", &[])
            .expect("encryption_keys singleton should exist");

        assert_eq!(row.get::<i64>("id").unwrap(), 1);
        assert!(row.get::<Option<Vec<u8>>>("wrapped_dek").unwrap().is_none());
        assert!(row.get::<Option<Vec<u8>>>("salt").unwrap().is_none());
    }

    #[test]
    fn test_get_and_set_last_sync() {
        let db = make_db();

        assert!(db.get_last_sync().is_none());

        let ts = "2026-07-01T12:00:00Z";
        db.set_last_sync(ts).expect("Failed to set last sync");

        assert_eq!(db.get_last_sync().as_deref(), Some(ts));

        // last_successful_sync should also be updated
        let row = db
            .query_one("SELECT last_successful_sync FROM sync_state WHERE id = 1", &[])
            .expect("Failed to query last_successful_sync");
        assert_eq!(
            row.get::<Option<String>>("last_successful_sync").unwrap().as_deref(),
            Some(ts)
        );
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
        assert_eq!(first.get::<String>("entity_type").unwrap(), "account");
        assert_eq!(first.get::<String>("entity_id").unwrap(), "uuid-123");
        assert_eq!(first.get::<String>("operation").unwrap(), "insert");
        // dirty should be 1 (set by record_change)
        assert_eq!(first.get::<i64>("dirty").unwrap(), 1);

        let third = &rows[2];
        assert_eq!(third.get::<String>("entity_type").unwrap(), "invoice");
        assert_eq!(third.get::<String>("operation").unwrap(), "delete");
    }

    // ── Row Type Tests ────────────────────────────────────

    #[test]
    fn test_row_get_by_name_and_index() {
        let db = make_db();
        let now = Utc::now().to_rfc3339();

        db.execute(
            "INSERT INTO accounts \
             (id, number, name, description, account_type, status, balance, currency, \
              is_bank_account, is_reconciled, created_at, updated_at, _dirty, _deleted) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
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

        assert_eq!(row.get::<String>("id").unwrap(), "test-id");
        assert_eq!(row.get::<String>("number").unwrap(), "5000");
        assert_eq!(row.get::<i64>("is_bank_account").unwrap(), 1);

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

    #[test]
    fn test_row_default() {
        let row = Row::default();
        assert!(row.is_empty());
        assert_eq!(row.column_count(), 0);
        assert!(row.columns().is_empty());
    }

    #[test]
    fn test_row_clone() {
        let row = Row::new(
            vec!["col".to_string()],
            vec![Value::Text("value".to_string())],
        );
        let cloned = row.clone();
        assert_eq!(cloned.get::<String>("col").unwrap(), "value");
    }

    // ── get_connection Tests ──────────────────────────────

    #[test]
    fn test_get_connection_file_based() {
        let dir = tempfile::tempdir().expect("Failed to create temp dir");
        let db_path = dir.path().join("test_gc.db");
        let path_str = db_path.to_str().expect("Path not valid UTF-8");

        let db = LocalDb::open(path_str).expect("Failed to open database");

        // Insert data via the main connection
        let now = Utc::now().to_rfc3339();
        db.execute(
            "INSERT INTO accounts \
             (id, number, name, description, account_type, status, balance, currency, \
              is_bank_account, is_reconciled, created_at, updated_at, _dirty, _deleted) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            rusqlite::params![
                "gc-test-id",
                "7777",
                "GC Test",
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
                0,
            ],
        )
        .expect("Failed to insert via main connection");

        // Open a new connection and verify the data is visible
        let conn = db.get_connection().expect("Failed to get connection");

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM accounts WHERE number = ?1",
                rusqlite::params!["7777"],
                |row| row.get(0),
            )
            .expect("Failed to query via new connection");

        assert_eq!(count, 1, "New connection should see data from the main connection");
    }

    #[test]
    fn test_get_connection_in_memory() {
        let db = make_db();

        // Insert data via the main connection
        let now = Utc::now().to_rfc3339();
        db.execute(
            "INSERT INTO accounts \
             (id, number, name, description, account_type, status, balance, currency, \
              is_bank_account, is_reconciled, created_at, updated_at, _dirty, _deleted) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            rusqlite::params![
                "mem-gc-id",
                "6666",
                "Mem GC Test",
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
                0,
            ],
        )
        .expect("Failed to insert via main connection");

        // Open a new connection — for in-memory shared-cache, it should see the same data
        let conn = db.get_connection().expect("Failed to get connection");

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM accounts WHERE number = ?1",
                rusqlite::params!["6666"],
                |row| row.get(0),
            )
            .expect("Failed to query via new connection");

        assert_eq!(
            count, 1,
            "New in-memory connection should see data from the main connection (shared cache)"
        );
    }

    // ── Clone Tests ───────────────────────────────────────

    #[test]
    fn test_clone_shares_connection() {
        let db = make_db();
        let now = Utc::now().to_rfc3339();

        // Insert via the original
        db.execute(
            "INSERT INTO accounts \
             (id, number, name, description, account_type, status, balance, currency, \
              is_bank_account, is_reconciled, created_at, updated_at, _dirty, _deleted) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            rusqlite::params![
                "clone-test-id",
                "5555",
                "Clone Test",
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
                0,
            ],
        )
        .expect("Failed to insert via original");

        // Clone and query via the clone
        let db_clone = db.clone();
        let row = db_clone
            .query_one(
                "SELECT number FROM accounts WHERE id = ?1",
                rusqlite::params!["clone-test-id"],
            )
            .expect("Failed to query via clone");

        assert_eq!(row.get::<String>("number").unwrap(), "5555");
    }

    // ── Error Display Tests ───────────────────────────────

    #[test]
    fn test_error_display() {
        let e = LocalDbError::ConnectionError("cannot open file".to_string());
        assert_eq!(format!("{}", e), "Connection error: cannot open file");

        let e = LocalDbError::MigrationError("v1 failed".to_string());
        assert_eq!(format!("{}", e), "Migration error: v1 failed");

        let e = LocalDbError::QueryError("syntax error".to_string());
        assert_eq!(format!("{}", e), "Query error: syntax error");

        let e = LocalDbError::NotFound;
        assert_eq!(format!("{}", e), "Record not found");

        let e = LocalDbError::NotInitialized;
        assert_eq!(format!("{}", e), "Database not initialized");

        let e = LocalDbError::InvalidData("bad value".to_string());
        assert_eq!(format!("{}", e), "Invalid data: bad value");

        let e = LocalDbError::PoisonedLock;
        assert_eq!(format!("{}", e), "Database lock poisoned");
    }

    #[test]
    fn test_error_from_rusqlite() {
        // Verify that rusqlite::Error auto-converts to LocalDbError
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let sqlite_err = conn.execute_batch("INVALID SQL").unwrap_err();
        let db_err: LocalDbError = sqlite_err.into();
        assert!(matches!(db_err, LocalDbError::Sqlite(_)));
    }

    #[test]
    fn test_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let db_err: LocalDbError = io_err.into();
        assert!(matches!(db_err, LocalDbError::Io(_)));
    }
}
