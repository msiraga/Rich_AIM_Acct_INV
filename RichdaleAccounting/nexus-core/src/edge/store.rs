//! Edge Local Store
//!
//! CRUD operations for offline-first accounting data.
//! Mirrors online ledger validation; all writes are marked dirty for sync.
//!
//! # Design
//!
//! - All `save_*` methods set `_dirty = 1` and `_modified_at = now()` on the
//!   saved record, then call `LocalDb::record_change()` to log the operation
//!   for the sync engine.
//! - `delete_*` methods perform a soft delete (set status to `Closed` or
//!   `Voided`) rather than removing the row, so historical data is preserved.
//! - Decimal values are stored as TEXT using `Decimal::to_string()` and
//!   `Decimal::from_str()`.
//! - DateTime values are stored as RFC 3339 TEXT.
//! - JSON fields (entries, metadata, document_ids, bank_details) are
//!   serialized as JSON TEXT via `serde_json`.
//! - UUID values are stored as TEXT.
//! - Transactions must be balanced (sum of debits = sum of credits) before
//!   saving; account and transaction numbers must be unique.

use std::sync::Arc;
use std::str::FromStr;
use chrono::{DateTime, Utc, NaiveDate};
use rust_decimal::Decimal;
use uuid::Uuid;
use serde::Serialize;
use thiserror::Error;
use tracing::{info, debug, warn, error};

use super::local_db::{LocalDb, LocalDbError, Row};
use crate::database::financial::{
    Account, AccountType, AccountStatus, BankAccountDetails,
    Transaction, TransactionType, TransactionStatus, TransactionEntry, EntryType,
    JournalEntry,
};

// ═══════════════════════════════════════════════════════════
// Error Type
// ═══════════════════════════════════════════════════════════

/// Errors that can occur during local store operations.
#[derive(Error, Debug)]
pub enum StoreError {
    /// Underlying SQLite/local database error.
    #[error("Local DB error: {0}")]
    Db(#[from] LocalDbError),
    /// Entity was not found by id or natural key.
    #[error("Not found: {0}")]
    NotFound(String),
    /// A business-rule validation failed (unbalanced transaction, etc.).
    #[error("Validation error: {0}")]
    Validation(String),
    /// Serialization or deserialization of a field failed.
    #[error("Serialization error: {0}")]
    Serialization(String),
    /// A uniqueness constraint was violated (duplicate number, etc.).
    #[error("Duplicate: {0}")]
    Duplicate(String),
}

// ═══════════════════════════════════════════════════════════
// LocalStore
// ═══════════════════════════════════════════════════════════

/// Offline-first CRUD store backed by a [`LocalDb`] (SQLite).
///
/// Wraps an `Arc<LocalDb>` so it can be cheaply cloned for sharing across
/// threads or tasks.  Every write marks the affected row `_dirty = 1` and
/// records a change-log entry for the sync engine to pick up.
#[derive(Debug, Clone)]
pub struct LocalStore {
    db: Arc<LocalDb>,
}

impl LocalStore {
    /// Create a new store wrapping the given local database.
    pub fn new(db: Arc<LocalDb>) -> Self {
        Self { db }
    }

    // ── Account CRUD ─────────────────────────────────────

    /// Save (insert or update) an account.
    ///
    /// Validates that the account number is unique (excluding the account's
    /// own id).  Sets `_dirty = 1` and `_modified_at = now()`, then records
    /// a change-log entry.
    pub fn save_account(&self, account: &Account) -> Result<(), StoreError> {
        let id_str = account.id.to_string();

        // Check for duplicate account number (different id)
        let duplicate = self.db.query_one(
            "SELECT id FROM accounts WHERE number = ?1 AND id != ?2",
            rusqlite::params![account.number, id_str],
        );
        match duplicate {
            Ok(_) => {
                warn!("Duplicate account number: {}", account.number);
                return Err(StoreError::Duplicate(format!(
                    "Account number '{}' already exists",
                    account.number
                )));
            }
            Err(LocalDbError::NotFound) => {} // good — no conflict
            Err(e) => return Err(StoreError::Db(e)),
        }

        // Determine whether this is an insert or an update
        let exists = self
            .db
            .query_one(
                "SELECT id FROM accounts WHERE id = ?1",
                rusqlite::params![id_str],
            )
            .is_ok();
        let operation = if exists { "update" } else { "insert" };

        // Prepare serialized values
        let account_type_str = enum_to_str(&account.account_type)?;
        let status_str = enum_to_str(&account.status)?;
        let parent_id = account.parent_id.map(|u| u.to_string());
        let balance_str = account.balance.to_string();
        let bank_details_json = account
            .bank_details
            .as_ref()
            .map(|bd| serde_json::to_string(bd))
            .transpose()
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let last_reconciled_str = account.last_reconciled.map(|dt| dt.to_rfc3339());
        let created_at_str = account.created_at.to_rfc3339();
        let updated_at_str = account.updated_at.to_rfc3339();
        let modified_at = Utc::now().to_rfc3339();

        let sql = "INSERT OR REPLACE INTO accounts \
            (id, number, name, description, account_type, parent_id, status, balance, currency, \
             is_bank_account, bank_details, is_reconciled, last_reconciled, created_at, updated_at, \
             _dirty, _modified_at) \
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, 1, ?16)";

        self.db.execute(
            sql,
            rusqlite::params![
                id_str,
                account.number,
                account.name,
                account.description,
                account_type_str,
                parent_id,
                status_str,
                balance_str,
                account.currency,
                account.is_bank_account,
                bank_details_json,
                account.is_reconciled,
                last_reconciled_str,
                created_at_str,
                updated_at_str,
                modified_at,
            ],
        )?;

        self.db.record_change("account", &id_str, operation)?;

        info!("Saved account {} ({})", account.number, id_str);
        Ok(())
    }

    /// Retrieve an account by its UUID.
    pub fn get_account(&self, id: Uuid) -> Result<Account, StoreError> {
        let id_str = id.to_string();
        let row = self.db.query_one(
            "SELECT * FROM accounts WHERE id = ?1",
            rusqlite::params![id_str],
        ).map_err(|e| match e {
            LocalDbError::NotFound => StoreError::NotFound(format!("Account {} not found", id_str)),
            e => StoreError::Db(e),
        })?;
        row_to_account(&row)
    }

    /// Retrieve an account by its number (natural key).
    pub fn get_account_by_number(&self, number: &str) -> Result<Account, StoreError> {
        let row = self.db.query_one(
            "SELECT * FROM accounts WHERE number = ?1",
            rusqlite::params![number],
        ).map_err(|e| match e {
            LocalDbError::NotFound => StoreError::NotFound(format!("Account with number '{}' not found", number)),
            e => StoreError::Db(e),
        })?;
        row_to_account(&row)
    }

    /// List all accounts, ordered by account number.
    pub fn list_accounts(&self) -> Result<Vec<Account>, StoreError> {
        let rows = self.db.query_all(
            "SELECT * FROM accounts ORDER BY number",
            &[],
        )?;
        rows.iter().map(|r| row_to_account(r)).collect()
    }

    /// List all accounts of a given type, ordered by account number.
    pub fn list_accounts_by_type(&self, account_type: &AccountType) -> Result<Vec<Account>, StoreError> {
        let type_str = enum_to_str(account_type)?;
        let rows = self.db.query_all(
            "SELECT * FROM accounts WHERE account_type = ?1 ORDER BY number",
            rusqlite::params![type_str],
        )?;
        rows.iter().map(|r| row_to_account(r)).collect()
    }

    /// Soft-delete an account: set status to `Closed`, mark dirty, record change.
    ///
    /// The row is preserved for historical/audit purposes.
    pub fn delete_account(&self, id: Uuid) -> Result<(), StoreError> {
        let id_str = id.to_string();
        let now = Utc::now().to_rfc3339();
        let closed_str = enum_to_str(&AccountStatus::Closed)?;

        let affected = self.db.execute(
            "UPDATE accounts SET status = ?1, _dirty = 1, _modified_at = ?2, updated_at = ?3 \
             WHERE id = ?4",
            rusqlite::params![closed_str, now, now, id_str],
        )?;

        if affected == 0 {
            return Err(StoreError::NotFound(format!("Account {} not found", id_str)));
        }

        self.db.record_change("account", &id_str, "delete")?;
        info!("Soft-deleted account {}", id_str);
        Ok(())
    }

