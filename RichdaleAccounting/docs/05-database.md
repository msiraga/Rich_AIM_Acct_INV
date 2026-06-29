# Data Layer

## Overview

The data layer uses the **Repository Pattern** — traits define the interface, with two implementations each:

| Repository Trait | SurrealDB Impl | In-Memory Impl | Status |
|---|---|---|---|
| `AuditRepository` | `SurrealAuditRepository` | `MemoryAuditRepository` | Complete |
| `DocumentRepository` | `SurrealDocumentRepository` | `MemoryDocumentRepository` | Complete |
| `UserRepository` | `SurrealUserRepository` | `MemoryUserRepository` | Complete |

Every SurrealDB implementation wraps `Arc<Mutex<Option<Surreal<Client>>>>` — the `Option` allows lazy connection. **In practice, no connection is ever established.**

---

## Core Data Models

### Financial Domain

```
Account
├── id: Uuid
├── number: String            ← e.g., "1000"
├── name: String              ← e.g., "Cash"
├── account_type: AccountType  ← Asset|Liability|Equity|Revenue|Expense
├── parent_id: Option<Uuid>   ← Hierarchical accounts
├── status: AccountStatus     ← Active|Inactive|Frozen|Closed
├── balance: Decimal
├── is_bank_account: bool
├── bank_details: Option<BankAccountDetails>
└── is_reconciled: bool

Transaction
├── id: Uuid
├── number: String            ← Auto-generated: "TRX-00000001"
├── description: String
├── date: DateTime<Utc>
├── transaction_type: TransactionType
├── status: TransactionStatus ← Draft|Pending|Posted|Reconciled|Voided
├── entries: Vec<TransactionEntry>
├── journal_entry_id: Option<Uuid>
├── document_ids: Vec<String>
└── metadata: serde_json::Value

TransactionEntry
├── id: Uuid
├── account_id: Uuid
├── entry_type: EntryType     ← Debit|Credit
├── amount: Decimal
├── description: String       ← Memo
└── reference: Option<String>

JournalEntry
├── id: Uuid
├── number: String            ← Auto-generated: "JE-00000001"
├── date: NaiveDate
├── description: String
├── entries: Vec<TransactionEntry>
├── is_posted: bool
├── posted_at: Option<DateTime<Utc>>
└── is_reconciled: bool

Reconciliation
├── id: Uuid
├── account_id: Uuid
├── statement_date: NaiveDate
├── starting_balance: Decimal
├── ending_balance: Decimal
├── statement_ending_balance: Decimal
├── reconciled_transactions: Vec<Uuid>
├── outstanding_transactions: Vec<Uuid>
├── difference: Decimal
├── status: ReconciliationStatus  ← InProgress|Completed|NeedsReview|Cancelled
└── notes: String
```

### Business Domain

```
Organization
├── id, name, description
├── address: Address
├── contact: ContactInfo
├── tax_id: Option<String>
├── currency: String          ← Default "USD"
└── accounting_period: AccountingPeriod

User
├── id, username, email, password_hash, display_name
├── role: UserRole            ← Admin|Manager|User|Viewer|Guest
├── is_active: bool
└── last_login: Option<DateTime<Utc>>

Document
├── id: String
├── name, document_type: DocumentType
├── content: Vec<u8>          ← Binary content
├── metadata: serde_json::Value
└── bounding_box: Option<BoundingBox>

AuditLog
├── id, user_id, action: AuditAction
├── entity_type, entity_id
├── old_values, new_values: Option<serde_json::Value>
├── timestamp, ip_address, user_agent
├── success: bool
└── error_message: Option<String>
```

---

## Database Choice: SurrealDB

The project targets **SurrealDB** — a multi-model (document + graph) database written in Rust. This is an interesting choice because:

| Advantage | Disadvantage |
|---|---|
| Schema-flexible (good for evolving accounting rules) | Relatively young project (1.0 released 2023) |
| Graph relationships (accounts → parent, transactions → documents) | Smaller ecosystem than PostgreSQL |
| Real-time subscriptions (useful for live dashboards) | Fewer hosted options |
| Rust-native client | Limited SQL compatibility |
| Embedded mode possible (good for edge/offline) | |

The `Cargo.toml` declares `surrealdb = { version = "1.0", features = ["kv-mem", "protocol-ws"] }` — enabling in-memory key-value store (testing) and WebSocket protocol (remote connections).

---

## Error Handling

The `DatabaseError` enum covers:

```rust
ConnectionError | QueryError | NotFound | DuplicateRecord |
ValidationError | SerializationError | DeserializationError |
ConstraintViolation | TransactionError | NotInitialized |
AlreadyInitialized | MigrationError | IoError | SurrealError | Other
```

With `From` impls for `surrealdb::Error`, `serde_json::Error`, `uuid::Error`, and `std::io::Error`.

---

## What's Missing

1. **No database initialization or migration scripts** — No schema definitions, no `DEFINE TABLE` statements
2. **The `Database` struct is referenced but never defined** — `api/mod.rs` uses `Arc<Mutex<Database>>` but `struct Database` does not exist in the codebase
3. **No connection pooling** — Each repository holds its own `Arc<Mutex<Option<Surreal<...>>>>`
4. **No indexes defined** — Queries like `find_by_username` would be full scans
5. **No data seeding** — The default chart of accounts is created in-memory by the Ledger, not in the database
6. **No encryption at rest** — `password_hash` is stored but no hashing logic exists
