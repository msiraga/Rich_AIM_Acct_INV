You are continuing development of NexusLedger, an agentic accounting platform
in Rust (Tauri + React frontend, SurrealDB backend, agent-based architecture).

## Repository
c:\Users\msira\Richdale_Accounting\Rich_AIM_Acct_INV
Latest commit on main: (Phase 4 just committed — check git log)

## Read These First
1. RichdaleAccounting/Phases/TRACKER.md        ← Phase 4 done, Phase 5 next
2. RichdaleAccounting/Phases/00-strategy.md    ← methodology + principles
3. RichdaleAccounting/Phases/06-phase-5-ai-pipeline.md ← THIS PHASE'S PLAN
4. RichdaleAccounting/Phases/phase-4-handoff.md (this file)
5. RichdaleAccounting/docs/02-architecture.md  ← system diagram

## Phase 4: Auth & Accounting Completeness — COMPLETED ✅

### Auth Track
- 4.1: argon2id hashing, empty-password rejection, 8 tests
- 4.2: JWT with token_type (access vs refresh), strict 0s leeway, refresh rotation, default secret refusal, WS auth via ?token=, 8 tests
- 4.3: Register (email + strength validation), Login (timing-attack fix with dummy hash), Refresh (rotation, role re-fetched from DB)
- 4.4: RBAC — UserRole levels (Guest→Admin), RequireViewer/User/Manager/Admin extractors on all endpoints, admin user-management endpoints
- 4.5: React Login/Register, ProtectedRoute, centralized api.ts with Bearer auth + 401-refresh interceptor, role-based nav, WS auth, isLoading fix

### Accounting Track
- 4.6: AP workflow (vendor→bill→payment), partial payments, multi-line bills, ApAgent, 4 API endpoints, 11 tests
- 4.7: AR aging (4 buckets), partial payment support, as-of date, metadata-based matching
- 4.8: Cash flow — 3 GAAP sections, AccountType classification, depreciation via 1050, reconciliation assertion
- 4.9: CSV import — quoted field parser, balance validation, 15 tests
- 4.10: CSV/OFX export — account name resolution, date filters, signed OFX TRNAMT, BANKACCTFROM, LEDGERBAL, 8 tests
- 4.11: Multi-currency — ExchangeRates, TransactionEntry currency fields (currency, exchange_rate, base_currency_amount)
- 4.12: Budget — sign-based variance, period overlap logic, BudgetManager, 3 tests
- 4.13: Fixed assets — SL/DDB depreciation, post_depreciation() auto-journal, dispose_asset() with gain/loss, 8 tests
- 4.14: 20 integration tests covering all freeze token conditions

### Verification
- cargo check: 0 errors
- cargo test: 287 passed, 0 failed
- Frontend: TypeScript compiles clean
- Freeze Token 4: all 6 conditions met

## IMPORTANT: Key Changes vs Phase 3 (Breaking)
- **TransactionEntry** has 3 new fields: `currency`, `exchange_rate`, `base_currency_amount` with defaults ("USD", None, None). Always use `..Default::default()` in struct literals.
- **total_amount()** now returns debit-sum only (not sum of all entries)
- **Chart of accounts** expanded from 18→20: added 1050 (Accumulated Depreciation), 5050 (Depreciation Expense)
- **record_transaction()** reassigns transaction.number to "TRX-00000XXX" format
- **JWT access token TTL** reduced to 30 min (1800s), refresh 7 days (604800s)
- **Auth middleware** skips /auth and /health, allows /ws/ with ?token= query param

## Phase 5: AI Pipeline — NEXT

Read: RichdaleAccounting/Phases/06-phase-5-ai-pipeline.md

### CROSS-PHASE STRATEGY: Phases 5 + 6 + 7 can all run in parallel.

All three phases create mostly NEW files. The only shared files across phases are
`Cargo.toml`, `api/mod.rs`, and frontend `App.tsx` — these are handled centrally
after all agents complete. This enables launching agents for all three phases
simultaneously.

## Maximum Parallelism: 13 Agents Across All 3 Phases

### File Conflict Map
```
                    ┌──────────────┐
                    │  Cargo.toml  │  ← CENTRAL (all 3 phases add deps)
                    │  api/mod.rs  │  ← CENTRAL (P5 health + P7 health/ready/rate-limit)
                    │  App.tsx     │  ← CENTRAL (P5 Documents + P6 SyncStatus routes)
                    └──────────────┘
        ┌──────────────────┼──────────────────┐
        │                  │                  │
   ┌────┴────┐        ┌────┴────┐        ┌────┴────┐
   │ Phase 5 │        │ Phase 6 │        │ Phase 7 │
   │ 6 agents│        │ 8 agents│        │ 5 agents│
   └─────────┘        └─────────┘        └─────────┘
```

### Round 1 — 13 Parallel Agents (ALL new files, no conflicts)

| # | Phase | Agent | Creates | Dependencies |
|---|-------|-------|---------|-------------|
| 1 | P5 | OCR | `ai/ocr.rs` | None |
| 2 | P5 | PDF | `ai/pdf.rs` | None |
| 3 | P5 | Embeddings | `ai/embeddings.rs` | None |
| 4 | P5 | Anomaly | `ai/analysis.rs` | None |
| 5 | P5 | Categorize | `ai/classification.rs` | None |
| 6 | P5 | Upload UI | `pages/Documents.tsx` | Frontend only |
| 7 | P6 | Encryption | `edge/encryption.rs` | None |
| 8 | P6 | Compression | `edge/compression.rs` | None |
| 9 | P6 | Local DB | `edge/local_db.rs` | None (schema self-contained) |
| 10 | P6 | Sync UI | `components/SyncStatus.tsx` | Frontend only |
| 11 | P7 | Rate Limiter | `api/middleware.rs` | None |
| 12 | P7 | Health Endpoints | `api/routes/health.rs` | None |
| 13 | P7 | Benchmarks | `benches/performance.rs` | None |
| 14 | P7 | Documentation | `README.md`, `docs/user/` | None |

