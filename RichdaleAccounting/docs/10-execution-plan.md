# NexusLedger — Master Execution Plan

**Status:** Draft — awaiting approval  
**Goal:** Ship a working MVP that can replace QuickBooks for a small business  
**Methodology:** Phase-gated with freeze tokens, parallel tracks, and audit checkpoints  

---

## Philosophy

```
┌─────────────────────────────────────────────────────┐
│  Each phase has:                                     │
│  • Defined IN scope (what we build)                  │
│  • Defined OUT scope (what we explicitly skip)       │
│  • Parallel tracks (independent work streams)        │
│  • Freeze Token: exact conditions to gate next phase │
│  • Audit checklist (testable verification steps)     │
│                                                      │
│  Rule: You CANNOT advance without the freeze token   │
│  Rule: Parallel tracks run simultaneously            │
│  Rule: Every phase ends with `cargo test` green      │
└─────────────────────────────────────────────────────┘
```

## Phase Summary

```
Phase 0 ──→ Phase 1 ──→ Phase 2 ──→ Phase 3 ──→ Phase 4 ──→ Phase 5 ──→ Phase 6 ──→ Phase 7
COMPILE   │ DATABASE  │ ENGINE    │ END-TO- │ ACCOUNTING│ AI      │ EDGE     │ PRODUCTION
  & FIX   │   & WIRE  │   & AGENT │   END   │ COMPLETE  │ PIPELINE│  & SYNC  │ HARDENING
          │           │           │         │           │         │          │
  ~2 wks    ~3 wks     ~4 wks      ~3 wks     ~4 wks     ~3 wks     ~3 wks     ~3 wks
                                                                       TOTAL: ~25 weeks
```

---

## PHASE 0: COMPILE & FIX
**Duration:** 1–2 weeks  
**Objective:** Get the codebase compiling cleanly. Fix all type mismatches. Define the missing `Database` struct.  

### Tasks

| ID | Task | File(s) | Depends On | Parallelizable |
|---|---|---|---|---|
| 0.1 | Define `Database` struct with connection management | `database/mod.rs` (new) | — | No (blocks everything) |
| 0.2 | Fix `AgentOrchestrator::add_agent()` type mismatches for all 9 agents | `agents/orchestrator.rs` | 0.1 | No |
| 0.3 | Fix `Agent` trait: `process_task` takes `&self` but mutates `self.status` → use `&mut self` or interior mutability | `agents/agent_types.rs` | — | With 0.2 |
| 0.4 | Fix `once_cell` dependency (missing from Cargo.toml, used in orchestrator) | `Cargo.toml` | — | With 0.2, 0.3 |
| 0.5 | Fix `LedgerAgent::process_record_transaction` — embedded ledger never initialized | `accounting/ledger.rs` | 0.2 | With 0.4 |
| 0.6 | Fix `ReportingAgent` mapping — currently points to `LedgerAgent` (wrong) | `agents/orchestrator.rs` | 0.2 | With 0.4, 0.5 |
| 0.7 | Remove or fix all `Arc<Mutex<Option<...>>>` patterns in agent constructors | `agents/orchestrator.rs` + all agent files | 0.1, 0.2 | No |
| 0.8 | Add `impl Default` for all structs missing it where `Default::default()` is used | Various | — | With 0.2 |
| 0.9 | Run `cargo check` and fix every remaining error | All | 0.1–0.8 | No |
| 0.10 | Run `cargo test` and ensure all existing tests pass | All | 0.9 | No |

### Parallel Tracks

```
Track A (blocking):   0.1 → 0.2 → 0.9 → 0.10
Track B (parallel):   0.3 ─┐
Track C (parallel):   0.4 ─┤  ← all merge into 0.9
Track D (parallel):   0.8 ─┘
```

### Freeze Token 0 🔒

```
cargo check passes with zero errors
cargo test passes with zero failures
cargo clippy has no error-level warnings
Database struct exists with new() and connect() methods
All 9 agents can be instantiated without panic
```

---

## PHASE 1: DATABASE & WIRE
**Duration:** 2–3 weeks  
**Objective:** Connect to SurrealDB. Create schema definitions. Wire the database into every agent so they read/write real data instead of mock responses.  

