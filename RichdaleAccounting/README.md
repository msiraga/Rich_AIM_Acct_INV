# NexusLedger — Agentic Accounting Platform

> AI-powered, autonomous-agent-based accounting for small businesses.
> Built with Rust (Tauri + Axum), React, and SurrealDB.

**Author:** Mounir Siraji <mounir@richdaleai.com>
**Organization:** RichdaleAI
**License:** Apache-2.0
**Platforms:** Windows, macOS, Linux

[![Build Status](https://img.shields.io/badge/build-passing-brightgreen)](https://github.com/msiraga/Rich_AIM_Acct_INV)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange)](https://rustup.rs)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

---

## Screenshots

<!-- Add screenshots here after first build -->

*Dashboard | Journal Entry | Invoice | Reports*

> Screenshots will be added after the first production build. The desktop app
> renders a dark-themed dashboard with account balances, recent transactions,
> invoice management, and a chat sidebar for AI agent interaction.

---

## Features

- **9 Autonomous Agents** — Ledger, Reconciliation, Tax, Payroll, Invoice,
  Receipt, Reporting, Audit, Document. Each agent handles a specific
  accounting domain independently and collaboratively.
- **Double-Entry Accounting** — Full chart of accounts (20 default accounts),
  journal entries with balance validation, trial balance, balance sheet, and
  income statement generation.
- **Multi-Currency** — Real-time exchange rate conversion with support for
  160+ currencies. Set custom rates or use the conversion API.
- **Budget Tracking** — Create period budgets (monthly/quarterly) for any
  account. Generate variance reports comparing actual vs. budgeted amounts.
- **Fixed Assets** — Track fixed assets with straight-line and
  double-declining balance depreciation methods. Compute depreciation per
  period automatically.
- **AP/AR Workflows** — Vendor bill management with payment tracking,
  customer invoice creation, and aging reports (current, 1-30, 31-60, 61-90,
  90+ days).
- **CSV/OFX Import/Export** — Import bank statements via CSV. Export
  transaction data to CSV (for spreadsheets) or OFX (for personal finance
  software).
- **Role-Based Access** — 5-tier RBAC system: Guest, Viewer, User, Manager,
  Admin. JWT-based authentication with token refresh.
- **Desktop App** — Tauri cross-platform desktop application (Windows + macOS)
  with a React frontend. Includes a chat sidebar for natural-language
  interaction with AI agents.
- **Audit Trail** — The AuditAgent maintains a tamper-evident log of all
  financial operations, detects anomalies, and checks for fraud patterns.
- **Offline Capable** — Core accounting works without internet. AI features
  use local models (Ollama) — no cloud dependency required.

---

## Quick Start

### Prerequisites

- **Rust** 1.75+ ([install via rustup](https://rustup.rs))
- **Node.js** 18+ and npm ([download](https://nodejs.org))
- **SurrealDB** 1.0+ (optional — in-memory mode works for development)

### 5-Step Setup

**1. Clone the repository**

```bash
git clone https://github.com/msiraga/Rich_AIM_Acct_INV.git
cd Rich_AIM_Acct_INV/RichdaleAccounting
```

**2. Build the backend**

```bash
cargo build --release
```

**3. Start SurrealDB** (optional — skip to use in-memory mode)

```bash
surreal start --user root --pass root --bind 0.0.0.0:8000
```

Or edit `config/server.toml` to use in-memory mode:

```toml
[database]
url = "mem://"
```

**4. Set the JWT secret and start the API server**

```bash
# Linux/macOS
export JWT_SECRET="$(openssl rand -base64 32)"
cargo run --release --bin nexus-core

# Windows (PowerShell)
$env:JWT_SECRET = "dev-secret-key-change-in-production-32b!"
cargo run --release --bin nexus-core
```

The API server starts on **http://localhost:8080**.

**5. Start the frontend (desktop app)**

```bash
cd ../nexus-ledger-tauri
npm install
npm run dev
```

The Vite dev server starts on **http://localhost:3000**. The Tauri backend
API runs on **port 4000**.

**6. Register and create your first transaction**

Open **http://localhost:3000** in your browser, click **Register**, create an
account, and follow the [Quick Start Guide](docs/user/quick-start.md) to
record your first transaction.

> **Detailed walkthrough:** See [docs/user/quick-start.md](docs/user/quick-start.md)
> for a complete step-by-step guide including API curl commands.

---

## Architecture

NexusLedger uses a multi-agent architecture where each accounting function is
handled by a specialized AI agent. The agents are coordinated by an
orchestrator and communicate through a task queue.

```
┌──────────────────────────────────────────────────────────────┐
│                     NEXUSLEDGER SYSTEM                         │
│                                                               │
│  ┌─────────────────┐     ┌──────────────────────────────┐    │
│  │  Desktop Client  │     │       NexusLedger Core        │    │
│  │  (Tauri + React) │────▶│                               │    │
│  │                  │     │  ┌────────────────────────┐  │    │
│  │  - Dashboard     │     │  │  AgentOrchestrator      │  │    │
│  │  - Accounts      │     │  │  Task Queue / In-Prog.  │  │    │
│  │  - Invoices      │     │  │  Completed / Failed     │  │    │
│  │  - Reports       │     │  └───────────┬────────────┘  │    │
│  │  - Chat Sidebar  │     │              │                │    │
│  └─────────────────┘     │  ┌───────────┴────────────┐  │    │
│                          │  │  9 Autonomous Agents     │  │    │
│  ┌─────────────────┐     │  │  Ledger | Reconciliation │  │    │
│  │  API Server      │────▶│  │  Tax | Payroll | Invoice│  │    │
│  │  (Axum, port 8080)│    │  │  Receipt | Reporting    │  │    │
│  │  REST + WebSocket │     │  │  Audit | Document      │  │    │
│  └─────────────────┘     │  └────────────────────────┘  │    │
│                          │  ┌────────────────────────┐  │    │
│                          │  │  Accounting Engine      │  │    │
│                          │  │  Double-entry ledger    │  │    │
│                          │  │  Tax / Payroll / Recon. │  │    │
│                          │  └───────────┬────────────┘  │    │
│                          │  ┌───────────┴────────────┐  │    │
│                          │  │  SurrealDB (doc-graph)  │  │    │
│                          │  └────────────────────────┘  │    │
│                          └───────────────────────────────┘    │
└──────────────────────────────────────────────────────────────┘
```

**Key design decisions:**

- **Rust** for type safety, performance, and zero-cost abstractions
- **Tauri** for a lightweight cross-platform desktop shell (no Electron
  bloat)
- **Axum** for the REST API with middleware (auth, CORS, request ID, error
  mapping)
- **SurrealDB** for a document-graph database that supports both in-memory
  and WebSocket modes
- **React + TypeScript** for the frontend with strict mode and full type
  safety
- **JWT** authentication with access/refresh token flow

For the full architecture document, see
[docs/02-architecture.md](docs/02-architecture.md).

### The 9 Agents

| Agent | Responsibility |
|---|---|
| **LedgerAgent** | Double-entry transaction recording, chart of accounts management |
| **ReconciliationAgent** | Bank statement matching, discrepancy detection |
| **TaxAgent** | Tax calculations (US Federal/State), filing deadline tracking |
| **PayrollAgent** | Payroll processing with tax withholding and employer contributions |
| **InvoiceAgent** | Customer invoice creation, billing, payment tracking |
| **ReceiptAgent** | Receipt processing, expense categorization |
| **ReportingAgent** | Balance sheet, P&L, cash flow, trial balance generation |
| **AuditAgent** | Audit trail maintenance, anomaly detection, fraud checks |
| **DocumentAgent** | Document storage, retrieval, OCR processing |

See [docs/03-agents.md](docs/03-agents.md) for detailed agent documentation.

---

## Documentation

### User Documentation

- [Installation Guide](docs/user/installation.md) — Windows, macOS, and Linux
  setup instructions
- [Quick Start Guide](docs/user/quick-start.md) — First transaction walkthrough
  with API and UI instructions
- [Reports Guide](docs/user/reports-guide.md) — Financial reports explained
  (balance sheet, P&L, cash flow, AR aging, budget variance, AP outstanding)
- [Troubleshooting](docs/user/troubleshooting.md) — Common errors and solutions
- [FAQ](docs/user/faq.md) — Frequently asked questions

### Developer Documentation

- [Architecture Overview](docs/02-architecture.md) — System design and module
  dependency graph
- [Agent System](docs/03-agents.md) — Agent types, trait, lifecycle, and
  orchestrator
- [Accounting Engine](docs/04-accounting.md) — Double-entry system,
  reconciliation, tax, and payroll modules
- [API, Edge, and Monitor](docs/07-api-edge-monitor.md) — REST API design,
  offline edge mode, and system monitoring
- [Frontend](docs/08-frontend.md) — Tauri + React architecture
- [Project Strategy](Phases/00-strategy.md) — Development methodology and phase
  plan

---

## API Reference

**Base URL:** `http://localhost:8080` (or `http://localhost:4000` when running
via the Tauri backend)

All endpoints except `/health`, `/api/auth/register`, `/api/auth/login`, and
`/api/auth/refresh` require a JWT bearer token:

```
Authorization: Bearer <access_token>
```

### Authentication

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/api/auth/register` | Register a new user (first user becomes Admin) |
| POST | `/api/auth/login` | Log in and receive access + refresh tokens |
| POST | `/api/auth/refresh` | Refresh an expired access token |

### Accounts & Transactions

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/v1/accounts` | List all accounts in the chart of accounts |
| GET | `/api/v1/accounts/:id` | Get details for a specific account |
| GET | `/api/v1/transactions` | List all transactions |
| POST | `/api/v1/transactions` | Create a new double-entry transaction |
| GET | `/api/v1/transactions/:id` | Get details for a specific transaction |

### Invoices

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/v1/invoices` | List all invoices |
| POST | `/api/v1/invoices` | Create a new customer invoice |

### Reports

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/v1/reports/trial_balance` | Trial balance (all account balances) |
| GET | `/api/v1/reports/balance_sheet` | Balance sheet (Assets = Liabilities + Equity) |
| GET | `/api/v1/reports/income_statement` | Income statement (Revenue - Expenses) |
| GET | `/api/v1/reports/cash-flow` | Cash flow statement |
| GET | `/api/v1/reports/ar-aging` | Accounts receivable aging report |

### Accounts Payable

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/v1/ap/bills` | List all vendor bills |
| POST | `/api/v1/ap/bills` | Create a new vendor bill |
| POST | `/api/v1/ap/bills/:id/pay` | Record a payment against a bill |
| GET | `/api/v1/ap/outstanding` | Outstanding AP report |

### Budgets & Fixed Assets

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/api/v1/budgets` | Create a period budget |
| GET | `/api/v1/budgets/variance` | Budget vs. actual variance report |
| GET | `/api/v1/assets` | List fixed assets |
| POST | `/api/v1/assets` | Register a new fixed asset |
| POST | `/api/v1/assets/depreciation` | Compute depreciation for an asset |

### Multi-Currency

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/v1/exchange-rates` | List current exchange rates |
| POST | `/api/v1/exchange-rates` | Set an exchange rate |
| POST | `/api/v1/convert` | Convert an amount between currencies |

### Import / Export

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/api/v1/import/csv` | Import transactions from a CSV file |
| GET | `/api/v1/export/csv` | Export transactions to CSV |
| GET | `/api/v1/export/ofx` | Export transactions to OFX format |

### Agents & Tasks

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/v1/agents` | List all 9 agents with their status |
| POST | `/api/v1/tasks` | Submit a task to an agent |
| GET | `/api/v1/tasks/queue` | View the current task queue |
| GET | `/api/v1/status` | System status (agent counts, health score) |

### User Management (Admin only)

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/v1/users` | List all registered users |
| POST | `/api/v1/users/:id/role` | Change a user's role |

### Real-Time

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/ws/chat` | WebSocket endpoint for chat with AI agents |

### Health

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/health` | Liveness check (200 if process is alive) |

### Response Format

All API responses follow a standard envelope:

```json
{
  "success": true,
  "data": { ... },
  "error": null,
  "metadata": {
    "request_id": "uuid",
    "timestamp": "2026-07-01T00:00:00Z",
    "response_time_ms": 12,
    "api_version": "v1"
  }
}
```

On error, `success` is `false`, `data` is `null`, and `error` contains the
error message.

---

## Development

### Running Tests

```bash
# Run all tests (unit + integration)
cargo test --all

# Run tests with output
cargo test --all -- --nocapture

# Run a specific test
cargo test test_create_transaction -- --nocapture
```

### Linting

```bash
# Run clippy (warnings as errors)
cargo clippy -- -D warnings

# Check formatting
cargo fmt --check

# Auto-format
cargo fmt
```

### Frontend Development

```bash
cd nexus-ledger-tauri

# Start dev server (port 3000)
npm run dev

# Build for production
npm run build

# Type-check
npx tsc --noEmit
```

### Configuration

The main configuration file is `config/server.toml`. Key sections:

```toml
[server]
port = 8080
host = "0.0.0.0"

[database]
url = "ws://localhost:8000"   # or "mem://" for in-memory
ns = "nexus"
db = "accounting"

[security]
jwt_expiry = 86400            # 24 hours
password_min_length = 8
max_login_attempts = 5

[features]
enable_ai = true
enable_multi_user = true
enable_audit_log = true
```

Environment variables override `server.toml` values. See the
[Installation Guide](docs/user/installation.md#6-configuration) for the full
list.

### Project Structure

```
Rich_AIM_Acct_INV/
├── RichdaleAccounting/          # Backend (Rust)
│   ├── nexus-core/
│   │   └── src/
│   │       ├── api/             # REST API (Axum)
│   │       ├── agents/          # 9 agent implementations + orchestrator
│   │       ├── accounting/      # Ledger, reconciliation, tax, payroll
│   │       ├── database/        # SurrealDB repositories + migrations
│   │       ├── ai/              # Ollama integration
│   │       ├── audit/           # Audit agent + repository
│   │       ├── monitor/         # System metrics + alerts
│   │       └── utils/           # Date, file, validation utilities
│   ├── config/                  # server.toml configuration
│   ├── data/                    # Runtime data (logs, documents, exports)
│   ├── docs/                    # Architecture + developer documentation
│   │   └── user/               # User-facing documentation
│   ├── Phases/                  # Development phase plans + tracker
│   ├── docker/                  # Docker build files
│   ├── Cargo.toml               # Workspace configuration
│   ├── Dockerfile               # Docker build
│   ├── docker-compose.yml       # Docker Compose (NexusLedger + SurrealDB)
│   ├── build.sh                 # Build script
│   └── run.sh                   # Run script
└── nexus-ledger-tauri/          # Frontend (Tauri + React)
    ├── src/
    │   ├── pages/               # Dashboard, Accounts, Transactions, etc.
    │   ├── components/          # Layout, ChatSidebar, ErrorBoundary
    │   ├── lib/                 # API client (api.ts)
    │   └── contexts/            # React contexts
    ├── backend/                 # Tauri backend (imports nexus-core)
    ├── src-tauri/               # Tauri configuration + icons
    ├── package.json             # Frontend dependencies
    └── vite.config.ts           # Vite dev server config
```

---

## Deployment

### Docker Compose (Recommended for Production)

```bash
cd RichdaleAccounting
docker-compose up -d

# View logs
docker-compose logs -f nexusledger

# Stop
docker-compose down
```

This starts NexusLedger and SurrealDB together. See the
[Installation Guide](docs/user/installation.md#8-docker-deployment-optional)
for details.

### Manual Docker

```bash
docker build -t nexusledger .
docker run -d \
  --name nexusledger \
  -p 8080:8080 \
  -v $(pwd)/data:/usr/src/nexusledger/data \
  -v $(pwd)/config:/usr/src/nexusledger/config \
  -e JWT_SECRET="$(openssl rand -base64 32)" \
  nexusledger
```

### Systemd (Linux Server)

```ini
[Unit]
Description=NexusLedger Accounting Platform
After=network.target

[Service]
ExecStart=/path/to/nexus-core
WorkingDirectory=/path/to/RichdaleAccounting
Environment=RUST_LOG=info
Environment=SURREALDB_URL=ws://localhost:8000
Environment=JWT_SECRET=your-secret-here
Restart=always

[Install]
WantedBy=multi-user.target
```

---

## License

Copyright 2026 RichdaleAI

Licensed under the **Apache License, Version 2.0** (the "License"); you may not
use this file except in compliance with the License. You may obtain a copy of
the License at:

```
http://www.apache.org/licenses/LICENSE-2.0
```

Unless required by applicable law or agreed to in writing, software distributed
under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
CONDITIONS OF ANY KIND, either express or implied. See the License for the
specific language governing permissions and limitations under the License.

---

## FAQ

See [docs/user/faq.md](docs/user/faq.md) for answers to common questions,
including:

- How do I add a new account?
- How do I import bank statements?
- How do I set up multi-currency?
- How do I generate a P&L for a specific period?
- What are the user roles?
- How do I backup my data?
- Can I use this offline?

---

## Support

- **Email**: mounir@richdaleai.com
- **Website**: https://richdaleai.com
- **GitHub**: https://github.com/msiraga/Rich_AIM_Acct_INV
- **Issues**: https://github.com/msiraga/Rich_AIM_Acct_INV/issues

---

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

Please ensure `cargo test --all` and `cargo clippy -- -D warnings` pass before
submitting a PR.
