//! Receipt Module
//!
//! Handles receipt processing — raw document → categorized expense → transaction.

use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;
use chrono::{DateTime, Utc, NaiveDate};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tracing::{info, debug, warn};
use crate::database::financial::{
    Account, AccountType, EntryType, Transaction, TransactionEntry,
    TransactionType, TransactionStatus,
};
use crate::agents::agent_types::{Agent, AgentConfig, AgentType, AgentStatus};
use crate::agents::task::{Task, TaskResult, TaskPayload, TaskType};
use crate::agents::error::AgentError;

/// Receipt error types
#[derive(Debug, thiserror::Error)]
pub enum ReceiptError {
    #[error("Receipt not found: {0}")]
    ReceiptNotFound(String),

    #[error("Invalid receipt data: {0}")]
    InvalidData(String),

    #[error("Ledger error: {0}")]
    LedgerError(String),

    #[error("Receipt error: {0}")]
    Other(String),
}

impl ReceiptError {
    pub fn other(msg: &str) -> Self {
        Self::Other(msg.to_string())
    }
}

pub type ReceiptResult<T> = Result<T, ReceiptError>;

/// Keyword → account number mapping for expense categorization.
/// Uses substring matching — "office depot supplies" matches "supply" → 5040.
/// Order matters: more specific keywords first to prevent false matches.
const CATEGORICAL_ACCOUNT_MAP: &[(&str, &str)] = &[
    // COGS (5000)
    ("cost of goods", "5000"), ("cogs", "5000"), ("inventory purchase", "5000"),
    ("raw material", "5000"), ("wholesale", "5000"),
    // Salaries (5010)
    ("salary", "5010"), ("salaries", "5010"), ("wage", "5010"), ("payroll", "5010"),
    ("contractor fee", "5010"), ("staffing", "5010"),
    // Rent (5020)
    ("rent", "5020"), ("lease", "5020"), ("property rental", "5020"),
    ("office space", "5020"), ("landlord", "5020"),
    // Utilities (5030)
    ("electric", "5030"), ("electricity", "5030"), ("power bill", "5030"),
    ("water", "5030"), ("gas bill", "5030"), ("utility", "5030"), ("utilities", "5030"),
    ("internet", "5030"), ("phone bill", "5030"), ("telecom", "5030"),
    // Office Supplies (5040)
    ("office supply", "5040"), ("office supplies", "5040"), ("supplies", "5040"),
    ("stationery", "5040"), ("printer", "5040"), ("toner", "5040"),
    ("paper", "5040"), ("staples", "5040"), ("office depot", "5040"),
    ("office max", "5040"),
];

/// Receipt status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ReceiptStatus {
    Pending,
    Processed,
    Categorized,
    Approved,
    Rejected,
}

impl Default for ReceiptStatus {
    fn default() -> Self {
        Self::Pending
    }
}

/// Receipt model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub id: Uuid,
    pub vendor_name: String,
    pub receipt_date: NaiveDate,
    pub amount: Decimal,
    pub currency: String,
    pub expense_category: String,
    pub description: String,
    pub status: ReceiptStatus,
    pub document_id: Option<String>,
    pub transaction_id: Option<Uuid>,
    pub account_id: Option<Uuid>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Default for Receipt {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            vendor_name: String::new(),
            receipt_date: now.date_naive(),
            amount: dec!(0),
            currency: "USD".to_string(),
            expense_category: String::new(),
            description: String::new(),
            status: ReceiptStatus::Pending,
            document_id: None,
            transaction_id: None,
            account_id: None,
            metadata: serde_json::json!({}),
            created_at: now,
            updated_at: now,
        }
    }
}

/// Receipt processor
#[derive(Debug, Clone)]
pub struct ReceiptProcessor {
    pub receipts: Arc<RwLock<BTreeMap<Uuid, Receipt>>>,
    pub db: Option<Arc<crate::database::Database>>,
    pub ledger: Option<Arc<crate::accounting::ledger::Ledger>>,
}

impl Default for ReceiptProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl ReceiptProcessor {
    pub fn new() -> Self {
        Self {
            receipts: Arc::new(RwLock::new(BTreeMap::new())),
            db: None,
            ledger: None,
        }
    }

    pub async fn initialize(&mut self) -> ReceiptResult<()> {
        info!("Initializing Receipt Processor...");
        Ok(())
    }

