//! Database Error Module
//!
//! Defines error types for database operations.

use thiserror::Error;
use std::fmt;

/// Error types for database operations
#[derive(Debug, Error)]
pub enum DatabaseError {
    /// Connection error
    #[error("Database connection error: {0}")]
    ConnectionError(String),
    
    /// Query execution error
    #[error("Query execution error: {0}")]
    QueryError(String),
    
    /// Record not found
    #[error("Record not found: {0}")]
    NotFound(String),
    
    /// Duplicate record
    #[error("Duplicate record: {0}")]
    DuplicateRecord(String),
    
    /// Validation error
    #[error("Validation error: {0}")]
    ValidationError(String),
    
    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    /// Deserialization error
    #[error("Deserialization error: {0}")]
    DeserializationError(String),
    
    /// Constraint violation
    #[error("Constraint violation: {0}")]
    ConstraintViolation(String),
    
    /// Transaction error
    #[error("Transaction error: {0}")]
    TransactionError(String),
    
    /// Database not initialized
    #[error("Database not initialized")]
    NotInitialized,
    
    /// Database already initialized
    #[error("Database already initialized")]
    AlreadyInitialized,
    
    /// Migration error
    #[error("Migration error: {0}")]
    MigrationError(String),
    
    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    
    /// SurrealDB specific error
    #[error("SurrealDB error: {0}")]
    SurrealError(String),
    
    /// Any other database error
    #[error("Database error: {0}")]
    Other(String),
}

impl DatabaseError {
    /// Create a new other error
    pub fn other(msg: &str) -> Self {
        Self::Other(msg.to_string())
    }
    
    /// Get the error message
    pub fn message(&self) -> String {
        match self {
            Self::ConnectionError(msg) => format!("Database connection error: {}", msg),
            Self::QueryError(msg) => format!("Query execution error: {}", msg),
            Self::NotFound(msg) => format!("Record not found: {}", msg),
            Self::DuplicateRecord(msg) => format!("Duplicate record: {}", msg),
            Self::ValidationError(msg) => format!("Validation error: {}", msg),
            Self::SerializationError(msg) => format!("Serialization error: {}", msg),
            Self::DeserializationError(msg) => format!("Deserialization error: {}", msg),
            Self::ConstraintViolation(msg) => format!("Constraint violation: {}", msg),
            Self::TransactionError(msg) => format!("Transaction error: {}", msg),
            Self::NotInitialized => "Database not initialized".to_string(),
            Self::AlreadyInitialized => "Database already initialized".to_string(),
            Self::MigrationError(msg) => format!("Migration error: {}", msg),
            Self::IoError(e) => format!("IO error: {}", e),
            Self::SurrealError(msg) => format!("SurrealDB error: {}", msg),
            Self::Other(msg) => msg.clone(),
        }
    }
}

/// Result type for database operations
pub type DatabaseResult<T> = Result<T, DatabaseError>;

/// Convert from SurrealDB error
impl From<surrealdb::Error> for DatabaseError {
    fn from(error: surrealdb::Error) -> Self {
        Self::SurrealError(error.to_string())
    }
}

/// Convert from serde_json error
impl From<serde_json::Error> for DatabaseError {
    fn from(error: serde_json::Error) -> Self {
        Self::SerializationError(error.to_string())
    }
}

/// Convert from UUID error
impl From<uuid::Error> for DatabaseError {
    fn from(error: uuid::Error) -> Self {
        Self::ValidationError(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_error_messages() {
        let error = DatabaseError::ConnectionError("Failed to connect".to_string());
        assert_eq!(error.message(), "Database connection error: Failed to connect");
        
        let error = DatabaseError::NotFound("User not found".to_string());
        assert_eq!(error.message(), "Record not found: User not found");
        
        let error = DatabaseError::NotInitialized;
        assert_eq!(error.message(), "Database not initialized");
    }

    #[test]
    fn test_database_error_display() {
        let error = DatabaseError::QueryError("Invalid SQL".to_string());
        assert_eq!(format!("{}", error), "Query execution error: Invalid SQL");
    }

    #[test]
    fn test_database_error_other() {
        let error = DatabaseError::other("Custom error");
        assert_eq!(error.message(), "Custom error");
    }

    #[test]
    fn test_database_error_conversions() {
        let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "File not found");
        let db_error: DatabaseError = io_error.into();
        assert!(matches!(db_error, DatabaseError::IoError(_)));
    }
}
