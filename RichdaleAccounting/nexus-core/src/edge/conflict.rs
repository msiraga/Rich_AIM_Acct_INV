//! Edge Conflict Resolution
//!
//! Resolves conflicts between local and remote data versions.
//! Last-write-wins by default, with full audit trail.
//! Never deletes data — both versions are preserved in conflict log.
//!
//! # Architecture
//!
//! When the sync engine's `upsert_remote()` detects that a local record's
//! `_modified_at` is later than the remote record's `modified_at`, it returns
//! a [`SyncConflict`] containing both versions.  The [`ConflictResolver`]
//! takes that conflict, applies a resolution strategy, and writes the
//! winning version back to local storage.
//!
//! ## Resolution Strategies
//!
//! | Strategy        | Behaviour                                            |
//! |-----------------|------------------------------------------------------|
//! | `RemoteWins`    | Remote version always wins (default)                |
//! | `LocalWins`     | Local version always wins                            |
//! | `LastWriteWins` | Compare timestamps; the later modification wins     |
//! | `Manual`        | Requires human intervention via `resolve_manually`  |
//!
//! ## Audit Trail
//!
//! Every conflict is logged in the `conflict_log` SQLite table with:
//! - Both `local_data` and `remote_data` (the loser is never deleted)
//! - The resolution strategy used
//! - Which version won
//! - The winning and losing data
//! - When the conflict was created and when it was resolved
//!
//! ## Conflict Log Table
//!
//! The table is created on first use via `CREATE TABLE IF NOT EXISTS`,
//! following the same idempotent schema-evolution pattern as
//! [`ChangeTracker`](super::tracking::ChangeTracker).

use std::sync::Arc;
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use thiserror::Error;
use tracing::{info, debug, warn, error};

use super::local_db::{LocalDb, LocalDbError, Row};
use super::store::LocalStore;
use super::sync::SyncConflict;
use crate::database::financial::{
    Account, Transaction, TransactionEntry, JournalEntry,
};

// ═══════════════════════════════════════════════════════════
// Resolution Strategy
// ═══════════════════════════════════════════════════════════

/// Conflict resolution strategy.
///
/// Determines which version wins when a conflict is detected during sync.
/// The default is [`ConflictResolutionStrategy::RemoteWins`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConflictResolutionStrategy {
    /// Remote version always wins.
    RemoteWins,
    /// Local version always wins.
    LocalWins,
    /// Last write wins — compare timestamps; the later modification wins.
    /// If timestamps are equal, remote wins (default bias).
    LastWriteWins,
    /// Requires manual human intervention via [`ConflictResolver::resolve_manually`].
    Manual,
}

impl Default for ConflictResolutionStrategy {
    fn default() -> Self {
        Self::RemoteWins
    }
}

// ═══════════════════════════════════════════════════════════
// Conflict Winner
// ═══════════════════════════════════════════════════════════

/// Which version was chosen as the winner.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConflictWinner {
    /// Local version was chosen.
    Local,
    /// Remote version was chosen.
    Remote,
    /// A manual / custom resolution was applied.
    Manual,
}

// ═══════════════════════════════════════════════════════════
// Resolved Conflict
// ═══════════════════════════════════════════════════════════

/// A conflict that has been resolved by applying a resolution strategy.
///
/// Contains the winning data (to be applied to local storage), the losing
/// data (preserved for audit), and metadata about the resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedConflict {
    /// ID of the conflict_log entry this resolution applies to.
    pub conflict_id: i64,
    /// Singular entity type (`"account"`, `"transaction"`, …).
    pub entity_type: String,
    /// UUID of the entity (as a string).
    pub entity_id: String,
    /// Which version won.
    pub winner: ConflictWinner,
    /// The data from the winning version (to be written to local storage).
    pub winning_data: serde_json::Value,
    /// The data from the losing version (preserved for audit).
    pub losing_data: serde_json::Value,
    /// Local `_modified_at` timestamp at the time of conflict.
    pub local_modified_at: DateTime<Utc>,
    /// Remote `modified_at` timestamp at the time of conflict.
    pub remote_modified_at: DateTime<Utc>,
    /// When the conflict was resolved.
    pub resolved_at: DateTime<Utc>,
    /// The strategy used to resolve the conflict.
    pub strategy: ConflictResolutionStrategy,
}

// ═══════════════════════════════════════════════════════════
// Conflict Log Entry
// ═══════════════════════════════════════════════════════════

/// An entry in the conflict audit log (stored in the `conflict_log` table).
///
/// Both `local_data` and `remote_data` are always present — the loser is
/// never deleted.  When unresolved, `winner`, `winning_data`, `losing_data`,
/// and `resolved_at` are `None`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictLogEntry {
    /// Auto-incremented primary key.
    pub id: i64,
    /// Singular entity type.
    pub entity_type: String,
    /// UUID of the entity.
    pub entity_id: String,
    /// Local record data at the time of conflict (JSON).
    pub local_data: serde_json::Value,
    /// Remote record data at the time of conflict (JSON).
    pub remote_data: serde_json::Value,
    /// Local `_modified_at` timestamp.
    pub local_modified_at: DateTime<Utc>,
    /// Remote `modified_at` timestamp.
    pub remote_modified_at: DateTime<Utc>,
    /// The resolution strategy used (or configured).
    pub resolution: ConflictResolutionStrategy,
    /// Which version won (`None` = unresolved).
    pub winner: Option<ConflictWinner>,
    /// Winning version data (`None` = unresolved).
    pub winning_data: Option<serde_json::Value>,
    /// Losing version data (`None` = unresolved).
    pub losing_data: Option<serde_json::Value>,
    /// When the conflict was resolved (`None` = unresolved).
    pub resolved_at: Option<DateTime<Utc>>,
    /// When the conflict was first logged.
    pub created_at: DateTime<Utc>,
}

// ═══════════════════════════════════════════════════════════
// Error Type
// ═══════════════════════════════════════════════════════════

/// Errors that can occur during conflict resolution.
#[derive(Error, Debug)]
pub enum ConflictError {
    /// Underlying SQLite / `LocalDb` error.
    #[error("Local DB error: {0}")]
    Db(#[from] LocalDbError),
    /// The requested conflict or entity was not found.
    #[error("Not found: {0}")]
    NotFound(String),
    /// A `LocalStore` operation failed.
    #[error("Store error: {0}")]
    Store(String),
    /// A serialization or deserialization failure.
    #[error("Serialization error: {0}")]
    Serialization(String),
    /// A conflict requires manual resolution; the conflict ID is included.
    #[error("Manual resolution required for conflict {0}")]
    ManualRequired(i64),
    /// An invalid strategy or winner string was supplied.
    #[error("Invalid strategy: {0}")]
    InvalidStrategy(String),
}

// ═══════════════════════════════════════════════════════════
// Conflict Resolver
// ═══════════════════════════════════════════════════════════

/// Resolves sync conflicts between local and remote data versions.
///
/// Wraps an `Arc<LocalDb>` and a default [`ConflictResolutionStrategy`].
/// On construction, ensures the `conflict_log` table exists.
///
/// # Construction
///
/// ```no_run
/// use std::sync::Arc;
/// use nexus_core::edge::local_db::LocalDb;
/// use nexus_core::edge::conflict::{ConflictResolver, ConflictResolutionStrategy};
///
/// let db = Arc::new(LocalDb::open_in_memory().unwrap());
/// let resolver = ConflictResolver::new(db, ConflictResolutionStrategy::RemoteWins);
/// ```
///
/// # Typical Flow
///
/// 1. The sync engine returns `SyncConflict` structs during pull.
/// 2. Call [`resolve_conflict`](Self::resolve_conflict) for each conflict.
/// 3. For `Manual` strategy, the conflict is logged and
///    [`ManualRequired`](ConflictError::ManualRequired) is returned.
/// 4. Later, call [`resolve_manually`](Self::resolve_manually) with the
///    conflict ID and the chosen version.
/// 5. Call [`apply_resolution`](Self::apply_resolution) to write the
///    winning version back to local storage.
pub struct ConflictResolver {
    /// The local SQLite database (shared with store and tracker).
    db: Arc<LocalDb>,
    /// The default resolution strategy.
    strategy: ConflictResolutionStrategy,
}

impl ConflictResolver {
    /// Create a new `ConflictResolver` with the given default strategy.
    ///
    /// Ensures the `conflict_log` table exists in the database.  If table
    /// creation fails, the error is logged but does not prevent construction —
    /// the caller will discover the issue when queries against `conflict_log`
    /// fail.  This mirrors the pattern in
    /// [`ChangeTracker::new`](super::tracking::ChangeTracker::new).
    pub fn new(db: Arc<LocalDb>, strategy: ConflictResolutionStrategy) -> Self {
        let resolver = Self { db, strategy };
        if let Err(e) = resolver.ensure_conflict_table() {
            error!("Failed to ensure conflict_log table: {}", e);
        }
        resolver
    }