    /// Process a receipt: categorize expense and create a ledger transaction
    pub async fn process_receipt(
        &mut self,
        vendor_name: &str,
        receipt_date: NaiveDate,
        amount: Decimal,
        expense_category: &str,
        description: &str,
        document_id: Option<String>,
    ) -> ReceiptResult<Receipt> {
        debug!("Processing receipt from {} for {}", vendor_name, amount);

        if amount <= dec!(0) {
            return Err(ReceiptError::InvalidData(
                "Receipt amount must be positive".to_string(),
            ));
        }

        // Map expense category to account number using fuzzy keyword matching.
        // Each keyword maps to an account number; first match wins.
        let category_lower = expense_category.to_lowercase();
        let expense_account_number = CATEGORICAL_ACCOUNT_MAP
            .iter()
            .find(|(keyword, _)| category_lower.contains(keyword))
            .map(|(_, account)| *account)
            .unwrap_or("5040"); // default: Office Supplies

        let mut receipt = Receipt {
            vendor_name: vendor_name.to_string(),
            receipt_date,
            amount,
            expense_category: expense_category.to_string(),
            description: description.to_string(),
            status: ReceiptStatus::Categorized,
            document_id,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            ..Default::default()
        };

        // Create expense transaction in ledger
        if let Some(ref ledger) = self.ledger {
            let expense_account = ledger
                .get_account_by_number(expense_account_number)
                .await
                .map_err(|e| ReceiptError::LedgerError(e.to_string()))?;
            let cash_account = ledger
                .get_account_by_number("1000")
                .await
                .map_err(|e| ReceiptError::LedgerError(e.to_string()))?;

            if let (Some(expense), Some(cash)) = (expense_account, cash_account) {
                let entries = vec![
                    TransactionEntry::new(
                        expense.id,
                        EntryType::Debit,
                        amount,
                        &format!("Expense: {} - {}", vendor_name, description),
                    ),
                    TransactionEntry::new(
                        cash.id,
                        EntryType::Credit,
                        amount,
                        &format!("Payment to {}", vendor_name),
                    ),
                ];

                let transaction = Transaction {
                    transaction_type: TransactionType::Expense,
                    ..Transaction::new(
                        format!("Receipt: {} - {}", vendor_name, description),
                        receipt_date.and_hms_opt(0, 0, 0).unwrap().and_utc(),
                        entries,
                    )
                };

                match ledger.record_transaction(transaction).await {
                    Ok(recorded) => {
                        receipt.transaction_id = Some(recorded.id);
                        receipt.account_id = Some(expense.id);
                        receipt.status = ReceiptStatus::Processed;
                    }
                    Err(e) => {
                        warn!("Failed to record receipt transaction: {}", e);
                    }
                }
            }
        }

        self.receipts.write().await.insert(receipt.id, receipt.clone());

        // Persist to SurrealDB
        if let Some(ref db) = self.db {
            if let Ok(client) = db.db().await {
                let value = serde_json::to_value(&receipt).unwrap_or_default();
                if let Err(e) = client
                    .create::<Vec<serde_json::Value>>("receipt")
                    .content(value)
                    .await
                {
                    warn!("Failed to persist receipt to SurrealDB: {}", e);
                }
            }
        }

        Ok(receipt)
    }

    /// Get a receipt by ID
    pub async fn get_receipt(&self, id: Uuid) -> ReceiptResult<Option<Receipt>> {
        Ok(self.receipts.read().await.get(&id).cloned())
    }

    /// List all receipts
    pub async fn list_receipts(&self) -> ReceiptResult<Vec<Receipt>> {
        Ok(self.receipts.read().await.values().cloned().collect())
    }
}

/// Receipt Agent for handling receipt-related tasks
#[derive(Debug, Clone)]
pub struct ReceiptAgent {
    pub config: AgentConfig,
    pub status: AgentStatus,
    pub processor: Arc<Mutex<ReceiptProcessor>>,
}

impl ReceiptAgent {
    pub fn new(config: AgentConfig, processor: ReceiptProcessor) -> Self {
        Self {
            config,
            status: AgentStatus::Idle,
            processor: Arc::new(Mutex::new(processor)),
        }
    }

    pub fn with_defaults() -> Self {
        let config = AgentConfig::receipt_agent();
        Self::new(config, ReceiptProcessor::new())
    }
}

#[async_trait]
impl Agent for ReceiptAgent {
    fn config(&self) -> &AgentConfig {
        &self.config
    }

    fn status(&self) -> AgentStatus {
        self.status.clone()
    }

