# NexusLedger — Phased Execution Plan

**Goal:** Ship a working MVP that can replace QuickBooks for a small business  
**Created:** 2026-06-29  
**Status:** Awaiting review  

---

## Documents

| # | Document | Phase | Duration |
|---|---|---|---|
| 00 | [Strategy & Methodology](00-strategy.md) | — | — |
| 01 | [Phase 0: Compile & Fix](01-phase-0-compile-fix.md) | Foundation | 1–2 wks |
| 02 | [Phase 1: Database & Wire](02-phase-1-database-wire.md) | Data Layer | 2–3 wks |
| 03 | [Phase 2: Real Agent Engine](03-phase-2-agent-engine.md) | Domain Logic | 3–4 wks |
| 04 | [Phase 3: End-to-End (API + UI)](04-phase-3-end-to-end.md) | Integration | 2–3 wks |
| 05 | [Phase 4: Auth & Accounting Completeness](05-phase-4-auth-accounting.md) | Features | 3–4 wks |
| 06 | [Phase 5: AI Pipeline](06-phase-5-ai-pipeline.md) | Intelligence | 2–3 wks |
| 07 | [Phase 6: Edge & Sync](07-phase-6-edge-sync.md) | Offline | 2–3 wks |
| 08 | [Phase 7: Production Hardening](08-phase-7-production.md) | Ship | 2–3 wks |

---

## Phase Gate Process

```
Phase N work ──→ Freeze Token check ──→ User review ──→ APPROVED ──→ Phase N+1
                      │                                      │
                      └── if any token fails ────────────────┘
                              fix → re-check → re-review
```

**Rules:**
1. No code work begins until the strategy doc is approved
2. Each phase ends with a freeze token audit — all conditions must pass
3. User must explicitly approve before the next phase begins
4. If a freeze token fails, we fix and re-audit (never skip)
5. Parallel tasks within a phase run simultaneously for time efficiency
