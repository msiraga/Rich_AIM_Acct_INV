//! Database Migrations Module
//!
//! A simple migration runner that tracks applied schema versions in the
//! `schema_version` table and applies pending schema statements.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use surrealdb::engine::local::Db;
use surrealdb::sql::Datetime;
use surrealdb::Surreal;

use crate::database::error::DatabaseError;
use crate::database::schema::{schema_statements, CURRENT_SCHEMA_VERSION};

/// Represents a migration version record stored in the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SchemaVersionRecord {
    version: i32,
    applied_at: Datetime,
    description: String,
}

/// Ensure the `schema_version` table exists.
///
/// This runs a lightweight `DEFINE TABLE` so the table is guaranteed to
/// exist before we query it. Errors are silently ignored since SurrealDB 1.0
/// does not support `IF NOT EXISTS` and re-definition errors are expected.
async fn ensure_schema_version_table(db: &Surreal<Db>) -> Result<(), DatabaseError> {
    // The schema_version table is already defined in schema_statements(),
    // but we run the DEFINE TABLE here defensively in case someone calls
    // run_migrations before apply_schema.
    let _ = db.query(
        "DEFINE TABLE schema_version TYPE NORMAL COMMENT 'Tracks database schema version for migrations';",
    )
    .await;

    let _ = db.query(
        "DEFINE FIELD version ON TABLE schema_version TYPE int;",
    )
    .await;

    let _ = db.query(
        "DEFINE FIELD applied_at ON TABLE schema_version TYPE datetime;",
    )
    .await;

    let _ = db.query(
        "DEFINE FIELD description ON TABLE schema_version TYPE string DEFAULT '';",
    )
    .await;

    Ok(())
}

/// Get the current schema version from the database.
///
/// Returns 0 if no version has been recorded yet.
async fn get_current_version(db: &Surreal<Db>) -> Result<i32, DatabaseError> {
    let mut response = db
        .query("SELECT * FROM schema_version ORDER BY version DESC LIMIT 1")
        .await
        .map_err(|e| DatabaseError::MigrationError(e.to_string()))?;

    let records: Vec<SchemaVersionRecord> = response
        .take(0)
        .map_err(|e| DatabaseError::MigrationError(e.to_string()))?;

    Ok(records.first().map(|r| r.version).unwrap_or(0))
}

/// Apply all DEFINE TABLE/FIELD statements from `schema_statements()`.
///
/// Each statement is executed individually so that partial failures
/// do not halt the entire migration.
pub async fn apply_schema(db: &Surreal<Db>) -> Result<(), DatabaseError> {
    let statements = schema_statements();

    for stmt in statements.iter() {
        // Silently ignore errors — DEFINE statements are idempotent
        // in intent; SurrealDB 1.0 errors on re-definition.
        let _ = db.query(*stmt).await;
    }

    Ok(())
}

/// Run pending migrations.
///
/// 1. Ensures the `schema_version` table exists.
/// 2. Checks the current version.
/// 3. If the current version is less than `CURRENT_SCHEMA_VERSION`,
///    applies all schema statements and records the new version.
/// 4. Idempotent — calling it multiple times is safe.
pub async fn run_migrations(db: &Surreal<Db>) -> Result<(), DatabaseError> {
    // Step 1: Ensure the schema_version tracking table exists
    ensure_schema_version_table(db).await?;

    // Step 2: Check the current version
    let current = get_current_version(db).await?;

    if current >= CURRENT_SCHEMA_VERSION {
        // Already up to date
        return Ok(());
    }

    // Step 3: Apply all schema definitions
    apply_schema(db).await?;

    // Step 4: Record the new version
    let record = SchemaVersionRecord {
        version: CURRENT_SCHEMA_VERSION,
        applied_at: Datetime::from(Utc::now()),
        description: format!(
            "Initial schema — account, transaction_entry, journal_entry, user, \
             organization, document, audit_log, reconciliation, tax_jurisdiction, \
             tax_filing, employee, pay_period, time_entry, schema_version"
        ),
    };

    let _: Vec<SchemaVersionRecord> = db
        .create("schema_version")
        .content(record)
        .await
        .map_err(|e| DatabaseError::MigrationError(format!("Failed to record schema version: {}", e)))?;

    Ok(())
}

/// Get the current schema version (public API for diagnostics).
pub async fn current_schema_version(db: &Surreal<Db>) -> Result<i32, DatabaseError> {
    ensure_schema_version_table(db).await?;
    get_current_version(db).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_schema_version_positive() {
        assert!(CURRENT_SCHEMA_VERSION >= 1);
    }
}
