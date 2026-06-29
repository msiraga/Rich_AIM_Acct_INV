//! AI Module
//!
//! This module contains AI-related functionality for the NexusLedger system.
//! 
//! # Submodules
//! - `ollama`: Ollama integration for local LLM inference
//! - `embeddings`: Text embedding functionality
//! - `classification`: Document and transaction classification
//! - `extraction`: Data extraction from documents

use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, error, debug, warn};
use crate::database::models::Document;
use crate::agents::orchestrator::AgentOrchestrator;

/// AI configuration
#[derive(Debug, Clone)]
pub struct AIConfig {
    /// Whether AI is enabled
    pub enabled: bool,
    /// Ollama base URL
    pub ollama_url: String,
    /// Default model for text generation
    pub default_model: String,
    /// Default model for embeddings
    pub embedding_model: String,
    /// Default model for classification
    pub classification_model: String,
    /// Default model for extraction
    pub extraction_model: String,
    /// API timeout in seconds
    pub timeout: u64,
    /// Maximum tokens for generation
    pub max_tokens: u32,
    /// Temperature for generation
    pub temperature: f32,
}

impl Default for AIConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ollama_url: "http://localhost:11434".to_string(),
            default_model: "llama3".to_string(),
            embedding_model: "llama3".to_string(),
            classification_model: "llama3".to_string(),
            extraction_model: "llama3".to_string(),
            timeout: 30,
            max_tokens: 2048,
            temperature: 0.7,
        }
    }
}

impl AIConfig {
    /// Create a new AI configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        Self {
            enabled: std::env::var("AI_ENABLED")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(true),
            ollama_url: std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".to_string()),
            default_model: std::env::var("AI_DEFAULT_MODEL").unwrap_or_else(|_| "llama3".to_string()),
            embedding_model: std::env::var("AI_EMBEDDING_MODEL").unwrap_or_else(|_| "llama3".to_string()),
            classification_model: std::env::var("AI_CLASSIFICATION_MODEL").unwrap_or_else(|_| "llama3".to_string()),
            extraction_model: std::env::var("AI_EXTRACTION_MODEL").unwrap_or_else(|_| "llama3".to_string()),
            timeout: std::env::var("AI_TIMEOUT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(30),
            max_tokens: std::env::var("AI_MAX_TOKENS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2048),
            temperature: std::env::var("AI_TEMPERATURE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.7),
        }
    }
}

/// AI service
#[derive(Debug, Clone)]
pub struct AIService {
    /// AI configuration
    pub config: AIConfig,
    /// Ollama client
    pub ollama: Option<ollama_rs::Ollama>,
    /// Agent orchestrator
    pub orchestrator: Arc<Mutex<AgentOrchestrator>>,
}

impl AIService {
    /// Create a new AI service
    pub fn new(config: AIConfig, orchestrator: Arc<Mutex<AgentOrchestrator>>) -> Self {
        Self {
            config,
            ollama: None,
            orchestrator,
        }
    }

    /// Initialize the AI service
    pub async fn initialize(&mut self) -> Result<(), anyhow::Error> {
        info!("Initializing AI Service...");
        
        if self.config.enabled {
            info!("AI is enabled");
            info!("Ollama URL: {}", self.config.ollama_url);
            
            // Initialize Ollama client
            self.initialize_ollama().await?;
        } else {
            info!("AI is disabled");
        }
        
        Ok(())
    }

    /// Initialize Ollama client
    async fn initialize_ollama(&mut self) -> Result<(), anyhow::Error> {
        info!("Initializing Ollama client...");

        // AI integration deferred to Phase 5
        warn!("Ollama integration not yet implemented");
        self.ollama = None;

        Ok(())
    }

    /// Check if AI is available
    pub fn is_available(&self) -> bool {
        self.config.enabled && self.ollama.is_some()
    }

    /// Generate text using the default model
    pub async fn generate_text(&self, prompt: &str) -> Result<String, anyhow::Error> {
        if !self.is_available() {
            return Err(anyhow::anyhow!("AI service is not available"));
        }

        // AI integration deferred to Phase 5
        Err(anyhow::anyhow!("AI not initialized"))
    }

    /// Generate embeddings for text
    pub async fn generate_embeddings(&self, text: &str) -> Result<Vec<f32>, anyhow::Error> {
        if !self.is_available() {
            return Err(anyhow::anyhow!("AI service is not available"));
        }

        // AI integration deferred to Phase 5
        Err(anyhow::anyhow!("AI not initialized"))
    }