    // ── Table Management ─────────────────────────────────

    /// Create the `conflict_log` table if it does not already exist.
    ///
    /// Uses `CREATE TABLE IF NOT EXISTS`, so calling this on a database
    /// that already has the table is a no-op.  An index on
    /// `(entity_type, entity_id)` is also created for efficient lookups.
    fn ensure_conflict_table(&self) -> Result<(), ConflictError> {
        let sql = r#"
            CREATE TABLE IF NOT EXISTS conflict_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_type TEXT NOT NULL,
                entity_id TEXT NOT NULL,
                local_data TEXT NOT NULL,
                remote_data TEXT NOT NULL,
                local_modified_at TEXT NOT NULL,
                remote_modified_at TEXT NOT NULL,
                resolution TEXT NOT NULL DEFAULT 'RemoteWins',
                winner TEXT,
                winning_data TEXT,
                losing_data TEXT,
                resolved_at TEXT,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_conflict_log_entity
                ON conflict_log(entity_type, entity_id);
            CREATE INDEX IF NOT EXISTS idx_conflict_log_resolved
                ON conflict_log(resolved_at);
        "#;

        self.db.execute_batch(sql)?;
        info!("ensure_conflict_table: conflict_log table ready");
        Ok(())
    }

    // ── Conflict Resolution ──────────────────────────────

    /// Resolve a single conflict using the configured default strategy.
    ///
    /// The conflict is logged to the audit trail, the strategy is applied,
    /// and the conflict_log entry is updated with the resolution.
    ///
    /// # Errors
    ///
    /// Returns [`ConflictError::ManualRequired`] if the strategy is
    /// [`Manual`](ConflictResolutionStrategy::Manual).  In that case the
    /// conflict has been logged and the caller should later call
    /// [`resolve_manually`](Self::resolve_manually) with the returned
    /// conflict ID.
    pub fn resolve_conflict(
        &self,
        conflict: &SyncConflict,
    ) -> Result<ResolvedConflict, ConflictError> {
        self.resolve_with_strategy(conflict, self.strategy.clone())
    }

    /// Resolve a conflict with a specific strategy (overrides the default).
    ///
    /// Logs the conflict, applies the given strategy, and updates the
    /// conflict_log entry.
    pub fn resolve_with_strategy(
        &self,
        conflict: &SyncConflict,
        strategy: ConflictResolutionStrategy,
    ) -> Result<ResolvedConflict, ConflictError> {
        // Log (or update) the conflict in the audit trail.
        let conflict_id = self.log_conflict(conflict)?;
        debug!(
            "resolve_with_strategy: conflict_id={}, strategy={}, entity={} {}",
            conflict_id, strategy_to_str(&strategy), conflict.entity_type, conflict.entity_id
        );

        // Update the stored strategy on the log entry.
        self.db.execute(
            "UPDATE conflict_log SET resolution = ?1 WHERE id = ?2",
            rusqlite::params![strategy_to_str(&strategy), conflict_id],
        )?;

        // Manual strategy: log and defer.
        if strategy == ConflictResolutionStrategy::Manual {
            warn!(
                "resolve_with_strategy: conflict {} requires manual resolution",
                conflict_id
            );
            return Err(ConflictError::ManualRequired(conflict_id));
        }

        // Determine the winner based on the strategy.
        let (winner, winning_data, losing_data) = self.determine_winner(conflict, &strategy);

        let now = Utc::now();
        let winning_str = serde_json::to_string(&winning_data)
            .map_err(|e| ConflictError::Serialization(e.to_string()))?;
        let losing_str = serde_json::to_string(&losing_data)
            .map_err(|e| ConflictError::Serialization(e.to_string()))?;

        // Update the conflict_log entry with the resolution.
        self.db.execute(
            "UPDATE conflict_log SET \
                resolution = ?1, \
                winner = ?2, \
                winning_data = ?3, \
                losing_data = ?4, \
                resolved_at = ?5 \
             WHERE id = ?6",
            rusqlite::params![
                strategy_to_str(&strategy),
                winner_to_str(&winner),
                winning_str,
                losing_str,
                now.to_rfc3339(),
                conflict_id,
            ],
        )?;

        info!(
            "resolve_with_strategy: resolved conflict {} — winner={}, strategy={}",
            conflict_id, winner_to_str(&winner), strategy_to_str(&strategy)
        );

        Ok(ResolvedConflict {
            conflict_id,
            entity_type: conflict.entity_type.clone(),
            entity_id: conflict.entity_id.clone(),
            winner,
            winning_data,
            losing_data,
            local_modified_at: conflict.local_modified_at,
            remote_modified_at: conflict.remote_modified_at,
            resolved_at: now,
            strategy,
        })
    }

    /// Determine the winner based on the strategy and conflict data.
    ///
    /// Returns `(winner, winning_data, losing_data)`.
    fn determine_winner(
        &self,
        conflict: &SyncConflict,
        strategy: &ConflictResolutionStrategy,
    ) -> (ConflictWinner, serde_json::Value, serde_json::Value) {
        match strategy {
            ConflictResolutionStrategy::RemoteWins => (
                ConflictWinner::Remote,
                conflict.remote_data.clone(),
                conflict.local_data.clone(),
            ),
            ConflictResolutionStrategy::LocalWins => (
                ConflictWinner::Local,
                conflict.local_data.clone(),
                conflict.remote_data.clone(),
            ),
            ConflictResolutionStrategy::LastWriteWins => {
                if conflict.local_modified_at > conflict.remote_modified_at {
                    debug!(
                        "determine_winner: LastWriteWins — local is newer ({})",
                        conflict.local_modified_at
                    );
                    (
                        ConflictWinner::Local,
                        conflict.local_data.clone(),
                        conflict.remote_data.clone(),
                    )
                } else {
                    // Remote is newer or timestamps are equal — remote wins.
                    debug!(
                        "determine_winner: LastWriteWins — remote is newer or equal ({})",
                        conflict.remote_modified_at
                    );
                    (
                        ConflictWinner::Remote,
                        conflict.remote_data.clone(),
                        conflict.local_data.clone(),
                    )
                }
            }
            // Manual is handled by the caller; this branch is unreachable
            // but included for exhaustiveness.
            ConflictResolutionStrategy::Manual => (
                ConflictWinner::Manual,
                conflict.remote_data.clone(),
                conflict.local_data.clone(),
            ),
        }
    }

    // ── Apply Resolution ─────────────────────────────────

