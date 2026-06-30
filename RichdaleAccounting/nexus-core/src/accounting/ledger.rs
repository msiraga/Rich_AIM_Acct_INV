//! Ledger Module
//!
//! Handles ledger operations and double-entry accounting.

use async_trait::async_trait;
use std::collections::{HashMap, BTreeMap};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;
use chrono::{DateTime, Utc, NaiveDate};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tracing::{info, error, debug, warn};
use crate::database::financial::{Account, AccountType, AccountStatus, EntryType, Transaction, TransactionEntry, TransactionType, TransactionStatus, JournalEntry};
use crate::database::error::DatabaseError;
use crate::agents::agent_types::{Agent, AgentConfig, AgentType, AgentStatus};
use crate::agents::task::{Task, TaskResult, TaskPayload};
use crate::agents::error::AgentError;

/// Ledger error types
#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    /// Account not found
    #[error("Account not found: {0}")]
    AccountNotFound(String),
    
    /// Transaction not balanced
    #[error("Transaction is not balanced")]
    UnbalancedTransaction,
    
    /// Invalid account type for operation
    #[error("Invalid account type for operation: {0}")]
    InvalidAccountType(String),
    
    /// Insufficient funds
    #[error("Insufficient funds in account: {0}")]
    InsufficientFunds(String),
    
    /// Account is inactive
    #[error("Account is inactive: {0}")]
    AccountInactive(String),
    
    /// Journal entry error
    #[error("Journal entry error: {0}")]
    JournalEntryError(String),
    
    /// Database error
    #[error("Database error: {0}")]
    DatabaseError(#[from] DatabaseError),
    
    /// Any other ledger error
    #[error("Ledger error: {0}")]
    Other(String),
}

impl LedgerError {
    /// Create a new other error
    pub fn other(msg: &str) -> Self {
        Self::Other(msg.to_string())
    }
}

impl From<String> for LedgerError {
    fn from(s: String) -> Self {
        LedgerError::Other(s)
    }
}

/// Result type for ledger operations
pub type LedgerResult<T> = Result<T, LedgerError>;

/// Ledger struct that manages the chart of accounts and transactions
#[derive(Debug, Clone)]
pub struct Ledger {
    /// Map of account ID to account
    pub accounts: Arc<RwLock<BTreeMap<Uuid, Account>>>,
    /// Map of transaction ID to transaction
    pub transactions: Arc<RwLock<BTreeMap<Uuid, Transaction>>>,
    /// Map of journal entry ID to journal entry
    pub journal_entries: Arc<RwLock<BTreeMap<Uuid, JournalEntry>>>,
    /// Current journal entry number
    pub current_journal_number: Arc<Mutex<u64>>,
    /// Current transaction number
    pub current_transaction_number: Arc<Mutex<u64>>,
    /// Optional database connection for SurrealDB persistence
    pub db: Option<Arc<crate::database::Database>>,
}

impl Default for Ledger {
    fn default() -> Self {
        Self::new()
    }
}

impl Ledger {
    /// Create a new ledger
    pub fn new() -> Self {
        Self {
            accounts: Arc::new(RwLock::new(BTreeMap::new())),
            transactions: Arc::new(RwLock::new(BTreeMap::new())),
            journal_entries: Arc::new(RwLock::new(BTreeMap::new())),
            current_journal_number: Arc::new(Mutex::new(1)),
            current_transaction_number: Arc::new(Mutex::new(1)),
            db: None,
        }
    }

    /// Initialize the ledger with default accounts.
    /// Idempotent — skips if accounts already exist (e.g. shared via Arc).
    pub async fn initialize(&mut self) -> LedgerResult<()> {
        info!("Initializing ledger with default accounts...");

        // Skip if accounts already exist (e.g. from a shared Arc)
        if !self.accounts.read().await.is_empty() {
            info!("Ledger already has {} accounts, skipping initialization", self.accounts.read().await.len());
            return Ok(());
        }

        // Create default chart of accounts
        self.create_default_accounts().await?;

        Ok(())
    }

