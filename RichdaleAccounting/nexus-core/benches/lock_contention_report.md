# Lock Contention Audit Report

**Phase 7, Task 7.1 — Lock Contention Audit**
**Date:** 2026-07-01
**Scope:** `nexus-core/src/` — orchestrator, ledger, API handlers, database layer
**Method:** Static code analysis (read-only). No runtime profiling performed.

---

## Executive Summary

The NexusLedger codebase has **291 lock-acquisition sites** (`.lock().await`, `.write().await`, `.read().await`) across `nexus-core/src/`, backed by **71 `Arc<Mutex<>>` / `Arc<RwLock<>>` declarations**. The dominant contention pattern is **coarse-grained outer mutexes in API handlers** (`Arc<Mutex<NexusLedger>>` and `Arc<Mutex<AgentOrchestrator>>`) that serialize every concurrent request, combined with **locks held across `.await` points** — including database I/O — that extend hold times from microseconds to tens or hundreds of milliseconds.

`dashmap` is already a dependency (declared in `nexus-core/Cargo.toml`) but is used in only one module (`agents/memory.rs`). No other `DashMap` usage exists despite numerous `Arc<RwLock<HashMap>>` patterns that are ideal candidates for replacement.

The `Database` struct (`Arc<Mutex<Option<Surreal<Db>>>>`) is a **global serialization bottleneck** — every SurrealDB query across user, document, audit, and ledger modules acquires the same mutex, making the database effectively single-threaded.

---

## 1. Inventory of All `Arc<Mutex<>>` and `Arc<RwLock<>>` Usages

### 1.1 API State Layer (`api/mod.rs`)

| Field | Type | Line | Purpose |
|-------|------|------|---------|
| `AppState.orchestrator` | `Arc<Mutex<AgentOrchestrator>>` | 118 | Serializes all orchestrator access from API handlers |
| `AppState.database` | `Arc<Mutex<Database>>` | 119 | Serializes all database access from API handlers |
| `AppState.nexus` | `Arc<Mutex<NexusLedger>>` | 120 | Serializes all NexusLedger (ledger + orchestrator) access |

**Key finding:** `NexusLedger` (lib.rs:57) contains both `orchestrator: AgentOrchestrator` and `ledger: Ledger` as plain fields. Wrapping the entire struct in `Arc<Mutex<>>` means a single mutex serializes access to BOTH the ledger and the inner orchestrator. Meanwhile, `AppState.orchestrator` is a *separate* `Arc<Mutex<AgentOrchestrator>>` that shares the same underlying `Arc` state (since `AgentOrchestrator` is `Clone` with `Arc` internals). This creates an asymmetric locking model where some handlers lock `state.nexus` and others lock `state.orchestrator`, but both can touch the same shared inner data.

### 1.2 Orchestrator (`agents/orchestrator.rs`)

| Field | Type | Line | Purpose |
|-------|------|------|---------|
| `agents` | `Arc<RwLock<HashMap<Uuid, Arc<Mutex<dyn Agent>>>>>` | 23 | Agent registry (map of agent ID → agent) |
| `agents_by_type` | `Arc<RwLock<HashMap<AgentType, Vec<Uuid>>>>` | 25 | Type-indexed agent lookup |
| `task_queue` | `Arc<Mutex<VecDeque<Task>>>` | 27 | Pending task queue |
| `in_progress_tasks` | `Arc<Mutex<HashMap<Uuid, Task>>>` | 29 | In-flight tasks |
| `completed_tasks` | `Arc<Mutex<VecDeque<Task>>>` | 31 | Completed task history |
| `failed_tasks` | `Arc<Mutex<VecDeque<Task>>>` | 33 | Failed task history |
| `agent_statuses` | `Arc<RwLock<HashMap<Uuid, AgentStatusInfo>>>` | 39 | Per-agent status tracking |
| `is_running` | `Arc<Mutex<bool>>` | 41 | Orchestrator running flag |

### 1.3 Ledger (`accounting/ledger.rs`)

| Field | Type | Line | Purpose |
|-------|------|------|---------|
| `accounts` | `Arc<RwLock<BTreeMap<Uuid, Account>>>` | 76 | Chart of accounts |
| `transactions` | `Arc<RwLock<BTreeMap<Uuid, Transaction>>>` | 78 | Transaction history |
| `journal_entries` | `Arc<RwLock<BTreeMap<Uuid, JournalEntry>>>` | 80 | Journal entries |
| `current_journal_number` | `Arc<Mutex<u64>>` | 82 | Journal entry counter |
| `current_transaction_number` | `Arc<Mutex<u64>>` | 84 | Transaction counter |

### 1.4 Database (`database/mod.rs`)

| Field | Type | Line | Purpose |
|-------|------|------|---------|
| `Database.client` | `Arc<Mutex<Option<Surreal<Db>>>>` | 39 | SurrealDB client (shared across all repos) |

This same `Arc<Mutex<Option<Surreal<Db>>>>` is cloned into:
- `SurrealUserRepository.db` (database/user.rs:96)
- `SurrealDocumentRepository.db` (database/document.rs:42)
- `SurrealAuditRepository.db` (database/audit.rs:42)
- `DocumentAgent.db` (agents/document.rs:82)

All five structs share the **same underlying mutex**. Every database operation across the entire system serializes through this single lock.

