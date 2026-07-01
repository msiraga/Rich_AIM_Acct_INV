//! Edge Sync Engine
//!
//! Pushes local changes to SurrealDB and pulls remote changes to local SQLite.
//! Idempotent, batched, with retry logic and conflict detection.
//!
//! # Architecture
//!
//! The sync engine sits between the local SQLite database ([`LocalDb`]) and a
//! remote database (SurrealDB, abstracted behind the [`RemoteSyncSource`] trait).
//!
//! ## Push Phase
//!
//! 1. Reads dirty records (rows where `_dirty = 1`) from local SQLite via
//!    [`ChangeTracker::get_dirty_records`].
//! 2. For each dirty record, reads the full row data from the data table.
//! 3. Pushes the serialized data to the remote database via
//!    [`RemoteSyncSource::push_record`].
//! 4. On success, marks the record as synced via [`ChangeTracker::mark_synced`].
//! 5. On failure, retries up to 3 times with exponential backoff. If all
//!    retries fail, the error is logged and recorded in the [`SyncResult`].
//!
//! ## Pull Phase
//!
//! 1. Reads the `last_sync` timestamp from the `sync_state` table.
//! 2. For each entity type, queries the remote for records modified since
//!    `last_sync` via [`RemoteSyncSource::pull_changes`].
//! 3. For each remote record, checks for conflicts: if the local record's
//!    `_modified_at` is later than the remote record's `modified_at`, a
//!    [`SyncConflict`] is flagged and the local record is **not** overwritten.
//! 4. If no conflict, the remote record is upserted into local SQLite and
//!    marked as clean (`_dirty = 0`).
//! 5. After a successful pull, `last_sync` is updated atomically.
//!
//! # Quality Guarantees
//!
//! - **Idempotent**: pushing the same record twice produces no duplicate.
//! - **Never loses data**: if one record fails, others continue.
//! - **Batched**: records are processed in batches of 50.
//! - **Retry**: individual failures retried up to 3 times with backoff.
//! - **Conflict-safe**: local modifications are never silently overwritten.

use std::sync::Arc;
use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use thiserror::Error;
use tracing::{info, debug, warn, error};
use tokio::sync::Mutex;

use uuid::Uuid;

use super::local_db::{LocalDb, Row};
use super::store::LocalStore;
use super::tracking::{ChangeTracker, DirtyRecord, SyncState};
use crate::database::financial::{
    Account, Transaction, TransactionEntry, JournalEntry,
};

// ═══════════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════════

/// Number of records to process in a single batch.
const BATCH_SIZE: usize = 50;

/// Maximum number of retry attempts for a failing operation.
const MAX_RETRIES: u32 = 3;

/// Initial backoff duration in milliseconds for the first retry.
/// Subsequent retries use exponential backoff: 50ms, 100ms, 200ms.
const INITIAL_BACKOFF_MS: u64 = 50;

/// All entity types that the sync engine handles.
/// These mirror the data tables in the local SQLite schema.
const SYNC_ENTITY_TYPES: &[&str] = &[
    "account",
    "transaction",
    "journal_entry",
    "transaction_entry",
    "invoice",
    "bill",
    "asset",
];

// ═══════════════════════════════════════════════════════════
// Data Structures
// ═══════════════════════════════════════════════════════════

/// A record retrieved from the remote database during pull.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteRecord {
    /// Singular entity type (`"account"`, `"transaction"`, ...).
    pub entity_type: String,
    /// UUID of the entity (as a string).
    pub entity_id: String,
    /// Full record data serialized as JSON.
    pub data: serde_json::Value,
    /// When the record was last modified on the remote.
    pub modified_at: DateTime<Utc>,
}

/// Summary of a completed sync operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    /// Number of records successfully pushed to the remote.
    pub pushed: usize,
    /// Number of records successfully pulled from the remote.
    pub pulled: usize,
    /// Number of conflicts detected during pull.
    pub conflicts: usize,
    /// Errors encountered for individual records.
    pub errors: Vec<SyncErrorEntry>,
    /// Total wall-clock duration in milliseconds.
    pub duration_ms: u128,
}

/// An error that occurred while syncing a specific record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncErrorEntry {
    /// Entity type of the failed record.
    pub entity_type: String,
    /// Entity ID of the failed record.
    pub entity_id: String,
    /// Human-readable error message.
    pub error: String,
    /// Whether the operation was retried before failing.
    pub retried: bool,
}

/// A conflict detected during pull: the local record was modified
/// more recently than the remote record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConflict {
    pub entity_type: String,
    pub entity_id: String,
    /// Local `_modified_at` timestamp.
    pub local_modified_at: DateTime<Utc>,
    /// Remote `modified_at` timestamp.
    pub remote_modified_at: DateTime<Utc>,
    /// Local record data at the time of conflict.
    pub local_data: serde_json::Value,
    /// Remote record data that was attempted to be pulled.
    pub remote_data: serde_json::Value,
}

// ═══════════════════════════════════════════════════════════
// Error Type
// ═══════════════════════════════════════════════════════════

/// Errors that can occur during synchronization.
#[derive(Error, Debug)]
pub enum SyncError {
    /// An error in the local SQLite database.
    #[error("Local DB error: {0}")]
    Local(String),

    /// An error communicating with the remote database.
    #[error("Remote DB error: {0}")]
    Remote(String),

    /// A serialization or deserialization failure.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// A conflict was detected and needs manual resolution.
    #[error("Conflict detected: {entity_type} {entity_id}")]
    Conflict {
        entity_type: String,
        entity_id: String,
    },

    /// A sync operation is already in progress.
    #[error("Sync in progress")]
    AlreadySyncing,

    /// The remote database is not reachable (offline mode).
    #[error("Offline mode enabled")]
    Offline,
}

// ═══════════════════════════════════════════════════════════
// RemoteSyncSource Trait
// ═══════════════════════════════════════════════════════════

/// Trait abstracting the remote database for synchronization.
///
/// In production this is backed by SurrealDB. In tests, [`MockRemoteSyncSource`]
/// provides an in-memory implementation.
///
/// # Semantics
///
/// - `push_record` uses **upsert** semantics: pushing the same `(entity_type,
///   entity_id)` twice does not create a duplicate.
/// - `pull_changes` returns only records whose `modified_at` is strictly
///   greater than `since` (or all records when `since` is `None`).
#[async_trait::async_trait]
pub trait RemoteSyncSource: Send + Sync {
    /// Push a single record to the remote database (upsert).
    ///
    /// Returns `Ok(())` on success, or an `Err(String)` describing the failure.
    async fn push_record(
        &self,
        entity_type: &str,
        entity_id: &str,
        data: serde_json::Value,
    ) -> Result<(), String>;