    /// Create default chart of accounts
    async fn create_default_accounts(&mut self) -> LedgerResult<()> {
        // Asset accounts
        self.create_account(Account::new("1000", "Cash", AccountType::Asset)).await?;
        self.create_account(Account::new("1010", "Bank Account", AccountType::Asset)).await?;
        self.create_account(Account::new("1020", "Accounts Receivable", AccountType::Asset)).await?;
        self.create_account(Account::new("1030", "Inventory", AccountType::Asset)).await?;
        self.create_account(Account::new("1040", "Fixed Assets", AccountType::Asset)).await?;
        self.create_account(Account::new("1050", "Accumulated Depreciation", AccountType::Asset)).await?;
        
        // Liability accounts
        self.create_account(Account::new("2000", "Accounts Payable", AccountType::Liability)).await?;
        self.create_account(Account::new("2010", "Loans Payable", AccountType::Liability)).await?;
        self.create_account(Account::new("2020", "Accrued Expenses", AccountType::Liability)).await?;
        
        // Equity accounts
        self.create_account(Account::new("3000", "Owner's Equity", AccountType::Equity)).await?;
        self.create_account(Account::new("3010", "Retained Earnings", AccountType::Equity)).await?;
        
        // Revenue accounts
        self.create_account(Account::new("4000", "Sales Revenue", AccountType::Revenue)).await?;
        self.create_account(Account::new("4010", "Service Revenue", AccountType::Revenue)).await?;
        self.create_account(Account::new("4020", "Interest Revenue", AccountType::Revenue)).await?;
        
        // Expense accounts
        self.create_account(Account::new("5000", "Cost of Goods Sold", AccountType::Expense)).await?;
        self.create_account(Account::new("5010", "Salaries Expense", AccountType::Expense)).await?;
        self.create_account(Account::new("5020", "Rent Expense", AccountType::Expense)).await?;
        self.create_account(Account::new("5030", "Utilities Expense", AccountType::Expense)).await?;
        self.create_account(Account::new("5040", "Office Supplies Expense", AccountType::Expense)).await?;
        self.create_account(Account::new("5050", "Depreciation Expense", AccountType::Expense)).await?;
        
        info!("Created {} default accounts", self.accounts.read().await.len());
        
        Ok(())
    }

    /// Create a new account
    pub async fn create_account(&mut self, mut account: Account) -> LedgerResult<Account> {
        debug!("Creating account: {} ({})", account.name, account.number);
        
        // Check if account number already exists
        let accounts = self.accounts.read().await;
        if accounts.values().any(|a| a.number == account.number) {
            return Err(LedgerError::other(&format!("Account number {} already exists", account.number)));
        }
        
        drop(accounts);
        
        // Set creation timestamp
        account.created_at = Utc::now();
        account.updated_at = Utc::now();

        // Insert the account
        self.accounts.write().await.insert(account.id, account.clone());

        // Persist to SurrealDB if configured (additive, non-blocking on failure)
        if let Some(ref db) = self.db {
            if let Ok(client) = db.db().await {
                let account_type_str = format!("{:?}", account.account_type);
                let status_str = format!("{:?}", account.status);
                let parent_id_str = account.parent_id.map(|id| id.to_string());
                if let Err(e) = client.query(
                    "CREATE account SET \
                     id = $id, number = $number, name = $name, \
                     description = $description, account_type = $account_type, \
                     parent_id = $parent_id, status = $status, \
                     balance = $balance, currency = $currency, \
                     is_bank_account = $is_bank_account, \
                     is_reconciled = $is_reconciled, \
                     created_at = $created_at, updated_at = $updated_at"
                )
                .bind(("id", account.id.to_string()))
                .bind(("number", account.number.clone()))
                .bind(("name", account.name.clone()))
                .bind(("description", account.description.clone()))
                .bind(("account_type", account_type_str))
                .bind(("parent_id", parent_id_str))
                .bind(("status", status_str))
                .bind(("balance", account.balance.to_string()))
                .bind(("currency", account.currency.clone()))
                .bind(("is_bank_account", account.is_bank_account))
                .bind(("is_reconciled", account.is_reconciled))
                .bind(("created_at", account.created_at.to_rfc3339()))
                .bind(("updated_at", account.updated_at.to_rfc3339()))
                .await
                {
                    warn!("Failed to persist account {} to SurrealDB: {}", account.number, e);
                }
            }
        }

        Ok(account)
    }