### Tasks

| ID | Task | File(s) | Depends On | Parallelizable |
|---|---|---|---|---|
| 1.1 | Create SurrealDB schema definitions (DEFINE TABLE for all models) | `database/schema.rs` (new) | P0 | No |
| 1.2 | Implement `Database::connect()` with in-memory fallback | `database/mod.rs` | 1.1 | No |
| 1.3 | Add database migration runner (run schema on startup) | `database/migrations.rs` (new) | 1.2 | No |
| 1.4 | Seed default chart of accounts into SurrealDB | `database/seed.rs` (new) | 1.3 | No |
| 1.5 | Wire `SurrealUserRepository` into orchestrator | `agents/orchestrator.rs`, `database/user.rs` | 1.2 | With 1.6, 1.7 |
| 1.6 | Wire `SurrealDocumentRepository` into DocumentAgent | `agents/document.rs` | 1.2 | With 1.5, 1.7 |
| 1.7 | Wire `SurrealAuditRepository` into AuditAgent | `audit/mod.rs` | 1.2 | With 1.5, 1.6 |
| 1.8 | Refactor `Ledger` to use SurrealDB persistence (not just BTreeMap) | `accounting/ledger.rs` | 1.2 | With 1.9 |
| 1.9 | Refactor `ReconciliationProcessor` to use SurrealDB | `accounting/reconciliation.rs` | 1.2 | With 1.8 |
| 1.10 | Refactor `TaxCalculator` to use SurrealDB | `accounting/tax.rs` | 1.2 | With 1.8, 1.9 |
| 1.11 | Refactor `PayrollProcessor` to use SurrealDB | `accounting/payroll.rs` | 1.2 | With 1.8, 1.9 |
| 1.12 | Integration test: create account → record transaction → verify balance | `tests/integration/` (new) | 1.4, 1.8 | No |

### Parallel Tracks

```
Track A (sequential): 1.1 → 1.2 → 1.3 → 1.4 → 1.12
Track B (parallel after 1.2):
  1.5 ─┐
  1.6 ─┤  ← all three repo wiring tasks in parallel
  1.7 ─┘
Track C (parallel after 1.2):
  1.8  ── 1.9  ── 1.10 ── 1.11  ← sequential refactors
```

### Freeze Token 1 🔒

```
SurrealDB starts and schema is applied on first run
All 4 repository implementations (User, Doc, Audit, Financial) persist to SurrealDB
Default chart of accounts is seeded on startup
Integration test passes: create account, record transaction, verify balance in SurrealDB
cargo test passes
```

---

## PHASE 2: REAL AGENT ENGINE
**Duration:** 3–4 weeks  
**Objective:** Replace all mock agent logic with real processing. Every agent must actually execute its domain logic and persist results.  

### Tasks

| ID | Task | File(s) | Depends On | Parallelizable |
|---|---|---|---|---|
| 2.1 | Implement real `LedgerAgent.process_task()` — record transaction, update balances, create journal entry | `accounting/ledger.rs` | P1 | With 2.2, 2.3 |
| 2.2 | Implement real `ReconciliationAgent.process_task()` — match statement to book transactions | `accounting/reconciliation.rs` | P1 | With 2.1, 2.3 |
| 2.3 | Implement real `TaxAgent.process_task()` — calculate taxes from task payload | `accounting/tax.rs` | P1 | With 2.1, 2.2 |
| 2.4 | Implement real `PayrollAgent.process_task()` — calculate payroll from employees + time entries | `accounting/payroll.rs` | P1 | With 2.1 |
| 2.5 | Implement `InvoiceAgent` — create, send, track invoices (currently missing entirely) | `accounting/invoice.rs` (new), `agents/orchestrator.rs` | P1, 2.1 | With 2.6 |
| 2.6 | Implement `ReceiptAgent` — process receipts, categorize expenses | `accounting/receipt.rs` (new), `agents/orchestrator.rs` | P1, 2.1 | With 2.5 |
| 2.7 | Implement real `ReportingAgent` — P&L, Balance Sheet, Trial Balance queries | `accounting/reporting.rs` (new) | P1, 2.1 | With 2.5, 2.6 |
| 2.8 | Implement real `AuditAgent.process_task()` — log actual audit entries with old/new values | `audit/mod.rs` | P1 | With 2.1 |
| 2.9 | Implement real `DocumentAgent.process_task()` — store/retrieve from SurrealDB | `agents/document.rs` | P1 | With 2.8 |
| 2.10 | Implement task processing loop with real dispatch (not just `process_next_task` sleep loop) | `agents/orchestrator.rs` | 2.1–2.9 | No |
| 2.11 | Add integration test: submit transaction task → agent processes → verify in DB | `tests/integration/` | 2.10 | No |