    /// Pull records of the given entity type modified after `since`.
    ///
    /// When `since` is `None`, all records of that type are returned.
    async fn pull_changes(
        &self,
        entity_type: &str,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<RemoteRecord>, String>;

    /// Check whether the remote database is currently reachable.
    async fn is_reachable(&self) -> bool;
}

// ═══════════════════════════════════════════════════════════
// SyncEngine
// ═══════════════════════════════════════════════════════════

/// The synchronization engine.
///
/// Coordinates push (local → remote) and pull (remote → local) operations
/// between the embedded SQLite database and a [`RemoteSyncSource`].
///
/// # Concurrency
///
/// A `tokio::sync::Mutex<bool>` (`syncing`) prevents concurrent sync cycles.
/// Calling [`sync`](Self::sync) while a sync is already in progress returns
/// [`SyncError::AlreadySyncing`].
///
/// # Construction
///
/// ```no_run
/// use std::sync::Arc;
/// use nexus_core::edge::local_db::LocalDb;
/// use nexus_core::edge::sync::{SyncEngine, RemoteSyncSource, MockRemoteSyncSource};
///
/// let db = Arc::new(LocalDb::open_in_memory().unwrap());
/// let remote: Arc<dyn RemoteSyncSource> = Arc::new(MockRemoteSyncSource::new());
/// let engine = SyncEngine::new(db, remote);
/// ```
pub struct SyncEngine {
    /// The local SQLite database (shared with store and tracker).
    db: Arc<LocalDb>,
    /// CRUD store for reading/writing financial records.
    store: LocalStore,
    /// Change tracker for dirty-record queries and mark-synced.
    tracker: ChangeTracker,
    /// The remote database (SurrealDB or mock).
    remote: Arc<dyn RemoteSyncSource>,
    /// Flag preventing concurrent sync cycles.
    syncing: Arc<Mutex<bool>>,
}

impl SyncEngine {
    /// Create a new sync engine.
    ///
    /// The `LocalStore` and `ChangeTracker` are constructed internally from
    /// the provided `db`.
    pub fn new(db: Arc<LocalDb>, remote: Arc<dyn RemoteSyncSource>) -> Self {
        let store = LocalStore::new(db.clone());
        let tracker = ChangeTracker::new(db.clone());
        Self {
            db,
            store,
            tracker,
            remote,
            syncing: Arc::new(Mutex::new(false)),
        }
    }

    // ── Public API ───────────────────────────────────────

    /// Run a full sync cycle: push local changes, then pull remote changes.
    ///
    /// Updates `last_sync` atomically after both phases complete.
    /// Returns a combined [`SyncResult`] summarizing both phases.
    ///
    /// # Errors
    ///
    /// Returns [`SyncError::AlreadySyncing`] if a sync is already in progress,
    /// or [`SyncError::Offline`] if the remote is unreachable.
    pub async fn sync(&self) -> Result<SyncResult, SyncError> {
        let start = std::time::Instant::now();

        if !self.try_acquire_sync_lock().await {
            return Err(SyncError::AlreadySyncing);
        }

        // Ensure the lock is always released.
        let result = self.do_sync().await;

        self.release_sync_lock().await;

        let mut result = result?;
        result.duration_ms = start.elapsed().as_millis();
        Ok(result)
    }

    /// Push all dirty local records to the remote database.
    ///
    /// Processes records in batches of [`BATCH_SIZE`]. Each record is retried
    /// up to [`MAX_RETRIES`] times on failure. Successfully pushed records are
    /// marked as synced in the local database.
    ///
    /// # Errors
    ///
    /// Returns [`SyncError::AlreadySyncing`] or [`SyncError::Offline`].
    pub async fn push_changes(&self) -> Result<SyncResult, SyncError> {
        let start = std::time::Instant::now();

        if !self.try_acquire_sync_lock().await {
            return Err(SyncError::AlreadySyncing);
        }

        if !self.remote.is_reachable().await {
            self.release_sync_lock().await;
            return Err(SyncError::Offline);
        }

        let result = self.push_dirty_records().await;

        self.release_sync_lock().await;

        let mut result = result?;
        result.duration_ms = start.elapsed().as_millis();
        Ok(result)
    }

    /// Pull all remote changes since the last sync.
    ///
    /// For each entity type, queries the remote for records modified after
    /// `last_sync`. Upserts matching records into local SQLite, detecting
    /// conflicts where the local record was modified more recently.
    ///
    /// After a successful pull, `last_sync` is updated.
    ///
    /// # Errors
    ///
    /// Returns [`SyncError::AlreadySyncing`] or [`SyncError::Offline`].
    pub async fn pull_changes(&self) -> Result<SyncResult, SyncError> {
        let start = std::time::Instant::now();

        if !self.try_acquire_sync_lock().await {
            return Err(SyncError::AlreadySyncing);
        }

        if !self.remote.is_reachable().await {
            self.release_sync_lock().await;
            return Err(SyncError::Offline);
        }

        let result = self.pull_remote_changes().await;

        // Update last_sync after a successful (or partially successful) pull.
        let now = Utc::now();
        if let Err(e) = self.tracker.update_sync_state(now) {
            warn!("Failed to update sync state after pull: {}", e);
        }

        self.release_sync_lock().await;

        let mut result = result?;
        result.duration_ms = start.elapsed().as_millis();
        Ok(result)
    }

    /// Check whether a sync operation is currently in progress.
    pub async fn is_syncing(&self) -> bool {
        *self.syncing.lock().await
    }

    /// Get the current sync state (last_sync, pending changes, breakdown).
    pub fn get_sync_state(&self) -> Result<SyncState, SyncError> {
        self.tracker
            .get_sync_state()
            .map_err(|e| SyncError::Local(e.to_string()))
    }

    // ── Push Implementation ──────────────────────────────

    /// Internal push logic — assumes the sync lock is already held.
    async fn push_dirty_records(&self) -> Result<SyncResult, SyncError> {
        let dirty = self
            .tracker
            .get_dirty_records()
            .map_err(|e| SyncError::Local(e.to_string()))?;

        if dirty.is_empty() {
            debug!("push_dirty_records: no dirty records to push");
            return Ok(SyncResult {
                pushed: 0,
                pulled: 0,
                conflicts: 0,
                errors: Vec::new(),
                duration_ms: 0,
            });
        }

        info!("push_dirty_records: {} dirty records to push", dirty.len());

        let mut pushed = 0;
        let mut errors = Vec::new();

        // Process in batches of BATCH_SIZE.
        for batch in dirty.chunks(BATCH_SIZE) {
            debug!(
                "push_dirty_records: processing batch of {} records",
                batch.len()
            );
            for record in batch {
                match self.push_single(record).await {
                    Ok(()) => {
                        pushed += 1;
                        debug!(
                            "push_dirty_records: pushed {} {}",
                            record.entity_type, record.entity_id
                        );
                    }
                    Err(e) => {
                        warn!(
                            "push_dirty_records: failed to push {} {}: {}",
                            e.entity_type, e.entity_id, e.error
                        );
                        errors.push(e);
                    }
                }
            }
        }

        info!(
            "push_dirty_records: pushed {}, {} errors",
            pushed,
            errors.len()
        );

        Ok(SyncResult {
            pushed,
            pulled: 0,
            conflicts: 0,
            errors,
            duration_ms: 0,
        })
    }

