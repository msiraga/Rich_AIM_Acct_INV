# Phase 2: Real Agent Engine

**Objective:** Replace all mock agent logic with real processing. Every agent must execute its domain logic and persist results to SurrealDB. Implement the two missing agent types (InvoiceAgent, ReceiptAgent).  
**Duration:** 3–4 weeks  
**Depends on:** Phase 1 (freeze token satisfied)  
**Blocks:** Phase 3  

---

## Why This Phase Exists

After Phase 1, the database is wired but agents still return mock data. `LedgerAgent.process_task()` doesn't actually use the real ledger. `TaxAgent` hardcodes a US-FED calculation on $10,000 regardless of input. `InvoiceAgent` and `ReceiptAgent` don't exist — they're aliased to `LedgerAgent` and `DocumentAgent` respectively. `ReportingAgent` was fixed in Phase 0 but has no logic. This phase makes every agent do real work.

---

## Task List

| ID | Task | Primary File(s) | Depends On | Parallel With |
|---|---|---|---|---|
| 2.1 | **Real `LedgerAgent.process_task()`** — extract `Transaction` from `TaskPayload`, call `ledger.record_transaction()`, return the recorded transaction with updated balances and journal entry ID. | `accounting/ledger.rs` | P1 | 2.2, 2.3, 2.4, 2.8 |
| 2.2 | **Real `ReconciliationAgent.process_task()`** — extract account + statement data, call `reconciliation_processor.reconcile_account()`, return reconciliation result with matched/unmatched transactions. | `accounting/reconciliation.rs` | P1 | 2.1, 2.3, 2.4 |
| 2.3 | **Real `TaxAgent.process_task()`** — extract jurisdiction, tax type, amount from `TaskPayload`, call `tax_calculator.calculate_tax()`, return calculation result. | `accounting/tax.rs` | P1 | 2.1, 2.2, 2.4 |
| 2.4 | **Real `PayrollAgent.process_task()`** — extract employee ID + pay period, call `payroll_processor.calculate_payroll()`, return full calculation with deductions and net pay. | `accounting/payroll.rs` | P1, 2.1 | 2.2, 2.3 |
| 2.5 | **Create `InvoiceAgent`** — new agent that creates invoices, tracks payment status, generates invoice documents. Currently mapped to `LedgerAgent` (wrong). Needs its own struct, its own `process_task()` for `GenerateInvoice` and `ProcessPayment` task types. | `accounting/invoice.rs` (new), `agents/orchestrator.rs` | P1, 2.1 | 2.6, 2.7 |
| 2.6 | **Create `ReceiptAgent`** — new agent that processes receipts (raw document → categorized expense → transaction). Currently mapped to `DocumentAgent` (wrong). | `accounting/receipt.rs` (new), `agents/orchestrator.rs` | P1, 2.1 | 2.5, 2.7 |
| 2.7 | **Create `ReportingAgent`** — new agent that queries the ledger and generates: Trial Balance, Balance Sheet, Income Statement. Currently mapped to `LedgerAgent` (wrong). | `accounting/reporting.rs` (new) | P1, 2.1 | 2.5, 2.6 |
| 2.8 | **Real `AuditAgent.process_task()`** — perform actual audit checks: verify transaction balance, check account existence, log audit entries with old/new values. | `audit/mod.rs` | P1 | 2.1, 2.2, 2.3 |
| 2.9 | **Real `DocumentAgent.process_task()`** — store documents to SurrealDB via `SurrealDocumentRepository`, retrieve by ID, classify by document type. | `agents/document.rs` | P1 | 2.8 |
| 2.10 | **Real task dispatch loop** — replace the `sleep(100ms)` busy-wait with proper async notification. When a task is submitted, the orchestrator immediately dispatches it. Use `tokio::sync::Notify` or a channel. | `agents/orchestrator.rs` | 2.1–2.9 | Nothing (integration) |
| 2.11 | **Integration test** — submit `RecordTransaction` task → `LedgerAgent` processes → verify transaction + journal entry + updated balances in SurrealDB. | `tests/integration/agents.rs` (new) | 2.10 | Nothing (final gate) |

---

## Dependency Graph

```
                          P1 (freeze token)
                               │
              ┌────────────────┼────────────────┐
              │                │                 │
       ┌──────┴──┐     ┌──────┴──┐       ┌─────┴─────┐
       │ Track A  │     │ Track B  │       │ Track C   │
       │ (core)   │     │ (domain) │       │ (new)     │
       │          │     │          │       │           │
       │  2.1     │     │ 2.2      │       │ 2.5       │
       │  2.8     │     │ 2.3      │       │ 2.6       │
       │  2.9     │     │ 2.4      │       │ 2.7       │
       │          │     │          │       │           │
       └────┬─────┘     └────┬─────┘       └─────┬─────┘
            │                │                    │
            └────────────────┼────────────────────┘
                          ┌──┴──┐
                          │ 2.10│  Real dispatch loop
                          └──┬──┘
                          ┌──┴──┐
                          │ 2.11│  Integration test
                          └─────┘
```

---

## Parallel Execution Strategy

**Session 1 (All parallel after P1):**
- Track A: 2.1 + 2.8 + 2.9 (core agent logic fixes)
- Track B: 2.2 + 2.3 + 2.4 (domain agent logic — all independent)
- Track C: 2.5 + 2.6 + 2.7 (new agents — all independent)

**Session 2 (Integration, after all tracks complete):**
- 2.10 → 2.11

---

## Freeze Token 2 🔒

All conditions must be true:

- [ ] `LedgerAgent.process_task()` records a real transaction: debits/credits applied, journal entry created, all persisted to SurrealDB
- [ ] `ReconciliationAgent.process_task()` matches statement transactions against book transactions, returns matched/unmatched lists
- [ ] `TaxAgent.process_task()` calculates tax from task payload (jurisdiction + amount extracted from payload, not hardcoded)
- [ ] `PayrollAgent.process_task()` calculates payroll from real employee + time entry data
- [ ] `InvoiceAgent` exists as its own struct, processes `GenerateInvoice` tasks, creates invoice records in SurrealDB
- [ ] `ReceiptAgent` exists as its own struct, processes `ProcessReceipt` tasks, creates expense transactions
- [ ] `ReportingAgent` exists as its own struct, generates Trial Balance / Balance Sheet / Income Statement from real ledger data
- [ ] `AuditAgent.process_task()` logs real audit entries with entity type, action, and old/new values
- [ ] `DocumentAgent.process_task()` stores/retrieves documents from SurrealDB
- [ ] Task dispatch is event-driven (no busy-wait sleep loop)
- [ ] Integration test: submit `RecordTransaction` → verify transaction + journal entry + balances in SurrealDB
- [ ] `cargo test` passes

---

## Notes for Reviewer

- This phase is the heaviest — 11 tasks, 3–4 weeks
- The three new agents (Invoice, Receipt, Reporting) are new code, not refactors
- Agent memory (`MemoryManager`) exists but is not populated with meaningful data in this phase — that's deferred to Phase 5 (AI)
- Task retry logic (already coded in orchestrator) will be exercised for the first time with real failures
