# Agent System

## Agent Types

NexusLedger defines **9 agent types**, each responsible for a specific accounting domain:

| Agent Type | Config Method | Responsibility | Implemented? |
|---|---|---|---|
| `LedgerAgent` | `ledger_agent()` | Double-entry accounting, transaction recording, chart of accounts | Partial (mock logic) |
| `ReconciliationAgent` | `reconciliation_agent()` | Bank statement matching, reconciliation | Partial (mock logic) |
| `InvoiceAgent` | `invoice_agent()` | Invoice creation, billing, customer statements | **Not implemented** (maps to LedgerAgent) |
| `PayrollAgent` | `payroll_agent()` | Payroll calculations, tax withholding, payments | Partial (mock logic) |
| `TaxAgent` | `tax_agent()` | Tax calculations, multi-jurisdiction filings | Partial (mock logic) |
| `ReceiptAgent` | `receipt_agent()` | Receipt processing and categorization | **Not implemented** (maps to DocumentAgent) |
| `DocumentAgent` | `document_agent()` | Document storage, retrieval, OCR | Partial (stub) |
| `AuditAgent` | `audit_agent()` | Audit trail, compliance checks | Partial (stub) |
| `ReportingAgent` | `reporting_agent()` | Financial report generation | **Not implemented** (maps to LedgerAgent — wrong) |

## Agent Trait

```rust
#[async_trait]
pub trait Agent: Send + Sync {
    fn config(&self) -> &AgentConfig;
    fn status(&self) -> AgentStatus;
    async fn initialize(&mut self) -> Result<(), anyhow::Error>;
    async fn shutdown(&mut self) -> Result<(), anyhow::Error>;
    async fn process_task(&self, task: Task) -> Result<Task, anyhow::Error>;
    fn agent_type(&self) -> AgentType;
}
```

### Design Issues

1. **`process_task` takes `&self`** but implementations mutate `self.status` — this violates Rust's borrowing rules and will not compile as written.

2. **`initialize` takes `&mut self`** but the orchestrator stores agents as `Arc<Mutex<dyn Agent>>`, so initialization happens through the Mutex guard and works correctly.

3. **No inter-agent communication API** — agents cannot send messages to each other, request help, or coordinate.

## Agent Configuration

```rust
pub struct AgentConfig {
    pub id: Uuid,
    pub agent_type: AgentType,
    pub name: String,
    pub description: String,
    pub priority: u8,           // 0-10
    pub max_concurrent_tasks: usize,
    pub enabled: bool,
    pub parameters: HashMap<String, String>,
}
```

The `AgentSystemConfig` defines how many instances of each agent type to spawn:

| Agent Type | Default Instances | Priority |
|---|---|---|
| Ledger | 1 | 10 (highest) |
| Reconciliation | 1 | 8 |
| Invoice | 2 | 7 |
| Payroll | 1 | 6 |
| Tax | 1 | 9 |
| Receipt | 2 | 5 |
| Document | 2 | 4 |
| Audit | 1 | 8 |
| Reporting | 1 | 3 (lowest) |

## Agent Lifecycle

```
  ┌──────────┐
  │   Idle   │◄──────────────┐
  └────┬─────┘               │
       │ submit_task()       │ task complete
       ▼                     │
  ┌──────────┐     ┌─────────┴──┐
  │   Busy   │────▶│  Complete  │
  └────┬─────┘     └────────────┘
       │ failure
       ▼
  ┌──────────┐     ┌──────────┐
  │  Error   │────▶│  Retry   │──▶ back to queue
  └──────────┘     └──────────┘
       │ max retries exceeded
       ▼
  ┌──────────┐
  │  Failed  │
  └──────────┘
```

## Orchestrator

The `AgentOrchestrator` is the central coordinator:

