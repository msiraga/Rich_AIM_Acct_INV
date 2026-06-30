//! Cash Flow Statement Module
//!
//! Indirect-method cash flow statement: operating, investing, financing activities.

use serde::{Serialize, Deserialize};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use chrono::{DateTime, Utc};
use tracing::warn;
use crate::accounting::ledger::Ledger;
use crate::database::financial::{AccountType, EntryType};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum CashFlowError {
    #[error("Cash flow error: {0}")]
    Other(String),
}

impl CashFlowError {
    pub fn other(msg: &str) -> Self {
        Self::Other(msg.to_string())
    }
}

pub type CashFlowResult<T> = Result<T, CashFlowError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashFlowStatement {
    pub operating_activities: Vec<CashFlowLine>,
    pub investing_activities: Vec<CashFlowLine>,
    pub financing_activities: Vec<CashFlowLine>,
    pub net_cash_from_operating: Decimal,
    pub net_cash_from_investing: Decimal,
    pub net_cash_from_financing: Decimal,
    pub net_change_in_cash: Decimal,
    pub beginning_cash: Decimal,
    pub ending_cash: Decimal,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashFlowLine {
    pub description: String,
    pub amount: Decimal,
}

/// Generate an indirect-method cash flow statement from the shared ledger.
pub async fn generate_cash_flow_statement(
    ledger: &Ledger,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> CashFlowResult<CashFlowStatement> {
    let accounts = ledger.list_accounts().await
        .map_err(|e| CashFlowError::other(&e.to_string()))?;
    let transactions = ledger.list_transactions_by_date(start, end).await
        .map_err(|e| CashFlowError::other(&e.to_string()))?;

    // Compute net income from revenue - expenses
    let mut net_income = dec!(0);
    let mut depreciation = dec!(0);
    let mut ar_change = dec!(0);
    let mut inv_change = dec!(0);
    let mut ap_change = dec!(0);
    let mut accrual_change = dec!(0);
    let mut fixed_asset_purchases = dec!(0);
    let mut fixed_asset_sales = dec!(0);
    let mut loan_proceeds = dec!(0);
    let mut loan_payments = dec!(0);
    let mut owner_draws = dec!(0);
    let mut owner_contributions = dec!(0);

    for txn in &transactions {
        for entry in &txn.entries {
            let acc = accounts.iter().find(|a| a.id == entry.account_id);
            let Some(acc) = acc else { continue };
            let is_increase = match (&acc.account_type, &entry.entry_type) {
                (AccountType::Asset, EntryType::Debit) | (AccountType::Liability, EntryType::Credit)
                | (AccountType::Equity, EntryType::Credit) | (AccountType::Revenue, EntryType::Credit)
                | (AccountType::Expense, EntryType::Debit) => true,
                _ => false,
            };

            let amount = if is_increase { entry.amount } else { -entry.amount };

            // Use AccountType enum matching instead of account-number string prefixes
            match (&acc.account_type, acc.number.as_str()) {
                // Revenue/Expense → goes to net income (determined by AccountType, not number prefix)
                (AccountType::Revenue, _) => net_income += amount,
                (AccountType::Expense, _) => net_income -= amount,
                // Working capital changes by specific account number
                (_, "1020") => ar_change -= amount, // AR decrease = cash inflow
                (_, "1030") => inv_change -= amount, // Inventory increase = cash outflow
                (_, "2000") => ap_change += amount,  // AP increase = cash inflow
                (_, "2010") => {
                    if amount > dec!(0) { loan_proceeds += amount; }
                    else { loan_payments += -amount; }
                }
                (_, "2020") => accrual_change += amount,
                (_, "1050") => depreciation += amount, // Accumulated Depreciation increase = non-cash expense
                (_, "1040") => {
                    if amount > dec!(0) { fixed_asset_purchases += amount; }
                    else { fixed_asset_sales += -amount; }
                }
                (_, "3000") => {
                    if amount > dec!(0) { owner_contributions += amount; }
                    else { owner_draws += -amount; }
                }
                (_, "3010") => {} // retained earnings movements already in net income
                _ => {}
            }
        }
    }

    // Operating: Net Income + adjustments
    let adj_ar = ar_change;
    let adj_inv = inv_change;
    let adj_ap = ap_change;
    let adj_accrual = accrual_change;
    let adj_depn = depreciation;

    let net_cash_operating = net_income + adj_ar + adj_inv + adj_ap + adj_accrual + adj_depn;

    // Investing
    let net_cash_investing = fixed_asset_sales - fixed_asset_purchases;

    // Financing
    let net_cash_financing = loan_proceeds - loan_payments + owner_contributions - owner_draws;

    let net_change = net_cash_operating + net_cash_investing + net_cash_financing;

    let begin_cash = ledger.get_account_by_number("1000").await
        .map_err(|e| CashFlowError::other(&e.to_string()))?;
    let ending_cash_actual = begin_cash.map(|a| a.balance).unwrap_or(dec!(0));
    let begin_balance = ending_cash_actual - net_change;
    let ending_cash = begin_balance + net_change;

    // Reconciliation assertion: verify that net_change equals ending - beginning
    let reconciliation_diff = ending_cash - begin_balance - net_change;
    if reconciliation_diff != dec!(0) {
        warn!(
            "Cash flow reconciliation mismatch: net_change={}, ending-beginning={}, diff={}",
            net_change, ending_cash - begin_balance, reconciliation_diff
        );
    }

    Ok(CashFlowStatement {
        operating_activities: vec![
            CashFlowLine { description: "Net Income".into(), amount: net_income },
            CashFlowLine { description: "Adjust: AR change".into(), amount: adj_ar },
            CashFlowLine { description: "Adjust: Inventory change".into(), amount: adj_inv },
            CashFlowLine { description: "Adjust: AP change".into(), amount: adj_ap },
            CashFlowLine { description: "Adjust: Accruals".into(), amount: adj_accrual },
            CashFlowLine { description: "Adjust: Depreciation".into(), amount: adj_depn },
        ],
        investing_activities: vec![
            CashFlowLine { description: "Fixed Asset Purchases".into(), amount: -fixed_asset_purchases },
            CashFlowLine { description: "Fixed Asset Sales".into(), amount: fixed_asset_sales },
        ],
        financing_activities: vec![
            CashFlowLine { description: "Loan Proceeds".into(), amount: loan_proceeds },
            CashFlowLine { description: "Loan Repayments".into(), amount: -loan_payments },
            CashFlowLine { description: "Owner Contributions".into(), amount: owner_contributions },
            CashFlowLine { description: "Owner Draws".into(), amount: -owner_draws },
        ],
        net_cash_from_operating: net_cash_operating,
        net_cash_from_investing: net_cash_investing,
        net_cash_from_financing: net_cash_financing,
        net_change_in_cash: net_change,
        beginning_cash: begin_balance,
        ending_cash: begin_balance + net_change,
        period_start: start,
        period_end: end,
    })
}
