//! Reporting Module
//!
//! Generates financial reports: Trial Balance, Balance Sheet, Income Statement.

use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;
use chrono::{DateTime, Utc, NaiveDate};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tracing::{info, debug};
use crate::database::financial::{Account, AccountType, EntryType};
use crate::agents::agent_types::{Agent, AgentConfig, AgentType, AgentStatus};
use crate::agents::task::{Task, TaskResult, TaskPayload, TaskType};
use crate::agents::error::AgentError;
use crate::accounting::cashflow;

/// Reporting error types
#[derive(Debug, thiserror::Error)]
pub enum ReportingError {
    #[error("Report generation error: {0}")]
    GenerationError(String),

    #[error("Missing data: {0}")]
    MissingData(String),

    #[error("Reporting error: {0}")]
    Other(String),
}

impl ReportingError {
    pub fn other(msg: &str) -> Self {
        Self::Other(msg.to_string())
    }
}

pub type ReportingResult<T> = Result<T, ReportingError>;

/// Report type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ReportType {
    TrialBalance,
    BalanceSheet,
    IncomeStatement,
    CashFlow,
}

impl Default for ReportType {
    fn default() -> Self {
        Self::TrialBalance
    }
}

/// Trial balance line item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrialBalanceLine {
    pub account_id: Uuid,
    pub account_number: String,
    pub account_name: String,
    pub account_type: AccountType,
    pub debit_balance: Decimal,
    pub credit_balance: Decimal,
}

/// Trial balance report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrialBalanceReport {
    pub lines: Vec<TrialBalanceLine>,
    pub total_debits: Decimal,
    pub total_credits: Decimal,
    pub is_balanced: bool,
    pub report_date: DateTime<Utc>,
}

/// Balance sheet report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceSheetReport {
    pub assets: Vec<ReportAccountLine>,
    pub liabilities: Vec<ReportAccountLine>,
    pub equity: Vec<ReportAccountLine>,
    pub total_assets: Decimal,
    pub total_liabilities: Decimal,
    pub total_equity: Decimal,
    pub total_liabilities_plus_equity: Decimal,
    pub is_balanced: bool,
    pub report_date: DateTime<Utc>,
}

/// Income statement report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomeStatementReport {
    pub revenue_lines: Vec<ReportAccountLine>,
    pub expense_lines: Vec<ReportAccountLine>,
    pub total_revenue: Decimal,
    pub total_expenses: Decimal,
    pub net_income: Decimal,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
}

/// A line item in a report referencing an account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportAccountLine {
    pub account_id: Uuid,
    pub account_number: String,
    pub account_name: String,
    pub balance: Decimal,
}

/// Reporting Agent for generating financial reports
#[derive(Debug, Clone)]
pub struct ReportingAgent {
    pub config: AgentConfig,
    pub status: AgentStatus,
    pub ledger: Option<Arc<crate::accounting::ledger::Ledger>>,
}

impl ReportingAgent {
    pub fn new(config: AgentConfig, ledger: Option<Arc<crate::accounting::ledger::Ledger>>) -> Self {
        Self {
            config,
            status: AgentStatus::Idle,
            ledger,
        }
    }

    pub fn with_defaults() -> Self {
        let config = AgentConfig::reporting_agent();
        Self::new(config, None)
    }

    /// Generate a trial balance report
    pub async fn generate_trial_balance(&self) -> ReportingResult<TrialBalanceReport> {
        let ledger = self
            .ledger
            .as_ref()
            .ok_or_else(|| ReportingError::MissingData("No ledger connected to ReportingAgent".to_string()))?;

        let accounts = ledger
            .list_accounts()
            .await
            .map_err(|e| ReportingError::GenerationError(e.to_string()))?;

        let mut lines = Vec::new();
        let mut total_debits = dec!(0);
        let mut total_credits = dec!(0);

        for account in &accounts {
            let (debit_balance, credit_balance) = if account.balance >= dec!(0) {
                if account.account_type.is_debit_normal() {
                    (account.balance, dec!(0))
                } else {
                    (dec!(0), account.balance)
                }
            } else {
                if account.account_type.is_debit_normal() {
                    (dec!(0), account.balance.abs())
                } else {
                    (account.balance.abs(), dec!(0))
                }
            };

            total_debits += debit_balance;
            total_credits += credit_balance;

            lines.push(TrialBalanceLine {
                account_id: account.id,
                account_number: account.number.clone(),
                account_name: account.name.clone(),
                account_type: account.account_type.clone(),
                debit_balance,
                credit_balance,
            });
        }

        Ok(TrialBalanceReport {
            lines,
            total_debits,
            total_credits,
            is_balanced: total_debits == total_credits,
            report_date: Utc::now(),
        })
    }

