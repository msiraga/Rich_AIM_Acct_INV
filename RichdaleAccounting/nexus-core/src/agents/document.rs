//! Document Agent Module
//!
//! Handles document processing, storage, and retrieval.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use crate::agents::agent_types::{Agent, AgentConfig, AgentType, AgentStatus};
use crate::agents::task::{Task, TaskResult, TaskPayload};
use crate::agents::error::AgentError;
use crate::database::models::Document;
use crate::database::BoundingBox;

/// Document Agent for handling document-related tasks
#[derive(Debug, Clone)]
pub struct DocumentAgent {
    /// Agent configuration
    pub config: AgentConfig,
    /// Current status
    pub status: AgentStatus,
    /// Database connection
    pub db: Arc<Mutex<Option<surrealdb::Surreal<surrealdb::engine::remote::ws::Client>>>>,
}

impl DocumentAgent {
    /// Create a new document agent
    pub fn new(config: AgentConfig, db: Arc<Mutex<Option<surrealdb::Surreal<surrealdb::engine::remote::ws::Client>>>>) -> Self {
        Self {
            config,
            status: AgentStatus::Idle,
            db,
        }
    }

    /// Create a document agent with default configuration
    pub fn with_defaults(db: Arc<Mutex<Option<surrealdb::Surreal<surrealdb::engine::remote::ws::Client>>>>) -> Self {
        let config = AgentConfig::document_agent();
        Self::new(config, db)
    }
}

#[async_trait]
impl Agent for DocumentAgent {
    fn config(&self) -> &AgentConfig {
        &self.config
    }

    fn status(&self) -> AgentStatus {
        self.status.clone()
    }

    async fn initialize(&mut self) -> Result<(), anyhow::Error> {
        self.status = AgentStatus::Initializing;
        
        // Initialize database connection if not already set
        // (In a real implementation, this would connect to the database)
        
        self.status = AgentStatus::Idle;
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), anyhow::Error> {
        self.status = AgentStatus::ShuttingDown;
        
        // Clean up resources
        // (In a real implementation, this would close connections)
        
        self.status = AgentStatus::Idle;
        Ok(())
    }

    async fn process_task(&self, task: Task) -> Result<Task, anyhow::Error> {
        if !self.config.enabled {
            return Err(AgentError::AgentDisabled(self.config.name.clone()).into());
        }

        match task.task_type {
            crate::agents::task::TaskType::StoreDocument => {
                self.process_store_document(task).await
            }
            crate::agents::task::TaskType::RetrieveDocument => {
                self.process_retrieve_document(task).await
            }
            _ => {
                Err(AgentError::TaskProcessingFailed(
                    format!("DocumentAgent cannot handle task type: {:?}", task.task_type)
                ).into())
            }
        }
    }

    fn agent_type(&self) -> AgentType {
        self.config.agent_type.clone()
    }
}

impl DocumentAgent {
    /// Process a store document task
    async fn process_store_document(&self, task: Task) -> Result<Task, anyhow::Error> {
        self.status = AgentStatus::Busy;
        
        let start_time = std::time::Instant::now();
        
        // Extract document from task payload
        let document = match task.payload {
            TaskPayload::Document(doc) => doc,
            _ => return Err(AgentError::TaskProcessingFailed(
                "Expected Document payload for StoreDocument task".to_string()
            ).into()),
        };

        // Store the document in the database
        let stored_document = self.store_document(document).await?;
        
        // Create success result
        let result = TaskResult::success_with_data(
            "Document stored successfully",
            TaskPayload::Document(stored_document)
        );
        
        let processing_time = start_time.elapsed().as_millis() as f64;
        
        self.status = AgentStatus::Idle;
        
        Ok(task.complete(result))
    }

    /// Process a retrieve document task
    async fn process_retrieve_document(&self, task: Task) -> Result<Task, anyhow::Error> {
        self.status = AgentStatus::Busy;
        
        let start_time = std::time::Instant::now();
        
        // Extract document ID from task payload
        let document_id = match task.payload {
            TaskPayload::String(id) => id,
            TaskPayload::Json(value) => value.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            _ => return Err(AgentError::TaskProcessingFailed(
                "Expected String or JSON payload with document ID for RetrieveDocument task".to_string()
            ).into()),
        };

        // Retrieve the document from the database
        let document = self.retrieve_document(&document_id).await?;
        
        // Create success result
        let result = TaskResult::success_with_data(
            "Document retrieved successfully",
            TaskPayload::Document(document)
        );
        
        let processing_time = start_time.elapsed().as_millis() as f64;
        
        self.status = AgentStatus::Idle;
        
        Ok(task.complete(result))
    }

    /// Store a document in the database
    async fn store_document(&self, document: Document) -> Result<Document, anyhow::Error> {
        // In a real implementation, this would store the document in SurrealDB
        // For now, we'll just return the document with an updated timestamp
        
        let mut stored_document = document;
        stored_document.created_at = Utc::now();
        stored_document.updated_at = Utc::now();
        
        Ok(stored_document)
    }

    /// Retrieve a document from the database
    async fn retrieve_document(&self, document_id: &str) -> Result<Document, anyhow::Error> {
        // In a real implementation, this would retrieve the document from SurrealDB
        // For now, we'll return a mock document
        
        if document_id.is_empty() {
            return Err(AgentError::TaskProcessingFailed("Document ID cannot be empty".to_string()).into());
        }
        
        Ok(Document {
            id: document_id.to_string(),
            name: format!("Document {}", document_id),
            document_type: crate::database::models::DocumentType::Other,
            content: vec![],
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            bounding_box: Some(crate::database::BoundingBox::default()),
        })
    }

