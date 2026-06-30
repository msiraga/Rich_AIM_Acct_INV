# NexusLedger — Commit, Push & Session Discipline

**Added:** 2026-06-29  
**Applies to:** All phases (Phase 0 onward)

---

## Commit & Push After Every Phase

At the end of each phase, BEFORE requesting user approval:

```bash
# 1. Stage all changes (exclude .qwen/ and target/)
git add RichdaleAccounting/

# 2. Commit with phase-tagged message
git commit -m "Phase N: <title> — <key achievement>"

# 3. Push to remote
git push origin main
```

### Commit Message Format

```
Phase N: <Title>

- <bullet 1: what was done>
- <bullet 2>
- <bullet 3>

cargo check: 0 errors
cargo test: N passed, 0 failed
Freeze Token: all M conditions met
```

### What NOT to commit

- `.qwen/` — local editor config
- `target/` — build artifacts
- `Cargo.lock` — acceptable to commit (deterministic builds)

---

## Session Handoff Protocol

When context window saturation degrades quality (typically after 2 phases or ~20 files edited):

1. **Commit and push** current state
2. **Update TRACKER.md** with current phase status
3. **Generate handoff prompt** (see template below)
4. **Notify user** — do NOT start a new session without approval

### Handoff Prompt Template

```
You are continuing development of NexusLedger, an agentic accounting platform 
in Rust. Read RichdaleAccounting/Phases/TRACKER.md and 
RichdaleAccounting/Phases/00-strategy.md for full context.

LAST COMPLETED: Phase N — <title>
- cargo check: 0 errors
- cargo test: N passed, 0 failed  
- All freeze tokens met
- Committed and pushed as: "<commit hash>"

NEXT: Phase N+1 — <title>
Read: RichdaleAccounting/Phases/XX-phase-N+1-<slug>.md

KEY STATE:
- Database: SurrealDB kv-mem, schema applied, seed data loaded
- Agents: All 9 types instantiate, agent logic is still mock/stub
- Persistence: Additive SurrealDB writes (in-memory cache primary)
- AI: Stubbed (Phase 5)
- API: Stubbed (Phase 3)
- Frontend: Stub backend, skeleton React (Phase 3)

FILES TO READ FIRST:
1. RichdaleAccounting/Phases/TRACKER.md
2. RichdaleAccounting/Phases/00-strategy.md
3. RichdaleAccounting/Phases/XX-phase-N+1.md
4. RichdaleAccounting/docs/02-architecture.md (system diagram)

Begin Phase N+1 Task 1.1 after reading the phase doc.
```

---

## Quality Triggers for New Session

Start a new session when:
- More than 2 phases completed in current session
- More than 20 files edited since session start
- cargo check output exceeds 500 lines
- Multiple agents/subagents have been dispatched and returned
- Context window feels saturated (slow responses, forgotten details)

---

## Phase History

| Phase | Commit | Tests | Date | Report |
|---|---|---|---|---|
| 0: Compile & Fix | `a767ff2` | 145 passed | 2026-06-29 | [phase-0-completion.md](phase-0-completion.md) |
| 1: Database & Wire | `a767ff2` | 156 passed | 2026-06-29 | [phase-1-completion.md](phase-1-completion.md) |
| 2: Real Agent Engine | `b4a97fc` | 200 passed | 2026-06-30 | [phase-2-completion.md](phase-2-completion.md) |
| 3: End-to-End (API + UI) | (pending commit) | 200+3 e2e | 2026-06-30 | [phase-3-completion.md](phase-3-completion.md) |