    /// Get an account by ID
    pub async fn get_account(&self, id: Uuid) -> LedgerResult<Option<Account>> {
        let accounts = self.accounts.read().await;
        Ok(accounts.get(&id).cloned())
    }

    /// Get an account by number
    pub async fn get_account_by_number(&self, number: &str) -> LedgerResult<Option<Account>> {
        let accounts = self.accounts.read().await;
        Ok(accounts.values().find(|a| a.number == number).cloned())
    }

    /// List all accounts
    pub async fn list_accounts(&self) -> LedgerResult<Vec<Account>> {
        let accounts = self.accounts.read().await;
        Ok(accounts.values().cloned().collect())
    }

    /// List accounts by type
    pub async fn list_accounts_by_type(&self, account_type: AccountType) -> LedgerResult<Vec<Account>> {
        let accounts = self.accounts.read().await;
        Ok(accounts.values()
            .filter(|a| a.account_type == account_type)
            .cloned()
            .collect())
    }

    /// Update an account
    pub async fn update_account(&mut self, id: Uuid, mut account: Account) -> LedgerResult<Account> {
        debug!("Updating account: {}", id);
        
        if account.id != id {
            return Err(LedgerError::other("Account ID mismatch"));
        }
        
        // Check if account exists
        let mut accounts = self.accounts.write().await;
        if !accounts.contains_key(&id) {
            return Err(LedgerError::AccountNotFound(id.to_string()));
        }
        
        // Update timestamp
        account.updated_at = Utc::now();
        
        // Update the account
        accounts.insert(id, account.clone());
        
        Ok(account)
    }

    /// Delete an account
    pub async fn delete_account(&mut self, id: Uuid) -> LedgerResult<bool> {
        debug!("Deleting account: {}", id);
        
        let mut accounts = self.accounts.write().await;
        Ok(accounts.remove(&id).is_some())
    }