    /// Classify a document
    pub async fn classify_document(&self, document: &Document) -> Result<String, anyhow::Error> {
        if !self.is_available() {
            return Err(anyhow::anyhow!("AI service is not available"));
        }
        
        let prompt = format!(
            "Classify the following document into one of these categories: 
             Invoice, Receipt, BankStatement, Check, PurchaseOrder, TaxForm, Contract, Other.
             
             Document name: {}
             Document type: {:?}
             
             Return only the category name, nothing else.",
             document.name,
             document.document_type
        );
        
        let classification = self.generate_text(&prompt).await?;
        
        // Clean up the response
        let classification = classification.trim().to_string();
        
        Ok(classification)
    }

    /// Extract data from a document
    pub async fn extract_data(&self, document: &Document, extraction_type: &str) -> Result<serde_json::Value, anyhow::Error> {
        if !self.is_available() {
            return Err(anyhow::anyhow!("AI service is not available"));
        }
        
        let prompt = match extraction_type {
            "invoice" => self.get_invoice_extraction_prompt(document),
            "receipt" => self.get_receipt_extraction_prompt(document),
            "bank_statement" => self.get_bank_statement_extraction_prompt(document),
            _ => self.get_generic_extraction_prompt(document),
        };
        
        let extraction = self.generate_text(&prompt).await?;
        
        // Try to parse as JSON
        if let Ok(json) = serde_json::from_str(&extraction) {
            Ok(json)
        } else {
            // Return as a string value
            Ok(serde_json::json!(extraction))
        }
    }

    /// Get invoice extraction prompt
    fn get_invoice_extraction_prompt(&self, document: &Document) -> String {
        format!(
            "Extract the following information from this invoice document as JSON:
            {{
                \"vendor\": \"vendor name\",
                \"invoice_number\": \"invoice number\",
                \"date\": \"invoice date (YYYY-MM-DD)\",
                \"due_date\": \"due date (YYYY-MM-DD)\",
                \"total_amount\": 0.00,
                \"tax_amount\": 0.00,
                \"items\": [
                    {{
                        \"description\": \"item description\",
                        \"quantity\": 1,
                        \"unit_price\": 0.00,
                        \"amount\": 0.00
                    }}
                ]
            }}
            
            Document name: {}
            Document content: (binary data, {} bytes)
            
            Return only valid JSON, nothing else.",
            document.name,
            document.content.len()
        )
    }

    /// Get receipt extraction prompt
    fn get_receipt_extraction_prompt(&self, document: &Document) -> String {
        format!(
            "Extract the following information from this receipt document as JSON:
            {{
                \"merchant\": \"merchant name\",
                \"transaction_date\": \"transaction date (YYYY-MM-DD)\",
                \"total_amount\": 0.00,
                \"tax_amount\": 0.00,
                \"payment_method\": \"payment method\",
                \"items\": [
                    {{
                        \"description\": \"item description\",
                        \"quantity\": 1,
                        \"unit_price\": 0.00,
                        \"amount\": 0.00
                    }}
                ]
            }}
            
            Document name: {}
            Document content: (binary data, {} bytes)
            
            Return only valid JSON, nothing else.",
            document.name,
            document.content.len()
        )
    }

    /// Get bank statement extraction prompt
    fn get_bank_statement_extraction_prompt(&self, document: &Document) -> String {
        format!(
            "Extract the following information from this bank statement document as JSON:
            {{
                \"bank_name\": \"bank name\",
                \"account_number\": \"account number\",
                \"statement_date\": \"statement date (YYYY-MM-DD)\",
                \"start_date\": \"start date (YYYY-MM-DD)\",
                \"end_date\": \"end date (YYYY-MM-DD)\",
                \"starting_balance\": 0.00,
                \"ending_balance\": 0.00,
                \"transactions\": [
                    {{
                        \"date\": \"transaction date (YYYY-MM-DD)\",
                        \"description\": \"transaction description\",
                        \"amount\": 0.00,
                        \"balance\": 0.00
                    }}
                ]
            }}
            
            Document name: {}
            Document content: (binary data, {} bytes)
            
            Return only valid JSON, nothing else.",
            document.name,
            document.content.len()
        )
    }

    /// Get generic extraction prompt
    fn get_generic_extraction_prompt(&self, document: &Document) -> String {
        format!(
            "Extract structured data from this document as JSON.
            Be as comprehensive as possible.
            
            Document name: {}
            Document type: {:?}
            Document content: (binary data, {} bytes)
            
            Return only valid JSON, nothing else.",
            document.name,
            document.document_type,
            document.content.len()
        )
    }

    /// Analyze a transaction for anomalies
    pub async fn analyze_transaction(&self, transaction_description: &str) -> Result<serde_json::Value, anyhow::Error> {
        if !self.is_available() {
            return Err(anyhow::anyhow!("AI service is not available"));
        }
        
        let prompt = format!(
            "Analyze the following transaction description for potential issues or anomalies:
            
            Transaction description: {}
            
            Return analysis as JSON with the following structure:
            {{
                \"is_suspicious\": true/false,
                \"confidence\": 0.0-1.0,
                \"issues\": [\"list of potential issues\"]
                \"recommendations\": [\"list of recommendations\"]
            }}
            
            Return only valid JSON, nothing else.",
            transaction_description
        );
        
        let analysis = self.generate_text(&prompt).await?;
        
        // Try to parse as JSON
        if let Ok(json) = serde_json::from_str(&analysis) {
            Ok(json)
        } else {
            // Return as a string value
            Ok(serde_json::json!(analysis))
        }
    }

    /// Suggest account categorization
    pub async fn suggest_account_category(&self, account_name: &str, account_description: Option<&str>) -> Result<String, anyhow::Error> {
        if !self.is_available() {
            return Err(anyhow::anyhow!("AI service is not available"));
        }
        
        let description = account_description.unwrap_or("");
        
        let prompt = format!(
            "Suggest the most appropriate account category for the following account:
            
            Account name: {}
            Account description: {}
            
            Choose from: Asset, Liability, Equity, Revenue, Expense
            
            Return only the category name, nothing else.",
            account_name,
            description
        );
        
        let category = self.generate_text(&prompt).await?;
        
        Ok(category.trim().to_string())
    }
}

