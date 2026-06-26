//! Agent Types Module
//!
//! Defines the types and configurations for all agents in the NexusLedger system.

use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Enum representing all possible agent types in the system
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AgentType {
    /// Handles ledger operations and double-entry accounting
    LedgerAgent,
    /// Manages bank reconciliation processes
    ReconciliationAgent,
    /// Processes invoices and billing
    InvoiceAgent,
    /// Manages payroll calculations and processing
    PayrollAgent,
    /// Handles tax calculations and filings
    TaxAgent,
    /// Processes and categorizes receipts
    ReceiptAgent,
    /// Manages document storage and retrieval
    DocumentAgent,
    /// Performs audit trail and compliance checks
    AuditAgent,
    /// Handles reporting and analytics
    ReportingAgent,
}

impl Default for AgentType {
    fn default() -> Self {
        AgentType::LedgerAgent
    }
}

/// Status of an agent
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AgentStatus {
    /// Agent is idle and waiting for tasks
    Idle,
    /// Agent is currently processing a task
    Busy,
    /// Agent encountered an error
    Error(String),
    /// Agent is initializing
    Initializing,
    /// Agent is shutting down
    ShuttingDown,
}

impl Default for AgentStatus {
    fn default() -> Self {
        AgentStatus::Idle
    }
}

/// Configuration for an individual agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Unique identifier for the agent
    pub id: Uuid,
    /// Type of agent
    pub agent_type: AgentType,
    /// Display name for the agent
    pub name: String,
    /// Description of what the agent does
    pub description: String,
    /// Priority level (0-10, higher is more important)
    pub priority: u8,
    /// Maximum concurrent tasks this agent can handle
    pub max_concurrent_tasks: usize,
    /// Whether the agent is enabled
    pub enabled: bool,
    /// Custom configuration parameters
    pub parameters: HashMap<String, String>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            agent_type: AgentType::default(),
            name: String::new(),
            description: String::new(),
            priority: 5,
            max_concurrent_tasks: 1,
            enabled: true,
            parameters: HashMap::new(),
        }
    }
}

impl AgentConfig {
    /// Create a new agent configuration
    pub fn new(agent_type: AgentType, name: &str, description: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            agent_type,
            name: name.to_string(),
            description: description.to_string(),
            ..Default::default()
        }
    }

    /// Create a ledger agent configuration
    pub fn ledger_agent() -> Self {
        Self::new(
            AgentType::LedgerAgent,
            "Ledger Agent",
            "Handles double-entry accounting and ledger operations"
        )
    }

    /// Create a reconciliation agent configuration
    pub fn reconciliation_agent() -> Self {
        Self::new(
            AgentType::ReconciliationAgent,
            "Reconciliation Agent",
            "Manages bank reconciliation and statement matching"
        )
    }

    /// Create an invoice agent configuration
    pub fn invoice_agent() -> Self {
        Self::new(
            AgentType::InvoiceAgent,
            "Invoice Agent",
            "Processes invoices, billing, and customer statements"
        )
    }

    /// Create a payroll agent configuration
    pub fn payroll_agent() -> Self {
        Self::new(
            AgentType::PayrollAgent,
            "Payroll Agent",
            "Handles payroll calculations, tax withholdings, and payments"
        )
    }

    /// Create a tax agent configuration
    pub fn tax_agent() -> Self {
        Self::new(
            AgentType::TaxAgent,
            "Tax Agent",
            "Manages tax calculations, filings, and compliance"
        )
    }

    /// Create a receipt agent configuration
    pub fn receipt_agent() -> Self {
        Self::new(
            AgentType::ReceiptAgent,
            "Receipt Agent",
            "Processes and categorizes receipts and expenses"
        )
    }

    /// Create a document agent configuration
    pub fn document_agent() -> Self {
        Self::new(
            AgentType::DocumentAgent,
            "Document Agent",
            "Manages document storage, retrieval, and organization"
        )
    }

    /// Create an audit agent configuration
    pub fn audit_agent() -> Self {
        Self::new(
            AgentType::AuditAgent,
            "Audit Agent",
            "Performs audit trail checks and compliance verification"
        )
    }

    /// Create a reporting agent configuration
    pub fn reporting_agent() -> Self {
        Self::new(
            AgentType::ReportingAgent,
            "Reporting Agent",
            "Generates financial reports and analytics"
        )
    }
}

/// Trait that all agents must implement
#[async_trait::async_trait]
pub trait Agent: Send + Sync {
    /// Get the agent's configuration
    fn config(&self) -> &AgentConfig;
    
    /// Get the agent's current status
    fn status(&self) -> AgentStatus;
    
    /// Initialize the agent
    async fn initialize(&mut self) -> Result<(), anyhow::Error>;
    
    /// Shutdown the agent gracefully
    async fn shutdown(&mut self) -> Result<(), anyhow::Error>;
    
    /// Process a task assigned to this agent
    async fn process_task(&self, task: crate::agents::task::Task) -> Result<crate::agents::task::Task, anyhow::Error>;
    
    /// Get the agent type
    fn agent_type(&self) -> AgentType;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_type_default() {
        assert_eq!(AgentType::default(), AgentType::LedgerAgent);
    }

    #[test]
    fn test_agent_status_default() {
        assert_eq!(AgentStatus::default(), AgentStatus::Idle);
    }

    #[test]
    fn test_agent_config_creation() {
        let config = AgentConfig::new(AgentType::LedgerAgent, "Test Agent", "Test description");
        assert_eq!(config.agent_type, AgentType::LedgerAgent);
        assert_eq!(config.name, "Test Agent");
        assert_eq!(config.description, "Test description");
    }

    #[test]
    fn test_all_agent_types() {
        let agent_types = [
            AgentType::LedgerAgent,
            AgentType::ReconciliationAgent,
            AgentType::InvoiceAgent,
            AgentType::PayrollAgent,
            AgentType::TaxAgent,
            AgentType::ReceiptAgent,
            AgentType::DocumentAgent,
            AgentType::AuditAgent,
            AgentType::ReportingAgent,
        ];
        
        assert_eq!(agent_types.len(), 9);
    }
}
