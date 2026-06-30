//! Accounts Payable Module
//!
//! Handles vendor bills, payment scheduling, and the full AP lifecycle:
//!   1. Enter vendor bill → Debit Expense, Credit Accounts Payable (2000)
//!   2. Schedule payment date
//!   3. Mark as paid → Debit Accounts Payable (2000), Credit Cash (1000)

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use chrono::{DateTime, Utc, NaiveDate};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tracing::info;

use crate::database::financial::{
    Account, EntryType, Transaction, TransactionEntry, TransactionType, TransactionStatus,
};
use crate::accounting::ledger::Ledger;
use crate::agents::agent_types::{Agent, AgentConfig, AgentType, AgentStatus};
use crate::agents::task::{Task, TaskResult, TaskPayload, TaskType};
use crate::agents::error::AgentError;

// ── Error Types ─────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum AccountsPayableError {
    #[error("Vendor not found: {0}")]
    VendorNotFound(String),
    #[error("Bill not found: {0}")]
    BillNotFound(String),
    #[error("Invalid bill state: {0}")]
    InvalidState(String),
    #[error("Ledger error: {0}")]
    LedgerError(String),
    #[error("AP error: {0}")]
    Other(String),
}

pub type ApResult<T> = Result<T, AccountsPayableError>;

// ── AP Bill Status ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ApBillStatus {
    Draft,
    Approved,
    Scheduled,
    Paid,
    Void,
}

// ── Vendor ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vendor {
    pub id: Uuid,
    pub name: String,
    pub contact_name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub tax_id: Option<String>,
    pub currency: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Vendor {
    pub fn new(name: &str) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            contact_name: None,
            email: None,
            phone: None,
            address: None,
            tax_id: None,
            currency: "USD".to_string(),
            created_at: now,
            updated_at: now,
        }
    }
}

// ── AP Bill Line ───────────────────────────────────────────────────────────

/// A single line on a multi-line AP bill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApBillLine {
    pub id: Uuid,
    /// Expense/asset/liability account to debit for this line.
    pub account_id: Uuid,
    pub amount: Decimal,
    pub description: String,
}

impl ApBillLine {
    pub fn new(account_id: Uuid, amount: Decimal, description: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            account_id,
            amount,
            description: description.to_string(),
        }
    }
}