### 1.5 Other Accounting Modules

| Module | Field | Type | Line |
|--------|-------|------|------|
| `invoice.rs` | `InvoiceProcessor.invoices` | `Arc<RwLock<BTreeMap<Uuid, Invoice>>>` | 140 |
| `invoice.rs` | `InvoiceProcessor.current_invoice_number` | `Arc<Mutex<u64>>` | 141 |
| `invoice.rs` | `InvoiceAgent.processor` | `Arc<Mutex<InvoiceProcessor>>` | 495 |
| `receipt.rs` | `ReceiptProcessor.receipts` | `Arc<RwLock<BTreeMap<Uuid, Receipt>>>` | 131 |
| `receipt.rs` | `ReceiptAgent.processor` | `Arc<Mutex<ReceiptProcessor>>` | 280 |
| `payroll.rs` | `PayrollProcessor.employees` | `Arc<RwLock<BTreeMap<Uuid, Employee>>>` | 429 |
| `payroll.rs` | `PayrollProcessor.pay_periods` | `Arc<RwLock<BTreeMap<Uuid, PayPeriod>>>` | 431 |
| `payroll.rs` | `PayrollProcessor.time_entries` | `Arc<RwLock<BTreeMap<Uuid, TimeEntry>>>` | 433 |
| `payroll.rs` | `PayrollProcessor.current_pay_period` | `Arc<Mutex<Option<PayPeriod>>>` | 435 |
| `tax.rs` | `TaxCalculator.jurisdictions` | `Arc<RwLock<BTreeMap<Uuid, TaxJurisdiction>>>` | 311 |
| `tax.rs` | `TaxCalculator.jurisdictions_by_code` | `Arc<RwLock<HashMap<String, Uuid>>>` | 313 |
| `tax.rs` | `TaxCalculator.filings` | `Arc<RwLock<BTreeMap<Uuid, TaxFiling>>>` | 315 |
| `ap.rs` | `ApAgent.processor` | `Arc<Mutex<ApProcessor>>` | 688 |
| `reconciliation.rs` | `ReconciliationProcessor.reconciliations` | `Arc<RwLock<BTreeMap<Uuid, Reconciliation>>>` | 96 |
| `reconciliation.rs` | `ReconciliationProcessor.reconciliations_by_account` | `Arc<RwLock<HashMap<Uuid, Vec<Uuid>>>>` | 98 |
| `reconciliation.rs` | `ReconciliationAgent.processor` | `Arc<Mutex<ReconciliationProcessor>>` | 526 |

### 1.6 Other Infrastructure

| Module | Field | Type | Line |
|--------|-------|------|------|
| `monitor/mod.rs` | `SystemMonitor.metrics` | `Arc<RwLock<HashMap<String, Metric>>>` | 94 |
| `monitor/mod.rs` | `SystemMonitor.historical_metrics` | `Arc<RwLock<Vec<SystemMetrics>>>` | 96 |
| `monitor/mod.rs` | `SystemMonitor.alerts` | `Arc<RwLock<Vec<Alert>>>` | 98 |
| `edge/mod.rs` | `EdgeManager.database` | `Arc<Mutex<Database>>` | 93 |
| `edge/mod.rs` | `EdgeManager.nexus` | `Arc<Mutex<NexusLedger>>` | 95 |
| `edge/mod.rs` | `EdgeManager.last_sync` | `Arc<Mutex<Option<DateTime<Utc>>>>` | 97 |
| `edge/mod.rs` | `EdgeManager.sync_in_progress` | `Arc<Mutex<bool>>` | 99 |
| `edge/mod.rs` | `EdgeSync.edge_manager` | `Arc<Mutex<EdgeManager>>` | 315 |
| `ai/mod.rs` | `AgentOrchestratorAI.orchestrator` | `Arc<Mutex<AgentOrchestrator>>` | 98 |
| `ai/mod.rs` | `DocumentClassifier.ai_service` | `Arc<Mutex<AIService>>` | 386 |
| `ai/mod.rs` | `InformationExtractor.ai_service` | `Arc<Mutex<AIService>>` | 424 |

### 1.7 DashMap Usage (Already Present)

| Module | Field | Type | Line |
|--------|-------|------|------|
| `agents/memory.rs` | `MemoryManager.agent_memories` | `Arc<DashMap<Uuid, AgentMemory>>` | 242 |
| `agents/memory.rs` | `MemoryManager.shared_memory` | `Arc<DashMap<String, MemoryEntry>>` | 244 |

**DashMap is declared as a dependency in `nexus-core/Cargo.toml` but is used in only this one module.** All other map-like collections use `Arc<RwLock<HashMap<>>` or `Arc<RwLock<BTreeMap<>>`.

---

## 2. Top 5 Hot Locks

