# Installation Guide

NexusLedger runs on Windows, macOS, and Linux. This guide covers every step from
installing prerequisites to verifying your installation is working.

---

## Table of Contents

1. [Prerequisites](#1-prerequisites)
2. [Windows Installation](#2-windows-installation)
3. [macOS Installation](#3-macos-installation)
4. [Linux Installation](#4-linux-installation)
5. [SurrealDB Setup](#5-surrealdb-setup)
6. [Configuration](#6-configuration)
7. [Verify Your Installation](#7-verify-your-installation)
8. [Docker Deployment (Optional)](#8-docker-deployment-optional)

---

## 1. Prerequisites

NexusLedger has three runtime dependencies. All are free and open source.

| Dependency | Minimum Version | Purpose |
|---|---|---|
| **Rust** | 1.75+ | Compiles the backend (nexus-core) and Tauri desktop shell |
| **Node.js** | 18+ | Builds and serves the React frontend |
| **SurrealDB** | 1.0+ | Database — can run in-memory for development (no install needed) |

You will also need **Git** to clone the repository.

### Quick Check

Open a terminal and run:

```bash
rustc --version    # Should print rustc 1.75.0 or higher
node --version     # Should print v18.x or higher
npm --version      # Should print 9.x or higher
git --version      # Should print git version 2.x
```

If any of these are missing, follow the platform-specific sections below.

---

## 2. Windows Installation

### 2.1 Install Rust

1. Go to [https://rustup.rs](https://rustup.rs).
2. Download `rustup-init.exe`.
3. Run the installer. Accept the default options (press Enter at each prompt).
4. Close and reopen your terminal (PowerShell or Command Prompt).
5. Verify: `rustc --version`

> **Note:** Rust on Windows also requires the MSVC build tools. If the installer
> reports they are missing, install **Visual Studio Build Tools 2022** from
> [Microsoft's download page](https://visualstudio.microsoft.com/visual-cpp-build-tools/),
> selecting the "Desktop development with C++" workload.

### 2.2 Install Node.js

1. Go to [https://nodejs.org](https://nodejs.org).
2. Download the **LTS** version (18.x or higher).
3. Run the installer with default options.
4. Verify: `node --version` and `npm --version`

### 2.3 Install SurrealDB (Optional)

SurrealDB is optional for development — NexusLedger can run in in-memory mode
(`mem://`) without a separate database server. For persistent data, install
SurrealDB:

1. Go to [https://surrealdb.com/install](https://surrealdb.com/install).
2. Download the Windows binary (`.exe`).
3. Add it to your `PATH` or place it in a known directory.
4. Verify: `surreal version`

Alternatively, run SurrealDB via Docker (see [Section 8](#8-docker-deployment-optional)).

### 2.4 Build NexusLedger

```powershell
# Clone the repository
git clone https://github.com/msiraga/Rich_AIM_Acct_INV.git
cd Rich_AIM_Acct_INV\RichdaleAccounting

# Build the backend
cargo build --release

# The binary will be at:
# .\target\release\nexus-core.exe
```

### 2.5 Start the Frontend (Desktop App)

```powershell
cd ..\nexus-ledger-tauri

# Install frontend dependencies
npm install

# Start the Tauri backend + Vite dev server
npm run dev
```

The frontend will be available at **http://localhost:3000**.

---

## 3. macOS Installation

### 3.1 Install Rust

Open Terminal and run:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Follow the prompts (press Enter for defaults). Then:

```bash
source "$HOME/.cargo/env"
rustc --version
```

> **Apple Silicon (M1/M2/M3):** Rust automatically targets the correct
> architecture. No special flags needed.

### 3.2 Install Node.js

**Option A — Official installer:**

1. Go to [https://nodejs.org](https://nodejs.org).
2. Download the LTS `.pkg` for macOS.
3. Run the installer.

**Option B — Homebrew (recommended):**

```bash
brew install node
```

Verify: `node --version` and `npm --version`

### 3.3 Install SurrealDB (Optional)

```bash
brew install surrealdb/tap/surreal
surreal version
```

### 3.4 Build NexusLedger

```bash
# Clone the repository
git clone https://github.com/msiraga/Rich_AIM_Acct_INV.git
cd Rich_AIM_Acct_INV/RichdaleAccounting

# Build the backend
cargo build --release

# The binary will be at:
# ./target/release/nexus-core
```

### 3.5 Start the Frontend (Desktop App)

```bash
cd ../nexus-ledger-tauri
npm install
npm run dev
```

The frontend will be available at **http://localhost:3000**.

---

## 4. Linux Installation

### 4.1 Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustc --version
```

> **Build tools required:** Install `build-essential` (Debian/Ubuntu) or
> `gcc make` (Fedora/RHEL) before building:
> ```bash
> # Debian/Ubuntu
> sudo apt install build-essential pkg-config libssl-dev
>
> # Fedora/RHEL
> sudo dnf install gcc make pkg-config openssl-devel
> ```

### 4.2 Install Node.js

**Debian/Ubuntu:**

```bash
# Add NodeSource repository for Node 18+
curl -fsSL https://deb.nodesource.com/setup_18.x | sudo -E bash -
sudo apt install -y nodejs
```

**Fedora/RHEL:**

```bash
sudo dnf install nodejs
```

**Arch Linux:**

```bash
sudo pacman -S nodejs npm
```

Verify: `node --version` and `npm --version`

### 4.3 Install SurrealDB (Optional)

```bash
# Download the latest binary
curl -sSf https://install.surrealdb.com | sh

# Or via Docker
docker run -d --name surrealdb -p 8000:8000 surrealdb/surrealdb:latest start --user root --pass root
```

Verify: `surreal version`

### 4.4 Build NexusLedger

```bash
git clone https://github.com/msiraga/Rich_AIM_Acct_INV.git
cd Rich_AIM_Acct_INV/RichdaleAccounting
cargo build --release
```

### 4.5 Start the Frontend

```bash
cd ../nexus-ledger-tauri
npm install
npm run dev
```

The frontend will be available at **http://localhost:3000**.

---

## 5. SurrealDB Setup

SurrealDB is the database that powers NexusLedger. You have two options:

### Option A: In-Memory Mode (Development — No Install)

No SurrealDB server needed. Edit `config/server.toml`:

```toml
[database]
url = "mem://"
```

Data is stored in RAM and lost when the server stops. Perfect for trying things
out, but **not suitable for production**.

### Option B: WebSocket Server (Production — Persistent)

Start SurrealDB as a server process:

```bash
# Start SurrealDB on port 8000 with authentication
surreal start --user root --pass root --bind 0.0.0.0:8000
```

Then configure NexusLedger to connect to it in `config/server.toml`:

```toml
[database]
url = "ws://localhost:8000"
ns = "nexus"
db = "accounting"
```

> **Security:** In production, use a strong password and restrict network access
> to the SurrealDB port. The `root`/`root` credentials above are for development
> only.

### Starting SurrealDB via Docker

If you prefer Docker for database management:

```bash
docker run -d \
  --name surrealdb \
  -p 8000:8000 \
  surrealdb/surrealdb:latest \
  start --user root --pass root --bind 0.0.0.0:8000
```

---

## 6. Configuration

NexusLedger is configured via `config/server.toml`. This file is read on startup.

### Key Settings

```toml
[server]
port = 8080              # API server port (Tauri backend uses 4000)
host = "0.0.0.0"         # Bind address
workers = 4              # Tokio worker threads

[database]
url = "ws://localhost:8000"  # or "mem://" for in-memory
ns = "nexus"                  # SurrealDB namespace
db = "accounting"             # SurrealDB database name

[security]
secret_key = "change-this-in-production-to-a-strong-secret"
jwt_expiry = 86400       # JWT token lifetime in seconds (24 hours)
password_min_length = 8
max_login_attempts = 5
lockout_duration = 900   # 15 minutes

[features]
enable_ai = true              # Enable AI agent features
enable_multi_user = true      # Multi-user mode
enable_audit_log = true       # Audit trail
enable_bank_feeds = false     # Bank feeds (coming soon)
enable_payroll = true
enable_reporting = true
```

### Environment Variables

Environment variables override settings in `server.toml`. This is useful for
production deployments and Docker containers.

| Variable | Default | Description |
|---|---|---|
| `RUST_LOG` | `info` | Log level (`debug`, `info`, `warn`, `error`) |
| `SURREALDB_URL` | `ws://localhost:8000` | SurrealDB connection URL |
| `SURREALDB_NS` | `nexus` | SurrealDB namespace |
| `SURREALDB_DB` | `accounting` | SurrealDB database name |
| `JWT_SECRET` | *(must be set)* | Cryptographic secret for JWT signing (min 32 bytes) |
| `OLLAMA_URL` | `http://localhost:11434` | Ollama AI service URL |

> **Critical:** The `JWT_SECRET` environment variable must be set to a
> cryptographically random string of at least 32 bytes. The server will refuse
> to start if it detects the default placeholder. Generate one with:
> ```bash
> openssl rand -base64 32
> ```

---

## 7. Verify Your Installation

After building and starting NexusLedger, verify each component is working:

### 7.1 Check the API Server

```bash
# Health check
curl http://localhost:8080/api/health

# Expected response:
# {"status":"healthy", ...}
```

If you are running via the Tauri backend (desktop app), the API is on port 4000:

```bash
curl http://localhost:4000/health
```

### 7.2 Check the Frontend

Open your browser and navigate to:

- **http://localhost:3000** — Vite dev server (development mode)
- **Tauri desktop window** — if running via `npm run tauri dev`

You should see the login page. Register a new account to get started.

### 7.3 Check the Database

If using SurrealDB in WebSocket mode, verify it is accepting connections:

```bash
# Check SurrealDB health
curl http://localhost:8000/health
```

### 7.4 Check Agent Status

```bash
# List all agents
curl http://localhost:8080/api/v1/agents \
  -H "Authorization: Bearer <your-jwt-token>"
```

You should see 9 agents (Ledger, Reconciliation, Tax, Payroll, Invoice,
Receipt, Reporting, Audit, Document), each with an `Idle` status.

---

## 8. Docker Deployment (Optional)

For containerized deployment, NexusLedger includes Docker support.

### Docker Compose (Recommended)

```bash
cd RichdaleAccounting

# Start NexusLedger + SurrealDB together
docker-compose up -d

# View logs
docker-compose logs -f nexusledger

# Stop all services
docker-compose down
```

### Manual Docker Build

```bash
# Build the image
docker build -t nexusledger .

# Create data directories
mkdir -p data/{invoices,receipts,documents,statements,logs,exports}

# Run the container
docker run -d \
  --name nexusledger \
  -p 8080:8080 \
  -v $(pwd)/data:/usr/src/nexusledger/data \
  -v $(pwd)/config:/usr/src/nexusledger/config \
  -e JWT_SECRET=$(openssl rand -base64 32) \
  nexusledger

# View logs
docker logs -f nexusledger
```

---

## Next Steps

- [Quick Start Guide](quick-start.md) — Create your first transaction
- [Reports Guide](reports-guide.md) — Generate financial statements
- [FAQ](faq.md) — Common questions
- [Troubleshooting](troubleshooting.md) — Fix common issues
