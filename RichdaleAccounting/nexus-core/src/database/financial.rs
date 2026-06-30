//! Financial Database Module
//!
//! Defines financial data models and operations for the NexusLedger system.

use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc, NaiveDate};
use uuid::Uuid;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Account types for the chart of accounts
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AccountType {
    /// Asset accounts (debit increases, credit decreases)
    Asset,
    /// Liability accounts (credit increases, debit decreases)
    Liability,
    /// Equity accounts (credit increases, debit decreases)
    Equity,
    /// Revenue accounts (credit increases, debit decreases)
    Revenue,
    /// Expense accounts (debit increases, credit decreases)
    Expense,
}

impl Default for AccountType {
    fn default() -> Self {
        Self::Asset
    }
}

impl AccountType {
    /// Get the normal balance type for this account type
    pub fn normal_balance(&self) -> BalanceType {
        match self {
            Self::Asset | Self::Expense => BalanceType::Debit,
            Self::Liability | Self::Equity | Self::Revenue => BalanceType::Credit,
        }
    }

    /// Check if this account type normally has a debit balance
    pub fn is_debit_normal(&self) -> bool {
        matches!(self, Self::Asset | Self::Expense)
    }

    /// Check if this account type normally has a credit balance
    pub fn is_credit_normal(&self) -> bool {
        matches!(self, Self::Liability | Self::Equity | Self::Revenue)
    }
}

/// Balance types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum BalanceType {
    /// Debit balance
    Debit,
    /// Credit balance
    Credit,
}

impl Default for BalanceType {
    fn default() -> Self {
        Self::Debit
    }
}

/// Entry types for journal entries
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EntryType {
    /// Debit entry
    Debit,
    /// Credit entry
    Credit,
}

impl Default for EntryType {
    fn default() -> Self {
        Self::Debit
    }
}

impl EntryType {
    /// Convert from BalanceType
    pub fn from_balance_type(balance_type: BalanceType) -> Self {
        match balance_type {
            BalanceType::Debit => Self::Debit,
            BalanceType::Credit => Self::Credit,
        }
    }

    /// Convert to BalanceType
    pub fn to_balance_type(&self) -> BalanceType {
        match self {
            Self::Debit => BalanceType::Debit,
            Self::Credit => BalanceType::Credit,
        }
    }
}

/// Account status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AccountStatus {
    /// Account is active
    Active,
    /// Account is inactive
    Inactive,
    /// Account is frozen
    Frozen,
    /// Account is closed
    Closed,
}

impl Default for AccountStatus {
    fn default() -> Self {
        Self::Active
    }
}

/// Account model representing a chart of accounts entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    /// Unique identifier
    pub id: Uuid,
    /// Account number/code
    pub number: String,
    /// Account name
    pub name: String,
    /// Account description
    pub description: String,
    /// Account type
    pub account_type: AccountType,
    /// Parent account ID (for hierarchical accounts)
    pub parent_id: Option<Uuid>,
    /// Account status
    pub status: AccountStatus,
    /// Current balance
    pub balance: Decimal,
    /// Currency
    pub currency: String,
    /// Whether the account is a bank account
    pub is_bank_account: bool,
    /// Bank account details (if applicable)
    pub bank_details: Option<BankAccountDetails>,
    /// Whether the account is reconciled
    pub is_reconciled: bool,
    /// Last reconciliation date
    pub last_reconciled: Option<DateTime<Utc>>,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    /// Updated timestamp
    pub updated_at: DateTime<Utc>,
}

impl Default for Account {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            number: String::new(),
            name: String::new(),
            description: String::new(),
            account_type: AccountType::default(),
            parent_id: None,
            status: AccountStatus::default(),
            balance: dec!(0),
            currency: "USD".to_string(),
            is_bank_account: false,
            bank_details: None,
            is_reconciled: false,
            last_reconciled: None,
            created_at: now,
            updated_at: now,
        }
    }
}

impl Account {
    /// Create a new account
    pub fn new(number: &str, name: &str, account_type: AccountType) -> Self {
        Self {
            number: number.to_string(),
            name: name.to_string(),
            account_type,
            ..Default::default()
        }
    }

    /// Create a bank account
    pub fn new_bank_account(number: &str, name: &str, bank_details: BankAccountDetails) -> Self {
        Self {
            number: number.to_string(),
            name: name.to_string(),
            account_type: AccountType::Asset,
            is_bank_account: true,
            bank_details: Some(bank_details),
            ..Default::default()
        }
    }

    /// Check if the account is active
    pub fn is_active(&self) -> bool {
        self.status == AccountStatus::Active
    }

    /// Get the normal balance type for this account
    pub fn normal_balance(&self) -> BalanceType {
        self.account_type.normal_balance()
    }

