You are starting Phase 7: Production Hardening for NexusLedger, an agentic accounting
platform in Rust (Tauri + React frontend, SurrealDB backend, agent-based architecture).

## Repository & State
c:\Users\msira\Richdale_Accounting\Rich_AIM_Acct_INV
Latest commit: Phase 4 complete (287 tests, 0 failures)
Phases 5 and 6 are running in separate sessions — no overlap except Cargo.toml/App.tsx (central).

## Read These First
1. RichdaleAccounting/Phases/TRACKER.md                ← Phase 4 done, P7 next
2. RichdaleAccounting/Phases/00-strategy.md            ← methodology + principles
3. RichdaleAccounting/Phases/08-phase-7-production.md   ← THIS PHASE'S PLAN
4. RichdaleAccounting/Phases/phase-4-handoff.md        ← cross-phase parallelism guide
5. RichdaleAccounting/docs/02-architecture.md          ← system diagram

## Phase 7: Production Hardening — 15 Tasks (FINAL PHASE)

**Objective:** Performance, security, observability, packaging, and documentation.
After this phase, the project is shippable.

### Current State
- `MonitorConfig` exists with `enable_prometheus: false`, `prometheus_port: 9090`
- `SystemMonitor` struct exists with `Metrics` and `Alerts` infrastructure
- No rate limiting, no CSRF/CSP, no health/ready endpoints
- No benchmarks, no packaging config, no user docs

### CRITICAL: Production-Grade Quality Standard
- **Performance:** Must handle 10K transactions with sub-2s response times. Lock contention
  must be measured, not guessed. Benchmarks must be reproducible.
- **Security:** Zero SurrealQL injection vectors. Every user-rendered string sanitized.
  CSRF tokens on all state-changing endpoints. CSP headers on all responses.
- **Observability:** Prometheus metrics in standard format. Health/ready endpoints
  return correct status codes. Log levels configurable per module.
- **Packaging:** Installers must work on clean machines (no pre-installed Rust/SurrealDB).
  Auto-update must verify signatures. System tray must minimize to tray, not close.
- **Documentation:** README with screenshots, quick-start that works from clone to first
  transaction in under 10 minutes, FAQ covering common errors.

## Maximum Parallelism Strategy

### File Conflict Analysis
```
Phase 7 NEW files:
  api/middleware.rs          ← 7.3 rate limiter + 7.5 CSRF/CSP
  api/routes/health.rs       ← 7.7 health/ready endpoints
  benches/performance.rs     ← 7.12 benchmarks
  tests/load/                ← 7.13 load test (directory)
  docs/user/                 ← 7.14 user documentation (directory)

Phase 7 EXISTING files touched:
  monitor/mod.rs             ← 7.6 Prometheus metrics
  database/mod.rs            ← 7.2 connection pooling
  database/* (all repos)     ← 7.4 SurrealQL injection audit (read + fix)
  api/mod.rs                 ← 7.3 rate limiter wiring + 7.7 health routes
  nexus-ledger-tauri/src-tauri/ ← 7.8-7.11 packaging config
  nexus-ledger-tauri/src/    ← 7.5 frontend XSS/CSRF/CSP audit
  README.md                  ← 7.14 documentation

Phase 7 READ-ONLY (no modifications, can run with anything):
  Various files              ← 7.1 lock contention audit (profiling)
```

### Round 1 — 11 Parallel Agents (no file conflicts)

