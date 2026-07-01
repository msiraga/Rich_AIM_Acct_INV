You are starting Phase 7: Production Hardening for NexusLedger, an agentic accounting
platform in Rust (Tauri + React frontend, SurrealDB backend, agent-based architecture).

## Repository & State
c:\Users\msira\Richdale_Accounting\Rich_AIM_Acct_INV
Latest commit: Phase 4 complete (287 tests, 0 failures)
Phases 5 and 6 are running in separate sessions — no overlap except Cargo.toml/api/mod.rs (central).

## Read These First
1. RichdaleAccounting/Phases/TRACKER.md                ← Phase 4 done, P7 next
2. RichdaleAccounting/Phases/00-strategy.md            ← methodology + principles
3. RichdaleAccounting/Phases/08-phase-7-production.md   ← original phase plan
4. RichdaleAccounting/Phases/phase-4-handoff.md        ← cross-phase parallelism guide
5. RichdaleAccounting/docs/02-architecture.md          ← system diagram

## Phase 7: Production Hardening — 15 Tasks (FINAL PHASE)

**Objective:** Performance, security, observability, packaging, and documentation.
After this phase, the project is shippable as v1.0.0.

### Current State (from codebase audit)
- `MonitorConfig` exists with `enable_prometheus: false`, `prometheus_port: 9090`
- `SystemMonitor` struct exists with `PrometheusExporter`, `Metrics`, `Alerts` infrastructure
- `PrometheusExporter` already has `update_from_system_status()` method
- `request_id_middleware` already generates UUID per request + measures response time
- `health_handler` already exists at `GET /health` returning orchestrator status
- `tauri.conf.json` already has: NSIS bundler config, macOS config, CSP header, tray-icon feature
- `src-tauri/Cargo.toml` already has `tray-icon` and `updater` plugins
- `database/INJECTION_AUDIT.md` confirms all SurrealDB queries use parameterized binds
- No `dangerouslySetInnerHTML`, `innerHTML`, `eval()`, or `Function()` in frontend
- **CSP vulnerability**: `tauri.conf.json` has `script-src 'self' 'unsafe-inline'` — inline scripts allowed, defeating XSS protection
- **DB bottleneck**: `Database` struct uses `Arc<Mutex<Option<Surreal<Db>>>>` — single connection behind a Mutex, every DB operation serializes
- No rate limiting, no CSRF tokens, no structured logging, no benchmarks, no user docs

### CRITICAL: Production-Grade Quality Standard (A+)
- **Performance:** 10K transactions with sub-2s response. Lock contention MEASURED with
  specific structures identified. Benchmarks reproducible with `criterion`. Memory growth tracked.
- **Security:** Zero injection vectors with regression prevention (clippy lint). CSP with NO
  `unsafe-inline`. CSRF double-submit cookies. Rate limiting per-role + per-endpoint.
- **Observability:** Prometheus in standard 0.0.4 text format. Request IDs propagated to ALL
  log lines. Structured JSON logging in production, pretty console in dev.
- **Packaging:** Installers work on clean machines. SurrealDB binary bundled. First-run wizard.
  Auto-update with signature verification. System tray with live sync status.
- **Documentation:** README with screenshots. API reference with curl examples. Architecture
  doc updated. Deployment guide. Developer setup guide. 10-minute quick-start.
- **Resilience:** Graceful shutdown drains in-flight requests. Backup/restore procedure
  documented. Secret management via environment variables + `.env` template.

## Maximum Parallelism Strategy