    /// Apply a resolved conflict to local storage.
    ///
    /// Deserializes the winning data into the appropriate typed model,
    /// saves it via [`LocalStore`] (which sets `_dirty = 1`), then clears
    /// the dirty flag and updates `_modified_at` to the resolution
    /// timestamp.
    ///
    /// This mirrors the sync engine's `save_remote_record` pattern.
    pub fn apply_resolution(
        &self,
        resolved: &ResolvedConflict,
        store: &LocalStore,
    ) -> Result<(), ConflictError> {
        info!(
            "apply_resolution: applying {} winner for {} {}",
            winner_to_str(&resolved.winner),
            resolved.entity_type,
            resolved.entity_id
        );

        let entity_id = &resolved.entity_id;
        let resolved_at = resolved.resolved_at;

        match resolved.entity_type.as_str() {
            "account" => {
                let account: Account = serde_json::from_value(resolved.winning_data.clone())
                    .map_err(|e| {
                        ConflictError::Serialization(format!(
                            "Failed to deserialize account: {}",
                            e
                        ))
                    })?;
                store
                    .save_account(&account)
                    .map_err(|e| ConflictError::Store(e.to_string()))?;
                self.post_apply("account", entity_id, resolved_at)?;
            }
            "transaction" => {
                let txn: Transaction = serde_json::from_value(resolved.winning_data.clone())
                    .map_err(|e| {
                        ConflictError::Serialization(format!(
                            "Failed to deserialize transaction: {}",
                            e
                        ))
                    })?;
                store
                    .save_transaction(&txn)
                    .map_err(|e| ConflictError::Store(e.to_string()))?;
                self.post_apply("transaction", entity_id, resolved_at)?;
            }
            "journal_entry" => {
                let je: JournalEntry = serde_json::from_value(resolved.winning_data.clone())
                    .map_err(|e| {
                        ConflictError::Serialization(format!(
                            "Failed to deserialize journal entry: {}",
                            e
                        ))
                    })?;
                store
                    .save_journal_entry(&je)
                    .map_err(|e| ConflictError::Store(e.to_string()))?;
                self.post_apply("journal_entry", entity_id, resolved_at)?;
            }
            "transaction_entry" => {
                let entry: TransactionEntry =
                    serde_json::from_value(resolved.winning_data.clone())
                        .map_err(|e| {
                            ConflictError::Serialization(format!(
                                "Failed to deserialize transaction entry: {}",
                                e
                            ))
                        })?;
                store
                    .save_transaction_entry(&entry)
                    .map_err(|e| ConflictError::Store(e.to_string()))?;
                self.post_apply("transaction_entry", entity_id, resolved_at)?;
            }
            _ => {
                warn!(
                    "apply_resolution: entity type '{}' not supported for typed apply; \
                     writing raw JSON",
                    resolved.entity_type
                );
                self.apply_raw_json(&resolved.entity_type, entity_id, &resolved.winning_data)?;
                self.post_apply(&resolved.entity_type, entity_id, resolved_at)?;
            }
        }

        debug!(
            "apply_resolution: applied resolution for {} {}",
            resolved.entity_type, resolved.entity_id
        );
        Ok(())
    }

    /// After saving via the store, clear the dirty flag and set
    /// `_modified_at` to the resolution timestamp.
    ///
    /// This mirrors the sync engine's `mark_synced` + `update_modified_at`
    /// pattern: the saved record should not be re-pushed just because it
    /// was written as part of conflict resolution.
    fn post_apply(
        &self,
        entity_type: &str,
        entity_id: &str,
        resolved_at: DateTime<Utc>,
    ) -> Result<(), ConflictError> {
        let table = entity_type_to_table(entity_type).ok_or_else(|| {
            ConflictError::InvalidStrategy(format!(
                "Unknown entity type: '{}'",
                entity_type
            ))
        })?;

        // Clear the dirty flag and set _modified_at to the resolution time.
        let sql = format!(
            "UPDATE {} SET _dirty = 0, _modified_at = ?1 WHERE id = ?2",
            table
        );
        self.db.execute(
            &sql,
            rusqlite::params![resolved_at.to_rfc3339(), entity_id],
        )?;

        Ok(())
    }

