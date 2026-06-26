//! Database Audit Module
//!
//! Handles audit log storage and retrieval in the database.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use crate::database::models::{AuditLog, AuditAction};
use crate::database::error::{DatabaseError, DatabaseResult};

/// Audit log repository
#[async_trait]
pub trait AuditRepository: Send + Sync {
    /// Log an audit entry
    async fn log(&self, audit_log: AuditLog) -> DatabaseResult<AuditLog>;
    
    /// Find audit logs by user ID
    async fn find_by_user(&self, user_id: Uuid) -> DatabaseResult<Vec<AuditLog>>;
    
    /// Find audit logs by entity type and ID
    async fn find_by_entity(&self, entity_type: &str, entity_id: &str) -> DatabaseResult<Vec<AuditLog>>;
    
    /// Find audit logs by action
    async fn find_by_action(&self, action: AuditAction) -> DatabaseResult<Vec<AuditLog>>;
    
    /// Find audit logs by date range
    async fn find_by_date_range(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> DatabaseResult<Vec<AuditLog>>;
    
    /// List all audit logs
    async fn list_all(&self) -> DatabaseResult<Vec<AuditLog>>;
    
    /// Delete audit logs older than a certain date
    async fn delete_older_than(&self, date: DateTime<Utc>) -> DatabaseResult<u64>;
}

/// SurrealDB implementation of AuditRepository
#[derive(Debug, Clone)]
pub struct SurrealAuditRepository {
    /// Database connection
    pub db: Arc<Mutex<Option<surrealdb::Surreal<surrealdb::engine::remote::ws::Client>>>>,
}

impl SurrealAuditRepository {
    /// Create a new SurrealDB audit repository
    pub fn new(db: Arc<Mutex<Option<surrealdb::Surreal<surrealdb::engine::remote::ws::Client>>>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl AuditRepository for SurrealAuditRepository {
    async fn log(&self, audit_log: AuditLog) -> DatabaseResult<AuditLog> {
        let client = self.db.lock().await;
        let client = client.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let log_data = serde_json::json!({
            "id": audit_log.id.to_string(),
            "user_id": audit_log.user_id.map(|u| u.to_string()),
            "action": self.action_to_string(&audit_log.action),
            "entity_type": audit_log.entity_type,
            "entity_id": audit_log.entity_id,
            "old_values": audit_log.old_values,
            "new_values": audit_log.new_values,
            "timestamp": audit_log.timestamp.to_rfc3339(),
            "ip_address": audit_log.ip_address,
            "user_agent": audit_log.user_agent,
            "success": audit_log.success,
            "error_message": audit_log.error_message,
        });

        let query = format!(
            "INSERT INTO audit_log CONTENT {};",
            serde_json::to_string(&log_data).map_err(|e| DatabaseError::SerializationError(e.to_string()))?
        );

        let _ = client.query(&query)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(audit_log)
    }

    async fn find_by_user(&self, user_id: Uuid) -> DatabaseResult<Vec<AuditLog>> {
        let client = self.db.lock().await;
        let client = client.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let query = format!("SELECT * FROM audit_log WHERE user_id = '{}';", user_id);
        
        let result: Vec<serde_json::Value> = client.query(&query)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let logs = result.into_iter()
            .filter_map(|v| self.parse_audit_log(v).ok())
            .collect();

        Ok(logs)
    }

    async fn find_by_entity(&self, entity_type: &str, entity_id: &str) -> DatabaseResult<Vec<AuditLog>> {
        let client = self.db.lock().await;
        let client = client.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let query = format!(
            "SELECT * FROM audit_log WHERE entity_type = '{}' AND entity_id = '{}';",
            entity_type, entity_id
        );
        
        let result: Vec<serde_json::Value> = client.query(&query)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let logs = result.into_iter()
            .filter_map(|v| self.parse_audit_log(v).ok())
            .collect();

        Ok(logs)
    }

    async fn find_by_action(&self, action: AuditAction) -> DatabaseResult<Vec<AuditLog>> {
        let client = self.db.lock().await;
        let client = client.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let action_str = self.action_to_string(&action);
        let query = format!("SELECT * FROM audit_log WHERE action = '{}';", action_str);
        
        let result: Vec<serde_json::Value> = client.query(&query)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let logs = result.into_iter()
            .filter_map(|v| self.parse_audit_log(v).ok())
            .collect();

        Ok(logs)
    }

    async fn find_by_date_range(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> DatabaseResult<Vec<AuditLog>> {
        let client = self.db.lock().await;
        let client = client.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let start_str = start.to_rfc3339();
        let end_str = end.to_rfc3339();
        let query = format!(
            "SELECT * FROM audit_log WHERE timestamp >= '{}' AND timestamp <= '{}';",
            start_str, end_str
        );
        
        let result: Vec<serde_json::Value> = client.query(&query)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let logs = result.into_iter()
            .filter_map(|v| self.parse_audit_log(v).ok())
            .collect();

        Ok(logs)
    }

    async fn list_all(&self) -> DatabaseResult<Vec<AuditLog>> {
        let client = self.db.lock().await;
        let client = client.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let query = "SELECT * FROM audit_log ORDER BY timestamp DESC;";
        
        let result: Vec<serde_json::Value> = client.query(query)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let logs = result.into_iter()
            .filter_map(|v| self.parse_audit_log(v).ok())
            .collect();

        Ok(logs)
    }

    async fn delete_older_than(&self, date: DateTime<Utc>) -> DatabaseResult<u64> {
        let client = self.db.lock().await;
        let client = client.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let date_str = date.to_rfc3339();
        let query = format!("DELETE FROM audit_log WHERE timestamp < '{}';", date_str);
        
        let result: Vec<serde_json::Value> = client.query(&query)
            .await
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(result.len() as u64)
    }
}

impl SurrealAuditRepository {
    /// Convert AuditAction to string
    fn action_to_string(&self, action: &AuditAction) -> String {
        match action {
            AuditAction::Create => "create",
            AuditAction::Read => "read",
            AuditAction::Update => "update",
            AuditAction::Delete => "delete",
            AuditAction::Login => "login",
            AuditAction::Logout => "logout",
            AuditAction::Export => "export",
            AuditAction::Import => "import",
            AuditAction::Custom(s) => s,
        }.to_string()
    }

    /// Parse a SurrealDB result into an AuditLog
    fn parse_audit_log(&self, value: serde_json::Value) -> DatabaseResult<AuditLog> {
        let obj = value.as_object().ok_or(DatabaseError::DeserializationError("Expected object".to_string()))?;

        let id = obj.get("id").and_then(|v| v.as_str()).and_then(|s| Uuid::parse_str(s).ok()).unwrap_or_else(Uuid::new_v4);
        let user_id = obj.get("user_id").and_then(|v| v.as_str()).and_then(|s| Uuid::parse_str(s).ok());
        let action_str = obj.get("action").and_then(|v| v.as_str()).unwrap_or("create");
        let action = match action_str {
            "create" => AuditAction::Create,
            "read" => AuditAction::Read,
            "update" => AuditAction::Update,
            "delete" => AuditAction::Delete,
            "login" => AuditAction::Login,
            "logout" => AuditAction::Logout,
            "export" => AuditAction::Export,
            "import" => AuditAction::Import,
            s => AuditAction::Custom(s.to_string()),
        };
        let entity_type = obj.get("entity_type").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let entity_id = obj.get("entity_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let old_values = obj.get("old_values").cloned();
        let new_values = obj.get("new_values").cloned();
        let timestamp = obj.get("timestamp").and_then(|v| v.as_str()).and_then(|s| DateTime::parse_from_rfc3339(s).ok()).unwrap_or_else(|| Utc::now());
        let ip_address = obj.get("ip_address").and_then(|v| v.as_str()).map(String::from);
        let user_agent = obj.get("user_agent").and_then(|v| v.as_str()).map(String::from);
        let success = obj.get("success").and_then(|v| v.as_bool()).unwrap_or(true);
        let error_message = obj.get("error_message").and_then(|v| v.as_str()).map(String::from);

        Ok(AuditLog {
            id,
            user_id,
            action,
            entity_type,
            entity_id,
            old_values,
            new_values,
            timestamp,
            ip_address,
            user_agent,
            success,
            error_message,
        })
    }
}

/// In-memory implementation of AuditRepository for testing
#[derive(Debug, Clone, Default)]
pub struct MemoryAuditRepository {
    /// In-memory storage
    pub logs: Arc<Mutex<Vec<AuditLog>>>,
}

impl MemoryAuditRepository {
    /// Create a new in-memory audit repository
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl AuditRepository for MemoryAuditRepository {
    async fn log(&self, audit_log: AuditLog) -> DatabaseResult<AuditLog> {
        let mut logs = self.logs.lock().await;
        logs.push(audit_log.clone());
        Ok(audit_log)
    }

    async fn find_by_user(&self, user_id: Uuid) -> DatabaseResult<Vec<AuditLog>> {
        let logs = self.logs.lock().await;
        Ok(logs.iter().filter(|l| l.user_id == Some(user_id)).cloned().collect())
    }

    async fn find_by_entity(&self, entity_type: &str, entity_id: &str) -> DatabaseResult<Vec<AuditLog>> {
        let logs = self.logs.lock().await;
        Ok(logs.iter()
            .filter(|l| l.entity_type == entity_type && l.entity_id == entity_id)
            .cloned()
            .collect())
    }

    async fn find_by_action(&self, action: AuditAction) -> DatabaseResult<Vec<AuditLog>> {
        let logs = self.logs.lock().await;
        Ok(logs.iter().filter(|l| l.action == action).cloned().collect())
    }

    async fn find_by_date_range(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> DatabaseResult<Vec<AuditLog>> {
        let logs = self.logs.lock().await;
        Ok(logs.iter()
            .filter(|l| l.timestamp >= start && l.timestamp <= end)
            .cloned()
            .collect())
    }

    async fn list_all(&self) -> DatabaseResult<Vec<AuditLog>> {
        let logs = self.logs.lock().await;
        Ok(logs.clone())
    }

    async fn delete_older_than(&self, date: DateTime<Utc>) -> DatabaseResult<u64> {
        let mut logs = self.logs.lock().await;
        let count = logs.len() as u64;
        logs.retain(|l| l.timestamp >= date);
        Ok(count - logs.len() as u64)
    }
}

/// Audit logger for convenient logging
#[derive(Debug, Clone)]
pub struct AuditLogger {
    /// Audit repository
    pub repository: Arc<dyn AuditRepository>,
}

impl AuditLogger {
    /// Create a new audit logger
    pub fn new(repository: Arc<dyn AuditRepository>) -> Self {
        Self { repository }
    }

    /// Log a create action
    pub async fn log_create(&self, user_id: Option<Uuid>, entity_type: &str, entity_id: &str, new_values: serde_json::Value) -> DatabaseResult<()> {
        let log = AuditLog {
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
        
        self.repository.log(log).await?;
        Ok(())
    }

    /// Log an update action
    pub async fn log_update(&self, user_id: Option<Uuid>, entity_type: &str, entity_id: &str, old_values: serde_json::Value, new_values: serde_json::Value) -> DatabaseResult<()> {
        let log = AuditLog {
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
        
        self.repository.log(log).await?;
        Ok(())
    }

    /// Log a delete action
    pub async fn log_delete(&self, user_id: Option<Uuid>, entity_type: &str, entity_id: &str, old_values: serde_json::Value) -> DatabaseResult<()> {
        let log = AuditLog {
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
        
        self.repository.log(log).await?;
        Ok(())
    }

    /// Log an error
    pub async fn log_error(&self, user_id: Option<Uuid>, entity_type: &str, entity_id: &str, action: AuditAction, error: &str) -> DatabaseResult<()> {
        let log = AuditLog {
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
        
        self.repository.log(log).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_memory_audit_repository() {
        let repo = MemoryAuditRepository::new();
        
        let log = AuditLog {
            user_id: Some(Uuid::new_v4()),
            action: AuditAction::Create,
            entity_type: "user".to_string(),
            entity_id: "123".to_string(),
            ..Default::default()
        };
        
        // Log an entry
        repo.log(log.clone()).await.unwrap();
        
        // Find by user
        let user_id = log.user_id.unwrap();
        let logs = repo.find_by_user(user_id).await.unwrap();
        assert_eq!(logs.len(), 1);
        
        // Find by entity
        let logs = repo.find_by_entity("user", "123").await.unwrap();
        assert_eq!(logs.len(), 1);
        
        // Find by action
        let logs = repo.find_by_action(AuditAction::Create).await.unwrap();
        assert_eq!(logs.len(), 1);
        
        // List all
        let all = repo.list_all().await.unwrap();
        assert_eq!(all.len(), 1);
    }

    #[tokio::test]
    async fn test_audit_logger() {
        let repo = Arc::new(MemoryAuditRepository::new());
        let logger = AuditLogger::new(repo);
        
        let user_id = Uuid::new_v4();
        let new_values = serde_json::json!({"name": "Test User", "email": "test@example.com"});
        
        // Log create
        logger.log_create(Some(user_id), "user", "123", new_values.clone()).await.unwrap();
        
        // Log update
        let old_values = serde_json::json!({"name": "Test User", "email": "old@example.com"});
        logger.log_update(Some(user_id), "user", "123", old_values, new_values).await.unwrap();
        
        // Log delete
        logger.log_delete(Some(user_id), "user", "123", new_values).await.unwrap();
        
        // Log error
        logger.log_error(Some(user_id), "user", "123", AuditAction::Delete, "Test error").await.unwrap();
        
        // Verify logs
        let all = repo.list_all().await.unwrap();
        assert_eq!(all.len(), 4);
    }
}