    // ── Transaction CRUD ─────────────────────────────────

    /// Save (insert or update) a transaction.
    ///
    /// Validates that:
    /// - The transaction is balanced (sum of debits = sum of credits).
    /// - The transaction number is unique (excluding the transaction's own id).
    ///
    /// Sets `_dirty = 1` and `_modified_at = now()`, then records a change-log
    /// entry.
    pub fn save_transaction(&self, txn: &Transaction) -> Result<(), StoreError> {
        // Validate: transaction must be balanced
        if !txn.is_balanced() {
            warn!(
                "Unbalanced transaction {}: debits={}, credits={}",
                txn.number,
                txn.entries.iter().filter(|e| e.entry_type == EntryType::Debit).map(|e| e.amount).sum::<Decimal>(),
                txn.entries.iter().filter(|e| e.entry_type == EntryType::Credit).map(|e| e.amount).sum::<Decimal>(),
            );
            return Err(StoreError::Validation(format!(
                "Transaction '{}' is not balanced (debits={}, credits={})",
                txn.number,
                txn.entries.iter().filter(|e| e.entry_type == EntryType::Debit).map(|e| e.amount).sum::<Decimal>(),
                txn.entries.iter().filter(|e| e.entry_type == EntryType::Credit).map(|e| e.amount).sum::<Decimal>(),
            )));
        }

        let id_str = txn.id.to_string();

        // Check for duplicate transaction number (different id)
        let duplicate = self.db.query_one(
            "SELECT id FROM transactions WHERE number = ?1 AND id != ?2",
            rusqlite::params![txn.number, id_str],
        );
        match duplicate {
            Ok(_) => {
                warn!("Duplicate transaction number: {}", txn.number);
                return Err(StoreError::Duplicate(format!(
                    "Transaction number '{}' already exists",
                    txn.number
                )));
            }
            Err(LocalDbError::NotFound) => {}
            Err(e) => return Err(StoreError::Db(e)),
        }

        // Determine insert vs update
        let exists = self
            .db
            .query_one(
                "SELECT id FROM transactions WHERE id = ?1",
                rusqlite::params![id_str],
            )
            .is_ok();
        let operation = if exists { "update" } else { "insert" };

        // Prepare serialized values
        let txn_type_str = enum_to_str(&txn.transaction_type)?;
        let status_str = enum_to_str(&txn.status)?;
        let entries_json = serde_json::to_string(&txn.entries)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let journal_entry_id = txn.journal_entry_id.map(|u| u.to_string());
        let document_ids_json = serde_json::to_string(&txn.document_ids)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let metadata_json = serde_json::to_string(&txn.metadata)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let date_str = txn.date.to_rfc3339();
        let created_at_str = txn.created_at.to_rfc3339();
        let updated_at_str = txn.updated_at.to_rfc3339();
        let modified_at = Utc::now().to_rfc3339();

        let sql = "INSERT OR REPLACE INTO transactions \
            (id, number, description, date, transaction_type, status, entries, journal_entry_id, \
             document_ids, metadata, created_at, updated_at, _dirty, _modified_at) \
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1, ?13)";

        self.db.execute(
            sql,
            rusqlite::params![
                id_str,
                txn.number,
                txn.description,
                date_str,
                txn_type_str,
                status_str,
                entries_json,
                journal_entry_id,
                document_ids_json,
                metadata_json,
                created_at_str,
                updated_at_str,
                modified_at,
            ],
        )?;

        self.db.record_change("transaction", &id_str, operation)?;

        info!("Saved transaction {} ({})", txn.number, id_str);
        Ok(())
    }

    /// Retrieve a transaction by its UUID.
    pub fn get_transaction(&self, id: Uuid) -> Result<Transaction, StoreError> {
        let id_str = id.to_string();
        let row = self.db.query_one(
            "SELECT * FROM transactions WHERE id = ?1",
            rusqlite::params![id_str],
        ).map_err(|e| match e {
            LocalDbError::NotFound => StoreError::NotFound(format!("Transaction {} not found", id_str)),
            e => StoreError::Db(e),
        })?;
        row_to_transaction(&row)
    }

    /// List all transactions, ordered by date descending.
    pub fn list_transactions(&self) -> Result<Vec<Transaction>, StoreError> {
        let rows = self.db.query_all(
            "SELECT * FROM transactions ORDER BY date DESC",
            &[],
        )?;
        rows.iter().map(|r| row_to_transaction(r)).collect()
    }

    /// List transactions within a date range (inclusive), ordered by date.
    pub fn list_transactions_by_date_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Transaction>, StoreError> {
        let start_str = start.to_rfc3339();
        let end_str = end.to_rfc3339();
        let rows = self.db.query_all(
            "SELECT * FROM transactions WHERE date >= ?1 AND date <= ?2 ORDER BY date",
            rusqlite::params![start_str, end_str],
        )?;
        rows.iter().map(|r| row_to_transaction(r)).collect()
    }

    /// Soft-delete a transaction: set status to `Voided`, mark dirty, record change.
    pub fn delete_transaction(&self, id: Uuid) -> Result<(), StoreError> {
        let id_str = id.to_string();
        let now = Utc::now().to_rfc3339();
        let voided_str = enum_to_str(&TransactionStatus::Voided)?;

        let affected = self.db.execute(
            "UPDATE transactions SET status = ?1, _dirty = 1, _modified_at = ?2, updated_at = ?3 \
             WHERE id = ?4",
            rusqlite::params![voided_str, now, now, id_str],
        )?;

        if affected == 0 {
            return Err(StoreError::NotFound(format!("Transaction {} not found", id_str)));
        }

        self.db.record_change("transaction", &id_str, "delete")?;
        info!("Soft-deleted transaction {}", id_str);
        Ok(())
    }

    // ── JournalEntry CRUD ────────────────────────────────

    /// Save (insert or update) a journal entry.
    ///
    /// Validates that the journal entry is balanced (sum of debits = sum of
    /// credits).  Sets `_dirty = 1` and `_modified_at = now()`, then records
    /// a change-log entry.
    pub fn save_journal_entry(&self, entry: &JournalEntry) -> Result<(), StoreError> {
        // Validate: journal entry must be balanced
        if !entry.is_balanced() {
            warn!(
                "Unbalanced journal entry {}: debits={}, credits={}",
                entry.number, entry.total_debits(), entry.total_credits()
            );
            return Err(StoreError::Validation(format!(
                "Journal entry '{}' is not balanced (debits={}, credits={})",
                entry.number, entry.total_debits(), entry.total_credits()
            )));
        }

        let id_str = entry.id.to_string();

        // Determine insert vs update
        let exists = self
            .db
            .query_one(
                "SELECT id FROM journal_entries WHERE id = ?1",
                rusqlite::params![id_str],
            )
            .is_ok();
        let operation = if exists { "update" } else { "insert" };

        // Prepare serialized values
        let date_str = entry.date.to_string();
        let entries_json = serde_json::to_string(&entry.entries)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        let posted_at_str = entry.posted_at.map(|dt| dt.to_rfc3339());
        let created_at_str = entry.created_at.to_rfc3339();
        let updated_at_str = entry.updated_at.to_rfc3339();
        let modified_at = Utc::now().to_rfc3339();

        let sql = "INSERT OR REPLACE INTO journal_entries \
            (id, number, date, description, reference, entries, is_posted, posted_at, \
             is_reconciled, created_at, updated_at, _dirty, _modified_at) \
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1, ?12)";

        self.db.execute(
            sql,
            rusqlite::params![
                id_str,
                entry.number,
                date_str,
                entry.description,
                entry.reference,
                entries_json,
                entry.is_posted,
                posted_at_str,
                entry.is_reconciled,
                created_at_str,
                updated_at_str,
                modified_at,
            ],
        )?;

        self.db.record_change("journal_entry", &id_str, operation)?;

        info!("Saved journal entry {} ({})", entry.number, id_str);
        Ok(())
    }

    /// Retrieve a journal entry by its UUID.
    pub fn get_journal_entry(&self, id: Uuid) -> Result<JournalEntry, StoreError> {
        let id_str = id.to_string();
        let row = self.db.query_one(
            "SELECT * FROM journal_entries WHERE id = ?1",
            rusqlite::params![id_str],
        ).map_err(|e| match e {
            LocalDbError::NotFound => StoreError::NotFound(format!("Journal entry {} not found", id_str)),
            e => StoreError::Db(e),
        })?;
        row_to_journal_entry(&row)
    }