| Agent | Task | Touches | Quality Standard |
|-------|------|---------|-----------------|
| **A** | 7.1 Lock audit | READ-ONLY: profiles orchestrator + ledger under concurrent access | Use `tokio-console` or manual instrumentation. Produce report: top 5 hot locks, avg wait time, suggested fixes. Flag any `Arc<RwLock<>>` nesting deeper than 3 levels. |
| **B** | 7.3 Rate limiter | CREATES `api/middleware.rs` (new only) | Token-bucket via `governor` crate. Configurable per-role: Admin=1000/min, User=100/min, Viewer=50/min. Returns 429 with `Retry-After` header. Include IP-based + user-based limiters. |
| **C** | 7.6 Prometheus | MODIFIES `monitor/mod.rs` | Export `/metrics` in Prometheus text format. Gauges: `nexus_agents_active`, `nexus_tasks_total`. Counters: `nexus_requests_total`, `nexus_errors_total`. Histogram: `nexus_request_duration_seconds` with buckets [0.01, 0.05, 0.1, 0.5, 1, 5]. |
| **D** | 7.7 Health endpoints | CREATES `api/routes/health.rs` (new only) | `GET /health` → 200 `{"status":"ok"}` if process alive. `GET /ready` → 200 `{"status":"ready","db":"connected","agents":9}` if DB connected + agents initialized. Both return JSON. |
| **E** | 7.8 Windows installer | MODIFIES `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` | Configure Tauri bundler: MSI target, app icon, license, auto-start backend. Bundle SurrealDB binary (`surreal.exe`). Test with `cargo tauri build`. |
| **F** | 7.12 Benchmarks | CREATES `benches/performance.rs` (new only) | Use `criterion` crate. Benchmarks: `create_10k_transactions`, `list_10k_transactions`, `generate_balance_sheet_10k`. Target: each < 2 seconds. Include warm-up phase. |
| **G** | 7.14 Docs | CREATES `docs/user/`, MODIFIES `README.md` | README: project description, screenshot (terminal + UI), quick-start (5 steps), FAQ. User docs: install guide, first transaction walkthrough, reports guide, troubleshooting. |
| **H** | 7.5 Frontend security | AUDITS + MODIFIES `nexus-ledger-tauri/src/` | Check all JSX for XSS (no `dangerouslySetInnerHTML`, sanitize user content). Add CSRF tokens to all POST/PUT/DELETE fetches (via api.ts). Add Content-Security-Policy meta tag in `index.html`. |
| **I** | 7.4 SQL injection | AUDITS + MODIFIES `database/` all repos | Search all `.query()` calls for string interpolation of user input. Replace with parameterized binds. Verify every `CREATE`, `UPDATE`, `DELETE`, `SELECT` uses `$param` syntax, not format!(). Run `cargo clippy` on database/ after fixes. |
| **J** | 7.13 Load test | CREATES `tests/load/` directory | Script using `wrk` or `hey`: 100 concurrent connections, 60s duration, against all API endpoints. Verify: 0 errors, p99 < 500ms for GET, < 1s for POST. Document results in `tests/load/RESULTS.md`. |
| **K** | 7.15 Prep | CREATES `scripts/audit.sh` | Shell/PowerShell script: `cargo test --all`, `cargo audit`, `cargo clippy -- -D warnings`, `cargo fmt --check`. Single command to verify everything passes. |

### Round 2 — Dependent Tasks (after Round 1)
| Agent | Task | Depends On | What |
|-------|------|------------|------|
| **L** | 7.2 Pooling | Agent A (lock audit) | If lock audit flags DB as bottleneck, implement `bb8` or `r2d2` connection pool in `database/mod.rs`. If not a bottleneck, skip (document decision). |
| **M** | 7.9 macOS DMG | Agent E (Windows installer) | Same Tauri bundler config for macOS. Target: DMG with app bundle. Code signing if certificate available. |
| **N** | 7.10 System tray | Agent E (Windows installer) | Use `tauri-plugin-tray`. Minimize to tray on close. Show sync status icon (green/yellow/red). Right-click menu: Open, Sync Now, Quit. |
| **O** | 7.11 Auto-update | Agent E (Windows installer) | Use `tauri-plugin-updater`. Check GitHub releases on startup. Prompt if newer version. Download + install + restart. |

### Round 3 — Central Coordinator (after ALL agents)
| Step | Files | What |
|------|-------|------|
| 1 | `Cargo.toml` | Add `governor`, `prometheus`, `criterion` (dev), `tower-http` CSRF features |
| 2 | `api/mod.rs` | Wire rate limiter layer (Agent B) into middleware stack, add health/ready routes (Agent D), add `/metrics` route (Agent C) |
| 3 | `database/mod.rs` | Apply pooling if needed (Agent L), verify SQL injection fixes (Agent I) |
| 4 | `index.html` | Add CSP meta tag from Agent H |
| 5 | `lib/api.ts` | Add CSRF token handling from Agent H |