    /// Record a transaction
    pub async fn record_transaction(&self, mut transaction: Transaction) -> LedgerResult<Transaction> {
        info!("Recording transaction: {}", transaction.description);
        
        // Validate the transaction
        self.validate_transaction(&transaction).await?;
        
        // Generate transaction number
        let mut counter = self.current_transaction_number.lock().await;
        transaction.number = format!("TRX-{:08}", *counter);
        *counter += 1;
        
        // Set timestamps
        transaction.created_at = Utc::now();
        transaction.updated_at = Utc::now();
        transaction.status = TransactionStatus::Posted;
        
        // Update account balances
        self.update_account_balances(&transaction).await?;
        
        // Store the transaction
        self.transactions.write().await.insert(transaction.id, transaction.clone());
        
        // Create a journal entry
        let journal_entry = self.create_journal_entry(&transaction).await?;
        transaction.journal_entry_id = Some(journal_entry.id);
        
        // Update the transaction with journal entry ID
        self.transactions.write().await.insert(transaction.id, transaction.clone());

        // Persist to SurrealDB if configured (additive, non-blocking on failure)
        if let Some(ref db) = self.db {
            if let Ok(client) = db.db().await {
                // Persist the transaction
                let txn_type_str = format!("{:?}", transaction.transaction_type);
                let txn_status_str = format!("{:?}", transaction.status);
                let journal_id_str = transaction.journal_entry_id.map(|id| id.to_string());
                if let Err(e) = client.query(
                    "CREATE transaction SET \
                     id = $id, number = $number, description = $description, \
                     date = $date, transaction_type = $transaction_type, \
                     status = $status, journal_entry_id = $journal_entry_id, \
                     created_at = $created_at, updated_at = $updated_at"
                )
                .bind(("id", transaction.id.to_string()))
                .bind(("number", transaction.number.clone()))
                .bind(("description", transaction.description.clone()))
                .bind(("date", transaction.date.to_rfc3339()))
                .bind(("transaction_type", txn_type_str))
                .bind(("status", txn_status_str))
                .bind(("journal_entry_id", journal_id_str))
                .bind(("created_at", transaction.created_at.to_rfc3339()))
                .bind(("updated_at", transaction.updated_at.to_rfc3339()))
                .await
                {
                    warn!("Failed to persist transaction {} to SurrealDB: {}", transaction.number, e);
                }

                // Persist individual transaction entries
                for entry in &transaction.entries {
                    let entry_type_str = format!("{:?}", entry.entry_type);
                    let reference_str = entry.reference.clone();
                    if let Err(e) = client.query(
                        "CREATE transaction_entry SET \
                         id = $id, transaction_id = $transaction_id, \
                         account_id = $account_id, entry_type = $entry_type, \
                         amount = $amount, description = $description, \
                         reference = $reference"
                    )
                    .bind(("id", entry.id.to_string()))
                    .bind(("transaction_id", transaction.id.to_string()))
                    .bind(("account_id", entry.account_id.to_string()))
                    .bind(("entry_type", entry_type_str))
                    .bind(("amount", entry.amount.to_string()))
                    .bind(("description", entry.description.clone()))
                    .bind(("reference", reference_str))
                    .await
                    {
                        warn!("Failed to persist transaction entry to SurrealDB: {}", e);
                    }
                }

                // Persist the journal entry
                let journal_ref_str = journal_entry.reference.clone();
                if let Err(e) = client.query(
                    "CREATE journal_entry SET \
                     id = $id, number = $number, description = $description, \
                     reference = $reference, date = $date, \
                     is_posted = $is_posted, is_reconciled = $is_reconciled, \
                     created_at = $created_at, updated_at = $updated_at"
                )
                .bind(("id", journal_entry.id.to_string()))
                .bind(("number", journal_entry.number.clone()))
                .bind(("description", journal_entry.description.clone()))
                .bind(("reference", journal_ref_str))
                .bind(("date", journal_entry.date.to_string()))
                .bind(("is_posted", journal_entry.is_posted))
                .bind(("is_reconciled", journal_entry.is_reconciled))
                .bind(("created_at", journal_entry.created_at.to_rfc3339()))
                .bind(("updated_at", journal_entry.updated_at.to_rfc3339()))
                .await
                {
                    warn!("Failed to persist journal entry {} to SurrealDB: {}", journal_entry.number, e);
                }

                // Update account balances in SurrealDB
                for entry in &transaction.entries {
                    let entry_type_str = format!("{:?}", entry.entry_type);
                    if let Err(e) = client.query(
                        "UPDATE account SET \
                         balance = $balance, updated_at = $updated_at \
                         WHERE id = $account_id"
                    )
                    .bind(("account_id", entry.account_id.to_string()))
                    .bind(("updated_at", Utc::now().to_rfc3339()))
                    .bind(("balance", {
                        // Read the current in-memory balance (already updated above)
                        let accounts = self.accounts.read().await;
                        accounts.get(&entry.account_id)
                            .map(|a| a.balance.to_string())
                            .unwrap_or_else(|| entry.amount.to_string())
                    }))
                    .await
                    {
                        warn!("Failed to update account balance in SurrealDB for entry type {}: {}", entry_type_str, e);
                    }
                }
            }
        }

        Ok(transaction)
    }

    /// Validate a transaction
    async fn validate_transaction(&self, transaction: &Transaction) -> LedgerResult<()> {
        // Check if transaction is balanced
        if !transaction.is_balanced() {
            return Err(LedgerError::UnbalancedTransaction);
        }
        
        // Check if all accounts exist and are active
        for entry in &transaction.entries {
            let account = self.get_account(entry.account_id).await?;
            match account {
                Some(acc) => {
                    if !acc.is_active() {
                        return Err(LedgerError::AccountInactive(acc.number));
                    }
                }
                None => return Err(LedgerError::AccountNotFound(entry.account_id.to_string())),
            }
        }
        
        Ok(())
    }

