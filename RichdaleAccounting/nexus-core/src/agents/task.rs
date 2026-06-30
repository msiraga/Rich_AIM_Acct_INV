//! Agent Task Module
//!
//! Defines task types and task management for the agent system.

use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use crate::agents::agent_types::AgentType;
use crate::database::financial::{Transaction, Account};
use crate::database::models::Document;

/// Enum representing different types of tasks that agents can process
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskType {
    /// Record a financial transaction
    RecordTransaction,
    /// Reconcile bank statements
    ReconcileAccount,
    /// Generate an invoice
    GenerateInvoice,
    /// Process a payment
    ProcessPayment,
    /// Calculate payroll
    CalculatePayroll,
    /// Calculate taxes
    CalculateTaxes,
    /// Process a receipt
    ProcessReceipt,
    /// Store a document
    StoreDocument,
    /// Retrieve a document
    RetrieveDocument,
    /// Perform audit check
    AuditCheck,
    /// Generate financial report
    GenerateReport,
    /// Validate data
    ValidateData,
    /// Export data
    ExportData,
    /// Import data
    ImportData,
}

impl Default for TaskType {
    fn default() -> Self {
        TaskType::RecordTransaction
    }
}

/// Priority levels for tasks
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskPriority {
    /// Low priority - can wait
    Low = 1,
    /// Normal priority - default
    Normal = 5,
    /// High priority - should be processed soon
    High = 8,
    /// Critical priority - must be processed immediately
    Critical = 10,
}

impl Default for TaskPriority {
    fn default() -> Self {
        TaskPriority::Normal
    }
}

/// Status of a task
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskStatus {
    /// Task is waiting to be processed
    Pending,
    /// Task is currently being processed
    Processing,
    /// Task completed successfully
    Completed,
    /// Task failed with an error
    Failed(String),
    /// Task was cancelled
    Cancelled,
    /// Task is retrying
    Retrying(usize),
}

impl Default for TaskStatus {
    fn default() -> Self {
        TaskStatus::Pending
    }
}

/// A task that can be assigned to an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique identifier for the task
    pub id: Uuid,
    /// Type of task
    pub task_type: TaskType,
    /// Priority of the task
    pub priority: TaskPriority,
    /// Status of the task
    pub status: TaskStatus,
    /// Timestamp when the task was created
    pub created_at: DateTime<Utc>,
    /// Timestamp when the task was last updated
    pub updated_at: DateTime<Utc>,
    /// The agent type that should handle this task
    pub assigned_agent_type: Option<AgentType>,
    /// The specific agent ID that is handling this task
    pub assigned_agent_id: Option<Uuid>,
    /// Data payload for the task
    pub payload: TaskPayload,
    /// Results from task execution
    pub result: Option<TaskResult>,
    /// Number of retry attempts
    pub retry_count: usize,
    /// Maximum number of retries allowed
    pub max_retries: usize,
    /// Timeout in milliseconds
    pub timeout_ms: u64,
}

impl Default for Task {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            task_type: TaskType::default(),
            priority: TaskPriority::default(),
            status: TaskStatus::default(),
            created_at: now,
            updated_at: now,
            assigned_agent_type: None,
            assigned_agent_id: None,
            payload: TaskPayload::default(),
            result: None,
            retry_count: 0,
            max_retries: 3,
            timeout_ms: 30000, // 30 seconds
        }
    }
}

impl Task {
    /// Create a new task
    pub fn new(task_type: TaskType) -> Self {
        Self {
            task_type,
            ..Default::default()
        }
    }

    /// Create a transaction recording task
    pub fn record_transaction(transaction: Transaction) -> Self {
        Self {
            task_type: TaskType::RecordTransaction,
            payload: TaskPayload::Transaction(transaction),
            assigned_agent_type: Some(AgentType::LedgerAgent),
            ..Default::default()
        }
    }

    /// Create a reconciliation task
    pub fn reconcile_account(account: Account) -> Self {
        Self {
            task_type: TaskType::ReconcileAccount,
            payload: TaskPayload::Account(account),
            assigned_agent_type: Some(AgentType::ReconciliationAgent),
            ..Default::default()
        }
    }

    /// Create a document storage task
    pub fn store_document(document: Document) -> Self {
        Self {
            task_type: TaskType::StoreDocument,
            payload: TaskPayload::Document(document),
            assigned_agent_type: Some(AgentType::DocumentAgent),
            ..Default::default()
        }
    }

    /// Create an invoice generation task
    pub fn generate_invoice(invoice_data: serde_json::Value) -> Self {
        Self {
            task_type: TaskType::GenerateInvoice,
            payload: TaskPayload::Json(invoice_data),
            assigned_agent_type: Some(AgentType::InvoiceAgent),
            ..Default::default()
        }
    }

    /// Create a payment processing task
    pub fn process_payment(payment_data: serde_json::Value) -> Self {
        Self {
            task_type: TaskType::ProcessPayment,
            payload: TaskPayload::Json(payment_data),
            assigned_agent_type: Some(AgentType::InvoiceAgent),
            ..Default::default()
        }
    }

    /// Create a receipt processing task
    pub fn process_receipt(receipt_data: serde_json::Value) -> Self {
        Self {
            task_type: TaskType::ProcessReceipt,
            payload: TaskPayload::Json(receipt_data),
            assigned_agent_type: Some(AgentType::ReceiptAgent),
            ..Default::default()
        }
    }

