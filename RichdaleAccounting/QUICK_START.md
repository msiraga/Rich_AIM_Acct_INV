# NexusLedger - Quick Start Guide

**Get up and running with NexusLedger in minutes!**

---

## 🚀 5-Minute Quick Start

### Step 1: Install Prerequisites

#### For Local Development:

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# Verify installation
rustc --version
cargo --version
```

#### For Docker Deployment:

```bash
# Install Docker
# Windows/macOS: Download from https://docker.com
# Linux:
curl -fsSL https://get.docker.com | sh

# Verify installation
docker --version
docker-compose --version
```

---

## 📥 Step 2: Get the Code

```bash
# Clone the repository
git clone https://github.com/msiraga/Rich_AIM_Acct_INV.git
cd Rich_AIM_Acct_INV/RichdaleAccounting
```

---

## 🏗️ Step 3: Choose Your Deployment Method

### Option A: Local Development (Recommended for Development)

```bash
# Build the project
./build.sh

# Run the application
./run.sh
```

**Expected Output:**
```
🚀 Starting NexusLedger...
📦 NexusLedger is starting...
🌐 API will be available at http://localhost:8080
📝 Press Ctrl+C to stop
```

### Option B: Docker Compose (Recommended for Production)

```bash
# Start all services (NexusLedger, SurrealDB, Ollama)
docker-compose up -d

# View logs to confirm startup
docker-compose logs -f nexusledger
```

**Expected Output:**
```
nexusledger_1  | 🚀 Starting NexusLedger...
nexusledger_1  | 📦 NexusLedger is starting...
nexusledger_1  | 🌐 API will be available at http://localhost:8080
nexusledger_1  | 📝 Press Ctrl+C to stop
```

### Option C: Manual Docker Deployment

```bash
# Build the Docker image
docker build -t nexusledger .

# Create data directories
mkdir -p data/{invoices,receipts,documents,statements,logs,exports,certs}
mkdir -p config

# Run the container
docker run -d \
  --name nexusledger \
  -p 8080:8080 \
  -p 8081:8081 \
  -v $(pwd)/data:/usr/src/nexusledger/data \
  -v $(pwd)/config:/usr/src/nexusledger/config \
  nexusledger

# View logs
docker logs -f nexusledger
```

---

## 🧪 Step 4: Test the Installation

### Check Health Endpoint

```bash
# Using curl
curl http://localhost:8080/api/health

# Expected response:
# {"status":"healthy","version":"1.0.0","timestamp":"2024-01-01T00:00:00Z"}
```

### Check Agents Endpoint

```bash
# List all agents
curl http://localhost:8080/api/agents

# Expected response: List of all 9 agent types with their status
```

### Check API Documentation

```bash
# Open in browser
open http://localhost:8080/api/docs

# Or use curl
curl http://localhost:8080/api/docs
```

---

## 📝 Step 5: Basic Configuration

### Edit Configuration File

```bash
# Edit the configuration file
nano config/server.toml
```

**Example Configuration:**
```toml
[server]
port = 8080
host = "0.0.0.0"

[database]
# For local SurrealDB
url = "ws://localhost:8000"
ns = "nexus"
db = "accounting"

# For embedded SurrealDB (no separate server needed)
# url = "mem://"

[ai]
# For local Ollama
ollama_url = "http://localhost:11434"
model = "llama3.2"

# For no AI (disable AI features)
# ollama_url = ""

[logging]
level = "info"
file = "data/logs/nexusledger.log"
```

### Environment Variables

You can also configure NexusLedger using environment variables:

```bash
# Set environment variables
export RUST_LOG=debug
export SURREALDB_URL=ws://localhost:8000
export OLLAMA_URL=http://localhost:11434

# Then run
./run.sh
```

---

## 🎯 Step 6: First Steps with NexusLedger

### Create Your First Account

```bash
# Create a bank account
curl -X POST http://localhost:8080/api/accounts \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Bank of America - Checking",
    "account_type": "Asset",
    "balance_type": "Debit",
    "code": "1000",
    "description": "Primary business checking account"
  }'
```

### Upload Your First Document

```bash
# Upload an invoice PDF
curl -X POST http://localhost:8080/api/documents \
  -H "Content-Type: multipart/form-data" \
  -F "file=@/path/to/invoice.pdf" \
  -F "document_type=Invoice"
