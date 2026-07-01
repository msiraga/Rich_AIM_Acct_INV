You are starting Phase 6: Edge & Sync for NexusLedger, an agentic accounting platform
in Rust (Tauri + React frontend, SurrealDB backend, agent-based architecture).

## Repository & State
c:\Users\msira\Richdale_Accounting\Rich_AIM_Acct_INV
Latest commit: Phase 4 complete (287 tests, 0 failures)
Phase 5 (AI Pipeline) is running in a separate session — no overlap.

## Read These First
1. RichdaleAccounting/Phases/TRACKER.md         ← Phase 4 done, P6 next
2. RichdaleAccounting/Phases/00-strategy.md     ← methodology + principles
3. RichdaleAccounting/Phases/07-phase-6-edge-sync.md ← original phase plan
4. RichdaleAccounting/Phases/phase-4-handoff.md ← cross-phase parallelism guide
5. RichdaleAccounting/docs/02-architecture.md   ← system diagram

## Phase 6: Edge & Sync — 11 Tasks (upgraded from 10)

**Objective:** Offline-first data storage, periodic sync with SurrealDB,
conflict resolution with user review, local encryption with key management.
App must work with no internet and sync when reconnected.

### Current State of `edge/mod.rs`
- `EdgeConfig` exists with `compress_data: true`, `encrypt_data: false`, `storage_path: "./data/edge"`
- `EdgeManager` struct exists with `start_sync()`, `perform_sync()` (stub), `is_online()`,
  `get_sync_status()`, `enable_offline_mode()`, `disable_offline_mode()`
- `LocalDb`, `LocalStore`, `ChangeTracker` types referenced but don't exist yet
- No local database, no change tracking, no persistence

### CRITICAL: QuickBooks-Grade Quality Standard (A+)
Every edge feature must be production-grade:
- **Offline CRUD:** Must work identically to online — same double-entry validation,
  same account existence checks, same journal entry creation, same balance updates
- **Sync:** Never lose data. Conflict presented to user for review, never auto-overwritten.
  Idempotent retries with exponential backoff. Partial batch failure handled gracefully.
- **Encryption:** AES-256-GCM with key wrapping (DEK encrypted by KEK from password).
  Keys zeroized from memory after use. Password changes don't break existing data.
- **Compression:** Verifiable round-trip with CRC32 checksums. Skip if compression
  makes data larger. Track compression ratio stats.
- **Conflict resolution:** Both versions preserved. User reviews field-by-field differences.
  Full audit trail: both records, differing fields, resolution, resolver, timestamp.
- **Data integrity:** Post-sync verification: compare COUNT and SUM between local and remote.
  SQLite WAL mode for concurrent read/write. Graceful degradation if SQLite fails.
- **Tests:** Real offline→online→sync flow, conflict scenario, deletion propagation,
  network failure mid-sync, large sync (100+ records), empty state.

## Maximum Parallelism Strategy

### File Conflict Analysis
```
Phase 6 files:
  NEW (no conflicts):     edge/local_db.rs         ← Agent A
  NEW (no conflicts):     edge/encryption.rs       ← Agent B
  NEW (no conflicts):     edge/compression.rs      ← Agent C
  NEW (no conflicts):     components/SyncStatus.tsx← Agent D (frontend)
  NEW (no conflicts):     components/ConflictResolver.tsx ← Agent D2 (frontend)
  NEW (depends on A):     edge/store.rs            ← Agent E (after A)
  NEW (depends on E):     edge/tracking.rs         ← Agent F (after E)
  NEW (depends on F):     edge/sync.rs             ← Agent G (after F)
  NEW (depends on G):     edge/conflict.rs         ← Agent H (after G)
  EXISTING (central):     edge/mod.rs              ← COORDINATOR
  EXISTING (central):     Cargo.toml               ← COORDINATOR
  EXISTING (central):     App.tsx                  ← COORDINATOR
  EXISTING (central):     index.css                ← COORDINATOR
```

### Round 1 — 5 Parallel Agents (NEW files, zero dependencies)

