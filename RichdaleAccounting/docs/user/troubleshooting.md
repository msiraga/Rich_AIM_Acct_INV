# Troubleshooting

This guide covers common errors and their solutions. If you encounter an issue
not listed here, check the [FAQ](faq.md) or contact support at
mounir@richdaleai.com.

---

## Table of Contents

1. [Installation & Build Issues](#1-installation--build-issues)
2. [Database Connection Issues](#2-database-connection-issues)
3. [Server & Port Issues](#3-server--port-issues)
4. [Authentication Issues](#4-authentication-issues)
5. [Transaction & Ledger Issues](#5-transaction--ledger-issues)
6. [Frontend Issues](#6-frontend-issues)
7. [AI Agent Issues](#7-ai-agent-issues)
8. [Debug Mode](#8-debug-mode)
9. [Reset the Database](#9-reset-the-database)
10. [Checking System Status](#10-checking-system-status)

---

## 1. Installation & Build Issues

### Error: "cargo: command not found"

**Cause:** Rust is not installed or not in your PATH.

**Solution:**

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# Verify
rustc --version
```

On Windows, download `rustup-init.exe` from [https://rustup.rs](https://rustup.rs)
and run it. Close and reopen your terminal afterward.

---

### Error: "error: linker 'link.exe' not found" (Windows)

**Cause:** The MSVC build tools are not installed. Rust on Windows requires the
Visual Studio C++ build tools.

**Solution:**

1. Download **Visual Studio Build Tools 2022** from
   [Microsoft's download page](https://visualstudio.microsoft.com/visual-cpp-build-tools/).
2. Run the installer and select the **"Desktop development with C++"** workload.
3. Restart your terminal.
4. Rebuild: `cargo build --release`

---

### Error: "error: failed to run custom build command for 'openssl-sys'"

**Cause:** OpenSSL development libraries are missing (Linux).

**Solution:**

```bash
# Debian/Ubuntu
sudo apt install libssl-dev pkg-config

# Fedora/RHEL
sudo dnf install openssl-devel pkg-config

# Arch Linux
sudo pacman -S openssl pkg-config
```

Then rebuild: `cargo build --release`

---

### Error: "npm: command not found"

**Cause:** Node.js is not installed.

**Solution:**

- **Windows/macOS**: Download the LTS installer from [https://nodejs.org](https://nodejs.org).
- **macOS (Homebrew)**: `brew install node`
- **Linux (Debian/Ubuntu)**:
  ```bash
  curl -fsSL https://deb.nodesource.com/setup_18.x | sudo -E bash -
  sudo apt install -y nodejs
  ```

---

### Error: Build fails with compilation errors

**Cause:** Your Rust version may be outdated, or the build cache is corrupt.

**Solution:**

```bash
# Update Rust to the latest stable
rustup update stable

# Clean the build cache
cargo clean

# Rebuild
cargo build --release
```

If errors persist, check that you are on the correct branch and have the latest
code:

```bash
git pull origin main
cargo build --release
```

---

### Error: "npm install" fails with dependency errors

**Cause:** Corrupted `node_modules` or lock file conflicts.

**Solution:**

```bash
cd nexus-ledger-tauri
rm -rf node_modules package-lock.json
npm install
```

On Windows:

```powershell
cd nexus-ledger-tauri
Remove-Item -Recurse -Force node_modules
Remove-Item -Force package-lock.json
npm install
```

---

## 2. Database Connection Issues

### Error: "SurrealDB connection failed" or "connection refused"

**Cause:** SurrealDB is not running, or the connection URL is wrong.

**Solution:**

**Step 1: Check if SurrealDB is running.**

```bash
# Check if the process is running
# Linux/macOS:
pgrep surreal

# Windows:
tasklist | findstr surreal
```

**Step 2: If not running, start it.**

```bash
surreal start --user root --pass root --bind 0.0.0.0:8000
```

Or via Docker:

```bash
docker run -d --name surrealdb -p 8000:8000 surrealdb/surrealdb:latest start --user root --pass root
```

**Step 3: If you don't need persistence, switch to in-memory mode.**

Edit `config/server.toml`:

```toml
[database]
url = "mem://"
```

This removes the SurrealDB dependency entirely. Data is stored in RAM and lost
on restart — fine for development and testing.

**Step 4: Verify the connection URL.**

Check that `config/server.toml` has the correct URL:

```toml
[database]
url = "ws://localhost:8000"   # Must match the port SurrealDB is running on
ns = "nexus"
db = "accounting"
```

---

### Error: "Database schema migration failed"

**Cause:** The database has a corrupt or incompatible schema from a previous
version.

**Solution:**

Reset the database (see [Section 9](#9-reset-the-database) below).

---

### Error: Data disappears after restart

**Cause:** You are using in-memory mode (`mem://`), which does not persist data.

**Solution:**

Switch to a persistent SurrealDB instance:

1. Start SurrealDB: `surreal start --user root --pass root`
2. Edit `config/server.toml`: set `url = "ws://localhost:8000"`
3. Restart NexusLedger.

---

## 3. Server & Port Issues

### Error: "Address already in use" or "port 8080 already in use"

**Cause:** Another process is using the port that NexusLedger wants to bind to.

**Solution:**

**Option A: Find and kill the process using the port.**

```bash
# Linux/macOS — find the process
lsof -i :8080

# Kill it
kill -9 <PID>
```

```powershell
# Windows — find the process
netstat -ano | findstr :8080

# Kill it (use the PID from the previous command)
taskkill /PID <PID> /F
```

**Option B: Change the port in the configuration.**

Edit `config/server.toml`:

```toml
[server]
port = 8081   # Use a different port
```

Then restart the server.

---

### Error: Frontend cannot connect to the API

**Cause:** The frontend is trying to reach the backend on a port that is not
running, or CORS is blocking the request.

**Solution:**

1. Verify the backend is running:
   ```bash
   curl http://localhost:4000/health
   # Should return {"status":"ok"}
   ```

2. Check the frontend's API configuration in
   `nexus-ledger-tauri/src/lib/api.ts`:
   ```typescript
   export const API_BASE = "http://localhost:4000";
   ```
   Make sure this matches the port your backend is running on.

3. If running the nexus-core API directly (not via Tauri backend), update the
   API_BASE to `http://localhost:8080`.

4. Check browser console (F12) for CORS errors. The backend should have CORS
   enabled for all origins in development mode.

---

### Error: "JWT secret is not configured" — server refuses to start

**Cause:** The `JWT_SECRET` environment variable is not set, and the server
detects the default placeholder in `config/server.toml`.

**Solution:**

Set the environment variable before starting the server:

```bash
# Linux/macOS
export JWT_SECRET="$(openssl rand -base64 32)"
cargo run --bin nexus-core

# Windows (PowerShell)
$env:JWT_SECRET = "dev-secret-key-change-in-production-32b!"
cargo run --bin nexus-core
```

For development, any 32+ character string works. For production, use a
cryptographically random value:

```bash
openssl rand -base64 32
```

---

## 4. Authentication Issues

### Error: "401 Unauthorized" on API calls

**Cause:** The JWT access token is missing, expired, or invalid.

**Solution:**

1. Check that you are including the Authorization header:
   ```bash
   -H "Authorization: Bearer <access_token>"
   ```

2. If the token has expired (default: 24 hours), refresh it:
   ```bash
   curl -X POST http://localhost:8080/api/auth/refresh \
     -H "Content-Type: application/json" \
     -d '{"refresh_token": "<your-refresh-token>"}'
   ```

3. If the refresh token is also expired, log in again:
   ```bash
   curl -X POST http://localhost:8080/api/auth/login \
     -H "Content-Type: application/json" \
     -d '{"email": "admin@example.com", "password": "yourpassword"}'
   ```

In the desktop UI, the app automatically refreshes expired tokens and redirects
to the login page if the refresh fails.

---

### Error: "403 Forbidden" on API calls

**Cause:** Your user role does not have permission for the requested action.

**Solution:**

NexusLedger has 5 roles with increasing permissions:

| Role | Can View | Can Create | Can Manage Users |
|---|---|---|---|
| Guest | No | No | No |
| Viewer | Yes | No | No |
| User | Yes | Yes (transactions, invoices) | No |
| Manager | Yes | Yes | No |
| Admin | Yes | Yes | Yes |

If you need higher permissions, ask an **Admin** to change your role:

```bash
# Admin only — change a user's role
curl -X POST http://localhost:8080/api/v1/users/<user-id>/role \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <admin-access-token>" \
  -d '{"role": "Manager"}'
```

---

### Error: "Account locked due to too many failed login attempts"

**Cause:** You have exceeded the maximum login attempts (default: 5). The
account is locked for 15 minutes.

**Solution:**

Wait 15 minutes for the lockout to expire, then try again. If you need
immediate access, an Admin can reset the lockout by restarting the server
(in-memory lockout state is cleared on restart).

---

## 5. Transaction & Ledger Issues

### Error: "Transaction does not balance" (debits != credits)

**Cause:** The sum of debit entries does not equal the sum of credit entries.

**Solution:**

Double-entry accounting requires that total debits = total credits. Review your
transaction entries:

```json
{
  "entries": [
    {"account_id": "...", "amount": "10000.00", "entry_type": "Debit"},
    {"account_id": "...", "amount": "10000.00", "entry_type": "Credit"}
  ]
}
```

- Sum all `Debit` amounts.
- Sum all `Credit` amounts.
- They must be exactly equal (to the cent).

Common mistakes:
- Transposed digits ($1,200 debit vs. $2,100 credit)
- Missing an entry (only one side recorded)
- Rounding errors (use exact decimal amounts, not floats)

---

### Error: "Account not found" or "Account is not active"

**Cause:** A transaction references an account ID that does not exist or has
been deactivated.

**Solution:**

1. List all accounts and find the correct ID:
   ```bash
   curl http://localhost:8080/api/v1/accounts \
     -H "Authorization: Bearer <access_token>"
   ```

2. Verify the account's `is_active` field is `true`.

3. If the account was deactivated, you may need to reactivate it (Admin only).

---

### Error: Balance sheet does not balance (Assets != Liabilities + Equity)

**Cause:** A transaction was posted with unbalanced entries, or a bug in the
ledger.

**Solution:**

1. Run a trial balance to check if the books are in balance:
   ```bash
   curl http://localhost:8080/api/v1/reports/trial_balance \
     -H "Authorization: Bearer <access_token>"
   ```
   The sum of all balances should be zero.

2. If the trial balance is off, review recent transactions for posting errors.

3. If the trial balance is correct but the balance sheet does not balance,
   this may indicate a bug — report it at
   [GitHub Issues](https://github.com/msiraga/Rich_AIM_Acct_INV/issues).

---

## 6. Frontend Issues

### Error: Blank page in the browser

**Cause:** JavaScript failed to load or React encountered a fatal error.

**Solution:**

1. Open the browser developer tools (F12) and check the **Console** tab for
   errors.
2. Verify the Vite dev server is running: `http://localhost:3000`
3. Clear browser cache and hard-reload (Ctrl+Shift+R or Cmd+Shift+R).
4. Restart the dev server:
   ```bash
   cd nexus-ledger-tauri
   # Stop the server (Ctrl+C), then:
   npm run dev
   ```

---

### Error: "Network Error" or "Failed to fetch" in the UI

**Cause:** The frontend cannot reach the backend API.

**Solution:**

1. Verify the backend is running:
   ```bash
   curl http://localhost:4000/health
   ```

2. Check `nexus-ledger-tauri/src/lib/api.ts` for the correct `API_BASE`:
   ```typescript
   export const API_BASE = "http://localhost:4000";
   ```

3. If you are running the nexus-core API on port 8080 instead of the Tauri
   backend, update `API_BASE` accordingly.

4. Check for firewall or antivirus software blocking localhost connections.

---

### Error: Redirected to login page repeatedly

**Cause:** The access token is expired or invalid, and the refresh token is
also expired.

**Solution:**

1. Log out and log back in through the UI.
2. If the problem persists, clear local storage:
   - Open browser developer tools (F12).
   - Go to **Application** > **Local Storage**.
   - Clear `nexus_access_token`, `nexus_refresh_token`, and `nexus_user`.
   - Refresh the page and log in again.

---

## 7. AI Agent Issues

### Error: "Agent not available" or "No agent found for task type"

**Cause:** The agent for the requested task type is not running or is in an
error state.

**Solution:**

1. Check agent status:
   ```bash
   curl http://localhost:8080/api/v1/agents \
     -H "Authorization: Bearer <access_token>"
   ```

2. If any agent shows status `Error`, restart the server to reinitialize all
   agents.

3. Verify the agent type is correctly spelled in your task request:
   ```json
   {
     "assigned_agent_type": "ReportingAgent"
   }
   ```

   Valid agent types: `LedgerAgent`, `ReconciliationAgent`, `TaxAgent`,
   `PayrollAgent`, `InvoiceAgent`, `ReceiptAgent`, `ReportingAgent`,
   `AuditAgent`, `DocumentAgent`.

---

### Error: AI features not working (Ollama unavailable)

**Cause:** The Ollama AI service is not running or not installed.

**Solution:**

AI features (document OCR, smart categorization) require Ollama. If you do not
need AI features, they degrade gracefully — core accounting still works.

To enable AI features:

1. Install Ollama: [https://ollama.ai](https://ollama.ai)
2. Pull a model: `ollama pull llama3.2`
3. Start the Ollama server: `ollama serve`
4. Verify: `curl http://localhost:11434/api/tags`
5. Restart NexusLedger.

To disable AI features (if Ollama causes issues):

Edit `config/server.toml`:

```toml
[ai]
ollama_url = ""

[features]
enable_ai = false
```

---

### Error: Task stays in queue and never processes

**Cause:** The agent orchestrator is not running, or all agents are busy.

**Solution:**

1. Check the task queue:
   ```bash
   curl http://localhost:8080/api/v1/tasks/queue \
     -H "Authorization: Bearer <access_token>"
   ```

2. Check system status:
   ```bash
   curl http://localhost:8080/api/v1/status \
     -H "Authorization: Bearer <access_token>"
   ```

3. If agents are all in `Busy` status, wait for them to complete their current
   tasks. Each agent processes one task at a time.

4. If agents are in `Error` status, restart the server.

---

## 8. Debug Mode

When troubleshooting, enable debug logging for detailed output.

### Enable Debug Logging

```bash
# Set the log level to debug
export RUST_LOG=debug

# Start the server
cargo run --bin nexus-core
```

On Windows (PowerShell):

```powershell
$env:RUST_LOG = "debug"
cargo run --bin nexus-core
```

### Log Levels

| Level | What It Shows |
|---|---|
| `error` | Only errors (minimal output) |
| `warn` | Warnings and errors |
| `info` | General operation info (default) |
| `debug` | Detailed debug information |
| `trace` | Very verbose — every function call |

### Per-Module Debugging

You can set different log levels for different modules:

```bash
# Debug everything except the database module (which gets info level)
export RUST_LOG="debug,nexus_core::database=info"

# Only debug the API module
export RUST_LOG="nexus_core::api=debug"
```

### Log File

Logs are also written to a file (configured in `config/server.toml`):

```toml
[logging]
file = "data/logs/nexusledger.log"
```

Check this file for errors that may have scrolled off the terminal.

---

## 9. Reset the Database

If your data is corrupt or you want to start fresh, you can reset the database.

### In-Memory Mode

Simply restart the server — all data is cleared automatically.

### SurrealDB (WebSocket Mode)

**Step 1: Stop the NexusLedger server.**

**Step 2: Connect to SurrealDB and drop the database.**

```bash
# Start the SurrealDB SQL shell
surreal sql --endpoint ws://localhost:8000 --user root --pass root --ns nexus --db accounting

# In the shell, run:
REMOVE DATABASE accounting;
# Then exit:
quit;
```

**Step 3: Restart NexusLedger.**

The server will recreate the schema and seed the default chart of accounts on
startup.

### Full Reset (Including SurrealDB Data Files)

If you are running SurrealDB with persistent storage and want to wipe
everything:

```bash
# Stop SurrealDB
docker stop surrealdb
docker rm surrealdb

# Remove the data volume
docker volume rm surrealdb_data

# Start fresh
docker run -d --name surrealdb -p 8000:8000 surrealdb/surrealdb:latest start --user root --pass root
```

---

## 10. Checking System Status

### Health Check

```bash
curl http://localhost:8080/health
```

Returns `200` if the server is alive. Returns `503` if the system health score
is below 0.5 (agents in error state, high failure rate).

### System Status

```bash
curl http://localhost:8080/api/v1/status \
  -H "Authorization: Bearer <access_token>"
```

Returns:
- Total, active, idle, busy, and error agent counts
- Task processing statistics
- System health score (0.0 to 1.0)

### Agent Status

```bash
curl http://localhost:8080/api/v1/agents \
  -H "Authorization: Bearer <access_token>"
```

Lists all 9 agents with their current status (`Idle`, `Busy`, `Error`).

---

## Getting More Help

If none of the solutions above resolve your issue:

1. **Check the [FAQ](faq.md)** for common questions.
2. **Search existing issues** at
   [GitHub Issues](https://github.com/msiraga/Rich_AIM_Acct_INV/issues).
3. **File a new issue** with:
   - The exact error message
   - Your operating system and Rust/Node versions
   - The steps to reproduce the problem
   - The relevant log output (with `RUST_LOG=debug`)
4. **Email support**: mounir@richdaleai.com
