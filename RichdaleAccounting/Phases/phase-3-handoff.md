You are continuing development of NexusLedger, an agentic accounting platform 
in Rust (Tauri + React frontend, SurrealDB backend, agent-based architecture).

## Repository
c:\Users\msira\Richdale_Accounting\Rich_AIM_Acct_INV
Latest commit on main

## Read These First
1. RichdaleAccounting/Phases/TRACKER.md        ← current progress (P2 done, P3 next)
2. RichdaleAccounting/Phases/00-strategy.md    ← methodology + principles
3. RichdaleAccounting/Phases/09-commit-discipline.md ← commit/push rules
4. RichdaleAccounting/docs/02-architecture.md  ← system diagram
5. RichdaleAccounting/docs/04-accounting.md    ← accounting engine reference
6. RichdaleAccounting/docs/05-agentic-design.md ← CRITICAL: agentic product vision

## Completed Phases
- Phase 0: Compile & Fix — 458 errors → 0, all 9 agents instantiate
- Phase 1: Database & Wire — SurrealDB schema (15 tables), kv-mem connection, 
  26 real repository methods, additive persistence in Ledger/Tax/Payroll/Recon
- Phase 2: Real Agent Engine — ALL 9 agents process real tasks (no mocks),
  3 new agents (InvoiceAgent, ReceiptAgent, ReportingAgent), Cash Flow Statement,
  progressive tax brackets, YTD payroll, pre-tax deductions, fuzzy reconciliation 
  matching, hash-chaining audit trail, overpayment→credit, cancel/reverse invoices,
  priority scheduling, dead letter queue, shared ledger across all agents

## Current State
- cargo check: 0 errors
- cargo test: 200 passed, 0 failed (164 lib + 1 bin + 23 comprehensive + 8 integration + 4 phase-1)
- Database: SurrealDB kv-mem (embedded), schema applied, seed data loaded
- Agents: All 9 types process real tasks with correct accounting logic
- Persistence: Additive SurrealDB writes on top of in-memory BTreeMap cache
- AI: Stubbed (Phase 5)
- API: Stubbed (Phase 3 — THIS IS NEXT)
- Frontend: Stub Tauri backend, skeleton React (Phase 3)
- Agentic Design: Complete design doc at docs/05-agentic-design.md

## CRITICAL: Product Vision
NexusLedger is NOT just an accounting engine — it is an AGENTIC PRODUCT.
Users interact through natural language (CLI chat, Telegram bot, WhatsApp, 
email, web chat). The system proactively reads emails, downloads attachments, 
classifies documents, creates transactions, and asks for approval via inline 
buttons. Users can send voice messages (transcribed with Whisper), forward 
vendor emails, photograph receipts — all without accounting knowledge.

Phase 3 MUST include:
- CLI `nexusledger chat` command (natural language → Task mapping)
- Basic NLU (intent extraction + entity recognition)
- WebSocket support for real-time chat
- React frontend with conversational sidebar

## CRITICAL: Quality Standard
Every feature must be world-class, QuickBooks-grade quality. No stubs, no MVPs.
- Accounting features: proper double-entry, verified math, edge case handling
- Agents: real processing logic, no mock returns, validate side-effects
- Tests: verify actual math, cross-agent flows, edge cases, database persistence
- When a task seems "done", ask: would a senior accountant trust this?

## Next: Phase 3 — End-to-End (API + Frontend + CLI Chat)
Read: RichdaleAccounting/Phases/TRACKER.md (Phase 3 section)

12 tasks:
1. Implement ApiServer::start() — bind axum with WebSocket support
2. Implement all API route handlers (accounts, transactions, invoices, reports, chat)
3. Add request middleware (ID, timing, error mapping)
4. Replace Tauri backend with nexus-core import
5. Add CORS + graceful shutdown
6. Add react-router to frontend
7. Build Account List page
8. Build Journal Entry form
9. Build Ledger/Transaction List page
10. Build Invoice pages
11. Add error boundaries + loading states + CHAT SIDEBAR
12. E2E test: UI → transaction → ledger

ALSO add (from agentic design, not in original tracker):
- CLI `nexusledger chat` command with basic NLU
- WebSocket /ws/chat endpoint for conversational interface
- Intent → Task mapping for: create_invoice, process_payment, record_receipt, 
  query_balance, generate_report

## Workflow Rules
- Commit and push after each phase completion
- Update Phases/TRACKER.md checkboxes as tasks complete
- Run `cargo check` and `cargo test` before each commit
- Get user approval before starting next phase
- If context gets saturated (2+ phases done), ping user for new session handoff

## Architecture Quick Reference
- Entry: nexus-core/src/main.rs → NexusLedger struct → AgentOrchestrator
- Agents: 9 types (Ledger, Reconciliation, Invoice, Payroll, Tax, Receipt, 
  Document, Audit, Reporting) — all share a single Ledger via Arc<RwLock<>>
- Dispatch: event-driven (tokio::sync::Notify), priority-sorted queue
- DB: SurrealDB kv-mem, 15 tables, migration runner, seed chart of accounts
- Accounting: Double-entry validated, 4 financial statements, progressive tax,
  YTD payroll, fuzzy reconciliation, hash-chained audit trail
- Frontend: Tauri + React + Vite + TypeScript (currently skeleton)

Begin by reading the tracker and Phase 3 section, then start Task 3.1.