| Agent | Task | Creates | A+ Quality Standard |
|-------|------|---------|---------------------|
| **A** | 6.1 Local DB | `edge/local_db.rs` | **Full schema** mirroring ALL SurrealDB tables including Phase 4 additions (multi-currency fields on transaction_entry: `currency TEXT DEFAULT 'USD'`, `exchange_rate TEXT`, `base_currency_amount TEXT`; AP bills with `vendor_id`, `bill_number`, `status`, `amount_paid`; budgets with `period_start`, `period_end`, `budgeted_amount`; fixed assets with `cost`, `salvage_value`, `useful_life_months`, `accumulated_depreciation`, `disposed_date`). **Migration runner** with `schema_version` table, `PRAGMA user_version`. **Foreign keys**: `PRAGMA foreign_keys = ON`, `FOREIGN KEY (account_id) REFERENCES accounts(id)`. **Indices**: `CREATE INDEX idx_changes_dirty ON changes(dirty)`, `idx_txn_date ON transactions(date)`, `idx_entry_account ON transaction_entries(account_id)`. **WAL mode**: `PRAGMA journal_mode=WAL` on every connection. Use `rusqlite` with `bundled` feature. Connection method: `LocalDb::open(path) -> Result<Self>`, `LocalDb::migrate(&self) -> Result<()>`. |
| **B** | 6.8 Encryption | `edge/encryption.rs` | **Key wrapping**: Data Encryption Key (DEK, 256-bit random) encrypts fields. Key Encryption Key (KEK) derived from password via HKDF-SHA256. DEK stored encrypted by KEK in a `encryption_keys` table. On password change: re-wrap DEK with new KEK (no data re-encryption needed). **Nonce**: random 96-bit nonce per encryption, stored as prefix of ciphertext. NEVER reuse nonces. **Zeroize**: use `zeroize` crate to clear KEK/DEK from memory after use. **Sensitive fields list**: `bank_account_number`, `routing_number`, `tax_id`, `ssn`, `credit_card_number`. Methods: `encrypt_field(plaintext, dek) -> Vec<u8>` (nonce+ciphertext+tag), `decrypt_field(blob, dek) -> Result<String>`, `derive_kek(password, salt) -> [u8;32]`, `generate_dek() -> [u8;32]`, `wrap_dek(dek, kek) -> Vec<u8>`, `unwrap_dek(wrapped, kek) -> [u8;32]`, `change_password(old_pw, new_pw) -> Result<()>`. |
| **C** | 6.9 Compression | `edge/compression.rs` | **CRC32 checksum**: stored as 4-byte prefix on compressed blobs. On decompress: verify checksum, return `Err` on mismatch (never decompress corrupted data). **Skip-if-larger**: if `compressed.len() >= original.len()`, store uncompressed with a 1-byte flag prefix (0=uncompressed, 1=compressed). **Stats tracking**: `CompressionStats { original_bytes, compressed_bytes, ratio, blob_count }` exposed via `get_stats()`. Methods: `compress_blob(data) -> Vec<u8>` (flag+checksum+compressed), `decompress_blob(data) -> Result<Vec<u8>>`, `get_stats() -> CompressionStats`. Use `lz4` crate. |
| **D** | 6.7 Sync UI | `components/SyncStatus.tsx` | **4 states with details**: "Up to date" (green dot + "Last synced: 2 min ago"), "Syncing..." (yellow spinner + progress bar "Pushing 47/100, Pulling 12"), "Offline — N changes pending" (orange + "Offline for 2h 15m" + pending count), "Sync error" (red + expandable error details "Network timeout at 10:32 AM" + "Retry" button). **Polling**: `GET /api/v1/edge/status` every 5s when online, every 30s when offline. **Manual sync**: `POST /api/v1/edge/sync` with debounce (disable button for 3s after click). **Queue warning**: if pending > 5000, show amber warning "Connect to sync soon." |
| **D2** | 6.5b Conflict UI | `components/ConflictResolver.tsx` | **Side-by-side diff**: left column = local version, right column = remote version. **Field-level highlighting**: fields that differ are highlighted yellow. **Resolution options**: "Keep local", "Keep remote", "Keep both" (for non-conflicting field changes). **Audit info**: shows who modified each version and when. **Batch resolve**: if 10 conflicts, show "Resolve all with: [local/remote]" shortcut. Calls `POST /api/v1/edge/conflicts/:id/resolve` with chosen version. |

### Round 2 — Sequential Chain (each builds on prior schema)

