//! Invoice Module
//!
//! Handles invoice creation, payment tracking, and customer billing.

use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;
use chrono::{DateTime, Utc, NaiveDate};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tracing::{info, error, debug, warn};
use crate::database::financial::{
    Account, AccountType, EntryType, Transaction, TransactionEntry,
    TransactionType, TransactionStatus,
};
use crate::agents::agent_types::{Agent, AgentConfig, AgentType, AgentStatus};
use crate::agents::task::{Task, TaskResult, TaskPayload, TaskType};
use crate::agents::error::AgentError;

/// Invoice error types
#[derive(Debug, thiserror::Error)]
pub enum InvoiceError {
    #[error("Invoice not found: {0}")]
    InvoiceNotFound(String),

    #[error("Invalid invoice state: {0}")]
    InvalidState(String),

    #[error("Payment error: {0}")]
    PaymentError(String),

    #[error("Ledger error: {0}")]
    LedgerError(String),

    #[error("Invoice error: {0}")]
    Other(String),
}

impl InvoiceError {
    pub fn other(msg: &str) -> Self {
        Self::Other(msg.to_string())
    }
}

pub type InvoiceResult<T> = Result<T, InvoiceError>;

/// Invoice line item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceItem {
    pub id: Uuid,
    pub description: String,
    pub quantity: Decimal,
    pub unit_price: Decimal,
    pub amount: Decimal,
    pub account_id: Option<Uuid>,
}

impl InvoiceItem {
    pub fn new(description: &str, quantity: Decimal, unit_price: Decimal) -> Self {
        Self {
            id: Uuid::new_v4(),
            description: description.to_string(),
            quantity,
            unit_price,
            amount: quantity * unit_price,
            account_id: None,
        }
    }
}

/// Invoice status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum InvoiceStatus {
    Draft,
    Sent,
    Paid,
    PartiallyPaid,
    Overdue,
    Cancelled,
    CreditIssued,
}

impl Default for InvoiceStatus {
    fn default() -> Self {
        Self::Draft
    }
}