    /// Update the account balance
    pub fn update_balance(&mut self, amount: Decimal, entry_type: EntryType) {
        let multiplier = match (self.normal_balance(), entry_type) {
            (BalanceType::Debit, EntryType::Debit) => dec!(1),
            (BalanceType::Debit, EntryType::Credit) => dec!(-1),
            (BalanceType::Credit, EntryType::Debit) => dec!(-1),
            (BalanceType::Credit, EntryType::Credit) => dec!(1),
        };
        
        self.balance += multiplier * amount;
        self.updated_at = Utc::now();
    }

    /// Get the current balance as a string
    pub fn balance_string(&self) -> String {
        self.balance.to_string()
    }
}

/// Bank account details
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BankAccountDetails {
    /// Bank name
    pub bank_name: String,
    /// Bank account number
    pub account_number: String,
    /// Bank routing number
    pub routing_number: String,
    /// Bank address
    pub address: String,
    /// Bank phone number
    pub phone: String,
    /// Bank website
    pub website: String,
    /// Account holder name
    pub account_holder: String,
    /// Account type (checking, savings, etc.)
    pub account_type: String,
    /// Currency
    pub currency: String,
}

/// Transaction entry within a transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionEntry {
    /// Unique identifier
    pub id: Uuid,
    /// Account ID this entry affects
    pub account_id: Uuid,
    /// Entry type (Debit or Credit)
    pub entry_type: EntryType,
    /// Amount (in the entry's transaction currency)
    pub amount: Decimal,
    /// Description/memo
    pub description: String,
    /// Reference number
    pub reference: Option<String>,
    /// Currency code (ISO 4217, e.g. "USD", "EUR")
    #[serde(default = "default_currency")]
    pub currency: String,
    /// Exchange rate to base currency (1 unit of `currency` = how many base units).
    /// `None` when the entry is already in base currency.
    #[serde(default)]
    pub exchange_rate: Option<Decimal>,
    /// Amount converted to the base currency. `None` when the entry is already
    /// in base currency (equals `amount`).
    #[serde(default)]
    pub base_currency_amount: Option<Decimal>,
}

fn default_currency() -> String {
    "USD".to_string()
}

impl Default for TransactionEntry {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            account_id: Uuid::new_v4(),
            entry_type: EntryType::default(),
            amount: dec!(0),
            description: String::new(),
            reference: None,
            currency: "USD".to_string(),
            exchange_rate: None,
            base_currency_amount: None,
        }
    }
}

impl TransactionEntry {
    /// Create a new transaction entry
    pub fn new(account_id: Uuid, entry_type: EntryType, amount: Decimal, description: &str) -> Self {
        Self {
            account_id,
            entry_type,
            amount,
            description: description.to_string(),
            ..Default::default()
        }
    }

    /// Check if this is a debit entry
    pub fn is_debit(&self) -> bool {
        self.entry_type == EntryType::Debit
    }

    /// Check if this is a credit entry
    pub fn is_credit(&self) -> bool {
        self.entry_type == EntryType::Credit
    }
}

/// Journal entry for recording financial transactions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    /// Unique identifier
    pub id: Uuid,
    /// Journal entry number
    pub number: String,
    /// Date of the journal entry
    pub date: NaiveDate,
    /// Description/memo
    pub description: String,
    /// Reference number
    pub reference: Option<String>,
    /// List of transaction entries (must balance to zero)
    pub entries: Vec<TransactionEntry>,
    /// Whether the journal entry is posted
    pub is_posted: bool,
    /// Posted timestamp
    pub posted_at: Option<DateTime<Utc>>,
    /// Whether the journal entry is reconciled
    pub is_reconciled: bool,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    /// Updated timestamp
    pub updated_at: DateTime<Utc>,
}

impl Default for JournalEntry {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            number: String::new(),
            date: now.date_naive(),
            description: String::new(),
            reference: None,
            entries: Vec::new(),
            is_posted: false,
            posted_at: None,
            is_reconciled: false,
            created_at: now,
            updated_at: now,
        }
    }
}

impl JournalEntry {
    /// Create a new journal entry
    pub fn new(description: &str, date: NaiveDate) -> Self {
        Self {
            description: description.to_string(),
            date,
            ..Default::default()
        }
    }

    /// Add an entry to the journal entry
    pub fn add_entry(&mut self, entry: TransactionEntry) {
        self.entries.push(entry);
    }

    /// Check if the journal entry is balanced (sum of debits equals sum of credits)
    pub fn is_balanced(&self) -> bool {
        let mut total_debits = dec!(0);
        let mut total_credits = dec!(0);
        
        for entry in &self.entries {
            match entry.entry_type {
                EntryType::Debit => total_debits += entry.amount,
                EntryType::Credit => total_credits += entry.amount,
            }
        }
        
        total_debits == total_credits
    }

