//! Reconciliation Module
//!
//! Handles bank reconciliation and statement matching.

use async_trait::async_trait;
use std::collections::{HashMap, HashSet, BTreeMap};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;
use chrono::{DateTime, Utc, NaiveDate};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tracing::{info, error, debug, warn};
use crate::database::financial::{Account, AccountType, EntryType, Transaction, TransactionEntry, TransactionStatus, Reconciliation, ReconciliationStatus};
use crate::database::error::DatabaseError;
use crate::agents::agent_types::{Agent, AgentConfig, AgentType, AgentStatus};
use crate::agents::task::{Task, TaskResult, TaskPayload};
use crate::agents::error::AgentError;

/// Reconciliation error types
#[derive(Debug, thiserror::Error)]
pub enum ReconciliationError {
    /// Account not found
    #[error("Account not found: {0}")]
    AccountNotFound(String),
    
    /// No statement data
    #[error("No statement data provided")]
    NoStatementData,
    
    /// Statement parsing error
    #[error("Statement parsing error: {0}")]
    StatementParsingError(String),
    
    /// Matching error
    #[error("Matching error: {0}")]
    MatchingError(String),
    
    /// Reconciliation already exists
    #[error("Reconciliation already exists for this statement")]
    ReconciliationExists,
    
    /// Database error
    #[error("Database error: {0}")]
    DatabaseError(#[from] DatabaseError),
    
    /// Any other reconciliation error
    #[error("Reconciliation error: {0}")]
    Other(String),
}

impl ReconciliationError {
    /// Create a new other error
    pub fn other(msg: &str) -> Self {
        Self::Other(msg.to_string())
    }
}

/// Result type for reconciliation operations
pub type ReconciliationResult<T> = Result<T, ReconciliationError>;

/// Statement transaction for reconciliation
#[derive(Debug, Clone)]
pub struct StatementTransaction {
    /// Transaction date
    pub date: NaiveDate,
    /// Description
    pub description: String,
    /// Amount
    pub amount: Decimal,
    /// Transaction type (debit or credit)
    pub transaction_type: StatementTransactionType,
    /// Reference number
    pub reference: String,
    /// Whether the transaction is matched
    pub is_matched: bool,
    /// Matched transaction ID
    pub matched_transaction_id: Option<Uuid>,
}

/// Statement transaction type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatementTransactionType {
    /// Debit transaction
    Debit,
    /// Credit transaction
    Credit,
}

/// Reconciliation processor
#[derive(Debug, Clone)]
pub struct ReconciliationProcessor {
    /// Map of reconciliation ID to reconciliation
    pub reconciliations: Arc<RwLock<BTreeMap<Uuid, Reconciliation>>>,
    /// Map of account ID to reconciliations
    pub reconciliations_by_account: Arc<RwLock<HashMap<Uuid, Vec<Uuid>>>>,
}

