//! Database Document Module
//!
//! Handles document storage and retrieval in the database.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use crate::database::models::{Document, DocumentType};
use crate::database::error::{DatabaseError, DatabaseResult};

/// Document database operations
#[async_trait]
pub trait DocumentRepository: Send + Sync {
    /// Save a document
    async fn save(&self, document: &Document) -> DatabaseResult<Document>;
    
    /// Find a document by ID
    async fn find_by_id(&self, id: &str) -> DatabaseResult<Option<Document>>;
    
    /// Find documents by type
    async fn find_by_type(&self, doc_type: DocumentType) -> DatabaseResult<Vec<Document>>;
    
    /// Find documents by name
    async fn find_by_name(&self, name: &str) -> DatabaseResult<Vec<Document>>;
    
    /// Delete a document
    async fn delete(&self, id: &str) -> DatabaseResult<bool>;
    
    /// List all documents
    async fn list_all(&self) -> DatabaseResult<Vec<Document>>;
    
    /// Update a document
    async fn update(&self, id: &str, document: &Document) -> DatabaseResult<Document>;
}

/// SurrealDB implementation of DocumentRepository
#[derive(Debug, Clone)]
pub struct SurrealDocumentRepository {
    /// Database connection (cloned Surreal handle — no Mutex needed)
    pub db: Option<surrealdb::Surreal<surrealdb::engine::local::Db>>,
}

