//! Accounting Module
//!
//! This module contains all accounting-related functionality for the NexusLedger system.
//!
//! # Submodules
//! - `ledger`: Ledger operations and double-entry accounting
//! - `reconciliation`: Bank reconciliation functionality
//! - `tax`: Tax calculation and processing
//! - `payroll`: Payroll processing
//! - `invoice`: Invoice creation and payment tracking
//! - `receipt`: Receipt processing and expense categorization
//! - `reporting`: Financial report generation (Trial Balance, Balance Sheet, Income Statement)

pub mod ledger;
pub mod reconciliation;
pub mod tax;
pub mod payroll;
pub mod invoice;
pub mod receipt;
pub mod reporting;
pub mod cashflow;
pub mod ap;
pub mod budget;
pub mod assets;

// ── Multi-Currency Support ────────────────────────────────────────────────

use rust_decimal::Decimal;
use std::collections::HashMap;
use crate::database::financial::TransactionEntry;

/// Exchange rates relative to base currency (USD).
#[derive(Debug, Clone, Default)]
pub struct ExchangeRates {
    pub base_currency: String,
    pub rates: HashMap<String, Decimal>,
}

impl ExchangeRates {
    pub fn new(base_currency: &str) -> Self {
        Self { base_currency: base_currency.to_string(), rates: HashMap::new() }
    }

    /// Set the exchange rate (1 unit of `currency` = how many base units).
    pub fn set_rate(&mut self, currency: &str, rate: Decimal) {
        self.rates.insert(currency.to_uppercase(), rate);
    }

    /// Convert an amount from a foreign currency to base currency.
    pub fn convert_to_base(&self, amount: Decimal, from_currency: &str) -> Option<Decimal> {
        if from_currency.eq_ignore_ascii_case(&self.base_currency) {
            return Some(amount);
        }
        let rate = self.rates.get(&from_currency.to_uppercase())?;
        Some(amount * rate)
    }

    /// Convert from base currency to a foreign currency.
    pub fn convert_from_base(&self, amount: Decimal, to_currency: &str) -> Option<Decimal> {
        if to_currency.eq_ignore_ascii_case(&self.base_currency) {
            return Some(amount);
        }
        let rate = self.rates.get(&to_currency.to_uppercase())?;
        if rate.is_zero() { return None; }
        Some(amount / rate)
    }

    /// Get the exchange rate for a currency.
    pub fn get_rate(&self, currency: &str) -> Option<Decimal> {
        self.rates.get(&currency.to_uppercase()).copied()
    }

    /// Convert a `TransactionEntry` to base currency, returning a new entry
    /// with `base_currency_amount`, `exchange_rate`, and `currency` populated.
    ///
    /// If the entry is already in the base currency, it is returned unchanged
    /// (with `exchange_rate` and `base_currency_amount` left as `None`).
    /// Returns `None` if no rate is available for the entry's currency.
    pub fn convert_entry_to_base(&self, entry: &TransactionEntry) -> Option<TransactionEntry> {
        if entry.currency.eq_ignore_ascii_case(&self.base_currency) {
            // Already in base currency
            return Some(TransactionEntry {
                base_currency_amount: Some(entry.amount),
                ..entry.clone()
            });
        }

        let rate = self.rates.get(&entry.currency.to_uppercase())?;
        let base_amount = entry.amount * *rate;

        Some(TransactionEntry {
            currency: entry.currency.clone(),
            exchange_rate: Some(*rate),
            base_currency_amount: Some(base_amount),
            ..entry.clone()
        })
    }
}

// Re-export key types for convenience
pub use ledger::{Ledger, LedgerError, LedgerAgent};
pub use reconciliation::{ReconciliationProcessor, ReconciliationResult, ReconciliationError, ReconciliationAgent};
pub use tax::{TaxCalculator, TaxError, TaxAgent};
pub use payroll::{PayrollProcessor, PayrollError, PayrollAgent};
pub use invoice::{InvoiceProcessor, InvoiceError, InvoiceAgent};
pub use receipt::{ReceiptProcessor, ReceiptError, ReceiptAgent};
pub use reporting::{ReportingAgent, ReportingError};