    /// Create a report generation task
    pub fn generate_report(report_type: &str) -> Self {
        Self {
            task_type: TaskType::GenerateReport,
            payload: TaskPayload::Json(serde_json::json!({ "report_type": report_type })),
            assigned_agent_type: Some(AgentType::ReportingAgent),
            ..Default::default()
        }
    }

    /// Create a tax calculation task
    pub fn calculate_taxes(tax_data: serde_json::Value) -> Self {
        Self {
            task_type: TaskType::CalculateTaxes,
            payload: TaskPayload::Json(tax_data),
            assigned_agent_type: Some(AgentType::TaxAgent),
            ..Default::default()
        }
    }

    /// Create a payroll calculation task
    pub fn calculate_payroll(payroll_data: serde_json::Value) -> Self {
        Self {
            task_type: TaskType::CalculatePayroll,
            payload: TaskPayload::Json(payroll_data),
            assigned_agent_type: Some(AgentType::PayrollAgent),
            ..Default::default()
        }
    }

    /// Create an audit check task
    pub fn audit_check(audit_data: serde_json::Value) -> Self {
        Self {
            task_type: TaskType::AuditCheck,
            payload: TaskPayload::Json(audit_data),
            assigned_agent_type: Some(AgentType::AuditAgent),
            ..Default::default()
        }
    }

    /// Mark the task as completed with a result
    pub fn complete(mut self, result: TaskResult) -> Self {
        self.status = TaskStatus::Completed;
        self.result = Some(result);
        self.updated_at = Utc::now();
        self
    }

    /// Mark the task as failed
    pub fn fail(mut self, error: &str) -> Self {
        self.status = TaskStatus::Failed(error.to_string());
        self.updated_at = Utc::now();
        self
    }

    /// Check if the task can be retried
    pub fn can_retry(&self) -> bool {
        self.retry_count < self.max_retries
    }

    /// Increment retry count
    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
        self.updated_at = Utc::now();
    }
}

/// Payload data for a task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskPayload {
    /// No payload
    Empty,
    /// Transaction data
    Transaction(Transaction),
    /// Account data
    Account(Account),
    /// Document data
    Document(Document),
    /// String data
    String(String),
    /// Binary data (base64 encoded)
    Binary(String),
    /// JSON data
    Json(serde_json::Value),
    /// Key-value pairs
    Map(HashMap<String, String>),
    /// Multiple transactions
    Transactions(Vec<Transaction>),
    /// Multiple documents
    Documents(Vec<Document>),
}

impl Default for TaskPayload {
    fn default() -> Self {
        TaskPayload::Empty
    }
}

/// Result from task execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    /// Whether the task succeeded
    pub success: bool,
    /// Message describing the result
    pub message: String,
    /// Data returned from the task
    pub data: Option<TaskPayload>,
    /// Timestamp when the task completed
    pub completed_at: DateTime<Utc>,
    /// Any warnings or additional info
    pub warnings: Vec<String>,
}

impl TaskResult {
    /// Create a successful result
    pub fn success(message: &str) -> Self {
        Self {
            success: true,
            message: message.to_string(),
            data: None,
            completed_at: Utc::now(),
            warnings: Vec::new(),
        }
    }

    /// Create a successful result with data
    pub fn success_with_data(message: &str, data: TaskPayload) -> Self {
        Self {
            success: true,
            message: message.to_string(),
            data: Some(data),
            completed_at: Utc::now(),
            warnings: Vec::new(),
        }
    }

    /// Create a failed result
    pub fn failure(message: &str) -> Self {
        Self {
            success: false,
            message: message.to_string(),
            data: None,
            completed_at: Utc::now(),
            warnings: Vec::new(),
        }
    }

    /// Add a warning
    pub fn with_warning(mut self, warning: &str) -> Self {
        self.warnings.push(warning.to_string());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::financial::{AccountType, BalanceType};

    #[test]
    fn test_task_creation() {
        let task = Task::new(TaskType::RecordTransaction);
        assert_eq!(task.task_type, TaskType::RecordTransaction);
        assert_eq!(task.status, TaskStatus::Pending);
    }

    #[test]
    fn test_record_transaction_task() {
        let transaction = Transaction::new(
            "Test transaction".to_string(),
            Utc::now(),
            vec![]
        );
        let task = Task::record_transaction(transaction);
        assert_eq!(task.task_type, TaskType::RecordTransaction);
        assert_eq!(task.assigned_agent_type, Some(AgentType::LedgerAgent));
    }

    #[test]
    fn test_task_completion() {
        let task = Task::new(TaskType::ValidateData);
        let result = TaskResult::success("Validation passed");
        let completed_task = task.complete(result);
        assert_eq!(completed_task.status, TaskStatus::Completed);
        assert!(completed_task.result.is_some());
    }

    #[test]
    fn test_task_failure() {
        let task = Task::new(TaskType::ExportData);
        let failed_task = task.fail("Export failed");
        assert_eq!(failed_task.status, TaskStatus::Failed("Export failed".to_string()));
    }

    #[test]
    fn test_task_retry() {
        let mut task = Task::new(TaskType::ImportData);
        assert!(task.can_retry());
        task.increment_retry();
        task.increment_retry();
        assert!(task.can_retry()); // max_retries is 3 by default
        task.increment_retry();
        assert!(!task.can_retry());
    }
}