### Round 4 — Final Gate (7.15)
| Step | What |
|------|------|
| 1 | Run `scripts/audit.sh` (Agent K): all tests + audit + clippy + fmt |
| 2 | Verify freeze token conditions |
| 3 | Fix any remaining issues |
| 4 | Tag release: `git tag v1.0.0` |

## Agent Prompt Templates

### Round 1 New-File Agent (A-K, creates standalone files)
```
You are implementing Phase 7 Task [7.X] [NAME] for NexusLedger, a Rust
accounting platform in its FINAL production-hardening phase.

Read the existing module (e.g., monitor/mod.rs, api/mod.rs) for patterns.

CREATE ONLY: [filename] — a single new file or directory.

Do NOT edit api/mod.rs, Cargo.toml, App.tsx, or any existing file except
the one specified in your task. Your registration/routing will be added
centrally after all agents complete.

[SPECIFIC_CONTEXT for this task]

Include comprehensive tests. Follow the existing code style.
```

### Round 1 Existing-File Agent (modifies existing files)
```
You are implementing Phase 7 Task [7.X] [NAME] for NexusLedger.

MODIFY ONLY: [specific file(s) listed]

Do NOT touch any other files. Your changes must be backward-compatible —
all 287 existing tests must still pass.

[SPECIFIC_CONTEXT]
```

## Key File Paths
```
nexus-core/src/
├── api/
│   ├── mod.rs           ← EXISTS — add rate limiter layer + routes
│   ├── middleware.rs     ← NEW Agent B — rate limiter (governor)
│   └── routes/
│       └── health.rs    ← NEW Agent D — /health + /ready endpoints
├── monitor/
│   └── mod.rs           ← EXISTS — add Prometheus metrics (Agent C)
├── database/
│   ├── mod.rs           ← EXISTS — connection pooling (Agent L)
│   ├── user.rs, financial.rs, document.rs, audit.rs  ← SQL audit (Agent I)
│   └── ...
├── benches/
│   └── performance.rs   ← NEW Agent F — criterion benchmarks
└── tests/
    └── load/            ← NEW Agent J — wrk/hey load tests

nexus-ledger-tauri/
├── src/
│   ├── lib/api.ts       ← EXISTS — add CSRF tokens (Agent H)
│   └── index.html       ← EXISTS — add CSP meta (Agent H)
└── src-tauri/
    ├── tauri.conf.json  ← MODIFY — Windows/MSI bundler (Agent E)
    ├── Cargo.toml       ← MODIFY — tauri plugins (Agents E, N, O)
    └── icons/           ← NEW — app icons for all platforms

scripts/
└── audit.sh             ← NEW Agent K — cargo test + audit + clippy

docs/
└── user/                ← NEW Agent G — user documentation
```

## Freeze Token 7 (FINAL — all must pass)
- [ ] `cargo test --all` green
- [ ] `cargo audit` zero vulnerabilities
- [ ] `cargo clippy -- -D warnings` clean
- [ ] 10K tx benchmark: create < 2s, list < 2s, balance sheet < 2s
- [ ] 100 concurrent requests: zero errors, p99 < 500ms
- [ ] No SurrealQL injection vectors (all queries parameterized)
- [ ] No XSS vectors in frontend (sanitized + CSP headers)
- [ ] CSRF protection on all state-changing endpoints
- [ ] `GET /health` returns 200
- [ ] `GET /ready` returns 200 when DB + agents healthy
- [ ] `GET /metrics` returns valid Prometheus format
- [ ] Windows MSI/EXE installer builds and installs
- [ ] macOS DMG builds (at least one architecture)
- [ ] System tray shows sync status + right-click menu
- [ ] Auto-update detects new release
- [ ] User documentation complete (README + quick-start + FAQ)

## Dependencies Reminder
- `governor` for rate limiting (Agent B)
- `prometheus` crate for metrics (Agent C)
- `criterion` for benchmarks (Agent F, dev-dependency)
- All added by coordinator in Round 3

After Phase 7: tag `v1.0.0` and ship. This is the final phase.