    /// Extract text from a document using OCR or other methods
    pub async fn extract_text(&self, document: &Document) -> Result<String, anyhow::Error> {
        // In a real implementation, this would use OCR or text extraction
        // For now, we'll return a placeholder
        
        if document.content.is_empty() {
            return Err(AgentError::TaskProcessingFailed("Document has no content".to_string()).into());
        }
        
        Ok("Extracted text from document".to_string())
    }

    /// Analyze document content and extract structured data
    pub async fn analyze_document(&self, document: &Document) -> Result<serde_json::Value, anyhow::Error> {
        // In a real implementation, this would use AI/ML to analyze the document
        // For now, we'll return a mock analysis
        
        let mut analysis = serde_json::json!({
            "type": "unknown",
            "entities": [],
            "amounts": [],
            "dates": [],
            "confidence": 0.0
        });
        
        // Simple analysis based on document name
        if document.name.contains("invoice") || document.name.contains("Invoice") {
            analysis["type"] = serde_json::json!("invoice");
            analysis["confidence"] = serde_json::json!(0.8);
        } else if document.name.contains("receipt") || document.name.contains("Receipt") {
            analysis["type"] = serde_json::json!("receipt");
            analysis["confidence"] = serde_json::json!(0.7);
        }
        
        Ok(analysis)
    }

    /// Classify document type
    pub async fn classify_document(&self, document: &Document) -> Result<crate::database::models::DocumentType, anyhow::Error> {
        // In a real implementation, this would use AI/ML to classify the document
        // For now, we'll use simple heuristics
        
        let name_lower = document.name.to_lowercase();
        
        if name_lower.contains("invoice") {
            Ok(crate::database::models::DocumentType::Invoice)
        } else if name_lower.contains("receipt") {
            Ok(crate::database::models::DocumentType::Receipt)
        } else if name_lower.contains("bank") || name_lower.contains("statement") {
            Ok(crate::database::models::DocumentType::BankStatement)
        } else if name_lower.contains("check") {
            Ok(crate::database::models::DocumentType::Check)
        } else if name_lower.contains("purchase") || name_lower.contains("order") {
            Ok(crate::database::models::DocumentType::PurchaseOrder)
        } else if name_lower.contains("tax") {
            Ok(crate::database::models::DocumentType::TaxForm)
        } else if name_lower.contains("contract") {
            Ok(crate::database::models::DocumentType::Contract)
        } else {
            Ok(crate::database::models::DocumentType::Other)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_document_agent_creation() {
        let db = Arc::new(Mutex::new(None));
        let agent = DocumentAgent::with_defaults(db);
        assert_eq!(agent.config.agent_type, AgentType::DocumentAgent);
        assert_eq!(agent.status, AgentStatus::Idle);
    }

    #[tokio::test]
    async fn test_document_agent_initialization() {
        let db = Arc::new(Mutex::new(None));
        let mut agent = DocumentAgent::with_defaults(db);
        let result = agent.initialize().await;
        assert!(result.is_ok());
        assert_eq!(agent.status, AgentStatus::Idle);
    }

    #[tokio::test]
    async fn test_store_document_task() {
        let db = Arc::new(Mutex::new(None));
        let agent = DocumentAgent::with_defaults(db);
        
        let document = Document {
            id: "test-doc-1".to_string(),
            name: "Test Document".to_string(),
            document_type: crate::database::models::DocumentType::Other,
            content: vec![1, 2, 3],
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            bounding_box: Some(crate::database::BoundingBox::default()),
        };
        
        let task = Task::store_document(document);
        let result = agent.process_task(task).await;
        
        assert!(result.is_ok());
        let completed_task = result.unwrap();
        assert_eq!(completed_task.status, crate::agents::task::TaskStatus::Completed);
        assert!(completed_task.result.is_some());
    }

    #[tokio::test]
    async fn test_retrieve_document_task() {
        let db = Arc::new(Mutex::new(None));
        let agent = DocumentAgent::with_defaults(db);
        
        let task = Task::new(crate::agents::task::TaskType::RetrieveDocument);
        let task = task.with_payload(TaskPayload::String("test-doc-1".to_string()));
        
        let result = agent.process_task(task).await;
        
        assert!(result.is_ok());
        let completed_task = result.unwrap();
        assert_eq!(completed_task.status, crate::agents::task::TaskStatus::Completed);
    }

    #[tokio::test]
    async fn test_document_classification() {
        let db = Arc::new(Mutex::new(None));
        let agent = DocumentAgent::with_defaults(db);
        
        let document = Document {
            id: "invoice-1".to_string(),
            name: "Invoice #123".to_string(),
            document_type: crate::database::models::DocumentType::Other,
            content: vec![],
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            bounding_box: Some(crate::database::BoundingBox::default()),
        };
        
        let doc_type = agent.classify_document(&document).await.unwrap();
        assert_eq!(doc_type, crate::database::models::DocumentType::Invoice);
    }
}

// Add a helper method to Task for setting payload
impl Task {
    pub fn with_payload(mut self, payload: TaskPayload) -> Self {
        self.payload = payload;
        self
    }
}
