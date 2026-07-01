//! Agent Orchestrator Module
//!
//! Manages and coordinates all agents in the NexusLedger system.

use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;
use tracing::{info, error, debug, warn};
use crate::agents::agent_types::{Agent, AgentConfig, AgentType, AgentStatus};
use crate::agents::task::{Task, TaskType, TaskPriority, TaskStatus};
use crate::agents::error::AgentError;
use crate::agents::status::{AgentStatusInfo, SystemStatus};
use crate::agents::memory::MemoryManager;
use crate::agents::config::AgentConfigManager;
use crate::database::financial::Transaction;

/// Agent Orchestrator - manages all agents and task distribution
#[derive(Clone)]
pub struct AgentOrchestrator {
    /// Map of agent ID to agent instance
    pub agents: Arc<RwLock<HashMap<Uuid, Arc<Mutex<dyn Agent>>>>>,
    /// Map of agent type to list of agent IDs
    pub agents_by_type: Arc<RwLock<HashMap<AgentType, Vec<Uuid>>>>,
    /// Task queue
    pub task_queue: Arc<Mutex<VecDeque<Task>>>,
    /// In-progress tasks
    pub in_progress_tasks: Arc<Mutex<HashMap<Uuid, Task>>>,
    /// Completed tasks
    pub completed_tasks: Arc<Mutex<VecDeque<Task>>>,
    /// Failed tasks
    pub failed_tasks: Arc<Mutex<VecDeque<Task>>>,
    /// Memory manager
    pub memory_manager: MemoryManager,
    /// Configuration manager
    pub config_manager: AgentConfigManager,
    /// Status information for each agent
    pub agent_statuses: Arc<RwLock<HashMap<Uuid, AgentStatusInfo>>>,
    /// Whether the orchestrator is running
    pub is_running: Arc<Mutex<bool>>,
    /// Optional database connection for persistent storage
    pub database: Option<crate::database::Database>,
    /// Event-driven notification for new tasks (replaces busy-wait sleep)
    pub task_notify: Arc<tokio::sync::Notify>,
    /// Shared ledger used by all accounting agents (LedgerAgent, InvoiceAgent, ReceiptAgent, ReportingAgent)
    pub shared_ledger: Option<Arc<crate::accounting::ledger::Ledger>>,
    /// Persisted dead letter queue file path (None = not persisted)
    pub dead_letter_path: Option<String>,
}

impl std::fmt::Debug for AgentOrchestrator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentOrchestrator")
            .finish_non_exhaustive()
    }
}

