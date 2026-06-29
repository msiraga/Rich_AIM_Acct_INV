//! API Module
//!
//! This module contains API-related functionality for the NexusLedger system.
//! 
//! # Submodules
//! - `rest`: REST API endpoints
//! - `graphql`: GraphQL API endpoints
//! - `websocket`: WebSocket API endpoints

use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::Mutex;
use tracing::{info, error, debug, warn};
use chrono::{DateTime, Utc};
use uuid::Uuid;
use crate::agents::orchestrator::AgentOrchestrator;
use crate::database::Database;
use crate::NexusLedger;

/// API configuration
#[derive(Debug, Clone)]
pub struct ApiConfig {
    /// API server host
    pub host: String,
    /// API server port
    pub port: u16,
    /// Whether to enable HTTPS
    pub enable_https: bool,
    /// SSL certificate path
    pub ssl_cert_path: Option<String>,
    /// SSL key path
    pub ssl_key_path: Option<String>,
    /// CORS allowed origins
    pub cors_origins: Vec<String>,
    /// API rate limit (requests per minute)
    pub rate_limit: u32,
    /// API timeout in seconds
    pub timeout: u64,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 8080,
            enable_https: false,
            ssl_cert_path: None,
            ssl_key_path: None,
            cors_origins: vec!["*".to_string()],
            rate_limit: 100,
            timeout: 30,
        }
    }
}

impl ApiConfig {
    /// Create a new API configuration
    pub fn new(host: &str, port: u16) -> Self {
        Self {
            host: host.to_string(),
            port,
            ..Default::default()
        }
    }

    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        Self {
            host: std::env::var("API_HOST").unwrap_or_else(|_| "localhost".to_string()),
            port: std::env::var("API_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(8080),
            enable_https: std::env::var("API_ENABLE_HTTPS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(false),
            ssl_cert_path: std::env::var("API_SSL_CERT_PATH").ok(),
            ssl_key_path: std::env::var("API_SSL_KEY_PATH").ok(),
            cors_origins: std::env::var("API_CORS_ORIGINS")
                .ok()
                .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_else(|| vec!["*".to_string()]),
            rate_limit: std::env::var("API_RATE_LIMIT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(100),
            timeout: std::env::var("API_TIMEOUT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(30),
        }
    }
}

/// API server struct
#[derive(Debug, Clone)]
pub struct ApiServer {
    /// API configuration
    pub config: ApiConfig,
    /// Agent orchestrator
    pub orchestrator: Arc<Mutex<AgentOrchestrator>>,
    /// Database connection
    pub database: Arc<Mutex<Database>>,
    /// NexusLedger instance
    pub nexus: Arc<Mutex<NexusLedger>>,
}

impl ApiServer {
    /// Create a new API server
    pub fn new(
        config: ApiConfig,
        orchestrator: Arc<Mutex<AgentOrchestrator>>,
        database: Arc<Mutex<Database>>,
        nexus: Arc<Mutex<NexusLedger>>,
    ) -> Self {
        Self {
            config,
            orchestrator,
            database,
            nexus,
        }
    }

    /// Start the API server
    pub async fn start(&self) -> Result<(), anyhow::Error> {
        info!("Starting API server on {}:{}", self.config.host, self.config.port);
        
        // In a real implementation, this would start the HTTP server
        // For now, we'll just log the information
        
        info!("API server configured:");
        info!("  Host: {}", self.config.host);
        info!("  Port: {}", self.config.port);
        info!("  HTTPS: {}", self.config.enable_https);
        info!("  CORS Origins: {:?}", self.config.cors_origins);
        info!("  Rate Limit: {}", self.config.rate_limit);
        info!("  Timeout: {}", self.config.timeout);
        
        Ok(())
    }

    /// Stop the API server
    pub async fn stop(&self) -> Result<(), anyhow::Error> {
        info!("Stopping API server...");
        Ok(())
    }
}

/// API response wrapper
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ApiResponse<T> {
    /// Whether the request was successful
    pub success: bool,
    /// Response data
    pub data: Option<T>,
    /// Error message (if any)
    pub error: Option<String>,
    /// Response metadata
    pub metadata: ApiResponseMetadata,
}

impl<T> ApiResponse<T> {
    /// Create a successful response
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            metadata: ApiResponseMetadata::default(),
        }
    }

    /// Create an error response
    pub fn error(error: &str) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(error.to_string()),
            metadata: ApiResponseMetadata::default(),
        }
    }

    /// Add metadata
    pub fn with_metadata(mut self, metadata: ApiResponseMetadata) -> Self {
        self.metadata = metadata;
        self
    }
}