/// Invoice model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invoice {
    pub id: Uuid,
    pub invoice_number: String,
    pub customer_name: String,
    pub customer_email: String,
    pub issue_date: NaiveDate,
    pub due_date: NaiveDate,
    pub items: Vec<InvoiceItem>,
    pub subtotal: Decimal,
    pub tax_amount: Decimal,
    pub total: Decimal,
    pub amount_paid: Decimal,
    pub status: InvoiceStatus,
    pub notes: String,
    pub transaction_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Default for Invoice {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            invoice_number: String::new(),
            customer_name: String::new(),
            customer_email: String::new(),
            issue_date: now.date_naive(),
            due_date: now.date_naive(),
            items: Vec::new(),
            subtotal: dec!(0),
            tax_amount: dec!(0),
            total: dec!(0),
            amount_paid: dec!(0),
            status: InvoiceStatus::Draft,
            notes: String::new(),
            transaction_id: None,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Invoice processor
#[derive(Debug, Clone)]
pub struct InvoiceProcessor {
    pub invoices: Arc<RwLock<BTreeMap<Uuid, Invoice>>>,
    pub current_invoice_number: Arc<Mutex<u64>>,
    pub db: Option<Arc<crate::database::Database>>,
    pub ledger: Option<Arc<crate::accounting::ledger::Ledger>>,
}

impl Default for InvoiceProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl InvoiceProcessor {
    pub fn new() -> Self {
        Self {
            invoices: Arc::new(RwLock::new(BTreeMap::new())),
            current_invoice_number: Arc::new(Mutex::new(1)),
            db: None,
            ledger: None,
        }
    }

    pub async fn initialize(&mut self) -> InvoiceResult<()> {
        info!("Initializing Invoice Processor...");
        Ok(())
    }

    /// Create a new invoice.
    ///
    /// If a ledger is connected, this also creates the double-entry journal:
    ///   Dr. Accounts Receivable (1020) — total
    ///       Cr. Revenue (4000)         — subtotal
    ///       Cr. Tax Payable (2020)     — tax_amount (if > 0)
    pub async fn create_invoice(
        &mut self,
        customer_name: &str,
        customer_email: &str,
        due_date: NaiveDate,
        items: Vec<InvoiceItem>,
        notes: &str,
        tax_rate: Option<Decimal>,
    ) -> InvoiceResult<Invoice> {
        debug!("Creating invoice for customer: {}", customer_name);

        let mut counter = self.current_invoice_number.lock().await;
        let invoice_number = format!("INV-{:08}", *counter);
        *counter += 1;
        drop(counter);

        let subtotal: Decimal = items.iter().map(|i| i.amount).sum();

        // Calculate sales tax if a rate is provided (e.g. 0.0725 for 7.25%)
        let tax_amount = match tax_rate {
            Some(rate) if rate > dec!(0) => subtotal * rate,
            _ => dec!(0),
        };

        let total = subtotal + tax_amount;

        let mut invoice = Invoice {
            id: Uuid::new_v4(),
            invoice_number: invoice_number.clone(),
            customer_name: customer_name.to_string(),
            customer_email: customer_email.to_string(),
            issue_date: Utc::now().date_naive(),
            due_date,
            items,
            subtotal,
            tax_amount,
            total,
            amount_paid: dec!(0),
            status: InvoiceStatus::Draft,
            notes: notes.to_string(),
            transaction_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        // Create the AR journal entry: Dr. AR, Cr. Revenue (+ Cr. Tax Payable if taxed)
        if let Some(ref ledger) = self.ledger {
            let ar_account = ledger.get_account_by_number("1020").await
                .map_err(|e| InvoiceError::LedgerError(e.to_string()))?;
            let revenue_account = ledger.get_account_by_number("4000").await
                .map_err(|e| InvoiceError::LedgerError(e.to_string()))?;

            if let (Some(ar), Some(revenue)) = (ar_account, revenue_account) {
                let mut entries = vec![
                    TransactionEntry::new(
                        ar.id,
                        EntryType::Debit,
                        total,
                        &format!("Invoice {} — {}", invoice.invoice_number, customer_name),
                    ),
                    TransactionEntry::new(
                        revenue.id,
                        EntryType::Credit,
                        subtotal,
                        &format!("Revenue for invoice {}", invoice.invoice_number),
                    ),
                ];

                // If there's tax, credit Accrued Expenses (2020) for the tax amount
                if tax_amount > dec!(0) {
                    let tax_payable = ledger.get_account_by_number("2020").await
                        .map_err(|e| InvoiceError::LedgerError(e.to_string()))?;
                    if let Some(tp) = tax_payable {
                        entries.push(TransactionEntry::new(
                            tp.id,
                            EntryType::Credit,
                            tax_amount,
                            &format!("Sales tax for invoice {}", invoice.invoice_number),
                        ));
                    }
                }

                let txn = Transaction {
                    transaction_type: TransactionType::Invoice,
                    ..Transaction::new(
                        format!("Invoice {}", invoice.invoice_number),
                        Utc::now(),
                        entries,
                    )
                };

                match ledger.record_transaction(txn).await {
                    Ok(recorded) => {
                        invoice.transaction_id = Some(recorded.id);
                        invoice.status = InvoiceStatus::Sent;
                    }
                    Err(e) => {
                        warn!("Failed to create AR entry for invoice {}: {}", invoice.invoice_number, e);
                    }
                }
            }
        }

        self.invoices.write().await.insert(invoice.id, invoice.clone());

        if let Some(ref db) = self.db {
            if let Ok(client) = db.db().await {
                let value = serde_json::to_value(&invoice).unwrap_or_default();
                if let Err(e) = client
                    .create::<Vec<serde_json::Value>>("invoice")
                    .content(value)
                    .await
                {
                    warn!("Failed to persist invoice to SurrealDB: {}", e);
                }
            }
        }

        Ok(invoice)
    }

    /// Process payment for an invoice. Overpayments create a customer credit
    /// (Dr. Cash, Cr. AR for full payment, Cr. Customer Deposits for overage).
    pub async fn process_payment(
        &mut self,
        invoice_id: Uuid,
        amount: Decimal,
        payment_date: NaiveDate,
        payment_reference: &str,
    ) -> InvoiceResult<Invoice> {
        debug!(
            "Processing payment of {} for invoice {}",
            amount, invoice_id
        );

        let mut invoices = self.invoices.write().await;
        let invoice = invoices
            .get_mut(&invoice_id)
            .ok_or_else(|| InvoiceError::InvoiceNotFound(invoice_id.to_string()))?;

        if invoice.status == InvoiceStatus::Paid || invoice.status == InvoiceStatus::Cancelled {
            return Err(InvoiceError::InvalidState(format!(
                "Cannot process payment for invoice in {:?} state",
                invoice.status
            )));
        }

        if amount <= dec!(0) {
            return Err(InvoiceError::PaymentError(
                "Payment amount must be positive".to_string(),
            ));
        }

        let remaining = invoice.total - invoice.amount_paid;
        let applied_amount = if amount <= remaining { amount } else { remaining };
        let overage = amount - applied_amount;

        invoice.amount_paid += applied_amount;
        invoice.status = if invoice.amount_paid >= invoice.total {
            InvoiceStatus::Paid
        } else {
            InvoiceStatus::PartiallyPaid
        };
        invoice.updated_at = Utc::now();

        // Create a ledger transaction for the payment if we have a ledger
        if let Some(ref ledger) = self.ledger {
            // Find AR and Cash accounts
            let ar_account = ledger
                .get_account_by_number("1020")
                .await
                .map_err(|e| InvoiceError::LedgerError(e.to_string()))?;
            let cash_account = ledger
                .get_account_by_number("1000")
                .await
                .map_err(|e| InvoiceError::LedgerError(e.to_string()))?;

            if let (Some(ar), Some(cash)) = (ar_account, cash_account) {
                let mut entries = vec![
                    TransactionEntry::new(
                        cash.id, EntryType::Debit, amount,
                        &format!("Payment for invoice {} - {}", invoice.invoice_number, payment_reference),
                    ),
                    TransactionEntry::new(
                        ar.id, EntryType::Credit, applied_amount,
                        &format!("Payment applied to {}", invoice.invoice_number),
                    ),
                ];

                // Overpayment → credit to Customer Deposits (2010)
                if overage > dec!(0) {
                    let deposit_account = ledger
                        .get_account_by_number("2010")
                        .await
                        .map_err(|e| InvoiceError::LedgerError(e.to_string()))?;
                    if let Some(deposits) = deposit_account {
                        entries.push(TransactionEntry::new(
                            deposits.id, EntryType::Credit, overage,
                            &format!("Overpayment credit for invoice {}", invoice.invoice_number),
                        ));
                    }
                }

                let txn = Transaction {
                    transaction_type: TransactionType::Payment,
                    ..Transaction::new(
                        format!("Payment for invoice {}", invoice.invoice_number),
                        payment_date.and_hms_opt(0, 0, 0).unwrap().and_utc(),
                        entries,
                    )
                };

                match ledger.record_transaction(txn).await {
                    Ok(recorded) => { invoice.transaction_id = Some(recorded.id); }
                    Err(e) => { warn!("Failed to record payment: {}", e); }
                }
            }
        }

        let updated_invoice = invoice.clone();
        drop(invoices);

        if let Some(ref db) = self.db {
            if let Ok(client) = db.db().await {
                let v = serde_json::to_value(&updated_invoice).unwrap_or_default();
                if let Err(e) = client.create::<Vec<serde_json::Value>>("invoice").content(v).await {
                    warn!("Failed to persist invoice: {}", e);
                }
            }
        }

        Ok(updated_invoice)
    }

    /// Cancel/void an invoice with reversing journal entry.
    pub async fn cancel_invoice(&mut self, invoice_id: Uuid) -> InvoiceResult<Invoice> {
        let mut invoices = self.invoices.write().await;
        let invoice = invoices.get_mut(&invoice_id)
            .ok_or_else(|| InvoiceError::InvoiceNotFound(invoice_id.to_string()))?;

        if invoice.status == InvoiceStatus::Cancelled || invoice.status == InvoiceStatus::Paid {
            return Err(InvoiceError::InvalidState(format!(
                "Cannot cancel invoice in {:?} state", invoice.status
            )));
        }

        // Reverse the original AR entry if ledger is connected
        if let Some(ref ledger) = self.ledger {
            let ar = ledger.get_account_by_number("1020").await
                .map_err(|e| InvoiceError::LedgerError(e.to_string()))?;
            let revenue = ledger.get_account_by_number("4000").await
                .map_err(|e| InvoiceError::LedgerError(e.to_string()))?;

            if let (Some(ar), Some(rev)) = (ar, revenue) {
                let entries = vec![
                    TransactionEntry::new(rev.id, EntryType::Debit, invoice.subtotal,
                        &format!("Cancel invoice {}", invoice.invoice_number)),
                    TransactionEntry::new(ar.id, EntryType::Credit, invoice.total,
                        &format!("Cancel invoice {}", invoice.invoice_number)),
                ];

                let txn = Transaction {
                    transaction_type: TransactionType::Adjustment,
                    ..Transaction::new(
                        format!("Cancel invoice {}", invoice.invoice_number),
                        Utc::now(), entries,
                    )
                };
                if let Err(e) = ledger.record_transaction(txn).await {
                    warn!("Failed to record cancellation: {}", e);
                }
            }
        }

        invoice.status = InvoiceStatus::Cancelled;
        invoice.updated_at = Utc::now();
        let result = invoice.clone();
        drop(invoices);

        if let Some(ref db) = self.db {
            if let Ok(client) = db.db().await {
                let v = serde_json::to_value(&result).unwrap_or_default();
                if let Err(e) = client.create::<Vec<serde_json::Value>>("invoice").content(v).await {
                    warn!("Failed to persist invoice: {}", e);
                }
            }
        }

        Ok(result)
    }

    /// Get an invoice by ID
    pub async fn get_invoice(&self, id: Uuid) -> InvoiceResult<Option<Invoice>> {
        Ok(self.invoices.read().await.get(&id).cloned())
    }

    /// List all invoices
    pub async fn list_invoices(&self) -> InvoiceResult<Vec<Invoice>> {
        Ok(self.invoices.read().await.values().cloned().collect())
    }

    /// List invoices by status
    pub async fn list_invoices_by_status(
        &self,
        status: InvoiceStatus,
    ) -> InvoiceResult<Vec<Invoice>> {
        Ok(self
            .invoices
            .read()
            .await
            .values()
            .filter(|i| i.status == status)
            .cloned()
            .collect())
    }
}

/// Invoice Agent for handling invoice-related tasks
#[derive(Debug, Clone)]
pub struct InvoiceAgent {
    pub config: AgentConfig,
    pub status: AgentStatus,
    pub processor: Arc<Mutex<InvoiceProcessor>>,
}

impl InvoiceAgent {
    pub fn new(config: AgentConfig, processor: InvoiceProcessor) -> Self {
        Self {
            config,
            status: AgentStatus::Idle,
            processor: Arc::new(Mutex::new(processor)),
        }
    }

    pub fn with_defaults() -> Self {
        let config = AgentConfig::invoice_agent();
        Self::new(config, InvoiceProcessor::new())
    }
}

#[async_trait]
impl Agent for InvoiceAgent {
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
            TaskType::GenerateInvoice => self.process_generate_invoice(task).await,
            TaskType::ProcessPayment => self.process_payment_task(task).await,
            _ => Err(AgentError::TaskProcessingFailed(format!(
                "InvoiceAgent cannot handle task type: {:?}",
                task.task_type
            ))
            .into()),
        }
    }

    fn agent_type(&self) -> AgentType {
        self.config.agent_type.clone()
    }
}

