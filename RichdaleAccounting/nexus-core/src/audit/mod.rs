//! Audit Module
//!
//! This module contains audit-related functionality for the NexusLedger system.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use tracing::{info, error, debug, warn};
use crate::database::models::{AuditLog, AuditAction};
use crate::database::audit::{AuditRepository, MemoryAuditRepository};
use crate::agents::agent_types::{Agent, AgentConfig, AgentType, AgentStatus};
use crate::agents::task::{Task, TaskResult, TaskPayload};
use crate::agents::error::AgentError;

/// Audit Agent for handling audit-related tasks
#[derive(Debug, Clone)]
pub struct AuditAgent {
    /// Agent configuration
    pub config: AgentConfig,
    /// Current status
    pub status: AgentStatus,
    /// Audit repository
    pub repository: Arc<dyn AuditRepository>,
}

impl AuditAgent {
    /// Create a new audit agent
    pub fn new(config: AgentConfig, repository: Arc<dyn AuditRepository>) -> Self {
        Self {
            config,
            status: AgentStatus::Idle,
            repository,
        }
    }

    /// Create an audit agent with default configuration
    pub fn with_defaults() -> Self {
        let config = AgentConfig::audit_agent();
        let repository = Arc::new(MemoryAuditRepository::new());
        Self::new(config, repository)
    }
}

#[async_trait]
impl Agent for AuditAgent {
    fn config(&self) -> &AgentConfig {
        &self.config
    }

    fn status(&self) -> AgentStatus {
        self.status.clone()
    }

    async fn initialize(&mut self) -> Result<(), anyhow::Error> {
        self.status = AgentStatus::Initializing;
        info!("Initializing Audit Agent...");
        self.status = AgentStatus::Idle;
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), anyhow::Error> {
        self.status = AgentStatus::ShuttingDown;
        info!("Shutting down Audit Agent...");
        self.status = AgentStatus::Idle;
        Ok(())
    }

    async fn process_task(&self, task: Task) -> Result<Task, anyhow::Error> {
        if !self.config.enabled {
            return Err(AgentError::AgentDisabled(self.config.name.clone()).into());
        }

        match task.task_type {
            crate::agents::task::TaskType::AuditCheck => {
                self.process_audit_check(task).await
            }
            _ => {
                Err(AgentError::TaskProcessingFailed(
                    format!("AuditAgent cannot handle task type: {:?}", task.task_type)
                ).into())
            }
        }
    }

    fn agent_type(&self) -> AgentType {
        self.config.agent_type.clone()
    }
}

impl AuditAgent {
    /// Process an audit check task
    async fn process_audit_check(&self, task: Task) -> Result<Task, anyhow::Error> {
        self.status = AgentStatus::Busy;
        
        let start_time = std::time::Instant::now();
        
        // In a real implementation, we would extract audit parameters from the task
        // For now, we'll perform a mock audit check
        
        // Log an audit entry
        let audit_log = AuditLog {
            user_id: None,
            action: AuditAction::Custom("Audit check performed".to_string()),
            entity_type: "system".to_string(),
            entity_id: "all".to_string(),
            old_values: None,
            new_values: None,
            timestamp: Utc::now(),
            ip_address: None,
            user_agent: None,
            success: true,
            error_message: None,
        };
        
        self.repository.log(audit_log).await?;
        
        // Create success result
        let result = TaskResult::success("Audit check completed successfully");
        
        let processing_time = start_time.elapsed().as_millis() as f64;
        
        self.status = AgentStatus::Idle;
        
        Ok(task.complete(result))
    }

    /// Log a create action
    pub async fn log_create(
        &self,
        user_id: Option<Uuid>,
        entity_type: &str,
        entity_id: &str,
        new_values: serde_json::Value,
    ) -> Result<(), anyhow::Error> {
        let audit_log = AuditLog {
            user_id,
            action: AuditAction::Create,
            entity_type: entity_type.to_string(),
            entity_id: entity_id.to_string(),
            old_values: None,
            new_values: Some(new_values),
            timestamp: Utc::now(),
            ip_address: None,
            user_agent: None,
            success: true,
            error_message: None,
        };
        
        self.repository.log(audit_log).await?;
        Ok(())
    }