/// API response metadata
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ApiResponseMetadata {
    /// Request ID
    pub request_id: String,
    /// Timestamp
    pub timestamp: DateTime<chrono::Utc>,
    /// Response time in milliseconds
    pub response_time_ms: u64,
    /// API version
    pub api_version: String,
}

impl ApiResponseMetadata {
    /// Create new metadata
    pub fn new() -> Self {
        Self {
            request_id: Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now(),
            response_time_ms: 0,
            api_version: "v1".to_string(),
        }
    }
}

/// API error types
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// Not found
    #[error("Not found: {0}")]
    NotFound(String),
    
    /// Bad request
    #[error("Bad request: {0}")]
    BadRequest(String),
    
    /// Unauthorized
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    
    /// Forbidden
    #[error("Forbidden: {0}")]
    Forbidden(String),
    
    /// Rate limit exceeded
    #[error("Rate limit exceeded")]
    RateLimitExceeded,
    
    /// Internal server error
    #[error("Internal server error: {0}")]
    InternalServerError(String),
    
    /// Service unavailable
    #[error("Service unavailable")]
    ServiceUnavailable,
}

impl ApiError {
    /// Get HTTP status code
    pub fn status_code(&self) -> u16 {
        match self {
            Self::NotFound(_) => 404,
            Self::BadRequest(_) => 400,
            Self::Unauthorized(_) => 401,
            Self::Forbidden(_) => 403,
            Self::RateLimitExceeded => 429,
            Self::InternalServerError(_) => 500,
            Self::ServiceUnavailable => 503,
        }
    }

    /// Get error message
    pub fn message(&self) -> String {
        match self {
            Self::NotFound(msg) => format!("Not found: {}", msg),
            Self::BadRequest(msg) => format!("Bad request: {}", msg),
            Self::Unauthorized(msg) => format!("Unauthorized: {}", msg),
            Self::Forbidden(msg) => format!("Forbidden: {}", msg),
            Self::RateLimitExceeded => "Rate limit exceeded".to_string(),
            Self::InternalServerError(msg) => format!("Internal server error: {}", msg),
            Self::ServiceUnavailable => "Service unavailable".to_string(),
        }
    }
}

/// Convert ApiError to ApiResponse
impl<T> From<ApiError> for ApiResponse<T> {
    fn from(error: ApiError) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(error.message()),
            metadata: ApiResponseMetadata::new(),
        }
    }
}

/// API endpoint handler trait
#[async_trait::async_trait]
pub trait ApiHandler: Send + Sync {
    /// Handle a GET request
    async fn handle_get(&self, path: &str, params: HashMap<String, String>) -> Result<ApiResponse<serde_json::Value>, ApiError>;
    
    /// Handle a POST request
    async fn handle_post(&self, path: &str, body: serde_json::Value) -> Result<ApiResponse<serde_json::Value>, ApiError>;
    
    /// Handle a PUT request
    async fn handle_put(&self, path: &str, body: serde_json::Value) -> Result<ApiResponse<serde_json::Value>, ApiError>;
    
    /// Handle a DELETE request
    async fn handle_delete(&self, path: &str) -> Result<ApiResponse<serde_json::Value>, ApiError>;
    
    /// Handle a PATCH request
    async fn handle_patch(&self, path: &str, body: serde_json::Value) -> Result<ApiResponse<serde_json::Value>, ApiError>;
}

