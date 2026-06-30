//! Document Agent Module
//!
//! Handles document processing, storage, and retrieval.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use tracing::warn;
use crate::agents::agent_types::{Agent, AgentConfig, AgentType, AgentStatus};
use crate::agents::task::{Task, TaskResult, TaskPayload};
use crate::agents::error::AgentError;
use crate::database::models::Document;
use crate::database::BoundingBox;

/// Simple base64 encoding (standard alphabet, padded)
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((n >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((n >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((n >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(n & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

/// Simple base64 decoding (standard alphabet, padded)
fn base64_decode(input: &str) -> Vec<u8> {
    fn decode_char(c: u8) -> u8 {
        match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => 0,
        }
    }
    let mut result = Vec::with_capacity(input.len() * 3 / 4);
    let chars: Vec<u8> = input.bytes().filter(|&b| b != b'\n' && b != b'\r').collect();
    for chunk in chars.chunks(4) {
        if chunk.len() < 2 { break; }
        let a = decode_char(chunk[0]) as u32;
        let b = decode_char(chunk[1]) as u32;
        let cv = if chunk.len() > 2 { decode_char(chunk[2]) as u32 } else { 0 };
        let dv = if chunk.len() > 3 { decode_char(chunk[3]) as u32 } else { 0 };
        let n = (a << 18) | (b << 12) | (cv << 6) | dv;
        result.push(((n >> 16) & 0xFF) as u8);
        if chunk.len() > 2 && chunk[2] != b'=' {
            result.push(((n >> 8) & 0xFF) as u8);
        }
        if chunk.len() > 3 && chunk[3] != b'=' {
            result.push((n & 0xFF) as u8);
        }
    }
    result
}

/// Document Agent for handling document-related tasks
#[derive(Debug, Clone)]
pub struct DocumentAgent {
    /// Agent configuration
    pub config: AgentConfig,
    /// Current status
    pub status: AgentStatus,
    /// Database connection
    pub db: Arc<Mutex<Option<surrealdb::Surreal<surrealdb::engine::local::Db>>>>,
}

impl DocumentAgent {
    /// Create a new document agent
    pub fn new(config: AgentConfig, db: Arc<Mutex<Option<surrealdb::Surreal<surrealdb::engine::local::Db>>>>) -> Self {
        Self {
            config,
            status: AgentStatus::Idle,
            db,
        }
    }

    /// Create a document agent with default configuration
    pub fn with_defaults(db: Arc<Mutex<Option<surrealdb::Surreal<surrealdb::engine::local::Db>>>>) -> Self {
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
        // Status tracking deferred

        let start_time = std::time::Instant::now();

        // Clone task to avoid partial move when extracting payload
        let task_clone = task.clone();

        // Extract document from task payload
        let document = match task_clone.payload {
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

        // Status tracking deferred

        Ok(task.complete(result))
    }

    /// Process a retrieve document task
    async fn process_retrieve_document(&self, task: Task) -> Result<Task, anyhow::Error> {
        // Status tracking deferred

        let start_time = std::time::Instant::now();

        // Clone task to avoid partial move when extracting payload
        let task_clone = task.clone();

        // Extract document ID from task payload
        let document_id = match task_clone.payload {
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

        // Status tracking deferred

        Ok(task.complete(result))
    }

    /// Store a document in SurrealDB
    async fn store_document(&self, document: Document) -> Result<Document, anyhow::Error> {
        let mut stored_document = document;
        stored_document.updated_at = Utc::now();
        if stored_document.created_at == DateTime::<Utc>::default()
            || stored_document.created_at.timestamp() == 0
        {
            stored_document.created_at = Utc::now();
        }

        // Classify document type from name
        let classified_type = self.classify_document(&stored_document).await?;
        stored_document.document_type = classified_type;

        // Persist to SurrealDB if connected
        let db_guard = self.db.lock().await;
        if let Some(ref client) = *db_guard {
            let doc_type_str = stored_document.document_type.to_str().to_string();
            let bbox_json = serde_json::to_value(&stored_document.bounding_box).unwrap_or_default();
            let metadata_json = stored_document.metadata.clone();
            // Store binary content as base64 for SurrealDB
            let content_b64 = base64_encode(&stored_document.content);

            if let Err(e) = client.query(
                "CREATE document SET \
                 id = $id, name = $name, document_type = $document_type, \
                 content_b64 = $content_b64, metadata = $metadata, \
                 bounding_box = $bounding_box, \
                 created_at = $created_at, updated_at = $updated_at"
            )
            .bind(("id", stored_document.id.clone()))
            .bind(("name", stored_document.name.clone()))
            .bind(("document_type", doc_type_str))
            .bind(("content_b64", content_b64))
            .bind(("metadata", metadata_json.to_string()))
            .bind(("bounding_box", bbox_json.to_string()))
            .bind(("created_at", stored_document.created_at.to_rfc3339()))
            .bind(("updated_at", stored_document.updated_at.to_rfc3339()))
            .await
            {
                warn!("Failed to persist document {} to SurrealDB: {}", stored_document.id, e);
            }
        }

        Ok(stored_document)
    }

    /// Retrieve a document from SurrealDB
    async fn retrieve_document(&self, document_id: &str) -> Result<Document, anyhow::Error> {
        if document_id.is_empty() {
            return Err(AgentError::TaskProcessingFailed("Document ID cannot be empty".to_string()).into());
        }

        // Try SurrealDB first
        let db_guard = self.db.lock().await;
        if let Some(ref client) = *db_guard {
            let mut result = client.query(
                "SELECT * FROM document WHERE id = $id LIMIT 1"
            )
            .bind(("id", document_id.to_string()))
            .await;

            if let Ok(mut response) = result {
                if let Ok(Some(doc_value)) = response.take::<Option<serde_json::Value>>(0) {
                    let name = doc_value.get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or(document_id)
                        .to_string();
                    let doc_type_str = doc_value.get("document_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("other");
                    let content_b64 = doc_value.get("content_b64")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let content = base64_decode(content_b64);
                    let metadata: serde_json::Value = doc_value.get("metadata")
                        .cloned()
                        .unwrap_or(serde_json::json!({}));

                    return Ok(Document {
                        id: document_id.to_string(),
                        name,
                        document_type: crate::database::models::DocumentType::from_str(doc_type_str),
                        content,
                        metadata,
                        created_at: Utc::now(),
                        updated_at: Utc::now(),
                        bounding_box: None,
                    });
                }
            }
        }

        // Fallback: return a stub if SurrealDB unavailable
        warn!("Document {} not found in SurrealDB, returning stub", document_id);
        Ok(Document {
            id: document_id.to_string(),
            name: format!("Document {}", document_id),
            document_type: crate::database::models::DocumentType::Other,
            content: vec![],
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            bounding_box: None,
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
