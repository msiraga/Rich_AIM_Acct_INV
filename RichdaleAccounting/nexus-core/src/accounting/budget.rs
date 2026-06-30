//! Budget Module
//!
//! Budgets per account per period, with budget-vs-actual variance reporting.

use serde::{Serialize, Deserialize};
use uuid::Uuid;
use chrono::{DateTime, Utc, NaiveDate};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;

use crate::accounting::ledger::Ledger;
use crate::database::financial::{AccountType, EntryType};

/// Budget error.
#[derive(Debug, thiserror::Error)]
pub enum BudgetError {
    #[error("Budget not found: {0}")]
    NotFound(String),
    #[error("Invalid period: {0}")]
    InvalidPeriod(String),
    #[error("Ledger error: {0}")]
    LedgerError(String),
    #[error("Budget error: {0}")]
    Other(String),
}

pub type BudgetResult<T> = Result<T, BudgetError>;

/// Budget period type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BudgetPeriod {
    Monthly,
    Quarterly,
    Annual,
}

/// A budget for a specific account in a specific period.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Budget {
    pub id: Uuid,
    pub account_id: Uuid,
    pub name: String,
    pub period: BudgetPeriod,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub budgeted_amount: Decimal,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Budget {
    pub fn new(
        account_id: Uuid,
        name: &str,
        period: BudgetPeriod,
        period_start: NaiveDate,
        period_end: NaiveDate,
        budgeted_amount: Decimal,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            account_id,
            name: name.to_string(),
            period,
            period_start,
            period_end,
            budgeted_amount,
            notes: None,
            created_at: now,
            updated_at: now,
        }
    }
}

/// A single line in a budget-vs-actual report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetVarianceLine {
    pub account_id: Uuid,
    pub account_number: String,
    pub account_name: String,
    pub budgeted: Decimal,
    pub actual: Decimal,
    pub variance: Decimal,
    pub variance_percent: Decimal,
}

/// Budget-vs-actual report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetVarianceReport {
    pub report_name: String,
    pub lines: Vec<BudgetVarianceLine>,
    pub total_budgeted: Decimal,
    pub total_actual: Decimal,
    pub total_variance: Decimal,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
}

/// Budget manager — stores budgets and computes variances.
#[derive(Debug, Default)]
pub struct BudgetManager {
    pub budgets: Vec<Budget>,
}

impl BudgetManager {
    pub fn new() -> Self {
        Self { budgets: Vec::new() }
    }

    /// Add a budget entry.
    pub fn add_budget(&mut self, budget: Budget) -> &Budget {
        self.budgets.push(budget);
        self.budgets.last().unwrap()
    }

