//! Accounting Module
//!
//! This module contains all accounting-related functionality for the NexusLedger system.
//! 
//! # Submodules
//! - `ledger`: Ledger operations and double-entry accounting
//! - `reconciliation`: Bank reconciliation functionality
//! - `tax`: Tax calculation and processing
//! - `payroll`: Payroll processing

pub mod ledger;
pub mod reconciliation;
pub mod tax;
pub mod payroll;

// Re-export key types for convenience
pub use ledger::{Ledger, LedgerError, LedgerAgent};
pub use reconciliation::{ReconciliationProcessor, ReconciliationResult, ReconciliationError, ReconciliationAgent};
pub use tax::{TaxCalculator, TaxError, TaxAgent};
pub use payroll::{PayrollProcessor, PayrollError, PayrollAgent};
