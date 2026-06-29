# Phase 0: Compile & Fix

**Objective:** Get the codebase compiling cleanly. Fix all type mismatches. Define the missing `Database` struct. Every agent must be instantiable without panic.  
**Duration:** 1–2 weeks  
**Depends on:** Nothing (first phase)  
**Blocks:** Phase 1  

---

## Why This Phase Exists

The current codebase does not compile. The orchestrator's `add_agent()` method passes wrong types to agent constructors (e.g., `Arc<Mutex<None>>` instead of `Ledger`). The `Database` struct is referenced in `api/mod.rs` and `edge/mod.rs` but never defined. The `Agent` trait has a `&self` method that tries to mutate `self.status`. These are compilation errors that block all other work.

---

## Task List

| ID | Task | Primary File(s) | Depends On | Parallel With |
|---|---|---|---|---|
| 0.1 | **Define `Database` struct** — wraps SurrealDB connection, provides `new()`, `connect()`, `disconnect()`, `is_connected()` methods. Must support in-memory mode for tests. | `database/mod.rs` | — | Nothing (critical path) |
| 0.2 | **Fix `AgentOrchestrator::add_agent()`** — all 9 agent factory branches pass wrong constructor arguments. Fix each to pass the correct type (e.g., `Ledger` instead of `Arc<Mutex<Option<...>>>`). | `agents/orchestrator.rs` | 0.1 | Nothing (critical path) |
| 0.3 | **Fix `Agent` trait mutability** — `process_task(&self, task)` cannot mutate `self.status`. Change to use `std::sync::atomic::AtomicU8` for status, or wrap status in `Mutex<AgentStatus>` inside each agent. | `agents/agent_types.rs` | — | 0.2, 0.4 |
| 0.4 | **Add missing dependencies** — `once_cell` (used in orchestrator's `ORCHESTRATOR_CONFIG`), and any other missing crates discovered during compilation. | `nexus-core/Cargo.toml` | — | 0.2, 0.3 |
| 0.5 | **Fix `LedgerAgent::process_record_transaction`** — the embedded `Ledger` is never initialized (no accounts created). Either initialize it on `Agent::initialize()` or accept a pre-initialized ledger. | `accounting/ledger.rs` | 0.2 | 0.6, 0.7 |
| 0.6 | **Fix `ReportingAgent` mapping** — currently maps to `LedgerAgent` instance. Create a minimal `ReportingAgent` struct or map to a real reporting struct. | `agents/orchestrator.rs` | 0.2 | 0.5, 0.7 |
| 0.7 | **Fix `Arc<Mutex<Option<...>>>` patterns** — agent constructors that take `Arc<Mutex<Option<surrealdb::Surreal<...>>>>` should take the `Database` struct instead (or `Arc<Database>`). Simplify constructor signatures. | `agents/orchestrator.rs`, `agents/document.rs` | 0.1, 0.2 | 0.5, 0.6 |
| 0.8 | **Add missing `impl Default`** — for any struct where `Default::default()` is called but no `impl Default` exists. | Various | — | 0.3, 0.4 |
| 0.9 | **`cargo check`** — run and fix every remaining compiler error across the entire crate. | All | 0.1–0.8 | Nothing (integration gate) |
| 0.10 | **`cargo test`** — run and ensure all existing unit tests pass. Fix any test failures caused by the changes above. | All | 0.9 | Nothing (final gate) |

---

## Dependency Graph

```
                    ┌─────┐
                    │ 0.1 │  Database struct (critical)
                    └──┬──┘
                       │
                    ┌──┴──┐
            ┌───────┤ 0.2 ├───────┐     Orchestrator fix (critical)
            │       └─────┘       │
    ┌───────┴──┐            ┌─────┴────┐
    │   0.3    │            │   0.7    │     Agent trait + constructor fixes
    └───────┬──┘            └─────┬────┘     (parallel)
            │                     │
    ┌───────┴──┐     ┌────────────┴──────┐
    │   0.4    │     │ 0.5   0.6   0.8   │   Dependencies + agent logic fixes
    └───────┬──┘     └────────────┬──────┘   (parallel)
            │                     │
            └──────────┬──────────┘
                    ┌──┴──┐
                    │ 0.9 │  cargo check
                    └──┬──┘
                    ┌──┴───┐
                    │ 0.10 │  cargo test
                    └──────┘
```

---

## Parallel Execution Strategy

**Session 1 (Critical path):**
- 0.1 → 0.2 (must be sequential — 0.2 depends on Database struct existing)

**Session 2 (Parallel fixes after 0.2):**
- 0.3 + 0.4 + 0.5 + 0.6 + 0.7 + 0.8 (all independent of each other, depend only on 0.1/0.2)

**Session 3 (Integration):**
- 0.9 → 0.10 (sequential — must compile before testing)

---

## Freeze Token 0 🔒

All conditions must be true:

- [ ] `cargo check` passes with **zero errors**
- [ ] `cargo test` passes with **zero failures**
- [ ] `cargo clippy -- -D warnings` has no error-level warnings (or clippy is not yet enforced — document if deferred)
- [ ] `Database` struct exists in `database/mod.rs` with `new()`, `connect()`, `disconnect()`, `is_connected()` methods
- [ ] `Database` supports in-memory mode (for tests without a SurrealDB server)
- [ ] All 9 agent types can be constructed via `AgentOrchestrator::add_agent()` without panicking
- [ ] `ReportingAgent` is no longer mapped to `LedgerAgent`
- [ ] The `Agent` trait compiles — `process_task` can read and write agent status
- [ ] No `Arc<Mutex<Option<...>>>` patterns remain in agent constructors (replaced with `Database` or `Arc<Database>`)

---

## Known Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Fixing the Agent trait (`&self` → `&mut self`) cascades through all agent impls | High | Use interior mutability (`Mutex<AgentStatus>`) on `status` field instead of changing the trait signature — keeps `process_task(&self)` |
| Some existing tests were written against mock types that no longer exist | Medium | Fix tests to match the corrected types; delete tests that tested incorrect mock behavior |
| `once_cell` may not be needed if we restructure the orchestrator config | Low | Add it, use it or remove it later — don't block on this |

---

## Notes for Reviewer

- This phase produces **no new features** — it only makes the existing code compile
- The `Database` struct in 0.1 is minimal: connection wrapper only. Real repository wiring happens in Phase 1
- Agent logic in this phase is still mock/stub — real processing happens in Phase 2
- The goal is a green `cargo test`, not a running application