### File Conflict Analysis
```
Phase 7 NEW files (no conflicts):
  api/middleware.rs          ← 7.3 rate limiter
  api/routes/health.rs       ← 7.7 /ready endpoint (separate from existing /health)
  benches/performance.rs     ← 7.12 benchmarks
  tests/load/                ← 7.13 load test scripts
  docs/user/                 ← 7.14 user documentation
  docs/api/                  ← 7.14 API reference
  scripts/audit.sh           ← 7.15 Unix audit script
  scripts/audit.ps1          ← 7.15 Windows audit script
  .env.example               ← 7.15 secret management template

Phase 7 EXISTING files (potential conflicts — assigned to specific agents):
  monitor/mod.rs             ← 7.6 ONLY (Agent C)
  database/mod.rs            ← 7.2 ONLY (Agent L)
  database/* repos           ← 7.4 ONLY (Agent I) — regression prevention
  api/mod.rs                 ← COORDINATOR ONLY (wire rate limiter + routes)
  tauri.conf.json            ← 7.8 ONLY (Agent E) — fix CSP + bundler config
  src-tauri/Cargo.toml       ← COORDINATOR ONLY (add deps)
  nexus-ledger-tauri/src/    ← 7.5 ONLY (Agent H) — frontend security
  README.md                  ← 7.14 ONLY (Agent G)

Phase 7 READ-ONLY (no modifications, can run with anything):
  All other files            ← 7.1 lock audit (Agent A) — produces report only
```

### Round 1 — 12 Parallel Agents (no file conflicts)