    /// Write raw JSON data directly to a data table.
    ///
    /// Used for entity types that don't have a typed `save_*` method on
    /// `LocalStore` (e.g. `invoice`, `bill`, `asset`).  The JSON object's
    /// keys must match the table column names.
    fn apply_raw_json(
        &self,
        entity_type: &str,
        entity_id: &str,
        data: &serde_json::Value,
    ) -> Result<(), ConflictError> {
        let table = entity_type_to_table(entity_type).ok_or_else(|| {
            ConflictError::InvalidStrategy(format!("Unknown entity type: '{}'", entity_type))
        })?;

        let obj = data.as_object().ok_or_else(|| {
            ConflictError::Serialization(format!(
                "Expected JSON object for {} {}",
                entity_type, entity_id
            ))
        })?;

        // Build SET clause and collect parameter values in a single pass
        // to ensure the placeholder count matches the parameter count.
        let mut set_parts: Vec<String> = Vec::new();
        let mut param_values: Vec<String> = Vec::new();

        for (key, value) in obj.iter() {
            if key == "id" {
                continue;
            }
            set_parts.push(format!("{} = ?", key));
            // Convert any JSON value to its string representation.
            // SQLite is flexible with type coercion for TEXT columns.
            let val_str = match value {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Null => String::new(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Number(n) => n.to_string(),
                other => other.to_string(),
            };
            param_values.push(val_str);
        }

        if set_parts.is_empty() {
            return Ok(());
        }

        let sql = format!(
            "UPDATE {} SET {} WHERE id = ?",
            table,
            set_parts.join(", ")
        );

        // Build the parameter slice: column values followed by the id.
        let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(param_values.len() + 1);
        for v in &param_values {
            params.push(v as &dyn rusqlite::ToSql);
        }
        params.push(&entity_id as &dyn rusqlite::ToSql);

        self.db.execute(&sql, &params)?;
        Ok(())
    }

    // ── Conflict Logging ─────────────────────────────────

    /// Log a conflict to the audit trail.
    ///
    /// If a conflict_log entry already exists for the same
    /// `(entity_type, entity_id)`, it is **updated** with the new data
    /// (resetting it to unresolved).  Otherwise, a new entry is inserted.
    ///
    /// This ensures re-resolving a conflict updates the existing entry
    /// rather than creating a duplicate.
    ///
    /// Returns the conflict_log entry ID.
    pub fn log_conflict(&self, conflict: &SyncConflict) -> Result<i64, ConflictError> {
        let local_data_str = serde_json::to_string(&conflict.local_data)
            .map_err(|e| ConflictError::Serialization(e.to_string()))?;
        let remote_data_str = serde_json::to_string(&conflict.remote_data)
            .map_err(|e| ConflictError::Serialization(e.to_string()))?;
        let now = Utc::now().to_rfc3339();

        // Check for an existing entry for this entity.
        let existing = self.db.query_one(
            "SELECT id FROM conflict_log \
             WHERE entity_type = ?1 AND entity_id = ?2 \
             ORDER BY id DESC LIMIT 1",
            rusqlite::params![&conflict.entity_type, &conflict.entity_id],
        );

        match existing {
            Ok(row) => {
                // Update the existing entry with fresh conflict data,
                // resetting it to unresolved.
                let id: i64 = row.get("id")?;
                debug!(
                    "log_conflict: updating existing entry {} for {} {}",
                    id, conflict.entity_type, conflict.entity_id
                );
                self.db.execute(
                    "UPDATE conflict_log SET \
                        local_data = ?1, \
                        remote_data = ?2, \
                        local_modified_at = ?3, \
                        remote_modified_at = ?4, \
                        resolution = ?5, \
                        winner = NULL, \
                        winning_data = NULL, \
                        losing_data = NULL, \
                        resolved_at = NULL, \
                        created_at = ?6 \
                     WHERE id = ?7",
                    rusqlite::params![
                        local_data_str,
                        remote_data_str,
                        conflict.local_modified_at.to_rfc3339(),
                        conflict.remote_modified_at.to_rfc3339(),
                        strategy_to_str(&self.strategy),
                        now,
                        id,
                    ],
                )?;
                Ok(id)
            }
            Err(LocalDbError::NotFound) => {
                // Insert a new entry.
                debug!(
                    "log_conflict: creating new entry for {} {}",
                    conflict.entity_type, conflict.entity_id
                );
                self.db.execute(
                    "INSERT INTO conflict_log \
                        (entity_type, entity_id, local_data, remote_data, \
                         local_modified_at, remote_modified_at, resolution, \
                         winner, winning_data, losing_data, resolved_at, created_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL, NULL, NULL, ?8)",
                    rusqlite::params![
                        &conflict.entity_type,
                        &conflict.entity_id,
                        local_data_str,
                        remote_data_str,
                        conflict.local_modified_at.to_rfc3339(),
                        conflict.remote_modified_at.to_rfc3339(),
                        strategy_to_str(&self.strategy),
                        now,
                    ],
                )?;

                let row = self.db.query_one(
                    "SELECT last_insert_rowid() AS id",
                    &[],
                )?;
                let id: i64 = row.get("id")?;
                info!(
                    "log_conflict: created entry {} for {} {}",
                    id, conflict.entity_type, conflict.entity_id
                );
                Ok(id)
            }
            Err(e) => Err(ConflictError::Db(e)),
        }
    }

    // ── Manual Resolution ────────────────────────────────

    /// Manually resolve a conflict by choosing a version.
    ///
    /// Looks up the conflict_log entry by `conflict_id`, determines the
    /// winning data based on `chosen_version`, and updates the entry.
    ///
    /// # Parameters
    ///
    /// - `conflict_id`: The ID returned by [`log_conflict`](Self::log_conflict)
    ///   or the `ManualRequired` error.
    /// - `chosen_version`: Which version to pick — must be
    ///   [`ConflictWinner::Local`] or [`ConflictWinner::Remote`].
    ///
    /// # Errors
    ///
    /// Returns [`ConflictError::NotFound`] if the conflict ID does not exist.
    /// Returns [`ConflictError::InvalidStrategy`] if `chosen_version` is
    /// [`ConflictWinner::Manual`] (use `Local` or `Remote` instead).
    pub fn resolve_manually(
        &self,
        conflict_id: i64,
        chosen_version: ConflictWinner,
    ) -> Result<ResolvedConflict, ConflictError> {
        let entry = self.get_conflict(conflict_id)?;

        debug!(
            "resolve_manually: conflict_id={}, chosen_version={}",
            conflict_id, winner_to_str(&chosen_version)
        );

        let (winner, winning_data, losing_data) = match chosen_version {
            ConflictWinner::Local => (
                ConflictWinner::Local,
                entry.local_data.clone(),
                entry.remote_data.clone(),
            ),
            ConflictWinner::Remote => (
                ConflictWinner::Remote,
                entry.remote_data.clone(),
                entry.local_data.clone(),
            ),
            ConflictWinner::Manual => {
                return Err(ConflictError::InvalidStrategy(
                    "Manual winner is not valid for resolve_manually. \
                     Use Local or Remote."
                        .to_string(),
                ));
            }
        };

        let now = Utc::now();
        let winning_str = serde_json::to_string(&winning_data)
            .map_err(|e| ConflictError::Serialization(e.to_string()))?;
        let losing_str = serde_json::to_string(&losing_data)
            .map_err(|e| ConflictError::Serialization(e.to_string()))?;

        self.db.execute(
            "UPDATE conflict_log SET \
                resolution = 'Manual', \
                winner = ?1, \
                winning_data = ?2, \
                losing_data = ?3, \
                resolved_at = ?4 \
             WHERE id = ?5",
            rusqlite::params![
                winner_to_str(&winner),
                winning_str,
                losing_str,
                now.to_rfc3339(),
                conflict_id,
            ],
        )?;

        info!(
            "resolve_manually: resolved conflict {} — winner={}",
            conflict_id, winner_to_str(&winner)
        );

        Ok(ResolvedConflict {
            conflict_id,
            entity_type: entry.entity_type,
            entity_id: entry.entity_id,
            winner,
            winning_data,
            losing_data,
            local_modified_at: entry.local_modified_at,
            remote_modified_at: entry.remote_modified_at,
            resolved_at: now,
            strategy: ConflictResolutionStrategy::Manual,
        })
    }

    // ── Query Methods ────────────────────────────────────

    /// Get all conflict log entries, ordered by creation time descending.
    pub fn get_conflicts(&self) -> Result<Vec<ConflictLogEntry>, ConflictError> {
        let rows = self.db.query_all(
            "SELECT * FROM conflict_log ORDER BY created_at DESC, id DESC",
            &[],
        )?;
        let entries: Result<Vec<_>, _> = rows.iter().map(row_to_conflict_log_entry).collect();
        entries
    }

    /// Get only unresolved conflicts (where `resolved_at IS NULL`).
    pub fn get_unresolved_conflicts(&self) -> Result<Vec<ConflictLogEntry>, ConflictError> {
        let rows = self.db.query_all(
            "SELECT * FROM conflict_log WHERE resolved_at IS NULL \
             ORDER BY created_at DESC, id DESC",
            &[],
        )?;
        let entries: Result<Vec<_>, _> = rows.iter().map(row_to_conflict_log_entry).collect();
        entries
    }

    /// Get only resolved conflicts (where `resolved_at IS NOT NULL`).
    pub fn get_resolved_conflicts(&self) -> Result<Vec<ConflictLogEntry>, ConflictError> {
        let rows = self.db.query_all(
            "SELECT * FROM conflict_log WHERE resolved_at IS NOT NULL \
             ORDER BY resolved_at DESC, id DESC",
            &[],
        )?;
        let entries: Result<Vec<_>, _> = rows.iter().map(row_to_conflict_log_entry).collect();
        entries
    }

    /// Get a specific conflict log entry by ID.
    pub fn get_conflict(&self, id: i64) -> Result<ConflictLogEntry, ConflictError> {
        let row = self.db.query_one(
            "SELECT * FROM conflict_log WHERE id = ?1",
            rusqlite::params![id],
        ).map_err(|e| match e {
            LocalDbError::NotFound => ConflictError::NotFound(format!("Conflict {}", id)),
            e => ConflictError::Db(e),
        })?;
        row_to_conflict_log_entry(&row)
    }

    /// Get the count of unresolved conflicts.
    pub fn count_unresolved(&self) -> Result<usize, ConflictError> {
        let row = self.db.query_one(
            "SELECT COUNT(*) AS count FROM conflict_log WHERE resolved_at IS NULL",
            &[],
        )?;
        let count: i64 = row.get("count")?;
        Ok(count as usize)
    }

    // ── Configuration ────────────────────────────────────

    /// Change the default resolution strategy.
    pub fn set_strategy(&mut self, strategy: ConflictResolutionStrategy) {
        debug!(
            "set_strategy: changing from {} to {}",
            strategy_to_str(&self.strategy),
            strategy_to_str(&strategy)
        );
        self.strategy = strategy;
    }

    // ── Batch Processing ─────────────────────────────────

    /// Process multiple conflicts at once using the default strategy.
    ///
    /// Resolves each conflict in order.  If any conflict requires manual
    /// resolution (strategy is `Manual`), the error is returned immediately
    /// and remaining conflicts are not processed.
    pub fn resolve_batch(
        &self,
        conflicts: &[SyncConflict],
    ) -> Result<Vec<ResolvedConflict>, ConflictError> {
        let mut results = Vec::with_capacity(conflicts.len());
        for conflict in conflicts {
            match self.resolve_conflict(conflict) {
                Ok(resolved) => results.push(resolved),
                Err(ConflictError::ManualRequired(id)) => {
                    warn!(
                        "resolve_batch: conflict {} requires manual resolution; \
                         stopping batch",
                        id
                    );
                    return Err(ConflictError::ManualRequired(id));
                }
                Err(e) => {
                    error!("resolve_batch: failed to resolve conflict: {}", e);
                    return Err(e);
                }
            }
        }
        info!("resolve_batch: resolved {} conflicts", results.len());
        Ok(results)
    }
}

// ═══════════════════════════════════════════════════════════
// Private Free Functions
// ═══════════════════════════════════════════════════════════

/// Map a singular entity type to its plural data-table name.
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

/// Convert a `ConflictResolutionStrategy` to its string representation
/// for database storage.
fn strategy_to_str(s: &ConflictResolutionStrategy) -> &'static str {
    match s {
        ConflictResolutionStrategy::RemoteWins => "RemoteWins",
        ConflictResolutionStrategy::LocalWins => "LocalWins",
        ConflictResolutionStrategy::LastWriteWins => "LastWriteWins",
        ConflictResolutionStrategy::Manual => "Manual",
    }
}

/// Parse a `ConflictResolutionStrategy` from its string representation.
fn str_to_strategy(s: &str) -> Result<ConflictResolutionStrategy, ConflictError> {
    match s {
        "RemoteWins" => Ok(ConflictResolutionStrategy::RemoteWins),
        "LocalWins" => Ok(ConflictResolutionStrategy::LocalWins),
        "LastWriteWins" => Ok(ConflictResolutionStrategy::LastWriteWins),
        "Manual" => Ok(ConflictResolutionStrategy::Manual),
        _ => Err(ConflictError::InvalidStrategy(format!(
            "Unknown resolution strategy: '{}'",
            s
        ))),
    }
}

/// Convert a `ConflictWinner` to its string representation for database
/// storage.
fn winner_to_str(w: &ConflictWinner) -> &'static str {
    match w {
        ConflictWinner::Local => "Local",
        ConflictWinner::Remote => "Remote",
        ConflictWinner::Manual => "Manual",
    }
}

/// Parse a `ConflictWinner` from its string representation.
fn str_to_winner(s: &str) -> Result<ConflictWinner, ConflictError> {
    match s {
        "Local" => Ok(ConflictWinner::Local),
        "Remote" => Ok(ConflictWinner::Remote),
        "Manual" => Ok(ConflictWinner::Manual),
        _ => Err(ConflictError::InvalidStrategy(format!(
            "Unknown winner: '{}'",
            s
        ))),
    }
}