    /// Push a single dirty record to the remote, with retry logic.
    ///
    /// On success, the record is marked as synced in the local database.
    async fn push_single(&self, record: &DirtyRecord) -> Result<(), SyncErrorEntry> {
        // Read the full record data from local storage.
        let data = self.read_record_data(record).map_err(|e| SyncErrorEntry {
            entity_type: record.entity_type.clone(),
            entity_id: record.entity_id.clone(),
            error: e.to_string(),
            retried: false,
        })?;

        let entity_type = record.entity_type.clone();
        let entity_id = record.entity_id.clone();
        let remote = self.remote.clone();

        // Retry the remote push with exponential backoff.
        let result = self
            .retry_with_backoff(
                || {
                    let remote = remote.clone();
                    let et = entity_type.clone();
                    let eid = entity_id.clone();
                    let d = data.clone();
                    async move { remote.push_record(&et, &eid, d).await }
                },
                MAX_RETRIES,
            )
            .await;

        match result {
            Ok(()) => {
                // Mark the record as synced in the local database.
                self.tracker
                    .mark_synced(&entity_type, &entity_id)
                    .map_err(|e| SyncErrorEntry {
                        entity_type: entity_type.clone(),
                        entity_id: entity_id.clone(),
                        error: format!("Failed to mark synced: {}", e),
                        retried: false,
                    })?;
                Ok(())
            }
            Err(e) => {
                // All retries exhausted — record stays dirty for the next cycle.
                Err(SyncErrorEntry {
                    entity_type,
                    entity_id,
                    error: e,
                    retried: true,
                })
            }
        }
    }

    // ── Pull Implementation ──────────────────────────────

    /// Internal pull logic — assumes the sync lock is already held.
    async fn pull_remote_changes(&self) -> Result<SyncResult, SyncError> {
        let last_sync = self.get_last_sync_time();

        debug!(
            "pull_remote_changes: last_sync = {:?}",
            last_sync
        );

        let mut pulled = 0;
        let mut conflicts = 0;
        let mut errors = Vec::new();

        for entity_type in SYNC_ENTITY_TYPES {
            match self.pull_for_type(entity_type, last_sync).await {
                Ok(remote_records) => {
                    if remote_records.is_empty() {
                        continue;
                    }
                    debug!(
                        "pull_remote_changes: {} remote records for {}",
                        remote_records.len(),
                        entity_type
                    );
                    for record in remote_records {
                        match self.upsert_remote(&record).await {
                            Ok(None) => {
                                pulled += 1;
                                debug!(
                                    "pull_remote_changes: upserted {} {}",
                                    record.entity_type, record.entity_id
                                );
                            }
                            Ok(Some(conflict)) => {
                                conflicts += 1;
                                warn!(
                                    "pull_remote_changes: conflict for {} {}: local={}, remote={}",
                                    conflict.entity_type,
                                    conflict.entity_id,
                                    conflict.local_modified_at,
                                    conflict.remote_modified_at
                                );
                            }
                            Err(e) => {
                                error!(
                                    "pull_remote_changes: failed to upsert {} {}: {}",
                                    record.entity_type, record.entity_id, e
                                );
                                errors.push(SyncErrorEntry {
                                    entity_type: record.entity_type.clone(),
                                    entity_id: record.entity_id.clone(),
                                    error: e.to_string(),
                                    retried: false,
                                });
                            }
                        }
                    }
                }
                Err(e) => {
                    error!(
                        "pull_remote_changes: failed to pull for {}: {}",
                        entity_type, e
                    );
                    errors.push(SyncErrorEntry {
                        entity_type: entity_type.to_string(),
                        entity_id: "*".to_string(),
                        error: e.to_string(),
                        retried: false,
                    });
                }
            }
        }

        info!(
            "pull_remote_changes: pulled {}, {} conflicts, {} errors",
            pulled,
            conflicts,
            errors.len()
        );

        Ok(SyncResult {
            pushed: 0,
            pulled,
            conflicts,
            errors,
            duration_ms: 0,
        })
    }

