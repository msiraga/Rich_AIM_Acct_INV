//! Agents Module
//!
//! This module contains all agent-related functionality for the NexusLedger system.
//! 
//! # Submodules
//! - `agent_types`: Defines agent types, configurations, and the Agent trait
//! - `config`: Handles agent configuration loading and management
//! - `document`: Document processing agent
//! - `error`: Agent error types
//! - `memory`: Agent memory and context management
//! - `orchestrator`: Agent orchestrator that manages all agents
//! - `status`: Agent status monitoring and reporting
//! - `task`: Task types and task management

pub mod agent_types;
pub mod config;
pub mod document;
pub mod error;
pub mod memory;
pub mod orchestrator;
pub mod status;
pub mod task;

// Re-export key types for convenience
pub use agent_types::{Agent, AgentConfig, AgentStatus, AgentType};
pub use config::{AgentConfigManager, AgentSystemConfig, AgentTypeConfig, TaskProcessingConfig};
pub use document::DocumentAgent;
pub use error::{AgentError, AgentResult};
pub use memory::{AgentMemory, MemoryEntry, MemoryManager};
pub use orchestrator::AgentOrchestrator;
pub use status::{AgentStatusInfo, StatusMonitor, SystemStatus};
pub use task::{Task, TaskPriority, TaskResult, TaskStatus, TaskType, TaskPayload};

/// Initialize all agents and return the orchestrator
pub async fn initialize_agents() -> Result<AgentOrchestrator, anyhow::Error> {
    let mut orchestrator = AgentOrchestrator::new();
    orchestrator.initialize().await?;
    Ok(orchestrator)
}

/// Create a default agent configuration for testing
pub fn create_test_agent_config(agent_type: AgentType) -> AgentConfig {
    AgentConfig::new(
        agent_type.clone(),
        &format!("Test {:?} Agent", agent_type),
        &format!("Test agent for {:?}", agent_type)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        // Test that all types are properly exported
        let _agent_type = AgentType::LedgerAgent;
        let _agent_status = AgentStatus::Idle;
        let _task_type = TaskType::RecordTransaction;
        let _task_priority = TaskPriority::Normal;
        let _task_status = TaskStatus::Pending;
    }

    #[tokio::test]
    async fn test_agents_initialization() {
        let orchestrator = initialize_agents().await.unwrap();
        assert!(!orchestrator.agents.read().await.is_empty());
    }
}