```

### Process the Document

```bash
# Process the uploaded document (extract data)
curl -X POST http://localhost:8080/api/documents/{document_id}/process
```

### Create Your First Transaction

```bash
# Create a simple transaction
curl -X POST http://localhost:8080/api/transactions \
  -H "Content-Type: application/json" \
  -d '{
    "date": "2024-01-01T00:00:00Z",
    "description": "Initial deposit",
    "reference": "DEP-001",
    "entries": [
      {
        "account_id": "{bank_account_id}",
        "amount": "1000.00",
        "entry_type": "Debit",
        "description": "Initial deposit"
      },
      {
        "account_id": "{equity_account_id}",
        "amount": "1000.00",
        "entry_type": "Credit",
        "description": "Owner capital"
      }
    ]
  }'
```

---

## 📊 Step 7: Generate Your First Report

### Balance Sheet

```bash
# Get balance sheet
curl http://localhost:8080/api/reports/balance-sheet \
  -H "Accept: application/json"
```

### Income Statement

```bash
# Get income statement for current month
curl http://localhost:8080/api/reports/income-statement?period=month \
  -H "Accept: application/json"
```

### Trial Balance

```bash
# Get trial balance
curl http://localhost:8080/api/reports/trial-balance \
  -H "Accept: application/json"
```

---

## 🔧 Step 8: Working with AI Agents

### List All Agents

```bash
curl http://localhost:8080/api/agents
```

### Get Agent Details

```bash
# Get details for a specific agent
curl http://localhost:8080/api/agents/{agent_id}
```

### Assign Task to Agent

```bash
# Ask the DocumentAgent to process a document
curl -X POST http://localhost:8080/api/agents/{document_agent_id}/task \
  -H "Content-Type: application/json" \
  -d '{
    "task_type": "process_document",
    "parameters": {
      "document_id": "{document_id}",
      "priority": "high"
    }
  }'
```

### Check Agent Status

```bash
# Check status of an agent
curl http://localhost:8080/api/agents/{agent_id}/status
```

---

## 🛠️ Troubleshooting

### Common Issues

#### Issue: Port 8080 already in use

**Solution:**
```bash
# Find and kill the process using port 8080
lsof -i :8080
kill -9 <PID>

# Or change the port in config/server.toml
port = 8081
```

#### Issue: SurrealDB connection failed

**Solution:**
```bash
# Start SurrealDB locally
docker run -d -p 8000:8000 --name surrealdb surrealdb/surrealdb:latest start --bind 0.0.0.0:8000

# Or use embedded mode in config/server.toml
url = "mem://"
```

#### Issue: Ollama not available

**Solution:**
```bash
# Install Ollama (https://ollama.ai)
curl -fsSL https://ollama.ai/install.sh | sh

# Pull a model
ollama pull llama3.2

# Start Ollama server
ollama serve

# Or disable AI features in config/server.toml
ollama_url = ""
```

#### Issue: Build failed

**Solution:**
```bash
# Update Rust
rustup update

# Clean and rebuild
cargo clean
cargo build --release
```

---

## 📚 Next Steps

### Learn More

1. **Read the full documentation**: [README.md](README.md)
2. **Explore the project structure**: [PROJECT_SUMMARY.md](PROJECT_SUMMARY.md)
3. **Check out the API documentation**: http://localhost:8080/api/docs

### Try Advanced Features

1. **Set up multiple users**: Configure RBAC in the admin panel
2. **Connect to your bank**: Use the bank integration features
3. **Set up recurring transactions**: Automate regular payments
4. **Create custom reports**: Use the reporting API
5. **Try the AI features**: Ask agents to perform complex tasks

### Get Involved

1. **Report issues**: https://github.com/msiraga/Rich_AIM_Acct_INV/issues
2. **Contribute code**: Fork the repository and submit pull requests
3. **Join the community**: Discuss with other users and developers

---

## 🎉 Success!

You've successfully installed and configured NexusLedger! 🎉

**What you've accomplished:**
- ✅ Installed all prerequisites
- ✅ Cloned the repository
- ✅ Built the project
- ✅ Started the server
- ✅ Tested the API endpoints
- ✅ Created your first account and transaction
- ✅ Generated your first financial report

**What's next:**
- 🚀 Explore the AI agent capabilities
- 📊 Set up your complete chart of accounts
- 📄 Import your existing financial data
- 🔄 Set up automatic bank feeds
- 📈 Generate advanced financial reports

---

## 📞 Need Help?

- **Documentation**: [README.md](README.md)
- **GitHub Issues**: https://github.com/msiraga/Rich_AIM_Acct_INV/issues
- **Email Support**: mounir@richdaleai.com
- **Community**: Join our Discord server (link in README)

---

**Happy Accounting with NexusLedger! 💰**