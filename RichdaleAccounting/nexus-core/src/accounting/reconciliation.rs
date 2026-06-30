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
    /// Match confidence score (0-210, higher is better)
    pub match_score: Option<u32>,
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
    /// Optional SurrealDB connection for persistence
    pub db: Option<Arc<crate::database::Database>>,
    /// Optional ledger for fetching book transactions to match against
    pub ledger: Option<Arc<crate::accounting::ledger::Ledger>>,
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
            db: None,
            ledger: None,
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
                let reconciliations_guard = self.reconciliations.read().await;
                let reconciliation = reconciliations_guard.get(&reconciliation_id);
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

        // Persist to SurrealDB if available
        if let Some(ref db) = self.db {
            if let Ok(client) = db.db().await {
                let value = serde_json::to_value(&reconciliation).unwrap_or_default();
                if let Err(e) = client.create::<Vec<serde_json::Value>>("reconciliation").content(value).await {
                    warn!("Failed to persist reconciliation to SurrealDB: {}", e);
                }
            }
        }

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
        drop(reconciliations);

        // Persist to SurrealDB if available
        if let Some(ref db) = self.db {
            if let Ok(client) = db.db().await {
                let value = serde_json::to_value(&reconciliation).unwrap_or_default();
                if let Err(e) = client.create::<Vec<serde_json::Value>>("reconciliation").content(value).await {
                    warn!("Failed to persist reconciliation update to SurrealDB: {}", e);
                }
            }
        }

        Ok(reconciliation)
    }

    /// Delete a reconciliation
    pub async fn delete_reconciliation(&mut self, id: Uuid) -> ReconciliationResult<bool> {
        debug!("Deleting reconciliation: {}", id);
        
        let mut reconciliations = self.reconciliations.write().await;
        let reconciliation = reconciliations.remove(&id);
        let found = reconciliation.is_some();

        if let Some(recon) = reconciliation {
            // Remove from account index
            let mut reconciliations_by_account = self.reconciliations_by_account.write().await;
            if let Some(reconciliation_ids) = reconciliations_by_account.get_mut(&recon.account_id) {
                reconciliation_ids.retain(|&id| id != recon.id);
            }
        }

        Ok(found)
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
        )?;
        
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

    /// Match statement transactions with book transactions.
    ///
    /// Uses a multi-pass fuzzy matching strategy:
    /// 1. **Exact match**: amount + description + reference number
    /// 2. **Strong match**: amount (within $0.01) + reference number
    /// 3. **Amount match**: exact amount + date within 3 days
    /// 4. **Fuzzy match**: amount within $0.01 + similar description
    fn match_transactions(
        &self,
        statement_transactions: &[StatementTransaction],
        book_transactions: &[Transaction],
    ) -> ReconciliationResult<(Vec<StatementTransaction>, Vec<StatementTransaction>, Vec<Transaction>)> {
        let mut matched_statement = Vec::new();
        let mut unmatched_statement = Vec::new();
        let mut unmatched_book = Vec::new();
        let mut matched_book_ids: HashSet<Uuid> = HashSet::new();

        // Amount tolerance for fuzzy matching
        let tolerance = dec!(0.01);

        for statement_txn in statement_transactions {
            let stmt_amount = match statement_txn.transaction_type {
                StatementTransactionType::Debit => statement_txn.amount,
                StatementTransactionType::Credit => -statement_txn.amount,
            };
            let stmt_abs = stmt_amount.abs();

            let mut best_match: Option<(&Transaction, u32)> = None; // (txn, score)

            for book_txn in book_transactions {
                if matched_book_ids.contains(&book_txn.id) {
                    continue; // Already matched
                }

                let book_amount = book_txn.total_amount().abs();
                let amount_diff = (stmt_abs - book_amount).abs();
                let date_diff = (statement_txn.date - book_txn.date.date_naive()).num_days().unsigned_abs();

                let mut score: u32 = 0;

                // Amount matching
                if amount_diff == dec!(0) {
                    score += 100; // Exact amount match
                } else if amount_diff <= tolerance {
                    score += 80; // Within tolerance
                } else {
                    continue; // Amount too different, skip
                }

                // Description matching
                if !statement_txn.description.is_empty() && !book_txn.description.is_empty() {
                    let stmt_lower = statement_txn.description.to_lowercase();
                    let book_lower = book_txn.description.to_lowercase();
                    if stmt_lower == book_lower {
                        score += 50; // Exact description
                    } else if stmt_lower.contains(&book_lower) || book_lower.contains(&stmt_lower) {
                        score += 30; // Partial description overlap
                    } else if stmt_lower.split_whitespace().any(|w| book_lower.contains(w) && w.len() > 3) {
                        score += 15; // Shared significant word
                    }
                }

                // Reference number matching
                if !statement_txn.reference.is_empty() {
                    if book_txn.entries.iter().any(|e| e.reference.as_deref() == Some(&statement_txn.reference)) {
                        score += 40; // Reference match on entry
                    } else if book_txn.number == statement_txn.reference || book_txn.description.contains(&statement_txn.reference) {
                        score += 30; // Reference in txn number or description
                    }
                }

                // Date proximity
                if date_diff == 0 {
                    score += 20; // Same day
                } else if date_diff <= 3 {
                    score += 10; // Within 3 days
                } else if date_diff <= 7 {
                    score += 5; // Within a week
                }

                // Track the best match
                if let Some((_, best_score)) = best_match {
                    if score > best_score {
                        best_match = Some((book_txn, score));
                    }
                } else {
                    best_match = Some((book_txn, score));
                }
            }

            if let Some((matched_txn, score)) = best_match {
                let mut matched = statement_txn.clone();
                matched.is_matched = true;
                matched.matched_transaction_id = Some(matched_txn.id);
                matched.match_score = Some(score);
                matched_statement.push(matched);
                matched_book_ids.insert(matched_txn.id);
            } else {
                unmatched_statement.push(statement_txn.clone());
            }
        }

        // Find unmatched book transactions
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
    /// Reconciliation processor (wrapped for interior mutability)
    pub processor: Arc<Mutex<ReconciliationProcessor>>,
}