### Parallel Tracks

```
Track A (core):  2.1 ──────────────────────────────────→ 2.10 → 2.11
Track B (domain):
  2.2 ─┐
  2.3 ─┤
  2.4 ─┤  ← all four accounting agents in parallel
  2.8 ─┘
Track C (new agents):
  2.5 ─┐
  2.6 ─┤  ← invoice, receipt, reporting in parallel
  2.7 ─┘
  2.9 ─  ← document agent
```

### Freeze Token 2 🔒

```
All 9 agents process real tasks (no mock returns)
InvoiceAgent and ReceiptAgent exist and work
Task queue dispatches to correct agent type
Integration test: submit RecordTransaction → agent processes → balance updated in SurrealDB
Integration test: submit GenerateInvoice → InvoiceAgent creates invoice in SurrealDB
cargo test passes (unit + integration)
```

---

## PHASE 3: END-TO-END (API + FRONTEND)
**Duration:** 2–3 weeks  
**Objective:** Build the real API server, connect the React frontend to it, and prove a complete user-facing workflow.  

### Tasks

| ID | Task | File(s) | Depends On | Parallelizable |
|---|---|---|---|---|
| 3.1 | Implement `ApiServer::start()` — bind axum to port, wire routes | `api/mod.rs` | P2 | No |
| 3.2 | Implement all API route handlers using real agent orchestrator | `api/routes/` (new) | 3.1 | No |
| 3.3 | Add request/response middleware (request ID, timing, error mapping) | `api/middleware.rs` (new) | 3.2 | No |
| 3.4 | Replace Tauri backend with one that imports `nexus-core` | `nexus-ledger-tauri/backend/` | 3.1 | With 3.5 |
| 3.5 | Add CORS, rate limiting, and graceful shutdown to real API | `api/mod.rs` | 3.1 | With 3.4 |
| 3.6 | Add react-router to frontend (dashboard, accounts, invoices, ledger pages) | `nexus-ledger-tauri/src/` | — | With 3.1 |
| 3.7 | Build Account List page — fetch from real API, display real data | `nexus-ledger-tauri/src/pages/` (new) | 3.2, 3.6 | With 3.8, 3.9 |
| 3.8 | Build Journal Entry form — create real transaction via API | `nexus-ledger-tauri/src/pages/` (new) | 3.2, 3.6 | With 3.7, 3.9 |
| 3.9 | Build Ledger/Transaction List page — fetch real transactions | `nexus-ledger-tauri/src/pages/` (new) | 3.2, 3.6 | With 3.7, 3.8 |
| 3.10 | Build Invoice Create page — POST invoice via API | `nexus-ledger-tauri/src/pages/` (new) | 3.2, 3.6 | With 3.7–3.9 |
| 3.11 | Add error boundaries and loading states to all frontend pages | `nexus-ledger-tauri/src/` | 3.7–3.10 | No |
| 3.12 | E2E test: user creates transaction in UI → appears in ledger | — | 3.11 | No |

### Parallel Tracks

```
Track A (backend):  3.1 → 3.2 → 3.3 → 3.5
Track B (frontend, starts after 3.2):
  3.6 → 3.7 ─┐
         3.8 ─┤  ← pages in parallel
         3.9 ─┤
         3.10─┘ → 3.11 → 3.12
Track C (Tauri, parallel):  3.4
```

### Freeze Token 3 🔒

```
Real API server starts on port 4000 and serves data from SurrealDB
Frontend on port 3000 fetches and displays real data
User can create a transaction through the UI and see it in the ledger
User can create an invoice through the UI
All API endpoints return real data (no mock)
Tauri backend IS nexus-core (no duplicate stub server)
E2E test passes
```

