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
    pub db: Arc<Mutex<Option<surrealdb::Surreal<surrealdb::engine::local::Db>>>>,
}

impl SurrealAuditRepository {
    /// Create a new SurrealDB audit repository
    pub fn new(db: Arc<Mutex<Option<surrealdb::Surreal<surrealdb::engine::local::Db>>>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl AuditRepository for SurrealAuditRepository {
    async fn log(&self, audit_log: AuditLog) -> DatabaseResult<AuditLog> {
        let guard = self.db.lock().await;
        let client = guard.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let user_id_str = audit_log.user_id.map(|id| id.to_string());
        let action_str = self.action_to_string(&audit_log.action);
        let old_values_str = audit_log.old_values.as_ref().map(|v| v.to_string());
        let new_values_str = audit_log.new_values.as_ref().map(|v| v.to_string());

        let mut response = client.query(
            "CREATE audit_log SET \
             user_id = $user_id, \
             action = $action, \
             entity_type = $entity_type, \
             entity_id = $entity_id, \
             old_values = $old_values, \
             new_values = $new_values, \
             timestamp = $timestamp, \
             ip_address = $ip_address, \
             user_agent = $user_agent, \
             success = $success, \
             error_message = $error_message"
        )
        .bind(("user_id", user_id_str))
        .bind(("action", action_str))
        .bind(("entity_type", audit_log.entity_type.clone()))
        .bind(("entity_id", audit_log.entity_id.clone()))
        .bind(("old_values", old_values_str))
        .bind(("new_values", new_values_str))
        .bind(("timestamp", audit_log.timestamp.to_rfc3339()))
        .bind(("ip_address", audit_log.ip_address.clone()))
        .bind(("user_agent", audit_log.user_agent.clone()))
        .bind(("success", audit_log.success))
        .bind(("error_message", audit_log.error_message.clone()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let results: Vec<serde_json::Value> = response.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        if let Some(val) = results.into_iter().next() {
            self.parse_audit_log(val)
        } else {
            Ok(audit_log)
        }
    }

    async fn find_by_user(&self, user_id: Uuid) -> DatabaseResult<Vec<AuditLog>> {
        let guard = self.db.lock().await;
        let client = guard.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let mut response = client.query(
            "SELECT * FROM audit_log WHERE user_id = $user_id ORDER BY timestamp DESC"
        )
        .bind(("user_id", user_id.to_string()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let results: Vec<serde_json::Value> = response.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        results.into_iter()
            .map(|val| self.parse_audit_log(val))
            .collect()
    }

    async fn find_by_entity(&self, entity_type: &str, entity_id: &str) -> DatabaseResult<Vec<AuditLog>> {
        let guard = self.db.lock().await;
        let client = guard.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let mut response = client.query(
            "SELECT * FROM audit_log WHERE entity_type = $entity_type AND entity_id = $entity_id ORDER BY timestamp DESC"
        )
        .bind(("entity_type", entity_type.to_string()))
        .bind(("entity_id", entity_id.to_string()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let results: Vec<serde_json::Value> = response.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        results.into_iter()
            .map(|val| self.parse_audit_log(val))
            .collect()
    }

    async fn find_by_action(&self, action: AuditAction) -> DatabaseResult<Vec<AuditLog>> {
        let guard = self.db.lock().await;
        let client = guard.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let action_str = self.action_to_string(&action);

        let mut response = client.query(
            "SELECT * FROM audit_log WHERE action = $action ORDER BY timestamp DESC"
        )
        .bind(("action", action_str))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let results: Vec<serde_json::Value> = response.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        results.into_iter()
            .map(|val| self.parse_audit_log(val))
            .collect()
    }

    async fn find_by_date_range(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> DatabaseResult<Vec<AuditLog>> {
        let guard = self.db.lock().await;
        let client = guard.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let mut response = client.query(
            "SELECT * FROM audit_log WHERE timestamp >= $start AND timestamp <= $end ORDER BY timestamp DESC"
        )
        .bind(("start", start.to_rfc3339()))
        .bind(("end", end.to_rfc3339()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let results: Vec<serde_json::Value> = response.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        results.into_iter()
            .map(|val| self.parse_audit_log(val))
            .collect()
    }

    async fn list_all(&self) -> DatabaseResult<Vec<AuditLog>> {
        let guard = self.db.lock().await;
        let client = guard.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let mut response = client.query(
            "SELECT * FROM audit_log ORDER BY timestamp DESC"
        )
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let results: Vec<serde_json::Value> = response.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        results.into_iter()
            .map(|val| self.parse_audit_log(val))
            .collect()
    }

    async fn delete_older_than(&self, date: DateTime<Utc>) -> DatabaseResult<u64> {
        let guard = self.db.lock().await;
        let client = guard.as_ref().ok_or(DatabaseError::NotInitialized)?;

        // Count the records that will be deleted
        let mut count_response = client.query(
            "SELECT count() FROM audit_log WHERE timestamp < $date GROUP ALL"
        )
        .bind(("date", date.to_rfc3339()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let count_results: Vec<serde_json::Value> = count_response.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let count = count_results.first()
            .and_then(|v| v.get("count"))
            .and_then(|c| c.as_u64())
            .unwrap_or(0);

        // Delete the old records
        client.query(
            "DELETE FROM audit_log WHERE timestamp < $date"
        )
        .bind(("date", date.to_rfc3339()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(count)
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
        let timestamp = obj.get("timestamp").and_then(|v| v.as_str()).and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|dt| dt.with_timezone(&Utc)).unwrap_or_else(|| Utc::now());
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
            prev_hash: None,
            chain_hash: None,
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
#[derive(Clone)]
pub struct AuditLogger {
    /// Audit repository
    pub repository: Arc<dyn AuditRepository>,
}

impl std::fmt::Debug for AuditLogger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AuditLogger {{ repository: <dyn AuditRepository> }}")
    }
}

impl AuditLogger {
    /// Create a new audit logger
    pub fn new(repository: Arc<dyn AuditRepository>) -> Self {
        Self { repository }
    }

    /// Log a create action
    pub async fn log_create(&self, user_id: Option<Uuid>, entity_type: &str, entity_id: &str, new_values: serde_json::Value) -> DatabaseResult<()> {
        let log = AuditLog {
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
        
        self.repository.log(log).await?;
        Ok(())
    }

    /// Log an update action
    pub async fn log_update(&self, user_id: Option<Uuid>, entity_type: &str, entity_id: &str, old_values: serde_json::Value, new_values: serde_json::Value) -> DatabaseResult<()> {
        let log = AuditLog {
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
        
        self.repository.log(log).await?;
        Ok(())
    }

    /// Log a delete action
    pub async fn log_delete(&self, user_id: Option<Uuid>, entity_type: &str, entity_id: &str, old_values: serde_json::Value) -> DatabaseResult<()> {
        let log = AuditLog {
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
        
        self.repository.log(log).await?;
        Ok(())
    }

    /// Log an error
    pub async fn log_error(&self, user_id: Option<Uuid>, entity_type: &str, entity_id: &str, action: AuditAction, error: &str) -> DatabaseResult<()> {
        let log = AuditLog {
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
        let logger = AuditLogger::new(repo.clone());
        
        let user_id = Uuid::new_v4();
        let new_values = serde_json::json!({"name": "Test User", "email": "test@example.com"});
        
        // Log create
        logger.log_create(Some(user_id), "user", "123", new_values.clone()).await.unwrap();
        
        // Log update
        let old_values = serde_json::json!({"name": "Test User", "email": "old@example.com"});
        logger.log_update(Some(user_id), "user", "123", old_values, new_values.clone()).await.unwrap();

        // Log delete
        logger.log_delete(Some(user_id), "user", "123", new_values).await.unwrap();
        
        // Log error
        logger.log_error(Some(user_id), "user", "123", AuditAction::Delete, "Test error").await.unwrap();
        
        // Verify logs
        let all = repo.list_all().await.unwrap();
        assert_eq!(all.len(), 4);
    }
}
