# Architecture

## System Architecture Diagram

```
┌──────────────────────────────────────────────────────────────────┐
│                     NEXUSLEDGER SYSTEM                            │
│                                                                   │
│  ┌─────────────────────┐     ┌──────────────────────────────┐    │
│  │   Desktop Client     │     │       NexusLedger Core        │    │
│  │   (Tauri + React)    │────▶│                                │    │
│  │                      │     │  ┌─────────────────────────┐  │    │
│  │  • Dashboard         │     │  │   AgentOrchestrator      │  │    │
│  │  • Account views     │     │  │                          │  │    │
│  │  • Invoice forms     │     │  │  Task Queue ───┐         │  │    │
│  │  • Reports           │     │  │  In-Progress   │         │  │    │
│  │                      │     │  │  Completed     │         │  │    │
│  └─────────────────────┘     │  │  Failed        │         │  │    │
│                               │  │  Memory Manager│         │  │    │
│  ┌─────────────────────┐     │  │  Config Manager│         │  │    │
│  │   API Server         │     │  └───────┬────────┘         │  │    │
│  │   (Axum)             │────▶│          │                  │  │    │
│  │                      │     │    ┌─────┼─────┐            │  │    │
│  │  REST / GraphQL / WS │     │    ▼     ▼     ▼            │  │    │
│  └─────────────────────┘     │  ┌──────┐┌──────┐┌──────┐   │  │    │
│                               │  │Ledger││Recon.││ Tax  │   │  │    │
│                               │  │Agent ││Agent ││Agent │   │  │    │
│                               │  └──┬───┘└──┬───┘└──┬───┘   │  │    │
│                               │     │       │       │        │  │    │
│                               │  ┌──┴───┐┌──┴───┐┌──┴───┐   │  │    │
│                               │  │Invoice││Payroll││Receipt│  │  │    │
│                               │  │Agent  ││Agent  ││Agent  │  │  │    │
│                               │  └──┬───┘└──┬───┘└──┬───┘   │  │    │
│                               │     │       │       │        │  │    │
│                               │  ┌──┴───┐┌──┴───┐┌──┴───┐   │  │    │
│                               │  │Doc    ││Audit ││Report │   │  │    │
│                               │  │Agent  ││Agent ││Agent  │   │  │    │
│                               │  └──────┘└──────┘└──────┘   │  │    │
│                               │                               │  │    │
│                               │  ┌─────────────────────────┐  │    │
│                               │  │   Accounting Engine      │  │    │
│                               │  │                          │  │    │
│                               │  │  Ledger (double-entry)   │  │    │
│                               │  │  Reconciliation          │  │    │
│                               │  │  Tax Calculator          │  │    │
│                               │  │  Payroll Processor       │  │    │
│                               │  └───────────┬─────────────┘  │    │
│                               │              │                 │    │
│                               │  ┌───────────┴─────────────┐  │    │
│                               │  │   Data Layer             │  │    │
│                               │  │                          │  │    │
│                               │  │  SurrealDB (doc-graph)   │  │    │
│                               │  │  Users / Orgs / Docs     │  │    │
│                               │  │  Accounts / Transactions │  │    │
│                               │  │  Audit Logs / Settings   │  │    │
│                               │  └─────────────────────────┘  │    │
│                               │                                │    │
│                               │  ┌─────────────────────────┐  │    │
│                               │  │   AI Service             │  │    │
│                               │  │                          │  │    │
│                               │  │  Ollama (local LLM)      │  │    │
│                               │  │  Text classification     │  │    │
│                               │  │  Data extraction         │  │    │
│                               │  │  Embeddings              │  │    │
│                               │  └─────────────────────────┘  │    │
│                               │                                │    │
│                               │  ┌─────────────────────────┐  │    │
│                               │  │   Edge Manager           │  │    │
│                               │  │                          │  │    │
│                               │  │  Offline mode            │  │    │
│                               │  │  Periodic sync           │  │    │
│                               │  │  Local storage           │  │    │
│                               │  └─────────────────────────┘  │    │
│                               │                                │    │
│                               │  ┌─────────────────────────┐  │    │
│                               │  │   System Monitor         │  │    │
│                               │  │                          │  │    │
│                               │  │  Metrics (counter/gauge) │  │    │
│                               │  │  Alerts (severity-based) │  │    │
│                               │  │  Health checks           │  │    │
│                               │  └─────────────────────────┘  │    │
│                               └────────────────────────────────┘    │
└──────────────────────────────────────────────────────────────────┘
```

