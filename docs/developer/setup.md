# Developer Setup Guide

This guide is for developers who want to contribute to NexusLedger or build
from source. It covers prerequisites, build commands, testing, project
structure, and contribution guidelines.

---

## Table of Contents

1. [Prerequisites](#prerequisites)
2. [Clone & Build](#clone--build)
3. [Test Commands](#test-commands)
4. [Project Structure](#project-structure)
5. [How to Add a New Agent](#how-to-add-a-new-agent)
6. [How to Add a New API Endpoint](#how-to-add-a-new-api-endpoint)
7. [Environment Variables](#environment-variables)
8. [Contribution Guidelines](#contribution-guidelines)

---

## Prerequisites

| Tool | Minimum Version | Install |
|---|---|---|
| **Rust** | 1.70+ (2021 edition) | [rustup.rs](https://rustup.rs/) |
| **Node.js** | 18+ | [nodejs.org](https://nodejs.org/) |
| **npm** | 9+ | bundled with Node.js |
| **Tauri CLI** | 2.x | `cargo install tauri-cli --version "^2.0"` |
| **Git** | any | [git-scm.com](https://git-scm.com/) |
| **SurrealDB** | 1.x (optional) | [surrealdb.com/install](https://surrealdb.com/install) |

### Platform-Specific Requirements

**Windows:**
- [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) (pre-installed on Windows 10/11)
- MSVC Build Tools (install via Visual Studio Installer → "Desktop development with C++" workload)

**macOS:**
- Xcode Command Line Tools: `xcode-select --install`

**Linux:**
- `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libappindicator3-dev`, `librsvg2-dev`, `libsoup-3.0-dev`
- Ubuntu/Debian: `sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev`

---

## Clone & Build

```bash
# Clone the repository
git clone https://github.com/msiraga/Rich_AIM_Acct_INV.git
cd Rich_AIM_Acct_INV
```

The repository has two main workspace directories:

| Directory | Description |
|---|---|
| `RichdaleAccounting/` | Rust workspace containing `nexus-core` (the core library + agents + API + AI + edge) |
| `nexus-ledger-tauri/` | Tauri 2 desktop app (React 18 frontend + Tauri shell that imports nexus-core) |

### Build the Rust core

```bash
cd RichdaleAccounting
cargo build
```

This compiles the `nexus-core` crate and all its modules (agents, accounting,
database, API, AI, edge, monitor).

### Build the Tauri desktop app

```bash
cd nexus-ledger-tauri
npm install
cargo tauri build    # production bundle
# or:
cargo tauri dev      # dev mode (hot-reload frontend + API)
```

### Run in development mode

```bash
# Terminal 1 — set required env var:
export JWT_SECRET="dev-secret-key-change-in-production-32b!"

# Terminal 2 — start the Tauri dev server:
cd nexus-ledger-tauri
cargo tauri dev
```

This starts:
- Vite dev server (frontend) on port 4000
- axum API server on port 8080
- Tauri desktop window

---

## Test Commands

### Rust tests

```bash
# Run all tests in the nexus-core crate:
cd RichdaleAccounting
cargo test

# Run a specific test module:
cargo test -- --test integration

# Run tests with output:
cargo test -- --nocapture

# Run only unit tests (skip integration):
cargo test --lib

# Run with verbose output:
cargo test -- --nocapture --test-threads=1
```

### Frontend type-checking

```bash
cd nexus-ledger-tauri
npx tsc --noEmit
```

### Security audit

```bash
cd RichdaleAccounting
cargo audit          # check dependencies for known vulnerabilities
cargo clippy -- -D warnings   # lint check, warnings as errors
```

### Benchmarks

```bash
cd RichdaleAccounting
cargo bench          # run criterion benchmarks (10K transaction throughput)
```

---

## Project Structure

```
Rich_AIM_Acct_INV/
├── README.md                          # Project overview, quick start, FAQ
├── docs/
│   ├── user/
│   │   ├── install.md                 # Installation guide
│   │   └── quick-start.md             # 10-minute walkthrough
│   ├── api/
│   │   └── reference.md              # Complete API reference
│   └── developer/
│       └── setup.md                   # This file
├── RichdaleAccounting/                # Rust workspace
│   ├── Cargo.toml                     # Workspace manifest + shared deps
│   ├── nexus-core/                    # Core library crate
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                 # Crate root, NexusLedger struct
│   │       ├── main.rs                # Binary entry point (API server)
│   │       ├── agents/                # 9 autonomous agents + orchestrator
│   │       │   ├── mod.rs             # Module declarations
│   │       │   ├── agent_types.rs     # AgentType enum, AgentConfig, Agent trait
│   │       │   ├── orchestrator.rs    # Task dispatch loop, agent registry
│   │       │   ├── task.rs            # Task struct, TaskType, TaskPriority
│   │       │   ├── config.rs          # Agent configuration
│   │       │   ├── memory.rs          # Agent memory manager
│   │       │   ├── status.rs          # Agent status, SystemStatus
│   │       │   ├── error.rs           # Agent error types
│   │       │   └── document.rs        # DocumentAgent implementation
│   │       ├── accounting/            # Accounting engine
│   │       │   ├── ledger.rs          # Double-entry ledger
│   │       │   ├── reconciliation.rs  # Bank reconciliation
│   │       │   ├── tax.rs             # Tax calculator
│   │       │   ├── payroll.rs         # Payroll processor
│   │       │   ├── reporting.rs       # P&L, balance sheet, cash flow, AR aging
│   │       │   ├── cashflow.rs        # Cash flow statement generator
│   │       │   ├── budget.rs          # Budget tracking + variance
│   │       │   └── assets.rs          # Fixed assets + depreciation
│   │       ├── api/                   # REST API (axum)
│   │       │   ├── mod.rs             # ApiServer, AppState, route handlers
│   │       │   ├── auth.rs            # JWT, argon2, RBAC extractors
│   │       │   ├── middleware.rs      # Rate limiting
│   │       │   └── routes/
│   │       │       └── health.rs     # /health, /ready, /metrics
│   │       ├── ai/                    # AI pipeline
│   │       │   ├── mod.rs             # Module registration
│   │       │   ├── ocr.rs             # Mistral OCR4 API integration
│   │       │   ├── pdf.rs             # PDF text extraction (pdf-extract)
│   │       │   ├── embeddings.rs      # Vector storage + similarity search
│   │       │   ├── analysis.rs        # Anomaly detection
│   │       │   └── classification.rs  # Smart account categorization
│   │       ├── edge/                  # Edge / sync layer
│   │       │   ├── mod.rs             # EdgeManager
│   │       │   ├── local_db.rs        # Embedded SQLite schema
│   │       │   ├── store.rs           # Local CRUD operations
│   │       │   ├── tracking.rs        # Change tracking (_dirty flags)
│   │       │   ├── sync.rs            # Push/pull sync engine
│   │       │   ├── conflict.rs        # Last-write-wins conflict resolution
│   │       │   ├── encryption.rs      # AES-256-GCM encryption
│   │       │   └── compression.rs     # lz4 compression
│   │       ├── database/              # Data layer (SurrealDB)
│   │       │   ├── mod.rs             # Database struct, connect/disconnect
│   │       │   ├── models.rs          # User, UserRole, Document, etc.
│   │       │   ├── financial.rs       # Account, Transaction, TransactionEntry
│   │       │   ├── user.rs            # SurrealUserRepository, password hashing
│   │       │   ├── audit.rs           # AuditRepository
│   │       │   └── document.rs        # DocumentRepository
│   │       ├── monitor/               # System monitor
│   │       │   └── mod.rs             # Metrics, alerts, health scoring
│   │       └── utils/                 # Utilities
│   │           ├── import.rs          # CSV import
│   │           ├── export.rs          # CSV/OFX export
│   │           ├── date_utils.rs      # Date helpers
│   │           ├── validation.rs      # Input validation
│   │           └── file_utils.rs      # File helpers
│   ├── tests/                         # Integration tests
│   ├── data/                          # Seed data, migrations
│   ├── config/                        # Configuration templates
│   ├── docker/                        # Docker files
│   └── Phases/                        # Phase tracking docs
│       └── TRACKER.md                 # Execution tracker
├── nexus-ledger-tauri/                # Tauri desktop app
│   ├── package.json                   # Frontend deps (React 18, react-router-dom)
│   ├── vite.config.ts                 # Vite config
│   ├── src/                           # React frontend
│   │   ├── App.tsx                    # Root component, routes
│   │   ├── main.tsx                   # Entry point
│   │   ├── index.css                  # Global styles
│   │   ├── pages/                     # Route pages
│   │   │   ├── DashboardPage.tsx
│   │   │   ├── AccountsPage.tsx
│   │   │   ├── TransactionsPage.tsx
│   │   │   ├── JournalEntryPage.tsx
│   │   │   ├── InvoicesPage.tsx
│   │   │   ├── DocumentsPage.tsx
│   │   │   ├── LoginPage.tsx
│   │   │   └── RegisterPage.tsx
│   │   ├── components/                # Shared components
│   │   │   └── SyncStatus.tsx
│   │   ├── contexts/                  # React contexts
│   │   └── lib/                       # API client, utilities
│   ├── src-tauri/                     # Tauri shell
│   │   ├── Cargo.toml                 # Tauri deps (tray-icon, updater)
│   │   └── src/                       # Tauri main.rs
│   └── backend/                       # Tauri backend binary
│       └── src/main.rs                # Imports nexus-core, starts API
└── docs/                             # Documentation (this directory)
```

---

## How to Add a New Agent

NexusLedger uses a trait-based agent system. To add a new agent:

### 1. Add the agent type to the enum

In `nexus-core/src/agents/agent_types.rs`, add a new variant to `AgentType`:

```rust
pub enum AgentType {
    // ... existing variants ...
    ForecastingAgent,  // your new agent
}
```

### 2. Implement the `Agent` trait

Create a new file (e.g., `nexus-core/src/agents/forecasting.rs`):

```rust
use async_trait::async_trait;
use crate::agents::agent_types::{Agent, AgentConfig, AgentStatus, AgentType};
use crate::agents::task::Task;

pub struct ForecastingAgent {
    config: AgentConfig,
    status: AgentStatus,
}

impl ForecastingAgent {
    pub fn new(config: AgentConfig) -> Self {
        Self { config, status: AgentStatus::Idle }
    }
}

#[async_trait]
impl Agent for ForecastingAgent {
    fn config(&self) -> &AgentConfig { &self.config }
    fn status(&self) -> AgentStatus { self.status.clone() }
    fn agent_type(&self) -> AgentType { AgentType::ForecastingAgent }

    async fn initialize(&mut self) -> Result<(), anyhow::Error> {
        self.status = AgentStatus::Idle;
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), anyhow::Error> {
        self.status = AgentStatus::ShuttingDown;
        Ok(())
    }

    async fn process_task(&self, task: Task) -> Result<Task, anyhow::Error> {
        // Implement your agent's logic here
        // Access the task payload, perform work, return the updated task
        Ok(task)
    }
}
```

### 3. Register the module

In `nexus-core/src/agents/mod.rs`, add:

```rust
pub mod forecasting;
```

### 4. Add the agent to the orchestrator

In the orchestrator initialization code (wherever `add_agent()` is called),
add:

```rust
orchestrator.add_agent(Arc::new(Mutex::new(
    ForecastingAgent::new(AgentConfig::new(
        AgentType::ForecastingAgent,
        "Forecasting Agent",
        "Generates cash flow forecasts and predictions"
    ))
))).await;
```

### 5. Add a task type (if needed)

In `nexus-core/src/agents/task.rs`, add a new `TaskType` variant and update the
dispatch logic in the orchestrator to route the new task type to your agent.

### 6. Write tests

Add unit tests in your agent file and an integration test in the `tests/`
directory.

---

## How to Add a New API Endpoint

### 1. Write the handler function

In `nexus-core/src/api/mod.rs`, add a new async handler function:

```rust
/// GET /api/v1/forecast
async fn forecast_handler(
    _guard: RequireViewer,   // or RequireUser for write operations
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let months = params.get("months")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(12);

    // Call into nexus-core to get the data
    let nexus = state.nexus.lock().await;
    // ... your logic ...

    Json(ApiResponse::success(serde_json::json!({
        "months": months,
        "forecast": [],
    }))).into_response()
}
```

### 2. Register the route

In the `ApiServer::start()` method, add the route to the `Router::new()` chain:

```rust
.route("/api/v1/forecast", get(forecast_handler))
```

For POST endpoints:

```rust
.route("/api/v1/forecast", get(forecast_handler).post(create_forecast_handler))
```

### 3. Choose the right auth guard

| Guard | Role Required | Use For |
|---|---|---|
| `RequireViewer` | Viewer | Read-only endpoints (GET) |
| `RequireUser` | User | Write endpoints (POST, PUT, DELETE) |
| `RequireManager` | Manager | Budget/asset management |
| `RequireAdmin` | Admin | User management, role changes |

Include the guard as the first parameter of your handler:

```rust
async fn my_handler(
    _guard: RequireUser,   // <-- this enforces RBAC
    State(state): State<AppState>,
    ...
)
```

### 4. Test the endpoint

Write an integration test in `RichdaleAccounting/tests/` that:
1. Starts the API server
2. Registers a user and obtains a token
3. Calls the new endpoint with the token
4. Asserts the response

### 5. Document the endpoint

Add the endpoint to `docs/api/reference.md` with method, path, auth requirement,
curl example, and request/response JSON.

---

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `JWT_SECRET` | (none — server refuses to start) | HS256 signing secret for JWT tokens. Must be 32+ bytes. |
| `API_HOST` | `127.0.0.1` | API server bind address |
| `API_PORT` | `8080` | API server port |
| `API_ENABLE_HTTPS` | `false` | Enable HTTPS |
| `API_SSL_CERT_PATH` | (none) | Path to SSL certificate |
| `API_SSL_KEY_PATH` | (none) | Path to SSL private key |
| `API_CORS_ORIGINS` | `*` | Comma-separated CORS allowed origins |
| `API_RATE_LIMIT` | `100` | Requests per minute per client |
| `API_TIMEOUT` | `30` | Request timeout in seconds |
| `MONITOR_ENABLE_PROMETHEUS` | `false` | Enable `/metrics` endpoint |

---

## Contribution Guidelines

### Branch naming

```
feature/<short-description>     # e.g., feature/csv-import-validation
fix/<short-description>         # e.g., fix/jwt-refresh-rotation
phase<N>/<task-id>-<desc>       # e.g., phase7/7.14-user-docs
```

### Commit messages

Use conventional commits:

```
feat: add multi-currency exchange rate endpoint
fix: resolve double-entry balance validation edge case
test: add integration test for AP bill payment
docs: add API reference for edge sync endpoints
refactor: reduce lock contention in orchestrator
```

### Before submitting a PR

1. **All tests pass:**
   ```bash
   cd RichdaleAccounting && cargo test
   ```

2. **No clippy warnings:**
   ```bash
   cargo clippy -- -D warnings
   ```

3. **No security vulnerabilities:**
   ```bash
   cargo audit
   ```

4. **Frontend compiles:**
   ```bash
   cd nexus-ledger-tauri && npx tsc --noEmit
   ```

5. **Documentation updated:** If you add or change an API endpoint, agent, or
   feature, update the relevant docs in `docs/`.

### Code style

- Follow standard Rust formatting (`cargo fmt`)
- Use `rust_decimal::Decimal` for all monetary values — never `f64` or `f32`
- All async functions use `async_trait` and `tokio` runtime
- Wrap shared state in `Arc<Mutex<T>>` (write-heavy) or `Arc<RwLock<T>>` (read-heavy)
- Use `thiserror` for library error types, `anyhow` for application-level errors
- Every public function should have a doc comment
- Unit tests live in the same file under `#[cfg(test)] mod tests`
- Integration tests live in `RichdaleAccounting/tests/`

### Architecture principles

- **No SQL injection:** All SurrealDB queries use parameterized bindings
- **No XSS:** Frontend uses React's built-in escaping; no `dangerouslySetInnerHTML`
- **No panics in hot paths:** Use `Result<T, E>` for fallible operations
- **AI degradation:** AI features must degrade gracefully when external services (Mistral API, GGUF models) are unavailable
- **Offline-first:** Core accounting works without network; edge sync is additive
- **Audit trail:** Every state-changing operation is logged via the AuditAgent

### Where to get help

- **Architecture docs:** `RichdaleAccounting/docs/02-architecture.md`
- **Agent design:** `RichdaleAccounting/docs/03-agents.md`
- **Phase tracker:** `RichdaleAccounting/Phases/TRACKER.md`
- **Phase 4 handoff:** `RichdaleAccounting/Phases/phase-4-handoff.md` (key file paths, breaking changes, API details)
- **GitHub Issues:** [https://github.com/msiraga/Rich_AIM_Acct_INV/issues](https://github.com/msiraga/Rich_AIM_Acct_INV/issues)
