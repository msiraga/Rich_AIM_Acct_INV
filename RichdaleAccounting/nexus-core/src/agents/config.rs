//! Agent Configuration Module
//!
//! Handles configuration loading and management for agents.

use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::path::Path;
use config::{Config, File, Environment};
use uuid::Uuid;
use crate::agents::agent_types::{AgentType, AgentConfig};
use crate::agents::error::AgentError;

/// Default configuration for the agent system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSystemConfig {
    /// Whether the agent system is enabled
    pub enabled: bool,
    /// Maximum number of concurrent agents
    pub max_concurrent_agents: usize,
    /// Default timeout for agent tasks (milliseconds)
    pub default_task_timeout_ms: u64,
    /// Maximum retry attempts for failed tasks
    pub max_task_retries: usize,
    /// Whether to enable auto-retry for failed tasks
    pub auto_retry_enabled: bool,
    /// Delay between retries (milliseconds)
    pub retry_delay_ms: u64,
    /// Whether to enable memory for agents
    pub memory_enabled: bool,
    /// Maximum short-term memory entries per agent
    pub max_short_term_memory: usize,
    /// Maximum long-term memory entries per agent
    pub max_long_term_memory: usize,
    /// Whether to enable status monitoring
    pub monitoring_enabled: bool,
    /// Interval for status updates (milliseconds)
    pub monitoring_interval_ms: u64,
    /// Configuration for individual agent types
    pub agent_configs: HashMap<String, AgentTypeConfig>,
}

impl Default for AgentSystemConfig {
    fn default() -> Self {
        let mut agent_configs = HashMap::new();
        
        // Default configurations for each agent type
        agent_configs.insert("ledger".to_string(), AgentTypeConfig {
            enabled: true,
            max_instances: 1,
            priority: 10,
            ..Default::default()
        });
        
        agent_configs.insert("reconciliation".to_string(), AgentTypeConfig {
            enabled: true,
            max_instances: 1,
            priority: 8,
            ..Default::default()
        });
        
        agent_configs.insert("invoice".to_string(), AgentTypeConfig {
            enabled: true,
            max_instances: 2,
            priority: 7,
            ..Default::default()
        });
        
        agent_configs.insert("payroll".to_string(), AgentTypeConfig {
            enabled: true,
            max_instances: 1,
            priority: 6,
            ..Default::default()
        });
        
        agent_configs.insert("tax".to_string(), AgentTypeConfig {
            enabled: true,
            max_instances: 1,
            priority: 9,
            ..Default::default()
        });
        
        agent_configs.insert("receipt".to_string(), AgentTypeConfig {
            enabled: true,
            max_instances: 2,
            priority: 5,
            ..Default::default()
        });
        
        agent_configs.insert("document".to_string(), AgentTypeConfig {
            enabled: true,
            max_instances: 2,
            priority: 4,
            ..Default::default()
        });
        
        agent_configs.insert("audit".to_string(), AgentTypeConfig {
            enabled: true,
            max_instances: 1,
            priority: 8,
            ..Default::default()
        });
        
        agent_configs.insert("reporting".to_string(), AgentTypeConfig {
            enabled: true,
            max_instances: 1,
            priority: 3,
            ..Default::default()
        });

        Self {
            enabled: true,
            max_concurrent_agents: 10,
            default_task_timeout_ms: 30000,
            max_task_retries: 3,
            auto_retry_enabled: true,
            retry_delay_ms: 1000,
            memory_enabled: true,
            max_short_term_memory: 100,
            max_long_term_memory: 1000,
            monitoring_enabled: true,
            monitoring_interval_ms: 5000,
            agent_configs,
        }
    }
}

impl AgentSystemConfig {
    /// Create a new agent system configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Load configuration from a file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, AgentError> {
        let settings = Config::builder()
            .add_source(File::from(path.as_ref()))
            .add_source(Environment::with_prefix("AGENT").prefix_separator("_"))
            .build()
            .map_err(|e| AgentError::InvalidConfiguration(e.to_string()))?;

        settings.try_deserialize().map_err(|e| AgentError::InvalidConfiguration(e.to_string()))
    }

    /// Get configuration for a specific agent type
    pub fn get_agent_config(&self, agent_type: &AgentType) -> AgentTypeConfig {
        let key = match agent_type {
            AgentType::LedgerAgent => "ledger",
            AgentType::ReconciliationAgent => "reconciliation",
            AgentType::InvoiceAgent => "invoice",
            AgentType::PayrollAgent => "payroll",
            AgentType::TaxAgent => "tax",
            AgentType::ReceiptAgent => "receipt",
            AgentType::DocumentAgent => "document",
            AgentType::AuditAgent => "audit",
            AgentType::ReportingAgent => "reporting",
        };
        
        self.agent_configs.get(key).cloned().unwrap_or_default()
    }

    /// Check if an agent type is enabled
    pub fn is_agent_enabled(&self, agent_type: &AgentType) -> bool {
        let config = self.get_agent_config(agent_type);
        config.enabled && self.enabled
    }

    /// Get the maximum instances for an agent type
    pub fn get_max_instances(&self, agent_type: &AgentType) -> usize {
        let config = self.get_agent_config(agent_type);
        config.max_instances.min(self.max_concurrent_agents)
    }
}