    async fn initialize(&mut self) -> Result<(), anyhow::Error> {
        self.status = AgentStatus::Initializing;
        self.processor.lock().await.initialize().await?;
        self.status = AgentStatus::Idle;
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), anyhow::Error> {
        self.status = AgentStatus::ShuttingDown;
        self.status = AgentStatus::Idle;
        Ok(())
    }

    async fn process_task(&self, task: Task) -> Result<Task, anyhow::Error> {
        if !self.config.enabled {
            return Err(AgentError::AgentDisabled(self.config.name.clone()).into());
        }

        match task.task_type {
            TaskType::ProcessReceipt => self.process_receipt_task(task).await,
            _ => Err(AgentError::TaskProcessingFailed(format!(
                "ReceiptAgent cannot handle task type: {:?}",
                task.task_type
            ))
            .into()),
        }
    }

    fn agent_type(&self) -> AgentType {
        self.config.agent_type.clone()
    }
}

impl ReceiptAgent {
    async fn process_receipt_task(&self, task: Task) -> Result<Task, anyhow::Error> {
        let start_time = std::time::Instant::now();

        let json = match &task.payload {
            TaskPayload::Json(v) => v.clone(),
            _ => {
                return Err(AgentError::TaskProcessingFailed(
                    "Expected Json payload for ProcessReceipt task".to_string(),
                )
                .into())
            }
        };

        let vendor_name = json
            .get("vendor_name")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown");
        let date_str = json
            .get("receipt_date")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let receipt_date = if !date_str.is_empty() {
            NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                .unwrap_or_else(|_| Utc::now().date_naive())
        } else {
            Utc::now().date_naive()
        };
        let amount = json
            .get("amount")
            .and_then(|v| v.as_f64())
            .map(Decimal::from_f64_retain)
            .flatten()
            .ok_or_else(|| {
                AgentError::TaskProcessingFailed(
                    "Missing or invalid amount in payload".to_string(),
                )
            })?;
        let expense_category = json
            .get("expense_category")
            .and_then(|v| v.as_str())
            .unwrap_or("other");
        let description = json
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let document_id = json
            .get("document_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut processor = self.processor.lock().await;
        let receipt = processor
            .process_receipt(
                vendor_name,
                receipt_date,
                amount,
                expense_category,
                description,
                document_id,
            )
            .await
            .map_err(|e| AgentError::TaskProcessingFailed(e.to_string()))?;

        let result = TaskResult::success_with_data(
            &format!(
                "Receipt from {} for {} processed as {:?}",
                vendor_name, amount, receipt.status
            ),
            TaskPayload::Json(serde_json::to_value(&receipt).unwrap_or_default()),
        );

        let _processing_time = start_time.elapsed().as_millis() as f64;

        Ok(task.complete(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_receipt_processor_creation() {
        let processor = ReceiptProcessor::new();
        assert!(processor.receipts.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_process_receipt_without_ledger() {
        let mut processor = ReceiptProcessor::new();

        let receipt = processor
            .process_receipt(
                "Office Depot",
                Utc::now().date_naive(),
                dec!(45.99),
                "office supplies",
                "Printer paper and toner",
                None,
            )
            .await
            .unwrap();

        assert_eq!(receipt.vendor_name, "Office Depot");
        assert_eq!(receipt.amount, dec!(45.99));
        assert_eq!(receipt.status, ReceiptStatus::Categorized);
    }

    #[tokio::test]
    async fn test_receipt_agent_process() {
        let agent = ReceiptAgent::with_defaults();

        let payload = serde_json::json!({
            "vendor_name": "Staples",
            "receipt_date": "2026-06-15",
            "amount": 75.50,
            "expense_category": "office supplies",
            "description": "Printer cartridges"
        });

        let task = Task {
            task_type: TaskType::ProcessReceipt,
            payload: TaskPayload::Json(payload),
            ..Default::default()
        };

        let result = agent.process_task(task).await;
        assert!(result.is_ok());
        let completed = result.unwrap();
        assert_eq!(completed.status, crate::agents::task::TaskStatus::Completed);
    }

    #[tokio::test]
    async fn test_receipt_invalid_amount() {
        let agent = ReceiptAgent::with_defaults();

        let payload = serde_json::json!({
            "vendor_name": "Test",
            "amount": -10.0,
            "expense_category": "other"
        });

        let task = Task {
            task_type: TaskType::ProcessReceipt,
            payload: TaskPayload::Json(payload),
            ..Default::default()
        };

        let result = agent.process_task(task).await;
        assert!(result.is_err());
    }
}