| Agent | Task | Creates | Depends On | A+ Quality Standard |
|-------|------|---------|------------|---------------------|
| **E** | 6.2 Local Store | `edge/store.rs` | Agent A (local_db schema) | **Double-entry validation**: before saving a transaction, verify `sum(debits) == sum(credits)`. Reject unbalanced. **Account existence check**: verify every `account_id` in entries exists in local `accounts` table. **Full parity with online ledger**: create journal entry, update account balances, assign transaction number (`TRX-XXXXX` format). **Batch operations**: `save_batch(transactions) -> Result<Vec<Transaction>>` wrapped in single `BEGIN TRANSACTION; ... COMMIT;`. **Soft-delete**: `delete_transaction(id)` sets `_deleted=true` (not hard delete — needed for sync). Methods: `save_account()`, `save_transaction()`, `get_transaction(id)`, `list_transactions()`, `list_accounts()`, `delete_transaction(id)`, `save_batch()`. |
| **F** | 6.3 Tracking | `edge/tracking.rs` | Agent E (store CRUD) | **Atomic writes**: every store operation wrapped in `BEGIN TRANSACTION; [INSERT/UPDATE]; UPDATE ... SET _dirty=1, _modified_at=now(); INSERT INTO changes(...); COMMIT;`. If any step fails, entire transaction rolls back. **Soft-delete tracking**: deletions record `operation='delete'` in changes table BEFORE the soft-delete flag is set (so the change record survives even if the data is later hard-deleted). **Ordering guarantee**: `changes` table has `seq INTEGER PRIMARY KEY AUTOINCREMENT` — sync processes in `seq` order. **Entity type enum**: `EntityType::Account, Transaction, JournalEntry, Invoice, Bill, Budget, Asset, Vendor, Document`. **Methods**: `record_change(entity_type, entity_id, operation)`, `get_dirty_records() -> Vec<ChangeRecord>`, `mark_synced(seq_ids: &[i64])`, `get_pending_count() -> u64`, `get_pending_by_type() -> HashMap<EntityType, u64>`. |
| **G** | 6.4 Sync Engine | `edge/sync.rs` | Agent F (tracking) | **Push order**: sync in dependency order — accounts first, then transactions (reference accounts), then journal entries (reference transactions), then invoices/bills/assets. **UUID dedup**: locally-created records have UUIDs. SurrealDB rejects duplicate IDs. Catch `DuplicateRecord` error → mark change as synced (it already exists remotely). **Pull idempotency**: each local record stores `_remote_updated_at`. On pull, compare incoming `updated_at` with `_remote_updated_at` — skip if same or older. **Retry with backoff**: on network error: retry after 1s, 2s, 4s, 8s, 16s, 32s, 60s (max). After 10 failures: stop, set sync state to Error, report to UI. **Partial batch failure**: push records one-by-one (or small batches of 10). If record #47 fails, log it, continue with #48-100. Failed records keep `_dirty=true`. **Progress events**: `SyncProgress { pushed: u32, push_total: u32, pulled: u32, pull_total: u32, conflicts: u32 }` updated after each record. **Atomic last_sync update**: `UPDATE sync_state SET last_sync = now()` in same transaction as `mark_synced()`. **Methods**: `push_changes() -> Result<SyncResult>`, `pull_changes() -> Result<SyncResult>`, `sync_all() -> Result<SyncResult>`. |
| **H** | 6.5 Conflict Resolution | `edge/conflict.rs` | Agent G (sync engine) | **No automatic overwrite**: when local `_modified_at` != remote `updated_at` AND both differ from `_remote_updated_at` (last synced version), flag as conflict. **Both versions preserved**: store local version in `conflicts` table as JSON, store remote version as JSON, store field-level diff. **Conflict record**: `Conflict { id, entity_type, entity_id, local_version: serde_json::Value, remote_version: serde_json::Value, diff_fields: Vec<String>, local_modified_at, remote_modified_at, status: Pending/Resolved, resolution: Option<Local/Remote/Merged>, resolved_by: Option<Uuid>, resolved_at: Option<DateTime> }`. **Field-level merge**: when user chooses "Keep both", non-conflicting field changes from both versions are merged (e.g., local changed description, remote changed amount → both survive). **Audit trail**: every conflict and resolution is written to the existing `AuditLog` via `AuditAction::Custom("sync_conflict")`. **Methods**: `detect_conflicts(pulled_records) -> Vec<Conflict>`, `resolve_conflict(id, resolution) -> Result<()>`, `list_pending_conflicts() -> Vec<Conflict>`, `merge_versions(local, remote, diff_fields) -> serde_json::Value`. |