impl ReconciliationAgent {
    /// Create a new reconciliation agent
    pub fn new(config: AgentConfig, processor: ReconciliationProcessor) -> Self {
        Self {
            config,
            status: AgentStatus::Idle,
            processor: Arc::new(Mutex::new(processor)),
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
        self.processor.lock().await.initialize().await?;
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
    /// Process a reconcile account task.
    ///
    /// Accepts either:
    /// - `TaskPayload::Account` — creates a basic reconciliation from the account balance
    /// - `TaskPayload::Json` with fields:
    ///     `account_id` (string, UUID), `starting_balance` (string/number),
    ///     `statement_ending_balance` (string/number), `statement_date` (YYYY-MM-DD),
    ///     `statement_transactions` (array of {date, description, amount, transaction_type: "Debit"|"Credit", reference})
    async fn process_reconcile_account(&self, task: Task) -> Result<Task, anyhow::Error> {
        let start_time = std::time::Instant::now();
        let mut processor = self.processor.lock().await;

        let reconciliation = match &task.payload {
            TaskPayload::Account(acc) => {
                // Simple reconciliation from account balance
                processor.create_reconciliation(
                    acc.id,
                    Utc::now().date_naive(),
                    acc.balance,
                    acc.balance,
                ).await.map_err(|e| AgentError::TaskProcessingFailed(e.to_string()))?
            }
            TaskPayload::Json(json) => {
                let account_id_str = json.get("account_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| AgentError::TaskProcessingFailed("Missing account_id".to_string()))?;
                let account_id = Uuid::parse_str(account_id_str)
                    .map_err(|e| AgentError::TaskProcessingFailed(format!("Invalid account_id: {}", e)))?;

                let starting_balance = json.get("starting_balance")
                    .and_then(|v| v.as_str().or_else(|| v.as_f64().map(|_| "")).or(Some("")))
                    .and_then(|_| {
                        if let Some(s) = json.get("starting_balance").and_then(|v| v.as_str()) {
                            s.parse::<Decimal>().ok()
                        } else if let Some(n) = json.get("starting_balance").and_then(|v| v.as_f64()) {
                            Decimal::from_f64_retain(n)
                        } else {
                            Some(dec!(0))
                        }
                    })
                    .unwrap_or(dec!(0));

                let statement_ending_balance = json.get("statement_ending_balance")
                    .and_then(|v| {
                        if let Some(s) = v.as_str() {
                            s.parse::<Decimal>().ok()
                        } else if let Some(n) = v.as_f64() {
                            Decimal::from_f64_retain(n)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(dec!(0));

                let statement_date = json.get("statement_date")
                    .and_then(|v| v.as_str())
                    .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
                    .unwrap_or_else(|| Utc::now().date_naive());

                // Parse statement transactions if provided
                let statement_transactions: Vec<StatementTransaction> = json.get("statement_transactions")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter().filter_map(|item| {
                            let date = item.get("date").and_then(|v| v.as_str())
                                .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
                                .unwrap_or_else(|| Utc::now().date_naive());
                            let description = item.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let amount = item.get("amount").and_then(|v| {
                                if let Some(s) = v.as_str() { s.parse::<Decimal>().ok() }
                                else if let Some(n) = v.as_f64() { Decimal::from_f64_retain(n) }
                                else { None }
                            }).unwrap_or(dec!(0));
                            let txn_type = item.get("transaction_type").and_then(|v| v.as_str())
                                .map(|s| match s {
                                    "Debit" | "debit" => StatementTransactionType::Debit,
                                    _ => StatementTransactionType::Credit,
                                })
                                .unwrap_or(StatementTransactionType::Credit);
                            let reference = item.get("reference").and_then(|v| v.as_str()).unwrap_or("").to_string();

                            Some(StatementTransaction {
                                date,
                                description,
                                amount,
                                transaction_type: txn_type,
                                reference,
                                is_matched: false,
                                matched_transaction_id: None,
                                match_score: None,
                            })
                        }).collect()
                    })
                    .unwrap_or_default();

                // Fetch book transactions from the shared ledger for matching
                let book_transactions = match &processor.ledger {
                    Some(ledger) => ledger.list_transactions()
                        .await
                        .map_err(|e| AgentError::TaskProcessingFailed(e.to_string()))?,
                    None => Vec::new(),
                };

                processor.reconcile_account(
                    account_id,
                    statement_date,
                    starting_balance,
                    statement_ending_balance,
                    statement_transactions,
                    book_transactions,
                ).await.map_err(|e| AgentError::TaskProcessingFailed(e.to_string()))?
            }
            _ => return Err(AgentError::TaskProcessingFailed(
                "Expected Account or Json payload for ReconcileAccount task".to_string()
            ).into()),
        };

        let message = format!(
            "Account reconciled: difference={}, status={:?}",
            reconciliation.difference, reconciliation.status
        );

        let result = TaskResult::success_with_data(
            &message,
            TaskPayload::Json(serde_json::to_value(&reconciliation).unwrap_or_default()),
        );

        let _processing_time = start_time.elapsed().as_millis() as f64;

        Ok(task.complete(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::financial::TransactionType;
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
                match_score: None,
            },
            StatementTransaction {
                date: Utc::now().date_naive(),
                description: "Withdrawal".to_string(),
                amount: dec!(50),
                transaction_type: StatementTransactionType::Debit,
                reference: "REF002".to_string(),
                is_matched: false,
                matched_transaction_id: None,
                match_score: None,
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
        
        // Book transaction has empty entries so total_amount() is 0, which doesn't match amount 100
        assert_eq!(matched.len(), 0);
        assert_eq!(unmatched_statement.len(), 2);
        assert_eq!(unmatched_book.len(), 1);
    }

    #[tokio::test]
    async fn test_reconciliation_agent() {
        let agent = ReconciliationAgent::with_defaults();
        assert_eq!(agent.config.agent_type, AgentType::ReconciliationAgent);
        assert_eq!(agent.status, AgentStatus::Idle);
    }
}