### Hot Lock #1: `AppState.nexus` — `Arc<Mutex<NexusLedger>>`
- **File:** `api/mod.rs:120`
- **What's protected:** The entire `NexusLedger` struct — both the `Ledger` (accounts, transactions, journal entries) and the inner `AgentOrchestrator` (agents, task queues, statuses)
- **Acquisitions in API handlers:** 20 (lines 453, 478, 505, 543, 633, 684, 799, 1099, 1135, 1210, 1295, 1325, 1375, 1431, 1457, 1484, 1539, 1572, 1987, 2103)
- **Estimated contention level:** **CRITICAL** — Every account, transaction, report, AP, budget, cash flow, CSV import/export, and AR aging request must acquire this mutex. Under concurrent API load, all these request types serialize through a single lock. Estimated hold time: 1–50ms per request (longer if DB I/O is involved).
- **Why it's hot:** A `GET /api/v1/accounts` request blocks a concurrent `POST /api/v1/transactions` even though they access different ledger sub-structures. The mutex is far too coarse-grained.

### Hot Lock #2: `Database.client` — `Arc<Mutex<Option<Surreal<Db>>>>`
- **File:** `database/mod.rs:39`
- **What's protected:** The SurrealDB client connection. Cloned into user, document, audit, and document-agent repositories — all sharing the same mutex.
- **Acquisitions:** 20+ across `database/user.rs` (lines 109, 150, 170, 190, 210, 231, 249, 305, 334, 356, 378, 409), `database/document.rs` (lines 55, 88, 108, 127, 146, 175, 193), `database/audit.rs` (lines 55, 102, 121, 141, 162, 182, 200), `database/mod.rs` (lines 96, 118, 125, 137), plus ledger persistence in `ledger.rs` (via `db.db().await`)
- **Estimated contention level:** **CRITICAL** — Every SurrealDB query across the entire system serializes through this single mutex. The database is effectively single-threaded. Even though SurrealDB's embedded engine is itself concurrent, the application-level mutex prevents any query parallelism.
- **Why it's hot:** A user login (`user.rs`) blocks a concurrent transaction persistence (`ledger.rs`) because they share the same `Database.client` mutex.

### Hot Lock #3: `AppState.orchestrator` — `Arc<Mutex<AgentOrchestrator>>`
- **File:** `api/mod.rs:118`
- **What's protected:** The `AgentOrchestrator` wrapper — agents map, task queues, statuses
- **Acquisitions in API handlers:** 14 (lines 397, 430, 665, 762, 779, 849, 2036, 2054, 2072, 2089, 2167, 2185, 2199, 2212)
- **Estimated contention level:** **HIGH** — All status, agent-list, task-submission, and WebSocket chat requests serialize through this lock. The `agents_handler` is particularly bad: it holds this lock AND acquires `agents.read()` AND calls `agent.blocking_lock()` on every agent (blocking the async thread).
- **Why it's hot:** Since `AgentOrchestrator` uses `Arc` internally for all fields, the `state.orchestrator` mutex and the `state.nexus` mutex protect overlapping data. A handler using `state.orchestrator` can contend with the processing loop running inside `state.nexus`'s orchestrator.

### Hot Lock #4: `Ledger.accounts` — `Arc<RwLock<BTreeMap<Uuid, Account>>>`
- **File:** `accounting/ledger.rs:76`
- **What's protected:** The chart of accounts (account ID → Account)
- **Acquisitions:** 20+ across `ledger.rs` (lines 114, 115, 157, 167, 179, 222, 228, 234, 240, 256, 274, 286, 395, 436, 493, 502, 508), `ap.rs` (lines 236, 790, 910, 956, 989, 1009, 1071, 1110, 1128, 1130), `budget.rs` (lines 249, 251), `api/mod.rs` (lines 480, 1099, 1135, 1210, 1295, 1484, 1539)
- **Estimated contention level:** **HIGH** — Every account lookup, balance update, and report generation touches this lock. Write locks (`accounts.write()`) are held during `record_transaction` → `update_account_balances` and during `create_account` (across DB persistence). Read locks are held during iteration in `list_accounts`, `get_balance_sheet`, `get_trial_balance`, and account-by-number lookups in AP/budget handlers.
- **Why it's hot:** The `create_account` function holds a write lock across DB I/O. The `record_transaction` function holds a write lock (via `update_account_balances`) while also holding `current_transaction_number.lock()`.

### Hot Lock #5: `Orchestrator.agents` — `Arc<RwLock<HashMap<Uuid, Arc<Mutex<dyn Agent>>>>>`
- **File:** `agents/orchestrator.rs:23`
- **What's protected:** The agent registry — a map from agent ID to `Arc<Mutex<dyn Agent>>`
- **Acquisitions:** 10+ across `orchestrator.rs` (lines 120, 202, 238, 270, 300, 330, 370, 430, 431, 760), `api/mod.rs` (line 431), `main.rs` (line 39), `lib.rs` (line 82)
- **Estimated contention level:** **HIGH** — The read lock is held during agent iteration in `find_available_agent`, `initialize_all_agents`, `stop`, and `agents_handler`. The write lock is held during `add_agent` and `remove_agent`. Critically, the read lock is held across `agent.lock().await` and `agent_guard.process_task().await` in `assign_task_to_agent` — meaning the agents map is locked for reading during the entire task processing duration.
- **Why it's hot:** The `Arc<Mutex<dyn Agent>>` inner lock creates a nested lock pattern. The `agents_handler` uses `blocking_lock()` (synchronous) inside an async context, which can stall the Tokio worker thread if an agent is busy.

---

## 3. Lock Nesting Chains

### Chain 1 (Depth 5 — CRITICAL)

**Location:** `orchestrator.rs` — `assign_task_to_agent()` → `handle_completed_task()`

