# Phase 7: Production Hardening

**Objective:** Performance optimization, security audit, Tauri desktop packaging (Windows + macOS), deployment readiness, documentation. This is the "ship it" phase.  
**Duration:** 2–3 weeks  
**Depends on:** Phase 6 (freeze token satisfied)  
**Blocks:** Nothing (final phase)  

---

## Why This Phase Exists

After Phase 6, every feature works but the system hasn't been stress-tested, audited for security, or packaged for distribution. Lock contention from the `Arc<RwLock<...>>>` nesting may cause performance issues under load. The Tauri desktop installer hasn't been built. There's no user documentation. This phase takes the platform from "works on my machine" to "ready for real users."

---

## Task List

### Performance Track

| ID | Task | Primary File(s) | Depends On | Parallel With |
|---|---|---|---|---|
| 7.1 | **Lock contention audit** — profile the orchestrator and ledger under concurrent access. Identify hot locks. Reduce `Arc<RwLock<>>` nesting where possible. Consider replacing with `DashMap` for agent registry. | Various | P6 | 7.2, 7.3 |
| 7.2 | **SurrealDB connection pooling** — if single-connection becomes a bottleneck under concurrent agent access, implement a connection pool (e.g., `bb8` or SurrealDB's built-in pooling). | `database/mod.rs` | P6 | 7.1 |
| 7.3 | **Request rate limiting** — implement token-bucket rate limiter (e.g., `governor` crate) on API endpoints. Configurable per-role (admin=higher limit). | `api/middleware.rs` | P6 | 7.1, 7.2 |
| 7.12 | **Performance benchmarks** — benchmark: create 10,000 transactions, list 10,000 transactions, generate balance sheet with 10,000 transactions. Target: < 2 seconds for each. | `benches/` (new) | 7.1, 7.2 | 7.13 |
| 7.13 | **Load test** — simulate 100 concurrent API requests using `wrk` or `hey`. Verify no errors, acceptable latency (< 500ms p99). | `tests/load/` (new) | 7.3 | 7.12 |

### Security Track

| ID | Task | Primary File(s) | Depends On | Parallel With |
|---|---|---|---|---|
| 7.4 | **SurrealQL injection prevention** — audit all repository queries for string interpolation of user input. Replace with parameterized queries or SurrealDB's binding API. | `database/` all repos | P6 | 7.1, 7.5 |
| 7.5 | **Frontend security** — audit for XSS (sanitize all user-rendered content), add CSRF tokens to state-changing requests, set Content-Security-Policy headers, validate all input client-side. | `nexus-ledger-tauri/src/`, `api/middleware.rs` | P6 | 7.4 |

### Observability Track

| ID | Task | Primary File(s) | Depends On | Parallel With |
|---|---|---|---|---|
| 7.6 | **Prometheus metrics export** — expose `/metrics` endpoint in Prometheus text format. Export: request count, request duration histogram, agent task count, error count, SurrealDB query count. | `monitor/mod.rs` | P6 | 7.1 |
| 7.7 | **Health check endpoints** — `GET /health` (liveness: always 200 if process running), `GET /ready` (readiness: 200 if DB connected + agents initialized). Kubernetes-compatible format. | `api/routes/health.rs` (new) | P6 | 7.6 |

### Packaging Track

| ID | Task | Primary File(s) | Depends On | Parallel With |
|---|---|---|---|---|
| 7.8 | **Windows installer** — build Tauri MSI/EXE installer for Windows. Bundle SurrealDB binary. Auto-start backend on app launch. | `nexus-ledger-tauri/src-tauri/` | P6 | 7.9 |
| 7.9 | **macOS installer** — build Tauri DMG for macOS (Intel + Apple Silicon). Code signing if possible. | `nexus-ledger-tauri/src-tauri/` | 7.8 | 7.8 |
| 7.10 | **System tray icon** — minimize to system tray, show sync status icon (green/yellow/red), right-click menu: Open, Sync Now, Quit. | `nexus-ledger-tauri/src-tauri/` | 7.8 | 7.11 |
| 7.11 | **Auto-update** — integrate Tauri updater plugin. Check GitHub releases on startup, prompt user to update, download and install. | `nexus-ledger-tauri/src-tauri/` | 7.8 | 7.10 |

### Documentation Track

| ID | Task | Primary File(s) | Depends On | Parallel With |
|---|---|---|---|---|
| 7.14 | **User documentation** — README update with screenshots, quick start guide (install → register → create first transaction → upload receipt → see AI extraction), FAQ, troubleshooting. | `README.md`, `docs/user/` (new) | All | 7.12 |

### Final Gate

| ID | Task | Depends On |
|---|---|---|
| 7.15 | **Final audit** — all integration + E2E + load tests pass. No `cargo audit` vulnerabilities. No clippy warnings. Installer builds clean on Windows and macOS. | All |

---

## Dependency Graph

```
                         P6 (freeze token)
                              │
         ┌────────────────────┼────────────────────┐
         │                    │                     │
    Track A               Track B              Track C
    (performance)         (security)           (packaging)
         │                    │                     │
    7.1 ─┐               7.4 ─┐               7.8 ──→ 7.9
    7.2 ─┤               7.5 ─┘                    │
    7.3 ─┘                                    7.10 ──→ 7.11
         │
    7.12 ──→ 7.13
         │
    Track D (observability)     Track E (docs)
         │                          │
    7.6 ──→ 7.7                 7.14
         │
         └────────────────────────┬──→ 7.15 (final audit)
```

---

## Parallel Execution Strategy

**Session 1 (Five parallel tracks start simultaneously):**
- Track A: 7.1 + 7.2 + 7.3 (performance)
- Track B: 7.4 + 7.5 (security)
- Track C: 7.8 → 7.9 (packaging — sequential)
- Track D: 7.6 → 7.7 (observability)
- Track E: 7.14 (documentation)

**Session 2 (After Track A):**
- 7.12 → 7.13

**Session 3 (After packaging):**
- 7.10 → 7.11

**Session 4 (Final):**
- 7.15

---

## Freeze Token 7 🔒 (FINAL — SHIP IT)

All conditions must be true:

- [ ] `cargo test --all` passes (unit + integration + E2E)
- [ ] `cargo audit` reports zero vulnerabilities
- [ ] `cargo clippy -- -D warnings` has zero warnings
- [ ] 10,000 transaction benchmark: create all in < 2 seconds
- [ ] 10,000 transaction benchmark: list all in < 2 seconds
- [ ] 10,000 transaction benchmark: balance sheet generation in < 2 seconds
- [ ] 100 concurrent requests: zero errors, p99 latency < 500ms
- [ ] No SurrealQL injection vectors found in audit
- [ ] No XSS vectors found in frontend audit
- [ ] CSRF protection on all state-changing endpoints
- [ ] `GET /health` returns 200
- [ ] `GET /ready` returns 200 when DB + agents are healthy
- [ ] `GET /metrics` returns valid Prometheus text format
- [ ] Windows MSI/EXE installer builds and installs cleanly
- [ ] macOS DMG builds and installs cleanly (at least one architecture)
- [ ] System tray icon shows sync status
- [ ] Auto-update detects a new release when available
- [ ] User documentation is complete (README + quick start + FAQ)
- [ ] All 9 agents process real tasks with real data
- [ ] AI pipeline processes at least one receipt end-to-end
- [ ] Edge sync works with conflict resolution

---

## Notes for Reviewer

- This phase has the most tasks (15) but they are all independent tracks — maximum parallelism
- The performance benchmarks (7.12) may reveal issues that require going back to fix lock contention — budget extra time if needed
- Windows and macOS packaging may require CI/CD setup (GitHub Actions) — factor in setup time
- The final audit (7.15) is the last gate before declaring the project an MVP
- After this phase, the project is **shippable** — not feature-complete, but a real business could start using it