impl Default for ReconciliationProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl ReconciliationProcessor {
    /// Create a new reconciliation processor
    pub fn new() -> Self {
        Self {
            reconciliations: Arc::new(RwLock::new(BTreeMap::new())),
            reconciliations_by_account: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Initialize the reconciliation processor
    pub async fn initialize(&mut self) -> ReconciliationResult<()> {
        info!("Initializing Reconciliation Processor...");
        Ok(())
    }

    /// Create a new reconciliation
    pub async fn create_reconciliation(
        &mut self,
        account_id: Uuid,
        statement_date: NaiveDate,
        starting_balance: Decimal,
        statement_ending_balance: Decimal,
    ) -> ReconciliationResult<Reconciliation> {
        debug!("Creating reconciliation for account {} on {}", account_id, statement_date);
        
        // Check if reconciliation already exists for this account and date
        let reconciliations_by_account = self.reconciliations_by_account.read().await;
        if let Some(reconciliation_ids) = reconciliations_by_account.get(&account_id) {
            for &reconciliation_id in reconciliation_ids {
                let reconciliation = self.reconciliations.read().await.get(&reconciliation_id);
                if let Some(recon) = reconciliation {
                    if recon.statement_date == statement_date {
                        return Err(ReconciliationError::ReconciliationExists);
                    }
                }
            }
        }
        
        drop(reconciliations_by_account);
        
        let mut reconciliation = Reconciliation {
            id: Uuid::new_v4(),
            account_id,
            statement_date,
            starting_balance,
            ending_balance: starting_balance,
            statement_ending_balance,
            reconciled_transactions: Vec::new(),
            outstanding_transactions: Vec::new(),
            difference: dec!(0),
            status: ReconciliationStatus::InProgress,
            notes: String::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        
        // Calculate initial difference
        reconciliation.difference = reconciliation.statement_ending_balance - reconciliation.starting_balance;
        
        // Store the reconciliation
        self.reconciliations.write().await.insert(reconciliation.id, reconciliation.clone());
        
        // Add to account index
        self.reconciliations_by_account.write().await
            .entry(account_id)
            .or_insert_with(Vec::new)
            .push(reconciliation.id);
        
        Ok(reconciliation)
    }

    /// Get a reconciliation by ID
    pub async fn get_reconciliation(&self, id: Uuid) -> ReconciliationResult<Option<Reconciliation>> {
        let reconciliations = self.reconciliations.read().await;
        Ok(reconciliations.get(&id).cloned())
    }

    /// List reconciliations by account
    pub async fn list_reconciliations_by_account(&self, account_id: Uuid) -> ReconciliationResult<Vec<Reconciliation>> {
        let reconciliations_by_account = self.reconciliations_by_account.read().await;
        let reconciliation_ids = reconciliations_by_account.get(&account_id).cloned().unwrap_or_default();
        
        let reconciliations = self.reconciliations.read().await;
        Ok(reconciliation_ids.into_iter()
            .filter_map(|id| reconciliations.get(&id).cloned())
            .collect())
    }

    /// List all reconciliations
    pub async fn list_all_reconciliations(&self) -> ReconciliationResult<Vec<Reconciliation>> {
        let reconciliations = self.reconciliations.read().await;
        Ok(reconciliations.values().cloned().collect())
    }

    /// Update a reconciliation
    pub async fn update_reconciliation(&mut self, id: Uuid, reconciliation: Reconciliation) -> ReconciliationResult<Reconciliation> {
        debug!("Updating reconciliation: {}", id);
        
        if reconciliation.id != id {
            return Err(ReconciliationError::other("Reconciliation ID mismatch"));
        }
        
        let mut reconciliations = self.reconciliations.write().await;
        if !reconciliations.contains_key(&id) {
            return Err(ReconciliationError::other(&format!("Reconciliation {} not found", id)));
        }
        
        reconciliations.insert(id, reconciliation.clone());
        
        Ok(reconciliation)
    }

    /// Delete a reconciliation
    pub async fn delete_reconciliation(&mut self, id: Uuid) -> ReconciliationResult<bool> {
        debug!("Deleting reconciliation: {}", id);
        
        let mut reconciliations = self.reconciliations.write().await;
        let reconciliation = reconciliations.remove(&id);
        
        if let Some(recon) = reconciliation {
            // Remove from account index
            let mut reconciliations_by_account = self.reconciliations_by_account.write().await;
            if let Some(reconciliation_ids) = reconciliations_by_account.get_mut(&recon.account_id) {
                reconciliation_ids.retain(|&id| id != recon.id);
            }
        }
        
        Ok(reconciliation.is_some())
    }

    /// Reconcile an account with statement data
    pub async fn reconcile_account(
        &mut self,
        account_id: Uuid,
        statement_date: NaiveDate,
        starting_balance: Decimal,
        statement_ending_balance: Decimal,
        statement_transactions: Vec<StatementTransaction>,
        transactions: Vec<Transaction>,
    ) -> ReconciliationResult<Reconciliation> {
        info!("Reconciling account {} for {}", account_id, statement_date);
        
        // Create a new reconciliation
        let mut reconciliation = self.create_reconciliation(
            account_id,
            statement_date,
            starting_balance,
            statement_ending_balance,
        ).await?;
        
        // Match transactions
        let (matched_transactions, unmatched_statement, unmatched_book) = self.match_transactions(
            &statement_transactions,
            &transactions,
        ).await?;
        
        // Update reconciliation with matched transactions
        reconciliation.reconciled_transactions = matched_transactions.iter()
            .filter_map(|t| t.matched_transaction_id)
            .collect();
        
        reconciliation.outstanding_transactions = unmatched_book.iter()
            .map(|t| t.id)
            .collect();
        
        // Update ending balance based on matched transactions
        let mut ending_balance = starting_balance;
        for entry in &matched_transactions {
            let amount = match entry.transaction_type {
                StatementTransactionType::Debit => entry.amount,
                StatementTransactionType::Credit => -entry.amount,
            };
            ending_balance += amount;
        }
        
        reconciliation.ending_balance = ending_balance;
        reconciliation.difference = statement_ending_balance - ending_balance;
        
        // Update status based on difference
        if reconciliation.difference == dec!(0) {
            reconciliation.status = ReconciliationStatus::Completed;
        } else {
            reconciliation.status = ReconciliationStatus::NeedsReview;
        }
        
        // Update the reconciliation
        self.update_reconciliation(reconciliation.id, reconciliation.clone()).await?;
        
        Ok(reconciliation)
    }

    /// Match statement transactions with book transactions
    fn match_transactions(
        &self,
        statement_transactions: &[StatementTransaction],
        book_transactions: &[Transaction],
    ) -> ReconciliationResult<(Vec<StatementTransaction>, Vec<StatementTransaction>, Vec<Transaction>)> {
        let mut matched_statement = Vec::new();
        let mut unmatched_statement = Vec::new();
        let mut unmatched_book = Vec::new();
        
        // Create a map of book transactions by amount and description for matching
        let mut book_map: HashMap<(Decimal, String), Vec<&Transaction>> = HashMap::new();
        for txn in book_transactions {
            let key = (txn.total_amount(), txn.description.clone());
            book_map.entry(key).or_default().push(txn);
        }
        
        // Try to match statement transactions with book transactions
        for statement_txn in statement_transactions {
            let amount = match statement_txn.transaction_type {
                StatementTransactionType::Debit => statement_txn.amount,
                StatementTransactionType::Credit => -statement_txn.amount,
            };
            
            let key = (amount.abs(), statement_txn.description.clone());
            
            if let Some(book_txns) = book_map.get(&key) {
                // Find the first unmatched book transaction
                if let Some(book_txn) = book_txns.first() {
                    let mut matched_txn = statement_txn.clone();
                    matched_txn.is_matched = true;
                    matched_txn.matched_transaction_id = Some(book_txn.id);
                    matched_statement.push(matched_txn);
                    continue;
                }
            }
            
            // No match found
            unmatched_statement.push(statement_txn.clone());
        }
        
        // Find unmatched book transactions
        let matched_book_ids: HashSet<Uuid> = matched_statement.iter()
            .filter_map(|t| t.matched_transaction_id)
            .collect();
        
        for txn in book_transactions {
            if !matched_book_ids.contains(&txn.id) {
                unmatched_book.push(txn.clone());
            }
        }
        
        Ok((matched_statement, unmatched_statement, unmatched_book))
    }

    /// Auto-match transactions based on amount and description
    pub async fn auto_match(
        &mut self,
        reconciliation_id: Uuid,
    ) -> ReconciliationResult<Reconciliation> {
        debug!("Auto-matching transactions for reconciliation {}", reconciliation_id);
        
        let mut reconciliation = self.get_reconciliation(reconciliation_id).await?;
        if reconciliation.is_none() {
            return Err(ReconciliationError::other(&format!("Reconciliation {} not found", reconciliation_id)));
        }
        
        let mut reconciliation = reconciliation.unwrap();
        
        // In a real implementation, this would fetch the statement transactions
        // and book transactions, then attempt to match them
        
        // For now, we'll just mark the reconciliation as needing review
        reconciliation.status = ReconciliationStatus::NeedsReview;
        
        self.update_reconciliation(reconciliation.id, reconciliation.clone()).await?;
        
        Ok(reconciliation)
    }

    /// Mark a transaction as reconciled
    pub async fn mark_transaction_reconciled(
        &mut self,
        reconciliation_id: Uuid,
        transaction_id: Uuid,
    ) -> ReconciliationResult<()> {
        debug!("Marking transaction {} as reconciled in reconciliation {}", transaction_id, reconciliation_id);
        
        let mut reconciliation = self.get_reconciliation(reconciliation_id).await?;
        if reconciliation.is_none() {
            return Err(ReconciliationError::other(&format!("Reconciliation {} not found", reconciliation_id)));
        }
        
        let mut reconciliation = reconciliation.unwrap();
        
        if !reconciliation.reconciled_transactions.contains(&transaction_id) {
            reconciliation.reconciled_transactions.push(transaction_id);
        }
        
        // Remove from outstanding if present
        reconciliation.outstanding_transactions.retain(|&id| id != transaction_id);
        
        self.update_reconciliation(reconciliation.id, reconciliation).await?;
        
        Ok(())
    }

    /// Mark a transaction as unreconciled
    pub async fn mark_transaction_unreconciled(
        &mut self,
        reconciliation_id: Uuid,
        transaction_id: Uuid,
    ) -> ReconciliationResult<()> {
        debug!("Marking transaction {} as unreconciled in reconciliation {}", transaction_id, reconciliation_id);
        
        let mut reconciliation = self.get_reconciliation(reconciliation_id).await?;
        if reconciliation.is_none() {
            return Err(ReconciliationError::other(&format!("Reconciliation {} not found", reconciliation_id)));
        }
        
        let mut reconciliation = reconciliation.unwrap();
        
        reconciliation.reconciled_transactions.retain(|&id| id != transaction_id);
        
        if !reconciliation.outstanding_transactions.contains(&transaction_id) {
            reconciliation.outstanding_transactions.push(transaction_id);
        }
        
        self.update_reconciliation(reconciliation.id, reconciliation).await?;
        
        Ok(())
    }
}

/// Reconciliation Agent for handling reconciliation-related tasks
#[derive(Debug, Clone)]
pub struct ReconciliationAgent {
    /// Agent configuration
    pub config: AgentConfig,
    /// Current status
    pub status: AgentStatus,
    /// Reconciliation processor
    pub processor: ReconciliationProcessor,
}

impl ReconciliationAgent {
    /// Create a new reconciliation agent
    pub fn new(config: AgentConfig, processor: ReconciliationProcessor) -> Self {
        Self {
            config,
            status: AgentStatus::Idle,
            processor,
        }
    }

    /// Create a reconciliation agent with default configuration
    pub fn with_defaults() -> Self {
        let config = AgentConfig::reconciliation_agent();
        let processor = ReconciliationProcessor::new();
        Self::new(config, processor)
    }
}

#[async_trait]
impl Agent for ReconciliationAgent {
    fn config(&self) -> &AgentConfig {
        &self.config
    }

    fn status(&self) -> AgentStatus {
        self.status.clone()
    }

    async fn initialize(&mut self) -> Result<(), anyhow::Error> {
        self.status = AgentStatus::Initializing;
        self.processor.initialize().await?;
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
            crate::agents::task::TaskType::ReconcileAccount => {
                self.process_reconcile_account(task).await
            }
            _ => {
                Err(AgentError::TaskProcessingFailed(
                    format!("ReconciliationAgent cannot handle task type: {:?}", task.task_type)
                ).into())
            }
        }
    }

    fn agent_type(&self) -> AgentType {
        self.config.agent_type.clone()
    }
}

impl ReconciliationAgent {
    /// Process a reconcile account task
    async fn process_reconcile_account(&self, task: Task) -> Result<Task, anyhow::Error> {
        self.status = AgentStatus::Busy;
        
        let start_time = std::time::Instant::now();
        
        // Extract account from task payload
        let account = match task.payload {
            TaskPayload::Account(acc) => acc,
            _ => return Err(AgentError::TaskProcessingFailed(
                "Expected Account payload for ReconcileAccount task".to_string()
            ).into()),
        };

        // In a real implementation, we would fetch statement data and transactions
        // For now, we'll create a mock reconciliation
        let reconciliation = Reconciliation {
            id: Uuid::new_v4(),
            account_id: account.id,
            statement_date: Utc::now().date_naive(),
            starting_balance: account.balance,
            ending_balance: account.balance,
            statement_ending_balance: account.balance,
            reconciled_transactions: Vec::new(),
            outstanding_transactions: Vec::new(),
            difference: dec!(0),
            status: ReconciliationStatus::Completed,
            notes: "Mock reconciliation".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        
        // Create success result
        let result = TaskResult::success_with_data(
            "Account reconciled successfully",
            TaskPayload::Json(serde_json::to_value(reconciliation).unwrap())
        );
        
        let processing_time = start_time.elapsed().as_millis() as f64;
        
        self.status = AgentStatus::Idle;
        
        Ok(task.complete(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn test_reconciliation_processor_creation() {
        let processor = ReconciliationProcessor::new();
        assert!(processor.reconciliations.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_create_reconciliation() {
        let mut processor = ReconciliationProcessor::new();
        
        let account_id = Uuid::new_v4();
        let statement_date = Utc::now().date_naive();
        
        let reconciliation = processor.create_reconciliation(
            account_id,
            statement_date,
            dec!(1000),
            dec!(1500),
        ).await.unwrap();
        
        assert_eq!(reconciliation.account_id, account_id);
        assert_eq!(reconciliation.statement_date, statement_date);
        assert_eq!(reconciliation.starting_balance, dec!(1000));
        assert_eq!(reconciliation.statement_ending_balance, dec!(1500));
        assert_eq!(reconciliation.difference, dec!(500));
    }

    #[tokio::test]
    async fn test_reconciliation_operations() {
        let mut processor = ReconciliationProcessor::new();
        
        let account_id = Uuid::new_v4();
        let reconciliation = processor.create_reconciliation(
            account_id,
            Utc::now().date_naive(),
            dec!(1000),
            dec!(1500),
        ).await.unwrap();
        
        // Get reconciliation
        let found = processor.get_reconciliation(reconciliation.id).await.unwrap();
        assert!(found.is_some());
        
        // List by account
        let reconciliations = processor.list_reconciliations_by_account(account_id).await.unwrap();
        assert_eq!(reconciliations.len(), 1);
        
        // Update reconciliation
        let mut updated = reconciliation.clone();
        updated.notes = "Updated notes".to_string();
        let updated_recon = processor.update_reconciliation(reconciliation.id, updated).await.unwrap();
        assert_eq!(updated_recon.notes, "Updated notes");
        
        // Delete reconciliation
        let deleted = processor.delete_reconciliation(reconciliation.id).await.unwrap();
        assert!(deleted);
    }

    #[tokio::test]
    async fn test_match_transactions() {
        let processor = ReconciliationProcessor::new();
        
        // Create statement transactions
        let statement_txns = vec![
            StatementTransaction {
                date: Utc::now().date_naive(),
                description: "Deposit".to_string(),
                amount: dec!(100),
                transaction_type: StatementTransactionType::Credit,
                reference: "REF001".to_string(),
                is_matched: false,
                matched_transaction_id: None,
            },
            StatementTransaction {
                date: Utc::now().date_naive(),
                description: "Withdrawal".to_string(),
                amount: dec!(50),
                transaction_type: StatementTransactionType::Debit,
                reference: "REF002".to_string(),
                is_matched: false,
                matched_transaction_id: None,
            },
        ];
        
        // Create book transactions
        let book_txns = vec![
            Transaction {
                id: Uuid::new_v4(),
                number: "TRX001".to_string(),
                description: "Deposit".to_string(),
                date: Utc::now(),
                transaction_type: TransactionType::Other,
                status: TransactionStatus::Posted,
                entries: vec![],
                journal_entry_id: None,
                document_ids: vec![],
                metadata: serde_json::json!({}),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        ];
        
        // Match transactions
        let (matched, unmatched_statement, unmatched_book) = processor.match_transactions(
            &statement_txns,
            &book_txns,
        ).unwrap();
        
        assert_eq!(matched.len(), 1);
        assert_eq!(unmatched_statement.len(), 1);
        assert_eq!(unmatched_book.len(), 0);
    }

    #[tokio::test]
    async fn test_reconciliation_agent() {
        let agent = ReconciliationAgent::with_defaults();
        assert_eq!(agent.config.agent_type, AgentType::ReconciliationAgent);
        assert_eq!(agent.status, AgentStatus::Idle);
    }
}