    /// Build a budget-vs-actual variance report for a given period.
    pub async fn generate_variance_report(
        &self,
        ledger: &Ledger,
        period_start: NaiveDate,
        period_end: NaiveDate,
    ) -> BudgetResult<BudgetVarianceReport> {
        let start_utc = period_start.and_hms_opt(0, 0, 0).unwrap().and_utc();
        let end_utc = period_end.and_hms_opt(23, 59, 59).unwrap().and_utc();

        let accounts = ledger.list_accounts().await
            .map_err(|e| BudgetError::LedgerError(e.to_string()))?;
        let transactions = ledger.list_transactions_by_date(start_utc, end_utc).await
            .map_err(|e| BudgetError::LedgerError(e.to_string()))?;

        // Compute actual account balances for the period, applying debit/credit
        // sign based on account type:
        //   Expense: debit entries increase actual (positive = over budget)
        //   Revenue: credit entries increase actual (positive = on/above target)
        let mut actuals: HashMap<Uuid, Decimal> = HashMap::new();
        for txn in &transactions {
            for entry in &txn.entries {
                // Find the account to determine its type
                let acc = accounts.iter().find(|a| a.id == entry.account_id);
                let signed_amount = match acc {
                    Some(acc) => match (&acc.account_type, &entry.entry_type) {
                        // Expense: debits increase (over budget), credits decrease (refunds)
                        (AccountType::Expense, EntryType::Debit) => entry.amount,
                        (AccountType::Expense, EntryType::Credit) => -entry.amount,
                        // Revenue: credits increase (on target), debits decrease (reversals)
                        (AccountType::Revenue, EntryType::Credit) => entry.amount,
                        (AccountType::Revenue, EntryType::Debit) => -entry.amount,
                        // Other account types: use raw amount
                        _ => entry.amount,
                    },
                    None => entry.amount,
                };
                *actuals.entry(entry.account_id).or_default() += signed_amount;
            }
        }

        let mut lines = Vec::new();
        let mut total_budgeted = dec!(0);
        let mut total_actual = dec!(0);

        for budget in &self.budgets {
            // Use overlap logic instead of containment:
            // Two ranges [a1, a2] and [b1, b2] overlap if a1 <= b2 && b1 <= a2
            let overlaps = budget.period_start <= period_end && period_start <= budget.period_end;
            if !overlaps {
                continue;
            }

            let actual = actuals.get(&budget.account_id).copied().unwrap_or(dec!(0));
            let variance = actual - budget.budgeted_amount;
            let variance_percent = if budget.budgeted_amount != dec!(0) {
                (variance / budget.budgeted_amount) * dec!(100)
            } else {
                dec!(0)
            };

            let account = accounts.iter().find(|a| a.id == budget.account_id);

            lines.push(BudgetVarianceLine {
                account_id: budget.account_id,
                account_number: account.map(|a| a.number.clone()).unwrap_or_default(),
                account_name: budget.name.clone(),
                budgeted: budget.budgeted_amount,
                actual,
                variance,
                variance_percent,
            });

            total_budgeted += budget.budgeted_amount;
            total_actual += actual;
        }

        let total_variance = total_actual - total_budgeted;

        Ok(BudgetVarianceReport {
            report_name: "Budget vs Actual".into(),
            lines,
            total_budgeted,
            total_actual,
            total_variance,
            period_start,
            period_end,
        })
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounting::ledger::Ledger;
    use crate::database::financial::{EntryType, TransactionEntry, TransactionType, TransactionStatus, Transaction};

    #[test]
    fn test_budget_creation() {
        let account_id = Uuid::new_v4();
        let budget = Budget::new(
            account_id,
            "Office Supplies Budget",
            BudgetPeriod::Monthly,
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 1, 31).unwrap(),
            dec!(500),
        );
        assert_eq!(budget.name, "Office Supplies Budget");
        assert_eq!(budget.budgeted_amount, dec!(500));
    }

    #[test]
    fn test_add_budget() {
        let mut mgr = BudgetManager::new();
        let budget = Budget::new(Uuid::new_v4(), "Test", BudgetPeriod::Monthly,
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 1, 31).unwrap(),
            dec!(1000));
        mgr.add_budget(budget);
        assert_eq!(mgr.budgets.len(), 1);
    }

    #[tokio::test]
    async fn test_variance_report() {
        let mut ledger = Ledger::new();
        ledger.initialize().await.unwrap();

        let expense_id = ledger.accounts.read().await
            .values().find(|a| a.number == "5000").unwrap().id;
        let cash_id = ledger.accounts.read().await
            .values().find(|a| a.number == "1000").unwrap().id;

        let mut mgr = BudgetManager::new();
        mgr.add_budget(Budget::new(
            expense_id,
            "Office Supplies",
            BudgetPeriod::Monthly,
            NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
            dec!(500),
        ));

        // Record actual expense: $300 (under budget)
        let now = Utc::now();
        let txn = Transaction {
            id: Uuid::new_v4(),
            number: "TXN-1".into(),
            description: "Test expense".into(),
            date: NaiveDate::from_ymd_opt(2026, 6, 15).unwrap().and_hms_opt(12, 0, 0).unwrap().and_utc(),
            transaction_type: TransactionType::JournalEntry,
            status: TransactionStatus::Pending,
            entries: vec![
                TransactionEntry {
                    id: Uuid::new_v4(),
                    account_id: expense_id,
                    amount: dec!(300),
                    entry_type: EntryType::Debit,
                    description: "Office supplies".into(),
                    reference: None,
                    ..Default::default()
                },
                TransactionEntry {
                    id: Uuid::new_v4(),
                    account_id: cash_id,
                    amount: dec!(300),
                    entry_type: EntryType::Credit,
                    description: "Cash".into(),
                    reference: None,
                    ..Default::default()
                },
            ],
            journal_entry_id: None,
            document_ids: vec![],
            metadata: serde_json::json!({}),
            created_at: now,
            updated_at: now,
        };
        let _ = ledger.record_transaction(txn).await;

        let report = mgr.generate_variance_report(
            &ledger,
            NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
        ).await.unwrap();

        assert_eq!(report.lines.len(), 1);
        assert_eq!(report.lines[0].budgeted, dec!(500));
        assert_eq!(report.lines[0].actual, dec!(300));
        assert_eq!(report.lines[0].variance, dec!(-200)); // under budget = negative
        assert_eq!(report.total_variance, dec!(-200));
    }
}
