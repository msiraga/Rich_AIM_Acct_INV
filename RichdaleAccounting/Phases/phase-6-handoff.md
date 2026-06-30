You are starting Phase 6: Edge & Sync for NexusLedger, an agentic accounting platform
in Rust (Tauri + React frontend, SurrealDB backend, agent-based architecture).

## Repository & State
c:\Users\msira\Richdale_Accounting\Rich_AIM_Acct_INV
Latest commit: Phase 4 complete (287 tests, 0 failures)
Phase 5 (AI Pipeline) is running in a separate session — no overlap.

## Read These First
1. RichdaleAccounting/Phases/TRACKER.md         ← Phase 4 done, P6 next
2. RichdaleAccounting/Phases/00-strategy.md     ← methodology + principles
3. RichdaleAccounting/Phases/07-phase-6-edge-sync.md ← THIS PHASE'S PLAN
4. RichdaleAccounting/Phases/phase-4-handoff.md ← cross-phase parallelism guide
5. RichdaleAccounting/docs/02-architecture.md   ← system diagram

## Phase 6: Edge & Sync — 10 Tasks

**Objective:** Offline-first data storage, periodic sync with SurrealDB,
conflict resolution, local encryption. App must work with no internet.

### Current State of `edge/mod.rs`
- `EdgeConfig` exists with `compress_data: true`, `encrypt_data: false`, `storage_path: "./data/edge"`
- `EdgeManager` struct exists but `perform_sync()` is a stub — logs "Syncing..." and returns `Ok(())`
- No local database, no change tracking, no persistence

### CRITICAL: QuickBooks-Grade Quality Standard
Every edge feature must be production-grade:
- **Offline CRUD:** Must work identically to online — same validation, same double-entry
- **Sync:** Never lose data. Conflict logged, not silently dropped. Idempotent retries.
- **Encryption:** AES-256-GCM with authenticated encryption. Key derived via HKDF. Never store plaintext keys.
- **Compression:** Verifiable round-trip. Compressed data must decompress bit-for-bit.
- **Conflict resolution:** Auditor can trace every conflict. Timestamps are server-authoritative.
- **Tests:** Verify actual offline→online→sync flow, not mocked network conditions.

## Maximum Parallelism Strategy

### File Conflict Analysis
```
Phase 6 files:
  NEW (no conflicts):     edge/local_db.rs      ← Agent 1
  NEW (no conflicts):     edge/encryption.rs    ← Agent 2
  NEW (no conflicts):     edge/compression.rs   ← Agent 3
  NEW (no conflicts):     components/SyncStatus.tsx  ← Agent 4 (frontend)
  NEW (depends on 1):     edge/store.rs         ← Agent 5 (after Agent 1)
  NEW (depends on 5):     edge/tracking.rs      ← Agent 6 (after Agent 5)
  NEW (depends on 6):     edge/sync.rs          ← Agent 7 (after Agent 6)
  NEW (depends on 7):     edge/conflict.rs      ← Agent 8 (after Agent 7)
  EXISTING (central):     edge/mod.rs           ← COORDINATOR (register + offline toggle)
  EXISTING (central):     Cargo.toml            ← COORDINATOR (deps)
  EXISTING (central):     App.tsx               ← COORDINATOR (SyncStatus route)
  EXISTING (central):     index.css             ← COORDINATOR (SyncStatus styles)
```

### Round 1 — 4 Parallel Agents (NEW files, zero dependencies)

| Agent | Task | Creates | Quality Standard |
|-------|------|---------|-----------------|
| **A** | 6.1 Local DB | `edge/local_db.rs` | SQLite schema mirroring SurrealDB (accounts, transactions, journal_entries, invoices, bills, assets). Use `rusqlite` with migrations pattern. Include `created_at`, `updated_at` columns. |
| **B** | 6.8 Encryption | `edge/encryption.rs` | AES-256-GCM via `aes-gcm` crate. Key from password via HKDF-SHA256. Methods: `encrypt_field(plaintext, key) -> Vec<u8>`, `decrypt_field(ciphertext, key) -> Result<String>`. NEVER log keys. Include nonce per encryption. |
| **C** | 6.9 Compression | `edge/compression.rs` | lz4 via `lz4` crate. Methods: `compress_blob(data) -> Vec<u8>`, `decompress_blob(data) -> Result<Vec<u8>>`. Verify round-trip in tests. Measure compression ratio. |
| **D** | 6.7 Sync UI | `components/SyncStatus.tsx` | React component showing: "Up to date" (green dot), "Syncing..." (yellow spinner), "Offline — N changes pending" (orange), "Sync error — retry" (red with button). Uses `useState` + `useEffect` polling `GET /api/v1/edge/status`. Manual sync button calls `POST /api/v1/edge/sync`. |

### Round 2 — Sequential Chain (each builds on prior schema)

| Agent | Task | Creates | Depends On | Quality Standard |
|-------|------|---------|------------|-----------------|
| **E** | 6.2 Local Store | `edge/store.rs` | Agent A (local_db schema) | CRUD for accounts, transactions, invoices against SQLite. Mirror same validation as online ledger. `save()`, `get_by_id()`, `list_all()`, `delete()`. |
| **F** | 6.3 Tracking | `edge/tracking.rs` | Agent E (store CRUD) | Every write: set `_dirty=true`, `_modified_at=now()`. `changes` table: entity_type, entity_id, operation (insert/update/delete), timestamp. `get_dirty_records()`, `mark_synced()`. |
| **G** | 6.4 Sync Engine | `edge/sync.rs` | Agent F (tracking) | Push: read changes table → POST to SurrealDB. Pull: GET from SurrealDB with `?since=last_sync` → upsert local. Idempotent: same record pushed twice = no duplicate. Update `last_sync` timestamp atomically. |
| **H** | 6.5 Conflict | `edge/conflict.rs` | Agent G (sync engine) | When local._modified_at > remote._modified_at after pull: log conflict to audit trail with both versions. Resolution: last-write-wins (remote wins by default, configurable). Never delete data — keep both versions. |