    /// List all journal entries, ordered by date descending.
    pub fn list_journal_entries(&self) -> Result<Vec<JournalEntry>, StoreError> {
        let rows = self.db.query_all(
            "SELECT * FROM journal_entries ORDER BY date DESC",
            &[],
        )?;
        rows.iter().map(|r| row_to_journal_entry(r)).collect()
    }

    // ── TransactionEntry CRUD ────────────────────────────

    /// Save (insert or update) a standalone transaction entry.
    ///
    /// The `transaction_id` and `journal_entry_id` columns are set to NULL
    /// since they are not part of the [`TransactionEntry`] model.  Sets
    /// `_dirty = 1` and `_modified_at = now()`, then records a change-log entry.
    pub fn save_transaction_entry(&self, entry: &TransactionEntry) -> Result<(), StoreError> {
        let id_str = entry.id.to_string();

        // Determine insert vs update
        let exists = self
            .db
            .query_one(
                "SELECT id FROM transaction_entries WHERE id = ?1",
                rusqlite::params![id_str],
            )
            .is_ok();
        let operation = if exists { "update" } else { "insert" };

        // Prepare serialized values
        let account_id_str = entry.account_id.to_string();
        let entry_type_str = enum_to_str(&entry.entry_type)?;
        let amount_str = entry.amount.to_string();
        let exchange_rate_str = entry.exchange_rate.map(|d| d.to_string());
        let base_currency_amount_str = entry.base_currency_amount.map(|d| d.to_string());
        let now = Utc::now();
        let created_at_str = now.to_rfc3339();
        let updated_at_str = now.to_rfc3339();
        let modified_at = now.to_rfc3339();

        let sql = "INSERT OR REPLACE INTO transaction_entries \
            (id, account_id, transaction_id, journal_entry_id, entry_type, amount, description, \
             reference, currency, exchange_rate, base_currency_amount, created_at, updated_at, \
             _dirty, _modified_at) \
            VALUES (?1, ?2, NULL, NULL, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1, ?12)";

        self.db.execute(
            sql,
            rusqlite::params![
                id_str,
                account_id_str,
                entry_type_str,
                amount_str,
                entry.description,
                entry.reference,
                entry.currency,
                exchange_rate_str,
                base_currency_amount_str,
                created_at_str,
                updated_at_str,
                modified_at,
            ],
        )?;

        self.db.record_change("transaction_entry", &id_str, operation)?;

        debug!("Saved transaction entry {}", id_str);
        Ok(())
    }

    /// Retrieve a transaction entry by its UUID.
    pub fn get_transaction_entry(&self, id: Uuid) -> Result<TransactionEntry, StoreError> {
        let id_str = id.to_string();
        let row = self.db.query_one(
            "SELECT * FROM transaction_entries WHERE id = ?1",
            rusqlite::params![id_str],
        ).map_err(|e| match e {
            LocalDbError::NotFound => StoreError::NotFound(format!("Transaction entry {} not found", id_str)),
            e => StoreError::Db(e),
        })?;
        row_to_transaction_entry(&row)
    }

    /// List all transaction entries for a given account, ordered by creation date.
    pub fn list_entries_for_account(&self, account_id: Uuid) -> Result<Vec<TransactionEntry>, StoreError> {
        let account_id_str = account_id.to_string();
        let rows = self.db.query_all(
            "SELECT * FROM transaction_entries WHERE account_id = ?1 ORDER BY created_at",
            rusqlite::params![account_id_str],
        )?;
        rows.iter().map(|r| row_to_transaction_entry(r)).collect()
    }

    // ── Utility ──────────────────────────────────────────

    /// Count the total number of dirty (unsynced) records across all data tables.
    pub fn count_dirty_records(&self) -> Result<usize, StoreError> {
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

    /// Mark a specific entity as clean (synced).
    ///
    /// `entity_type` must be one of: `account`, `transaction`, `journal_entry`,
    /// `transaction_entry`, `invoice`, `bill`, `asset`.
    pub fn mark_clean(&self, entity_type: &str, entity_id: &str) -> Result<(), StoreError> {
        let table = match entity_type {
            "account" => "accounts",
            "transaction" => "transactions",
            "journal_entry" => "journal_entries",
            "transaction_entry" => "transaction_entries",
            "invoice" => "invoices",
            "bill" => "bills",
            "asset" => "assets",
            _ => {
                return Err(StoreError::Validation(format!(
                    "Unknown entity type: '{}'",
                    entity_type
                )));
            }
        };

        let sql = format!("UPDATE {} SET _dirty = 0 WHERE id = ?1", table);
        let affected = self.db.execute(&sql, rusqlite::params![entity_id])?;

        if affected == 0 {
            debug!("mark_clean: no rows affected for {} {}", entity_type, entity_id);
        }

        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════
// Private Serialization Helpers
// ═══════════════════════════════════════════════════════════

/// Serialize a serde-serializable enum to its variant name string.
///
/// `serde_json::to_string` on a unit variant like `AccountType::Asset`
/// produces `"Asset"` (with surrounding quotes).  We strip the quotes so the
/// database stores clean values like `Asset`, `Active`, etc.
fn enum_to_str<T: Serialize>(val: &T) -> Result<String, StoreError> {
    let s = serde_json::to_string(val)
        .map_err(|e| StoreError::Serialization(e.to_string()))?;
    // serde_json wraps unit variants in quotes — strip them
    Ok(s.trim_matches('"').to_string())
}

/// Deserialize a serde-deserializable enum from its variant name string.
///
/// Re-wraps the string in quotes to produce valid JSON, then deserializes.
fn str_to_enum<T>(s: &str) -> Result<T, StoreError>
where
    T: serde::de::DeserializeOwned,
{
    let json = format!("\"{}\"", s);
    serde_json::from_str(&json)
        .map_err(|e| StoreError::Serialization(format!("Failed to deserialize enum from '{}': {}", s, e)))
}

/// Parse a `Decimal` from its string representation.
fn parse_decimal(s: &str) -> Result<Decimal, StoreError> {
    Decimal::from_str(s).map_err(|e| {
        StoreError::Serialization(format!("Failed to parse decimal '{}': {}", s, e))
    })
}

/// Parse a `DateTime<Utc>` from an RFC 3339 string.
fn parse_datetime(s: &str) -> Result<DateTime<Utc>, StoreError> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| StoreError::Serialization(format!("Failed to parse datetime '{}': {}", s, e)))
}

/// Parse a `Uuid` from its string representation.
fn parse_uuid(s: &str) -> Result<Uuid, StoreError> {
    Uuid::parse_str(s).map_err(|e| {
        StoreError::Serialization(format!("Failed to parse UUID '{}': {}", s, e))
    })
}

/// Parse a `NaiveDate` from a `YYYY-MM-DD` string.
fn parse_naive_date(s: &str) -> Result<NaiveDate, StoreError> {
    NaiveDate::from_str(s).map_err(|e| {
        StoreError::Serialization(format!("Failed to parse date '{}': {}", s, e))
    })
}

// ── Row Deserialization ─────────────────────────────────

/// Deserialize an [`Account`] from a database [`Row`].
fn row_to_account(row: &Row) -> Result<Account, StoreError> {
    let id_str: String = row.get("id")?;
    let account_type_str: String = row.get("account_type")?;
    let status_str: String = row.get("status")?;
    let parent_id_str: Option<String> = row.get("parent_id")?;
    let balance_str: String = row.get("balance")?;
    let bank_details_str: Option<String> = row.get("bank_details")?;
    let last_reconciled_str: Option<String> = row.get("last_reconciled")?;
    let created_at_str: String = row.get("created_at")?;
    let updated_at_str: String = row.get("updated_at")?;

    Ok(Account {
        id: parse_uuid(&id_str)?,
        number: row.get("number")?,
        name: row.get("name")?,
        description: row.get("description")?,
        account_type: str_to_enum(&account_type_str)?,
        parent_id: parent_id_str.map(|s| parse_uuid(&s)).transpose()?,
        status: str_to_enum(&status_str)?,
        balance: parse_decimal(&balance_str)?,
        currency: row.get("currency")?,
        is_bank_account: row.get("is_bank_account")?,
        bank_details: bank_details_str
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(|e| StoreError::Serialization(e.to_string()))?,
        is_reconciled: row.get("is_reconciled")?,
        last_reconciled: last_reconciled_str.map(|s| parse_datetime(&s)).transpose()?,
        created_at: parse_datetime(&created_at_str)?,
        updated_at: parse_datetime(&updated_at_str)?,
    })
}

/// Deserialize a [`Transaction`] from a database [`Row`].
fn row_to_transaction(row: &Row) -> Result<Transaction, StoreError> {
    let id_str: String = row.get("id")?;
    let date_str: String = row.get("date")?;
    let txn_type_str: String = row.get("transaction_type")?;
    let status_str: String = row.get("status")?;
    let entries_str: String = row.get("entries")?;
    let journal_entry_id_str: Option<String> = row.get("journal_entry_id")?;
    let document_ids_str: String = row.get("document_ids")?;
    let metadata_str: String = row.get("metadata")?;
    let created_at_str: String = row.get("created_at")?;
    let updated_at_str: String = row.get("updated_at")?;

    Ok(Transaction {
        id: parse_uuid(&id_str)?,
        number: row.get("number")?,
        description: row.get("description")?,
        date: parse_datetime(&date_str)?,
        transaction_type: str_to_enum(&txn_type_str)?,
        status: str_to_enum(&status_str)?,
        entries: serde_json::from_str(&entries_str)
            .map_err(|e| StoreError::Serialization(e.to_string()))?,
        journal_entry_id: journal_entry_id_str.map(|s| parse_uuid(&s)).transpose()?,
        document_ids: serde_json::from_str(&document_ids_str)
            .map_err(|e| StoreError::Serialization(e.to_string()))?,
        metadata: serde_json::from_str(&metadata_str)
            .map_err(|e| StoreError::Serialization(e.to_string()))?,
        created_at: parse_datetime(&created_at_str)?,
        updated_at: parse_datetime(&updated_at_str)?,
    })
}

/// Deserialize a [`JournalEntry`] from a database [`Row`].
fn row_to_journal_entry(row: &Row) -> Result<JournalEntry, StoreError> {
    let id_str: String = row.get("id")?;
    let date_str: String = row.get("date")?;
    let entries_str: String = row.get("entries")?;
    let posted_at_str: Option<String> = row.get("posted_at")?;
    let created_at_str: String = row.get("created_at")?;
    let updated_at_str: String = row.get("updated_at")?;

    Ok(JournalEntry {
        id: parse_uuid(&id_str)?,
        number: row.get("number")?,
        date: parse_naive_date(&date_str)?,
        description: row.get("description")?,
        reference: row.get("reference")?,
        entries: serde_json::from_str(&entries_str)
            .map_err(|e| StoreError::Serialization(e.to_string()))?,
        is_posted: row.get("is_posted")?,
        posted_at: posted_at_str.map(|s| parse_datetime(&s)).transpose()?,
        is_reconciled: row.get("is_reconciled")?,
        created_at: parse_datetime(&created_at_str)?,
        updated_at: parse_datetime(&updated_at_str)?,
    })
}

/// Deserialize a [`TransactionEntry`] from a database [`Row`].
fn row_to_transaction_entry(row: &Row) -> Result<TransactionEntry, StoreError> {
    let id_str: String = row.get("id")?;
    let account_id_str: String = row.get("account_id")?;
    let entry_type_str: String = row.get("entry_type")?;
    let amount_str: String = row.get("amount")?;
    let exchange_rate_str: Option<String> = row.get("exchange_rate")?;
    let base_currency_amount_str: Option<String> = row.get("base_currency_amount")?;

    Ok(TransactionEntry {
        id: parse_uuid(&id_str)?,
        account_id: parse_uuid(&account_id_str)?,
        entry_type: str_to_enum(&entry_type_str)?,
        amount: parse_decimal(&amount_str)?,
        description: row.get("description")?,
        reference: row.get("reference")?,
        currency: row.get("currency")?,
        exchange_rate: exchange_rate_str.map(|s| parse_decimal(&s)).transpose()?,
        base_currency_amount: base_currency_amount_str.map(|s| parse_decimal(&s)).transpose()?,
    })
}

// ═══════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    // ── Test Helpers ──────────────────────────────────────