```
agents.read().await                    [orchestrator.rs:370 — READ lock on agent map]
  └─ agent.lock().await                [orchestrator.rs:371 — Mutex on specific agent]
       └─ in_progress_tasks.lock().await  [orchestrator.rs:365 — before agent lock, but agent lock outlives]
       └─ process_task(task).await     [orchestrator.rs:397 — agent lock STILL HELD]
       └─ handle_completed_task()      [orchestrator.rs:399 — called while agent lock STILL HELD]
            └─ in_progress_tasks.lock().await   [orchestrator.rs:413]
            └─ completed_tasks.lock().await     [orchestrator.rs:416]
            └─ agent_statuses.write().await     [orchestrator.rs:419]
```

The `agents.read()` guard and `agent.lock()` guard are both alive through `process_task().await` and `handle_completed_task().await`. This is the deepest nesting chain in the codebase. Five locks deep, with the agent mutex held across an `.await` that performs arbitrary accounting work + DB I/O.

### Chain 2 (Depth 4 — CRITICAL)

**Location:** `api/mod.rs` — `agents_handler()`

```
state.orchestrator.lock().await        [api/mod.rs:430 — Mutex on orchestrator wrapper]
  └─ orchestrator.agents.read().await  [api/mod.rs:431 — READ lock on agent map]
       └─ agent.blocking_lock()        [api/mod.rs:435 — SYNC Mutex on each agent (in .map())]
            └─ (agent.config() / agent.status() — reads agent state)
```

Three locks held simultaneously, with `blocking_lock()` stalling the Tokio worker thread. If any agent is being processed by `assign_task_to_agent` (which holds `agent.lock()`), this handler blocks the entire async worker thread until that agent finishes processing.

### Chain 3 (Depth 4 — CRITICAL)

**Location:** `orchestrator.rs` — `assign_task_to_agent()` → `process_task()` → Ledger operations

```
agents.read().await                    [orchestrator.rs:370]
  └─ agent.lock().await                [orchestrator.rs:371]
       └─ process_task().await         [orchestrator.rs:397 — calls agent.process_task()]
            └─ ledger.record_transaction()  [ledger.rs:278]
                 ├─ current_transaction_number.lock().await  [ledger.rs:286]
                 ├─ accounts.write().await                   [ledger.rs:436 (via update_account_balances)]
                 ├─ transactions.write().await               [ledger.rs:299, 306]
                 ├─ current_journal_number.lock().await      [ledger.rs:449]
                 ├─ journal_entries.write().await            [ledger.rs:465]
                 └─ db.client.lock().await                   [database/mod.rs:137 (via db.db().await)]
```

The agent mutex is held across the full transaction recording pipeline, which itself acquires 6 additional locks. Total depth: 4 (agents.read → agent.lock → process_task → ledger locks). The agent lock is held for the entire duration of DB I/O.

### Chain 4 (Depth 3 — HIGH)

**Location:** `ledger.rs` — `record_transaction()`

```
current_transaction_number.lock().await   [ledger.rs:286 — Mutex on counter]
  ├─ accounts.write().await                [ledger.rs:436 (via update_account_balances)]
  ├─ transactions.write().await            [ledger.rs:299]
  ├─ current_journal_number.lock().await   [ledger.rs:449 (via create_journal_entry)]
  ├─ journal_entries.write().await         [ledger.rs:465 (via create_journal_entry)]
  ├─ transactions.write().await            [ledger.rs:306 — second insert]
  └─ db.client.lock().await                [database/mod.rs:137 (via db.db().await) — multiple queries]
```

The `current_transaction_number` mutex is held for the **entire** `record_transaction` function — there is no `drop(counter)` call. This means the transaction counter lock serializes all transaction recording, including DB persistence that could take 10–100ms+ per transaction.

### Chain 5 (Depth 3 — HIGH)

**Location:** `api/mod.rs` — `ap_create_bill_handler()`

```
state.nexus.lock().await               [api/mod.rs:1135 — Mutex on NexusLedger]
  ├─ nexus.ledger.accounts.read().await [api/mod.rs:1137 — scoped block, released]
  ├─ nexus.ledger.accounts.read().await [api/mod.rs:1142 — scoped block, released]
  └─ nexus.process_transaction().await  [api/mod.rs:1210]
       └─ orchestrator.process_transaction()  [orchestrator.rs:155]
            └─ submit_task().await      [orchestrator.rs:162]
                 └─ task_queue.lock().await  [orchestrator.rs:163]
```

The `state.nexus` mutex is held across `process_transaction()`, which delegates to the orchestrator's task submission pipeline. The nexus lock is also held during account lookups (though those are in scoped blocks that release the inner lock).

### Chain 6 (Depth 3 — MODERATE)

**Location:** `ledger.rs` — `get_income_statement()`

```
transactions.read().await              [ledger.rs:534 — READ lock on transactions]
  └─ self.get_account().await          [ledger.rs:545 — called in a loop for each entry]
       └─ accounts.read().await        [ledger.rs:493 (inside get_account)]
```

Read-read nesting. The `transactions` read lock is held across multiple `accounts.read()` acquisitions (one per transaction entry). This extends the hold time of the transactions lock proportionally to the number of transactions.

### Chain 7 (Depth 3 — MODERATE)