impl SurrealDocumentRepository {
    /// Create a new SurrealDB document repository
    pub fn new(db: Option<surrealdb::Surreal<surrealdb::engine::local::Db>>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl DocumentRepository for SurrealDocumentRepository {
    async fn save(&self, document: &Document) -> DatabaseResult<Document> {
        let client = self.db.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let content_b64 = base64::encode(&document.content);
        let mut response = client.query(
            "CREATE document SET \
             name = $name, \
             document_type = $doc_type, \
             content = $content, \
             metadata = $metadata, \
             created_at = $created_at, \
             updated_at = $updated_at"
        )
        .bind(("name", document.name.clone()))
        .bind(("doc_type", document.document_type.to_str().to_string()))
        .bind(("content", content_b64))
        .bind(("metadata", document.metadata.to_string()))
        .bind(("created_at", document.created_at.to_rfc3339()))
        .bind(("updated_at", document.updated_at.to_rfc3339()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let results: Vec<serde_json::Value> = response.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        if let Some(val) = results.into_iter().next() {
            self.parse_document(val)
        } else {
            Ok(document.clone())
        }
    }

    async fn find_by_id(&self, id: &str) -> DatabaseResult<Option<Document>> {
        let client = self.db.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let mut response = client.query(
            "SELECT * FROM document WHERE id = $id"
        )
        .bind(("id", id.to_string()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let results: Vec<serde_json::Value> = response.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        match results.into_iter().next() {
            Some(val) => Ok(Some(self.parse_document(val)?)),
            None => Ok(None),
        }
    }

    async fn find_by_type(&self, doc_type: DocumentType) -> DatabaseResult<Vec<Document>> {
        let client = self.db.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let mut response = client.query(
            "SELECT * FROM document WHERE document_type = $doc_type"
        )
        .bind(("doc_type", doc_type.to_str().to_string()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let results: Vec<serde_json::Value> = response.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        results.into_iter()
            .map(|val| self.parse_document(val))
            .collect()
    }

    async fn find_by_name(&self, name: &str) -> DatabaseResult<Vec<Document>> {
        let client = self.db.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let mut response = client.query(
            "SELECT * FROM document WHERE string::contains(name, $name)"
        )
        .bind(("name", name.to_string()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let results: Vec<serde_json::Value> = response.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        results.into_iter()
            .map(|val| self.parse_document(val))
            .collect()
    }

    async fn delete(&self, id: &str) -> DatabaseResult<bool> {
        let client = self.db.as_ref().ok_or(DatabaseError::NotInitialized)?;

        // First check if the document exists
        let mut check = client.query(
            "SELECT * FROM document WHERE id = $id"
        )
        .bind(("id", id.to_string()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let existing: Vec<serde_json::Value> = check.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        if existing.is_empty() {
            return Ok(false);
        }

        client.query(
            "DELETE FROM document WHERE id = $id"
        )
        .bind(("id", id.to_string()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        Ok(true)
    }

    async fn list_all(&self) -> DatabaseResult<Vec<Document>> {
        let client = self.db.as_ref().ok_or(DatabaseError::NotInitialized)?;

        let mut response = client.query(
            "SELECT * FROM document"
        )
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let results: Vec<serde_json::Value> = response.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        results.into_iter()
            .map(|val| self.parse_document(val))
            .collect()
    }

    async fn update(&self, id: &str, document: &Document) -> DatabaseResult<Document> {
        let client = self.db.as_ref().ok_or(DatabaseError::NotInitialized)?;

        // Verify the document exists first
        let mut check = client.query(
            "SELECT * FROM document WHERE id = $id"
        )
        .bind(("id", id.to_string()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let existing: Vec<serde_json::Value> = check.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        if existing.is_empty() {
            return Err(DatabaseError::NotFound(format!("Document with id {} not found", id)));
        }

        let content_b64 = base64::encode(&document.content);
        let mut response = client.query(
            "UPDATE document SET \
             name = $name, \
             document_type = $doc_type, \
             content = $content, \
             metadata = $metadata, \
             updated_at = $updated_at \
             WHERE id = $id"
        )
        .bind(("name", document.name.clone()))
        .bind(("doc_type", document.document_type.to_str().to_string()))
        .bind(("content", content_b64))
        .bind(("metadata", document.metadata.to_string()))
        .bind(("updated_at", document.updated_at.to_rfc3339()))
        .bind(("id", id.to_string()))
        .await
        .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        let results: Vec<serde_json::Value> = response.take(0)
            .map_err(|e| DatabaseError::QueryError(e.to_string()))?;

        if let Some(val) = results.into_iter().next() {
            self.parse_document(val)
        } else {
            Ok(document.clone())
        }
    }
}

impl SurrealDocumentRepository {
    /// Parse a SurrealDB result into a Document
    fn parse_document(&self, value: serde_json::Value) -> DatabaseResult<Document> {
        let obj = value.as_object().ok_or(DatabaseError::DeserializationError("Expected object".to_string()))?;

        let id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let doc_type = obj.get("document_type").and_then(|v| v.as_str()).map(|s| DocumentType::from_str(s)).unwrap_or(DocumentType::Other);
        let content = obj.get("content").and_then(|v| v.as_str()).map(|s| base64::decode(s).unwrap_or_default()).unwrap_or_default();
        let metadata = obj.get("metadata").cloned().unwrap_or(serde_json::json!({}));
        let created_at = obj.get("created_at").and_then(|v| v.as_str()).and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|dt| dt.with_timezone(&Utc)).unwrap_or_else(|| Utc::now());
        let updated_at = obj.get("updated_at").and_then(|v| v.as_str()).and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|dt| dt.with_timezone(&Utc)).unwrap_or_else(|| Utc::now());

        Ok(Document {
            id,
            name,
            document_type: doc_type,
            content,
            metadata,
            created_at,
            updated_at,
            bounding_box: None,
        })
    }
}

/// In-memory implementation of DocumentRepository for testing
#[derive(Debug, Clone, Default)]
pub struct MemoryDocumentRepository {
    /// In-memory storage
    pub documents: Arc<Mutex<Vec<Document>>>,
}

impl MemoryDocumentRepository {
    /// Create a new in-memory document repository
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl DocumentRepository for MemoryDocumentRepository {
    async fn save(&self, document: &Document) -> DatabaseResult<Document> {
        let mut docs = self.documents.lock().await;
        
        // Remove existing document with same ID if it exists
        docs.retain(|d| d.id != document.id);
        
        docs.push(document.clone());
        
        Ok(document.clone())
    }

    async fn find_by_id(&self, id: &str) -> DatabaseResult<Option<Document>> {
        let docs = self.documents.lock().await;
        Ok(docs.iter().find(|d| d.id == id).cloned())
    }

    async fn find_by_type(&self, doc_type: DocumentType) -> DatabaseResult<Vec<Document>> {
        let docs = self.documents.lock().await;
        Ok(docs.iter().filter(|d| d.document_type == doc_type).cloned().collect())
    }

    async fn find_by_name(&self, name: &str) -> DatabaseResult<Vec<Document>> {
        let docs = self.documents.lock().await;
        Ok(docs.iter().filter(|d| d.name.contains(name)).cloned().collect())
    }

    async fn delete(&self, id: &str) -> DatabaseResult<bool> {
        let mut docs = self.documents.lock().await;
        let len_before = docs.len();
        docs.retain(|d| d.id != id);
        Ok(len_before != docs.len())
    }

    async fn list_all(&self) -> DatabaseResult<Vec<Document>> {
        let docs = self.documents.lock().await;
        Ok(docs.clone())
    }

    async fn update(&self, id: &str, document: &Document) -> DatabaseResult<Document> {
        let mut docs = self.documents.lock().await;
        
        if let Some(index) = docs.iter().position(|d| d.id == id) {
            docs[index] = document.clone();
            Ok(document.clone())
        } else {
            Err(DatabaseError::NotFound(format!("Document with id {} not found", id)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_memory_document_repository() {
        let repo = MemoryDocumentRepository::new();
        
        let doc = Document::new("Test Doc", DocumentType::Invoice, vec![1, 2, 3]);
        
        // Save document
        let saved = repo.save(&doc).await.unwrap();
        assert_eq!(saved.name, "Test Doc");
        
        // Find by ID
        let found = repo.find_by_id(&saved.id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Test Doc");
        
        // Find by type
        let invoices = repo.find_by_type(DocumentType::Invoice).await.unwrap();
        assert_eq!(invoices.len(), 1);
        
        // List all
        let all = repo.list_all().await.unwrap();
        assert_eq!(all.len(), 1);
        
        // Delete
        let deleted = repo.delete(&saved.id).await.unwrap();
        assert!(deleted);
        
        let all = repo.list_all().await.unwrap();
        assert_eq!(all.len(), 0);
    }

    #[tokio::test]
    async fn test_document_operations() {
        let repo = MemoryDocumentRepository::new();
        
        // Create and save a document
        let mut doc = Document::new("Invoice #1", DocumentType::Invoice, vec![1, 2, 3]);
        doc = repo.save(&doc).await.unwrap();
        
        // Update the document
        let mut updated_doc = doc.clone();
        updated_doc.name = "Invoice #1 Updated".to_string();
        let updated = repo.update(&doc.id, &updated_doc).await.unwrap();
        assert_eq!(updated.name, "Invoice #1 Updated");
        
        // Verify update
        let found = repo.find_by_id(&doc.id).await.unwrap();
        assert_eq!(found.unwrap().name, "Invoice #1 Updated");
    }
}