```
AgentOrchestrator
├── agents: Arc<RwLock<HashMap<Uuid, Arc<Mutex<dyn Agent>>>>>
├── agents_by_type: Arc<RwLock<HashMap<AgentType, Vec<Uuid>>>>
├── task_queue: Arc<Mutex<VecDeque<Task>>>
├── in_progress_tasks: Arc<Mutex<HashMap<Uuid, Task>>>
├── completed_tasks: Arc<Mutex<VecDeque<Task>>>
├── failed_tasks: Arc<Mutex<VecDeque<Task>>>
├── memory_manager: MemoryManager
├── config_manager: AgentConfigManager
├── agent_statuses: Arc<RwLock<HashMap<Uuid, AgentStatusInfo>>>
└── is_running: Arc<Mutex<bool>>
```

### Task Processing Loop

```
start()
  └── while is_running:
        └── process_next_task()
              ├── Pop task from queue
              ├── find_available_agent(task)
              │     └── Check by assigned_agent_type, fallback to any idle
              ├── assign_task_to_agent(task, agent_id)
              │     ├── Move to in_progress
              │     ├── agent.process_task(task)
              │     ├── On success → handle_completed_task()
              │     └── On failure → handle_failed_task()
              │           ├── If can_retry → push back to queue
              │           └── Else → push to failed_tasks
              └── sleep(100ms)
```

### Bug: Agent Construction Type Mismatches

The `add_agent()` method in the orchestrator constructs agents with incorrect argument types:

```rust
// Line ~95 in orchestrator.rs — DOES NOT COMPILE
AgentType::LedgerAgent => {
    Arc::new(Mutex::new(crate::accounting::ledger::LedgerAgent::new(
        config.clone(),
        Arc::new(Mutex::new(None))  // ← Expected Ledger, got Arc<Mutex<Option<...>>>
    )))
}
```

The same bug affects ReconciliationAgent, PayrollAgent, TaxAgent, DocumentAgent, and AuditAgent. All receive `Arc<Mutex<None>>` instead of their actual dependencies.

## Task System

### Task Types

```rust
pub enum TaskType {
    RecordTransaction,
    ReconcileAccount,
    GenerateInvoice,
    ProcessPayment,
    CalculatePayroll,
    CalculateTaxes,
    ProcessReceipt,
    StoreDocument,
    RetrieveDocument,
    AuditCheck,
    GenerateReport,
    ValidateData,
    ExportData,
    ImportData,
}
```

### Task Structure

```rust
pub struct Task {
    pub id: Uuid,
    pub task_type: TaskType,
    pub priority: TaskPriority,      // Low=1, Normal=5, High=8, Critical=10
    pub status: TaskStatus,
    pub assigned_agent_type: Option<AgentType>,
    pub assigned_agent_id: Option<Uuid>,
    pub payload: TaskPayload,        // Typed payload enum
    pub result: Option<TaskResult>,
    pub retry_count: usize,
    pub max_retries: usize,          // Default 3
    pub timeout_ms: u64,             // Default 30000
}
```

### Task Payload

```rust
pub enum TaskPayload {
    Empty,
    Transaction(Transaction),
    Account(Account),
    Document(Document),
    String(String),
    Binary(String),             // base64
    Json(serde_json::Value),
    Map(HashMap<String, String>),
    Transactions(Vec<Transaction>),
    Documents(Vec<Document>),
}
```

## Agent Memory System

Each agent has a three-tier memory:

| Tier | Structure | Purpose | Max Entries |
|---|---|---|---|
| Short-term | `VecDeque<MemoryEntry>` | Recent task context | 100 |
| Long-term | `HashMap<String, MemoryEntry>` | Persistent knowledge | 1000 |
| Working | `HashMap<String, serde_json::Value>` | Current task state | Unlimited |

A global `MemoryManager` (backed by `DashMap`) provides:
- Shared memory accessible by all agents
- Tag-based search across short-term, long-term, and shared memory
- Access counting and priority scoring

**Current state:** Memory is created but never populated with meaningful data — no learning or knowledge accumulation occurs.
