//! NexusLedger - Fully Agentic Accounting Platform
//!
//! A QuickBooks replacement built with autonomous agents for each accounting function.
//! 
//! # Architecture
//! - **Agents**: Autonomous agents handle specific accounting tasks
//! - **Database**: SurrealDB for flexible data storage
//! - **AI Integration**: Ollama for AI-powered features
//! - **Edge Support**: Cross-platform deployment

pub mod agents;
pub mod ai;
pub mod accounting;
pub mod api;
pub mod audit;
pub mod database;
pub mod edge;
pub mod models;
pub mod monitor;
pub mod utils;

// Re-export key types for convenience
pub use agents::agent_types::{AgentType, AgentStatus, AgentConfig};
pub use agents::error::AgentError;
pub use agents::task::Task;
pub use agents::orchestrator::AgentOrchestrator;

pub use database::models::{User, Organization, Document, AuditLog};
pub use database::financial::{Account, Transaction, TransactionEntry, JournalEntry, EntryType, AccountType, BalanceType};
pub use database::error::DatabaseError;

pub use accounting::ledger::{Ledger, LedgerError};
pub use accounting::reconciliation::{ReconciliationResult, ReconciliationError};
pub use accounting::tax::{TaxCalculator, TaxError};
pub use accounting::payroll::{PayrollProcessor, PayrollError};

pub use utils::validation::{ValidationError, Validator};
pub use utils::date_utils::{DateRange, DateError};
pub use utils::file_utils::{FileError, FileProcessor};

/// NexusLedger main struct that orchestrates all accounting operations
#[derive(Debug, Clone)]
pub struct NexusLedger {
    pub orchestrator: AgentOrchestrator,
    pub ledger: Ledger,
}

impl NexusLedger {
    /// Create a new NexusLedger instance
    pub fn new() -> Self {
        Self {
            orchestrator: AgentOrchestrator::new(),
            ledger: Ledger::new(),
        }
    }

    /// Initialize the system with configuration
    pub async fn initialize(&mut self) -> Result<(), anyhow::Error> {
        self.orchestrator.initialize().await?;
        self.ledger.initialize().await?;
        Ok(())
    }

    /// Process a transaction through the agent system
    pub async fn process_transaction(&self, transaction: Transaction) -> Result<Transaction, anyhow::Error> {
        self.orchestrator.process_transaction(transaction).await
    }

    /// Generate financial reports
    pub async fn generate_reports(&self) -> Result<(), anyhow::Error> {
        self.orchestrator.generate_reports().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nexus_ledger_creation() {
        let nexus = NexusLedger::new();
        assert!(nexus.orchestrator.agents.is_empty());
    }
}