---

## PHASE 4: AUTH & ACCOUNTING COMPLETENESS
**Duration:** 3–4 weeks  
**Objective:** Add authentication, authorization, and fill in missing accounting features to reach QuickBooks-parity.  

### Tasks

| ID | Task | File(s) | Depends On | Parallelizable |
|---|---|---|---|---|
| 4.1 | Add password hashing (argon2/bcrypt) to User model | `database/user.rs`, `database/models.rs` | P3 | With 4.2 |
| 4.2 | Implement JWT auth middleware for API | `api/auth.rs` (new) | P3 | With 4.1 |
| 4.3 | Add login/register API endpoints | `api/routes/auth.rs` (new) | 4.1, 4.2 | No |
| 4.4 | Add role-based access control (Admin/Manager/User/Viewer) | `api/auth.rs` | 4.2 | With 4.5 |
| 4.5 | Add login UI to frontend | `nexus-ledger-tauri/src/pages/Login.tsx` (new) | 4.3 | With 4.4 |
| 4.6 | Implement Accounts Payable workflow (bill entry, payment tracking) | `accounting/ap.rs` (new) | P3 | With 4.7, 4.8 |
| 4.7 | Implement Accounts Receivable aging report | `accounting/reporting.rs` | P3 | With 4.6, 4.8 |
| 4.8 | Implement Cash Flow Statement generation | `accounting/reporting.rs` | P3 | With 4.6, 4.7 |
| 4.9 | Implement CSV import for transactions and accounts | `utils/import.rs` (new) | P3 | With 4.10 |
| 4.10 | Implement CSV/OFX export for transactions | `utils/export.rs` (new) | P3 | With 4.9 |
| 4.11 | Implement multi-currency support with exchange rates | `database/financial.rs` (extend) | P3 | With 4.6 |
| 4.12 | Add budget tracking (budget vs. actual) | `accounting/budget.rs` (new) | P3 | With 4.8 |
| 4.13 | Add fixed asset tracking with depreciation | `accounting/assets.rs` (new) | P3 | With 4.6 |
| 4.14 | Audit test: unauthorized user cannot access admin endpoints | `tests/integration/` | 4.4 | No |

### Parallel Tracks

```
Track A (auth, sequential):  4.1 → 4.2 → 4.3 → 4.4 → 4.5 → 4.14
Track B (accounting, parallel after P3):
  4.6 ─┐
  4.7 ─┤
  4.8 ─┤  ← all accounting features in parallel
  4.11─┤
  4.12─┤
  4.13─┘
Track C (import/export, parallel):
  4.9 ── 4.10
```

### Freeze Token 4 🔒

```
User registration and login work end-to-end
JWT tokens are issued and validated
Role-based access: Viewer cannot create, User cannot delete, Admin can do everything
AP workflow: enter bill → schedule payment → mark paid → journal entry created
AR aging report: shows outstanding invoices grouped by 30/60/90 days
Cash flow statement: operating + investing + financing activities
CSV import: upload file → transactions created in system
CSV export: download transactions as CSV
Multi-currency: transaction in EUR → converted to USD base currency
All integration tests pass
```

---

## PHASE 5: AI PIPELINE
**Duration:** 2–3 weeks  
**Objective:** Build a real AI document processing pipeline. OCR → extraction → classification → auto-categorization.  

### Tasks