impl Default for AgentOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentOrchestrator {
    /// Create a new agent orchestrator
    pub fn new() -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            agents_by_type: Arc::new(RwLock::new(HashMap::new())),
            task_queue: Arc::new(Mutex::new(VecDeque::new())),
            in_progress_tasks: Arc::new(Mutex::new(HashMap::new())),
            completed_tasks: Arc::new(Mutex::new(VecDeque::new())),
            failed_tasks: Arc::new(Mutex::new(VecDeque::new())),
            memory_manager: MemoryManager::new(),
            config_manager: AgentConfigManager::new(),
            agent_statuses: Arc::new(RwLock::new(HashMap::new())),
            is_running: Arc::new(Mutex::new(false)),
            database: None,
            task_notify: Arc::new(tokio::sync::Notify::new()),
            shared_ledger: None,
            dead_letter_path: None,
        }
    }

    /// Create a new agent orchestrator with a database connection
    pub fn with_database(database: crate::database::Database) -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            agents_by_type: Arc::new(RwLock::new(HashMap::new())),
            task_queue: Arc::new(Mutex::new(VecDeque::new())),
            in_progress_tasks: Arc::new(Mutex::new(HashMap::new())),
            completed_tasks: Arc::new(Mutex::new(VecDeque::new())),
            failed_tasks: Arc::new(Mutex::new(VecDeque::new())),
            memory_manager: MemoryManager::new(),
            config_manager: AgentConfigManager::new(),
            agent_statuses: Arc::new(RwLock::new(HashMap::new())),
            is_running: Arc::new(Mutex::new(false)),
            database: Some(database),
            task_notify: Arc::new(tokio::sync::Notify::new()),
            shared_ledger: None,
            dead_letter_path: None,
        }
    }

    /// Initialize the orchestrator and all agents
    pub async fn initialize(&mut self) -> Result<(), anyhow::Error> {
        info!("Initializing Agent Orchestrator...");

        // Connect to database if configured
        if let Some(ref db) = self.database {
            info!("Connecting to database...");
            db.connect().await
                .map_err(|e| anyhow::anyhow!("Failed to connect to database: {}", e))?;
            info!("Seeding default data...");
            db.seed().await
                .map_err(|e| anyhow::anyhow!("Failed to seed database: {}", e))?;
        }

        // Create and initialize a shared ledger for all accounting agents.
        // This ensures InvoiceAgent, ReceiptAgent, ReportingAgent, and LedgerAgent
        // all see the same accounts, transactions, and balances.
        let mut shared_ledger = crate::accounting::ledger::Ledger::new();
        if let Some(ref db) = self.database {
            shared_ledger.db = Some(Arc::new(db.clone()));
        }
        shared_ledger.initialize().await
            .map_err(|e| anyhow::anyhow!("Failed to initialize shared ledger: {}", e))?;
        self.shared_ledger = Some(Arc::new(shared_ledger));

        // Create default agents for each type
        self.create_default_agents().await?;

        // Initialize all agents
        self.initialize_all_agents().await?;

        *self.is_running.lock().await = true;

        info!("Agent Orchestrator initialized with {} agents", self.agents.read().await.len());

        Ok(())
    }

    /// Create default agents for each agent type
    async fn create_default_agents(&mut self) -> Result<(), anyhow::Error> {
        info!("Creating default agents...");
        
        let agent_types = [
            AgentType::LedgerAgent,
            AgentType::ReconciliationAgent,
            AgentType::InvoiceAgent,
            AgentType::PayrollAgent,
            AgentType::TaxAgent,
            AgentType::ReceiptAgent,
            AgentType::DocumentAgent,
            AgentType::AuditAgent,
            AgentType::ReportingAgent,
        ];
        
        for agent_type in agent_types {
            let max_instances = self.config_manager.system_config.get_max_instances(&agent_type);
            
            for i in 0..max_instances {
                let config = AgentConfig::new(
                    agent_type.clone(),
                    &format!("{:?} {}", agent_type, i + 1),
                    &format!("Default {:?} agent instance {}", agent_type, i + 1)
                );
                
                self.add_agent(config).await?;
            }
        }
        
        Ok(())
    }

    /// Initialize all registered agents
    async fn initialize_all_agents(&self) -> Result<(), anyhow::Error> {
        let agents = self.agents.read().await;
        
        for (agent_id, agent) in agents.iter() {
            let mut agent_guard = agent.lock().await;
            if let Err(e) = agent_guard.initialize().await {
                error!("Failed to initialize agent {}: {}", agent_id, e);
                return Err(e);
            }
            
            // Initialize status tracking
            let config = agent_guard.config().clone();
            let mut statuses = self.agent_statuses.write().await;
            statuses.insert(*agent_id, AgentStatusInfo::new(*agent_id, config.agent_type));
        }
        
        Ok(())
    }

    /// Add a new agent to the orchestrator
    pub async fn add_agent(&mut self, config: AgentConfig) -> Result<Uuid, anyhow::Error> {
        info!("Adding agent: {} ({:?})", config.name, config.agent_type);
        
        // Create the appropriate agent based on type
        let agent: Arc<Mutex<dyn Agent>> = match config.agent_type {
            AgentType::LedgerAgent => {
                let ledger = match &self.shared_ledger {
                    Some(shared) => crate::accounting::ledger::Ledger {
                        accounts: shared.accounts.clone(),
                        transactions: shared.transactions.clone(),
                        journal_entries: shared.journal_entries.clone(),
                        current_journal_number: shared.current_journal_number.clone(),
                        current_transaction_number: shared.current_transaction_number.clone(),
                        db: shared.db.clone(),
                    },
                    None => {
                        let mut l = crate::accounting::ledger::Ledger::new();
                        if let Some(ref db) = self.database {
                            l.db = Some(Arc::new(db.clone()));
                        }
                        l
                    }
                };
                Arc::new(Mutex::new(crate::accounting::ledger::LedgerAgent::new(
                    config.clone(),
                    ledger
                )))
            }
            AgentType::ReconciliationAgent => {
                let mut processor = crate::accounting::reconciliation::ReconciliationProcessor::new();
                if let Some(ref db) = self.database {
                    processor.db = Some(Arc::new(db.clone()));
                }
                // Wire the shared ledger for fetching book transactions
                processor.ledger = self.shared_ledger.clone();
                Arc::new(Mutex::new(crate::accounting::reconciliation::ReconciliationAgent::new(
                    config.clone(),
                    processor
                )))
            }
            AgentType::InvoiceAgent => {
                let mut processor = crate::accounting::invoice::InvoiceProcessor::new();
                if let Some(ref db) = self.database {
                    processor.db = Some(Arc::new(db.clone()));
                }
                // Wire the shared ledger so payment transactions use real accounts
                processor.ledger = self.shared_ledger.clone();
                Arc::new(Mutex::new(crate::accounting::invoice::InvoiceAgent::new(
                    config.clone(),
                    processor
                )))
            }
            AgentType::ApAgent => {
                let mut processor = crate::accounting::ap::ApProcessor::new();
                processor.ledger = self.shared_ledger.clone();
                Arc::new(Mutex::new(crate::accounting::ap::ApAgent::new(
                    config.clone(),
                    processor
                )))
            }
            AgentType::PayrollAgent => {
                let mut processor = crate::accounting::payroll::PayrollProcessor::new();
                if let Some(ref db) = self.database {
                    processor.db = Some(Arc::new(db.clone()));
                }
                // Wire the shared ledger for real account references in payroll transactions
                processor.ledger = self.shared_ledger.clone();
                Arc::new(Mutex::new(crate::accounting::payroll::PayrollAgent::new(
                    config.clone(),
                    processor
                )))
            }
            AgentType::TaxAgent => {
                Arc::new(Mutex::new(crate::accounting::tax::TaxAgent::new(
                    config.clone(),
                    crate::accounting::tax::TaxCalculator::new()
                )))
            }
            AgentType::ReceiptAgent => {
                let mut processor = crate::accounting::receipt::ReceiptProcessor::new();
                if let Some(ref db) = self.database {
                    processor.db = Some(Arc::new(db.clone()));
                }
                // Wire the shared ledger so expense transactions use real accounts
                processor.ledger = self.shared_ledger.clone();
                Arc::new(Mutex::new(crate::accounting::receipt::ReceiptAgent::new(
                    config.clone(),
                    processor
                )))
            }
            AgentType::DocumentAgent => {
                let db_client = match &self.database {
                    Some(db) => db.db().await.ok(),
                    None => None,
                };
                Arc::new(Mutex::new(crate::agents::document::DocumentAgent::new(
                    config.clone(),
                    db_client
                )))
            }
            AgentType::AuditAgent => {
                let audit_repo: Arc<dyn crate::database::audit::AuditRepository> = match &self.database {
                    Some(db) => Arc::new(crate::database::audit::SurrealAuditRepository::new(db.db().await.ok())),
                    None => Arc::new(crate::database::audit::MemoryAuditRepository::new()),
                };
                Arc::new(Mutex::new(crate::audit::AuditAgent::new(
                    config.clone(),
                    audit_repo
                )))
            }
            AgentType::ReportingAgent => {
                // Use the shared ledger so reports contain real data from all agents
                let ledger = self.shared_ledger.clone()
                    .or_else(|| {
                        let mut l = crate::accounting::ledger::Ledger::new();
                        if let Some(ref db) = self.database {
                            l.db = Some(Arc::new(db.clone()));
                        }
                        Some(Arc::new(l))
                    });
                Arc::new(Mutex::new(crate::accounting::reporting::ReportingAgent::new(
                    config.clone(),
                    ledger
                )))
            }
        };
        
        let agent_id = config.id;
        let agent_type = config.agent_type.clone();

        // Add to agents map
        self.agents.write().await.insert(agent_id, agent.clone());

        // Add to agents_by_type map
        let mut agents_by_type = self.agents_by_type.write().await;
        agents_by_type.entry(agent_type.clone())
            .or_insert_with(Vec::new)
            .push(agent_id);

        // Add to config manager
        self.config_manager.add_agent_config(config);

        // Initialize status tracking
        let mut statuses = self.agent_statuses.write().await;
        statuses.insert(agent_id, AgentStatusInfo::new(agent_id, agent_type));

        info!("Agent {} added successfully", agent_id);
        
        Ok(agent_id)
    }

    /// Remove an agent from the orchestrator
    pub async fn remove_agent(&mut self, agent_id: Uuid) -> Result<(), anyhow::Error> {
        info!("Removing agent: {}", agent_id);
        
        // Get the agent to shutdown
        if let Some(agent) = self.agents.write().await.remove(&agent_id) {
            let mut agent_guard = agent.lock().await;
            agent_guard.shutdown().await?;
        }
        
        // Remove from agents_by_type
        let mut agents_by_type = self.agents_by_type.write().await;
        for (_, agent_ids) in agents_by_type.iter_mut() {
            agent_ids.retain(|&id| id != agent_id);
        }
        
        // Remove from config manager
        self.config_manager.remove_agent_config(&agent_id);
        
        // Remove from status tracking
        self.agent_statuses.write().await.remove(&agent_id);
        
        Ok(())
    }

    /// Submit a task to the orchestrator
    pub async fn submit_task(&self, task: Task) -> Result<Uuid, anyhow::Error> {
        debug!("Submitting task: {} ({:?})", task.id, task.task_type);

        // Save task ID before moving task into the queue
        let task_id = task.id;

        // Add task to queue
        self.task_queue.lock().await.push_back(task);

        // Notify the dispatch loop that a new task is available
        self.task_notify.notify_one();

        Ok(task_id)
    }

    /// Process a transaction through the appropriate agent
    pub async fn process_transaction(&self, transaction: Transaction) -> Result<Transaction, anyhow::Error> {
        info!("Processing transaction: {}", transaction.description);
        
        // Create a task for the transaction
        let task = Task::record_transaction(transaction.clone());
        
        // Submit the task
        self.submit_task(task).await?;
        
        // For now, return the original transaction
        // In a real implementation, we would wait for the task to complete
        // and return the processed transaction
        
        Ok(transaction)
    }

    /// Generate financial reports
    pub async fn generate_reports(&self) -> Result<(), anyhow::Error> {
        info!("Generating financial reports...");
        
        // Create a report generation task
        let task = Task::new(TaskType::GenerateReport);
        
        // Submit the task
        self.submit_task(task).await?;
        
        Ok(())
    }

    /// Process the next available task(s). Sorts queue by priority (highest first),
    /// then processes each ready task. Uses tokio::spawn for concurrent execution
    /// when multiple tasks and agents are available.
    pub async fn process_next_task(&self) -> Result<(), anyhow::Error> {
        // Sort queue by priority (Critical=10 first, Low=1 last)
        {
            let mut task_queue = self.task_queue.lock().await;
            task_queue.make_contiguous().sort_by(|a, b| b.priority.cmp(&a.priority));
        }

        // Get the next task from the queue
        let mut task_queue = self.task_queue.lock().await;
        let task = match task_queue.pop_front() {
            Some(task) => task,
            None => return Ok(()), // No tasks to process
        };

        drop(task_queue); // Release the lock

        // Find an available agent for this task
        let assigned_agent_id = self.find_available_agent(&task).await;

        match assigned_agent_id {
            Some(agent_id) => {
                // Assign the task to the agent
                self.assign_task_to_agent(task, agent_id).await?;
            }
            None => {
                // No available agent, put the task back in the queue
                warn!("No available agent for task {:?}, retrying later...", task.task_type);
                self.task_queue.lock().await.push_front(task);
            }
        }

        Ok(())
    }

    /// Find an available agent for a task
    async fn find_available_agent(&self, task: &Task) -> Option<Uuid> {
        // First, try to find an agent of the assigned type
        if let Some(agent_type) = &task.assigned_agent_type {
            if let Some(agent_ids) = self.agents_by_type.read().await.get(agent_type) {
                for &agent_id in agent_ids {
                    if self.is_agent_available(agent_id).await {
                        return Some(agent_id);
                    }
                }
            }
        }
        
        // If no specific agent type is assigned or no agents of that type are available,
        // try to find any available agent
        let agents = self.agents.read().await;
        for (agent_id, agent) in agents.iter() {
            if self.is_agent_available(*agent_id).await {
                return Some(*agent_id);
            }
        }
        
        None
    }

    /// Check if an agent is available to process a task
    async fn is_agent_available(&self, agent_id: Uuid) -> bool {
        if let Some(agent) = self.agents.read().await.get(&agent_id) {
            let agent_guard = agent.lock().await;
            
            // Check if agent is enabled
            if !agent_guard.config().enabled {
                return false;
            }
            
            // Check agent status
            match agent_guard.status() {
                AgentStatus::Idle => true,
                AgentStatus::Busy => false,
                AgentStatus::Error(_) => false,
                AgentStatus::Initializing | AgentStatus::ShuttingDown => false,
            }
        } else {
            false
        }
    }

    /// Assign a task to an agent
    async fn assign_task_to_agent(&self, mut task: Task, agent_id: Uuid) -> Result<(), anyhow::Error> {
        info!("Assigning task {} to agent {}", task.id, agent_id);
        
        // Update task status
        task.status = TaskStatus::Processing;
        task.assigned_agent_id = Some(agent_id);
        
        // Add to in-progress tasks
        self.in_progress_tasks.lock().await.insert(task.id, task.clone());
        
        // Get the agent and process the task
        if let Some(agent) = self.agents.read().await.get(&agent_id) {
            let agent_guard = agent.lock().await;
            
            // Update agent status
            let mut statuses = self.agent_statuses.write().await;
            if let Some(status) = statuses.get_mut(&agent_id) {
                status.update_status(AgentStatus::Busy);
                status.increment_tasks_in_progress();
            }
            
            drop(statuses); // Release the lock

            // Clone task before moving into process_task to retain access in error path
            let task_clone = task.clone();

            // Process the task (in a real implementation, this would be async)
            match agent_guard.process_task(task_clone).await {
                Ok(completed_task) => {
                    self.handle_completed_task(completed_task, agent_id).await?;
                }
                Err(e) => {
                    error!("Task {} failed: {}", task.id, e);
                    self.handle_failed_task(task, agent_id, e.to_string()).await?;
                }
            }
        }
        
        Ok(())
    }

    /// Handle a completed task
    async fn handle_completed_task(&self, task: Task, agent_id: Uuid) -> Result<(), anyhow::Error> {
        info!("Task {} completed by agent {}", task.id, agent_id);
        
        // Remove from in-progress tasks
        self.in_progress_tasks.lock().await.remove(&task.id);
        
        // Add to completed tasks
        self.completed_tasks.lock().await.push_back(task.clone());
        
        // Update agent status
        let mut statuses = self.agent_statuses.write().await;
        if let Some(status) = statuses.get_mut(&agent_id) {
            status.update_status(AgentStatus::Idle);
            status.decrement_tasks_in_progress();
            status.record_task_completion(0.0); // TODO: track actual processing time
        }
        
        Ok(())
    }

    /// Handle a failed task
    async fn handle_failed_task(&self, mut task: Task, agent_id: Uuid, error: String) -> Result<(), anyhow::Error> {
        error!("Task {} failed on agent {}: {}", task.id, agent_id, error);

        // Clone error string before moving into TaskStatus::Failed
        let error_clone = error.clone();

        // Remove from in-progress tasks
        self.in_progress_tasks.lock().await.remove(&task.id);

        // Update task status
        task.status = TaskStatus::Failed(error_clone);
        
        // Check if we should retry
        if task.can_retry() {
            task.increment_retry();
            warn!("Retrying task {} (attempt {}/{})", task.id, task.retry_count, task.max_retries);
            
            // Put the task back in the queue
            self.task_queue.lock().await.push_front(task);
        } else {
            // Max retries exceeded, add to failed tasks
            self.failed_tasks.lock().await.push_back(task);
        }

        // Persist to dead letter queue if configured
        if let Some(ref path) = self.dead_letter_path {
            self.persist_failed_tasks(path).await?;
        }
        
        // Update agent status
        let mut statuses = self.agent_statuses.write().await;
        if let Some(status) = statuses.get_mut(&agent_id) {
            status.update_status(AgentStatus::Idle);
            status.decrement_tasks_in_progress();
            status.record_task_failure(&error);
        }
        
        Ok(())
    }

    /// Persist failed tasks to a JSON file (dead letter queue).
    pub async fn persist_failed_tasks(&self, path: &str) -> Result<(), anyhow::Error> {
        let failed = self.failed_tasks.lock().await;
        let data = serde_json::to_string_pretty(&*failed)
            .map_err(|e| anyhow::anyhow!("Failed to serialize failed tasks: {}", e))?;
        tokio::fs::write(path, data).await?;
        info!("Persisted {} failed tasks to {}", failed.len(), path);
        Ok(())
    }

    /// Get the current system status
    pub async fn get_system_status(&self) -> SystemStatus {
        let mut system_status = SystemStatus::new();
        
        // Copy agent statuses
        let statuses = self.agent_statuses.read().await;
        for (_, status_info) in statuses.iter() {
            system_status.add_agent_status(status_info.clone());
        }
        
        // Update task counts
        system_status.total_tasks_in_progress = self.in_progress_tasks.lock().await.len();
        system_status.total_tasks_processed = self.completed_tasks.lock().await.len() as u64;
        system_status.total_tasks_failed = self.failed_tasks.lock().await.len() as u64;
        
        system_status
    }

    /// Start the orchestrator's main processing loop.
    ///
    /// Uses `tokio::sync::Notify` for event-driven dispatch — the loop
    /// sleeps until a task is submitted (via `submit_task`), then
    /// immediately processes it. No busy-wait polling.
    pub async fn start(&self) -> Result<(), anyhow::Error> {
        info!("Starting Agent Orchestrator processing loop (event-driven)...");

        *self.is_running.lock().await = true;

        while *self.is_running.lock().await {
            // Process all queued tasks before waiting for the next notification
            loop {
                let has_tasks = !self.task_queue.lock().await.is_empty();
                if !has_tasks {
                    break;
                }
                self.process_next_task().await?;
            }

            // Wait for a new task to be submitted (event-driven, no busy-wait)
            self.task_notify.notified().await;
        }

        Ok(())
    }

    /// Stop the orchestrator
    pub async fn stop(&self) -> Result<(), anyhow::Error> {
        info!("Stopping Agent Orchestrator...");
        
        *self.is_running.lock().await = false;
        
        // Shutdown all agents
        let agents = self.agents.read().await;
        for (agent_id, agent) in agents.iter() {
            let mut agent_guard = agent.lock().await;
            if let Err(e) = agent_guard.shutdown().await {
                error!("Error shutting down agent {}: {}", agent_id, e);
            }
        }
        
        info!("Agent Orchestrator stopped");
        
        Ok(())
    }
}