**Location:** `orchestrator.rs` — `initialize_all_agents()`

```
agents.read().await                    [orchestrator.rs:120 — READ lock on agent map]
  └─ agent.lock().await                [orchestrator.rs:122 — Mutex on each agent]
       └─ agent.initialize().await     [orchestrator.rs:123 — may do DB I/O]
  └─ agent_statuses.write().await      [orchestrator.rs:128 — WRITE lock, still inside agents.read()]
```

The `agents` read lock is held across the initialization of ALL agents (sequential loop) and the status map write. Agent initialization may perform DB I/O (e.g., `LedgerAgent.initialize()` calls `Ledger.initialize()` which creates 20+ accounts with DB persistence).

---

## 4. Locks Held Across `.await` Points

This is the most impactful contention pattern. When a lock guard is alive across an `.await`, the lock is held during whatever the `.await` does — which may include DB I/O, network calls, or long computations. This blocks all other tasks that need the same lock.

| # | Lock | File:Line | Held Across | Estimated Hold Time | Severity |
|---|------|-----------|-------------|---------------------|----------|
| 1 | `current_transaction_number.lock()` | ledger.rs:286 | Entire `record_transaction()` — balance updates, journal entry creation, DB persistence (multiple SurrealDB queries) | 10–100ms+ | CRITICAL |
| 2 | `agents.read()` + `agent.lock()` | orchestrator.rs:370–371 | `process_task().await` + `handle_completed_task().await` / `handle_failed_task().await` | 50–500ms+ | CRITICAL |
| 3 | `state.nexus.lock()` | api/mod.rs (20 sites) | `nexus.ledger.list_accounts().await`, `nexus.process_transaction().await`, `nexus.ledger.get_balance_sheet().await`, etc. | 1–50ms | HIGH |
| 4 | `state.orchestrator.lock()` | api/mod.rs (14 sites) | `orchestrator.get_system_status().await`, `orchestrator.submit_task().await` | 1–10ms | HIGH |
| 5 | `accounts.write()` | ledger.rs:436 (via `update_account_balances`) | Balance update loop (in-memory, fast) | <1ms | LOW |
| 6 | `accounts.write()` | ledger.rs:179 (in `create_account`) | DB persistence query (`db.db().await` + `client.query().await`) | 1–10ms | MODERATE |
| 7 | `transactions.read()` | ledger.rs:534 (in `get_income_statement`) | Loop of `self.get_account().await` calls | 1–10ms (scales with txn count) | MODERATE |
| 8 | `agents.read()` | orchestrator.rs:120 (in `initialize_all_agents`) | Loop of `agent.lock()` + `agent.initialize().await` | 100ms–seconds | HIGH |
| 9 | `agents.read()` | orchestrator.rs:760 (in `stop`) | Loop of `agent.lock()` + `agent.shutdown().await` | 100ms–seconds | HIGH |
| 10 | `agent_statuses.write()` | orchestrator.rs:128 (in `initialize_all_agents`) | Held across `statuses.insert()` only (fast) | <1ms | LOW |
| 11 | `agents.read()` | orchestrator.rs:430 (in `agents_handler` via API) | Loop of `agent.blocking_lock()` (sync) | 5–50ms | HIGH |

---

## 5. Average Lock Hold Time Estimation

Based on code-path analysis (no runtime profiling). Estimates assume in-memory SurrealDB (kv-mem). Production with persistent storage would be 5–50x slower for DB-bound paths.

| Lock | Typical Hold Time | Bottleneck During Hold |
|------|-------------------|------------------------|
| `state.nexus.lock()` | 1–50ms | All ledger + nexus.orchestrator operations |
| `state.orchestrator.lock()` | 1–10ms (status/submit) to 5–50ms (agents_handler) | All orchestrator operations |
| `Database.client.lock()` | 0.1–5ms per query | All DB operations system-wide |
| `Ledger.accounts.read()` | <1ms (single lookup) to 1–5ms (full iteration) | Account reads |
| `Ledger.accounts.write()` | <1ms (balance update) to 5–20ms (create_account w/ DB) | Account writes |
| `Ledger.transactions.read()` | 1–10ms (scales with txn count) | Transaction reads |
| `Ledger.transactions.write()` | <1ms (insert) | Transaction writes |
| `current_transaction_number.lock()` | **10–100ms+** (held across entire record_transaction) | All transaction recording |
| `current_journal_number.lock()` | <1ms (counter increment) | Journal entry numbering |
| `agents.read()` | <1ms (lookup) to **100ms–seconds** (init/stop loops) | Agent registry reads |
| `agent.lock()` | **50–500ms+** (held across process_task) | Per-agent task processing |
| `task_queue.lock()` | <1ms (push/pop) | Task queue operations |
| `agent_statuses.read()/write()` | <1ms (HashMap op) | Status tracking |

---

## 6. `Arc<RwLock<>>` Nesting Deeper Than 3 Levels

### CRITICAL: Chain 1 — Depth 5
```
agents.read() [RwLock READ]
  → agent.lock() [Mutex]
    → process_task() / handle_completed_task()
      → in_progress_tasks.lock() [Mutex]
        → completed_tasks.lock() [Mutex]
          → agent_statuses.write() [RwLock WRITE]
```
**File:** `orchestrator.rs:370–430`
**Risk:** If any code path reverses this acquisition order (e.g., acquires `agent_statuses.write()` before `agents.read()`), a deadlock is possible. The current code acquires in a consistent order, but the depth makes it fragile.