### Round 3 — Central Coordinator (after ALL agents complete)

| Step | File(s) | What |
|------|---------|------|
| 1 | `Cargo.toml` | Add `rusqlite = { version = "0.31", features = ["bundled"] }`, `aes-gcm = "0.10"`, `hkdf = "0.12"`, `sha2 = "0.10"`, `lz4 = "1.24"`, `rand = "0.8"` |
| 2 | `edge/mod.rs` | Register: `pub mod local_db; pub mod store; pub mod tracking; pub mod sync; pub mod conflict; pub mod encryption; pub mod compression;` Wire `EdgeManager` to use `LocalStore` when `offline_mode=true`. Implement `perform_sync()` calling `sync::push_changes()` then `sync::pull_changes()`. Add offline toggle: `enable_offline_mode()`, `disable_offline_mode()`. |
| 3 | `api/mod.rs` | Add routes: `GET /api/v1/edge/status` (sync state), `POST /api/v1/edge/sync` (manual trigger). Require `RequireUser` guard. |
| 4 | `App.tsx` | Add `<SyncStatus />` inside Layout or as fixed footer. Import from `./components/SyncStatus`. |
| 5 | `index.css` | Add `.sync-status` styles (position: fixed bottom-right, colored dot + text). |

### Round 4 — Integration Test

| Task | File | Quality Standard |
|------|------|-----------------|
| 6.10 E2E | `tests/integration/edge.rs` | 1) Create fresh SQLite DB. 2) Go offline (set offline_mode=true). 3) Create 5 transactions locally (all balanced). 4) Verify all have `_dirty=true`. 5) Go online (set offline_mode=false). 6) Trigger sync. 7) Verify all 5 appear in SurrealDB. 8) Verify `_dirty` cleared. 9) Verify local SQLite matches remote. 10) Create transaction remotely, pull, verify appears locally. |

## Agent Prompt Templates

### Round 1 Agent (A-D: independent new files)
```
You are implementing Phase 6 Task [6.X] [NAME] for NexusLedger.
Read edge/mod.rs for existing patterns and EdgeConfig.

CREATE ONLY: edge/[filename].rs

Do NOT edit edge/mod.rs, Cargo.toml, api/mod.rs, App.tsx, or any existing file.
Your file will be registered centrally after all agents complete.

[SPECIFIC_CONTEXT for this task]

Include comprehensive unit tests. Follow the existing code style
(#[derive(Debug, Clone)], thiserror for errors, tracing for logging).
```

### Round 2 Agent (E-H: depends on prior agent's file)
```
You are implementing Phase 6 Task [6.X] [NAME] for NexusLedger.
Read the file created by the previous agent: edge/[dependency].rs
Read edge/mod.rs for existing patterns.

CREATE ONLY: edge/[filename].rs

Do NOT edit edge/mod.rs, Cargo.toml, or any existing file.
[SPECIFIC_CONTEXT]
```

## Key File Paths (what you'll create)
```
nexus-core/src/edge/
├── mod.rs              ← EXISTS — register submodules + offline toggle
├── local_db.rs         ← NEW Agent A — SQLite schema + connection
├── store.rs            ← NEW Agent E — CRUD operations
├── tracking.rs         ← NEW Agent F — change tracking (_dirty flags)
├── sync.rs             ← NEW Agent G — push/pull engine
├── conflict.rs         ← NEW Agent H — conflict resolution
├── encryption.rs       ← NEW Agent B — AES-256-GCM field encryption
└── compression.rs      ← NEW Agent C — lz4 blob compression

nexus-ledger-tauri/src/
└── components/
    └── SyncStatus.tsx  ← NEW Agent D — sync status UI component

nexus-core/tests/
└── integration/
    └── edge.rs         ← NEW (Round 4) — offline→online→sync E2E
```

## Freeze Token 6 (all must pass)
- [ ] App starts in offline mode and all CRUD works against local SQLite
- [ ] Creating a transaction offline stores it with `_dirty=true`
- [ ] Going online triggers sync — all dirty records pushed to SurrealDB
- [ ] Pulling remote changes: records modified since last_sync appear locally
- [ ] Conflict: same record modified locally and remotely → logged, not lost
- [ ] Sync status UI shows correct state (offline/syncing/up-to-date/error)
- [ ] Manual sync button triggers sync on demand
- [ ] Sensitive fields (bank account numbers) encrypted at rest (AES-256-GCM)
- [ ] Large documents compressed in local storage (verifiable size reduction)
- [ ] Integration test: offline 5 txns → online → verify in SurrealDB → local matches
- [ ] `cargo test` passes (all unit + integration)

## Dependencies Reminder
- `rusqlite` with `bundled` feature (no system SQLite required)
- `aes-gcm` + `hkdf` + `sha2` for encryption
- `lz4` for compression
- All added by coordinator in Round 3 — agents do NOT touch Cargo.toml

Begin Round 1: launch agents A, B, C, D in parallel.
