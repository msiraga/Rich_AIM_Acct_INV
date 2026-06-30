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

// Re-export key types for convenience
pub use ledger::{Ledger, LedgerError, LedgerAgent};
pub use reconciliation::{ReconciliationProcessor, ReconciliationResult, ReconciliationError, ReconciliationAgent};
pub use tax::{TaxCalculator, TaxError, TaxAgent};
pub use payroll::{PayrollProcessor, PayrollError, PayrollAgent};
pub use invoice::{InvoiceProcessor, InvoiceError, InvoiceAgent};
pub use receipt::{ReceiptProcessor, ReceiptError, ReceiptAgent};
pub use reporting::{ReportingAgent, ReportingError};