### Round 3 — Central Coordinator (after ALL agents complete)

| Step | File(s) | What |
|------|---------|------|
| 1 | `Cargo.toml` | Add: `rusqlite = { version = "0.31", features = ["bundled"] }`, `aes-gcm = "0.10"`, `hkdf = "0.12"`, `sha2 = "0.10"`, `lz4 = "1.24"`, `rand = "0.8"`, `zeroize = { version = "1.7", features = ["derive"] }`, `crc32fast = "1.4"` |
| 2 | `edge/mod.rs` | Register ALL submodules. Wire `EdgeManager`: when `offline_mode=true`, route all writes through `LocalStore` (with tracking). When online, `perform_sync()` calls `sync::push_changes()` → `sync::pull_changes()` → `conflict::detect_conflicts()`. **Network detection**: background task pings `GET /health` every 30s. 3 consecutive failures → auto-enable offline mode. On success after failures: wait 5s (debounce), then `disable_offline_mode()` → auto-trigger sync. **Queue depth warning**: if `get_pending_count() > 5000`, log warning. **Periodic sync**: `tokio::spawn` background task that calls `sync_all()` every `sync_interval` seconds when online. **Graceful degradation**: if SQLite fails to open (disk full/corrupt), fall back to online-only mode, show red error banner in UI. |
| 3 | `api/mod.rs` | Add routes: `GET /api/v1/edge/status` (sync state + pending count + last sync + conflicts), `POST /api/v1/edge/sync` (manual trigger), `GET /api/v1/edge/conflicts` (list pending), `POST /api/v1/edge/conflicts/:id/resolve` (resolve with body `{resolution: "local"/"remote"/"merged"}`). All require `RequireUser` guard. |
| 4 | `App.tsx` | Add `<SyncStatus />` as fixed bottom-right indicator inside ProtectedRoute. Add `/conflicts` route rendering `<ConflictResolver />`. |
| 5 | `index.css` | Add `.sync-status` (fixed bottom-right, 280px wide, z-index 1000), `.sync-dot` (12px circle, color variants), `.sync-progress-bar` (animated), `.conflict-diff` (side-by-side grid), `.conflict-highlight` (yellow background). |

### Round 4 — Integration Test

| Task | File | A+ Quality Standard |
|------|------|---------------------|
| 6.10 E2E | `tests/integration/edge.rs` | **Test 1 — Basic sync**: Fresh SQLite → offline → create 5 balanced transactions → verify `_dirty=true` → online → sync → verify all 5 in SurrealDB → verify `_dirty=false` → verify COUNT and SUM match. **Test 2 — Pull**: Create transaction on SurrealDB directly → sync → verify appears in local SQLite. **Test 3 — Conflict**: Modify same transaction locally (change amount to $200) AND remotely (change amount to $300) → sync → verify conflict detected → verify both versions preserved → resolve (keep local) → verify $200 in both. **Test 4 — Deletion**: Delete transaction offline → sync → verify deleted on SurrealDB. Delete transaction on SurrealDB → sync → verify deleted locally. **Test 5 — Network failure mid-sync**: Start sync with 100 records → simulate network failure at record 50 → verify records 1-49 synced, 50-100 still dirty → retry → verify all 100 synced. **Test 6 — Large sync**: Create 100 transactions offline → sync → verify all 100 in SurrealDB within 10 seconds. **Test 7 — Empty state**: Clean install, no local data → sync → verify no errors, no phantom records. **Test 8 — Data integrity**: After all syncs, run `SELECT COUNT(*), SUM(total_debits) FROM transactions` locally vs SurrealDB → verify match. **Test 9 — Encryption**: Create transaction with bank_account_number → verify field is encrypted in SQLite (read raw bytes, confirm not plaintext) → decrypt → verify matches original. **Test 10 — Compression**: Store 10KB document blob → verify compressed size < original → decompress → verify byte-for-byte match → verify CRC32 checksum matches. |

## Agent Prompt Templates