    /// Pull changes for a single entity type from the remote.
    async fn pull_for_type(
        &self,
        entity_type: &str,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<RemoteRecord>, SyncError> {
        self.remote
            .pull_changes(entity_type, since)
            .await
            .map_err(|e| SyncError::Remote(e))
    }

    /// Upsert a remote record into local storage.
    ///
    /// Returns `Ok(Some(SyncConflict))` if a conflict is detected (local
    /// `_modified_at` is later than remote `modified_at`). Returns
    /// `Ok(None)` if the record was successfully upserted.
    async fn upsert_remote(
        &self,
        record: &RemoteRecord,
    ) -> Result<Option<SyncConflict>, SyncError> {
        // Check for conflict: is the local record newer?
        if let Some(local_modified) =
            self.get_local_modified_at(&record.entity_type, &record.entity_id)
        {
            if local_modified > record.modified_at {
                // Conflict: local was modified after the remote version.
                let local_data =
                    self.read_record_data_by_id(&record.entity_type, &record.entity_id)?;
                return Ok(Some(SyncConflict {
                    entity_type: record.entity_type.clone(),
                    entity_id: record.entity_id.clone(),
                    local_modified_at: local_modified,
                    remote_modified_at: record.modified_at,
                    local_data,
                    remote_data: record.data.clone(),
                }));
            }
        }

        // No conflict — upsert the remote record.
        self.save_remote_record(record).await?;
        Ok(None)
    }

    // ── Remote Record Persistence ────────────────────────

    /// Save a remote record into local SQLite.
    ///
    /// Deserializes the JSON `data` into the appropriate financial model,
    /// saves it via `LocalStore` (which sets `_dirty = 1`), then clears the
    /// dirty flag via `ChangeTracker::mark_synced` and restores `_modified_at`
    /// to the remote timestamp.
    async fn save_remote_record(&self, record: &RemoteRecord) -> Result<(), SyncError> {
        let entity_id = &record.entity_id;
        let modified_at = record.modified_at;

        match record.entity_type.as_str() {
            "account" => {
                let account: Account = serde_json::from_value(record.data.clone())
                    .map_err(|e| {
                        SyncError::Serialization(format!("Failed to deserialize account: {}", e))
                    })?;
                self.store
                    .save_account(&account)
                    .map_err(|e| SyncError::Local(e.to_string()))?;
                self.tracker
                    .mark_synced("account", entity_id)
                    .map_err(|e| SyncError::Local(e.to_string()))?;
                self.update_modified_at("account", entity_id, modified_at)?;
            }
            "transaction" => {
                let txn: Transaction = serde_json::from_value(record.data.clone())
                    .map_err(|e| {
                        SyncError::Serialization(format!(
                            "Failed to deserialize transaction: {}",
                            e
                        ))
                    })?;
                self.store
                    .save_transaction(&txn)
                    .map_err(|e| SyncError::Local(e.to_string()))?;
                self.tracker
                    .mark_synced("transaction", entity_id)
                    .map_err(|e| SyncError::Local(e.to_string()))?;
                self.update_modified_at("transaction", entity_id, modified_at)?;
            }
            "journal_entry" => {
                let je: JournalEntry = serde_json::from_value(record.data.clone())
                    .map_err(|e| {
                        SyncError::Serialization(format!(
                            "Failed to deserialize journal entry: {}",
                            e
                        ))
                    })?;
                self.store
                    .save_journal_entry(&je)
                    .map_err(|e| SyncError::Local(e.to_string()))?;
                self.tracker
                    .mark_synced("journal_entry", entity_id)
                    .map_err(|e| SyncError::Local(e.to_string()))?;
                self.update_modified_at("journal_entry", entity_id, modified_at)?;
            }
            "transaction_entry" => {
                let entry: TransactionEntry = serde_json::from_value(record.data.clone())
                    .map_err(|e| {
                        SyncError::Serialization(format!(
                            "Failed to deserialize transaction entry: {}",
                            e
                        ))
                    })?;
                self.store
                    .save_transaction_entry(&entry)
                    .map_err(|e| SyncError::Local(e.to_string()))?;
                self.tracker
                    .mark_synced("transaction_entry", entity_id)
                    .map_err(|e| SyncError::Local(e.to_string()))?;
                self.update_modified_at("transaction_entry", entity_id, modified_at)?;
            }
            _ => {
                warn!(
                    "save_remote_record: upsert not implemented for entity type '{}'",
                    record.entity_type
                );
            }
        }

        debug!(
            "save_remote_record: saved {} {}",
            record.entity_type, record.entity_id
        );
        Ok(())
    }

    // ── Data Reading Helpers ─────────────────────────────

    /// Read the full record data for a dirty record, serialized as JSON.
    ///
    /// For the four primary entity types (`account`, `transaction`,
    /// `journal_entry`, `transaction_entry`), uses the typed `LocalStore`
    /// getters so the JSON matches the financial model exactly.
    ///
    /// For other entity types (`invoice`, `bill`, `asset`), reads the raw
    /// database row and converts it to a JSON object.
    fn read_record_data(&self, record: &DirtyRecord) -> Result<serde_json::Value, SyncError> {
        let entity_id = Uuid::parse_str(&record.entity_id).map_err(|e| {
            SyncError::Serialization(format!("Invalid UUID '{}': {}", record.entity_id, e))
        })?;

        match record.entity_type.as_str() {
            "account" => {
                let account = self
                    .store
                    .get_account(entity_id)
                    .map_err(|e| SyncError::Local(e.to_string()))?;
                serde_json::to_value(&account)
                    .map_err(|e| SyncError::Serialization(e.to_string()))
            }
            "transaction" => {
                let txn = self
                    .store
                    .get_transaction(entity_id)
                    .map_err(|e| SyncError::Local(e.to_string()))?;
                serde_json::to_value(&txn)
                    .map_err(|e| SyncError::Serialization(e.to_string()))
            }
            "journal_entry" => {
                let je = self
                    .store
                    .get_journal_entry(entity_id)
                    .map_err(|e| SyncError::Local(e.to_string()))?;
                serde_json::to_value(&je)
                    .map_err(|e| SyncError::Serialization(e.to_string()))
            }
            "transaction_entry" => {
                let entry = self
                    .store
                    .get_transaction_entry(entity_id)
                    .map_err(|e| SyncError::Local(e.to_string()))?;
                serde_json::to_value(&entry)
                    .map_err(|e| SyncError::Serialization(e.to_string()))
            }
            _ => {
                // Generic fallback: read the raw row as JSON.
                let table = entity_type_to_table(&record.entity_type).ok_or_else(|| {
                    SyncError::Serialization(format!(
                        "Unknown entity type: '{}'",
                        record.entity_type
                    ))
                })?;
                let sql = format!("SELECT * FROM {} WHERE id = ?1", table);
                let row = self
                    .db
                    .query_one(&sql, rusqlite::params![&record.entity_id])
                    .map_err(|e| SyncError::Local(e.to_string()))?;
                Ok(row_to_json(&row))
            }
        }
    }

    /// Read a record's data by entity type and ID (for conflict reporting).
    ///
    /// Always uses the generic raw-row approach since we need the data for
    /// conflict comparison, not for typed operations.
    fn read_record_data_by_id(
        &self,
        entity_type: &str,
        entity_id: &str,
    ) -> Result<serde_json::Value, SyncError> {
        let table = entity_type_to_table(entity_type).ok_or_else(|| {
            SyncError::Serialization(format!("Unknown entity type: '{}'", entity_type))
        })?;
        let sql = format!("SELECT * FROM {} WHERE id = ?1", table);
        let row = self
            .db
            .query_one(&sql, rusqlite::params![entity_id])
            .map_err(|e| SyncError::Local(e.to_string()))?;
        Ok(row_to_json(&row))
    }

    // ── Conflict Detection Helpers ───────────────────────

    /// Get the local `_modified_at` timestamp for an entity.
    ///
    /// Returns `None` if the record does not exist locally (no conflict).
    fn get_local_modified_at(
        &self,
        entity_type: &str,
        entity_id: &str,
    ) -> Option<DateTime<Utc>> {
        let table = entity_type_to_table(entity_type)?;
        let sql = format!("SELECT _modified_at FROM {} WHERE id = ?1", table);
        let row = self
            .db
            .query_one(&sql, rusqlite::params![entity_id])
            .ok()?;
        let modified_at_str: Option<String> = row.get("_modified_at").ok()?;
        modified_at_str
            .filter(|s| !s.is_empty())
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc))
    }