    /// Generate a balance sheet report
    pub async fn generate_balance_sheet(&self) -> ReportingResult<BalanceSheetReport> {
        let ledger = self
            .ledger
            .as_ref()
            .ok_or_else(|| ReportingError::MissingData("No ledger connected to ReportingAgent".to_string()))?;

        let accounts = ledger
            .list_accounts()
            .await
            .map_err(|e| ReportingError::GenerationError(e.to_string()))?;

        let mut assets = Vec::new();
        let mut liabilities = Vec::new();
        let mut equity = Vec::new();
        let mut total_assets = dec!(0);
        let mut total_liabilities = dec!(0);
        let mut total_equity = dec!(0);
        let mut total_revenue = dec!(0);
        let mut total_expenses = dec!(0);

        for account in &accounts {
            let line = ReportAccountLine {
                account_id: account.id,
                account_number: account.number.clone(),
                account_name: account.name.clone(),
                balance: account.balance,
            };

            match account.account_type {
                AccountType::Asset => {
                    total_assets += account.balance;
                    assets.push(line);
                }
                AccountType::Liability => {
                    total_liabilities += account.balance;
                    liabilities.push(line);
                }
                AccountType::Equity => {
                    total_equity += account.balance;
                    equity.push(line);
                }
                AccountType::Revenue => {
                    total_revenue += account.balance;
                }
                AccountType::Expense => {
                    total_expenses += account.balance;
                }
            }
        }

        // Compute net income and include as a computed equity line.
        // This ensures: Assets = Liabilities + Equity + Net Income
        let net_income = total_revenue - total_expenses;
        if net_income != dec!(0) {
            equity.push(ReportAccountLine {
                account_id: Uuid::nil(),
                account_number: "—".to_string(),
                account_name: "Current Period Net Income".to_string(),
                balance: net_income,
            });
            total_equity += net_income;
        }

        let total_liabilities_plus_equity = total_liabilities + total_equity;

        Ok(BalanceSheetReport {
            assets,
            liabilities,
            equity,
            total_assets,
            total_liabilities,
            total_equity,
            total_liabilities_plus_equity,
            is_balanced: total_assets == total_liabilities_plus_equity,
            report_date: Utc::now(),
        })
    }