/// Text classifier for document and transaction classification
#[derive(Debug, Clone)]
pub struct TextClassifier {
    /// AI service
    pub ai_service: Arc<Mutex<AIService>>,
}

impl TextClassifier {
    /// Create a new text classifier
    pub fn new(ai_service: Arc<Mutex<AIService>>) -> Self {
        Self { ai_service }
    }

    /// Classify text into categories
    pub async fn classify(&self, text: &str, categories: &[&str]) -> Result<String, anyhow::Error> {
        let ai_service = self.ai_service.lock().await;
        
        if !ai_service.is_available() {
            return Err(anyhow::anyhow!("AI service is not available"));
        }
        
        let categories_str = categories.join(", ");
        let prompt = format!(
            "Classify the following text into one of these categories: {}
            
            Text: {}
            
            Return only the category name, nothing else.",
            categories_str,
            text
        );
        
        let classification = ai_service.generate_text(&prompt).await?;
        
        Ok(classification.trim().to_string())
    }
}

/// Text extractor for extracting structured data from text
#[derive(Debug, Clone)]
pub struct TextExtractor {
    /// AI service
    pub ai_service: Arc<Mutex<AIService>>,
}

impl TextExtractor {
    /// Create a new text extractor
    pub fn new(ai_service: Arc<Mutex<AIService>>) -> Self {
        Self { ai_service }
    }

    /// Extract structured data from text
    pub async fn extract(&self, text: &str, schema: &str) -> Result<serde_json::Value, anyhow::Error> {
        let ai_service = self.ai_service.lock().await;
        
        if !ai_service.is_available() {
            return Err(anyhow::anyhow!("AI service is not available"));
        }
        
        let prompt = format!(
            "Extract structured data from the following text according to this schema:
            
            Schema: {}
            
            Text: {}
            
            Return only valid JSON matching the schema, nothing else.",
            schema,
            text
        );
        
        let extraction = ai_service.generate_text(&prompt).await?;
        
        // Try to parse as JSON
        if let Ok(json) = serde_json::from_str(&extraction) {
            Ok(json)
        } else {
            // Return as a string value
            Ok(serde_json::json!(extraction))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[test]
    fn test_ai_config_default() {
        let config = AIConfig::default();
        assert!(config.enabled);
        assert_eq!(config.ollama_url, "http://localhost:11434");
        assert_eq!(config.default_model, "llama3");
    }

    #[test]
    fn test_ai_config_creation() {
        let config = AIConfig::new();
        assert!(config.enabled);
    }

    #[tokio::test]
    async fn test_ai_service_creation() {
        let config = AIConfig::default();
        let orchestrator = Arc::new(Mutex::new(AgentOrchestrator::new()));
        
        let service = AIService::new(config, orchestrator);
        assert!(service.config.enabled);
    }

    #[tokio::test]
    async fn test_text_classifier() {
        let config = AIConfig::default();
        let orchestrator = Arc::new(Mutex::new(AgentOrchestrator::new()));
        let ai_service = Arc::new(Mutex::new(AIService::new(config, orchestrator)));

        let classifier = TextClassifier::new(ai_service);
        assert!(classifier.ai_service.lock().await.config.enabled);
    }

    #[tokio::test]
    async fn test_text_extractor() {
        let config = AIConfig::default();
        let orchestrator = Arc::new(Mutex::new(AgentOrchestrator::new()));
        let ai_service = Arc::new(Mutex::new(AIService::new(config, orchestrator)));

        let extractor = TextExtractor::new(ai_service);
        assert!(extractor.ai_service.lock().await.config.enabled);
    }
}
