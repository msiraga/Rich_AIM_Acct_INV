//! Database Module
//!
//! This module contains all database-related functionality for the NexusLedger system.
//! It provides a SurrealDB-backed storage layer with in-memory support for testing.

pub mod models;
pub mod financial;
pub mod document;
pub mod error;
pub mod audit;
pub mod user;
pub mod schema;
pub mod seed;
pub mod migrations;

pub use models::{BoundingBox, DocumentType, User, UserRole, Organization, Address, ContactInfo, AccountingPeriod, Document, AuditLog, AuditAction, Settings};
pub use financial::{Account, AccountType, AccountStatus, BalanceType, EntryType, BankAccountDetails, TransactionEntry, JournalEntry, Transaction, TransactionType, TransactionStatus, Reconciliation, ReconciliationStatus};
pub use error::{DatabaseError, DatabaseResult};
pub use schema::schema_statements;
pub use seed::seed_default_accounts;
pub use migrations::run_migrations;

use std::sync::Arc;
use tokio::sync::Mutex;

use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::engine::local::Mem;

/// Database connection manager
///
/// Wraps a `Surreal<Db>` client (local/embedded engine). For the current
/// Phase 1 implementation the database runs in-memory via the `kv-mem`
/// feature. A future phase will add WebSocket (`Surreal<ws::Client>`)
/// support behind a `DatabaseInner` enum.
#[derive(Debug, Clone)]
pub struct Database {
    /// SurrealDB client connection (local/embedded engine)
    pub client: Arc<Mutex<Option<Surreal<Db>>>>,
    /// Whether using in-memory mode
    pub in_memory: bool,
    /// Connection URL (None for in-memory)
    pub url: Option<String>,
}

impl Default for Database {
    fn default() -> Self {
        Self::new()
    }
}

impl Database {
    /// Create a new in-memory database (for testing)
    pub fn new() -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
            in_memory: true,
            url: None,
        }
    }

    /// Create a database with a connection URL
    pub fn with_url(url: &str) -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
            in_memory: false,
            url: Some(url.to_string()),
        }
    }

    /// Connect to the database.
    ///
    /// For in-memory mode (default), creates an embedded SurrealDB instance
    /// backed by `kv-mem`. After connecting, applies all schema definitions
    /// and runs migrations.
    ///
    /// If a URL is configured, attempts a WebSocket connection first and
    /// falls back to in-memory on failure.
    pub async fn connect(&self) -> Result<(), DatabaseError> {
        let db = if let Some(ref _url) = self.url {
            // WebSocket path: try to connect, fall back to in-memory on failure.
            // For Phase 1 we always fall back since there is no running server.
            // TODO: wire up surrealdb::engine::remote::ws::Ws connection when
            //       a SurrealDB server is available.
            Self::connect_in_memory().await?
        } else {
            Self::connect_in_memory().await?
        };

        // Select namespace and database
        db.use_ns("nexus").use_db("ledger").await
            .map_err(|e| DatabaseError::ConnectionError(format!("Failed to select ns/db: {}", e)))?;

        // Store the connection
        {
            let mut client = self.client.lock().await;
            *client = Some(db.clone());
        }

        // Apply schema definitions
        schema::apply_all_statements(&db).await?;

        // Run migrations
        migrations::run_migrations(&db).await?;

        Ok(())
    }

    /// Create an in-memory SurrealDB instance.
    async fn connect_in_memory() -> Result<Surreal<Db>, DatabaseError> {
        let db = Surreal::new::<Mem>(()).await
            .map_err(|e| DatabaseError::ConnectionError(format!("Failed to create in-memory DB: {}", e)))?;
        Ok(db)
    }

    /// Disconnect from the database
    pub async fn disconnect(&self) -> Result<(), DatabaseError> {
        let mut client = self.client.lock().await;
        *client = None;
        Ok(())
    }

    /// Check if connected
    pub async fn is_connected(&self) -> bool {
        self.client.lock().await.is_some()
    }

    /// Get the client reference
    pub fn client(&self) -> Arc<Mutex<Option<Surreal<Db>>>> {
        self.client.clone()
    }

    /// Get a direct reference to the underlying Surreal<Db>.
    ///
    /// Returns `DatabaseError::NotInitialized` if not connected.
    pub async fn db(&self) -> Result<Surreal<Db>, DatabaseError> {
        let client = self.client.lock().await;
        client.clone().ok_or(DatabaseError::NotInitialized)
    }

    /// Seed the default chart of accounts.
    pub async fn seed(&self) -> Result<usize, DatabaseError> {
        let db = self.db().await?;
        seed::seed_default_accounts(&db).await
    }
}