### Phase 6 Sequential Chain (must run after Agent 9)
Agents 7-9 are independent. But `edge/store.rs`, `edge/tracking.rs`, `edge/sync.rs`,
and `edge/conflict.rs` form a logical chain — each builds on the previous schema:

| Agent | File | Depends On |
|-------|------|------------|
| 6A | `edge/store.rs` | Agent 9 (local_db.rs schema) |
| 6B | `edge/tracking.rs` | 6A (store CRUD ops) |
| 6C | `edge/sync.rs` | 6B (change tracking) |
| 6D | `edge/conflict.rs` | 6C (sync engine) |

### Round 2 — Central Coordinator (after all agents)
| Step | Files | What |
|------|-------|------|
| 1 | `Cargo.toml` | Add ALL deps: llama-cpp-rs, reqwest (Mistral OCR4), rusqlite, aes-gcm, lz4, governor, prometheus |
| 2 | `ai/mod.rs` | Register 5 submodules + wire OCR→AI pipeline (5.4) |
| 3 | `agents/document.rs` | Wire auto-transaction from extraction (5.5) |
| 4 | `api/mod.rs` | Add AI health route (5.9) + health/ready routes (7.7) + rate limiter layer (7.3) |
| 5 | `edge/mod.rs` | Register 7 submodules + offline toggle (6.6) |
| 6 | `monitor/mod.rs` | Add Prometheus metrics export (7.6) |
| 7 | `App.tsx` | Add Documents page route + SyncStatus component |
| 8 | `index.css` | Add styles for Documents page + SyncStatus component |

### Round 3 — Integration Tests
| Phase | Test | File |
|-------|------|------|
| P5 | Receipt→OCR→AI→Transaction E2E | `tests/integration/ai.rs` |
| P6 | Offline→5 txns→Online→Sync E2E | `tests/integration/edge.rs` |
| P7 | 100 concurrent requests load test | `tests/load/` |

### Round 4 — Final Gate (Phase 7 only)
| Step | What |
|------|------|
| 7.15 | `cargo test --all` + `cargo audit` + `cargo clippy` |
| 7.8-7.11 | Tauri packaging (Windows MSI + macOS DMG + system tray + auto-update) |

### Agent Prompt Template (for ANY Round 1 agent)
```
You are implementing Phase [N] Task [N.X] for NexusLedger, a Rust accounting
platform. Read the existing module's mod.rs for patterns.

CREATE ONLY ONE NEW FILE. Do NOT edit Cargo.toml, api/mod.rs, App.tsx,
or any module's mod.rs. Your file will be registered centrally.

Phase-specific context:
- P5 (AI): Mistral OCR4 API for PDFs, GGUF models via llama-cpp-rs (NOT Ollama),
  graceful degradation required, GGUF paths in project memory.
- P6 (Edge): SQLite via rusqlite, AES-256-GCM encryption, lz4 compression,
  last-write-wins conflict resolution, eventually consistent sync.
- P7 (Prod): governor rate limiter, prometheus metrics, tower-http CSRF/CSP,
  serde_json for health responses, criterion for benchmarks.

Include unit tests in your file.
```

## AI Stack Preferences (Phase 5)
- **OCR:** Mistral OCR4 API (cloud) — NOT Tesseract
- **LLM:** Two GGUF models already downloaded, NOT Ollama:
  - Mistral: `C:\Users\msira\Downloads\Self_Healing_Python_Architect\mistral.gguf`
  - Qwen3-4B: `C:\Users\msira\Downloads\aletheia_gauntlet\models\Qwen3-4B-Q8_0.gguf`
- **Inference:** `llama-cpp-rs` crate
- **Fallback:** `pdf-extract` crate for offline PDFs when Mistral API unavailable

## Freeze Token 5 (all must pass)
- [ ] Receipt photo → OCR text → AI JSON → transaction created
- [ ] AI degrades gracefully when Mistral API / GGUF unavailable
- [ ] Anomaly detection flags test case
- [ ] Embeddings searchable by similarity
- [ ] cargo test: 0 failures

## Freeze Token 6 (all must pass)
- [ ] CRUD works offline against local SQLite with `_dirty=true` flag
- [ ] Online sync: dirty records pushed to SurrealDB
- [ ] Pull: remote changes since last_sync appear in local SQLite
- [ ] Conflict: last-write-wins, logged in audit trail
- [ ] Sync status UI shows correct state (offline/syncing/up-to-date/error)
- [ ] Sensitive fields encrypted at rest (AES-256-GCM)
- [ ] Large documents compressed (lz4, measurable reduction)
- [ ] Integration test: offline 5 txns → online → verify in SurrealDB → local matches

## Freeze Token 7 (FINAL — all must pass)
- [ ] `cargo test --all` green
- [ ] `cargo audit` zero vulnerabilities
- [ ] `cargo clippy -- -D warnings` clean
- [ ] 10K tx benchmark < 2s (create, list, balance sheet)
- [ ] 100 concurrent requests: zero errors, p99 < 500ms
- [ ] No SurrealQL injection, no XSS vectors
- [ ] CSRF protection on state-changing endpoints
- [ ] `/health` returns 200, `/ready` returns 200, `/metrics` valid Prometheus
- [ ] Windows MSI/EXE + macOS DMG build and install
- [ ] System tray + auto-update functional
- [ ] User documentation complete

Begin by launching 14 parallel agents for Round 1, then proceed to Round 2 central wiring.