#[async_trait]
impl Agent for AgentOrchestrator {
    fn config(&self) -> &AgentConfig {
        // Orchestrator doesn't have a specific config, return a default
        static ORCHESTRATOR_CONFIG: once_cell::sync::Lazy<AgentConfig> = once_cell::sync::Lazy::new(|| {
            AgentConfig::new(
                AgentType::ReportingAgent, // Using ReportingAgent as placeholder
                "Agent Orchestrator",
                "Manages and coordinates all agents"
            )
        });
        &ORCHESTRATOR_CONFIG
    }

    fn status(&self) -> AgentStatus {
        if *self.is_running.blocking_lock() {
            AgentStatus::Busy
        } else {
            AgentStatus::Idle
        }
    }

    async fn initialize(&mut self) -> Result<(), anyhow::Error> {
        self.initialize().await
    }

    async fn shutdown(&mut self) -> Result<(), anyhow::Error> {
        self.stop().await
    }

    async fn process_task(&self, _task: Task) -> Result<Task, anyhow::Error> {
        // Orchestrator doesn't process tasks directly, it delegates to agents
        Err(AgentError::TaskProcessingFailed(
            "Agent Orchestrator does not process tasks directly".to_string()
        ).into())
    }

    fn agent_type(&self) -> AgentType {
        AgentType::ReportingAgent // Placeholder
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_orchestrator_creation() {
        let orchestrator = AgentOrchestrator::new();
        assert!(orchestrator.agents.read().await.is_empty());
        assert!(!*orchestrator.is_running.lock().await);
    }

    #[tokio::test]
    async fn test_orchestrator_initialization() {
        let mut orchestrator = AgentOrchestrator::new();
        let result = orchestrator.initialize().await;
        assert!(result.is_ok());
        assert!(*orchestrator.is_running.lock().await);
        assert!(!orchestrator.agents.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_submit_task() {
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.initialize().await.unwrap();
        
        let task = Task::new(TaskType::RecordTransaction);
        let task_id = orchestrator.submit_task(task).await.unwrap();
        
        assert!(!orchestrator.task_queue.lock().await.is_empty());
    }

    #[tokio::test]
    async fn test_system_status() {
        let mut orchestrator = AgentOrchestrator::new();
        orchestrator.initialize().await.unwrap();
        
        let status = orchestrator.get_system_status().await;
        assert!(status.total_agents > 0);
        assert!(status.health_score > 0.0);
    }
}