### CRITICAL: Chain 3 — Depth 4
```
agents.read() [RwLock READ]
  → agent.lock() [Mutex]
    → process_task()
      → current_transaction_number.lock() [Mutex]
        → accounts.write() [RwLock WRITE]
```
**File:** `orchestrator.rs:370` → `ledger.rs:286,436`
**Risk:** The `agents.read()` (RwLock read) is held while `accounts.write()` (RwLock write) is acquired. If another path writes to `agents` while holding a read lock on `accounts`, a RwLock writer starvation or deadlock could occur.

### CRITICAL: Chain 2 — Depth 4
```
state.orchestrator.lock() [Mutex]
  → agents.read() [RwLock READ]
    → agent.blocking_lock() [Mutex — SYNC, not async]
      → (agent state read)
```
**File:** `api/mod.rs:430–435`
**Risk:** `blocking_lock()` inside an async context can stall the Tokio worker thread. If the agent is being processed by the orchestrator loop (which holds `agent.lock()`), this handler blocks the thread until processing completes. Under load, this can exhaust the thread pool.

---

## 7. Database Access Bottleneck Assessment

### The Problem

`Database.client` is `Arc<Mutex<Option<Surreal<Db>>>>` (`database/mod.rs:39`). This single mutex is shared across:

1. **`SurrealUserRepository`** — 12 methods, each acquiring `self.db.lock().await` (user.rs:109–409)
2. **`SurrealDocumentRepository`** — 7 methods, each acquiring `self.db.lock().await` (document.rs:55–193)
3. **`SurrealAuditRepository`** — 7 methods, each acquiring `self.db.lock().await` (audit.rs:55–200)
4. **`Database` itself** — 4 methods (connect, disconnect, is_connected, db) (mod.rs:96–137)
5. **Ledger persistence** — `record_transaction()` calls `db.db().await` which locks, then runs multiple queries while the lock is released (the `db()` method clones the Surreal client out of the lock)

### Key Observation: `db()` Method Clones the Client

```rust
pub async fn db(&self) -> Result<Surreal<Db>, DatabaseError> {
    let client = self.client.lock().await;   // Lock acquired
    client.clone().ok_or(DatabaseError::NotInitialized)  // Clone, then guard drops
}
```

The `db()` method acquires the lock, clones the `Surreal<Db>` (which is `Arc` internally), and immediately releases the lock. The actual queries then run without holding the `Database.client` mutex. **This mitigates the bottleneck for ledger persistence** — the lock is only held for the clone, not for the query duration.

However, the user/document/audit repositories use a different pattern:

```rust
// user.rs:109
let guard = self.db.lock().await;  // Lock held for ENTIRE query
let result = guard.as_ref().unwrap().query("...").await;  // .await while lock held!
```

These repositories hold the lock across the entire `.await` of the SurrealDB query. This is the real bottleneck: user authentication, document storage, and audit logging all serialize through the same mutex, with each holding the lock for the full query duration.

### Assessment

| Aspect | Status |
|--------|--------|
| Is DB access a bottleneck? | **YES** — for user/document/audit repositories |
| Is DB access a bottleneck for ledger? | **PARTIALLY** — `db()` clones and releases, but `create_account` holds `accounts.write()` across DB I/O |
| Connection pooling needed? | **YES** — see Task 7.2 recommendation below |
| Current concurrency model | Single-connection, mutex-serialized |
| Estimated max DB throughput | ~100–500 queries/sec (limited by mutex serialization) |

---

## 8. Orchestrator Mutex Lock Count in API Handlers

From `api/mod.rs`:

| Mutex | Lock Count | Handler Functions |
|-------|------------|-------------------|
| `state.orchestrator.lock().await` | **14** | `status_handler`, `agents_handler`, `create_invoice_handler`, `submit_task_handler`, `task_queue_handler`, `health_handler`, `handle_create_invoice` (NLU), `handle_process_payment` (NLU), `handle_record_receipt` (NLU), `handle_record_transaction` (NLU), `handle_reconcile` (NLU), `handle_calculate_tax` (NLU), `handle_run_payroll` (NLU), `handle_status` (NLU) |
| `state.nexus.lock().await` | **20** | `accounts_handler`, `account_by_id_handler`, `transactions_handler`, `transaction_by_id_handler`, `create_transaction_handler`, `invoices_handler`, `report_handler`, `ap_bills_handler`, `ap_create_bill_handler`, `ap_pay_bill_handler`, `ap_outstanding_handler`, `ar_aging_handler`, `cash_flow_report_handler`, `csv_import_handler`, `csv_export_handler`, `ofx_export_handler`, `create_budget_handler`, `budget_variance_handler`, `handle_query_balance` (NLU), `handle_generate_report` (NLU) |
| `state.database.lock().await` | **0** | Not directly locked in API handlers (DB accessed via `state.nexus` → `nexus.orchestrator.database` or via `state.user_repo`) |
| **Total** | **34** | |

**Every API request** (except auth and static asset endpoints) acquires at least one of these two top-level mutexes. Under concurrent HTTP load, all requests serialize through `state.nexus` or `state.orchestrator`.