/// Parse an RFC 3339 datetime string into a `DateTime<Utc>`.
fn parse_dt(s: &str) -> Result<DateTime<Utc>, ConflictError> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            ConflictError::Serialization(format!(
                "Failed to parse datetime '{}': {}",
                s, e
            ))
        })
}

/// Parse a `serde_json::Value` from a JSON string.
fn parse_json(s: &str) -> Result<serde_json::Value, ConflictError> {
    serde_json::from_str(s).map_err(|e| {
        ConflictError::Serialization(format!("Failed to parse JSON: {}", e))
    })
}

/// Parse a `Row` from the `conflict_log` table into a `ConflictLogEntry`.
fn row_to_conflict_log_entry(row: &Row) -> Result<ConflictLogEntry, ConflictError> {
    let id: i64 = row.get("id")?;
    let entity_type: String = row.get("entity_type")?;
    let entity_id: String = row.get("entity_id")?;

    let local_data_str: String = row.get("local_data")?;
    let remote_data_str: String = row.get("remote_data")?;

    let local_modified_at_str: String = row.get("local_modified_at")?;
    let remote_modified_at_str: String = row.get("remote_modified_at")?;

    let resolution_str: String = row.get("resolution")?;

    let winner_str: Option<String> = row.get("winner")?;
    let winning_data_str: Option<String> = row.get("winning_data")?;
    let losing_data_str: Option<String> = row.get("losing_data")?;
    let resolved_at_str: Option<String> = row.get("resolved_at")?;
    let created_at_str: String = row.get("created_at")?;

    Ok(ConflictLogEntry {
        id,
        entity_type,
        entity_id,
        local_data: parse_json(&local_data_str)?,
        remote_data: parse_json(&remote_data_str)?,
        local_modified_at: parse_dt(&local_modified_at_str)?,
        remote_modified_at: parse_dt(&remote_modified_at_str)?,
        resolution: str_to_strategy(&resolution_str)?,
        winner: winner_str.and_then(|s| str_to_winner(&s).ok()),
        winning_data: winning_data_str.and_then(|s| parse_json(&s).ok()),
        losing_data: losing_data_str.and_then(|s| parse_json(&s).ok()),
        resolved_at: resolved_at_str.and_then(|s| parse_dt(&s).ok()),
        created_at: parse_dt(&created_at_str)?,
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
        Account, AccountType, AccountStatus,
    };
    use chrono::Utc;
    use rust_decimal_macros::dec;
    use uuid::Uuid;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    // ── Test Helpers ──────────────────────────────────────

    /// Monotonically increasing counter for unique account numbers.
    static ACCOUNT_COUNTER: AtomicU64 = AtomicU64::new(1);

    /// Create a test account with a unique number.
    fn make_test_account() -> Account {
        let num = ACCOUNT_COUNTER.fetch_add(1, Ordering::SeqCst);
        Account {
            id: Uuid::new_v4(),
            number: format!("ACC-{:05}", num),
            name: format!("Test Account {}", num),
            description: "Test account for conflict resolution".to_string(),
            account_type: AccountType::Asset,
            parent_id: None,
            status: AccountStatus::Active,
            balance: dec!(1000),
            currency: "USD".to_string(),
            is_bank_account: false,
            bank_details: None,
            is_reconciled: false,
            last_reconciled: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// Create a modified version of an account (same ID, different name).
    fn make_modified_account(original: &Account, new_name: &str) -> Account {
        let mut modified = original.clone();
        modified.name = new_name.to_string();
        modified.updated_at = Utc::now();
        modified
    }

    /// Create a `SyncConflict` with the given entity ID, local data, remote
    /// data, and timestamps.
    fn make_conflict(
        entity_id: &str,
        local_data: serde_json::Value,
        remote_data: serde_json::Value,
        local_modified_at: DateTime<Utc>,
        remote_modified_at: DateTime<Utc>,
    ) -> SyncConflict {
        SyncConflict {
            entity_type: "account".to_string(),
            entity_id: entity_id.to_string(),
            local_modified_at,
            remote_modified_at,
            local_data,
            remote_data,
        }
    }

    /// Create a `ConflictResolver` with an in-memory DB and the given strategy.
    fn make_resolver(strategy: ConflictResolutionStrategy) -> ConflictResolver {
        let db = Arc::new(
            LocalDb::open_in_memory().expect("Failed to open in-memory database"),
        );
        ConflictResolver::new(db, strategy)
    }

    /// Create a `ConflictResolver` and `LocalStore` sharing the same
    /// in-memory `LocalDb`.
    fn make_resolver_and_store(
        strategy: ConflictResolutionStrategy,
    ) -> (ConflictResolver, LocalStore, Arc<LocalDb>) {
        let db = Arc::new(
            LocalDb::open_in_memory().expect("Failed to open in-memory database"),
        );
        let resolver = ConflictResolver::new(db.clone(), strategy);
        let store = LocalStore::new(db.clone());
        (resolver, store, db)
    }

    // ── Strategy: RemoteWins ─────────────────────────────

    #[test]
    fn test_resolve_conflict_remote_wins() {
        let resolver = make_resolver(ConflictResolutionStrategy::RemoteWins);

        let local = serde_json::json!({"name": "Local Version"});
        let remote = serde_json::json!({"name": "Remote Version"});
        let now = Utc::now();
        let conflict = make_conflict("acc-1", local, remote, now, now);

        let resolved = resolver
            .resolve_conflict(&conflict)
            .expect("resolve_conflict failed");

        assert_eq!(resolved.winner, ConflictWinner::Remote);
        assert_eq!(resolved.winning_data, serde_json::json!({"name": "Remote Version"}));
        assert_eq!(resolved.losing_data, serde_json::json!({"name": "Local Version"}));
        assert_eq!(resolved.strategy, ConflictResolutionStrategy::RemoteWins);
        assert!(resolved.conflict_id > 0);
    }

    // ── Strategy: LocalWins ──────────────────────────────

    #[test]
    fn test_resolve_conflict_local_wins() {
        let resolver = make_resolver(ConflictResolutionStrategy::LocalWins);

        let local = serde_json::json!({"name": "Local Version"});
        let remote = serde_json::json!({"name": "Remote Version"});
        let now = Utc::now();
        let conflict = make_conflict("acc-2", local, remote, now, now);

        let resolved = resolver
            .resolve_conflict(&conflict)
            .expect("resolve_conflict failed");

        assert_eq!(resolved.winner, ConflictWinner::Local);
        assert_eq!(resolved.winning_data, serde_json::json!({"name": "Local Version"}));
        assert_eq!(resolved.losing_data, serde_json::json!({"name": "Remote Version"}));
        assert_eq!(resolved.strategy, ConflictResolutionStrategy::LocalWins);
    }

    // ── Strategy: LastWriteWins (local newer) ────────────

    #[test]
    fn test_resolve_conflict_last_write_wins_local_newer() {
        let resolver = make_resolver(ConflictResolutionStrategy::LastWriteWins);

        let local = serde_json::json!({"name": "Local Version"});
        let remote = serde_json::json!({"name": "Remote Version"});

        // Local modified after remote — local should win.
        let remote_time = Utc::now();
        let local_time = remote_time + chrono::Duration::hours(1);
        let conflict = make_conflict("acc-3", local, remote, local_time, remote_time);

        let resolved = resolver
            .resolve_conflict(&conflict)
            .expect("resolve_conflict failed");

        assert_eq!(resolved.winner, ConflictWinner::Local);
        assert_eq!(resolved.winning_data, serde_json::json!({"name": "Local Version"}));
        assert_eq!(resolved.losing_data, serde_json::json!({"name": "Remote Version"}));
    }

    // ── Strategy: LastWriteWins (remote newer) ───────────

    #[test]
    fn test_resolve_conflict_last_write_wins_remote_newer() {
        let resolver = make_resolver(ConflictResolutionStrategy::LastWriteWins);

        let local = serde_json::json!({"name": "Local Version"});
        let remote = serde_json::json!({"name": "Remote Version"});

        // Remote modified after local — remote should win.
        let local_time = Utc::now();
        let remote_time = local_time + chrono::Duration::hours(1);
        let conflict = make_conflict("acc-4", local, remote, local_time, remote_time);

        let resolved = resolver
            .resolve_conflict(&conflict)
            .expect("resolve_conflict failed");

        assert_eq!(resolved.winner, ConflictWinner::Remote);
        assert_eq!(resolved.winning_data, serde_json::json!({"name": "Remote Version"}));
        assert_eq!(resolved.losing_data, serde_json::json!({"name": "Local Version"}));
    }

    // ── Strategy: Manual ─────────────────────────────────

    #[test]
    fn test_resolve_conflict_manual_returns_error() {
        let resolver = make_resolver(ConflictResolutionStrategy::Manual);

        let local = serde_json::json!({"name": "Local Version"});
        let remote = serde_json::json!({"name": "Remote Version"});
        let now = Utc::now();
        let conflict = make_conflict("acc-5", local, remote, now, now);

        let result = resolver.resolve_conflict(&conflict);

        assert!(
            matches!(result, Err(ConflictError::ManualRequired(id)) if id > 0),
            "Expected ManualRequired error, got: {:?}",
            result
        );

        // Verify the conflict was logged.
        let conflicts = resolver.get_conflicts().expect("get_conflicts");
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts[0].resolved_at.is_none(), "Conflict should be unresolved");
    }

    // ── resolve_with_strategy overrides default ──────────

    #[test]
    fn test_resolve_with_strategy_overrides_default() {
        // Default strategy is RemoteWins.
        let resolver = make_resolver(ConflictResolutionStrategy::RemoteWins);

        let local = serde_json::json!({"name": "Local Version"});
        let remote = serde_json::json!({"name": "Remote Version"});
        let now = Utc::now();
        let conflict = make_conflict("acc-6", local, remote, now, now);

        // Override with LocalWins.
        let resolved = resolver
            .resolve_with_strategy(&conflict, ConflictResolutionStrategy::LocalWins)
            .expect("resolve_with_strategy failed");

        assert_eq!(resolved.winner, ConflictWinner::Local);
        assert_eq!(resolved.strategy, ConflictResolutionStrategy::LocalWins);

        // The default strategy should still be RemoteWins.
        let resolved_default = resolver
            .resolve_conflict(&make_conflict(
                "acc-7",
                serde_json::json!({"name": "Local"}),
                serde_json::json!({"name": "Remote"}),
                now,
                now,
            ))
            .expect("resolve_conflict failed");

        assert_eq!(resolved_default.winner, ConflictWinner::Remote);
    }

    // ── apply_resolution writes winning version ──────────

    #[test]
    fn test_apply_resolution_writes_winning_version() {
        let (resolver, store, _db) =
            make_resolver_and_store(ConflictResolutionStrategy::RemoteWins);

        // Create and save an account locally.
        let account = make_test_account();
        store.save_account(&account).expect("save account");

        // Create a modified version (remote wins).
        let modified = make_modified_account(&account, "Remote Updated Name");
        let modified_json = serde_json::to_value(&modified).expect("serialize modified");

        let local_json = serde_json::to_value(&account).expect("serialize original");

        let conflict = make_conflict(
            &account.id.to_string(),
            local_json,
            modified_json,
            Utc::now(),
            Utc::now() + chrono::Duration::hours(1),
        );

        // Resolve with RemoteWins.
        let resolved = resolver
            .resolve_conflict(&conflict)
            .expect("resolve_conflict failed");

        assert_eq!(resolved.winner, ConflictWinner::Remote);

        // Apply the resolution.
        resolver
            .apply_resolution(&resolved, &store)
            .expect("apply_resolution failed");

        // Verify the local store now has the modified version.
        let retrieved = store
            .get_account(account.id)
            .expect("get_account failed");
        assert_eq!(retrieved.name, "Remote Updated Name");
        assert_eq!(retrieved.id, account.id);
        assert_eq!(retrieved.number, account.number);
    }

    // ── log_conflict creates audit trail entry ───────────

    #[test]
    fn test_log_conflict_creates_audit_entry() {
        let resolver = make_resolver(ConflictResolutionStrategy::RemoteWins);

        let local = serde_json::json!({"name": "Local"});
        let remote = serde_json::json!({"name": "Remote"});
        let now = Utc::now();
        let conflict = make_conflict("acc-log-1", local, remote, now, now);

        let id = resolver
            .log_conflict(&conflict)
            .expect("log_conflict failed");

        assert!(id > 0);

        // Verify the entry exists.
        let entry = resolver.get_conflict(id).expect("get_conflict failed");
        assert_eq!(entry.entity_type, "account");
        assert_eq!(entry.entity_id, "acc-log-1");
        assert_eq!(entry.local_data, serde_json::json!({"name": "Local"}));
        assert_eq!(entry.remote_data, serde_json::json!({"name": "Remote"}));
        assert!(entry.winner.is_none(), "Should be unresolved");
        assert!(entry.resolved_at.is_none(), "Should be unresolved");
    }

    // ── resolve_manually ─────────────────────────────────

    #[test]
    fn test_resolve_manually() {
        let resolver = make_resolver(ConflictResolutionStrategy::Manual);

        let local = serde_json::json!({"name": "Local Version"});
        let remote = serde_json::json!({"name": "Remote Version"});
        let now = Utc::now();
        let conflict = make_conflict("acc-manual-1", local, remote, now, now);

        // Resolve with Manual strategy — should return ManualRequired.
        let result = resolver.resolve_conflict(&conflict);
        let conflict_id = match result {
            Err(ConflictError::ManualRequired(id)) => id,
            other => panic!("Expected ManualRequired, got: {:?}", other),
        };

        // Verify the conflict is unresolved.
        let unresolved = resolver
            .get_unresolved_conflicts()
            .expect("get_unresolved_conflicts");
        assert_eq!(unresolved.len(), 1);

        // Manually resolve by choosing the local version.
        let resolved = resolver
            .resolve_manually(conflict_id, ConflictWinner::Local)
            .expect("resolve_manually failed");

        assert_eq!(resolved.winner, ConflictWinner::Local);
        assert_eq!(resolved.winning_data, serde_json::json!({"name": "Local Version"}));
        assert_eq!(resolved.losing_data, serde_json::json!({"name": "Remote Version"}));
        assert_eq!(resolved.strategy, ConflictResolutionStrategy::Manual);

        // Verify the conflict is now resolved.
        let unresolved_after = resolver
            .get_unresolved_conflicts()
            .expect("get_unresolved_conflicts after");
        assert_eq!(unresolved_after.len(), 0, "Should have 0 unresolved after manual resolve");

        let resolved_entries = resolver
            .get_resolved_conflicts()
            .expect("get_resolved_conflicts");
        assert_eq!(resolved_entries.len(), 1);
        assert_eq!(resolved_entries[0].winner, Some(ConflictWinner::Local));
    }

    // ── resolve_manually with Remote ─────────────────────

    #[test]
    fn test_resolve_manually_remote() {
        let resolver = make_resolver(ConflictResolutionStrategy::Manual);

        let local = serde_json::json!({"name": "Local Version"});
        let remote = serde_json::json!({"name": "Remote Version"});
        let now = Utc::now();
        let conflict = make_conflict("acc-manual-2", local, remote, now, now);

        // Log and get the conflict ID.
        let conflict_id = resolver
            .log_conflict(&conflict)
            .expect("log_conflict failed");

        // Manually resolve by choosing the remote version.
        let resolved = resolver
            .resolve_manually(conflict_id, ConflictWinner::Remote)
            .expect("resolve_manually failed");

        assert_eq!(resolved.winner, ConflictWinner::Remote);
        assert_eq!(resolved.winning_data, serde_json::json!({"name": "Remote Version"}));
        assert_eq!(resolved.losing_data, serde_json::json!({"name": "Local Version"}));
    }

    // ── resolve_manually with Manual winner returns error ─

    #[test]
    fn test_resolve_manually_with_manual_winner_errors() {
        let resolver = make_resolver(ConflictResolutionStrategy::Manual);

        let local = serde_json::json!({"name": "Local"});
        let remote = serde_json::json!({"name": "Remote"});
        let now = Utc::now();
        let conflict = make_conflict("acc-manual-3", local, remote, now, now);

        let conflict_id = resolver
            .log_conflict(&conflict)
            .expect("log_conflict failed");

        let result = resolver.resolve_manually(conflict_id, ConflictWinner::Manual);
        assert!(
            matches!(result, Err(ConflictError::InvalidStrategy(_))),
            "Expected InvalidStrategy error, got: {:?}",
            result
        );
    }

    // ── get_conflicts returns all entries ────────────────

    #[test]
    fn test_get_conflicts_returns_all() {
        let resolver = make_resolver(ConflictResolutionStrategy::RemoteWins);

        let now = Utc::now();

        // Log and resolve 3 conflicts.
        for i in 0..3 {
            let id = format!("acc-all-{}", i);
            let conflict = make_conflict(
                &id,
                serde_json::json!({"v": "local"}),
                serde_json::json!({"v": "remote"}),
                now,
                now,
            );
            resolver.resolve_conflict(&conflict).expect("resolve_conflict");
        }

        let conflicts = resolver.get_conflicts().expect("get_conflicts");
        assert_eq!(conflicts.len(), 3, "Should have 3 conflict entries");
    }

    // ── get_unresolved_conflicts filters correctly ───────

    #[test]
    fn test_get_unresolved_conflicts_filters() {
        let resolver = make_resolver(ConflictResolutionStrategy::RemoteWins);

        let now = Utc::now();

        // Create 2 resolved conflicts.
        for i in 0..2 {
            let conflict = make_conflict(
                &format!("acc-unres-{}", i),
                serde_json::json!({"v": "local"}),
                serde_json::json!({"v": "remote"}),
                now,
                now,
            );
            resolver.resolve_conflict(&conflict).expect("resolve_conflict");
        }

        // Create 1 unresolved conflict (Manual strategy).
        let manual_conflict = make_conflict(
            "acc-unres-manual",
            serde_json::json!({"v": "local"}),
            serde_json::json!({"v": "remote"}),
            now,
            now,
        );
        let _ = resolver.resolve_with_strategy(&manual_conflict, ConflictResolutionStrategy::Manual);

        let unresolved = resolver
            .get_unresolved_conflicts()
            .expect("get_unresolved_conflicts");
        assert_eq!(unresolved.len(), 1, "Should have 1 unresolved conflict");

        let resolved = resolver
            .get_resolved_conflicts()
            .expect("get_resolved_conflicts");
        assert_eq!(resolved.len(), 2, "Should have 2 resolved conflicts");
    }

    // ── get_resolved_conflicts filters correctly ──────────

    #[test]
    fn test_get_resolved_conflicts_filters() {
        let resolver = make_resolver(ConflictResolutionStrategy::LocalWins);

        let now = Utc::now();

        // Create 3 resolved conflicts.
        for i in 0..3 {
            let conflict = make_conflict(
                &format!("acc-res-{}", i),
                serde_json::json!({"v": "local"}),
                serde_json::json!({"v": "remote"}),
                now,
                now,
            );
            resolver.resolve_conflict(&conflict).expect("resolve_conflict");
        }

        let resolved = resolver
            .get_resolved_conflicts()
            .expect("get_resolved_conflicts");
        assert_eq!(resolved.len(), 3, "Should have 3 resolved conflicts");

        // All should be resolved.
        for entry in &resolved {
            assert!(entry.resolved_at.is_some(), "Entry should be resolved");
            assert!(entry.winner.is_some(), "Entry should have a winner");
        }

        let unresolved = resolver
            .get_unresolved_conflicts()
            .expect("get_unresolved_conflicts");
        assert_eq!(unresolved.len(), 0, "Should have 0 unresolved conflicts");
    }

    // ── get_conflict by ID ───────────────────────────────

    #[test]
    fn test_get_conflict_by_id() {
        let resolver = make_resolver(ConflictResolutionStrategy::RemoteWins);

        let local = serde_json::json!({"name": "Local"});
        let remote = serde_json::json!({"name": "Remote"});
        let now = Utc::now();
        let conflict = make_conflict("acc-by-id-1", local, remote, now, now);

        let id = resolver
            .log_conflict(&conflict)
            .expect("log_conflict failed");

        let entry = resolver.get_conflict(id).expect("get_conflict failed");
        assert_eq!(entry.id, id);
        assert_eq!(entry.entity_id, "acc-by-id-1");
        assert_eq!(entry.local_data, serde_json::json!({"name": "Local"}));
        assert_eq!(entry.remote_data, serde_json::json!({"name": "Remote"}));
    }

    // ── get_conflict by non-existent ID ──────────────────

    #[test]
    fn test_get_conflict_not_found() {
        let resolver = make_resolver(ConflictResolutionStrategy::RemoteWins);

        let result = resolver.get_conflict(99999);
        assert!(
            matches!(result, Err(ConflictError::NotFound(_))),
            "Expected NotFound error, got: {:?}",
            result
        );
    }

    // ── count_unresolved ─────────────────────────────────

    #[test]
    fn test_count_unresolved() {
        let resolver = make_resolver(ConflictResolutionStrategy::RemoteWins);

        // Initially 0 unresolved.
        assert_eq!(
            resolver.count_unresolved().expect("count_unresolved"),
            0
        );

        let now = Utc::now();

        // Create 2 resolved conflicts.
        for i in 0..2 {
            let conflict = make_conflict(
                &format!("acc-count-{}", i),
                serde_json::json!({"v": "local"}),
                serde_json::json!({"v": "remote"}),
                now,
                now,
            );
            resolver.resolve_conflict(&conflict).expect("resolve_conflict");
        }

        // Still 0 unresolved.
        assert_eq!(
            resolver.count_unresolved().expect("count_unresolved"),
            0
        );

        // Create 1 unresolved conflict.
        let manual_resolver = ConflictResolver::new(
            resolver.db.clone(),
            ConflictResolutionStrategy::Manual,
        );
        let conflict = make_conflict(
            "acc-count-unresolved",
            serde_json::json!({"v": "local"}),
            serde_json::json!({"v": "remote"}),
            now,
            now,
        );
        let _ = manual_resolver.resolve_conflict(&conflict);

        assert_eq!(
            manual_resolver.count_unresolved().expect("count_unresolved"),
            1
        );
    }

    // ── set_strategy changes default ─────────────────────

    #[test]
    fn test_set_strategy_changes_default() {
        let mut resolver = make_resolver(ConflictResolutionStrategy::RemoteWins);

        let local = serde_json::json!({"name": "Local"});
        let remote = serde_json::json!({"name": "Remote"});
        let now = Utc::now();

        // With RemoteWins, remote should win.
        let conflict1 = make_conflict("acc-strat-1", local.clone(), remote.clone(), now, now);
        let resolved1 = resolver.resolve_conflict(&conflict1).expect("resolve_conflict");
        assert_eq!(resolved1.winner, ConflictWinner::Remote);

        // Change to LocalWins.
        resolver.set_strategy(ConflictResolutionStrategy::LocalWins);

        // Now local should win.
        let conflict2 = make_conflict("acc-strat-2", local, remote, now, now);
        let resolved2 = resolver.resolve_conflict(&conflict2).expect("resolve_conflict");
        assert_eq!(resolved2.winner, ConflictWinner::Local);
    }

    // ── resolve_batch processes multiple conflicts ───────

    #[test]
    fn test_resolve_batch() {
        let resolver = make_resolver(ConflictResolutionStrategy::RemoteWins);

        let now = Utc::now();
        let conflicts: Vec<SyncConflict> = (0..5)
            .map(|i| {
                make_conflict(
                    &format!("acc-batch-{}", i),
                    serde_json::json!({"v": "local"}),
                    serde_json::json!({"v": "remote"}),
                    now,
                    now,
                )
            })
            .collect();

        let results = resolver
            .resolve_batch(&conflicts)
            .expect("resolve_batch failed");

        assert_eq!(results.len(), 5, "Should resolve all 5 conflicts");
        for resolved in &results {
            assert_eq!(resolved.winner, ConflictWinner::Remote);
        }

        // Verify all are logged.
        let entries = resolver.get_conflicts().expect("get_conflicts");
        assert_eq!(entries.len(), 5);
    }

    // ── resolve_batch stops on Manual ────────────────────

    #[test]
    fn test_resolve_batch_stops_on_manual() {
        let resolver = make_resolver(ConflictResolutionStrategy::Manual);

        let now = Utc::now();
        let conflicts: Vec<SyncConflict> = (0..3)
            .map(|i| {
                make_conflict(
                    &format!("acc-batch-manual-{}", i),
                    serde_json::json!({"v": "local"}),
                    serde_json::json!({"v": "remote"}),
                    now,
                    now,
                )
            })
            .collect();

        let result = resolver.resolve_batch(&conflicts);
        assert!(
            matches!(result, Err(ConflictError::ManualRequired(_))),
            "Expected ManualRequired error, got: {:?}",
            result
        );
    }

    // ── conflict log preserves both versions ─────────────

    #[test]
    fn test_conflict_log_preserves_both_versions() {
        let resolver = make_resolver(ConflictResolutionStrategy::RemoteWins);

        let local = serde_json::json!({"name": "Local Version", "amount": 100});
        let remote = serde_json::json!({"name": "Remote Version", "amount": 200});
        let now = Utc::now();
        let conflict = make_conflict("acc-preserve-1", local, remote, now, now);

        let resolved = resolver
            .resolve_conflict(&conflict)
            .expect("resolve_conflict failed");

        // The loser (local) data should be preserved in losing_data.
        assert_eq!(
            resolved.losing_data,
            serde_json::json!({"name": "Local Version", "amount": 100})
        );
        assert_eq!(
            resolved.winning_data,
            serde_json::json!({"name": "Remote Version", "amount": 200})
        );

        // Verify the conflict_log entry has both versions.
        let entry = resolver
            .get_conflict(resolved.conflict_id)
            .expect("get_conflict failed");

        assert_eq!(
            entry.local_data,
            serde_json::json!({"name": "Local Version", "amount": 100})
        );
        assert_eq!(
            entry.remote_data,
            serde_json::json!({"name": "Remote Version", "amount": 200})
        );

        // winning_data and losing_data should also be present.
        assert!(entry.winning_data.is_some(), "winning_data should be present");
        assert!(entry.losing_data.is_some(), "losing_data should be present");
        assert_eq!(
            entry.winning_data.unwrap(),
            serde_json::json!({"name": "Remote Version", "amount": 200})
        );
        assert_eq!(
            entry.losing_data.unwrap(),
            serde_json::json!({"name": "Local Version", "amount": 100})
        );
    }

    // ── re-resolving a conflict updates, not duplicates ──

    #[test]
    fn test_re_resolving_updates_not_duplicates() {
        let resolver = make_resolver(ConflictResolutionStrategy::RemoteWins);

        let local = serde_json::json!({"name": "Local V1"});
        let remote = serde_json::json!({"name": "Remote V1"});
        let now = Utc::now();
        let conflict = make_conflict("acc-reresolve-1", local, remote, now, now);

        // First resolution.
        let resolved1 = resolver
            .resolve_conflict(&conflict)
            .expect("first resolve_conflict failed");
        let id1 = resolved1.conflict_id;

        // Second resolution of the same conflict (different strategy).
        let resolved2 = resolver
            .resolve_with_strategy(
                &conflict,
                ConflictResolutionStrategy::LocalWins,
            )
            .expect("second resolve_with_strategy failed");
        let id2 = resolved2.conflict_id;

        // Should be the same entry (updated, not duplicated).
        assert_eq!(id1, id2, "Re-resolving should update the same entry, not create a new one");

        // Verify only 1 entry in the log.
        let entries = resolver.get_conflicts().expect("get_conflicts");
        assert_eq!(entries.len(), 1, "Should have 1 entry, not 2");

        // Verify the entry reflects the latest resolution (LocalWins).
        let entry = &entries[0];
        assert_eq!(entry.resolution, ConflictResolutionStrategy::LocalWins);
        assert_eq!(entry.winner, Some(ConflictWinner::Local));
    }

    // ── conflict table creation is idempotent ────────────

    #[test]
    fn test_conflict_table_creation_is_idempotent() {
        let db = Arc::new(
            LocalDb::open_in_memory().expect("Failed to open in-memory database"),
        );

        // Create the resolver (which creates the table).
        let resolver1 = ConflictResolver::new(db.clone(), ConflictResolutionStrategy::RemoteWins);

        // Log a conflict.
        let now = Utc::now();
        let conflict = make_conflict(
            "acc-idem-1",
            serde_json::json!({"v": "local"}),
            serde_json::json!({"v": "remote"}),
            now,
            now,
        );
        let id1 = resolver1
            .log_conflict(&conflict)
            .expect("log_conflict failed");

        // Create a second resolver with the same DB — should not error
        // and should not drop existing data.
        let resolver2 = ConflictResolver::new(db.clone(), ConflictResolutionStrategy::RemoteWins);

        // The previously logged conflict should still be there.
        let entry = resolver2.get_conflict(id1).expect("get_conflict failed");
        assert_eq!(entry.entity_id, "acc-idem-1");

        // Verify the table exists by querying it.
        let entries = resolver2.get_conflicts().expect("get_conflicts");
        assert_eq!(entries.len(), 1, "Should still have 1 entry after re-creating table");

        // Explicitly call ensure_conflict_table again — should be a no-op.
        resolver2.ensure_conflict_table().expect("ensure_conflict_table should be idempotent");

        let entries_after = resolver2.get_conflicts().expect("get_conflicts after");
        assert_eq!(entries_after.len(), 1, "Data should be preserved after idempotent table creation");
    }

    // ── Default strategy is RemoteWins ───────────────────

    #[test]
    fn test_default_strategy_is_remote_wins() {
        assert_eq!(
            ConflictResolutionStrategy::default(),
            ConflictResolutionStrategy::RemoteWins
        );
    }

    // ── apply_resolution with LocalWins ──────────────────

    #[test]
    fn test_apply_resolution_local_wins() {
        let (resolver, store, _db) =
            make_resolver_and_store(ConflictResolutionStrategy::LocalWins);

        // Create and save an account locally.
        let account = make_test_account();
        store.save_account(&account).expect("save account");

        // Create a modified version (remote).
        let modified = make_modified_account(&account, "Remote Modified Name");
        let modified_json = serde_json::to_value(&modified).expect("serialize modified");
        let local_json = serde_json::to_value(&account).expect("serialize original");

        let conflict = make_conflict(
            &account.id.to_string(),
            local_json,
            modified_json,
            Utc::now(),
            Utc::now(),
        );

        // Resolve with LocalWins — local should win.
        let resolved = resolver
            .resolve_conflict(&conflict)
            .expect("resolve_conflict failed");

        assert_eq!(resolved.winner, ConflictWinner::Local);

        // Apply the resolution.
        resolver
            .apply_resolution(&resolved, &store)
            .expect("apply_resolution failed");

        // Verify the local store still has the original name.
        let retrieved = store
            .get_account(account.id)
            .expect("get_account failed");
        assert_eq!(
            retrieved.name,
            account.name,
            "Local version should be preserved after LocalWins resolution"
        );
    }

    // ── LastWriteWins with equal timestamps defaults to remote ─

    #[test]
    fn test_last_write_wins_equal_timestamps_defaults_remote() {
        let resolver = make_resolver(ConflictResolutionStrategy::LastWriteWins);

        let local = serde_json::json!({"name": "Local"});
        let remote = serde_json::json!({"name": "Remote"});
        let now = Utc::now();
        let conflict = make_conflict("acc-equal-ts", local, remote, now, now);

        let resolved = resolver
            .resolve_conflict(&conflict)
            .expect("resolve_conflict failed");

        // Equal timestamps — remote wins by default.
        assert_eq!(resolved.winner, ConflictWinner::Remote);
    }

    // ── Conflict log entry has correct metadata ──────────

    #[test]
    fn test_conflict_log_entry_metadata() {
        let resolver = make_resolver(ConflictResolutionStrategy::RemoteWins);

        let local = serde_json::json!({"name": "Local"});
        let remote = serde_json::json!({"name": "Remote"});
        let local_time = Utc::now();
        let remote_time = local_time + chrono::Duration::minutes(5);
        let conflict = make_conflict("acc-meta-1", local, remote, local_time, remote_time);

        let resolved = resolver
            .resolve_conflict(&conflict)
            .expect("resolve_conflict failed");

        let entry = resolver
            .get_conflict(resolved.conflict_id)
            .expect("get_conflict failed");

        assert_eq!(entry.entity_type, "account");
        assert_eq!(entry.entity_id, "acc-meta-1");
        assert_eq!(entry.local_modified_at, local_time);
        assert_eq!(entry.remote_modified_at, remote_time);
        assert_eq!(entry.resolution, ConflictResolutionStrategy::RemoteWins);
        assert_eq!(entry.winner, Some(ConflictWinner::Remote));
        assert!(entry.resolved_at.is_some());
        assert!(entry.created_at <= Utc::now());
    }
}