| ID | Task | File(s) | Depends On | Parallelizable |
|---|---|---|---|---|
| 5.1 | Integrate OCR engine (Tesseract or cloud vision API) for text extraction from images/PDFs | `ai/ocr.rs` (new) | P4 | With 5.2 |
| 5.2 | Add PDF text extraction (using pdf-extract crate or similar) | `ai/pdf.rs` (new) | P4 | With 5.1 |
| 5.3 | Implement receipt photo upload in frontend (drag-and-drop) | `nexus-ledger-tauri/src/pages/` | P4 | With 5.1, 5.2 |
| 5.4 | Wire OCR output → AI extraction prompts (replace binary placeholder) | `ai/mod.rs` | 5.1, 5.2 | No |
| 5.5 | Implement extracted data → auto-create Transaction pipeline | `agents/document.rs` | 5.4 | No |
| 5.6 | Add embedding storage + vector similarity search in SurrealDB | `ai/embeddings.rs` (new) | P4 | With 5.4, 5.5 |
| 5.7 | Implement anomaly detection on transactions (flag suspicious entries) | `ai/analysis.rs` (new) | P4 | With 5.4 |
| 5.8 | Implement smart account categorization suggestions | `ai/classification.rs` (new) | P4 | With 5.4 |
| 5.9 | Add AI status/health endpoint (is Ollama available?) | `api/routes/ai.rs` (new) | 5.4 | With 5.6 |
| 5.10 | E2E test: upload receipt image → AI extracts → transaction created | `tests/integration/` | 5.5 | No |

### Parallel Tracks

```
Track A (OCR pipeline, sequential): 5.1,5.2 → 5.4 → 5.5 → 5.10
Track B (frontend, parallel):       5.3
Track C (AI features, parallel):    5.6 ─┐
                                    5.7 ─┤  ← embedding, anomaly, categorization
                                    5.8 ─┘
Track D (API, parallel):            5.9
```

### Freeze Token 5 🔒

```
Upload a receipt photo → OCR extracts text → AI extracts structured data → transaction auto-created
Upload a PDF invoice → text extracted → vendor, amount, date parsed → transaction created
Embeddings are stored and searchable
Anomaly detection flags at least one test case (duplicate transaction, unusual amount)
Account categorization suggests correct type for test accounts
AI health endpoint returns Ollama connectivity status
E2E test passes
```

---

## PHASE 6: EDGE & SYNC
**Duration:** 2–3 weeks  
**Objective:** Implement offline-first data storage, periodic sync, and conflict resolution.  

### Tasks

| ID | Task | File(s) | Depends On | Parallelizable |
|---|---|---|---|---|
| 6.1 | Add embedded SQLite as local edge database (using rusqlite or sqlx-sqlite) | `edge/local_db.rs` (new) | P5 | With 6.2 |
| 6.2 | Implement local data store — serialize/deserialize accounts, transactions | `edge/store.rs` (new) | P5 | With 6.1 |
| 6.3 | Implement change tracking (dirty flags, last-sync timestamps per record) | `edge/tracking.rs` (new) | 6.1, 6.2 | No |
| 6.4 | Implement sync engine — push local changes to SurrealDB, pull remote changes | `edge/sync.rs` (new) | 6.3 | No |
| 6.5 | Implement conflict resolution (last-write-wins with audit log) | `edge/conflict.rs` (new) | 6.4 | No |
| 6.6 | Implement offline mode toggle — block sync when offline, queue changes | `edge/mod.rs` | 6.3 | With 6.4 |
| 6.7 | Add sync status indicator to frontend | `nexus-ledger-tauri/src/` | 6.4 | With 6.5, 6.6 |
| 6.8 | Add local encryption at rest for sensitive data | `edge/encryption.rs` (new) | P5 | With 6.1 |
| 6.9 | Add compression for local storage (lz4 or zstd) | `edge/compression.rs` (new) | P5 | With 6.1 |
| 6.10 | Integration test: go offline → create transaction → go online → verify sync | `tests/integration/` | 6.5, 6.6 | No |

### Parallel Tracks

```
Track A (sync engine, sequential): 6.1 → 6.2 → 6.3 → 6.4 → 6.5 → 6.10
Track B (offline mode, parallel):  6.6
Track C (frontend, parallel):      6.7
Track D (security/perf, parallel): 6.8, 6.9
```

### Freeze Token 6 🔒

```
App can start in offline mode and create transactions locally
Going online triggers sync — local transactions appear in SurrealDB
Conflict resolution: two devices edit same account → merge without data loss
Sync status shows "syncing...", "up to date", or "offline"
Local data is encrypted (sensitive fields: SSN, bank account numbers)
Integration test passes: offline create → online sync → verify
```

---

## PHASE 7: PRODUCTION HARDENING
**Duration:** 2–3 weeks  
**Objective:** Performance optimization, security audit, Tauri packaging, deployment readiness.  

### Tasks