    /// Get the total debit amount
    pub fn total_debits(&self) -> Decimal {
        self.entries.iter()
            .filter(|e| e.entry_type == EntryType::Debit)
            .map(|e| e.amount)
            .sum()
    }

    /// Get the total credit amount
    pub fn total_credits(&self) -> Decimal {
        self.entries.iter()
            .filter(|e| e.entry_type == EntryType::Credit)
            .map(|e| e.amount)
            .sum()
    }

    /// Post the journal entry
    pub fn post(&mut self) -> Result<(), String> {
        if !self.is_balanced() {
            return Err("Journal entry is not balanced".to_string());
        }
        
        self.is_posted = true;
        self.posted_at = Some(Utc::now());
        
        Ok(())
    }

    /// Unpost the journal entry
    pub fn unpost(&mut self) {
        self.is_posted = false;
        self.posted_at = None;
    }
}

/// Transaction model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    /// Unique identifier
    pub id: Uuid,
    /// Transaction number/reference
    pub number: String,
    /// Description
    pub description: String,
    /// Date of the transaction
    pub date: DateTime<Utc>,
    /// Transaction type
    pub transaction_type: TransactionType,
    /// Status of the transaction
    pub status: TransactionStatus,
    /// List of transaction entries (must balance to zero)
    pub entries: Vec<TransactionEntry>,
    /// Related journal entry ID
    pub journal_entry_id: Option<Uuid>,
    /// Related document IDs
    pub document_ids: Vec<String>,
    /// Metadata
    pub metadata: serde_json::Value,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    /// Updated timestamp
    pub updated_at: DateTime<Utc>,
}

impl Default for Transaction {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            number: String::new(),
            description: String::new(),
            date: now,
            transaction_type: TransactionType::default(),
            status: TransactionStatus::default(),
            entries: Vec::new(),
            journal_entry_id: None,
            document_ids: Vec::new(),
            metadata: serde_json::json!({}),
            created_at: now,
            updated_at: now,
        }
    }
}

impl Transaction {
    /// Create a new transaction
    pub fn new(description: String, date: DateTime<Utc>, entries: Vec<TransactionEntry>) -> Self {
        Self {
            description,
            date,
            entries,
            ..Default::default()
        }
    }

    /// Check if the transaction is balanced
    pub fn is_balanced(&self) -> bool {
        let mut total_debits = dec!(0);
        let mut total_credits = dec!(0);
        
        for entry in &self.entries {
            match entry.entry_type {
                EntryType::Debit => total_debits += entry.amount,
                EntryType::Credit => total_credits += entry.amount,
            }
        }
        
        total_debits == total_credits
    }

    /// Get the total amount of the transaction (sum of debit entries).
    ///
    /// For a balanced double-entry transaction, debit total equals credit total,
    /// so this returns the transaction's monetary value without doubling it.
    pub fn total_amount(&self) -> Decimal {
        self.entries.iter()
            .filter(|e| e.entry_type == EntryType::Debit)
            .map(|e| e.amount)
            .sum()
    }

    /// Add a document ID to the transaction
    pub fn add_document_id(&mut self, document_id: &str) {
        self.document_ids.push(document_id.to_string());
    }

    /// Get the account IDs involved in this transaction
    pub fn account_ids(&self) -> Vec<Uuid> {
        self.entries.iter()
            .map(|e| e.account_id)
            .collect()
    }
}

/// Transaction types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum TransactionType {
    /// Invoice
    Invoice,
    /// Payment
    Payment,
    /// Expense
    Expense,
    /// Transfer
    Transfer,
    /// Journal entry
    JournalEntry,
    /// Adjustment
    Adjustment,
    /// Reconciliation
    Reconciliation,
    /// Other
    Other,
}

impl Default for TransactionType {
    fn default() -> Self {
        Self::Other
    }
}

/// Transaction status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum TransactionStatus {
    /// Draft
    Draft,
    /// Pending
    Pending,
    /// Posted
    Posted,
    /// Reconciled
    Reconciled,
    /// Voided
    Voided,
    /// Error
    Error,
}

impl Default for TransactionStatus {
    fn default() -> Self {
        Self::Draft
    }
}