    /// Generate an income statement report
    pub async fn generate_income_statement(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> ReportingResult<IncomeStatementReport> {
        let ledger = self
            .ledger
            .as_ref()
            .ok_or_else(|| ReportingError::MissingData("No ledger connected to ReportingAgent".to_string()))?;

        let accounts = ledger
            .list_accounts()
            .await
            .map_err(|e| ReportingError::GenerationError(e.to_string()))?;

        let transactions = ledger
            .list_transactions_by_date(start, end)
            .await
            .map_err(|e| ReportingError::GenerationError(e.to_string()))?;

        // Accumulate revenue and expense totals per account.
        // Respect entry_type: Debit increases expenses, Credit increases revenue;
        // Credit on expense = refund (decrease), Debit on revenue = reversal (decrease).
        let mut revenue_totals: HashMap<Uuid, Decimal> = HashMap::new();
        let mut expense_totals: HashMap<Uuid, Decimal> = HashMap::new();

        for txn in &transactions {
            for entry in &txn.entries {
                if let Some(account) = accounts.iter().find(|a| a.id == entry.account_id) {
                    let signed_amount = match entry.entry_type {
                        EntryType::Credit => entry.amount,  // Credit = increases revenue, decreases expense
                        EntryType::Debit => -entry.amount,   // Debit = decreases revenue, increases expense
                    };
                    match account.account_type {
                        AccountType::Revenue => {
                            *revenue_totals.entry(entry.account_id).or_default() += signed_amount;
                        }
                        AccountType::Expense => {
                            // Expenses work inversely: credits reduce expenses
                            *expense_totals.entry(entry.account_id).or_default() += -signed_amount;
                        }
                        _ => {}
                    }
                }
            }
        }

        let mut revenue_lines = Vec::new();
        let mut total_revenue = dec!(0);
        for account in accounts.iter().filter(|a| a.account_type == AccountType::Revenue) {
            let balance = revenue_totals.get(&account.id).copied().unwrap_or(dec!(0));
            total_revenue += balance;
            revenue_lines.push(ReportAccountLine {
                account_id: account.id,
                account_number: account.number.clone(),
                account_name: account.name.clone(),
                balance,
            });
        }

        let mut expense_lines = Vec::new();
        let mut total_expenses = dec!(0);
        for account in accounts.iter().filter(|a| a.account_type == AccountType::Expense) {
            let balance = expense_totals.get(&account.id).copied().unwrap_or(dec!(0));
            total_expenses += balance;
            expense_lines.push(ReportAccountLine {
                account_id: account.id,
                account_number: account.number.clone(),
                account_name: account.name.clone(),
                balance,
            });
        }

        Ok(IncomeStatementReport {
            revenue_lines,
            expense_lines,
            total_revenue,
            total_expenses,
            net_income: total_revenue - total_expenses,
            period_start: start,
            period_end: end,
        })
    }

    /// Generate an AR aging report — outstanding invoices grouped by age.
    ///
    /// `as_of_date` determines the cutoff for aging calculations. Pass `None`
    /// to default to `Utc::now()`.
    ///
    /// Payment matching uses transaction metadata:
    /// - `invoice_number` field matching the invoice's number, OR
    /// - `ap_payment` flag with an `invoice_id` field matching the invoice's ID
    ///
    /// Partial payments are supported: all matching payment amounts are summed,
    /// and only the remaining balance is shown. Invoices with `remaining <= 0`
    /// are excluded.
    pub async fn generate_ar_aging(
        &self,
        as_of_date: Option<DateTime<Utc>>,
    ) -> ReportingResult<ArAgingReport> {
        let ledger = self
            .ledger
            .as_ref()
            .ok_or_else(|| ReportingError::MissingData("No ledger connected".to_string()))?;

        let transactions = ledger
            .list_transactions()
            .await
            .map_err(|e| ReportingError::GenerationError(e.to_string()))?;

        let as_of = as_of_date.unwrap_or_else(Utc::now);
        let mut buckets = ArAgingBuckets::default();

        for txn in &transactions {
            if txn.transaction_type != crate::database::financial::TransactionType::Invoice {
                continue;
            }

            let invoice_amount = txn.total_amount();

            // Sum all payment amounts that reference this invoice via metadata
            let mut total_payments = dec!(0);
            for t in &transactions {
                if t.transaction_type != crate::database::financial::TransactionType::Payment {
                    continue;
                }

                // Check if this payment references the invoice via metadata:
                // 1. invoice_number field matching the invoice's number
                // 2. ap_payment flag with invoice_id matching the invoice's ID
                let matches = t
                    .metadata
                    .get("invoice_number")
                    .and_then(|v| v.as_str())
                    .map(|s| s == txn.number)
                    .unwrap_or(false)
                    || (t
                        .metadata
                        .get("ap_payment")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                        && t
                            .metadata
                            .get("invoice_id")
                            .and_then(|v| v.as_str())
                            .map(|s| s == txn.id.to_string())
                            .unwrap_or(false));

                if matches {
                    total_payments += t.total_amount();
                }
            }

            // Compute remaining balance (supports partial payments)
            let remaining = invoice_amount - total_payments;
            if remaining <= dec!(0) {
                continue; // fully paid or overpaid — skip
            }

            // Use due_date if available, otherwise fall back to invoice date
            let due_date = txn
                .metadata
                .get("due_date")
                .and_then(|v| v.as_str())
                .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
                .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc());

            // Age from the earlier of invoice date or due date
            let reference_date = due_date.unwrap_or(txn.date);
            let days_outstanding = (as_of - reference_date).num_days().max(0);

            let bucket = buckets.get_mut(days_outstanding);
            bucket.amount += remaining;
            bucket.count += 1;
            bucket.invoices.push(ArAgingInvoice {
                invoice_id: txn.id,
                invoice_number: txn.number.clone(),
                customer: txn
                    .metadata
                    .get("customer_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown")
                    .to_string(),
                amount: remaining,
                days_outstanding,
                due_date,
            });
        }

        Ok(buckets.into_report())
    }
}