---

## 9. Suggested Fixes

### Priority 1: Eliminate Coarse-Grained API Mutexes

**Problem:** `Arc<Mutex<NexusLedger>>` and `Arc<Mutex<AgentOrchestrator>>` in `AppState` serialize all API requests.

**Fix:** Remove the outer `Mutex` wrappers. Since `NexusLedger.ledger` and `NexusLedger.orchestrator` already use `Arc<RwLock<>>` / `Arc<Mutex<>>` internally for all their fields, the outer mutex is redundant — it just adds a serialization layer on top of already-thread-safe data.

```rust
// BEFORE
pub struct AppState {
    pub orchestrator: Arc<Mutex<AgentOrchestrator>>,
    pub nexus: Arc<Mutex<NexusLedger>>,
    // ...
}

// AFTER
pub struct AppState {
    pub orchestrator: Arc<AgentOrchestrator>,  // AgentOrchestrator is already Clone with Arc internals
    pub ledger: Arc<Ledger>,                    // Ledger is already Clone with Arc internals
    // ...
}
```

**Impact:** Removes 34 serialization points. Each API handler would acquire only the specific inner lock it needs (e.g., `accounts.read()` for `GET /accounts`), allowing concurrent access to different ledger sub-structures.

**Risk:** Medium — requires audit of all `&mut self` methods on `AgentOrchestrator` and `Ledger` to ensure they use interior mutability (they already do for most operations, but `initialize()` and `add_agent()` take `&mut self`).

### Priority 2: Replace `Arc<RwLock<HashMap<>>` with `DashMap`

**Problem:** 15+ `Arc<RwLock<HashMap<>>` / `Arc<RwLock<BTreeMap<>>` patterns across the codebase. `RwLock` allows concurrent reads but serializes writes, and the lock is held for the entire operation even for a single-key access.

**Fix:** Replace with `DashMap` (already a dependency). `DashMap` uses shard-level locking, allowing truly concurrent reads and writes to different keys.

**High-value targets:**
1. `Orchestrator.agents` (`Arc<RwLock<HashMap<Uuid, Arc<Mutex<dyn Agent>>>>>`) → `DashMap<Uuid, Arc<Mutex<dyn Agent>>>`
2. `Orchestrator.agent_statuses` (`Arc<RwLock<HashMap<Uuid, AgentStatusInfo>>>`) → `DashMap<Uuid, AgentStatusInfo>`
3. `Orchestrator.agents_by_type` (`Arc<RwLock<HashMap<AgentType, Vec<Uuid>>>>`) → `DashMap<AgentType, Vec<Uuid>>`
4. `Ledger.accounts` (`Arc<RwLock<BTreeMap<Uuid, Account>>>`) → `DashMap<Uuid, Account>` (lose sorting, but add concurrent access)
5. `Ledger.transactions` (`Arc<RwLock<BTreeMap<Uuid, Transaction>>>`) → `DashMap<Uuid, Transaction>`
6. `SystemMonitor.metrics` (`Arc<RwLock<HashMap<String, Metric>>>`) → `DashMap<String, Metric>`

**Impact:** Eliminates read/write contention on the most frequently accessed maps. `DashMap` also eliminates the "lock held across `.await`" problem for simple get/insert operations (each `DashMap` entry access locks only one shard, not the entire map).

### Priority 3: Fix Locks Held Across `.await` in `record_transaction`

**Problem:** `current_transaction_number.lock()` is held across the entire `record_transaction()` function — including DB I/O — because there is no `drop(counter)` call.

**Fix:** Acquire the counter, increment it, release it immediately:

```rust
// BEFORE (ledger.rs:286)
let mut counter = self.current_transaction_number.lock().await;
transaction.number = format!("TRX-{:08}", *counter);
*counter += 1;
// ... 100+ lines of work with lock still held ...

// AFTER
{
    let mut counter = self.current_transaction_number.lock().await;
    transaction.number = format!("TRX-{:08}", *counter);
    *counter += 1;
} // Lock released here
// ... rest of function without holding the counter lock ...
```

**Impact:** Reduces `current_transaction_number` hold time from 10–100ms+ to <1μs. Eliminates the depth-3 nesting chain.

### Priority 4: Fix Locks Held Across `.await` in `assign_task_to_agent`

**Problem:** `agents.read()` and `agent.lock()` are held across `process_task().await` — the entire task execution, which may include DB I/O and ledger operations.

**Fix:** Clone the `Arc<Mutex<dyn Agent>>` out of the map, release the map read lock, then lock the agent:

```rust
// BEFORE
if let Some(agent) = self.agents.read().await.get(&agent_id) {
    let agent_guard = agent.lock().await;
    // ... process_task with both locks held ...
}

// AFTER
let agent_arc = {
    let agents = self.agents.read().await;
    agents.get(&agent_id).cloned()
};
if let Some(agent_arc) = agent_arc {
    let agent_guard = agent_arc.lock().await;
    // ... process_task with only agent lock held ...
}
```

**Impact:** Reduces nesting from depth 4–5 to depth 2. The `agents` map is no longer locked during task processing, allowing other operations (e.g., `find_available_agent`) to run concurrently.

### Priority 5: Fix User/Document/Audit Repository DB Lock Pattern