    /// Update account balances based on transaction entries
    async fn update_account_balances(&self, transaction: &Transaction) -> LedgerResult<()> {
        let mut accounts = self.accounts.write().await;
        
        for entry in &transaction.entries {
            if let Some(account) = accounts.get_mut(&entry.account_id) {
                account.update_balance(entry.amount, entry.entry_type.clone());
            }
        }
        
        Ok(())
    }

    /// Create a journal entry from a transaction
    async fn create_journal_entry(&self, transaction: &Transaction) -> LedgerResult<JournalEntry> {
        let mut counter = self.current_journal_number.lock().await;
        let journal_number = format!("JE-{:08}", *counter);
        *counter += 1;
        
        let mut journal_entry = JournalEntry::new(
            &format!("Journal entry for transaction {}", transaction.number),
            transaction.date.date_naive()
        );
        
        journal_entry.number = journal_number;
        journal_entry.entries = transaction.entries.clone();
        journal_entry.post()?;
        journal_entry.created_at = Utc::now();
        journal_entry.updated_at = Utc::now();
        
        // Store the journal entry
        self.journal_entries.write().await.insert(journal_entry.id, journal_entry.clone());
        
        Ok(journal_entry)
    }

    /// Get a transaction by ID
    pub async fn get_transaction(&self, id: Uuid) -> LedgerResult<Option<Transaction>> {
        let transactions = self.transactions.read().await;
        Ok(transactions.get(&id).cloned())
    }

    /// List all transactions
    pub async fn list_transactions(&self) -> LedgerResult<Vec<Transaction>> {
        let transactions = self.transactions.read().await;
        Ok(transactions.values().cloned().collect())
    }

