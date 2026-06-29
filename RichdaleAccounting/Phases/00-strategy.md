# Strategy & Methodology

## Approach

NexusLedger is ~10–15% complete. The skeleton is well-architected but nothing runs end-to-end. The strategy is to **build outward from the core** — fix compilation, wire the database, make agents real, connect the frontend, then add features layer by layer.

## Principles

### 1. Fix Before Build
Never add new features on broken foundations. Phase 0 exists solely to make the codebase compile. No new logic until it compiles clean.

### 2. Vertical Slices Over Horizontal Layers
Each phase delivers a **vertical slice** — a thin but complete feature that works from database → agent → API → UI. Not "build all the database layer first, then all the agents, then all the API." A vertical slice is testable and demonstrable.

### 3. Phase Gates Are Non-Negotiable
Each phase has a **Freeze Token** — a checklist of verifiable conditions. Every condition must be true before the phase is considered complete. The user reviews and approves before the next phase begins.

### 4. Parallel Where Independent
Tasks that have no dependency on each other are flagged as parallelizable and should be executed simultaneously. This is documented explicitly in each phase file with a dependency graph.

### 5. Test at Every Gate
Every phase ends with `cargo test` green. Integration tests are added progressively:
- Phase 1: "create account → record transaction → verify balance"
- Phase 2: "submit task → agent processes → result in DB"
- Phase 3: "user clicks button → transaction appears in UI"
- Phase 5: "upload receipt → AI extracts → transaction created"

### 6. No Speculative Work
Only build what the current phase requires. Don't pre-build Phase 5 AI features during Phase 2. Don't add Phase 6 edge sync during Phase 4 auth. Scope discipline prevents the "90% done but nothing ships" trap.

---

## Phase Dependency Graph

```
Phase 0 ──────→ Phase 1 ──────→ Phase 2 ──────→ Phase 3 ──────→ Phase 4 ──────→ Phase 5 ──────→ Phase 6 ──────→ Phase 7
COMPILE & FIX    DATABASE &      REAL AGENT      END-TO-END      AUTH &          AI              EDGE &          PRODUCTION
                 WIRE            ENGINE          (API + UI)      ACCOUNTING      PIPELINE        SYNC            HARDENING
```

Each arrow represents a **freeze token gate**. No skipping.

---

## Time Estimates

| Phase | Tasks | Duration | Cumulative | Milestone |
|---|---|---|---|---|
| 0 | 10 | 1–2 weeks | Week 2 | Code compiles, all agents instantiate |
| 1 | 12 | 2–3 weeks | Week 5 | SurrealDB connected, data persists |
| 2 | 11 | 3–4 weeks | Week 9 | All agents process real tasks |
| 3 | 12 | 2–3 weeks | Week 12 | User can use the app end-to-end |
| 4 | 14 | 3–4 weeks | Week 16 | Auth works, full accounting features |
| 5 | 10 | 2–3 weeks | Week 19 | AI document pipeline works |
| 6 | 10 | 2–3 weeks | Week 22 | Offline mode + sync works |
| 7 | 15 | 2–3 weeks | Week 25 | Production-ready, packaged, shipped |

**Total: ~25 weeks (~6 months)** for a single full-time developer.

---

## Parallel Execution Model

Within each phase, tasks are organized into **tracks**:

```
Track A (critical path):  Sequential tasks that block everything else
Track B (parallel):       Tasks that can run simultaneously after a dependency is met
Track C (parallel):       Additional parallel work streams
```

Tasks in different tracks that share no dependencies are executed in the same work session to compress the timeline. The dependency graph in each phase file makes this explicit.

---

## Audit & Review Protocol

### Per-Phase Audit

At the end of each phase, before requesting user approval:

1. Run `cargo check` — zero errors
2. Run `cargo test` — zero failures
3. Run `cargo clippy` — no error-level warnings
4. Verify every freeze token condition manually
5. Document what was done, what changed, what was skipped
6. Note any technical debt carried forward
7. Present to user for approval

### Approval Gate

The user reviews the audit results. If all freeze tokens are satisfied:
- **APPROVED** → Next phase begins
- **CONDITIONAL** → Specific fixes requested, then re-audit
- **REJECTED** → Phase reworked from scratch

### Carry-Forward Debt Log

If a phase produces technical debt that doesn't block the freeze token, it is logged in the phase audit notes and addressed in a later phase (typically Phase 7).

---

## Risk Register

| Risk | Impact | Mitigation |
|---|---|---|
| SurrealDB API changes break repository code | High | Pin exact version, test against real instance |
| Ollama unavailable or slow | Medium | AI features degrade gracefully; `is_available()` checks |
| Lock contention in agent orchestrator | Medium | Profile in Phase 7, refactor if needed |
| Scope creep within a phase | High | Strict freeze token — anything not in the token is deferred |
| Agent trait design too restrictive | Low | Refactor trait in Phase 2 if needed, before agents are built |
