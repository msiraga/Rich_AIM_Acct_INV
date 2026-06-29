# Phase 1: Database & Wire

**Objective:** Connect to SurrealDB. Create schema definitions. Wire the database into every agent and repository so they read/write real data instead of mock responses.  
**Duration:** 2–3 weeks  
**Depends on:** Phase 0 (freeze token satisfied)  
**Blocks:** Phase 2  

---

## Why This Phase Exists

Phase 0 gets the code compiling, but the `Database` struct has no real connection logic and no schema. All repositories (`SurrealUserRepository`, `SurrealDocumentRepository`, `SurrealAuditRepository`) exist as code but are never instantiated with a real database connection. The `Ledger`, `ReconciliationProcessor`, `TaxCalculator`, and `PayrollProcessor` all store data in `BTreeMap`s that vanish when the process exits. This phase connects everything to SurrealDB and proves data persists.

---

## Task List

| ID | Task | Primary File(s) | Depends On | Parallel With |
|---|---|---|---|---|
| 1.1 | **Create SurrealDB schema definitions** — `DEFINE TABLE` statements for all models: `account`, `transaction`, `transaction_entry`, `journal_entry`, `user`, `organization`, `document`, `audit_log`, `reconciliation`, `tax_jurisdiction`, `tax_filing`, `employee`, `pay_period`, `time_entry`. | `database/schema.rs` (new) | P0 | Nothing (critical) |
| 1.2 | **Implement `Database::connect()`** — connect to SurrealDB (WebSocket), select namespace + database, apply schema. Fallback to in-memory (`kv-mem`) when no URL configured. | `database/mod.rs` | 1.1 | Nothing (critical) |
| 1.3 | **Add migration runner** — detect if schema is applied, apply incrementally on startup. Use a `schema_version` table. | `database/migrations.rs` (new) | 1.2 | Nothing (critical) |
| 1.4 | **Seed default chart of accounts** — insert the 20 default accounts into SurrealDB on first run (if not already present). | `database/seed.rs` (new) | 1.3 | Nothing (critical) |
| 1.5 | **Wire `SurrealUserRepository` into orchestrator** — orchestrator holds `Arc<Database>`, passes to user repo. | `agents/orchestrator.rs`, `database/user.rs` | 1.2 | 1.6, 1.7 |
| 1.6 | **Wire `SurrealDocumentRepository` into DocumentAgent** — agent receives `Arc<Database>` in constructor. | `agents/document.rs` | 1.2 | 1.5, 1.7 |
| 1.7 | **Wire `SurrealAuditRepository` into AuditAgent** — agent receives `Arc<Database>` in constructor. | `audit/mod.rs` | 1.2 | 1.5, 1.6 |
| 1.8 | **Refactor `Ledger` to persist to SurrealDB** — `create_account()`, `record_transaction()`, `get_account()` etc. write to/read from SurrealDB tables instead of `BTreeMap`. Keep in-memory cache for hot reads. | `accounting/ledger.rs` | 1.2 | 1.9 |
| 1.9 | **Refactor `ReconciliationProcessor` to persist to SurrealDB** — reconciliation records stored in SurrealDB. | `accounting/reconciliation.rs` | 1.2 | 1.8 |
| 1.10 | **Refactor `TaxCalculator` to persist to SurrealDB** — jurisdictions and filings stored in SurrealDB. | `accounting/tax.rs` | 1.2 | 1.8, 1.9 |
| 1.11 | **Refactor `PayrollProcessor` to persist to SurrealDB** — employees, pay periods, time entries stored in SurrealDB. | `accounting/payroll.rs` | 1.2 | 1.8, 1.9, 1.10 |
| 1.12 | **Integration test** — create account → record transaction → verify balance persists in SurrealDB → restart process → verify balance survives restart. | `tests/integration/database.rs` (new) | 1.4, 1.8 | Nothing (final gate) |

---

## Dependency Graph

```
                          P0 (freeze token)
                               │
                          ┌────┴────┐
                          │   1.1   │  Schema definitions
                          └────┬────┘
                          ┌────┴────┐
                          │   1.2   │  Database::connect()
                          └────┬────┘
                          ┌────┴────┐
                          │   1.3   │  Migration runner
                          └────┬────┘
                          ┌────┴────┐
                          │   1.4   │  Seed chart of accounts
                          └────┬────┘
                               │
              ┌────────────────┼────────────────┐
              │                │                 │
       ┌──────┴──┐     ┌──────┴──┐       ┌─────┴─────┐
       │ 1.5     │     │ 1.8     │       │ 1.12      │
       │ 1.6     │     │ 1.9     │       │ (after    │
       │ 1.7     │     │ 1.10    │       │  1.4+1.8) │
       └─────────┘     │ 1.11    │       └───────────┘
     (repo wiring,      └─────────┘
      parallel)      (accounting refactors,
                       sequential)
```

---

## Parallel Execution Strategy

**Session 1 (Critical path, sequential):**
- 1.1 → 1.2 → 1.3 → 1.4

**Session 2 (After 1.2, two parallel tracks):**
- Track B: 1.5 + 1.6 + 1.7 (repository wiring — all independent)
- Track C: 1.8 → 1.9 → 1.10 → 1.11 (accounting refactors — sequential within track)

**Session 3 (Integration):**
- 1.12 (after 1.4 and 1.8 are both complete)

---

## Freeze Token 1 🔒

All conditions must be true:

- [ ] SurrealDB starts (either local or embedded `kv-mem` mode)
- [ ] Schema is applied on first run — all `DEFINE TABLE` statements execute
- [ ] Migrations detect already-applied schema and skip on subsequent runs
- [ ] Default chart of accounts (20 accounts) is seeded on first run
- [ ] All 3 repository implementations persist to SurrealDB:
  - `SurrealUserRepository`: create/find/update/delete user
  - `SurrealDocumentRepository`: save/find/delete document
  - `SurrealAuditRepository`: log/find/list audit entries
- [ ] `Ledger` operations persist: create account, record transaction, get balance — all read/write SurrealDB
- [ ] Integration test passes: create account → record transaction → verify balance → **restart process** → verify balance survives
- [ ] `cargo test` passes (unit + new integration tests)

---

## Known Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| SurrealDB query syntax differs from expected SQL | Medium | Test each query interactively before coding; use raw string queries |
| `kv-mem` mode doesn't support all SurrealDB features | Medium | If kv-mem is limited, use embedded file mode for tests |
| Refactoring Ledger to use SurrealDB changes the API surface | Medium | Keep the same public method signatures; only change internal storage |
| Connection pool exhaustion under concurrent agent access | Low | Start with single connection, add pooling in Phase 7 if needed |

---

## Notes for Reviewer

- This phase introduces a **real database dependency** — SurrealDB must be available (or `kv-mem` used for tests)
- The in-memory `BTreeMap` storage in Ledger/Reconciliation/Tax/Payroll is **replaced**, not removed — keep BTreeMap as a read-through cache if performance warrants
- No API endpoints are built in this phase — that's Phase 3
- No agent processing logic changes — agents still return mock data in Phase 1; real processing is Phase 2
