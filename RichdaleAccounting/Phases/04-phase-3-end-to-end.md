# Phase 3: End-to-End (API + Frontend)

**Objective:** Build the real API server on top of the agent engine, replace the Tauri stub backend with one that imports `nexus-core`, and prove a complete user-facing workflow from UI to database.  
**Duration:** 2–3 weeks  
**Depends on:** Phase 2 (freeze token satisfied)  
**Blocks:** Phase 4  

---

## Why This Phase Exists

After Phase 2, all agents work but there is no way for a user to interact with them. The `ApiServer::start()` method only logs — it never binds a socket. The Tauri backend (`nexus-ledger-tauri/backend/`) is a standalone stub server that doesn't import `nexus-core` at all. The React frontend fetches from this stub. This phase bridges the gap: real HTTP server, real routes, real frontend pages, real data flow.

---

## Task List

| ID | Task | Primary File(s) | Depends On | Parallel With |
|---|---|---|---|---|
| 3.1 | **Implement `ApiServer::start()`** — bind axum to configured host:port, wire all route handlers, start accepting connections. | `api/mod.rs` | P2 | Nothing (critical) |
| 3.2 | **Implement API route handlers** — wire all endpoints to the real `AgentOrchestrator` and `NexusLedger`: `GET /accounts`, `GET /transactions`, `POST /transactions`, `GET /invoices`, `POST /invoices`, `GET /reconcile`, `GET /reports/trial-balance`, `GET /reports/balance-sheet`, `GET /reports/income-statement`, `GET /status`. | `api/routes/` (new directory) | 3.1 | Nothing (critical) |
| 3.3 | **Add request middleware** — request ID generation, timing, error-to-HTTP-status mapping, structured logging. | `api/middleware.rs` (new) | 3.2 | Nothing (critical) |
| 3.4 | **Replace Tauri backend** — rewrite `nexus-ledger-tauri/backend/src/main.rs` to import `nexus-core`, create `NexusLedger` + `Database`, start `ApiServer`. Remove the standalone stub server entirely. | `nexus-ledger-tauri/backend/` | 3.1 | 3.5, 3.6 |
| 3.5 | **Add CORS + graceful shutdown** — configure tower-http CORS for dev, add `tokio::signal::ctrl_c()` handler for clean shutdown. | `api/mod.rs` | 3.1 | 3.4 |
| 3.6 | **Add react-router to frontend** — set up routing for: `/` (dashboard), `/accounts`, `/ledger`, `/invoices`, `/reports`. | `nexus-ledger-tauri/src/` | — | 3.4, 3.5 |
| 3.7 | **Build Account List page** — fetch from `GET /api/accounts`, display real chart of accounts with balances. | `nexus-ledger-tauri/src/pages/Accounts.tsx` (new) | 3.2, 3.6 | 3.8, 3.9, 3.10 |
| 3.8 | **Build Journal Entry form** — form to create a transaction (select accounts, enter debit/credit amounts, description). POST to `/api/transactions`. | `nexus-ledger-tauri/src/pages/JournalEntry.tsx` (new) | 3.2, 3.6 | 3.7, 3.9 |
| 3.9 | **Build Ledger/Transaction List page** — fetch from `GET /api/transactions`, display real transactions with date, description, amounts. | `nexus-ledger-tauri/src/pages/Ledger.tsx` (new) | 3.2, 3.6 | 3.7, 3.8 |
| 3.10 | **Build Invoice pages** — list invoices, create new invoice via form. POST to `/api/invoices`. | `nexus-ledger-tauri/src/pages/Invoices.tsx` (new) | 3.2, 3.6 | 3.7, 3.8, 3.9 |
| 3.11 | **Add error boundaries + loading states** — React error boundaries on all pages, loading spinners during API fetches, error toasts. | `nexus-ledger-tauri/src/components/` (new) | 3.7–3.10 | Nothing |
| 3.12 | **E2E test** — user creates a transaction in the UI → transaction appears in ledger list → account balance updates. | — | 3.11 | Nothing (final gate) |

---

## Dependency Graph

```
                    P2 (freeze token)
                         │
                    ┌────┴────┐
                    │   3.1   │  ApiServer::start()
                    └────┬────┘
                    ┌────┴────┐
                    │   3.2   │  Route handlers
                    └────┬────┘
                    ┌────┴────┐
                    │   3.3   │  Middleware
                    └────┬────┘
                         │
         ┌───────────────┼───────────────┐
         │               │               │
    ┌────┴──┐     ┌──────┴──┐     ┌─────┴───┐
    │  3.4  │     │   3.6   │     │   3.5   │
    │Tauri  │     │ Router  │     │  CORS   │
    │rewrite│     └────┬────┘     └─────────┘
    └───────┘          │
                ┌──────┼──────┐
                │      │      │
           ┌────┴─┐ ┌──┴──┐ ┌┴─────┐
           │ 3.7  │ │ 3.8 │ │ 3.9  │  ← pages in parallel
           │Accts │ │ JE  │ │Ledger│
           └───┬──┘ └──┬──┘ └──┬───┘
               │        │       │
           ┌───┴────────┴───────┴───┐
           │        3.10            │  Invoice pages
           └───────────┬────────────┘
                  ┌────┴────┐
                  │  3.11   │  Error boundaries
                  └────┬────┘
                  ┌────┴────┐
                  │  3.12   │  E2E test
                  └─────────┘
```

---

## Parallel Execution Strategy

**Session 1 (Critical path, sequential):**
- 3.1 → 3.2 → 3.3

**Session 2 (After 3.2, three parallel tracks):**
- Track A: 3.4 + 3.5 (Tauri rewrite + CORS)
- Track B: 3.6 (router setup)

**Session 3 (After 3.6, all pages parallel):**
- 3.7 + 3.8 + 3.9 + 3.10

**Session 4 (Integration):**
- 3.11 → 3.12

---

## Freeze Token 3 🔒

All conditions must be true:

- [ ] Real API server starts on port 4000 and responds to requests
- [ ] `GET /api/accounts` returns real chart of accounts from SurrealDB
- [ ] `GET /api/transactions` returns real transactions from SurrealDB
- [ ] `POST /api/transactions` creates a real transaction (journal entry + balance update) via `LedgerAgent`
- [ ] `GET /api/invoices` returns real invoices
- [ ] `POST /api/invoices` creates a real invoice via `InvoiceAgent`
- [ ] `GET /api/reports/trial-balance` returns real trial balance from `ReportingAgent`
- [ ] `GET /api/reports/balance-sheet` returns real balance sheet
- [ ] `GET /api/status` returns real agent count, health score, task counts
- [ ] Tauri backend IS `nexus-core` — no duplicate stub server exists
- [ ] Frontend fetches from real API and displays real data
- [ ] User can create a transaction through the UI → it appears in the ledger
- [ ] User can create an invoice through the UI
- [ ] All pages have error boundaries and loading states
- [ ] `cargo test` passes

---

## Notes for Reviewer

- This phase is where the project becomes **user-visible** for the first time
- The Tauri rewrite (3.4) is the highest-risk task — the two subprojects merge into one
- Frontend uses plain React (no state management library) — keep it simple
- No authentication in this phase — all endpoints are public. Auth is Phase 4
- No AI features — document upload and processing is Phase 5