/// AR aging report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArAgingReport {
    pub current: ArAgingBucket,
    pub days_31_60: ArAgingBucket,
    pub days_61_90: ArAgingBucket,
    pub days_90_plus: ArAgingBucket,
    pub total_outstanding: Decimal,
}

/// A single aging bucket.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArAgingBucket {
    pub amount: Decimal,
    pub count: u64,
    pub invoices: Vec<ArAgingInvoice>,
}

/// An outstanding invoice in the aging report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArAgingInvoice {
    pub invoice_id: Uuid,
    pub invoice_number: String,
    pub customer: String,
    pub amount: Decimal,
    pub days_outstanding: i64,
    pub due_date: Option<DateTime<Utc>>,
}

#[derive(Default)]
struct ArAgingBuckets {
    current: ArAgingBucket,
    days_31_60: ArAgingBucket,
    days_61_90: ArAgingBucket,
    days_90_plus: ArAgingBucket,
}

impl ArAgingBuckets {
    fn get_mut(&mut self, days: i64) -> &mut ArAgingBucket {
        match days {
            0..=30 => &mut self.current,
            31..=60 => &mut self.days_31_60,
            61..=90 => &mut self.days_61_90,
            _ => &mut self.days_90_plus,
        }
    }

    fn into_report(self) -> ArAgingReport {
        let total = self.current.amount
            + self.days_31_60.amount
            + self.days_61_90.amount
            + self.days_90_plus.amount;

        ArAgingReport {
            current: self.current,
            days_31_60: self.days_31_60,
            days_61_90: self.days_61_90,
            days_90_plus: self.days_90_plus,
            total_outstanding: total,
        }
    }
}

#[async_trait]
impl Agent for ReportingAgent {
    fn config(&self) -> &AgentConfig {
        &self.config
    }

    fn status(&self) -> AgentStatus {
        self.status.clone()
    }

    async fn initialize(&mut self) -> Result<(), anyhow::Error> {
        self.status = AgentStatus::Initializing;
        info!("Initializing Reporting Agent...");
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
            TaskType::GenerateReport => self.process_generate_report(task).await,
            _ => Err(AgentError::TaskProcessingFailed(format!(
                "ReportingAgent cannot handle task type: {:?}",
                task.task_type
            ))
            .into()),
        }
    }

    fn agent_type(&self) -> AgentType {
        self.config.agent_type.clone()
    }
}