/// Default API handler implementation
#[derive(Debug, Clone)]
pub struct DefaultApiHandler {
    /// Agent orchestrator
    pub orchestrator: Arc<Mutex<AgentOrchestrator>>,
    /// Database connection
    pub database: Arc<Mutex<Database>>,
    /// NexusLedger instance
    pub nexus: Arc<Mutex<NexusLedger>>,
}

impl DefaultApiHandler {
    /// Create a new default API handler
    pub fn new(
        orchestrator: Arc<Mutex<AgentOrchestrator>>,
        database: Arc<Mutex<Database>>,
        nexus: Arc<Mutex<NexusLedger>>,
    ) -> Self {
        Self {
            orchestrator,
            database,
            nexus,
        }
    }
}

#[async_trait::async_trait]
impl ApiHandler for DefaultApiHandler {
    async fn handle_get(&self, path: &str, params: HashMap<String, String>) -> Result<ApiResponse<serde_json::Value>, ApiError> {
        debug!("GET {} with params: {:?}", path, params);
        
        match path {
            "/api/v1/status" => self.handle_status().await,
            "/api/v1/agents" => self.handle_list_agents().await,
            "/api/v1/accounts" => self.handle_list_accounts().await,
            "/api/v1/transactions" => self.handle_list_transactions().await,
            _ => Err(ApiError::NotFound(path.to_string())),
        }
    }

    async fn handle_post(&self, path: &str, body: serde_json::Value) -> Result<ApiResponse<serde_json::Value>, ApiError> {
        debug!("POST {} with body: {:?}", path, body);
        
        match path {
            "/api/v1/transactions" => self.handle_create_transaction(body).await,
            "/api/v1/agents/tasks" => self.handle_create_task(body).await,
            _ => Err(ApiError::NotFound(path.to_string())),
        }
    }

    async fn handle_put(&self, path: &str, body: serde_json::Value) -> Result<ApiResponse<serde_json::Value>, ApiError> {
        debug!("PUT {} with body: {:?}", path, body);
        Err(ApiError::NotFound(path.to_string()))
    }

    async fn handle_delete(&self, path: &str) -> Result<ApiResponse<serde_json::Value>, ApiError> {
        debug!("DELETE {}", path);
        Err(ApiError::NotFound(path.to_string()))
    }

    async fn handle_patch(&self, path: &str, body: serde_json::Value) -> Result<ApiResponse<serde_json::Value>, ApiError> {
        debug!("PATCH {} with body: {:?}", path, body);
        Err(ApiError::NotFound(path.to_string()))
    }
}

impl DefaultApiHandler {
    /// Handle status endpoint
    async fn handle_status(&self) -> Result<ApiResponse<serde_json::Value>, ApiError> {
        let orchestrator = self.orchestrator.lock().await;
        let system_status = orchestrator.get_system_status().await;
        
        let status_data = serde_json::json!({
            "status": "ok",
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "agents": system_status.total_agents,
            "tasks_processed": system_status.total_tasks_processed,
            "tasks_failed": system_status.total_tasks_failed,
            "health_score": system_status.health_score,
        });
        
        Ok(ApiResponse::success(status_data))
    }

    /// Handle list agents endpoint
    async fn handle_list_agents(&self) -> Result<ApiResponse<serde_json::Value>, ApiError> {
        let orchestrator = self.orchestrator.lock().await;
        let agents = orchestrator.agents.read().await;
        
        let agents_data: Vec<serde_json::Value> = agents.values()
            .map(|agent| {
                let agent_guard = agent.blocking_lock();
                serde_json::json!({
                    "id": agent_guard.config().id.to_string(),
                    "name": agent_guard.config().name.clone(),
                    "type": format!("{:?}", agent_guard.config().agent_type),
                    "status": format!("{:?}", agent_guard.status()),
                })
            })
            .collect();
        
        Ok(ApiResponse::success(serde_json::json!(agents_data)))
    }

