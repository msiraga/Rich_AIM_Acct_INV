# Installation Guide

This guide covers installing NexusLedger on Windows, macOS, and from source.

---

## Table of Contents

1. [Windows](#windows)
2. [macOS](#macos)
3. [Development Setup (from source)](#development-setup-from-source)
4. [Post-Install: Set JWT_SECRET](#post-install-set-jwt_secret)
5. [SurrealDB (optional but recommended)](#surrealdb-optional-but-recommended)
6. [Verify Installation](#verify-installation)

---

## Windows

### Option A: MSI Installer (Recommended)

1. Download the latest `NexusLedger_x.x.x_x64.msi` from the
   [GitHub Releases](https://github.com/msiraga/Rich_AIM_Acct_INV/releases) page.
2. Double-click the `.msi` file to launch the installer.
3. Follow the wizard — accept the license agreement and choose the install
   directory (default: `C:\Program Files\NexusLedger`).
4. Click **Install** and wait for completion.
5. The installer creates a Start Menu shortcut and desktop icon.

### Option B: Portable EXE

1. Download `NexusLedger.exe` from Releases.
2. Place it in any directory (e.g., `C:\Users\<you>\NexusLedger\`).
3. Create a shortcut if desired.

### Set JWT_SECRET on Windows

NexusLedger requires a `JWT_SECRET` environment variable before it will start.
Set it before launching the app:

**PowerShell (current session):**

```powershell
$env:JWT_SECRET = "your-secret-key-at-least-32-bytes-long!"
```

**PowerShell (permanent, all sessions):**

```powershell
[System.Environment]::SetEnvironmentVariable("JWT_SECRET", "your-secret-key-at-least-32-bytes-long!", "User")
```

Restart your terminal after setting a permanent variable.

### Launch

Double-click the NexusLedger desktop icon, or run from PowerShell:

```powershell
& "C:\Program Files\NexusLedger\NexusLedger.exe"
```

---

## macOS

### Option A: DMG Installer (Recommended)

1. Download the latest `NexusLedger_x.x.x_aarch64.dmg` (Apple Silicon) or
   `NexusLedger_x.x.x_x64.dmg` (Intel) from
   [GitHub Releases](https://github.com/msiraga/Rich_AIM_Acct_INV/releases).
2. Double-click the `.dmg` file to mount it.
3. Drag the **NexusLedger.app** icon into the **Applications** folder.
4. Eject the DMG.

### Option B: Homebrew Cask (if available)

```bash
brew install --cask nexusledger
```

### Set JWT_SECRET on macOS

Add the environment variable to your shell profile:

```bash
# For zsh (default on macOS):
echo 'export JWT_SECRET="your-secret-key-at-least-32-bytes-long!"' >> ~/.zshrc
source ~/.zshrc
```

### Launch

Open **Spotlight** (Cmd+Space), type "NexusLedger", and press Enter. Or:

```bash
open /Applications/NexusLedger.app
```

> **Note:** On first launch, macOS may show a security warning because the app
> is not code-signed with an Apple Developer ID. Right-click the app and select
> **Open** to bypass the warning. You only need to do this once.

---

## Development Setup (from source)

For contributors and users who want to build from source.

### Prerequisites

| Tool | Minimum Version | Check |
|---|---|---|
| **Rust** | 1.70+ | `rustc --version` |
| **Node.js** | 18+ | `node --version` |
| **npm** | 9+ | `npm --version` |
| **Tauri CLI** | 2.x | `cargo tauri --version` |
| **Git** | any | `git --version` |

Install Rust via [rustup](https://rustup.rs/):

```bash
# macOS / Linux:
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# Windows: download and run rustup-init.exe from https://rustup.rs
```

Install the Tauri CLI:

```bash
cargo install tauri-cli --version "^2.0"
```

> **Windows only:** Tauri requires the
> [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/)
> (pre-installed on Windows 10/11) and MSVC build tools (install via Visual
> Studio Build Tools with the "Desktop development with C++" workload).

> **macOS only:** Tauri requires Xcode Command Line Tools:
> ```bash
> xcode-select --install
> ```

### Steps

```bash
# 1. Clone the repository
git clone https://github.com/msiraga/Rich_AIM_Acct_INV.git
cd Rich_AIM_Acct_INV

# 2. Build the Rust core (nexus-core library)
cd RichdaleAccounting
cargo build

# 3. Run tests to verify everything compiles and passes
cargo test

# 4. Install frontend dependencies
cd ../nexus-ledger-tauri
npm install

# 5. Set JWT_SECRET (required — server refuses to start without it)
export JWT_SECRET="dev-secret-key-change-in-production-32b!"

# 6. Launch the desktop app in dev mode (starts API + Vite + Tauri window)
cargo tauri dev
```

### Build a production bundle

```bash
cd nexus-ledger-tauri
cargo tauri build
```

Output artifacts are placed in `nexus-ledger-tauri/src-tauri/target/release/bundle/`:

| Platform | Output |
|---|---|
| Windows | `msi/NexusLedger_x.x.x_x64.msi` and `nsis/NexusLedger_x.x.x_x64-setup.exe` |
| macOS | `dmg/NexusLedger_x.x.x_aarch64.dmg` and `.app` bundle |
| Linux | `deb/NexusLedger_x.x.x_amd64.deb` and `AppImage/` |

---

## Post-Install: Set JWT_SECRET

NexusLedger refuses to start if the `JWT_SECRET` environment variable is not
set or contains the default placeholder value. The secret is used to sign and
verify JWT authentication tokens (HS256).

Generate a secure secret:

```bash
openssl rand -base64 32
# Example output: K7x9mP2vQ4rT8wY3aB6cD1eF5gH0jN4lS7uZ2xI9oM=
```

Set it as an environment variable before launching NexusLedger. See the
[Windows](#set-jwt_secret-on-windows) and [macOS](#set-jwt_secret-on-macos)
sections above for platform-specific instructions.

---

## SurrealDB (optional but recommended)

NexusLedger falls back to an in-memory database if SurrealDB is not running.
For persistent storage across restarts, install SurrealDB.

### Install SurrealDB

```bash
# macOS:
brew install surrealdb/tap/surreal

# Linux:
curl -sSf https://install.surrealdb.com | sh

# Windows:
# Download from https://surrealdb.com/install
```

### Start SurrealDB

```bash
# In-memory (data lost on restart):
surreal start --user root --pass root --bind 127.0.0.1:8000 memory

# File-based persistence (rocksdb):
surreal start --user root --pass root --bind 127.0.0.1:8000 rocksdb:data.db

# WebSocket protocol (required for NexusLedger WS auth):
surreal start --user root --pass root --bind 127.0.0.1:8000 --protocol ws rocksdb:data.db
```

NexusLedger connects to `ws://127.0.0.1:8000` by default and creates the
`nexus` namespace and `accounting` database automatically on first run.

---

## Verify Installation

After launching NexusLedger (desktop app or `cargo tauri dev`), verify the API
server is responding:

```bash
# Liveness check (always 200 if the process is alive):
curl http://localhost:8080/health
# Expected: {"status":"ok","uptime_seconds":5,"timestamp":"2026-07-01T..."}

# Readiness check (200 if DB connected + agents registered):
curl http://localhost:8080/ready
# Expected: {"status":"ready","db":"connected","agents":9,...}

# Prometheus metrics (requires MONITOR_ENABLE_PROMETHEUS=true):
MONITOR_ENABLE_PROMETHEUS=true curl http://localhost:8080/metrics
# Expected: # HELP nexus_agents_total ...
```

If you used the Tauri dev server, the desktop window should open automatically
and display the login/register screen.

---

## Next Steps

- [Quick Start Guide](quick-start.md) — Create your first transaction
- [API Reference](../api/reference.md) — Every endpoint documented
- [Developer Setup](../developer/setup.md) — Contributing to NexusLedger