impl ReportingAgent {
    async fn process_generate_report(&self, task: Task) -> Result<Task, anyhow::Error> {
        let start_time = std::time::Instant::now();

        // Determine report type from payload
        let report_type = match &task.payload {
            TaskPayload::Json(v) => {
                let rt = v
                    .get("report_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("trial_balance");
                match rt {
                    "trial_balance" | "TrialBalance" => ReportType::TrialBalance,
                    "balance_sheet" | "BalanceSheet" => ReportType::BalanceSheet,
                    "income_statement" | "IncomeStatement" => ReportType::IncomeStatement,
                    "cash_flow" | "CashFlow" | "cashflow" => ReportType::CashFlow,
                    _ => ReportType::TrialBalance,
                }
            }
            _ => ReportType::TrialBalance,
        };

        let result_data = match report_type {
            ReportType::TrialBalance => {
                let report = self.generate_trial_balance().await.map_err(|e| {
                    AgentError::TaskProcessingFailed(e.to_string())
                })?;
                TaskResult::success_with_data(
                    &format!(
                        "Trial Balance generated: debits={}, credits={}, balanced={}",
                        report.total_debits, report.total_credits, report.is_balanced
                    ),
                    TaskPayload::Json(serde_json::to_value(&report).unwrap_or_default()),
                )
            }
            ReportType::BalanceSheet => {
                let report = self.generate_balance_sheet().await.map_err(|e| {
                    AgentError::TaskProcessingFailed(e.to_string())
                })?;
                TaskResult::success_with_data(
                    &format!(
                        "Balance Sheet generated: assets={}, liabilities+equity={}, balanced={}",
                        report.total_assets, report.total_liabilities_plus_equity, report.is_balanced
                    ),
                    TaskPayload::Json(serde_json::to_value(&report).unwrap_or_default()),
                )
            }
            ReportType::IncomeStatement => {
                // Extract date range from payload or use defaults
                let (start, end) = match &task.payload {
                    TaskPayload::Json(v) => {
                        let start_str = v.get("start_date").and_then(|v| v.as_str()).unwrap_or("");
                        let end_str = v.get("end_date").and_then(|v| v.as_str()).unwrap_or("");
                        let start = if !start_str.is_empty() {
                            NaiveDate::parse_from_str(start_str, "%Y-%m-%d")
                                .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc())
                                .unwrap_or_else(|_| Utc::now() - chrono::Duration::days(365))
                        } else {
                            Utc::now() - chrono::Duration::days(365)
                        };
                        let end = if !end_str.is_empty() {
                            NaiveDate::parse_from_str(end_str, "%Y-%m-%d")
                                .map(|d| d.and_hms_opt(23, 59, 59).unwrap().and_utc())
                                .unwrap_or_else(|_| Utc::now())
                        } else {
                            Utc::now()
                        };
                        (start, end)
                    }
                    _ => (
                        Utc::now() - chrono::Duration::days(365),
                        Utc::now(),
                    ),
                };

                let report = self.generate_income_statement(start, end).await.map_err(|e| {
                    AgentError::TaskProcessingFailed(e.to_string())
                })?;
                TaskResult::success_with_data(
                    &format!(
                        "Income Statement generated: revenue={}, expenses={}, net_income={}",
                        report.total_revenue, report.total_expenses, report.net_income
                    ),
                    TaskPayload::Json(serde_json::to_value(&report).unwrap_or_default()),
                )
            }
            ReportType::CashFlow => {
                let (start, end) = match &task.payload {
                    TaskPayload::Json(v) => {
                        let start_str = v.get("start_date").and_then(|v| v.as_str()).unwrap_or("");
                        let end_str = v.get("end_date").and_then(|v| v.as_str()).unwrap_or("");
                        let start = if !start_str.is_empty() {
                            NaiveDate::parse_from_str(start_str, "%Y-%m-%d")
                                .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc())
                                .unwrap_or_else(|_| Utc::now() - chrono::Duration::days(365))
                        } else {
                            Utc::now() - chrono::Duration::days(365)
                        };
                        let end = if !end_str.is_empty() {
                            NaiveDate::parse_from_str(end_str, "%Y-%m-%d")
                                .map(|d| d.and_hms_opt(23, 59, 59).unwrap().and_utc())
                                .unwrap_or_else(|_| Utc::now())
                        } else {
                            Utc::now()
                        };
                        (start, end)
                    }
                    _ => (Utc::now() - chrono::Duration::days(365), Utc::now()),
                };

                let this_ledger = self.ledger.as_ref().ok_or_else(|| {
                    AgentError::TaskProcessingFailed("No ledger connected to ReportingAgent".to_string())
                })?;

                let cf = cashflow::generate_cash_flow_statement(this_ledger, start, end).await
                    .map_err(|e| AgentError::TaskProcessingFailed(e.to_string()))?;

                TaskResult::success_with_data(
                    &format!(
                        "Cash Flow: op={}, inv={}, fin={}, net={}",
                        cf.net_cash_from_operating, cf.net_cash_from_investing,
                        cf.net_cash_from_financing, cf.net_change_in_cash
                    ),
                    TaskPayload::Json(serde_json::to_value(&cf).unwrap_or_default()),
                )
            }
            _ => {
                return Err(AgentError::TaskProcessingFailed(format!(
                    "Unsupported report type: {:?}",
                    report_type
                ))
                .into());
            }
        };