    /// Handle list accounts endpoint
    async fn handle_list_accounts(&self) -> Result<ApiResponse<serde_json::Value>, ApiError> {
        let nexus = self.nexus.lock().await;
        let accounts = nexus.ledger.list_accounts().await
            .map_err(|e| ApiError::InternalServerError(e.to_string()))?;
        
        let accounts_data: Vec<serde_json::Value> = accounts.into_iter()
            .map(|acc| serde_json::json!({
                "id": acc.id.to_string(),
                "number": acc.number,
                "name": acc.name,
                "type": format!("{:?}", acc.account_type),
                "balance": acc.balance.to_string(),
            }))
            .collect();
        
        Ok(ApiResponse::success(serde_json::json!(accounts_data)))
    }

    /// Handle list transactions endpoint
    async fn handle_list_transactions(&self) -> Result<ApiResponse<serde_json::Value>, ApiError> {
        let nexus = self.nexus.lock().await;
        let transactions = nexus.ledger.list_transactions().await
            .map_err(|e| ApiError::InternalServerError(e.to_string()))?;
        
        let transactions_data: Vec<serde_json::Value> = transactions.into_iter()
            .map(|txn| serde_json::json!({
                "id": txn.id.to_string(),
                "number": txn.number,
                "description": txn.description,
                "date": txn.date.to_rfc3339(),
                "status": format!("{:?}", txn.status),
                "total_amount": txn.total_amount().to_string(),
            }))
            .collect();
        
        Ok(ApiResponse::success(serde_json::json!(transactions_data)))
    }

    /// Handle create transaction endpoint
    async fn handle_create_transaction(&self, body: serde_json::Value) -> Result<ApiResponse<serde_json::Value>, ApiError> {
        // In a real implementation, we would parse the transaction from the body
        // For now, we'll return a mock response
        
        let transaction_data = serde_json::json!({
            "id": Uuid::new_v4().to_string(),
            "number": "TRX-MOCK",
            "description": "Mock transaction",
            "date": chrono::Utc::now().to_rfc3339(),
            "status": "Draft",
        });
        
        Ok(ApiResponse::success(transaction_data))
    }

    /// Handle create task endpoint
    async fn handle_create_task(&self, body: serde_json::Value) -> Result<ApiResponse<serde_json::Value>, ApiError> {
        // In a real implementation, we would parse the task from the body
        // For now, we'll return a mock response
        
        let task_data = serde_json::json!({
            "id": Uuid::new_v4().to_string(),
            "status": "Pending",
        });
        
        Ok(ApiResponse::success(task_data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[test]
    fn test_api_config_default() {
        let config = ApiConfig::default();
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 8080);
        assert!(!config.enable_https);
    }

    #[test]
    fn test_api_config_creation() {
        let config = ApiConfig::new("example.com", 8000);
        assert_eq!(config.host, "example.com");
        assert_eq!(config.port, 8000);
    }

    #[test]
    fn test_api_response() {
        let data = serde_json::json!({"test": "value"});
        let response = ApiResponse::success(data);
        assert!(response.success);
        assert!(response.data.is_some());
        assert!(response.error.is_none());
        
        let error_response = ApiResponse::<serde_json::Value>::error("Test error");
        assert!(!error_response.success);
        assert!(error_response.error.is_some());
    }

    #[test]
    fn test_api_error() {
        let error = ApiError::NotFound("Resource not found".to_string());
        assert_eq!(error.status_code(), 404);
        assert_eq!(error.message(), "Not found: Resource not found");
        
        let error = ApiError::BadRequest("Invalid data".to_string());
        assert_eq!(error.status_code(), 400);
        
        let error = ApiError::Unauthorized("Not authorized".to_string());
        assert_eq!(error.status_code(), 401);
        
        let error = ApiError::RateLimitExceeded;
        assert_eq!(error.status_code(), 429);
    }

    #[tokio::test]
    async fn test_api_server_creation() {
        let config = ApiConfig::default();
        let orchestrator = Arc::new(Mutex::new(AgentOrchestrator::new()));
        let database = Arc::new(Mutex::new(Database::new()));
        let nexus = Arc::new(Mutex::new(NexusLedger::new()));
        
        let server = ApiServer::new(config, orchestrator, database, nexus);
        assert_eq!(server.config.host, "localhost");
    }
}
