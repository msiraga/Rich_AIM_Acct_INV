//! Database Module
//!
//! This module contains all database-related functionality for the NexusLedger system.

pub mod models;
pub mod financial;
pub mod document;
pub mod error;
pub mod audit;
pub mod user;

pub use models::{BoundingBox, DocumentType, User, UserRole, Organization, Address, ContactInfo, AccountingPeriod, Document, AuditLog, AuditAction, Settings};
pub use financial::{Account, AccountType, AccountStatus, BalanceType, EntryType, BankAccountDetails, TransactionEntry, JournalEntry, Transaction, TransactionType, TransactionStatus, Reconciliation, ReconciliationStatus};
pub use error::{DatabaseError, DatabaseResult};