    /// Log an update action
    pub async fn log_update(
        &self,
        user_id: Option<Uuid>,
        entity_type: &str,
        entity_id: &str,
        old_values: serde_json::Value,
        new_values: serde_json::Value,
    ) -> Result<(), anyhow::Error> {
        let audit_log = AuditLog {
            user_id,
            action: AuditAction::Update,
            entity_type: entity_type.to_string(),
            entity_id: entity_id.to_string(),
            old_values: Some(old_values),
            new_values: Some(new_values),
            timestamp: Utc::now(),
            ip_address: None,
            user_agent: None,
            success: true,
            error_message: None,
        };
        
        self.repository.log(audit_log).await?;
        Ok(())
    }

    /// Log a delete action
    pub async fn log_delete(
        &self,
        user_id: Option<Uuid>,
        entity_type: &str,
        entity_id: &str,
        old_values: serde_json::Value,
    ) -> Result<(), anyhow::Error> {
        let audit_log = AuditLog {
            user_id,
            action: AuditAction::Delete,
            entity_type: entity_type.to_string(),
            entity_id: entity_id.to_string(),
            old_values: Some(old_values),
            new_values: None,
            timestamp: Utc::now(),
            ip_address: None,
            user_agent: None,
            success: true,
            error_message: None,
        };
        
        self.repository.log(audit_log).await?;
        Ok(())
    }

    /// Log an error
    pub async fn log_error(
        &self,
        user_id: Option<Uuid>,
        entity_type: &str,
        entity_id: &str,
        action: AuditAction,
        error: &str,
    ) -> Result<(), anyhow::Error> {
        let audit_log = AuditLog {
            user_id,
            action,
            entity_type: entity_type.to_string(),
            entity_id: entity_id.to_string(),
            old_values: None,
            new_values: None,
            timestamp: Utc::now(),
            ip_address: None,
            user_agent: None,
            success: false,
            error_message: Some(error.to_string()),
        };
        
        self.repository.log(audit_log).await?;
        Ok(())
    }

    /// Get audit logs by user
    pub async fn get_logs_by_user(&self, user_id: Uuid) -> Result<Vec<AuditLog>, anyhow::Error> {
        self.repository.find_by_user(user_id).await.map_err(|e| e.into())
    }

    /// Get audit logs by entity
    pub async fn get_logs_by_entity(&self, entity_type: &str, entity_id: &str) -> Result<Vec<AuditLog>, anyhow::Error> {
        self.repository.find_by_entity(entity_type, entity_id).await.map_err(|e| e.into())
    }

    /// Get audit logs by action
    pub async fn get_logs_by_action(&self, action: AuditAction) -> Result<Vec<AuditLog>, anyhow::Error> {
        self.repository.find_by_action(action).await.map_err(|e| e.into())
    }

    /// Get all audit logs
    pub async fn get_all_logs(&self) -> Result<Vec<AuditLog>, anyhow::Error> {
        self.repository.list_all().await.map_err(|e| e.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_audit_agent_creation() {
        let agent = AuditAgent::with_defaults();
        assert_eq!(agent.config.agent_type, AgentType::AuditAgent);
        assert_eq!(agent.status, AgentStatus::Idle);
    }

    #[tokio::test]
    async fn test_audit_agent_initialization() {
        let mut agent = AuditAgent::with_defaults();
        let result = agent.initialize().await;
        assert!(result.is_ok());
        assert_eq!(agent.status, AgentStatus::Idle);
    }

    #[tokio::test]
    async fn test_audit_logging() {
        let agent = AuditAgent::with_defaults();
        
        let user_id = Uuid::new_v4();
        let new_values = serde_json::json!({"name": "Test", "value": 123});
        
        // Log create
        agent.log_create(Some(user_id), "test", "123", new_values.clone()).await.unwrap();
        
        // Log update
        let old_values = serde_json::json!({"name": "Test", "value": 100});
        agent.log_update(Some(user_id), "test", "123", old_values, new_values).await.unwrap();
        
        // Log delete
        agent.log_delete(Some(user_id), "test", "123", new_values).await.unwrap();
        
        // Log error
        agent.log_error(Some(user_id), "test", "123", AuditAction::Delete, "Test error").await.unwrap();
        
        // Get logs
        let logs = agent.get_all_logs().await.unwrap();
        assert_eq!(logs.len(), 4);
    }

    #[tokio::test]
    async fn test_audit_check_task() {
        let agent = AuditAgent::with_defaults();
        
        let task = Task::new(crate::agents::task::TaskType::AuditCheck);
        let result = agent.process_task(task).await;
        
        assert!(result.is_ok());
        let completed_task = result.unwrap();
        assert_eq!(completed_task.status, crate::agents::task::TaskStatus::Completed);
    }
}
