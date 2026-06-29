# Phase 6: Edge & Sync

**Objective:** Implement offline-first data storage, periodic synchronization with the central SurrealDB instance, conflict resolution, and local encryption. The app must work on a laptop with no internet and sync when reconnected.  
**Duration:** 2–3 weeks  
**Depends on:** Phase 5 (freeze token satisfied)  
**Blocks:** Phase 7  

---

## Why This Phase Exists

The `EdgeManager` module exists with the right structure but every method is a stub — `perform_sync()` logs "Syncing accounts..." and returns `Ok(())`. No local database exists. No change tracking exists. A traveling accountant or someone in a rural office with intermittent internet cannot use the platform. This phase makes NexusLedger genuinely offline-capable.

---

## Task List

| ID | Task | Primary File(s) | Depends On | Parallel With |
|---|---|---|---|---|
| 6.1 | **Embedded SQLite local database** — add `rusqlite` or `sqlx-sqlite` for local storage. Create tables mirroring the SurrealDB schema for accounts, transactions, journal entries. | `edge/local_db.rs` (new), `Cargo.toml` | P5 | 6.2 |
| 6.2 | **Local data store** — serialize/deserialize accounting records to/from SQLite. CRUD operations for accounts, transactions, invoices that work without SurrealDB. | `edge/store.rs` (new) | 6.1 | 6.1 |
| 6.3 | **Change tracking** — every local write gets a `_dirty` flag and `_modified_at` timestamp. A `changes` table records: entity_type, entity_id, operation (insert/update/delete), timestamp. | `edge/tracking.rs` (new) | 6.1, 6.2 | Nothing |
| 6.4 | **Sync engine** — push: read `changes` table → apply to SurrealDB. Pull: query SurrealDB for records modified since `last_sync` → upsert into SQLite. Update `last_sync` timestamp. | `edge/sync.rs` (new) | 6.3 | Nothing |
| 6.5 | **Conflict resolution** — when both local and remote have modified the same record: last-write-wins based on `_modified_at`. Log conflicts in audit trail. Never silently lose data. | `edge/conflict.rs` (new) | 6.4 | Nothing |
| 6.6 | **Offline mode toggle** — when offline: block sync attempts, queue all changes, show "offline" indicator. When coming online: auto-trigger sync. Network detection via periodic health check. | `edge/mod.rs` | 6.3 | 6.4 |
| 6.7 | **Sync status UI** — frontend shows: "Up to date" (green), "Syncing..." (yellow), "Offline — N changes pending" (orange), "Sync error" (red). Click to trigger manual sync. | `nexus-ledger-tauri/src/components/SyncStatus.tsx` (new) | 6.4 | 6.5, 6.6 |
| 6.8 | **Local encryption** — encrypt sensitive fields (SSN in tax_info, bank account numbers in bank_details) at rest in SQLite using AES-256-GCM. Key derived from user password via HKDF. | `edge/encryption.rs` (new) | P5 | 6.1 |
| 6.9 | **Local compression** — compress large document blobs (receipts, invoices) using lz4 before storing in SQLite. Decompress on read. | `edge/compression.rs` (new) | P5 | 6.1 |
| 6.10 | **Integration test** — go offline → create 5 transactions → go online → verify all 5 transactions appear in SurrealDB → verify local SQLite matches remote. | `tests/integration/edge.rs` (new) | 6.5, 6.6 | Nothing |

---

## Dependency Graph

```
                    P5 (freeze token)
                         │
              ┌──────────┼──────────┐
              │          │          │
         Track A     Track B    Track C
         (sync)      (UI)       (security)
              │          │          │
         6.1 ─┐     6.7       6.8 ─┐
         6.2 ─┘                  6.9 ─┘
              │
         ┌────┴────┐
         │   6.3   │  Change tracking
         └────┬────┘
              │
    ┌─────────┼─────────┐
    │                   │
┌───┴──┐            ┌───┴──┐
│ 6.4  │            │ 6.6  │
│ sync │            │offline│
└───┬──┘            └───┬──┘
    │                   │
┌───┴──┐                │
│ 6.5  │                │
│conflict│               │
└───┬──┘                │
    │                   │
    └────────┬──────────┘
         ┌───┴───┐
         │ 6.10  │
         └───────┘
```

---

## Parallel Execution Strategy

**Session 1 (Three parallel tracks):**
- Track A: 6.1 → 6.2 → 6.3 (local DB + tracking)
- Track B: 6.7 (sync status UI — can be mocked initially)
- Track C: 6.8 + 6.9 (encryption + compression — independent)

**Session 2 (After 6.3, sequential):**
- 6.4 → 6.5 → 6.6

**Session 3 (Integration):**
- 6.10

---

## Freeze Token 6 🔒

All conditions must be true:

- [ ] App starts in offline mode (no SurrealDB connection) and all CRUD operations work against local SQLite
- [ ] Creating a transaction offline stores it in SQLite with `_dirty=true` flag
- [ ] Going online triggers sync — all dirty records are pushed to SurrealDB
- [ ] Pulling remote changes: records modified in SurrealDB since last sync appear in local SQLite
- [ ] Conflict resolution: same record modified locally and remotely → last-write-wins, conflict logged in audit trail
- [ ] Sync status UI shows correct state (offline/syncing/up-to-date/error)
- [ ] Manual sync button triggers sync on demand
- [ ] Sensitive fields (SSN, bank account numbers) are encrypted at rest in SQLite
- [ ] Large documents are compressed in local storage (measurable size reduction)
- [ ] Integration test: offline create 5 transactions → online sync → verify in SurrealDB → verify local matches
- [ ] `cargo test` passes

---

## Notes for Reviewer

- SQLite is chosen over SurrealDB's embedded mode because SQLite is battle-tested for local storage and has excellent Rust bindings
- The sync engine is **eventually consistent** — there will be a window where local and remote diverge. Conflict resolution is last-write-wins, not CRDT-based
- Encryption key is derived from the user's password — if the user forgets their password, encrypted local data is unrecoverable. This must be documented in the UI
- Compression uses lz4 (fast, good ratio) rather than zstd (better ratio, slower) because sync speed matters more than storage savings
- Network detection is best-effort (try to connect, catch error) — not a reliable connectivity check