### Round 1 Agent (A-D2: independent new files)
```
You are implementing Phase 6 Task [6.X] [NAME] for NexusLedger.
Read edge/mod.rs for existing patterns and EdgeConfig.

CREATE ONLY: [filename] — a single new file.

Do NOT edit edge/mod.rs, Cargo.toml, api/mod.rs, App.tsx, or any existing file.
Your file will be registered centrally after all agents complete.

[SPECIFIC_CONTEXT — see A+ Quality Standard column above]

Include comprehensive unit tests. Follow the existing code style
(#[derive(Debug, Clone)], thiserror for errors, tracing for logging).
All financial amounts use rust_decimal::Decimal.
```

### Round 2 Agent (E-H: depends on prior agent's file)
```
You are implementing Phase 6 Task [6.X] [NAME] for NexusLedger.
Read the file created by the previous agent: edge/[dependency].rs
Read edge/mod.rs for existing patterns. Read database/financial.rs for
Transaction/TransactionEntry/Account types.

CREATE ONLY: edge/[filename].rs

Do NOT edit edge/mod.rs, Cargo.toml, or any existing file.

[SPECIFIC_CONTEXT — see A+ Quality Standard column above]
```

## Key File Paths (what you'll create)
```
nexus-core/src/edge/
├── mod.rs              ← EXISTS — register submodules + offline toggle + periodic sync + network detection
├── local_db.rs         ← NEW Agent A — SQLite schema (ALL tables) + migrations + WAL + FK + indices
├── store.rs            ← NEW Agent E — CRUD with double-entry validation + batch + soft-delete
├── tracking.rs         ← NEW Agent F — atomic change tracking + seq ordering + entity types
├── sync.rs             ← NEW Agent G — push/pull with dedup + idempotency + backoff + progress
├── conflict.rs         ← NEW Agent H — both-version preservation + field-level merge + audit
├── encryption.rs       ← NEW Agent B — AES-256-GCM + DEK/KEK key wrapping + zeroize
└── compression.rs      ← NEW Agent C — lz4 + CRC32 checksum + skip-if-larger + stats

nexus-ledger-tauri/src/
└── components/
    ├── SyncStatus.tsx       ← NEW Agent D — 4-state indicator + progress bar + error details
    └── ConflictResolver.tsx← NEW Agent D2 — side-by-side diff + field highlighting + batch resolve

nexus-core/tests/
└── integration/
    └── edge.rs         ← NEW (Round 4) — 10 comprehensive E2E tests
```