/// Configuration for a specific agent type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTypeConfig {
    /// Whether this agent type is enabled
    pub enabled: bool,
    /// Maximum number of instances of this agent type
    pub max_instances: usize,
    /// Priority level (higher = more important)
    pub priority: u8,
    /// Custom parameters for this agent type
    pub parameters: HashMap<String, String>,
}

impl Default for AgentTypeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_instances: 1,
            priority: 5,
            parameters: HashMap::new(),
        }
    }
}

/// Agent configuration manager
#[derive(Debug, Clone)]
pub struct AgentConfigManager {
    /// System-wide configuration
    pub system_config: AgentSystemConfig,
    /// Individual agent configurations
    pub agent_configs: HashMap<Uuid, AgentConfig>,
}

impl Default for AgentConfigManager {
    fn default() -> Self {
        Self {
            system_config: AgentSystemConfig::default(),
            agent_configs: HashMap::new(),
        }
    }
}

impl AgentConfigManager {
    /// Create a new configuration manager
    pub fn new() -> Self {
        Self::default()
    }

    /// Load configuration from a file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, AgentError> {
        let system_config = AgentSystemConfig::from_file(path)?;
        Ok(Self {
            system_config,
            ..Default::default()
        })
    }

    /// Add an agent configuration
    pub fn add_agent_config(&mut self, config: AgentConfig) {
        self.agent_configs.insert(config.id, config);
    }

    /// Get an agent configuration by ID
    pub fn get_agent_config(&self, agent_id: &Uuid) -> Option<&AgentConfig> {
        self.agent_configs.get(agent_id)
    }

    /// Remove an agent configuration
    pub fn remove_agent_config(&mut self, agent_id: &Uuid) -> Option<AgentConfig> {
        self.agent_configs.remove(agent_id)
    }

    /// Get all agent configurations for a specific type
    pub fn get_agent_configs_by_type(&self, agent_type: &AgentType) -> Vec<&AgentConfig> {
        self.agent_configs.values()
            .filter(|config| config.agent_type == *agent_type)
            .collect()
    }

    /// Check if the system is enabled
    pub fn is_system_enabled(&self) -> bool {
        self.system_config.enabled
    }

    /// Get system configuration
    pub fn get_system_config(&self) -> &AgentSystemConfig {
        &self.system_config
    }

    /// Update system configuration
    pub fn update_system_config(&mut self, config: AgentSystemConfig) {
        self.system_config = config;
    }
}

/// Configuration for agent task processing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProcessingConfig {
    /// Maximum concurrent tasks per agent
    pub max_concurrent_tasks: usize,
    /// Task timeout in milliseconds
    pub task_timeout_ms: u64,
    /// Whether to enable task batching
    pub batching_enabled: bool,
    /// Maximum batch size
    pub max_batch_size: usize,
    /// Batch timeout in milliseconds
    pub batch_timeout_ms: u64,
}

impl Default for TaskProcessingConfig {
    fn default() -> Self {
        Self {
            max_concurrent_tasks: 1,
            task_timeout_ms: 30000,
            batching_enabled: false,
            max_batch_size: 10,
            batch_timeout_ms: 5000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_system_config_default() {
        let config = AgentSystemConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_concurrent_agents, 10);
        assert_eq!(config.agent_configs.len(), 9);
    }

    #[test]
    fn test_agent_type_config() {
        let config = AgentSystemConfig::default();
        let ledger_config = config.get_agent_config(&AgentType::LedgerAgent);
        assert!(ledger_config.enabled);
        assert_eq!(ledger_config.priority, 10);
    }

    #[test]
    fn test_agent_config_manager() {
        let mut manager = AgentConfigManager::new();
        
        let agent_config = AgentConfig::ledger_agent();
        manager.add_agent_config(agent_config.clone());
        
        assert_eq!(manager.agent_configs.len(), 1);
        assert!(manager.get_agent_config(&agent_config.id).is_some());
    }

    #[test]
    fn test_task_processing_config() {
        let config = TaskProcessingConfig::default();
        assert_eq!(config.max_concurrent_tasks, 1);
        assert_eq!(config.task_timeout_ms, 30000);
    }
}