| ID | Task | File(s) | Depends On | Parallelizable |
|---|---|---|---|---|
| 7.1 | Lock contention audit — identify and reduce Arc<RwLock<>> nesting | Various | P6 | With 7.2 |
| 7.2 | Add connection pooling for SurrealDB | `database/mod.rs` | P6 | With 7.1 |
| 7.3 | Add request rate limiting (token bucket) | `api/middleware.rs` | P6 | With 7.1 |
| 7.4 | Security audit: SQL injection (SurrealQL injection) prevention | `database/` all repos | P6 | With 7.1 |
| 7.5 | Security audit: XSS, CSRF, input validation hardening | `api/`, frontend | P6 | With 7.1 |
| 7.6 | Add Prometheus metrics export endpoint | `monitor/mod.rs` | P6 | With 7.3 |
| 7.7 | Add health check endpoint (k8s-compatible: /health, /ready) | `api/routes/health.rs` (new) | P6 | With 7.3 |
| 7.8 | Package Tauri desktop app for Windows (MSI/exe installer) | `nexus-ledger-tauri/` | P6 | With 7.9 |
| 7.9 | Package Tauri desktop app for macOS (DMG) | `nexus-ledger-tauri/` | P6 | With 7.8 |
| 7.10 | Add system tray icon with sync status | `nexus-ledger-tauri/src-tauri/` | 7.8 | With 7.7 |
| 7.11 | Add auto-update mechanism (Tauri updater) | `nexus-ledger-tauri/src-tauri/` | 7.8 | With 7.7 |
| 7.12 | Performance benchmarks: 10K transactions, 1K accounts | `benches/` (new) | 7.1, 7.2 | No |
| 7.13 | Load test: 100 concurrent API requests | `tests/load/` (new) | 7.3 | With 7.12 |
| 7.14 | Write user documentation (README update, quick start guide) | `docs/user/` (new) | All | With 7.12 |
| 7.15 | Final audit: all integration + E2E tests green | `tests/` | All | No |

### Parallel Tracks

```
Track A (performance):  7.1 → 7.2 → 7.12 → 7.13 → 7.15
Track B (security):     7.4 ─┐
                        7.5 ─┤  ← parallel audits
                        7.3 ─┘ → 7.6, 7.7
Track C (packaging):    7.8 → 7.9 → 7.10 → 7.11
Track D (docs):         7.14
```

### Freeze Token 7 🔒 (FINAL — SHIP IT)

```
cargo test --all passes (unit + integration + E2E)
No security vulnerabilities in dependency audit (cargo audit clean)
10K transaction benchmark completes in < 2 seconds
100 concurrent requests sustain without errors
Windows MSI installer builds and installs cleanly
macOS DMG builds and installs cleanly
System tray icon shows sync status
Auto-update detects new version
User documentation is complete
Health endpoint returns 200
Prometheus metrics endpoint returns valid metrics
All 9 agents process real tasks with real data
AI pipeline processes at least one receipt end-to-end
Edge sync works with conflict resolution
```

---

## Dependency Graph

```
P0 ───→ P1 ───→ P2 ───→ P3 ───→ P4 ───→ P5 ───→ P6 ───→ P7
COMPILE  DB     AGENTS   E2E     ACCT    AI      EDGE    PROD
 FIX    WIRE   REAL    API+UI  +AUTH   PIPE    SYNC    HARDEN
```

No phase can begin without its predecessor's freeze token being satisfied.

## Time Estimates

| Phase | Duration | Cumulative |
|---|---|---|
| Phase 0: Compile & Fix | 1–2 weeks | Week 2 |
| Phase 1: Database & Wire | 2–3 weeks | Week 5 |
| Phase 2: Real Agent Engine | 3–4 weeks | Week 9 |
| Phase 3: End-to-End | 2–3 weeks | Week 12 |
| Phase 4: Auth & Accounting | 3–4 weeks | Week 16 |
| Phase 5: AI Pipeline | 2–3 weeks | Week 19 |
| Phase 6: Edge & Sync | 2–3 weeks | Week 22 |
| Phase 7: Production Hardening | 2–3 weeks | Week 25 |

**Total: ~6 months** of focused, full-time development to MVP.
