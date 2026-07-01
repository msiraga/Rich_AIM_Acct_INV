# NexusLedger

**Agentic Accounting Platform — QuickBooks Replacement**

NexusLedger is a full-featured, agent-based accounting platform built in Rust.
It replaces QuickBooks with a modern, offline-first, AI-enhanced architecture:
double-entry ledger, accounts payable/receivable, invoicing, multi-currency,
budgets, fixed-asset depreciation, bank reconciliation, tax calculation,
payroll, financial reporting, OCR-powered receipt processing, anomaly
detection, and encrypted local sync — all driven by 9 autonomous agents.

---

## Table of Contents

- [Architecture](#architecture)
- [Features](#features)
- [10-Minute Quick Start](#10-minute-quick-start)
- [Tech Stack](#tech-stack)
- [FAQ](#faq)
- [License](#license)

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        NEXUSLEDGER                                      │
│                                                                         │
│  ┌──────────────────────┐          ┌──────────────────────────────────┐ │
│  │  Desktop Client       │          │         NexusLedger Core         │ │
│  │  (Tauri 2 + React 18) │─────────▶│                                  │ │
│  │                       │   HTTP   │  ┌────────────────────────────┐  │ │
│  │  • Dashboard          │   /WS    │  │   AgentOrchestrator         │  │ │
│  │  • Accounts           │          │  │   ┌────────────────────┐   │  │ │
│  │  • Journal Entry      │          │  │   │ Task Queue          │   │  │ │
│  │  • Invoices           │          │  │   │ In-Progress         │   │  │ │
│  │  • Transactions       │          │  │   │ Completed / Failed  │   │  │ │
│  │  • Documents (OCR)    │          │  │   └─────────┬──────────┘   │  │ │
│  │  • Reports            │          │  └────────────┼────────────────┘  │ │
│  │  • Sync Status        │          │               │                    │ │
│  └──────────────────────┘          │  ┌────────────┼────────────────┐  │ │
│                                     │  │   9 Agents (dispatch)       │  │ │
│  ┌──────────────────────┐          │  │                             │  │ │
│  │  REST API (axum)     │─────────▶│  │  Ledger    Reconciliation  │  │ │
│  │  Port 8080           │          │  │  Tax       Payroll         │  │ │
│  │                      │          │  │  Invoice   Receipt          │  │ │
│  │  • Auth (JWT/argon2) │          │  │  Document  Audit           │  │ │
│  │  • Rate limiting     │          │  │  Reporting                 │  │ │
│  │  • Prometheus metrics│          │  └────────────────────────────┘  │ │
│  └──────────────────────┘          │                                    │ │
│                                     │  ┌────────────────────────────┐  │ │
│                                     │  │  Accounting Engine          │  │ │
│                                     │  │  • Double-entry Ledger      │  │ │
│                                     │  │  • Reconciliation           │  │ │
│                                     │  │  • Tax Calculator           │  │ │
│                                     │  │  • Payroll Processor        │  │ │
│                                     │  └─────────────┬──────────────┘  │ │
│                                     │                │                   │ │
│                                     │  ┌─────────────┴──────────────┐  │ │
│                                     │  │  AI Pipeline                │  │ │
│                                     │  │  • Mistral OCR4 (cloud)     │  │ │
│                                     │  │  • llama-cpp-rs (local GGUF)│  │ │
│                                     │  │  • Embeddings & search      │  │ │
│                                     │  │  • Anomaly detection        │  │ │
│                                     │  │  • Smart categorization     │  │ │
│                                     │  └────────────────────────────┘  │ │
│                                     │                                    │ │
│                                     │  ┌────────────────────────────┐  │ │
│                                     │  │  Edge / Sync Layer          │  │ │
│                                     │  │  • Local SQLite (rusqlite)  │  │ │
│                                     │  │  • Change tracking (_dirty)  │  │ │
│                                     │  │  • Push/pull sync engine    │  │ │
│                                     │  │  • Conflict resolution      │  │ │
│                                     │  │  • AES-256-GCM encryption   │  │ │
│                                     │  │  • lz4 compression          │  │ │
│                                     │  └────────────────────────────┘  │ │
│                                     │                                    │ │
│                                     │  ┌────────────────────────────┐  │ │
│                                     │  │  Data Layer (SurrealDB)     │  │ │
│                                     │  │  • Users / Orgs / Roles     │  │ │
│                                     │  │  • Accounts / Transactions  │  │ │
│                                     │  │  • Documents / Audit Logs   │  │ │
│                                     │  └────────────────────────────┘  │ │
│                                     └──────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────┘
```

### Module Dependency Graph

```
                 ┌─────────────┐
                 │   lib.rs    │  ← Crate root, NexusLedger struct
                 └──────┬──────┘
        ┌───────────────┼───────────────┐
        ▼               ▼               ▼
  ┌──────────┐   ┌──────────┐   ┌──────────┐
  │ agents/  │   │accounting│   │database/ │
  │ 9 agents │◄──┤  ledger  │◄──┤  models  │
  │ orchestr.│   │  recon.  │   │  financial│
  └────┬─────┘   │  tax     │   │  user    │
       │         │  payroll │   └────┬─────┘
       ▼         └──────────┘        │
  ┌──────────┐  ┌──────────┐  ┌──────────┐
  │   ai/    │  │  edge/   │  │  api/    │
  │ OCR      │  │ local DB │  │  axum    │
  │ extract  │  │ sync     │  │  auth    │
  │ anomaly  │  │ encrypt  │  │  routes  │
  └──────────┘  └──────────┘  └──────────┘
```

---

## Features

### Phase 0–1: Foundation & Database

- **SurrealDB persistence** — document-graph database with auto-schema on first run
- **Migration runner** — `schema_version` tracking, idempotent migrations
- **Default chart of accounts** — 20 pre-seeded accounts covering all standard categories
- **Connection management** — WebSocket + in-memory fallback

### Phase 2: Agent Engine

- **9 autonomous agents** with real task processing:
  - **LedgerAgent** — double-entry transactions, chart of accounts, trial balance
  - **ReconciliationAgent** — bank statement matching, discrepancy identification
  - **TaxAgent** — US federal/state tax calculations, filing deadline tracking
  - **PayrollAgent** — payroll with Social Security, Medicare, federal/state withholding
  - **InvoiceAgent** — customer invoices, payment tracking, customer statements
  - **ReceiptAgent** — receipt processing, expense categorization
  - **DocumentAgent** — financial document storage, retrieval, organization
  - **AuditAgent** — tamper-evident audit trail, anomaly detection, fraud checks
  - **ReportingAgent** — balance sheet, P&L, cash flow, trial balance
- **Event-driven task dispatch loop** — queued, in-progress, completed, failed states

### Phase 3: API & Frontend

- **axum REST API** on port 8080 — full CRUD for accounts, transactions, invoices
- **WebSocket chat** — conversational natural-language interface at `ws://localhost:8080/ws/chat`
- **React 18 frontend** with react-router — dashboard, accounts, transactions, journal entry, invoices
- **CORS + graceful shutdown** — production-ready middleware
- **Request ID + timing middleware** — `X-Request-Id` and `X-Response-Time-Ms` headers

### Phase 4: Auth & Accounting Completeness

- **Argon2id password hashing** — empty-password rejection, timing-attack mitigation with dummy hash
- **JWT authentication** — access tokens (30 min TTL), refresh tokens (7 days), rotation on refresh
- **Default-secret refusal** — server refuses to start without a proper `JWT_SECRET`
- **Role-based access control (RBAC)** — 5-tier hierarchy: Guest → Viewer → User → Manager → Admin
- **Accounts Payable** — vendor → bill → payment workflow, partial payments, multi-line bills
- **Accounts Receivable aging** — 4 buckets (current, 31-60, 61-90, 90+), as-of date filtering
- **Cash Flow Statement** — 3 GAAP sections (operating, investing, financing)
- **CSV import** — quoted-field parser, balance validation
- **CSV/OFX export** — date filtering, OFX with `TRNAMT` signing, `BANKACCTFROM`, `LEDGERBAL`
- **Multi-currency** — exchange rate management, per-entry currency fields, base-currency conversion
- **Budget tracking** — sign-based variance analysis, period overlap logic
- **Fixed assets & depreciation** — Straight-Line and Double-Declining Balance methods, auto-journal entries, asset disposal with gain/loss

### Phase 5: AI Pipeline

- **Mistral OCR4 API** — cloud-based OCR for receipt and document text extraction
- **PDF text extraction** — `pdf-extract` crate for offline PDF processing
- **Document upload UI** — drag-and-drop interface
- **OCR-to-transaction pipeline** — OCR text → AI extraction → auto-created transaction
- **Embedding storage & vector search** — semantic similarity for document lookup
- **Transaction anomaly detection** — statistical outlier flagging
- **Smart account categorization** — ML-assisted account suggestion
- **Graceful degradation** — AI features degrade cleanly when Mistral API / GGUF models unavailable

### Phase 6: Edge & Sync

- **Embedded SQLite** — offline-first local database via `rusqlite`
- **Change tracking** — `_dirty` flags on every record
- **Sync engine** — push dirty records to SurrealDB, pull remote changes to local
- **Conflict resolution** — last-write-wins, logged in audit trail
- **Offline mode toggle** — UI component showing sync state (offline/syncing/up-to-date/error)
- **Local encryption** — AES-256-GCM for sensitive fields at rest
- **Local compression** — lz4 for large documents

### Phase 7: Production Hardening

- **Lock contention audit** — reduced lock granularity for throughput
- **SurrealDB connection pooling** — efficient connection reuse
- **Rate limiting** — `governor`-based per-client request throttling
- **SurrealQL injection audit** — parameterized queries, no injection vectors
- **Frontend security audit** — XSS/CSRF/CSP hardening
- **Prometheus `/metrics`** — text exposition format, 11 metric series
- **Health endpoints** — `/health` (liveness), `/ready` (readiness with DB + agent checks)
- **Windows installer (MSI/EXE)** — Tauri bundler
- **macOS installer (DMG)** — Tauri bundler
- **System tray icon** — sync status indicator
- **Auto-update** — Tauri updater plugin
- **Performance benchmarks** — 10K transactions in < 2s
- **Load testing** — 100 concurrent requests, zero errors, p99 < 500ms

---

## 10-Minute Quick Start

```bash
# 1. Clone the repository
git clone https://github.com/msiraga/Rich_AIM_Acct_INV.git
cd Rich_AIM_Acct_INV

# 2. Build the Rust core
cd RichdaleAccounting
cargo build

# 3. Install frontend dependencies and launch the desktop app
cd ../nexus-ledger-tauri
npm install

# 4. Set your JWT secret (required — server refuses to start without it)
#    On Windows (PowerShell):
$env:JWT_SECRET = "your-secret-key-at-least-32-bytes-long!"
#    On macOS / Linux:
export JWT_SECRET="your-secret-key-at-least-32-bytes-long"

# 5. Launch the Tauri desktop app (starts API + frontend together)
cargo tauri dev
```

Once the app window opens:

1. **Register** — click Register, enter username, email, and password (8+ chars, at least one letter and one number). The first user is automatically Admin.
2. **Log in** — enter your credentials, you'll land on the Dashboard.
3. **Review accounts** — navigate to Accounts to see the 20 pre-seeded chart of accounts.
4. **Create your first transaction** — go to Journal Entry, add a debit to Cash (1000) for $1000 and a credit to Sales Revenue (4000) for $1000. Verify debits = credits, then save.
5. **View the dashboard** — return to the Dashboard to see your updated balances.

The API server runs on `http://localhost:8080`. Verify it is live:

```bash
curl http://localhost:8080/health
# {"status":"ok","uptime_seconds":12,"timestamp":"2026-07-01T..."}
```

For the full step-by-step walkthrough (register → seed → journal entry → trial balance → invoice → receipt → balance sheet), see [docs/user/quick-start.md](docs/user/quick-start.md).

---

## Tech Stack

| Category | Technology | Version |
|---|---|---|
| **Core language** | Rust | 2021 edition, MSRV 1.70+ |
| **Desktop framework** | Tauri | 2.x |
| **Frontend** | React | 18.2 |
| **Frontend routing** | react-router-dom | 7.x |
| **Build tool (frontend)** | Vite | 5.x |
| **HTTP framework** | axum | 0.7 |
| **Database** | SurrealDB | 1.x (kv-mem + protocol-ws) |
| **Embedded DB** | SQLite (rusqlite) | 0.31 (bundled) |
| **Authentication** | argon2 | 0.5 (argon2id) |
| **Token auth** | jsonwebtoken | 9.x (HS256 JWT) |
| **Rate limiting** | governor | 0.6 |
| **Metrics** | prometheus | 0.13 |
| **Encryption** | aes-gcm | 0.10 (AES-256-GCM) |
| **Key derivation** | hkdf | 0.12 |
| **Compression** | lz4 | 1.24 |
| **AI — OCR** | Mistral OCR4 API | cloud (via reqwest) |
| **AI — PDF** | pdf-extract | 0.8 |
| **AI — LLM inference** | llama-cpp-rs | 0.3 (local GGUF models) |
| **Decimal arithmetic** | rust_decimal | 1.36 |
| **HTTP client** | reqwest | 0.11 |
| **Middleware** | tower / tower-http | 0.4 / 0.5 (CORS) |
| **Tracing** | tracing / tracing-subscriber | 0.1 / 0.3 |
| **Error handling** | thiserror / anyhow | 1.0 / 1.0 |
| **Benchmarking** | criterion | 0.5 |
| **TypeScript** | TypeScript | 5.3 |

---

## FAQ

### "JWT_SECRET not set" / server refuses to start

The API server checks for a default/insecure JWT secret on startup and refuses to bind if one is not configured. Set the `JWT_SECRET` environment variable to a cryptographically random string of at least 32 bytes:

```bash
# Generate a secure secret:
openssl rand -base64 32

# Set it:
# Windows (PowerShell):
$env:JWT_SECRET = "<generated-secret>"
# macOS / Linux:
export JWT_SECRET="<generated-secret>"
```

### "Port 8080 (or 4000) is already in use"

The axum API server defaults to port 8080. The Tauri Vite dev server defaults to port 4000. If either is occupied:

```bash
# Override the API port:
export API_PORT=8081   # or set API_PORT in your environment

# Override the Vite dev server port:
# Edit nexus-ledger-tauri/vite.config.ts → server.port

# Find what's using the port:
# Windows:
netstat -ano | findstr :8080
# macOS / Linux:
lsof -i :8080
```

### "SurrealDB not found" / database connection failed

NexusLedger falls back to an in-memory database (`mem://`) if SurrealDB is not running. For persistent storage, install and start SurrealDB:

```bash
# Install SurrealDB: https://surrealdb.com/install
# Start it:
surreal start --user root --pass root --bind 127.0.0.1:8000 memory
# Or with file-based persistence:
surreal start --user root --pass root --bind 127.0.0.1:8000 rocksdb:data.db
```

The app connects to `ws://127.0.0.1:8000` by default and creates the `nexus` namespace and `accounting` database automatically.

### How do I reset a user's password?

NexusLedger does not expose a password-reset endpoint in the public API. To reset a password:

1. **Admin method**: Use the user-management API to verify the user exists:
   ```bash
   curl http://localhost:8080/api/v1/users \
     -H "Authorization: Bearer <admin-token>"
   ```
2. **Database method**: Connect to SurrealDB directly and update the password hash:
   ```bash
   surreal sql --conn ws://localhost:8000 --user root --pass root --ns nexus --db accounting
   # Then run:
   # UPDATE user:<username> SET password_hash = "<new-argon2-hash>";
   ```
   Generate a new argon2id hash using your preferred tool (e.g., `argon2` CLI or a small Rust script).
3. **Fresh start**: Delete the user record and re-register via `POST /api/auth/register`.

---

## License

Licensed under the **Apache License, Version 2.0**.

```
Copyright 2026 Richdale Accounting

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```