    /// Create a `LocalStore` backed by an in-memory SQLite database.
    fn make_store() -> LocalStore {
        let db = Arc::new(
            LocalDb::open_in_memory().expect("Failed to open in-memory database"),
        );
        LocalStore::new(db)
    }

    /// Create a basic non-bank account for testing.
    fn make_account() -> Account {
        Account {
            id: Uuid::new_v4(),
            number: "1000".to_string(),
            name: "Cash".to_string(),
            description: "Operating cash account".to_string(),
            account_type: AccountType::Asset,
            parent_id: Some(Uuid::new_v4()),
            status: AccountStatus::Active,
            balance: dec!(12345.67),
            currency: "USD".to_string(),
            is_bank_account: false,
            bank_details: None,
            is_reconciled: true,
            last_reconciled: Some(Utc::now()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// Create a bank account with full bank details for testing.
    fn make_bank_account() -> Account {
        let bank_details = BankAccountDetails {
            bank_name: "First National Bank".to_string(),
            account_number: "1234567890".to_string(),
            routing_number: "021000021".to_string(),
            address: "123 Main St, Anytown, USA".to_string(),
            phone: "555-123-4567".to_string(),
            website: "www.firstnational.com".to_string(),
            account_holder: "Richdale Accounting LLC".to_string(),
            account_type: "checking".to_string(),
            currency: "USD".to_string(),
        };
        Account {
            id: Uuid::new_v4(),
            number: "1010".to_string(),
            name: "Operating Bank Account".to_string(),
            description: "Main checking account".to_string(),
            account_type: AccountType::Asset,
            parent_id: None,
            status: AccountStatus::Active,
            balance: dec!(50000.00),
            currency: "USD".to_string(),
            is_bank_account: true,
            bank_details: Some(bank_details),
            is_reconciled: false,
            last_reconciled: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// Create a balanced transaction with entries, metadata, and document_ids.
    fn make_transaction() -> Transaction {
        let acct = Uuid::new_v4();
        let entries = vec![
            TransactionEntry::new(acct, EntryType::Debit, dec!(100), "Debit entry"),
            TransactionEntry::new(acct, EntryType::Credit, dec!(100), "Credit entry"),
        ];
        let mut txn = Transaction::new("Test transaction".to_string(), Utc::now(), entries);
        txn.id = Uuid::new_v4();
        txn.number = "TXN-001".to_string();
        txn.transaction_type = TransactionType::Payment;
        txn.status = TransactionStatus::Posted;
        txn.document_ids = vec!["DOC-001".to_string(), "DOC-002".to_string()];
        txn.metadata = serde_json::json!({"customer": "Acme Corp", "amount": 100});
        txn
    }

    /// Create an unbalanced transaction (debits != credits).
    fn make_unbalanced_transaction() -> Transaction {
        let acct = Uuid::new_v4();
        let entries = vec![
            TransactionEntry::new(acct, EntryType::Debit, dec!(100), "Debit"),
            TransactionEntry::new(acct, EntryType::Credit, dec!(50), "Credit"),
        ];
        let mut txn = Transaction::new("Unbalanced".to_string(), Utc::now(), entries);
        txn.id = Uuid::new_v4();
        txn.number = "TXN-UNBAL-001".to_string();
        txn
    }

    /// Create a balanced journal entry with a reference.
    fn make_journal_entry() -> JournalEntry {
        let mut je = JournalEntry::new(
            "Test journal entry",
            NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(),
        );
        je.id = Uuid::new_v4();
        je.number = "JE-001".to_string();
        je.reference = Some("REF-001".to_string());
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

    /// Create a transaction entry with multi-currency fields.
    fn make_transaction_entry() -> TransactionEntry {
        TransactionEntry {
            id: Uuid::new_v4(),
            account_id: Uuid::new_v4(),
            entry_type: EntryType::Debit,
            amount: dec!(1000.00),
            description: "Multi-currency test entry".to_string(),
            reference: Some("REF-MC-001".to_string()),
            currency: "EUR".to_string(),
            exchange_rate: Some(dec!(1.0876)),
            base_currency_amount: Some(dec!(1087.60)),
        }
    }

    // ── Account Round-Trip Tests ──────────────────────────

    #[test]
    fn test_account_round_trip_without_bank_details() {
        let store = make_store();
        let account = make_account();

        store.save_account(&account).expect("Failed to save account");
        let retrieved = store.get_account(account.id).expect("Failed to get account");

        assert_eq!(retrieved.id, account.id);
        assert_eq!(retrieved.number, account.number);
        assert_eq!(retrieved.name, account.name);
        assert_eq!(retrieved.description, account.description);
        assert_eq!(retrieved.account_type, account.account_type);
        assert_eq!(retrieved.parent_id, account.parent_id);
        assert_eq!(retrieved.status, account.status);
        assert_eq!(retrieved.balance, account.balance);
        assert_eq!(retrieved.currency, account.currency);
        assert_eq!(retrieved.is_bank_account, account.is_bank_account);
        assert!(retrieved.bank_details.is_none());
        assert_eq!(retrieved.is_reconciled, account.is_reconciled);
        assert!(retrieved.last_reconciled.is_some());
        assert_eq!(retrieved.created_at, account.created_at);
        assert_eq!(retrieved.updated_at, account.updated_at);
    }

    #[test]
    fn test_account_round_trip_with_bank_details() {
        let store = make_store();
        let account = make_bank_account();

        store.save_account(&account).expect("Failed to save account");
        let retrieved = store.get_account(account.id).expect("Failed to get account");

        assert_eq!(retrieved.id, account.id);
        assert_eq!(retrieved.number, account.number);
        assert_eq!(retrieved.is_bank_account, true);
        assert!(retrieved.bank_details.is_some());

        let bd = retrieved.bank_details.unwrap();
        let orig = account.bank_details.unwrap();
        assert_eq!(bd.bank_name, orig.bank_name);
        assert_eq!(bd.account_number, orig.account_number);
        assert_eq!(bd.routing_number, orig.routing_number);
        assert_eq!(bd.address, orig.address);
        assert_eq!(bd.phone, orig.phone);
        assert_eq!(bd.website, orig.website);
        assert_eq!(bd.account_holder, orig.account_holder);
        assert_eq!(bd.account_type, orig.account_type);
        assert_eq!(bd.currency, orig.currency);
    }

    #[test]
    fn test_get_account_by_number() {
        let store = make_store();
        let account = make_account();

        store.save_account(&account).expect("Failed to save");
        let retrieved = store
            .get_account_by_number(&account.number)
            .expect("Failed to get by number");

        assert_eq!(retrieved.id, account.id);
        assert_eq!(retrieved.number, account.number);
    }

    // ── Transaction Round-Trip Tests ──────────────────────

    #[test]
    fn test_transaction_round_trip() {
        let store = make_store();
        let txn = make_transaction();

        store.save_transaction(&txn).expect("Failed to save transaction");
        let retrieved = store.get_transaction(txn.id).expect("Failed to get transaction");

        assert_eq!(retrieved.id, txn.id);
        assert_eq!(retrieved.number, txn.number);
        assert_eq!(retrieved.description, txn.description);
        assert_eq!(retrieved.date, txn.date);
        assert_eq!(retrieved.transaction_type, txn.transaction_type);
        assert_eq!(retrieved.status, txn.status);
        assert_eq!(retrieved.entries.len(), txn.entries.len());
        assert_eq!(retrieved.entries[0].amount, txn.entries[0].amount);
        assert_eq!(retrieved.entries[0].entry_type, txn.entries[0].entry_type);
        assert_eq!(retrieved.entries[1].amount, txn.entries[1].amount);
        assert_eq!(retrieved.entries[1].entry_type, txn.entries[1].entry_type);
        assert_eq!(retrieved.journal_entry_id, txn.journal_entry_id);
        assert_eq!(retrieved.document_ids, txn.document_ids);
        assert_eq!(retrieved.metadata, txn.metadata);
        assert_eq!(retrieved.created_at, txn.created_at);
        assert_eq!(retrieved.updated_at, txn.updated_at);
    }

    // ── JournalEntry Round-Trip Tests ─────────────────────

    #[test]
    fn test_journal_entry_round_trip() {
        let store = make_store();
        let je = make_journal_entry();

        store.save_journal_entry(&je).expect("Failed to save journal entry");
        let retrieved = store.get_journal_entry(je.id).expect("Failed to get journal entry");

        assert_eq!(retrieved.id, je.id);
        assert_eq!(retrieved.number, je.number);
        assert_eq!(retrieved.date, je.date);
        assert_eq!(retrieved.description, je.description);
        assert_eq!(retrieved.reference, je.reference);
        assert_eq!(retrieved.entries.len(), je.entries.len());
        assert_eq!(retrieved.entries[0].amount, je.entries[0].amount);
        assert_eq!(retrieved.entries[0].entry_type, je.entries[0].entry_type);
        assert_eq!(retrieved.entries[1].amount, je.entries[1].amount);
        assert_eq!(retrieved.entries[1].entry_type, je.entries[1].entry_type);
        assert_eq!(retrieved.is_posted, je.is_posted);
        assert_eq!(retrieved.posted_at, je.posted_at);
        assert_eq!(retrieved.is_reconciled, je.is_reconciled);
        assert_eq!(retrieved.created_at, je.created_at);
        assert_eq!(retrieved.updated_at, je.updated_at);
    }

    // ── TransactionEntry Round-Trip Tests ─────────────────

    #[test]
    fn test_transaction_entry_round_trip() {
        let store = make_store();
        let entry = make_transaction_entry();

        store
            .save_transaction_entry(&entry)
            .expect("Failed to save transaction entry");
        let retrieved = store
            .get_transaction_entry(entry.id)
            .expect("Failed to get transaction entry");

        assert_eq!(retrieved.id, entry.id);
        assert_eq!(retrieved.account_id, entry.account_id);
        assert_eq!(retrieved.entry_type, entry.entry_type);
        assert_eq!(retrieved.amount, entry.amount);
        assert_eq!(retrieved.description, entry.description);
        assert_eq!(retrieved.reference, entry.reference);
        assert_eq!(retrieved.currency, entry.currency);
        assert_eq!(retrieved.exchange_rate, entry.exchange_rate);
        assert_eq!(retrieved.base_currency_amount, entry.base_currency_amount);
    }

    // ── Validation Tests ──────────────────────────────────

    #[test]
    fn test_unbalanced_transaction_rejected() {
        let store = make_store();
        let txn = make_unbalanced_transaction();

        let result = store.save_transaction(&txn);
        assert!(result.is_err());
        match result {
            Err(StoreError::Validation(msg)) => {
                assert!(
                    msg.contains("balanced"),
                    "Error should mention balanced: {}",
                    msg
                );
            }
            Err(e) => panic!("Expected Validation error, got: {:?}", e),
            Ok(_) => panic!("Expected error for unbalanced transaction"),
        }
    }

    #[test]
    fn test_duplicate_account_number_rejected() {
        let store = make_store();
        let account1 = make_account();
        store.save_account(&account1).expect("Failed to save first account");

        // Same number, different id and name
        let account2 = Account {
            id: Uuid::new_v4(),
            number: account1.number.clone(),
            name: "Different Account".to_string(),
            ..make_account()
        };

        let result = store.save_account(&account2);
        assert!(result.is_err());
        match result {
            Err(StoreError::Duplicate(msg)) => {
                assert!(
                    msg.contains("1000"),
                    "Error should mention the number: {}",
                    msg
                );
            }
            Err(e) => panic!("Expected Duplicate error, got: {:?}", e),
            Ok(_) => panic!("Expected error for duplicate account number"),
        }
    }

    #[test]
    fn test_duplicate_transaction_number_rejected() {
        let store = make_store();
        let txn1 = make_transaction();
        store.save_transaction(&txn1).expect("Failed to save first transaction");

        // Same number, different id
        let txn2 = make_transaction();
        let mut txn2 = txn2;
        txn2.id = Uuid::new_v4(); // different id, same number "TXN-001"

        let result = store.save_transaction(&txn2);
        assert!(result.is_err());
        match result {
            Err(StoreError::Duplicate(msg)) => {
                assert!(
                    msg.contains("TXN-001"),
                    "Error should mention the number: {}",
                    msg
                );
            }
            Err(e) => panic!("Expected Duplicate error, got: {:?}", e),
            Ok(_) => panic!("Expected error for duplicate transaction number"),
        }
    }

    #[test]
    fn test_unbalanced_journal_entry_rejected() {
        let store = make_store();

        let mut je = JournalEntry::new("Unbalanced JE", NaiveDate::from_ymd_opt(2026, 7, 1).unwrap());
        je.id = Uuid::new_v4();
        je.number = "JE-UNBAL".to_string();
        je.add_entry(TransactionEntry::new(
            Uuid::new_v4(),
            EntryType::Debit,
            dec!(100),
            "Debit",
        ));
        je.add_entry(TransactionEntry::new(
            Uuid::new_v4(),
            EntryType::Credit,
            dec!(50),
            "Credit",
        ));

        let result = store.save_journal_entry(&je);
        assert!(result.is_err());
        match result {
            Err(StoreError::Validation(msg)) => {
                assert!(msg.contains("balanced"), "Error should mention balanced: {}", msg);
            }
            Err(e) => panic!("Expected Validation error, got: {:?}", e),
            Ok(_) => panic!("Expected error for unbalanced journal entry"),
        }
    }

    // ── List / Filter Tests ───────────────────────────────

    #[test]
    fn test_list_accounts() {
        let store = make_store();

        let a1 = Account::new("1000", "Cash", AccountType::Asset);
        let a2 = Account::new("2000", "Accounts Payable", AccountType::Liability);
        let a3 = Account::new("3000", "Revenue", AccountType::Revenue);

        store.save_account(&a1).expect("save 1");
        store.save_account(&a2).expect("save 2");
        store.save_account(&a3).expect("save 3");

        let accounts = store.list_accounts().expect("list_accounts failed");
        assert_eq!(accounts.len(), 3);
        // Ordered by number
        assert_eq!(accounts[0].number, "1000");
        assert_eq!(accounts[1].number, "2000");
        assert_eq!(accounts[2].number, "3000");
    }

    #[test]
    fn test_list_accounts_by_type() {
        let store = make_store();

        let a1 = Account::new("1000", "Cash", AccountType::Asset);
        let a2 = Account::new("1500", "Inventory", AccountType::Asset);
        let a3 = Account::new("2000", "Accounts Payable", AccountType::Liability);

        store.save_account(&a1).expect("save 1");
        store.save_account(&a2).expect("save 2");
        store.save_account(&a3).expect("save 3");

        let assets = store
            .list_accounts_by_type(&AccountType::Asset)
            .expect("list by type failed");
        assert_eq!(assets.len(), 2);
        assert!(assets.iter().all(|a| a.account_type == AccountType::Asset));

        let liabilities = store
            .list_accounts_by_type(&AccountType::Liability)
            .expect("list by type failed");
        assert_eq!(liabilities.len(), 1);
        assert_eq!(liabilities[0].number, "2000");
    }

    #[test]
    fn test_list_transactions() {
        let store = make_store();

        let t1 = make_transaction();
        store.save_transaction(&t1).expect("save 1");

        let mut t2 = make_transaction();
        t2.id = Uuid::new_v4();
        t2.number = "TXN-002".to_string();
        store.save_transaction(&t2).expect("save 2");

        let txns = store.list_transactions().expect("list_transactions failed");
        assert_eq!(txns.len(), 2);
    }

    #[test]
    fn test_list_transactions_by_date_range() {
        let store = make_store();

        let mut t1 = make_transaction();
        t1.number = "TXN-001".to_string();
        t1.date = DateTime::parse_from_rfc3339("2026-01-15T10:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let mut t2 = make_transaction();
        t2.id = Uuid::new_v4();
        t2.number = "TXN-002".to_string();
        t2.date = DateTime::parse_from_rfc3339("2026-06-15T10:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let mut t3 = make_transaction();
        t3.id = Uuid::new_v4();
        t3.number = "TXN-003".to_string();
        t3.date = DateTime::parse_from_rfc3339("2026-12-15T10:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        store.save_transaction(&t1).expect("save 1");
        store.save_transaction(&t2).expect("save 2");
        store.save_transaction(&t3).expect("save 3");

        let start = DateTime::parse_from_rfc3339("2026-03-01T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        let end = DateTime::parse_from_rfc3339("2026-09-01T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let txns = store
            .list_transactions_by_date_range(start, end)
            .expect("list by date range failed");
        assert_eq!(txns.len(), 1);
        assert_eq!(txns[0].number, "TXN-002");
    }

    #[test]
    fn test_list_journal_entries() {
        let store = make_store();

        let je1 = make_journal_entry();
        store.save_journal_entry(&je1).expect("save 1");

        let mut je2 = make_journal_entry();
        je2.id = Uuid::new_v4();
        je2.number = "JE-002".to_string();
        store.save_journal_entry(&je2).expect("save 2");

        let entries = store.list_journal_entries().expect("list failed");
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_list_entries_for_account() {
        let store = make_store();
        let account_id = Uuid::new_v4();

        let mut e1 = make_transaction_entry();
        e1.account_id = account_id;
        store.save_transaction_entry(&e1).expect("save 1");

        let mut e2 = make_transaction_entry();
        e2.id = Uuid::new_v4();
        e2.account_id = account_id;
        store.save_transaction_entry(&e2).expect("save 2");

        // Entry for a different account
        let e3 = make_transaction_entry();
        store.save_transaction_entry(&e3).expect("save 3");

        let entries = store
            .list_entries_for_account(account_id)
            .expect("list entries failed");
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|e| e.account_id == account_id));
    }

    // ── Soft Delete Tests ─────────────────────────────────

    #[test]
    fn test_soft_delete_account() {
        let store = make_store();
        let account = make_account();
        store.save_account(&account).expect("Failed to save account");

        // After save, 1 dirty record
        let dirty_before = store.count_dirty_records().expect("count failed");
        assert_eq!(dirty_before, 1);

        // Soft-delete
        store.delete_account(account.id).expect("Failed to delete account");

        // Account should still exist
        let deleted = store
            .get_account(account.id)
            .expect("Account should still exist after soft delete");
        assert_eq!(deleted.status, AccountStatus::Closed);

        // Still 1 dirty record (same row, _dirty=1)
        let dirty_after = store.count_dirty_records().expect("count failed");
        assert_eq!(dirty_after, 1);
    }

    #[test]
    fn test_soft_delete_transaction() {
        let store = make_store();
        let txn = make_transaction();
        store.save_transaction(&txn).expect("Failed to save transaction");

        // Soft-delete
        store
            .delete_transaction(txn.id)
            .expect("Failed to delete transaction");

        // Transaction should still exist
        let deleted = store
            .get_transaction(txn.id)
            .expect("Transaction should still exist after soft delete");
        assert_eq!(deleted.status, TransactionStatus::Voided);
    }

    // ── Dirty / Clean Utility Tests ───────────────────────

    #[test]
    fn test_count_dirty_records() {
        let store = make_store();

        // Initially 0 dirty records
        let count = store.count_dirty_records().expect("count failed");
        assert_eq!(count, 0);

        // Save an account -> 1 dirty
        let account = make_account();
        store.save_account(&account).expect("save account");
        assert_eq!(store.count_dirty_records().unwrap(), 1);

        // Save a transaction -> 2 dirty
        let txn = make_transaction();
        store.save_transaction(&txn).expect("save txn");
        assert_eq!(store.count_dirty_records().unwrap(), 2);

        // Save a journal entry -> 3 dirty
        let je = make_journal_entry();
        store.save_journal_entry(&je).expect("save je");
        assert_eq!(store.count_dirty_records().unwrap(), 3);
    }

    #[test]
    fn test_mark_clean() {
        let store = make_store();
        let account = make_account();
        store.save_account(&account).expect("save account");

        // 1 dirty record
        assert_eq!(store.count_dirty_records().unwrap(), 1);

        // Mark clean
        store
            .mark_clean("account", &account.id.to_string())
            .expect("mark_clean failed");

        // 0 dirty records
        assert_eq!(store.count_dirty_records().unwrap(), 0);
    }

    #[test]
    fn test_mark_clean_unknown_entity_type() {
        let store = make_store();
        let result = store.mark_clean("unknown_type", "some-id");
        assert!(result.is_err());
        match result {
            Err(StoreError::Validation(msg)) => {
                assert!(msg.contains("unknown_type"));
            }
            Err(e) => panic!("Expected Validation error, got: {:?}", e),
            Ok(_) => panic!("Expected error for unknown entity type"),
        }
    }

    // ── NotFound Tests ────────────────────────────────────

    #[test]
    fn test_get_account_not_found() {
        let store = make_store();
        let result = store.get_account(Uuid::new_v4());
        assert!(result.is_err());
        match result {
            Err(StoreError::NotFound(_)) => {}
            Err(e) => panic!("Expected NotFound, got: {:?}", e),
            Ok(_) => panic!("Expected NotFound error"),
        }
    }

    #[test]
    fn test_get_transaction_not_found() {
        let store = make_store();
        let result = store.get_transaction(Uuid::new_v4());
        assert!(result.is_err());
        match result {
            Err(StoreError::NotFound(_)) => {}
            Err(e) => panic!("Expected NotFound, got: {:?}", e),
            Ok(_) => panic!("Expected NotFound error"),
        }
    }

    #[test]
    fn test_get_journal_entry_not_found() {
        let store = make_store();
        let result = store.get_journal_entry(Uuid::new_v4());
        assert!(result.is_err());
        match result {
            Err(StoreError::NotFound(_)) => {}
            Err(e) => panic!("Expected NotFound, got: {:?}", e),
            Ok(_) => panic!("Expected NotFound error"),
        }
    }

    #[test]
    fn test_get_transaction_entry_not_found() {
        let store = make_store();
        let result = store.get_transaction_entry(Uuid::new_v4());
        assert!(result.is_err());
        match result {
            Err(StoreError::NotFound(_)) => {}
            Err(e) => panic!("Expected NotFound, got: {:?}", e),
            Ok(_) => panic!("Expected NotFound error"),
        }
    }

    #[test]
    fn test_get_account_by_number_not_found() {
        let store = make_store();
        let result = store.get_account_by_number("nonexistent");
        assert!(result.is_err());
        match result {
            Err(StoreError::NotFound(_)) => {}
            Err(e) => panic!("Expected NotFound, got: {:?}", e),
            Ok(_) => panic!("Expected NotFound error"),
        }
    }

    #[test]
    fn test_delete_account_not_found() {
        let store = make_store();
        let result = store.delete_account(Uuid::new_v4());
        assert!(result.is_err());
        match result {
            Err(StoreError::NotFound(_)) => {}
            Err(e) => panic!("Expected NotFound, got: {:?}", e),
            Ok(_) => panic!("Expected NotFound error"),
        }
    }

    #[test]
    fn test_delete_transaction_not_found() {
        let store = make_store();
        let result = store.delete_transaction(Uuid::new_v4());
        assert!(result.is_err());
        match result {
            Err(StoreError::NotFound(_)) => {}
            Err(e) => panic!("Expected NotFound, got: {:?}", e),
            Ok(_) => panic!("Expected NotFound error"),
        }
    }

    // ── Decimal Precision Tests ───────────────────────────

    #[test]
    fn test_decimal_precision_large_value() {
        let store = make_store();
        let mut account = make_account();
        account.balance = dec!("9999999999.999999");

        store.save_account(&account).expect("save");
        let retrieved = store.get_account(account.id).expect("get");
        assert_eq!(retrieved.balance, dec!("9999999999.999999"));
    }

    #[test]
    fn test_decimal_precision_small_value() {
        let store = make_store();
        let mut account = make_account();
        account.number = "1001".to_string();
        account.balance = dec!("0.00000001");

        store.save_account(&account).expect("save");
        let retrieved = store.get_account(account.id).expect("get");
        assert_eq!(retrieved.balance, dec!("0.00000001"));
    }

    #[test]
    fn test_decimal_precision_high_digits() {
        let store = make_store();
        let mut account = make_account();
        account.number = "1002".to_string();
        account.balance = dec!("123456789.123456789012345678");

        store.save_account(&account).expect("save");
        let retrieved = store.get_account(account.id).expect("get");
        assert_eq!(retrieved.balance, dec!("123456789.123456789012345678"));
    }

    // ── Multi-Currency Tests ──────────────────────────────

    #[test]
    fn test_multi_currency_fields_preserved() {
        let store = make_store();
        let entry = make_transaction_entry();

        store
            .save_transaction_entry(&entry)
            .expect("save entry");
        let retrieved = store
            .get_transaction_entry(entry.id)
            .expect("get entry");

        assert_eq!(retrieved.currency, "EUR");
        assert_eq!(retrieved.exchange_rate, Some(dec!(1.0876)));
        assert_eq!(retrieved.base_currency_amount, Some(dec!(1087.60)));
    }

    #[test]
    fn test_multi_currency_no_exchange_rate() {
        let store = make_store();
        let mut entry = make_transaction_entry();
        entry.id = Uuid::new_v4();
        entry.exchange_rate = None;
        entry.base_currency_amount = None;
        entry.currency = "USD".to_string();

        store
            .save_transaction_entry(&entry)
            .expect("save entry");
        let retrieved = store
            .get_transaction_entry(entry.id)
            .expect("get entry");

        assert_eq!(retrieved.currency, "USD");
        assert_eq!(retrieved.exchange_rate, None);
        assert_eq!(retrieved.base_currency_amount, None);
    }

    // ── Update (Re-save) Tests ────────────────────────────

    #[test]
    fn test_update_account_preserves_id() {
        let store = make_store();
        let account = make_account();
        store.save_account(&account).expect("save 1");

        // Update the same account (same id, same number)
        let mut updated = account.clone();
        updated.name = "Updated Cash Account".to_string();
        updated.balance = dec!(99999.99);
        store.save_account(&updated).expect("save 2");

        // Should still be only 1 account
        let accounts = store.list_accounts().expect("list");
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].name, "Updated Cash Account");
        assert_eq!(accounts[0].balance, dec!(99999.99));
    }

    #[test]
    fn test_update_transaction_preserves_id() {
        let store = make_store();
        let txn = make_transaction();
        store.save_transaction(&txn).expect("save 1");

        // Update the same transaction
        let mut updated = txn.clone();
        updated.description = "Updated description".to_string();
        store.save_transaction(&updated).expect("save 2");

        let txns = store.list_transactions().expect("list");
        assert_eq!(txns.len(), 1);
        assert_eq!(txns[0].description, "Updated description");
    }

    // ── Change-Log Verification Tests ─────────────────────

    #[test]
    fn test_save_records_change() {
        let store = make_store();
        let account = make_account();

        store.save_account(&account).expect("save");

        // Check that a change was recorded
        let changes = store
            .db
            .query_all(
                "SELECT entity_type, entity_id, operation FROM changes ORDER BY id",
                &[],
            )
            .expect("query changes");

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].get::<String, _>("entity_type").unwrap(), "account");
        assert_eq!(
            changes[0].get::<String, _>("entity_id").unwrap(),
            account.id.to_string()
        );
        assert_eq!(changes[0].get::<String, _>("operation").unwrap(), "insert");
    }

    #[test]
    fn test_delete_records_change() {
        let store = make_store();
        let account = make_account();
        store.save_account(&account).expect("save");

        store.delete_account(account.id).expect("delete");

        // Should have 2 changes: insert + delete
        let changes = store
            .db
            .query_all(
                "SELECT operation FROM changes WHERE entity_type = 'account' ORDER BY id",
                &[],
            )
            .expect("query changes");

        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].get::<String, _>("operation").unwrap(), "insert");
        assert_eq!(changes[1].get::<String, _>("operation").unwrap(), "delete");
    }

    #[test]
    fn test_update_records_update_operation() {
        let store = make_store();
        let account = make_account();
        store.save_account(&account).expect("save 1");

        // Re-save (update)
        let mut updated = account.clone();
        updated.name = "Updated".to_string();
        store.save_account(&updated).expect("save 2");

        let changes = store
            .db
            .query_all(
                "SELECT operation FROM changes WHERE entity_type = 'account' ORDER BY id",
                &[],
            )
            .expect("query changes");

        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].get::<String, _>("operation").unwrap(), "insert");
        assert_eq!(changes[1].get::<String, _>("operation").unwrap(), "update");
    }

    // ── Enum Round-Trip Tests ─────────────────────────────

    #[test]
    fn test_all_account_types_round_trip() {
        let store = make_store();

        let types = vec![
            (AccountType::Asset, "1000"),
            (AccountType::Liability, "2000"),
            (AccountType::Equity, "3000"),
            (AccountType::Revenue, "4000"),
            (AccountType::Expense, "5000"),
        ];

        for (at, num) in &types {
            let account = Account::new(num, &format!("{} Account", num), at.clone());
            store.save_account(&account).expect("save");
            let retrieved = store.get_account(account.id).expect("get");
            assert_eq!(
                retrieved.account_type, *at,
                "Account type {:?} did not round-trip",
                at
            );
        }
    }

    #[test]
    fn test_all_account_statuses_round_trip() {
        let store = make_store();

        let statuses = vec![
            (AccountStatus::Active, "1000"),
            (AccountStatus::Inactive, "2000"),
            (AccountStatus::Frozen, "3000"),
            (AccountStatus::Closed, "4000"),
        ];

        for (status, num) in &statuses {
            let mut account = Account::new(num, &format!("{} Account", num), AccountType::Asset);
            account.status = status.clone();
            store.save_account(&account).expect("save");
            let retrieved = store.get_account(account.id).expect("get");
            assert_eq!(
                retrieved.status, *status,
                "Account status {:?} did not round-trip",
                status
            );
        }
    }

    #[test]
    fn test_all_transaction_types_round_trip() {
        let store = make_store();

        let types = vec![
            (TransactionType::Invoice, "TXN-INV"),
            (TransactionType::Payment, "TXN-PAY"),
            (TransactionType::Expense, "TXN-EXP"),
            (TransactionType::Transfer, "TXN-TRF"),
            (TransactionType::JournalEntry, "TXN-JE"),
            (TransactionType::Adjustment, "TXN-ADJ"),
            (TransactionType::Reconciliation, "TXN-REC"),
            (TransactionType::Other, "TXN-OTH"),
        ];

        for (tt, num) in &types {
            let acct = Uuid::new_v4();
            let entries = vec![
                TransactionEntry::new(acct, EntryType::Debit, dec!(10), "d"),
                TransactionEntry::new(acct, EntryType::Credit, dec!(10), "c"),
            ];
            let mut txn = Transaction::new("Test".to_string(), Utc::now(), entries);
            txn.id = Uuid::new_v4();
            txn.number = num.to_string();
            txn.transaction_type = tt.clone();
            store.save_transaction(&txn).expect("save");
            let retrieved = store.get_transaction(txn.id).expect("get");
            assert_eq!(
                retrieved.transaction_type, *tt,
                "Transaction type {:?} did not round-trip",
                tt
            );
        }
    }

    #[test]
    fn test_all_entry_types_round_trip() {
        let store = make_store();

        // Debit entry
        let mut debit = make_transaction_entry();
        debit.entry_type = EntryType::Debit;
        store.save_transaction_entry(&debit).expect("save debit");
        let r_debit = store.get_transaction_entry(debit.id).expect("get debit");
        assert_eq!(r_debit.entry_type, EntryType::Debit);

        // Credit entry
        let mut credit = make_transaction_entry();
        credit.id = Uuid::new_v4();
        credit.entry_type = EntryType::Credit;
        store.save_transaction_entry(&credit).expect("save credit");
        let r_credit = store.get_transaction_entry(credit.id).expect("get credit");
        assert_eq!(r_credit.entry_type, EntryType::Credit);
    }

    // ── Metadata / Document IDs Tests ─────────────────────

    #[test]
    fn test_empty_metadata_and_document_ids() {
        let store = make_store();
        let acct = Uuid::new_v4();
        let entries = vec![
            TransactionEntry::new(acct, EntryType::Debit, dec!(10), "d"),
            TransactionEntry::new(acct, EntryType::Credit, dec!(10), "c"),
        ];
        let mut txn = Transaction::new("Empty metadata".to_string(), Utc::now(), entries);
        txn.id = Uuid::new_v4();
        txn.number = "TXN-EMPTY".to_string();
        // metadata and document_ids use defaults (empty)
        store.save_transaction(&txn).expect("save");
        let retrieved = store.get_transaction(txn.id).expect("get");
        assert!(retrieved.document_ids.is_empty());
        assert_eq!(retrieved.metadata, serde_json::json!({}));
    }

    #[test]
    fn test_complex_metadata_round_trip() {
        let store = make_store();
        let txn = make_transaction();
        let mut txn = txn;
        txn.metadata = serde_json::json!({
            "customer": {
                "name": "Acme Corp",
                "id": "cust-123",
                "address": {
                    "street": "456 Business Ave",
                    "city": "Commerce City",
                    "zip": "12345"
                }
            },
            "items": [
                {"sku": "WIDGET-001", "qty": 10, "price": 9.99},
                {"sku": "GADGET-002", "qty": 5, "price": 19.99}
            ],
            "discount": 0.10,
            "notes": "Rush order"
        });

        store.save_transaction(&txn).expect("save");
        let retrieved = store.get_transaction(txn.id).expect("get");
        assert_eq!(retrieved.metadata, txn.metadata);
    }

    // ── Journal Entry Posted/Reconciled Tests ─────────────

    #[test]
    fn test_posted_journal_entry_round_trip() {
        let store = make_store();
        let mut je = make_journal_entry();
        je.post().expect("Failed to post journal entry");

        store.save_journal_entry(&je).expect("save");
        let retrieved = store.get_journal_entry(je.id).expect("get");

        assert!(retrieved.is_posted);
        assert!(retrieved.posted_at.is_some());
    }

    #[test]
    fn test_reconciled_journal_entry_round_trip() {
        let store = make_store();
        let mut je = make_journal_entry();
        je.is_reconciled = true;

        store.save_journal_entry(&je).expect("save");
        let retrieved = store.get_journal_entry(je.id).expect("get");

        assert!(retrieved.is_reconciled);
    }

    // ── Parent Account Reference Test ─────────────────────

    #[test]
    fn test_account_with_parent_id_round_trip() {
        let store = make_store();
        let parent = Account::new("1000", "Parent Account", AccountType::Asset);
        store.save_account(&parent).expect("save parent");

        let mut child = make_account();
        child.number = "1001".to_string();
        child.name = "Child Account".to_string();
        child.parent_id = Some(parent.id);

        store.save_account(&child).expect("save child");
        let retrieved = store.get_account(child.id).expect("get child");

        assert_eq!(retrieved.parent_id, Some(parent.id));
    }

    #[test]
    fn test_account_without_parent_id_round_trip() {
        let store = make_store();
        let mut account = make_account();
        account.parent_id = None;

        store.save_account(&account).expect("save");
        let retrieved = store.get_account(account.id).expect("get");

        assert!(retrieved.parent_id.is_none());
    }

    // ── Journal Entry Without Reference Test ──────────────

    #[test]
    fn test_journal_entry_without_reference() {
        let store = make_store();
        let mut je = make_journal_entry();
        je.reference = None;

        store.save_journal_entry(&je).expect("save");
        let retrieved = store.get_journal_entry(je.id).expect("get");

        assert!(retrieved.reference.is_none());
    }

    // ── Transaction Entry Without Reference Test ──────────

    #[test]
    fn test_transaction_entry_without_reference() {
        let store = make_store();
        let mut entry = make_transaction_entry();
        entry.reference = None;

        store.save_transaction_entry(&entry).expect("save");
        let retrieved = store.get_transaction_entry(entry.id).expect("get");

        assert!(retrieved.reference.is_none());
    }

    // ── Error Display Tests ───────────────────────────────

    #[test]
    fn test_store_error_display() {
        let e = StoreError::NotFound("Account 123".to_string());
        assert_eq!(format!("{}", e), "Not found: Account 123");

        let e = StoreError::Validation("Unbalanced".to_string());
        assert_eq!(format!("{}", e), "Validation error: Unbalanced");

        let e = StoreError::Duplicate("TXN-001".to_string());
        assert_eq!(format!("{}", e), "Duplicate: TXN-001");

        let e = StoreError::Serialization("bad json".to_string());
        assert_eq!(format!("{}", e), "Serialization error: bad json");
    }
}