impl InvoiceAgent {
    async fn process_generate_invoice(&self, task: Task) -> Result<Task, anyhow::Error> {
        let start_time = std::time::Instant::now();

        let json = match &task.payload {
            TaskPayload::Json(v) => v.clone(),
            _ => {
                return Err(AgentError::TaskProcessingFailed(
                    "Expected Json payload for GenerateInvoice task".to_string(),
                )
                .into())
            }
        };

        let customer_name = json
            .get("customer_name")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown");
        let customer_email = json
            .get("customer_email")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let due_date_str = json
            .get("due_date")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let due_date = if !due_date_str.is_empty() {
            NaiveDate::parse_from_str(due_date_str, "%Y-%m-%d")
                .unwrap_or_else(|_| Utc::now().date_naive() + chrono::Duration::days(30))
        } else {
            Utc::now().date_naive() + chrono::Duration::days(30)
        };
        let notes = json
            .get("notes")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Parse items from JSON
        let items: Vec<InvoiceItem> = json
            .get("items")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        let desc = item.get("description")?.as_str()?;
                        let qty = item
                            .get("quantity")
                            .and_then(|v| v.as_f64())
                            .map(Decimal::from_f64_retain)
                            .flatten()
                            .unwrap_or(dec!(1));
                        let price = item
                            .get("unit_price")
                            .and_then(|v| v.as_f64())
                            .map(Decimal::from_f64_retain)
                            .flatten()
                            .unwrap_or(dec!(0));
                        Some(InvoiceItem::new(desc, qty, price))
                    })
                    .collect()
            })
            .unwrap_or_default();

        if items.is_empty() {
            return Err(AgentError::TaskProcessingFailed(
                "Invoice must have at least one item".to_string(),
            )
            .into());
        }

        // Extract optional tax rate from payload (e.g. 0.0725 for 7.25% sales tax)
        let tax_rate = json.get("tax_rate")
            .and_then(|v| v.as_f64())
            .map(Decimal::from_f64_retain)
            .flatten();

        let mut processor = self.processor.lock().await;
        let invoice = processor
            .create_invoice(customer_name, customer_email, due_date, items, notes, tax_rate)
            .await
            .map_err(|e| AgentError::TaskProcessingFailed(e.to_string()))?;

        let result = TaskResult::success_with_data(
            &format!("Invoice {} created for {}", invoice.invoice_number, customer_name),
            TaskPayload::Json(serde_json::to_value(&invoice).unwrap_or_default()),
        );

        let _processing_time = start_time.elapsed().as_millis() as f64;

        Ok(task.complete(result))
    }

    async fn process_payment_task(&self, task: Task) -> Result<Task, anyhow::Error> {
        let start_time = std::time::Instant::now();

        let json = match &task.payload {
            TaskPayload::Json(v) => v.clone(),
            _ => {
                return Err(AgentError::TaskProcessingFailed(
                    "Expected Json payload for ProcessPayment task".to_string(),
                )
                .into())
            }
        };

        let invoice_id_str = json
            .get("invoice_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AgentError::TaskProcessingFailed("Missing invoice_id in payload".to_string())
            })?;
        let invoice_id = Uuid::parse_str(invoice_id_str).map_err(|e| {
            AgentError::TaskProcessingFailed(format!("Invalid invoice_id: {}", e))
        })?;

        let amount = json
            .get("amount")
            .and_then(|v| v.as_f64())
            .map(Decimal::from_f64_retain)
            .flatten()
            .ok_or_else(|| {
                AgentError::TaskProcessingFailed("Missing or invalid amount in payload".to_string())
            })?;

        let payment_date_str = json
            .get("payment_date")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let payment_date = if !payment_date_str.is_empty() {
            NaiveDate::parse_from_str(payment_date_str, "%Y-%m-%d")
                .unwrap_or_else(|_| Utc::now().date_naive())
        } else {
            Utc::now().date_naive()
        };

        let reference = json
            .get("reference")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let mut processor = self.processor.lock().await;
        let invoice = processor
            .process_payment(invoice_id, amount, payment_date, reference)
            .await
            .map_err(|e| AgentError::TaskProcessingFailed(e.to_string()))?;

        let result = TaskResult::success_with_data(
            &format!("Payment of {} processed for invoice {}", amount, invoice.invoice_number),
            TaskPayload::Json(serde_json::to_value(&invoice).unwrap_or_default()),
        );

        let _processing_time = start_time.elapsed().as_millis() as f64;

        Ok(task.complete(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_invoice_processor_creation() {
        let processor = InvoiceProcessor::new();
        assert!(processor.invoices.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_create_invoice() {
        let mut processor = InvoiceProcessor::new();

        let items = vec![
            InvoiceItem::new("Consulting services", dec!(10), dec!(150)),
            InvoiceItem::new("Software license", dec!(1), dec!(500)),
        ];

        let invoice = processor
            .create_invoice(
                "Acme Corp",
                "billing@acme.com",
                Utc::now().date_naive() + chrono::Duration::days(30),
                items,
                "Net 30",
                None,
            )
            .await
            .unwrap();

        assert_eq!(invoice.customer_name, "Acme Corp");
        assert_eq!(invoice.subtotal, dec!(2000));
        assert_eq!(invoice.total, dec!(2000));
        // No ledger connected, so no AR entry — status stays Draft
        assert_eq!(invoice.status, InvoiceStatus::Draft);
    }

    #[tokio::test]
    async fn test_process_payment() {
        let mut processor = InvoiceProcessor::new();

        let items = vec![InvoiceItem::new("Service", dec!(1), dec!(1000))];
        let invoice = processor
            .create_invoice("Test Co", "test@test.com", Utc::now().date_naive(), items, "", None)
            .await
            .unwrap();

        // Partial payment
        let updated = processor
            .process_payment(invoice.id, dec!(500), Utc::now().date_naive(), "CHK-001")
            .await
            .unwrap();
        assert_eq!(updated.amount_paid, dec!(500));
        assert_eq!(updated.status, InvoiceStatus::PartiallyPaid);

        // Full payment
        let updated = processor
            .process_payment(invoice.id, dec!(500), Utc::now().date_naive(), "CHK-002")
            .await
            .unwrap();
        assert_eq!(updated.amount_paid, dec!(1000));
        assert_eq!(updated.status, InvoiceStatus::Paid);
    }

    #[tokio::test]
    async fn test_invoice_agent() {
        let agent = InvoiceAgent::with_defaults();
        assert_eq!(agent.config.agent_type, AgentType::InvoiceAgent);
    }

    #[tokio::test]
    async fn test_invoice_agent_generate() {
        let agent = InvoiceAgent::with_defaults();

        let payload = serde_json::json!({
            "customer_name": "Test Corp",
            "customer_email": "test@test.com",
            "due_date": "2026-12-31",
            "items": [
                {"description": "Consulting", "quantity": 5, "unit_price": 200}
            ],
            "notes": "Net 30"
        });

        let task = Task {
            task_type: TaskType::GenerateInvoice,
            payload: TaskPayload::Json(payload),
            ..Default::default()
        };

        let result = agent.process_task(task).await;
        assert!(result.is_ok());
        let completed = result.unwrap();
        assert_eq!(completed.status, crate::agents::task::TaskStatus::Completed);
    }
}