        let _processing_time = start_time.elapsed().as_millis() as f64;

        Ok(task.complete(result_data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounting::ledger::Ledger;
    use crate::database::financial::TransactionEntry;

    async fn setup_ledger_with_data() -> Ledger {
        let mut ledger = Ledger::new();
        ledger.initialize().await.unwrap();

        // Record a test transaction: cash received for revenue
        let cash = ledger.get_account_by_number("1000").await.unwrap().unwrap();
        let revenue = ledger.get_account_by_number("4000").await.unwrap().unwrap();

        let entries = vec![
            TransactionEntry::new(cash.id, EntryType::Debit, dec!(5000), "Cash received"),
            TransactionEntry::new(revenue.id, EntryType::Credit, dec!(5000), "Revenue earned"),
        ];

        let txn = Transaction::new("Test sale".to_string(), Utc::now(), entries);
        ledger.record_transaction(txn).await.unwrap();

        // Record an expense
        let cash = ledger.get_account_by_number("1000").await.unwrap().unwrap();
        let expense = ledger.get_account_by_number("5020").await.unwrap().unwrap();

        let entries = vec![
            TransactionEntry::new(expense.id, EntryType::Debit, dec!(1500), "Rent payment"),
            TransactionEntry::new(cash.id, EntryType::Credit, dec!(1500), "Rent paid"),
        ];

        let txn = Transaction::new("Rent expense".to_string(), Utc::now(), entries);
        ledger.record_transaction(txn).await.unwrap();

        ledger
    }

    #[tokio::test]
    async fn test_trial_balance() {
        let ledger = setup_ledger_with_data().await;
        let agent = ReportingAgent::new(
            AgentConfig::reporting_agent(),
            Some(Arc::new(ledger)),
        );

        let report = agent.generate_trial_balance().await.unwrap();
        assert!(!report.lines.is_empty());
        assert_eq!(report.total_debits, report.total_credits);
        assert!(report.is_balanced);
    }

    #[tokio::test]
    async fn test_balance_sheet() {
        let ledger = setup_ledger_with_data().await;
        let agent = ReportingAgent::new(
            AgentConfig::reporting_agent(),
            Some(Arc::new(ledger)),
        );

        let report = agent.generate_balance_sheet().await.unwrap();
        assert_eq!(report.total_assets, dec!(3500));
    }

    #[tokio::test]
    async fn test_income_statement() {
        let ledger = setup_ledger_with_data().await;
        let agent = ReportingAgent::new(
            AgentConfig::reporting_agent(),
            Some(Arc::new(ledger)),
        );

        let start = Utc::now() - chrono::Duration::days(365);
        let end = Utc::now() + chrono::Duration::days(1);
        let report = agent.generate_income_statement(start, end).await.unwrap();
        assert_eq!(report.total_revenue, dec!(5000));
        assert_eq!(report.total_expenses, dec!(1500));
        assert_eq!(report.net_income, dec!(3500));
    }

    #[tokio::test]
    async fn test_reporting_agent_task() {
        let ledger = setup_ledger_with_data().await;
        let agent = ReportingAgent::new(
            AgentConfig::reporting_agent(),
            Some(Arc::new(ledger)),
        );

        let payload = serde_json::json!({
            "report_type": "trial_balance"
        });

        let task = Task {
            task_type: TaskType::GenerateReport,
            payload: TaskPayload::Json(payload),
            ..Default::default()
        };

        let result = agent.process_task(task).await;
        assert!(result.is_ok());
    }

    use crate::database::financial::Transaction;
}