/// Reconciliation model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reconciliation {
    /// Unique identifier
    pub id: Uuid,
    /// Account ID being reconciled
    pub account_id: Uuid,
    /// Statement date
    pub statement_date: NaiveDate,
    /// Starting balance
    pub starting_balance: Decimal,
    /// Ending balance
    pub ending_balance: Decimal,
    /// Statement ending balance
    pub statement_ending_balance: Decimal,
    /// List of reconciled transaction IDs
    pub reconciled_transactions: Vec<Uuid>,
    /// List of outstanding transactions
    pub outstanding_transactions: Vec<Uuid>,
    /// Difference amount
    pub difference: Decimal,
    /// Status
    pub status: ReconciliationStatus,
    /// Notes
    pub notes: String,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    /// Updated timestamp
    pub updated_at: DateTime<Utc>,
}

impl Default for Reconciliation {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            account_id: Uuid::new_v4(),
            statement_date: now.date_naive(),
            starting_balance: dec!(0),
            ending_balance: dec!(0),
            statement_ending_balance: dec!(0),
            reconciled_transactions: Vec::new(),
            outstanding_transactions: Vec::new(),
            difference: dec!(0),
            status: ReconciliationStatus::default(),
            notes: String::new(),
            created_at: now,
            updated_at: now,
        }
    }
}

/// Reconciliation status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ReconciliationStatus {
    /// In progress
    InProgress,
    /// Completed
    Completed,
    /// Needs review
    NeedsReview,
    /// Cancelled
    Cancelled,
}

impl Default for ReconciliationStatus {
    fn default() -> Self {
        Self::InProgress
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_types() {
        assert!(AccountType::Asset.is_debit_normal());
        assert!(AccountType::Liability.is_credit_normal());
        assert!(AccountType::Expense.is_debit_normal());
        assert!(AccountType::Revenue.is_credit_normal());
        assert!(AccountType::Equity.is_credit_normal());
    }

    #[test]
    fn test_entry_types() {
        assert_eq!(EntryType::Debit.to_balance_type(), BalanceType::Debit);
        assert_eq!(EntryType::Credit.to_balance_type(), BalanceType::Credit);
        assert_eq!(EntryType::from_balance_type(BalanceType::Debit), EntryType::Debit);
    }

    #[test]
    fn test_account_balance_update() {
        let mut account = Account::new("1000", "Cash", AccountType::Asset);
        
        // Debit increases asset balance
        account.update_balance(dec!(100), EntryType::Debit);
        assert_eq!(account.balance, dec!(100));
        
        // Credit decreases asset balance
        account.update_balance(dec!(50), EntryType::Credit);
        assert_eq!(account.balance, dec!(50));
    }

    #[test]
    fn test_transaction_entry() {
        let entry = TransactionEntry::new(
            Uuid::new_v4(),
            EntryType::Debit,
            dec!(100),
            "Test entry"
        );
        
        assert!(entry.is_debit());
        assert!(!entry.is_credit());
    }

    #[test]
    fn test_journal_entry_balance() {
        let mut journal = JournalEntry::new("Test journal", Utc::now().date_naive());
        
        // Add unbalanced entries
        journal.add_entry(TransactionEntry::new(
            Uuid::new_v4(),
            EntryType::Debit,
            dec!(100),
            "Debit entry"
        ));
        
        assert!(!journal.is_balanced());
        
        // Add balancing credit entry
        journal.add_entry(TransactionEntry::new(
            Uuid::new_v4(),
            EntryType::Credit,
            dec!(100),
            "Credit entry"
        ));
        
        assert!(journal.is_balanced());
        assert_eq!(journal.total_debits(), dec!(100));
        assert_eq!(journal.total_credits(), dec!(100));
    }

    #[test]
    fn test_transaction_balance() {
        let entries = vec![
            TransactionEntry::new(Uuid::new_v4(), EntryType::Debit, dec!(100), "Debit"),
            TransactionEntry::new(Uuid::new_v4(), EntryType::Credit, dec!(100), "Credit"),
        ];
        
        let transaction = Transaction::new("Test transaction".to_string(), Utc::now(), entries);
        assert!(transaction.is_balanced());
        assert_eq!(transaction.total_amount(), dec!(100));
    }

    #[test]
    fn test_journal_entry_posting() {
        let mut journal = JournalEntry::new("Test journal", Utc::now().date_naive());
        
        // Add balanced entries
        journal.add_entry(TransactionEntry::new(
            Uuid::new_v4(),
            EntryType::Debit,
            dec!(100),
            "Debit entry"
        ));
        journal.add_entry(TransactionEntry::new(
            Uuid::new_v4(),
            EntryType::Credit,
            dec!(100),
            "Credit entry"
        ));
        
        // Should be able to post
        assert!(journal.post().is_ok());
        assert!(journal.is_posted);
        assert!(journal.posted_at.is_some());
        
        // Unpost
        journal.unpost();
        assert!(!journal.is_posted);
        assert!(journal.posted_at.is_none());
    }
}