    /// List transactions by date range
    pub async fn list_transactions_by_date(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> LedgerResult<Vec<Transaction>> {
        let transactions = self.transactions.read().await;
        Ok(transactions.values()
            .filter(|t| t.date >= start && t.date <= end)
            .cloned()
            .collect())
    }

    /// Get account balance
    pub async fn get_account_balance(&self, account_id: Uuid) -> LedgerResult<Decimal> {
        let accounts = self.accounts.read().await;
        match accounts.get(&account_id) {
            Some(account) => Ok(account.balance),
            None => Err(LedgerError::AccountNotFound(account_id.to_string())),
        }
    }

    /// Get trial balance
    pub async fn get_trial_balance(&self) -> LedgerResult<HashMap<Uuid, Decimal>> {
        let accounts = self.accounts.read().await;
        Ok(accounts.iter().map(|(id, acc)| (*id, acc.balance)).collect())
    }

    /// Get balance sheet
    pub async fn get_balance_sheet(&self) -> LedgerResult<BalanceSheet> {
        let accounts = self.accounts.read().await;
        
        let mut assets = dec!(0);
        let mut liabilities = dec!(0);
        let mut equity = dec!(0);
        
        for account in accounts.values() {
            match account.account_type {
                AccountType::Asset => assets += account.balance,
                AccountType::Liability => liabilities += account.balance,
                AccountType::Equity => equity += account.balance,
                _ => {}
            }
        }
        
        Ok(BalanceSheet {
            assets,
            liabilities,
            equity,
            total_assets: assets,
            total_liabilities_plus_equity: liabilities + equity,
        })
    }

    /// Get income statement
    pub async fn get_income_statement(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> LedgerResult<IncomeStatement> {
        let transactions = self.transactions.read().await;
        
        let mut revenue = dec!(0);
        let mut expenses = dec!(0);
        
        for transaction in transactions.values() {
            if transaction.date >= start && transaction.date <= end {
                for entry in &transaction.entries {
                    let account = self.get_account(entry.account_id).await?;
                    if let Some(acc) = account {
                        match acc.account_type {
                            AccountType::Revenue => revenue += entry.amount,
                            AccountType::Expense => expenses += entry.amount,
                            _ => {}
                        }
                    }
                }
            }
        }
        
        Ok(IncomeStatement {
            revenue,
            expenses,
            net_income: revenue - expenses,
        })
    }
}

/// Balance sheet data
#[derive(Debug, Clone)]
pub struct BalanceSheet {
    /// Total assets
    pub assets: Decimal,
    /// Total liabilities
    pub liabilities: Decimal,
    /// Total equity
    pub equity: Decimal,
    /// Total assets (should equal liabilities + equity)
    pub total_assets: Decimal,
    /// Total liabilities plus equity
    pub total_liabilities_plus_equity: Decimal,
}

/// Income statement data
#[derive(Debug, Clone)]
pub struct IncomeStatement {
    /// Total revenue
    pub revenue: Decimal,
    /// Total expenses
    pub expenses: Decimal,
    /// Net income (revenue - expenses)
    pub net_income: Decimal,
}

/// Ledger Agent for handling ledger-related tasks
#[derive(Debug, Clone)]
pub struct LedgerAgent {
    /// Agent configuration
    pub config: AgentConfig,
    /// Current status
    pub status: AgentStatus,
    /// Ledger instance
    pub ledger: Ledger,
}

impl LedgerAgent {
    /// Create a new ledger agent
    pub fn new(config: AgentConfig, ledger: Ledger) -> Self {
        Self {
            config,
            status: AgentStatus::Idle,
            ledger,
        }
    }

    /// Create a ledger agent with default configuration
    pub fn with_defaults() -> Self {
        let config = AgentConfig::ledger_agent();
        let ledger = Ledger::new();
        Self::new(config, ledger)
    }
}

#[async_trait]
impl Agent for LedgerAgent {
    fn config(&self) -> &AgentConfig {
        &self.config
    }

    fn status(&self) -> AgentStatus {
        self.status.clone()
    }

    async fn initialize(&mut self) -> Result<(), anyhow::Error> {
        self.status = AgentStatus::Initializing;
        self.ledger.initialize().await?;
        self.status = AgentStatus::Idle;
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), anyhow::Error> {
        self.status = AgentStatus::ShuttingDown;
        // Clean up resources
        self.status = AgentStatus::Idle;
        Ok(())
    }

    async fn process_task(&self, task: Task) -> Result<Task, anyhow::Error> {
        if !self.config.enabled {
            return Err(AgentError::AgentDisabled(self.config.name.clone()).into());
        }

        match task.task_type {
            crate::agents::task::TaskType::RecordTransaction => {
                self.process_record_transaction(task).await
            }
            _ => {
                Err(AgentError::TaskProcessingFailed(
                    format!("LedgerAgent cannot handle task type: {:?}", task.task_type)
                ).into())
            }
        }
    }

    fn agent_type(&self) -> AgentType {
        self.config.agent_type.clone()
    }
}

impl LedgerAgent {
    /// Process a record transaction task
    async fn process_record_transaction(&self, task: Task) -> Result<Task, anyhow::Error> {
        // Status tracking deferred - requires interior mutability

        let start_time = std::time::Instant::now();

        // Extract transaction from task payload
        let transaction = match &task.payload {
            TaskPayload::Transaction(txn) => txn.clone(),
            _ => return Err(AgentError::TaskProcessingFailed(
                "Expected Transaction payload for RecordTransaction task".to_string()
            ).into()),
        };

        // Record the transaction in the ledger
        let recorded_transaction = self.ledger.record_transaction(transaction).await?;

        // Create success result
        let result = TaskResult::success_with_data(
            "Transaction recorded successfully",
            TaskPayload::Transaction(recorded_transaction)
        );

        let processing_time = start_time.elapsed().as_millis() as f64;

        // Status tracking deferred - requires interior mutability

        Ok(task.complete(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn test_ledger_creation() {
        let ledger = Ledger::new();
        assert!(ledger.accounts.read().await.is_empty());
        assert!(ledger.transactions.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_ledger_initialization() {
        let mut ledger = Ledger::new();
        ledger.initialize().await.unwrap();
        
        assert!(!ledger.accounts.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_account_operations() {
        let mut ledger = Ledger::new();
        
        // Create an account
        let account = Account::new("1000", "Test Account", AccountType::Asset);
        let created = ledger.create_account(account).await.unwrap();
        assert_eq!(created.number, "1000");
        
        // Get the account
        let found = ledger.get_account(created.id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().number, "1000");
        
        // List accounts
        let accounts = ledger.list_accounts().await.unwrap();
        assert_eq!(accounts.len(), 1);
        
        // Delete the account
        let deleted = ledger.delete_account(created.id).await.unwrap();
        assert!(deleted);
    }

    #[tokio::test]
    async fn test_transaction_recording() {
        let mut ledger = Ledger::new();
        
        // Create accounts
        let cash_account = ledger.create_account(Account::new("1000", "Cash", AccountType::Asset)).await.unwrap();
        let revenue_account = ledger.create_account(Account::new("4000", "Revenue", AccountType::Revenue)).await.unwrap();
        
        // Create a balanced transaction
        let entries = vec![
            TransactionEntry::new(cash_account.id, EntryType::Debit, dec!(100), "Cash received"),
            TransactionEntry::new(revenue_account.id, EntryType::Credit, dec!(100), "Revenue earned"),
        ];
        
        let transaction = Transaction::new("Test transaction".to_string(), Utc::now(), entries);
        
        // Record the transaction
        let recorded = ledger.record_transaction(transaction).await.unwrap();
        assert!(recorded.is_balanced());
        assert!(!recorded.number.is_empty());
        
        // Check account balances
        let cash_balance = ledger.get_account_balance(cash_account.id).await.unwrap();
        assert_eq!(cash_balance, dec!(100));
        
        let revenue_balance = ledger.get_account_balance(revenue_account.id).await.unwrap();
        assert_eq!(revenue_balance, dec!(100));
    }

    #[tokio::test]
    async fn test_trial_balance() {
        let mut ledger = Ledger::new();
        
        // Create accounts
        let cash_account = ledger.create_account(Account::new("1000", "Cash", AccountType::Asset)).await.unwrap();
        let revenue_account = ledger.create_account(Account::new("4000", "Revenue", AccountType::Revenue)).await.unwrap();
        
        // Record a transaction
        let entries = vec![
            TransactionEntry::new(cash_account.id, EntryType::Debit, dec!(100), "Cash received"),
            TransactionEntry::new(revenue_account.id, EntryType::Credit, dec!(100), "Revenue earned"),
        ];
        
        let transaction = Transaction::new("Test transaction".to_string(), Utc::now(), entries);
        ledger.record_transaction(transaction).await.unwrap();
        
        // Get trial balance
        let trial_balance = ledger.get_trial_balance().await.unwrap();
        assert_eq!(trial_balance.len(), 2);
    }

    #[tokio::test]
    async fn test_balance_sheet() {
        let mut ledger = Ledger::new();
        
        // Create accounts
        let cash_account = ledger.create_account(Account::new("1000", "Cash", AccountType::Asset)).await.unwrap();
        let revenue_account = ledger.create_account(Account::new("4000", "Revenue", AccountType::Revenue)).await.unwrap();
        
        // Record a transaction
        let entries = vec![
            TransactionEntry::new(cash_account.id, EntryType::Debit, dec!(100), "Cash received"),
            TransactionEntry::new(revenue_account.id, EntryType::Credit, dec!(100), "Revenue earned"),
        ];
        
        let transaction = Transaction::new("Test transaction".to_string(), Utc::now(), entries);
        ledger.record_transaction(transaction).await.unwrap();
        
        // Get balance sheet
        let balance_sheet = ledger.get_balance_sheet().await.unwrap();
        assert_eq!(balance_sheet.assets, dec!(100));
        assert_eq!(balance_sheet.liabilities, dec!(0));
        assert_eq!(balance_sheet.equity, dec!(0));
    }

    #[tokio::test]
    async fn test_ledger_agent() {
        let agent = LedgerAgent::with_defaults();
        assert_eq!(agent.config.agent_type, AgentType::LedgerAgent);
        assert_eq!(agent.status, AgentStatus::Idle);
    }
}