**Problem:** Repositories hold `self.db.lock().await` across the entire SurrealDB query `.await`.

**Fix:** Use the same clone-and-release pattern as `Database::db()`:

```rust
// BEFORE (user.rs:109)
let guard = self.db.lock().await;
let result = guard.as_ref().unwrap().query("...").await;

// AFTER
let client = self.db.lock().await.clone().ok_or(DatabaseError::NotInitialized)?;
let result = client.query("...").await;
```

**Impact:** Reduces `Database.client` lock hold time from full-query-duration to clone-only (<1μs). Enables concurrent DB queries across all repositories.

### Priority 6: Replace `blocking_lock()` in `agents_handler`

**Problem:** `agent.blocking_lock()` (sync mutex) is called inside an async handler, which can stall the Tokio worker thread.

**Fix:** Use `agent.lock().await` (async mutex) instead, or better, cache agent metadata (config + status) in `agent_statuses` so the handler doesn't need to lock each agent:

```rust
// Option A: Use async lock
let agents_data: Vec<serde_json::Value> = agents.values()
    .map(|agent| {
        let guard = agent.lock().await;  // Changed from blocking_lock()
        // ...
    })
    .collect();

// Option B: Read from agent_statuses (no agent lock needed)
let statuses = self.agent_statuses.read().await;
let agents_data: Vec<_> = statuses.values().map(|s| {
    serde_json::json!({ "id": s.agent_id, "status": s.status, ... })
}).collect();
```

### Priority 7: Consider Actor Model for Orchestrator Processing Loop

**Problem:** The orchestrator's `start()` loop and `assign_task_to_agent()` create deep lock nesting and hold locks across long-running task processing.

**Fix:** Use a channel-based actor model where the orchestrator owns its state and communicates via message passing:

```rust
enum OrchestratorCommand {
    SubmitTask(Task),
    GetStatus(tokio::sync::oneshot::Sender<SystemStatus>),
    // ...
}

let (tx, mut rx) = mpsc::channel(100);
tokio::spawn(async move {
    let mut state = OrchestratorState::new();  // Owned, no locks needed
    while let Some(cmd) = rx.recv().await {
        match cmd {
            OrchestratorCommand::SubmitTask(task) => { state.submit(task).await }
            OrchestratorCommand::GetStatus(reply) => { let _ = reply.send(state.status()); }
        }
    }
});
```

**Impact:** Eliminates all orchestrator locks. State is owned by the actor task, accessed only via channels. No lock contention, no nesting, no deadlocks. This is a larger refactor but aligns with the "agentic" architecture vision.

---

## 10. Summary of Critical Findings

| # | Finding | Severity | Impact |
|---|---------|----------|--------|
| 1 | `state.nexus` and `state.orchestrator` are coarse-grained mutexes that serialize all API requests | CRITICAL | Max throughput = 1 concurrent API request per mutex |
| 2 | `Database.client` mutex serializes all DB queries across user/document/audit repos | CRITICAL | Database is single-threaded despite embedded SurrealDB being concurrent |
| 3 | `current_transaction_number.lock()` held across entire `record_transaction()` including DB I/O | CRITICAL | All transaction recording is serialized |
| 4 | `agents.read()` + `agent.lock()` held across `process_task().await` | CRITICAL | Agent map locked during task execution; depth-5 nesting |
| 5 | `blocking_lock()` used in async `agents_handler` | HIGH | Can stall Tokio worker threads |
| 6 | Lock nesting up to depth 5 in `assign_task_to_agent` → `handle_completed_task` | CRITICAL | Deadlock risk if acquisition order changes |
| 7 | `DashMap` dependency exists but is used in only 1 module | MODERATE | Easy win — many `Arc<RwLock<HashMap>>` patterns are ready for replacement |
| 8 | User/document/audit repos hold DB lock across full query `.await` | HIGH | All DB operations serialize through one mutex |
| 9 | 34 top-level mutex acquisitions across API handlers | HIGH | Every API request waits on a global lock |
| 10 | `agents.read()` held across all agent `initialize()`/`shutdown()` calls | HIGH | Orchestrator startup/shutdown blocks all agent map access for seconds |

---

## 11. Recommendations for Task 7.2 (Connection Pooling)

1. **Replace `Arc<Mutex<Option<Surreal<Db>>>>` with a connection pool.** SurrealDB's `Surreal<Db>` is already `Clone` (internally `Arc`), so the current `Mutex` adds no safety — it only serializes. Replace with a bare `Surreal<Db>` (no `Option`, no `Mutex`), initialized once at startup.

2. **If a remote SurrealDB server is used in production**, create a pool of `Surreal<ws::Client>` connections. Use `deadpool` or `bb8` for connection pooling. Each API handler checks out a connection, runs its query, and returns it.

3. **Fix the repository lock pattern** (Priority 5 above) regardless of pooling decision — the clone-and-release pattern works with both embedded and pooled connections.

4. **Separate read and write connections** if SurrealDB supports read replicas. Read-heavy operations (account listing, report generation, transaction listing) can use read replicas, while writes (transaction recording, account creation) use the primary.

5. **Consider moving DB I/O out of lock-held code paths entirely.** The pattern in `record_transaction` — hold lock, do in-memory mutation, release lock, then persist to DB — would eliminate the most impactful lock-held-across-await instances.
