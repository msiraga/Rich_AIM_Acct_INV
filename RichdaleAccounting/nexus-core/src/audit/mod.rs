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

/// Compute SHA-256 hash of an audit entry for tamper-proof chaining.
fn compute_audit_hash(entry: &AuditLog, prev_hash: Option<&str>) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(entry.id.to_string());
    hasher.update(&entry.entity_type);
    hasher.update(&entry.entity_id);
    hasher.update(format!("{:?}", entry.action));
    hasher.update(entry.timestamp.to_rfc3339());
    if let Some(ref old) = entry.old_values {
        hasher.update(old.to_string());
    }
    if let Some(ref new) = entry.new_values {
        hasher.update(new.to_string());
    }
    if let Some(prev) = prev_hash {
        hasher.update(prev);
    }
    hex::encode(hasher.finalize())
}

/// Log with hash-chaining: each entry links to the previous via SHA-256.
async fn log_with_chain(
    repository: &Arc<dyn AuditRepository>,
    mut entry: AuditLog,
) -> Result<(), anyhow::Error> {
    // Get the previous entry's hash for chain linking
    let all_logs = repository.list_all().await?;
    let prev_hash = all_logs.last().and_then(|e| e.chain_hash.clone());
    entry.prev_hash = prev_hash.clone();
    entry.chain_hash = Some(compute_audit_hash(&entry, prev_hash.as_deref()));
    repository.log(entry).await?;
    Ok(())
}

/// Audit Agent for handling audit-related tasks
#[derive(Clone)]
pub struct AuditAgent {
    /// Agent configuration
    pub config: AgentConfig,
    /// Current status
    pub status: AgentStatus,
    /// Audit repository
    pub repository: Arc<dyn AuditRepository>,
}

impl std::fmt::Debug for AuditAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "AuditAgent {{ config: {:?}, status: {:?} }}", self.config, self.status)
    }
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
    /// Process an audit check task.
    ///
    /// Accepts `TaskPayload::Json` with fields:
    ///   `entity_type` (string, e.g. "transaction", "account"),
    ///   `entity_id` (string),
    ///   `action` (string: "Create", "Update", "Delete", "Read", or custom),
    ///   `user_id` (string UUID, optional),
    ///   `old_values` (JSON object, optional),
    ///   `new_values` (JSON object, optional).
    ///
    /// If the payload is `Empty`, performs a system-level audit check.
    async fn process_audit_check(&self, task: Task) -> Result<Task, anyhow::Error> {
        let start_time = std::time::Instant::now();

        match &task.payload {
            TaskPayload::Json(json) => {
                let entity_type = json.get("entity_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let entity_id = json.get("entity_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let action_str = json.get("action")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Custom");
                let action = match action_str {
                    "Create" => AuditAction::Create,
                    "Read" => AuditAction::Read,
                    "Update" => AuditAction::Update,
                    "Delete" => AuditAction::Delete,
                    "Login" => AuditAction::Login,
                    "Logout" => AuditAction::Logout,
                    "Export" => AuditAction::Export,
                    "Import" => AuditAction::Import,
                    other => AuditAction::Custom(other.to_string()),
                };

                let user_id = json.get("user_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok());

                let old_values = json.get("old_values").cloned();
                let new_values = json.get("new_values").cloned();

                // Perform validation checks
                let mut warnings = Vec::new();

                // Check for update without old/new values
                if action == AuditAction::Update && old_values.is_none() && new_values.is_none() {
                    warnings.push("Update action logged without old or new values".to_string());
                }

                // Check for delete without old values
                if action == AuditAction::Delete && old_values.is_none() {
                    warnings.push("Delete action logged without old values for reference".to_string());
                }

                // Log the audit entry with hash chaining
                let audit_log = AuditLog {
                    id: Uuid::new_v4(),
                    user_id,
                    action: action.clone(),
                    entity_type: entity_type.to_string(),
                    entity_id: entity_id.to_string(),
                    old_values,
                    new_values,
                    timestamp: Utc::now(),
                    ip_address: json.get("ip_address").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    user_agent: json.get("user_agent").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    success: true,
                    error_message: None,
                    prev_hash: None,
                    chain_hash: None,
                };

                log_with_chain(&self.repository, audit_log).await?;

                let mut result = TaskResult::success(&format!(
                    "Audit check: {} {} on {}/{}",
                    if warnings.is_empty() { "passed" } else { "warnings" },
                    action_str,
                    entity_type,
                    entity_id,
                ));
                result.warnings = warnings;

                let _processing_time = start_time.elapsed().as_millis() as f64;

                Ok(task.complete(result))
            }
            _ => {
                // System-level audit check
                let audit_log = AuditLog {
                    id: Uuid::new_v4(),
                    user_id: None,
                    action: AuditAction::Custom("System audit check".to_string()),
                    entity_type: "system".to_string(),
                    entity_id: "all".to_string(),
                    old_values: None,
                    new_values: None,
                    timestamp: Utc::now(),
                    ip_address: None,
                    user_agent: None,
                    success: true,
                    error_message: None,
                    prev_hash: None,
                    chain_hash: None,
                };

                log_with_chain(&self.repository, audit_log).await?;

                let result = TaskResult::success("System audit check completed");
                let _processing_time = start_time.elapsed().as_millis() as f64;

                Ok(task.complete(result))
            }
        }
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
            id: Uuid::new_v4(),
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
            ..Default::default()
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
            id: Uuid::new_v4(),
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
            ..Default::default()
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
            id: Uuid::new_v4(),
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
            ..Default::default()
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
            id: Uuid::new_v4(),
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
            ..Default::default()
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
        agent.log_update(Some(user_id), "test", "123", old_values, new_values.clone()).await.unwrap();

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