## Module Dependency Graph

```
                    ┌─────────────┐
                    │   lib.rs    │  ← Crate root, NexusLedger struct
                    └──────┬──────┘
           ┌───────────────┼───────────────┐
           ▼               ▼               ▼
    ┌──────────┐    ┌──────────┐    ┌──────────┐
    │ agents/  │    │accounting│    │database/ │
    │          │◄───┤          │◄───┤          │
    │orchestr. │    │ ledger   │    │ models   │
    │task      │    │ recon.   │    │ financial│
    │memory    │    │ tax      │    │ error    │
    │config    │    │ payroll  │    │ audit    │
    │status    │    │          │    │ document │
    │error     │    └──────────┘    │ user     │
    │document  │                    └────┬─────┘
    └────┬─────┘                         │
         │          ┌────────────────────┤
         ▼          ▼                    ▼
    ┌──────────┐┌──────────┐    ┌──────────────┐
    │   ai/    ││  audit/  │    │   utils/      │
    │          ││          │    │               │
    │ AIService││AuditAgent│    │ date_utils    │
    │TextClass.││AuditRepo │    │ file_utils    │
    │TextExtr. ││          │    │ validation    │
    └──────────┘└──────────┘    └───────────────┘
         │                            │
         ▼                            ▼
    ┌──────────┐              ┌──────────────┐
    │  api/    │              │   monitor/    │
    │          │              │               │
    │ApiServer │              │SystemMonitor  │
    │ApiHandler│              │Metrics/Alerts │
    └──────────┘              └───────────────┘
         │
         ▼
    ┌──────────┐
    │  edge/   │
    │          │
    │EdgeMgr   │
    │Offline   │
    └──────────┘
```

**Key:** Arrows indicate "imports from". The dependency graph is densely connected — most modules import from `database/` and `agents/`. The `agents/` module imports from `accounting/` (coupling the orchestration layer to the domain logic).

## Data Flow

```
User/API Request
       │
       ▼
  ApiHandler (api/mod.rs)
       │
       ├── GET  /api/v1/accounts  → nexus.ledger.list_accounts()
       ├── GET  /api/v1/transactions → nexus.ledger.list_transactions()
       ├── POST /api/v1/transactions → creates Task → orchestrator.submit_task()
       └── GET  /api/v1/status → orchestrator.get_system_status()
                │
                ▼
         AgentOrchestrator
                │
         find_available_agent(task)
                │
                ▼
           Agent.process_task(task)
                │
                ├── LedgerAgent → Ledger.record_transaction()
                ├── TaxAgent    → TaxCalculator.calculate_tax()
                ├── PayrollAgent→ PayrollProcessor.calculate_payroll()
                └── ...
                        │
                        ▼
                  Database (SurrealDB)
```

## Concurrency Model

Almost every struct field is wrapped in one of:

| Wrapper | Use |
|---|---|
| `Arc<RwLock<T>>` | Shared read-heavy data (accounts map, transactions map) |
| `Arc<Mutex<T>>` | Shared write-heavy data (agent registry, task queues) |
| `Arc<dyn Trait>` | Polymorphic repository injection |

This creates deeply nested lock hierarchies. A typical access chain:

```
orchestrator.agents.read().await   // RwLock<HashMap>
  .get(&agent_id)                  // Option<Arc<Mutex<dyn Agent>>>
  .lock().await                    // Mutex<dyn Agent>
  .process_task(task)              // Agent impl
    → ledger.accounts.write().await // RwLock<BTreeMap>
```

This pattern is safe but creates lock contention and makes the code hard to follow. A future refactor should consider using actor-model patterns (message passing) or reducing the lock granularity.