## SQLite Schema (Agent A must implement ALL of these)
```sql
-- Enable WAL mode and foreign keys
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;

-- Migration versioning
CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY);
INSERT OR IGNORE INTO schema_version (version) VALUES (1);

-- Sync state
CREATE TABLE IF NOT EXISTS sync_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    last_sync TEXT,
    last_successful_sync TEXT
);

-- Accounts (mirrors SurrealDB account table, 20 default accounts)
CREATE TABLE IF NOT EXISTS accounts (
    id TEXT PRIMARY KEY,
    number TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    account_type TEXT NOT NULL,
    balance TEXT NOT NULL DEFAULT '0',
    status TEXT NOT NULL DEFAULT 'active',
    currency TEXT NOT NULL DEFAULT 'USD',
    _dirty INTEGER NOT NULL DEFAULT 0,
    _deleted INTEGER NOT NULL DEFAULT 0,
    _modified_at TEXT NOT NULL,
    _remote_updated_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_accounts_dirty ON accounts(_dirty);
CREATE INDEX IF NOT EXISTS idx_accounts_number ON accounts(number);

-- Transactions
CREATE TABLE IF NOT EXISTS transactions (
    id TEXT PRIMARY KEY,
    number TEXT NOT NULL,
    description TEXT NOT NULL,
    date TEXT NOT NULL,
    transaction_type TEXT NOT NULL,
    status TEXT NOT NULL,
    journal_entry_id TEXT,
    metadata TEXT DEFAULT '{}',
    _dirty INTEGER NOT NULL DEFAULT 0,
    _deleted INTEGER NOT NULL DEFAULT 0,
    _modified_at TEXT NOT NULL,
    _remote_updated_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_transactions_dirty ON transactions(_dirty);
CREATE INDEX IF NOT EXISTS idx_transactions_date ON transactions(date);

-- Transaction entries (with Phase 4 multi-currency fields)
CREATE TABLE IF NOT EXISTS transaction_entries (
    id TEXT PRIMARY KEY,
    transaction_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    entry_type TEXT NOT NULL,
    amount TEXT NOT NULL,
    description TEXT,
    reference TEXT,
    currency TEXT NOT NULL DEFAULT 'USD',
    exchange_rate TEXT,
    base_currency_amount TEXT,
    FOREIGN KEY (transaction_id) REFERENCES transactions(id),
    FOREIGN KEY (account_id) REFERENCES accounts(id)
);
CREATE INDEX IF NOT EXISTS idx_entries_transaction ON transaction_entries(transaction_id);
CREATE INDEX IF NOT EXISTS idx_entries_account ON transaction_entries(account_id);

-- Change tracking (sync queue)
CREATE TABLE IF NOT EXISTS changes (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    operation TEXT NOT NULL,  -- 'insert', 'update', 'delete'
    timestamp TEXT NOT NULL,
    dirty INTEGER NOT NULL DEFAULT 1,
    synced_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_changes_dirty ON changes(dirty);
CREATE INDEX IF NOT EXISTS idx_changes_entity ON changes(entity_type, entity_id);

-- Encryption keys (DEK wrapped by KEK)
CREATE TABLE IF NOT EXISTS encryption_keys (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    wrapped_dek BLOB NOT NULL,    -- DEK encrypted by password-derived KEK
    salt BLOB NOT NULL,            -- HKDF salt
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Conflicts (both versions preserved for user review)
CREATE TABLE IF NOT EXISTS conflicts (
    id TEXT PRIMARY KEY,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    local_version TEXT NOT NULL,   -- JSON of local record
    remote_version TEXT NOT NULL,  -- JSON of remote record
    diff_fields TEXT NOT NULL,     -- JSON array of field names that differ
    local_modified_at TEXT NOT NULL,
    remote_modified_at TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',  -- 'pending', 'resolved'
    resolution TEXT,               -- 'local', 'remote', 'merged'
    resolved_by TEXT,
    resolved_at TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_conflicts_status ON conflicts(status);
```

## Freeze Token 6 (all must pass — A+ standard)
- [ ] App starts in offline mode and ALL CRUD works against local SQLite (same validation as online)
- [ ] Creating a transaction offline: double-entry validated, journal entry created, balances updated, `_dirty=true`
- [ ] Going online triggers sync — all dirty records pushed to SurrealDB in dependency order
- [ ] Pulling remote changes: records modified since last_sync appear locally (idempotent — no duplicates)
- [ ] Conflict: same record modified locally and remotely → both versions preserved → user reviews → resolves
- [ ] Sync status UI shows 4 states with details (last sync time, pending count, error details, progress bar)
- [ ] Manual sync button triggers sync with debounce
- [ ] Conflict resolver UI shows side-by-side diff with field-level highlighting
- [ ] Sensitive fields (bank_account_number, tax_id, ssn) encrypted at rest (AES-256-GCM with DEK/KEK)
- [ ] Password change does NOT break existing encrypted data (DEK re-wrapped with new KEK)
- [ ] Large documents compressed with CRC32 checksum (verifiable round-trip, measurable size reduction)
- [ ] Network failure mid-sync: partial sync doesn't corrupt data, retry syncs remaining records
- [ ] Deletion propagation: delete offline → syncs to SurrealDB; delete on SurrealDB → syncs to local
- [ ] Data integrity verification: COUNT and SUM match between local SQLite and SurrealDB after sync
- [ ] SQLite WAL mode enabled (concurrent reads don't block writes)
- [ ] Integration test: 10 comprehensive E2E tests (basic sync, pull, conflict, deletion, network failure, large sync, empty state, data integrity, encryption, compression)
- [ ] `cargo test` passes (all unit + integration)

## Dependencies Reminder
- `rusqlite` with `bundled` feature (no system SQLite required)
- `aes-gcm` + `hkdf` + `sha2` + `zeroize` for encryption
- `lz4` + `crc32fast` for compression
- All added by coordinator in Round 3 — agents do NOT touch Cargo.toml

Begin Round 1: launch agents A, B, C, D, D2 in parallel.