| Agent | Task | Touches | A+ Quality Standard |
|-------|------|---------|---------------------|
| **A** | 7.1 Lock audit | READ-ONLY: all files | **Instrument specific structures**: measure lock wait time on `orchestrator.agents` (RwLock<HashMap>), `task_queue` (Mutex<VecDeque>), `ledger.accounts` (RwLock<BTreeMap>), `ledger.transactions` (RwLock<BTreeMap>), `Database.client` (Mutex<Option<Surreal>>). Use `tokio::time::Instant` to measure acquire time. **Produce report**: top 5 hot locks with avg/p99 wait time, contention ratio (contended/total), nesting depth. **Flag**: any `Arc<RwLock<>>` nesting > 3 levels (e.g., `orchestrator.agents.read().await` → `agent.lock().await` → `ledger.accounts.write().await` = 3 levels). **Actionable fixes**: suggest DashMap for agent registry, sharded locks for transactions, connection pool for DB. |
| **B** | 7.3 Rate limiter | CREATES `api/middleware.rs` | **Token-bucket** via `governor` crate. **Per-role limits**: Admin=1000/min, Manager=500/min, User=100/min, Viewer=50/min. **Per-endpoint overrides**: `/api/auth/login` = 10/min (brute force prevention), `/api/auth/register` = 5/min, `/api/v1/import/csv` = 5/min. **Burst capacity**: 2x steady-state rate (allows short bursts). **Response headers**: `X-RateLimit-Limit`, `X-RateLimit-Remaining`, `X-RateLimit-Reset` (Unix timestamp). **429 body**: `{"success":false,"error":"Rate limit exceeded","retry_after":N}`. **Extract AuthUser** from request extensions (set by auth middleware) to determine role. **IP-based fallback** for unauthenticated routes (login/register). |
| **C** | 7.6 Prometheus | MODIFIES `monitor/mod.rs` ONLY | **Reference existing `PrometheusExporter`** — extend it, don't replace. **Metrics**: `nexus_http_requests_total{method,path,status}` (counter), `nexus_http_request_duration_seconds{method,path}` (histogram, buckets [0.005, 0.01, 0.05, 0.1, 0.5, 1, 5, 10]), `nexus_agents_active` (gauge), `nexus_agents_total` (gauge), `nexus_tasks_processed_total` (counter), `nexus_tasks_failed_total` (counter), `nexus_db_query_duration_seconds` (histogram), `nexus_sync_pending_changes` (gauge from Phase 6). **Wire from `request_id_middleware`**: the existing middleware already measures response time — feed it into the Prometheus histogram. **Exposition**: `GET /metrics` returns `text/plain; version=0.0.4; charset=utf-8`. **NO auth required** on `/metrics` (Prometheus scrapers can't send JWT) — but restrict to localhost. |
| **D** | 7.7 Health endpoints | CREATES `api/routes/health.rs` | **Existing `/health` already works** — don't duplicate. **Add `/ready`**: returns 200 `{"status":"ready","db":"connected","agents_initialized":true,"accounts_count":20}` when: (1) DB connection is Some (try `db.is_connected().await`), (2) orchestrator `is_running` is true, (3) `ledger.accounts.read().await.len() > 0`. Returns 503 `{"status":"not_ready","reason":"db_not_connected"}` otherwise. **Kubernetes-compatible**: `/health` = liveness (process alive), `/ready` = readiness (can serve requests). |
| **E** | 7.8 Windows installer | MODIFIES `tauri.conf.json` ONLY | **Fix CSP**: change `script-src 'self' 'unsafe-inline'` to `script-src 'self'` (Vite doesn't need inline scripts in production build). Keep `style-src 'self' 'unsafe-inline'` (CSS injection is lower risk). **Bundle SurrealDB**: add `surreal.exe` to `externalBin` array — download from GitHub releases, place in `src-tauri/binaries/`. **NSIS config**: add `displayLanguageSelector: false`, `installerIcon`, `installMode: perMachine`. **First-run wizard**: Tauri command that checks if `JWT_SECRET` env var is set, if not generates one and writes to `.env` file in app data directory. **Windows Registry**: NSIS automatically creates uninstall entries. **Shortcut**: NSIS creates Start Menu + Desktop shortcuts by default. |
| **F** | 7.12 Benchmarks | CREATES `benches/performance.rs` | **Use `criterion`** with `iter_batched`. **Benchmarks**: (1) `create_10k_transactions` — create 10K balanced transactions via `ledger.record_transaction()`, target < 2s. (2) `list_10k_transactions` — list all 10K, target < 500ms. (3) `generate_balance_sheet_10k` — generate balance sheet with 10K txns, target < 2s. (4) `generate_trial_balance_10k` — target < 1s. (5) `csv_import_10k_rows` — parse + import 10K CSV rows, target < 5s. (6) `sync_push_1k_dirty` — push 1K dirty records to SurrealDB, target < 10s. **Memory tracking**: use `jemalloc_ctl::stats` or `sysinfo` crate to track peak memory during benchmarks. **Reproducible**: fixed seed for test data generation, `criterion` warm-up 3s, measurement 5s. |
| **G** | 7.14 Docs | CREATES `docs/user/`, `docs/api/`, `README.md` | **README.md**: project description, architecture diagram, feature list, screenshots (use Tauri's `tauri dev` to capture), **10-minute quick-start** (clone → `cargo build` → `tauri dev` → register → create first transaction → view dashboard), FAQ (common errors: JWT_SECRET not set, SurrealDB not found, port 4000 in use). **docs/user/**: install guide (Windows + macOS), first transaction walkthrough, reports guide (trial balance, balance sheet, income statement, cash flow, AR aging), invoice/bill workflow, troubleshooting. **docs/api/**: endpoint reference with curl examples for every endpoint (auth, accounts, transactions, invoices, AP, budgets, assets, export, import, edge sync). **docs/developer/**: dev setup (Rust + Node + Tauri CLI), test commands, contribution guide. |
| **H** | 7.5 Frontend security | MODIFIES `nexus-ledger-tauri/src/` files ONLY | **XSS audit**: grep all `.tsx` files for `dangerouslySetInnerHTML` (none found — confirm). Verify all user-generated content (invoice descriptions, customer names, vendor names, chat messages) is rendered via JSX (auto-escaped). **CSRF**: add double-submit cookie pattern — `api.ts` reads `XSRF-TOKEN` cookie and sends as `X-XSRF-TOKEN` header on all POST/PUT/DELETE. Backend middleware verifies match. For Tauri desktop (no cross-origin), CSRF risk is low but API on localhost:4000 is accessible from browsers. **CSP**: ensure `tauri.conf.json` has `script-src 'self'` (Agent E fixes this, Agent H verifies). **Input validation**: add client-side validation for all forms (max length, allowed characters, numeric ranges). |
| **I** | 7.4 SQL injection | MODIFIES `database/` repo files ONLY | **Existing audit confirms queries are parameterized** — focus on REGRESSION PREVENTION. (1) Add `#![deny(clippy::format_in_query)]` or equivalent lint to `database/mod.rs`. (2) Add a test that greps all `.rs` files in `database/` for `format!(` near `.query(` and fails if found. (3) Audit the NLU chat handler in `api/mod.rs` — verify user text input doesn't construct SurrealQL. (4) Audit CSV import — verify account numbers from CSV are used as bind parameters, not interpolated. (5) Add `cargo clippy` to CI with `-D warnings` specifically for `database/` module. |
| **J** | 7.13 Load test | CREATES `tests/load/` directory | **Scripts**: (1) `load_get.sh` — `wrk -t4 -c100 -d60s http://localhost:4000/api/v1/accounts -H "Authorization: Bearer $TOKEN"`. (2) `load_post.sh` — `wrk -t4 -c50 -d60s -s post_script.lua http://localhost:4000/api/v1/transactions -H "Authorization: Bearer $TOKEN"`. (3) `load_concurrent_rw.sh` — 100 readers + 10 writers simultaneously. (4) `load_ws.sh` — 50 concurrent WebSocket connections to `/ws/chat?token=$TOKEN`. **Token generation**: script that registers a test user, logs in, extracts JWT, exports as `$TOKEN`. **Results doc**: `tests/load/RESULTS.md` with table: endpoint, concurrent users, RPS, p50, p99, errors. **Pass criteria**: 0 errors, p99 < 500ms for GET, p99 < 1s for POST. |
| **K** | 7.15 Prep | CREATES `scripts/audit.sh`, `scripts/audit.ps1`, `.env.example` | **audit.sh** (Unix): `#!/bin/bash\nset -e\ncargo test --all\ncargo audit\ncargo clippy --all -- -D warnings\ncargo fmt --all --check\ncargo deny check\nnpm audit --prefix ../nexus-ledger-tauri\n`. **audit.ps1** (Windows): PowerShell equivalent. **`.env.example`**: template with all env vars: `JWT_SECRET=`, `API_HOST=127.0.0.1`, `API_PORT=4000`, `DATABASE_URL=`, `EDGE_STORAGE_PATH=`, `EDGE_ENCRYPT_DATA=true`, `MISTRAL_OCR_API_KEY=`, `GGUF_MODEL_PATH=`, `MONITOR_ENABLE_PROMETHEUS=true`. **Verify ALL freeze tokens**: script checks Phase 4 (auth, AP, AR, cash flow, CSV, multi-currency, budget, assets), Phase 5 (OCR, AI, embeddings, anomalies), Phase 6 (offline, sync, conflict, encryption, compression), Phase 7 (benchmarks, load, security, packaging). |
| **L** | 7.2 Pooling | MODIFIES `database/mod.rs` ONLY (after Agent A) | **Current bottleneck**: `Arc<Mutex<Option<Surreal<Db>>>>` — single connection, every operation serializes through one Mutex. **Fix**: replace with connection pool. Use `deadpool-surreal` or `bb8-surreal` if available, else implement simple pool: `Vec<Surreal<Db>>` behind a `Mutex`, `acquire()` pops one, `release()` pushes back. Pool size: 10 (configurable). **Migration path**: `Database::connect()` creates pool of N connections instead of one. `db()` method returns a connection from pool (not the same connection every time). **Backward compatible**: existing `db.client()` method still works (returns first connection from pool). **Test**: 100 concurrent `db.db()` calls should not serialize (measure throughput before/after). |

### Round 2 — Dependent Tasks (after Round 1)
| Agent | Task | Depends On | A+ Quality Standard |
|-------|------|------------|---------------------|
| **M** | 7.9 macOS DMG | Agent E (Windows installer / CSP fix) | **Tauri bundler** already has macOS config. **Universal binary**: `cargo tauri build --target universal-apple-darwin` (arm64 + x86_64). **Code signing**: self-signed cert for dev (`tauri signer generate`), or Apple Developer ID for distribution. **Notarization**: `xcrun notarytool submit app.dmg --keychain-profile "NexusLedger"` — required for Gatekeeper. **DMG**: Tauri generates DMG automatically with `bundle.macOS.targets`. **Test**: mount DMG on clean macOS, drag to Applications, verify app launches. |
| **N** | 7.10 System tray | Agent E (installer config) | **`tray-icon` feature already enabled**. **Window close intercept**: Tauri `on_window_event` — intercept `CloseRequested`, call `window.hide()` instead of closing. **Tray icon**: reflects sync status from Phase 6's `EdgeManager.get_sync_status()` — green (up to date), yellow (syncing), orange (offline), red (error). **Right-click menu**: "Open NexusLedger", "Sync Now" (only when online + pending > 0), "N pending changes" (label only), separator, "Quit". **Tooltip**: "NexusLedger — Up to date (last sync: 2m ago)" or "NexusLedger — Offline (15 changes pending)". **Dynamic menu**: update every 5s from sync status. |
| **O** | 7.11 Auto-update | Agent E (installer config) | **`tauri-plugin-updater` already in Cargo.toml**. **Signing key**: `tauri signer generate -w ~/.tauri/nexusledger.key` — store securely, add public key to `tauri.conf.json` `updater.pubkey`. **Update URL**: GitHub releases — `https://github.com/msiraga/Rich_AIM_Acct_INV/releases/latest/download/latest.json`. **Check frequency**: on startup + every 7 days. **Update flow**: check URL → if newer version → prompt user "Version X.Y.Z available. Update now?" → download → verify signature → install → restart. **Silent fail**: if update check fails (no internet), don't block app startup. **Channel**: stable (default), beta (opt-in via env var). |

### Round 3 — Central Coordinator (after ALL agents)
| Step | File(s) | What |
|------|---------|------|
| 1 | `Cargo.toml` | Add: `governor = "0.6"`, `prometheus = "0.13"`, `criterion = { version = "0.5", features = ["html_reports"] }` (dev), `deadpool-surreal` or manual pool, `sysinfo = "0.30"` (for memory benchmarks), `cargo-deny` (dev) |
| 2 | `api/mod.rs` | Wire rate limiter middleware (Agent B) AFTER auth middleware (needs AuthUser). Add `/ready` route (Agent D). Add `/metrics` route (Agent C) — NO auth, localhost only. Add CSRF verification middleware for POST/PUT/DELETE. Add `X-RateLimit-*` headers to all responses. |
| 3 | `tauri.conf.json` | Apply CSP fix from Agent E (remove `unsafe-inline` from script-src). Apply updater config from Agent O (pubkey, endpoints). |
| 4 | `src-tauri/Cargo.toml` | Add `tauri-plugin-updater` config, tray icon dependencies. |
| 5 | `main.rs` | Wire `SystemMonitor` into startup (call `monitor.initialize()`). Start Prometheus exporter if enabled. Add structured logging: `tracing_subscriber::fmt().json()` in production, pretty in dev. Add graceful shutdown: `shutdown_signal()` calls `axum::serve` graceful shutdown with 30s drain timeout. |

### Round 4 — Final Gate (7.15)
| Step | What |
|------|------|
| 1 | Run `scripts/audit.sh` (or `audit.ps1` on Windows): `cargo test --all` + `cargo audit` + `cargo clippy -- -D warnings` + `cargo fmt --check` + `cargo deny check` + `npm audit` |
| 2 | Run benchmarks: `cargo bench` — verify 10K < 2s for create/list/balance_sheet |
| 3 | Run load tests: `tests/load/load_get.sh` + `load_post.sh` — verify 0 errors, p99 < 500ms |
| 4 | Verify ALL freeze tokens from ALL phases (4+5+6+7) |
| 5 | Build installers: `cargo tauri build` — verify MSI (Windows) + DMG (macOS) |
| 6 | Fix any remaining issues |
| 7 | Tag release: `git tag v1.0.0 && git push origin v1.0.0` |
| 8 | Create GitHub release with release notes + installer downloads |

## Cross-Cutting Requirements (apply to ALL agents)

### Structured Logging
- Production: `tracing_subscriber::fmt().json().with_env_filter(EnvFilter::from_default_env())`
- Development: `tracing_subscriber::fmt().pretty().with_env_filter("debug")`
- Request IDs: `request_id_middleware` already generates UUIDs — propagate via `tracing::span!` to all log lines
- All `info!`, `warn!`, `error!` calls should include the request ID when available

### Graceful Shutdown
- Replace existing `shutdown_signal()` with: catch Ctrl+C → log "Shutting down..." → `axum::serve` graceful shutdown with 30s drain → wait for in-flight requests to complete → close DB connections → log "Shutdown complete"
- System tray: closing to tray should NOT trigger shutdown — only "Quit" menu item triggers shutdown

### Secret Management
- `.env.example` template (Agent K creates) lists ALL env vars
- Production: load from `.env` file in app data directory (Tauri `app_data_dir`)
- Never log secrets: `JWT_SECRET`, `MISTRAL_OCR_API_KEY`, encryption keys
- `zeroize` crate clears secrets from memory after use

### Backup/Restore
- Document in `docs/user/backup.md`: (1) SurrealDB data export (`surreal export`), (2) SQLite local DB copy, (3) encryption key backup (print recovery key during first-run wizard)
- Restore: `surreal import` for SurrealDB, copy SQLite file back, enter recovery key

## Agent Prompt Template
```
You are implementing Phase 7 Task [7.X] [NAME] for NexusLedger, a Rust
accounting platform in its FINAL production-hardening phase.

Read the existing code you're building on (specified in your task).
All 287 existing tests must still pass after your changes.

[SPECIFIC_CONTEXT — see A+ Quality Standard column above]

Follow existing code style. Include tests. Use thiserror for errors.
All financial amounts use rust_decimal::Decimal.
```

## Key File Paths
```
nexus-core/src/
├── api/
│   ├── mod.rs           ← EXISTS — coordinator wires rate limiter + routes + CSRF
│   ├── auth.rs          ← EXISTS — JWT, RBAC extractors (from Phase 4)
│   ├── middleware.rs    ← NEW Agent B — rate limiter (governor) + CSRF verification
│   └── routes/
│       └── health.rs    ← NEW Agent D — /ready endpoint (liveness already exists)
├── monitor/
│   └── mod.rs           ← EXISTS — Agent C extends PrometheusExporter
├── database/
│   ├── mod.rs           ← EXISTS — Agent L adds connection pool
│   ├── INJECTION_AUDIT.md ← EXISTS — Agent I adds regression prevention
│   └── *.rs             ← EXISTS — Agent I adds clippy lint + grep test
├── benches/
│   └── performance.rs   ← NEW Agent F — criterion benchmarks (6 scenarios)
└── tests/
    └── load/            ← NEW Agent J — wrk scripts + RESULTS.md

nexus-ledger-tauri/
├── src/
│   ├── lib/api.ts       ← EXISTS — Agent H adds CSRF token handling
│   └── (all .tsx files) ← EXISTS — Agent H verifies XSS safety + adds input validation
├── src-tauri/
│   ├── tauri.conf.json  ← EXISTS — Agent E fixes CSP + bundler config
│   ├── Cargo.toml       ← EXISTS — coordinator adds deps
│   └── binaries/        ← NEW — surreal.exe binary for bundling
├── index.html           ← generated by Vite (CSP in tauri.conf.json applies)

scripts/
├── audit.sh             ← NEW Agent K — Unix: cargo test + audit + clippy + fmt + deny
├── audit.ps1            ← NEW Agent K — Windows PowerShell equivalent
└── .env.example         ← NEW Agent K — all env vars template

docs/
├── user/                ← NEW Agent G — install + walkthrough + reports + troubleshooting
├── api/                 ← NEW Agent G — endpoint reference with curl
├── developer/           ← NEW Agent G — dev setup + test + contribution
└── user/backup.md       ← NEW Agent G — backup/restore procedure
```

## Freeze Token 7 (FINAL — all must pass — A+ standard)
- [ ] `cargo test --all` green (all unit + integration + e2e from ALL phases)
- [ ] `cargo audit` zero vulnerabilities
- [ ] `cargo clippy --all -- -D warnings` clean
- [ ] `cargo fmt --all --check` clean
- [ ] `cargo deny check` passes (licenses + advisories)
- [ ] `npm audit` clean (frontend dependencies)
- [ ] 10K tx benchmark: create < 2s, list < 500ms, balance sheet < 2s, trial balance < 1s
- [ ] 10K CSV import < 5s
- [ ] 1K sync push < 10s
- [ ] 100 concurrent GET requests: 0 errors, p99 < 500ms
- [ ] 50 concurrent POST requests: 0 errors, p99 < 1s
- [ ] 50 concurrent WebSocket connections: 0 errors
- [ ] No SurrealQL injection vectors (regression test passes)
- [ ] No XSS vectors (no dangerouslySetInnerHTML, CSP script-src 'self' only)
- [ ] CSRF protection on all POST/PUT/DELETE endpoints (double-submit cookie)
- [ ] Rate limiting: 429 returned when limit exceeded, correct headers
- [ ] `GET /health` returns 200 (liveness)
- [ ] `GET /ready` returns 200 when DB + agents ready, 503 otherwise
- [ ] `GET /metrics` returns valid Prometheus 0.0.4 text format (no auth, localhost only)
- [ ] Request IDs appear in ALL log lines
- [ ] Structured JSON logging in production
- [ ] Graceful shutdown drains in-flight requests (30s timeout)
- [ ] Windows MSI/NSIS installer builds and installs on clean machine
- [ ] SurrealDB binary bundled in installer
- [ ] First-run wizard generates JWT_SECRET if not set
- [ ] macOS DMG builds (universal binary: arm64 + x86_64)
- [ ] macOS DMG notarized (or documented as self-signed for dev)
- [ ] System tray: close minimizes to tray, icon shows sync status, right-click menu works
- [ ] Auto-update: checks GitHub releases, verifies signature, prompts user
- [ ] `.env.example` documents all environment variables
- [ ] User documentation complete (README + quick-start + FAQ + install + walkthrough)
- [ ] API reference with curl examples for every endpoint
- [ ] Developer setup guide
- [ ] Backup/restore procedure documented
- [ ] All freeze tokens from Phase 4, 5, 6 still pass

## Dependencies Reminder
- `governor = "0.6"` for rate limiting (Agent B)
- `prometheus = "0.13"` for metrics (Agent C)
- `criterion = { version = "0.5", features = ["html_reports"] }` for benchmarks (Agent F, dev-dep)
- `deadpool-surreal` or manual pool for DB pooling (Agent L)
- `sysinfo = "0.30"` for memory tracking in benchmarks (Agent F)
- `cargo-deny` (dev) for license/advisory checks (Agent K)
- `zeroize` already in Phase 6 deps
- All added by coordinator in Round 3 — agents do NOT touch Cargo.toml

After Phase 7: tag `v1.0.0`, create GitHub release with installer downloads.
This is the final phase. The project is shippable.

Begin Round 1: launch agents A through L in parallel (12 agents).