// ── AP Bill ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApBill {
    pub id: Uuid,
    pub vendor_id: Uuid,
    pub bill_number: String,
    pub description: String,
    pub amount: Decimal,
    pub expense_account_id: Uuid,
    /// Multi-line bill support: each line debits a different account.
    /// For single-line bills (backward compat), this contains one entry.
    #[serde(default)]
    pub lines: Vec<ApBillLine>,
    /// Total amount paid so far (supports partial payments).
    #[serde(default)]
    pub paid_amount: Decimal,
    pub due_date: NaiveDate,
    pub scheduled_payment_date: Option<NaiveDate>,
    pub status: ApBillStatus,
    pub transaction_id: Option<Uuid>,
    pub payment_transaction_id: Option<Uuid>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ApBill {
    pub fn new(
        vendor_id: Uuid,
        description: &str,
        amount: Decimal,
        expense_account_id: Uuid,
        due_date: NaiveDate,
    ) -> Self {
        let now = Utc::now();
        let line = ApBillLine::new(expense_account_id, amount, description);
        Self {
            id: Uuid::new_v4(),
            vendor_id,
            bill_number: format!("BILL-{}", &Uuid::new_v4().to_string()[..8]),
            description: description.to_string(),
            amount,
            expense_account_id,
            lines: vec![line],
            paid_amount: dec!(0),
            due_date,
            scheduled_payment_date: None,
            status: ApBillStatus::Draft,
            transaction_id: None,
            payment_transaction_id: None,
            notes: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Create a multi-line bill.
    pub fn new_multi_line(
        vendor_id: Uuid,
        description: &str,
        lines: Vec<ApBillLine>,
        due_date: NaiveDate,
    ) -> Self {
        let now = Utc::now();
        let amount: Decimal = lines.iter().map(|l| l.amount).sum();
        let expense_account_id = lines.first().map(|l| l.account_id).unwrap_or_else(Uuid::nil);
        Self {
            id: Uuid::new_v4(),
            vendor_id,
            bill_number: format!("BILL-{}", &Uuid::new_v4().to_string()[..8]),
            description: description.to_string(),
            amount,
            expense_account_id,
            lines,
            paid_amount: dec!(0),
            due_date,
            scheduled_payment_date: None,
            status: ApBillStatus::Draft,
            transaction_id: None,
            payment_transaction_id: None,
            notes: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Remaining balance on the bill (amount - paid_amount).
    pub fn remaining_balance(&self) -> Decimal {
        self.amount - self.paid_amount
    }
}

// ── AP Processor ────────────────────────────────────────────────────────────

/// Accounts Payable processor.
///
/// Manages vendor bills and their payment lifecycle. Integrates with the
/// shared `Ledger` for double-entry accounting.
#[derive(Debug)]
pub struct ApProcessor {
    pub vendors: Vec<Vendor>,
    pub bills: Vec<ApBill>,
    pub ledger: Option<Arc<Ledger>>,
    /// Account ID for Accounts Payable liability (default: 2000).
    pub ap_account_id: Uuid,
    /// Account ID for Cash (default: 1000).
    pub cash_account_id: Uuid,
}

impl ApProcessor {
    pub fn new() -> Self {
        Self {
            vendors: Vec::new(),
            bills: Vec::new(),
            ledger: None,
            ap_account_id: Uuid::nil(), // Will be set on init
            cash_account_id: Uuid::nil(),
        }
    }

    /// Resolve the AP liability account and Cash account from the ledger.
    pub async fn initialize(&mut self) -> ApResult<()> {
        if let Some(ref ledger) = self.ledger {
            let accounts = ledger.accounts.read().await;
            // Find AP account by number 2000 (Accounts Payable)
            if let Some(ap) = accounts.values().find(|a| a.number == "2000") {
                self.ap_account_id = ap.id;
            } else {
                return Err(AccountsPayableError::LedgerError(
                    "AP account (2000) not found in chart of accounts".into(),
                ));
            }
            // Find Cash account by number 1000
            if let Some(cash) = accounts.values().find(|a| a.number == "1000") {
                self.cash_account_id = cash.id;
            } else {
                return Err(AccountsPayableError::LedgerError(
                    "Cash account (1000) not found in chart of accounts".into(),
                ));
            }
        }
        Ok(())
    }

    /// Register a new vendor.
    pub fn add_vendor(&mut self, vendor: Vendor) -> Vendor {
        self.vendors.push(vendor.clone());
        vendor
    }

    /// Find a vendor by ID.
    pub fn find_vendor(&self, id: Uuid) -> Option<&Vendor> {
        self.vendors.iter().find(|v| v.id == id)
    }

    // ── Bill Lifecycle ────────────────────────────────────────────────────

    /// Enter a vendor bill — creates the bill and posts it to the ledger:
    ///   Debit: Expense account   (increase expense)
    ///   Credit: Accounts Payable  (increase liability)
    pub async fn enter_bill(&mut self, mut bill: ApBill) -> ApResult<ApBill> {
        let ledger = self
            .ledger
            .as_ref()
            .ok_or_else(|| AccountsPayableError::LedgerError("No ledger configured".into()))?;

        // Validate
        if bill.amount <= dec!(0) {
            return Err(AccountsPayableError::Other(
                "Bill amount must be positive".into(),
            ));
        }

        // Create the double-entry transaction — one debit per bill line,
        // one credit to Accounts Payable for the total.
        let now = Utc::now();
        let mut entries: Vec<TransactionEntry> = Vec::new();
        for line in &bill.lines {
            entries.push(TransactionEntry {
                id: Uuid::new_v4(),
                account_id: line.account_id,
                amount: line.amount,
                entry_type: EntryType::Debit,
                description: format!("Bill {} — {}", bill.bill_number, line.description),
                reference: None,
                ..Default::default()
            });
        }
        // Credit Accounts Payable for the total bill amount
        entries.push(TransactionEntry {
            id: Uuid::new_v4(),
            account_id: self.ap_account_id,
            amount: bill.amount,
            entry_type: EntryType::Credit,
            description: format!("Bill {} — AP liability", bill.bill_number),
            reference: None,
            ..Default::default()
        });

        let txn = Transaction {
            id: Uuid::new_v4(),
            number: format!("AP-{}", &bill.bill_number),
            description: format!("AP Bill: {} — {}", bill.bill_number, bill.description),
            date: now,
            transaction_type: TransactionType::JournalEntry,
            status: TransactionStatus::Pending,
            entries,
            journal_entry_id: None,
            document_ids: vec![],
            metadata: serde_json::json!({
                "ap_bill_id": bill.id.to_string(),
                "vendor_id": bill.vendor_id.to_string(),
            }),
            created_at: now,
            updated_at: now,
        };

        match ledger.record_transaction(txn).await {
            Ok(recorded) => {
                bill.transaction_id = Some(recorded.id);
                bill.status = ApBillStatus::Approved;
                bill.updated_at = now;
                self.bills.push(bill.clone());
                info!(
                    "AP bill {} entered: {} for ${}",
                    bill.bill_number, bill.description, bill.amount
                );
                Ok(bill)
            }
            Err(e) => Err(AccountsPayableError::LedgerError(e.to_string())),
        }
    }

    /// Schedule a payment date for an approved bill.
    pub fn schedule_payment(&mut self, bill_id: Uuid, payment_date: NaiveDate) -> ApResult<ApBill> {
        let bill = self
            .bills
            .iter_mut()
            .find(|b| b.id == bill_id)
            .ok_or_else(|| AccountsPayableError::BillNotFound(bill_id.to_string()))?;

        if bill.status != ApBillStatus::Approved && bill.status != ApBillStatus::Scheduled {
            return Err(AccountsPayableError::InvalidState(format!(
                "Bill {} must be Approved or Scheduled to schedule payment (current: {:?})",
                bill.bill_number, bill.status
            )));
        }

        bill.scheduled_payment_date = Some(payment_date);
        bill.status = ApBillStatus::Scheduled;
        bill.updated_at = Utc::now();
        info!(
            "AP bill {} scheduled for payment on {}",
            bill.bill_number, payment_date
        );
        Ok(bill.clone())
    }

    /// Pay a bill — records the payment in the ledger:
    ///   Debit: Accounts Payable (reduce liability)
    ///   Credit: Cash              (reduce asset)
    pub async fn pay_bill(&mut self, bill_id: Uuid) -> ApResult<ApBill> {
        let ledger = self
            .ledger
            .as_ref()
            .ok_or_else(|| AccountsPayableError::LedgerError("No ledger configured".into()))?;

        // Find the bill
        let bill_data = {
            let bill = self
                .bills
                .iter()
                .find(|b| b.id == bill_id)
                .ok_or_else(|| AccountsPayableError::BillNotFound(bill_id.to_string()))?;

            if bill.status == ApBillStatus::Paid {
                return Err(AccountsPayableError::InvalidState(format!(
                    "Bill {} is already paid",
                    bill.bill_number
                )));
            }
            if bill.status == ApBillStatus::Void {
                return Err(AccountsPayableError::InvalidState(format!(
                    "Bill {} is void",
                    bill.bill_number
                )));
            }

            bill.clone()
        };

        // Create the payment transaction
        let now = Utc::now();
        let payment_txn = Transaction {
            id: Uuid::new_v4(),
            number: format!("AP-PMT-{}", &bill_data.bill_number),
            description: format!("Payment for AP Bill: {}", bill_data.bill_number),
            date: now,
            transaction_type: TransactionType::JournalEntry,
            status: TransactionStatus::Pending,
            entries: vec![
                TransactionEntry {
                    id: Uuid::new_v4(),
                    account_id: self.ap_account_id,
                    amount: bill_data.amount,
                    entry_type: EntryType::Debit,
                    description: format!("Payment of bill {} — reduce AP", bill_data.bill_number),
                    reference: None,
                    ..Default::default()
                },
                TransactionEntry {
                    id: Uuid::new_v4(),
                    account_id: self.cash_account_id,
                    amount: bill_data.amount,
                    entry_type: EntryType::Credit,
                    description: format!("Payment of bill {} — cash out", bill_data.bill_number),
                    reference: None,
                    ..Default::default()
                },
            ],
            journal_entry_id: None,
            document_ids: vec![],
            metadata: serde_json::json!({
                "ap_bill_id": bill_data.id.to_string(),
                "vendor_id": bill_data.vendor_id.to_string(),
                "payment": true,
            }),
            created_at: now,
            updated_at: now,
        };

        match ledger.record_transaction(payment_txn).await {
            Ok(recorded) => {
                // Update the bill status
                let bill = self
                    .bills
                    .iter_mut()
                    .find(|b| b.id == bill_id)
                    .unwrap();
                bill.paid_amount = bill.amount; // Full payment
                bill.status = ApBillStatus::Paid;
                bill.payment_transaction_id = Some(recorded.id);
                bill.updated_at = now;
                info!(
                    "AP bill {} paid: ${} from cash",
                    bill.bill_number, bill.amount
                );
                Ok(bill.clone())
            }
            Err(e) => Err(AccountsPayableError::LedgerError(e.to_string())),
        }
    }

    /// Make a partial payment on a bill — records the payment in the ledger
    /// for the specified amount:
    ///   Debit: Accounts Payable (reduce liability)
    ///   Credit: Cash              (reduce asset)
    ///
    /// The bill is marked as Paid only when the full amount has been paid.
    /// Returns the updated bill.
    pub async fn pay_bill_partial(
        &mut self,
        bill_id: Uuid,
        amount: Decimal,
    ) -> ApResult<ApBill> {
        let ledger = self
            .ledger
            .as_ref()
            .ok_or_else(|| AccountsPayableError::LedgerError("No ledger configured".into()))?;

        // Find the bill and validate state
        let bill_data = {
            let bill = self
                .bills
                .iter()
                .find(|b| b.id == bill_id)
                .ok_or_else(|| AccountsPayableError::BillNotFound(bill_id.to_string()))?;

            if bill.status == ApBillStatus::Paid {
                return Err(AccountsPayableError::InvalidState(format!(
                    "Bill {} is already paid",
                    bill.bill_number
                )));
            }
            if bill.status == ApBillStatus::Void {
                return Err(AccountsPayableError::InvalidState(format!(
                    "Bill {} is void",
                    bill.bill_number
                )));
            }

            if amount <= dec!(0) {
                return Err(AccountsPayableError::Other(
                    "Payment amount must be positive".into(),
                ));
            }

            let remaining = bill.remaining_balance();
            if amount > remaining {
                return Err(AccountsPayableError::InvalidState(format!(
                    "Payment ${} exceeds remaining balance ${} on bill {}",
                    amount, remaining, bill.bill_number
                )));
            }

            bill.clone()
        };

        // Create the partial payment transaction
        let now = Utc::now();
        let payment_txn = Transaction {
            id: Uuid::new_v4(),
            number: format!("AP-PMT-PARTIAL-{}", &bill_data.bill_number),
            description: format!(
                "Partial payment for AP Bill: {} (${})",
                bill_data.bill_number, amount
            ),
            date: now,
            transaction_type: TransactionType::JournalEntry,
            status: TransactionStatus::Pending,
            entries: vec![
                TransactionEntry {
                    id: Uuid::new_v4(),
                    account_id: self.ap_account_id,
                    amount,
                    entry_type: EntryType::Debit,
                    description: format!(
                        "Partial payment of bill {} — reduce AP",
                        bill_data.bill_number
                    ),
                    reference: None,
                    ..Default::default()
                },
                TransactionEntry {
                    id: Uuid::new_v4(),
                    account_id: self.cash_account_id,
                    amount,
                    entry_type: EntryType::Credit,
                    description: format!(
                        "Partial payment of bill {} — cash out",
                        bill_data.bill_number
                    ),
                    reference: None,
                    ..Default::default()
                },
            ],
            journal_entry_id: None,
            document_ids: vec![],
            metadata: serde_json::json!({
                "ap_bill_id": bill_data.id.to_string(),
                "vendor_id": bill_data.vendor_id.to_string(),
                "payment": true,
                "partial": true,
                "amount": amount.to_string(),
            }),
            created_at: now,
            updated_at: now,
        };

        match ledger.record_transaction(payment_txn).await {
            Ok(recorded) => {
                let bill = self
                    .bills
                    .iter_mut()
                    .find(|b| b.id == bill_id)
                    .unwrap();
                bill.paid_amount += amount;
                // Mark as Paid only if fully paid
                if bill.paid_amount >= bill.amount {
                    bill.status = ApBillStatus::Paid;
                }
                bill.payment_transaction_id = Some(recorded.id);
                bill.updated_at = now;
                info!(
                    "AP bill {} partial payment: ${} (paid ${} of ${})",
                    bill.bill_number, amount, bill.paid_amount, bill.amount
                );
                Ok(bill.clone())
            }
            Err(e) => Err(AccountsPayableError::LedgerError(e.to_string())),
        }
    }

    /// Void a bill (reverses the original entry if already approved).
    pub async fn void_bill(&mut self, bill_id: Uuid) -> ApResult<ApBill> {
        let bill = self
            .bills
            .iter_mut()
            .find(|b| b.id == bill_id)
            .ok_or_else(|| AccountsPayableError::BillNotFound(bill_id.to_string()))?;

        if bill.status == ApBillStatus::Paid {
            return Err(AccountsPayableError::InvalidState(
                "Cannot void a paid bill".into(),
            ));
        }

        if bill.status == ApBillStatus::Approved || bill.status == ApBillStatus::Scheduled {
            // Reverse the original AP entry if there was one
            if let Some(ref ledger) = self.ledger {
                if let Some(txn_id) = bill.transaction_id {
                    // Create reversal entry
                    let now = Utc::now();
                    let reversal = Transaction {
                        id: Uuid::new_v4(),
                        number: format!("AP-REV-{}", &bill.bill_number),
                        description: format!("Reversal of AP Bill: {}", bill.bill_number),
                        date: now,
                        transaction_type: TransactionType::JournalEntry,
                        status: TransactionStatus::Pending,
                        entries: vec![
                            TransactionEntry {
                                id: Uuid::new_v4(),
                                account_id: self.ap_account_id,
                                amount: bill.amount,
                                entry_type: EntryType::Debit,
                                description: format!("Reverse bill {} — remove AP", bill.bill_number),
                                reference: None,
                                ..Default::default()
                            },
                            TransactionEntry {
                                id: Uuid::new_v4(),
                                account_id: bill.expense_account_id,
                                amount: bill.amount,
                                entry_type: EntryType::Credit,
                                description: format!("Reverse bill {} — remove expense", bill.bill_number),
                                reference: None,
                                ..Default::default()
                            },
                        ],
                        journal_entry_id: None,
                        document_ids: vec![],
                        metadata: serde_json::json!({
                            "ap_bill_id": bill.id.to_string(),
                            "reversal_of": txn_id.to_string(),
                        }),
                        created_at: now,
                        updated_at: now,
                    };
                    let _ = ledger.record_transaction(reversal).await;
                }
            }
        }

        bill.status = ApBillStatus::Void;
        bill.updated_at = Utc::now();
        info!("AP bill {} voided", bill.bill_number);
        Ok(bill.clone())
    }

    /// List all bills optionally filtered by status.
    pub fn list_bills(&self, status: Option<ApBillStatus>) -> Vec<ApBill> {
        match status {
            Some(s) => self.bills.iter().filter(|b| b.status == s).cloned().collect(),
            None => self.bills.clone(),
        }
    }

    /// Get total outstanding AP (approved + scheduled, not paid/void).
    /// Uses remaining_balance() to account for partial payments.
    pub fn outstanding_total(&self) -> Decimal {
        self.bills
            .iter()
            .filter(|b| b.status == ApBillStatus::Approved || b.status == ApBillStatus::Scheduled)
            .map(|b| b.remaining_balance())
            .sum()
    }
}

// ── AP Agent ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ApAgent {
    pub config: AgentConfig,
    pub status: AgentStatus,
    pub processor: Arc<Mutex<ApProcessor>>,
}

impl ApAgent {
    pub fn new(config: AgentConfig, processor: ApProcessor) -> Self {
        Self {
            config,
            status: AgentStatus::Idle,
            processor: Arc::new(Mutex::new(processor)),
        }
    }

    pub fn with_defaults() -> Self {
        let config = AgentConfig {
            id: Uuid::new_v4(),
            name: "Accounts Payable Agent".into(),
            agent_type: AgentType::ApAgent,
            enabled: true,
            ..AgentConfig::default()
        };
        Self::new(config, ApProcessor::new())
    }
}

#[async_trait]
impl Agent for ApAgent {
    fn config(&self) -> &AgentConfig {
        &self.config
    }

    fn status(&self) -> AgentStatus {
        self.status.clone()
    }

    async fn initialize(&mut self) -> Result<(), anyhow::Error> {
        self.status = AgentStatus::Initializing;
        let mut processor = self.processor.lock().await;
        processor.initialize().await.map_err(|e| anyhow::anyhow!(e.to_string()))?;
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

        let mut processor = self.processor.lock().await;

        match task.task_type {
            TaskType::RecordTransaction => self.handle_ap_bill(&mut processor, task).await,
            TaskType::ProcessPayment => self.handle_ap_payment(&mut processor, task).await,
            _ => Err(AgentError::TaskProcessingFailed(format!(
                "ApAgent cannot handle task type: {:?}",
                task.task_type
            ))
            .into()),
        }
    }

    fn agent_type(&self) -> AgentType {
        self.config.agent_type.clone()
    }
}

impl ApAgent {
    async fn handle_ap_bill(
        &self,
        processor: &mut ApProcessor,
        task: Task,
    ) -> Result<Task, anyhow::Error> {
        let json = match &task.payload {
            TaskPayload::Json(v) => v.clone(),
            _ => return Err(anyhow::anyhow!("Expected Json payload for AP bill")),
        };

        let vendor_name = json.get("vendor_name").and_then(|v| v.as_str()).unwrap_or("Unknown Vendor");
        let description = json.get("description").and_then(|v| v.as_str()).unwrap_or("AP Bill");
        let amount_str = json.get("amount").and_then(|v| v.as_str()).unwrap_or("0");
        let amount: Decimal = amount_str.parse().unwrap_or(dec!(0));

        // Find or create vendor
        let vendor = {
            let existing = processor.vendors.iter().find(|v| v.name.eq_ignore_ascii_case(vendor_name));
            if let Some(v) = existing {
                v.clone()
            } else {
                let v = Vendor::new(vendor_name);
                processor.add_vendor(v.clone());
                v
            }
        };

        // Resolve expense account from ledger
        let expense_account_id = {
            if let Some(ref ledger) = processor.ledger {
                let accounts = ledger.accounts.read().await;
                // Default to Office Supplies (5000) if not specified
                let account_number = json.get("expense_account").and_then(|v| v.as_str()).unwrap_or("5000");
                accounts.values()
                    .find(|a| a.number == account_number)
                    .map(|a| a.id)
                    .unwrap_or_else(Uuid::nil)
            } else {
                Uuid::nil()
            }
        };

        let due_date_str = json.get("due_date").and_then(|v| v.as_str()).unwrap_or("");
        let due_date = if due_date_str.is_empty() {
            Utc::now().date_naive() + chrono::Duration::days(30)
        } else {
            NaiveDate::parse_from_str(due_date_str, "%Y-%m-%d").unwrap_or_else(|_| {
                Utc::now().date_naive() + chrono::Duration::days(30)
            })
        };

        let bill = ApBill::new(vendor.id, description, amount, expense_account_id, due_date);
        let bill = processor.enter_bill(bill).await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        let result_data = serde_json::json!({
            "bill_id": bill.id.to_string(),
            "bill_number": bill.bill_number,
            "vendor": vendor.name,
            "amount": bill.amount.to_string(),
            "status": format!("{:?}", bill.status),
        });

        Ok(task.complete(TaskResult {
            success: true,
            message: format!("AP bill {} entered for {}", bill.bill_number, vendor.name),
            data: Some(TaskPayload::Json(result_data)),
            completed_at: Utc::now(),
            warnings: vec![],
        }))
    }

    async fn handle_ap_payment(
        &self,
        processor: &mut ApProcessor,
        task: Task,
    ) -> Result<Task, anyhow::Error> {
        let json = match &task.payload {
            TaskPayload::Json(v) => v.clone(),
            _ => return Err(anyhow::anyhow!("Expected Json payload for AP payment")),
        };

        let bill_id_str = json.get("bill_id").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("bill_id is required"))?;
        let bill_id = Uuid::parse_str(bill_id_str)
            .map_err(|e| anyhow::anyhow!("Invalid bill_id: {}", e))?;

        let bill = processor.pay_bill(bill_id).await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        let result_data = serde_json::json!({
            "bill_id": bill.id.to_string(),
            "bill_number": bill.bill_number,
            "amount_paid": bill.amount.to_string(),
            "status": "Paid",
        });

        Ok(task.complete(TaskResult {
            success: true,
            message: format!("AP bill {} paid", bill.bill_number),
            data: Some(TaskPayload::Json(result_data)),
            completed_at: Utc::now(),
            warnings: vec![],
        }))
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounting::ledger::Ledger;
    use crate::database::financial::{Account, AccountType, AccountStatus, BalanceType};

    async fn setup_ledger() -> Ledger {
        let mut ledger = Ledger::new();
        ledger.initialize().await.unwrap();
        ledger
    }

    async fn setup_processor(ledger: Arc<Ledger>) -> ApProcessor {
        let mut processor = ApProcessor::new();
        processor.ledger = Some(ledger.clone());
        processor.initialize().await.unwrap();
        processor
    }

    #[tokio::test]
    async fn test_ap_processor_creation() {
        let processor = ApProcessor::new();
        assert!(processor.vendors.is_empty());
        assert!(processor.bills.is_empty());
    }

    #[tokio::test]
    async fn test_add_vendor() {
        let mut processor = ApProcessor::new();
        let vendor = processor.add_vendor(Vendor::new("Acme Supplies"));
        assert_eq!(vendor.name, "Acme Supplies");
        assert_eq!(processor.vendors.len(), 1);
        assert!(processor.find_vendor(vendor.id).is_some());
    }

    #[tokio::test]
    async fn test_enter_bill_creates_transaction() {
        let ledger = Arc::new(setup_ledger().await);
        let mut processor = setup_processor(ledger.clone()).await;
        let vendor = processor.add_vendor(Vendor::new("Office Depot"));

        let expense_id = ledger.accounts.read().await
            .values().find(|a| a.number == "5000").unwrap().id;

        let bill = ApBill::new(
            vendor.id,
            "Office supplies",
            dec!(250),
            expense_id,
            Utc::now().date_naive() + chrono::Duration::days(30),
        );

        let bill = processor.enter_bill(bill).await.unwrap();
        assert_eq!(bill.status, ApBillStatus::Approved);
        assert!(bill.transaction_id.is_some());

        // Verify the transaction was recorded
        let txns = ledger.list_transactions().await.unwrap();
        assert!(!txns.is_empty());
        let txn = txns.iter().find(|t| t.id == bill.transaction_id.unwrap()).unwrap();
        assert_eq!(txn.entries.len(), 2);
        assert_eq!(txn.status, TransactionStatus::Posted);
    }

    #[tokio::test]
    async fn test_schedule_payment() {
        let mut processor = ApProcessor::new();
        let vendor = processor.add_vendor(Vendor::new("TestCo"));
        let bill = ApBill::new(vendor.id, "Test", dec!(100), Uuid::new_v4(), NaiveDate::from_ymd_opt(2026, 7, 15).unwrap());
        processor.bills.push(bill.clone());

        // Can't schedule a Draft bill
        assert!(processor.schedule_payment(bill.id, NaiveDate::from_ymd_opt(2026, 7, 10).unwrap()).is_err());

        // Set to Approved
        processor.bills[0].status = ApBillStatus::Approved;
        let scheduled = processor.schedule_payment(bill.id, NaiveDate::from_ymd_opt(2026, 7, 10).unwrap()).unwrap();
        assert_eq!(scheduled.status, ApBillStatus::Scheduled);
        assert_eq!(scheduled.scheduled_payment_date, Some(NaiveDate::from_ymd_opt(2026, 7, 10).unwrap()));
    }

    #[tokio::test]
    async fn test_pay_bill_full_workflow() {
        let ledger = Arc::new(setup_ledger().await);
        let mut processor = setup_processor(ledger.clone()).await;
        let vendor = processor.add_vendor(Vendor::new("Staples"));

        let expense_id = ledger.accounts.read().await
            .values().find(|a| a.number == "5000").unwrap().id;

        let bill = ApBill::new(vendor.id, "Printer ink", dec!(89.99), expense_id, NaiveDate::from_ymd_opt(2026, 8, 1).unwrap());

        // 1. Enter bill
        let bill = processor.enter_bill(bill).await.unwrap();
        assert_eq!(bill.status, ApBillStatus::Approved);

        // Verify AP liability increased
        let ap_balance = ledger.get_account_balance(processor.ap_account_id).await.unwrap();
        assert_eq!(ap_balance, dec!(89.99)); // Credit to AP = positive liability

        // 2. Schedule payment
        let bill = processor.schedule_payment(bill.id, NaiveDate::from_ymd_opt(2026, 7, 25).unwrap()).unwrap();
        assert_eq!(bill.status, ApBillStatus::Scheduled);

        // 3. Pay
        let bill = processor.pay_bill(bill.id).await.unwrap();
        assert_eq!(bill.status, ApBillStatus::Paid);
        assert!(bill.payment_transaction_id.is_some());

        // After payment, AP should be back to 0 (debit to AP offsets the credit)
        let ap_balance = ledger.get_account_balance(processor.ap_account_id).await.unwrap();
        assert_eq!(ap_balance, dec!(0));
    }

    #[tokio::test]
    async fn test_void_bill() {
        let ledger = Arc::new(setup_ledger().await);
        let mut processor = setup_processor(ledger.clone()).await;
        let vendor = processor.add_vendor(Vendor::new("Supplies Inc"));

        let expense_id = ledger.accounts.read().await
            .values().find(|a| a.number == "5000").unwrap().id;

        let bill = ApBill::new(vendor.id, "Test void", dec!(50), expense_id, NaiveDate::from_ymd_opt(2026, 7, 1).unwrap());
        let bill = processor.enter_bill(bill).await.unwrap();

        let bill = processor.void_bill(bill.id).await.unwrap();
        assert_eq!(bill.status, ApBillStatus::Void);

        // After void+reversal, AP should be back to 0
        let ap_balance = ledger.get_account_balance(processor.ap_account_id).await.unwrap();
        assert_eq!(ap_balance, dec!(0));
    }

    #[tokio::test]
    async fn test_outstanding_total() {
        let ledger = Arc::new(setup_ledger().await);
        let mut processor = setup_processor(ledger.clone()).await;
        let vendor = processor.add_vendor(Vendor::new("VendorA"));

        let expense_id = ledger.accounts.read().await
            .values().find(|a| a.number == "5000").unwrap().id;

        // Enter two bills
        let bill1 = ApBill::new(vendor.id, "Bill 1", dec!(100), expense_id, NaiveDate::from_ymd_opt(2026, 8, 1).unwrap());
        let bill2 = ApBill::new(vendor.id, "Bill 2", dec!(200), expense_id, NaiveDate::from_ymd_opt(2026, 9, 1).unwrap());
        processor.enter_bill(bill1).await.unwrap();
        processor.enter_bill(bill2).await.unwrap();

        // Both should be outstanding
        assert_eq!(processor.outstanding_total(), dec!(300));

        // Pay one
        let bill_id = processor.bills[0].id;
        processor.pay_bill(bill_id).await.unwrap();

        // Only one outstanding now
        assert_eq!(processor.outstanding_total(), dec!(200));
    }

    #[tokio::test]
    async fn test_ap_agent_process_bill() {
        let ledger = Arc::new(setup_ledger().await);
        let mut processor = setup_processor(ledger.clone()).await;
        processor.add_vendor(Vendor::new("AgentCo"));

        let agent = ApAgent::new(
            AgentConfig {
                id: Uuid::new_v4(),
                name: "AP Agent Test".into(),
                agent_type: AgentType::ApAgent,
                enabled: true,
                ..AgentConfig::default()
            },
            processor,
        );

        let task = Task::new(TaskType::RecordTransaction);
        let task = task.with_payload(TaskPayload::Json(serde_json::json!({
            "vendor_name": "AgentCo",
            "description": "Agent test bill",
            "amount": "150.00",
            "expense_account": "5000",
        })));

        let result = agent.process_task(task).await.unwrap();
        assert_eq!(result.status, crate::agents::task::TaskStatus::Completed);
        let task_result = result.result.as_ref().unwrap();
        if let TaskPayload::Json(ref data) = task_result.data.as_ref().unwrap() {
            assert_eq!(data["vendor"], "AgentCo");
            assert_eq!(data["amount"], "150.00");
        } else {
            panic!("Expected Json payload");
        }
    }

    #[tokio::test]
    async fn test_pay_bill_partial() {
        let ledger = Arc::new(setup_ledger().await);
        let mut processor = setup_processor(ledger.clone()).await;
        let vendor = processor.add_vendor(Vendor::new("PartialCo"));

        let expense_id = ledger.accounts.read().await
            .values().find(|a| a.number == "5000").unwrap().id;

        let bill = ApBill::new(
            vendor.id,
            "Partial payment test",
            dec!(1000),
            expense_id,
            NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(),
        );

        let bill = processor.enter_bill(bill).await.unwrap();
        assert_eq!(bill.paid_amount, dec!(0));

        // Make a partial payment of $400
        let bill = processor.pay_bill_partial(bill.id, dec!(400)).await.unwrap();
        assert_eq!(bill.paid_amount, dec!(400));
        assert_ne!(bill.status, ApBillStatus::Paid); // not fully paid yet

        // Verify AP was reduced by $400
        let ap_balance = ledger.get_account_balance(processor.ap_account_id).await.unwrap();
        assert_eq!(ap_balance, dec!(600)); // 1000 - 400 = 600 remaining

        // Make another partial payment of $600 to complete
        let bill = processor.pay_bill_partial(bill.id, dec!(600)).await.unwrap();
        assert_eq!(bill.paid_amount, dec!(1000));
        assert_eq!(bill.status, ApBillStatus::Paid);

        // AP should be back to 0
        let ap_balance = ledger.get_account_balance(processor.ap_account_id).await.unwrap();
        assert_eq!(ap_balance, dec!(0));
    }

    #[tokio::test]
    async fn test_pay_bill_partial_exceeds_remaining() {
        let ledger = Arc::new(setup_ledger().await);
        let mut processor = setup_processor(ledger.clone()).await;
        let vendor = processor.add_vendor(Vendor::new("ExcessCo"));

        let expense_id = ledger.accounts.read().await
            .values().find(|a| a.number == "5000").unwrap().id;

        let bill = ApBill::new(vendor.id, "Excess test", dec!(500), expense_id,
            NaiveDate::from_ymd_opt(2026, 8, 1).unwrap());
        let bill = processor.enter_bill(bill).await.unwrap();

        // Try to pay more than the bill amount
        let result = processor.pay_bill_partial(bill.id, dec!(600)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_multi_line_bill() {
        let ledger = Arc::new(setup_ledger().await);
        let mut processor = setup_processor(ledger.clone()).await;
        let vendor = processor.add_vendor(Vendor::new("MultiLineCo"));

        let expense_5000 = ledger.accounts.read().await
            .values().find(|a| a.number == "5000").unwrap().id;
        let expense_5020 = ledger.accounts.read().await
            .values().find(|a| a.number == "5020").unwrap().id;

        // Create a multi-line bill: $300 COGS + $200 Rent = $500 total
        let lines = vec![
            ApBillLine::new(expense_5000, dec!(300), "Office supplies"),
            ApBillLine::new(expense_5020, dec!(200), "Rent"),
        ];
        let bill = ApBill::new_multi_line(
            vendor.id,
            "Multi-line bill",
            lines,
            NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(),
        );

        assert_eq!(bill.amount, dec!(500));
        assert_eq!(bill.lines.len(), 2);

        let bill = processor.enter_bill(bill).await.unwrap();
        assert_eq!(bill.status, ApBillStatus::Approved);
        assert!(bill.transaction_id.is_some());

        // Verify the transaction has 3 entries (2 debits + 1 credit)
        let txns = ledger.list_transactions().await.unwrap();
        let txn = txns.iter().find(|t| t.id == bill.transaction_id.unwrap()).unwrap();
        assert_eq!(txn.entries.len(), 3);

        // Verify AP liability = $500
        let ap_balance = ledger.get_account_balance(processor.ap_account_id).await.unwrap();
        assert_eq!(ap_balance, dec!(500));

        // Verify expense accounts were debited
        let cogs_balance = ledger.get_account_balance(expense_5000).await.unwrap();
        assert_eq!(cogs_balance, dec!(300));
        let rent_balance = ledger.get_account_balance(expense_5020).await.unwrap();
        assert_eq!(rent_balance, dec!(200));
    }
}