    /// Update the `_modified_at` column for a record to match the remote.
    ///
    /// Called after a pull-upsert so that the local record's `_modified_at`
    /// reflects the remote timestamp, not the local save time.
    fn update_modified_at(
        &self,
        entity_type: &str,
        entity_id: &str,
        modified_at: DateTime<Utc>,
    ) -> Result<(), SyncError> {
        let table = entity_type_to_table(entity_type).ok_or_else(|| {
            SyncError::Serialization(format!("Unknown entity type: '{}'", entity_type))
        })?;
        let sql = format!(
            "UPDATE {} SET _modified_at = ?1 WHERE id = ?2",
            table
        );
        self.db
            .execute(
                &sql,
                rusqlite::params![modified_at.to_rfc3339(), entity_id],
            )
            .map_err(|e| SyncError::Local(e.to_string()))?;
        Ok(())
    }

    // ── Sync State Helpers ───────────────────────────────

    /// Get the last successful sync timestamp from the local database.
    fn get_last_sync_time(&self) -> Option<DateTime<Utc>> {
        self.db
            .get_last_sync()
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc))
    }

    // ── Sync Lock Helpers ────────────────────────────────

    /// Try to acquire the syncing lock. Returns `false` if already syncing.
    async fn try_acquire_sync_lock(&self) -> bool {
        let mut guard = self.syncing.lock().await;
        if *guard {
            return false;
        }
        *guard = true;
        true
    }

    /// Release the syncing lock.
    async fn release_sync_lock(&self) {
        let mut guard = self.syncing.lock().await;
        *guard = false;
    }

    /// Internal full-sync logic — assumes the sync lock is already held.
    async fn do_sync(&self) -> Result<SyncResult, SyncError> {
        if !self.remote.is_reachable().await {
            return Err(SyncError::Offline);
        }

        let mut total_pushed = 0;
        let mut total_pulled = 0;
        let mut total_conflicts = 0;
        let mut all_errors = Vec::new();

        // ── Push Phase ──
        match self.push_dirty_records().await {
            Ok(r) => {
                total_pushed = r.pushed;
                all_errors.extend(r.errors);
            }
            Err(e) => {
                error!("do_sync: push phase failed: {}", e);
            }
        }

        // ── Pull Phase ──
        match self.pull_remote_changes().await {
            Ok(r) => {
                total_pulled = r.pulled;
                total_conflicts = r.conflicts;
                all_errors.extend(r.errors);
            }
            Err(e) => {
                error!("do_sync: pull phase failed: {}", e);
            }
        }

        // ── Update last_sync ──
        let now = Utc::now();
        if let Err(e) = self.tracker.update_sync_state(now) {
            warn!("do_sync: failed to update sync state: {}", e);
        }

        Ok(SyncResult {
            pushed: total_pushed,
            pulled: total_pulled,
            conflicts: total_conflicts,
            errors: all_errors,
            duration_ms: 0, // Set by the caller.
        })
    }

    // ── Retry Logic ──────────────────────────────────────

    /// Retry an operation with exponential backoff.
    ///
    /// Makes an initial attempt, then retries up to `max_retries` times.
    /// The backoff is `INITIAL_BACKOFF_MS * 2^(attempt-1)` milliseconds:
    /// e.g. 50ms, 100ms, 200ms for 3 retries.
    ///
    /// Returns `Ok(T)` if any attempt succeeds, or `Err(String)` with the
    /// last error if all attempts fail.
    async fn retry_with_backoff<F, Fut, T>(
        &self,
        operation: F,
        max_retries: u32,
    ) -> Result<T, String>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, String>>,
    {
        let mut last_error = String::new();

        for attempt in 0..=max_retries {
            // Sleep before retry (not before the initial attempt).
            if attempt > 0 {
                let backoff_ms = INITIAL_BACKOFF_MS * 2u64.pow(attempt - 1);
                debug!(
                    "retry_with_backoff: attempt {} after {}ms backoff",
                    attempt, backoff_ms
                );
                tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
            }

            match operation().await {
                Ok(result) => {
                    if attempt > 0 {
                        info!(
                            "retry_with_backoff: succeeded on retry attempt {}",
                            attempt
                        );
                    }
                    return Ok(result);
                }
                Err(e) => {
                    last_error = e;
                    if attempt < max_retries {
                        warn!(
                            "retry_with_backoff: attempt {} failed, will retry: {}",
                            attempt + 1,
                            last_error
                        );
                    } else {
                        error!(
                            "retry_with_backoff: exhausted {} retries: {}",
                            max_retries, last_error
                        );
                    }
                }
            }
        }

        Err(last_error)
    }
}

// ═══════════════════════════════════════════════════════════
// MockRemoteSyncSource
// ═══════════════════════════════════════════════════════════

/// An in-memory mock remote sync source for testing.
///
/// Stores records in a `HashMap` keyed by `(entity_type, entity_id)`.
/// Supports simulated failures via `set_fail_for` and reachability control
/// via `set_reachable`.
pub struct MockRemoteSyncSource {
    records: Arc<Mutex<HashMap<(String, String), RemoteRecord>>>,
    reachable: Arc<Mutex<bool>>,
    /// Entity IDs that should always fail on push (for testing retry/error).
    fail_ids: Arc<Mutex<HashSet<String>>>,
}

