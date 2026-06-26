//! Agent Error Module
//!
//! Defines error types for the agent system.

use thiserror::Error;
use std::fmt;

/// Error types for the agent system
#[derive(Debug, Error)]
pub enum AgentError {
    /// Agent is not initialized
    #[error("Agent {0} is not initialized")]
    NotInitialized(String),
    
    /// Agent is already initialized
    #[error("Agent {0} is already initialized")]
    AlreadyInitialized(String),
    
    /// Agent is busy and cannot accept more tasks
    #[error("Agent {0} is busy (max concurrent tasks: {1})")]
    AgentBusy(String, usize),
    
    /// Agent is disabled
    #[error("Agent {0} is disabled")]
    AgentDisabled(String),
    
    /// Task processing failed
    #[error("Task processing failed: {0}")]
    TaskProcessingFailed(String),
    
    /// Task timeout
    #[error("Task timeout after {0}ms")]
    TaskTimeout(u64),
    
    /// Agent not found
    #[error("Agent not found: {0}")]
    AgentNotFound(String),
    
    /// Invalid agent configuration
    #[error("Invalid agent configuration: {0}")]
    InvalidConfiguration(String),
    
    /// Database error
    #[error("Database error: {0}")]
    DatabaseError(#[from] crate::database::error::DatabaseError),
    
    /// Validation error
    #[error("Validation error: {0}")]
    ValidationError(#[from] crate::utils::validation::ValidationError),
    
    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    
    /// Join error
    #[error("Join error: {0}")]
    JoinError(#[from] tokio::task::JoinError),
    
    /// Any other error
    #[error("Agent error: {0}")]
    Other(String),
}

impl AgentError {
    /// Create a new other error
    pub fn other(msg: &str) -> Self {
        Self::Other(msg.to_string())
    }
    
    /// Get the error message
    pub fn message(&self) -> String {
        match self {
            Self::NotInitialized(agent) => format!("Agent {} is not initialized", agent),
            Self::AlreadyInitialized(agent) => format!("Agent {} is already initialized", agent),
            Self::AgentBusy(agent, max) => format!("Agent {} is busy (max concurrent tasks: {})", agent, max),
            Self::AgentDisabled(agent) => format!("Agent {} is disabled", agent),
            Self::TaskProcessingFailed(msg) => format!("Task processing failed: {}", msg),
            Self::TaskTimeout(ms) => format!("Task timeout after {}ms", ms),
            Self::AgentNotFound(agent) => format!("Agent not found: {}", agent),
            Self::InvalidConfiguration(msg) => format!("Invalid agent configuration: {}", msg),
            Self::DatabaseError(e) => format!("Database error: {}", e),
            Self::ValidationError(e) => format!("Validation error: {}", e),
            Self::IoError(e) => format!("IO error: {}", e),
            Self::JoinError(e) => format!("Join error: {}", e),
            Self::Other(msg) => msg.clone(),
        }
    }
}

impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message())
    }
}

/// Result type for agent operations
pub type AgentResult<T> = Result<T, AgentError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_error_messages() {
        let error = AgentError::NotInitialized("LedgerAgent".to_string());
        assert_eq!(error.message(), "Agent LedgerAgent is not initialized");
        
        let error = AgentError::AgentBusy("InvoiceAgent".to_string(), 5);
        assert_eq!(error.message(), "Agent InvoiceAgent is busy (max concurrent tasks: 5)");
        
        let error = AgentError::AgentNotFound("UnknownAgent".to_string());
        assert_eq!(error.message(), "Agent not found: UnknownAgent");
    }

    #[test]
    fn test_agent_error_display() {
        let error = AgentError::TaskProcessingFailed("Invalid data".to_string());
        assert_eq!(format!("{}", error), "Task processing failed: Invalid data");
    }

    #[test]
    fn test_agent_error_other() {
        let error = AgentError::other("Custom error");
        assert_eq!(error.message(), "Custom error");
    }
}