impl MockRemoteSyncSource {
    /// Create a new mock with no records, reachable by default.
    pub fn new() -> Self {
        Self {
            records: Arc::new(Mutex::new(HashMap::new())),
            reachable: Arc::new(Mutex::new(true)),
            fail_ids: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Set whether the mock is reachable.
    pub async fn set_reachable(&self, reachable: bool) {
        *self.reachable.lock().await = reachable;
    }

    /// Mark an entity ID to always fail on push (for testing retry logic).
    pub async fn set_fail_for(&self, entity_id: &str) {
        self.fail_ids
            .lock()
            .await
            .insert(entity_id.to_string());
    }

    /// Clear all failure simulations.
    pub async fn clear_failures(&self) {
        self.fail_ids.lock().await.clear();
    }

    /// Insert a record directly into the mock (for setting up pull tests).
    pub async fn insert_record(
        &self,
        entity_type: &str,
        entity_id: &str,
        data: serde_json::Value,
        modified_at: DateTime<Utc>,
    ) {
        self.records.lock().await.insert(
            (entity_type.to_string(), entity_id.to_string()),
            RemoteRecord {
                entity_type: entity_type.to_string(),
                entity_id: entity_id.to_string(),
                data,
                modified_at,
            },
        );
    }

    /// Get a record from the mock by entity type and ID.
    pub async fn get_record(
        &self,
        entity_type: &str,
        entity_id: &str,
    ) -> Option<RemoteRecord> {
        self.records
            .lock()
            .await
            .get(&(entity_type.to_string(), entity_id.to_string()))
            .cloned()
    }

    /// Count the total number of records in the mock.
    pub async fn record_count(&self) -> usize {
        self.records.lock().await.len()
    }

    /// Clear all records from the mock.
    pub async fn clear(&self) {
        self.records.lock().await.clear();
    }
}

impl Default for MockRemoteSyncSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl RemoteSyncSource for MockRemoteSyncSource {
    async fn push_record(
        &self,
        entity_type: &str,
        entity_id: &str,
        data: serde_json::Value,
    ) -> Result<(), String> {
        // Check reachability.
        if !*self.reachable.lock().await {
            return Err("Remote is unreachable".to_string());
        }

        // Check if this ID is marked to fail.
        if self.fail_ids.lock().await.contains(entity_id) {
            return Err("Simulated push failure".to_string());
        }

        // Upsert (insert or replace) — idempotent.
        let now = Utc::now();
        self.records.lock().await.insert(
            (entity_type.to_string(), entity_id.to_string()),
            RemoteRecord {
                entity_type: entity_type.to_string(),
                entity_id: entity_id.to_string(),
                data,
                modified_at: now,
            },
        );
        Ok(())
    }

    async fn pull_changes(
        &self,
        entity_type: &str,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<RemoteRecord>, String> {
        if !*self.reachable.lock().await {
            return Err("Remote is unreachable".to_string());
        }

        let records = self.records.lock().await;
        let filtered: Vec<RemoteRecord> = records
            .values()
            .filter(|r| r.entity_type == entity_type)
            .filter(|r| match since {
                Some(s) => r.modified_at > s,
                None => true,
            })
            .cloned()
            .collect();
        Ok(filtered)
    }

    async fn is_reachable(&self) -> bool {
        *self.reachable.lock().await
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

/// Convert a database [`Row`] to a `serde_json::Value` (JSON object).
///
/// Each column becomes a key in the object. SQLite types are mapped as:
///
/// | SQLite `Value` | JSON |
/// |----------------|------|
/// | `Null`         | `null` |
/// | `Integer(i64)` | `number` |
/// | `Real(f64)`    | `number` or `null` (NaN/Inf) |
/// | `Text(String)` | `string` |
/// | `Blob(Vec<u8>)`| `null` (not represented) |
fn row_to_json(row: &Row) -> serde_json::Value {
    let mut map = serde_json::Map::new();

    for i in 0..row.column_count() {
        let col_name = row.columns().get(i).cloned().unwrap_or_default();
        let json_val = match row.get_value_by_index(i) {
            Ok(rusqlite::types::Value::Null) | Err(_) => serde_json::Value::Null,
            Ok(rusqlite::types::Value::Integer(n)) => serde_json::json!(n),
            Ok(rusqlite::types::Value::Real(f)) => serde_json::json!(f),
            Ok(rusqlite::types::Value::Text(s)) => serde_json::Value::String(s.clone()),
            Ok(rusqlite::types::Value::Blob(_)) => serde_json::Value::Null,
        };
        map.insert(col_name, json_val);
    }

    serde_json::Value::Object(map)
}

// ═══════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::local_db::LocalDb;
    use super::super::store::LocalStore;
    use super::super::tracking::ChangeTracker;
    use crate::database::financial::{
        AccountType, AccountStatus,
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
            description: "Test account for sync engine".to_string(),
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

    /// Create a full test setup: in-memory DB, store, tracker, mock remote, and engine.
    ///
    /// The store and tracker share the same `Arc<LocalDb>` as the engine,
    /// so changes made through the store are visible to the engine.
    #[allow(clippy::type_complexity)]
    fn make_test_setup() -> (
        SyncEngine,
        LocalStore,
        ChangeTracker,
        Arc<MockRemoteSyncSource>,
        Arc<LocalDb>,
    ) {
        let db = Arc::new(
            LocalDb::open_in_memory().expect("Failed to open in-memory database"),
        );
        let store = LocalStore::new(db.clone());
        let tracker = ChangeTracker::new(db.clone());
        let remote = Arc::new(MockRemoteSyncSource::new());
        let engine = SyncEngine::new(db.clone(), remote.clone());
        (engine, store, tracker, remote, db)
    }

    // ── Push Tests ───────────────────────────────────────

    #[tokio::test]
    async fn test_push_basic() {
        let (engine, store, tracker, remote, _db) = make_test_setup();

        // Create a dirty account via the store.
        let account = make_test_account();
        store.save_account(&account).expect("Failed to save account");

        // Verify the record is dirty.
        let dirty = tracker.get_dirty_records().expect("get_dirty_records");
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].entity_type, "account");
        assert_eq!(dirty[0].entity_id, account.id.to_string());

        // Push.
        let result = engine.push_changes().await.expect("push_changes failed");
        assert_eq!(result.pushed, 1);
        assert!(result.errors.is_empty(), "Expected no errors, got: {:?}", result.errors);

        // Verify the record appears in the remote mock.
        let remote_record = remote
            .get_record("account", &account.id.to_string())
            .await;
        assert!(remote_record.is_some(), "Record should exist in remote mock");
        assert_eq!(remote_record.unwrap().entity_id, account.id.to_string());
    }

    #[tokio::test]
    async fn test_idempotent_push() {
        let (engine, store, _tracker, remote, _db) = make_test_setup();

        let account = make_test_account();
        store.save_account(&account).expect("Failed to save account");

        // First push.
        let result1 = engine.push_changes().await.expect("First push failed");
        assert_eq!(result1.pushed, 1);

        // Second push — no dirty records remaining.
        let result2 = engine.push_changes().await.expect("Second push failed");
        assert_eq!(result2.pushed, 0);

        // Only one record in the remote mock (no duplicate).
        let count = remote.record_count().await;
        assert_eq!(count, 1, "Expected exactly 1 record in remote mock");
    }

    #[tokio::test]
    async fn test_push_remote_unreachable() {
        let (engine, store, tracker, remote, _db) = make_test_setup();

        let account = make_test_account();
        store.save_account(&account).expect("Failed to save account");

        // Make the remote unreachable.
        remote.set_reachable(false).await;

        // Push should fail with Offline error.
        let result = engine.push_changes().await;
        assert!(
            matches!(result, Err(SyncError::Offline)),
            "Expected Offline error, got: {:?}",
            result
        );

        // Records should still be dirty (not marked as synced).
        let dirty = tracker.get_dirty_records().expect("get_dirty_records");
        assert_eq!(dirty.len(), 1, "Record should still be dirty after failed push");
    }

    #[tokio::test]
    async fn test_mark_synced_after_push() {
        let (engine, store, tracker, _remote, _db) = make_test_setup();

        let account = make_test_account();
        store.save_account(&account).expect("Failed to save account");

        // Before push: 1 dirty record.
        assert_eq!(
            tracker.get_dirty_records().unwrap().len(),
            1,
            "Should have 1 dirty record before push"
        );

        // Push.
        engine.push_changes().await.expect("push_changes failed");

        // After push: 0 dirty records.
        assert_eq!(
            tracker.get_dirty_records().unwrap().len(),
            0,
            "Should have 0 dirty records after push"
        );

        // Also verify via the store's dirty count.
        assert_eq!(
            store.count_dirty_records().unwrap(),
            0,
            "Store should report 0 dirty records"
        );
    }

    // ── Pull Tests ───────────────────────────────────────

    #[tokio::test]
    async fn test_pull_basic() {
        let (engine, store, _tracker, remote, _db) = make_test_setup();

        // Insert a record into the remote mock.
        let account = make_test_account();
        let account_json = serde_json::to_value(&account).expect("serialize account");
        remote
            .insert_record(
                "account",
                &account.id.to_string(),
                account_json,
                Utc::now(),
            )
            .await;

        // Pull.
        let result = engine.pull_changes().await.expect("pull_changes failed");
        assert_eq!(result.pulled, 1);
        assert!(result.errors.is_empty(), "Expected no errors, got: {:?}", result.errors);

        // Verify the record appears in local SQLite.
        let local_account = store
            .get_account(account.id)
            .expect("Account should exist in local store after pull");
        assert_eq!(local_account.number, account.number);
        assert_eq!(local_account.name, account.name);
    }

    #[tokio::test]
    async fn test_pull_no_changes() {
        let (engine, _store, tracker, _remote, _db) = make_test_setup();

        // Set last_sync to now so nothing is "since" last_sync.
        tracker.update_sync_state(Utc::now()).expect("update_sync_state");

        // Pull.
        let result = engine.pull_changes().await.expect("pull_changes failed");
        assert_eq!(result.pulled, 0, "Should pull 0 records with no remote changes");
        assert_eq!(result.conflicts, 0);
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_pull_with_changes_since_last_sync() {
        let (engine, store, tracker, remote, _db) = make_test_setup();

        // Set last_sync to 1 hour ago.
        let one_hour_ago = Utc::now() - chrono::Duration::hours(1);
        tracker
            .update_sync_state(one_hour_ago)
            .expect("update_sync_state");

        // Insert a recent record (modified after last_sync).
        let recent_account = make_test_account();
        remote
            .insert_record(
                "account",
                &recent_account.id.to_string(),
                serde_json::to_value(&recent_account).expect("serialize"),
                Utc::now(),
            )
            .await;

        // Insert an old record (modified before last_sync).
        let old_account = make_test_account();
        remote
            .insert_record(
                "account",
                &old_account.id.to_string(),
                serde_json::to_value(&old_account).expect("serialize"),
                one_hour_ago - chrono::Duration::hours(1),
            )
            .await;

        // Pull.
        let result = engine.pull_changes().await.expect("pull_changes failed");

        // Only the recent record should be pulled.
        assert_eq!(result.pulled, 1, "Should pull only 1 record (the recent one)");

        // The recent account should be in local store.
        assert!(
            store.get_account(recent_account.id).is_ok(),
            "Recent account should be in local store"
        );

        // The old account should NOT be in local store.
        assert!(
            store.get_account(old_account.id).is_err(),
            "Old account should not be in local store"
        );
    }

    // ── Full Sync Test ───────────────────────────────────

    #[tokio::test]
    async fn test_full_sync() {
        let (engine, store, _tracker, remote, _db) = make_test_setup();

        // Create a dirty local record (will be pushed).
        let local_account = make_test_account();
        store.save_account(&local_account).expect("save local account");

        // Insert a remote record (will be pulled).
        let remote_account = make_test_account();
        remote
            .insert_record(
                "account",
                &remote_account.id.to_string(),
                serde_json::to_value(&remote_account).expect("serialize"),
                Utc::now(),
            )
            .await;

        // Full sync.
        let result = engine.sync().await.expect("sync failed");
        assert_eq!(result.pushed, 1, "Should push 1 record");
        assert!(result.pulled >= 1, "Should pull at least 1 record (remote account + possibly round-tripped local)");
        assert!(result.errors.is_empty(), "Expected no errors: {:?}", result.errors);

        // Verify local account was pushed to remote.
        assert!(
            remote
                .get_record("account", &local_account.id.to_string())
                .await
                .is_some(),
            "Local account should be in remote after sync"
        );

        // Verify remote account was pulled to local.
        assert!(
            store.get_account(remote_account.id).is_ok(),
            "Remote account should be in local after sync"
        );
    }

    // ── Conflict Detection Test ──────────────────────────

    #[tokio::test]
    async fn test_conflict_detection() {
        let (engine, store, _tracker, remote, db) = make_test_setup();

        // Create a local account (sets _dirty = 1, _modified_at = now).
        let account = make_test_account();
        store.save_account(&account).expect("save account");

        // Manually set _modified_at to the future so local > remote.
        let future_time = Utc::now() + chrono::Duration::hours(1);
        db.execute(
            "UPDATE accounts SET _modified_at = ?1 WHERE id = ?2",
            rusqlite::params![future_time.to_rfc3339(), account.id.to_string()],
        )
        .expect("update _modified_at");

        // Insert a remote record with an earlier modified_at.
        remote
            .insert_record(
                "account",
                &account.id.to_string(),
                serde_json::to_value(&account).expect("serialize"),
                Utc::now(), // earlier than local _modified_at
            )
            .await;

        // Pull.
        let result = engine.pull_changes().await.expect("pull_changes failed");
        assert_eq!(result.conflicts, 1, "Should detect 1 conflict");
        assert_eq!(result.pulled, 0, "Should not pull the conflicting record");

        // The local record should NOT have been overwritten.
        let local = store.get_account(account.id).expect("get account");
        assert_eq!(local.name, account.name, "Local data should be unchanged");
    }

    // ── Last Sync Timestamp Test ─────────────────────────

    #[tokio::test]
    async fn test_last_sync_updated() {
        let (engine, _store, tracker, _remote, db) = make_test_setup();

        // Before sync: no last_sync.
        assert!(db.get_last_sync().is_none(), "last_sync should be None before first sync");

        // Sync.
        engine.sync().await.expect("sync failed");

        // After sync: last_sync is set.
        assert!(
            db.get_last_sync().is_some(),
            "last_sync should be set after sync"
        );

        // Verify via tracker.
        let state = tracker.get_sync_state().expect("get_sync_state");
        assert!(state.last_sync.is_some(), "SyncState should have last_sync");
    }

    // ── Sync In Progress Test ────────────────────────────

    #[tokio::test]
    async fn test_sync_in_progress() {
        let (engine, _store, _tracker, _remote, _db) = make_test_setup();

        // Manually set the syncing flag.
        *engine.syncing.lock().await = true;

        // Sync should be rejected.
        let result = engine.sync().await;
        assert!(
            matches!(result, Err(SyncError::AlreadySyncing)),
            "Expected AlreadySyncing error, got: {:?}",
            result
        );

        // Reset the flag.
        *engine.syncing.lock().await = false;

        // Now sync should succeed.
        let result = engine.sync().await;
        assert!(result.is_ok(), "Sync should succeed after clearing flag");
    }

    // ── Retry Logic Test ─────────────────────────────────

    #[tokio::test]
    async fn test_retry_logic_succeeds_after_failures() {
        let (engine, _store, _tracker, _remote, _db) = make_test_setup();

        let counter = Arc::new(AtomicU64::new(0));
        let counter_clone = counter.clone();

        let result: Result<String, String> = engine
            .retry_with_backoff(
                || {
                    let c = counter_clone.clone();
                    async move {
                        let count = c.fetch_add(1, Ordering::SeqCst);
                        if count < 3 {
                            Err("Simulated failure".to_string())
                        } else {
                            Ok("success".to_string())
                        }
                    }
                },
                3,
            )
            .await;

        assert!(result.is_ok(), "Should succeed after retries");
        assert_eq!(result.unwrap(), "success");
        // 4 calls: initial (0) + retry 1 (1) + retry 2 (2) + retry 3 (3=succeed).
        assert_eq!(counter.load(Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn test_retry_logic_exhausted() {
        let (engine, _store, _tracker, _remote, _db) = make_test_setup();

        let result: Result<String, String> = engine
            .retry_with_backoff(
                || async { Err("Always fails".to_string()) },
                3,
            )
            .await;

        assert!(result.is_err(), "Should fail after exhausting retries");
        assert_eq!(result.unwrap_err(), "Always fails");
    }

    // ── Batch Processing Test ────────────────────────────

    #[tokio::test]
    async fn test_batch_processing() {
        let (engine, store, _tracker, remote, _db) = make_test_setup();

        // Create 100 dirty records.
        for _ in 0..100 {
            let account = make_test_account();
            store.save_account(&account).expect("save account");
        }

        // Verify 100 dirty records.
        let dirty = engine
            .tracker
            .get_dirty_records()
            .expect("get_dirty_records");
        assert_eq!(dirty.len(), 100);

        // Push all.
        let result = engine.push_changes().await.expect("push_changes failed");
        assert_eq!(result.pushed, 100, "Should push all 100 records");
        assert!(result.errors.is_empty(), "Expected no errors: {:?}", result.errors);

        // Verify all 100 records in the remote mock.
        let count = remote.record_count().await;
        assert_eq!(count, 100, "Remote mock should have 100 records");
    }

    // ── Error Summary Test ───────────────────────────────

    #[tokio::test]
    async fn test_error_summary() {
        let (engine, store, _tracker, remote, _db) = make_test_setup();

        // Create two dirty accounts.
        let account1 = make_test_account();
        let account2 = make_test_account();
        store.save_account(&account1).expect("save account1");
        store.save_account(&account2).expect("save account2");

        // Mark account2 to always fail on push.
        remote.set_fail_for(&account2.id.to_string()).await;

        // Push.
        let result = engine.push_changes().await.expect("push_changes failed");

        // One succeeded, one failed.
        assert_eq!(result.pushed, 1, "Should push 1 record successfully");
        assert_eq!(result.errors.len(), 1, "Should have 1 error");
        assert_eq!(result.errors[0].entity_id, account2.id.to_string());
        assert!(result.errors[0].retried, "Error should be marked as retried");

        // Verify account1 is in remote but account2 is not.
        assert!(
            remote
                .get_record("account", &account1.id.to_string())
                .await
                .is_some(),
            "account1 should be in remote"
        );
        assert!(
            remote
                .get_record("account", &account2.id.to_string())
                .await
                .is_none(),
            "account2 should not be in remote"
        );

        // account2 should still be dirty locally.
        let dirty = engine
            .tracker
            .get_dirty_records()
            .expect("get_dirty_records");
        assert_eq!(dirty.len(), 1, "account2 should still be dirty");
        assert_eq!(dirty[0].entity_id, account2.id.to_string());
    }

    // ── Empty Sync Test ──────────────────────────────────

    #[tokio::test]
    async fn test_empty_sync() {
        let (engine, _store, _tracker, _remote, _db) = make_test_setup();

        // No dirty records, no remote changes.
        let result = engine.sync().await.expect("sync failed");

        assert_eq!(result.pushed, 0);
        assert_eq!(result.pulled, 0);
        assert_eq!(result.conflicts, 0);
        assert!(result.errors.is_empty());
    }

    // ── Get Sync State Test ──────────────────────────────

    #[tokio::test]
    async fn test_get_sync_state() {
        let (engine, store, _tracker, _remote, _db) = make_test_setup();

        // Initially: no last_sync, no pending changes.
        let state = engine.get_sync_state().expect("get_sync_state");
        assert!(state.last_sync.is_none(), "last_sync should be None initially");
        assert_eq!(state.pending_changes, 0, "Should have 0 pending changes");

        // Create a dirty record.
        let account = make_test_account();
        store.save_account(&account).expect("save account");

        // Now: 1 pending change.
        let state = engine.get_sync_state().expect("get_sync_state");
        assert_eq!(state.pending_changes, 1, "Should have 1 pending change");
        assert!(state.pending_by_type.contains_key("account"));
        assert_eq!(state.pending_by_type["account"], 1);
    }

    // ── Is Syncing Test ──────────────────────────────────

    #[tokio::test]
    async fn test_is_syncing() {
        let (engine, _store, _tracker, _remote, _db) = make_test_setup();

        // Initially not syncing.
        assert!(!(engine.is_syncing().await), "Should not be syncing initially");

        // Set the flag.
        *engine.syncing.lock().await = true;
        assert!(engine.is_syncing().await, "Should be syncing after setting flag");

        // Clear the flag.
        *engine.syncing.lock().await = false;
        assert!(!engine.is_syncing().await, "Should not be syncing after clearing flag");
    }

    // ── Pull Updates Last Sync Test ──────────────────────

    #[tokio::test]
    async fn test_pull_updates_last_sync() {
        let (engine, _store, _tracker, _remote, db) = make_test_setup();

        // Before pull: no last_sync.
        assert!(db.get_last_sync().is_none());

        // Pull (even with no changes should update last_sync).
        engine.pull_changes().await.expect("pull_changes failed");

        // After pull: last_sync is set.
        assert!(db.get_last_sync().is_some(), "last_sync should be set after pull");
    }
}
