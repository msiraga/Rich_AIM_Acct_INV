# Quick Start Guide

This guide walks you through creating your first transaction in NexusLedger —
from registration to viewing the result in the ledger. If you have not yet
installed NexusLedger, see the [Installation Guide](installation.md) first.

You can interact with NexusLedger through the **web/desktop UI** or the
**REST API**. Both paths are documented below.

---

## Table of Contents

1. [Start the Server](#1-start-the-server)
2. [Register an Account](#2-register-an-account)
3. [Log In](#3-log-in)
4. [Review the Chart of Accounts](#4-review-the-chart-of-accounts)
5. [Create Your First Transaction](#5-create-your-first-transaction)
6. [View the Transaction in the Ledger](#6-view-the-transaction-in-the-ledger)
7. [Generate a Financial Report](#7-generate-a-financial-report)
8. [Explore the UI](#8-explore-the-ui)
9. [Working with AI Agents](#9-working-with-ai-agents)

---

## 1. Start the Server

### Option A: Tauri Desktop App (Recommended for Beginners)

```bash
cd nexus-ledger-tauri
npm install
npm run dev
```

This starts the Vite dev server on **http://localhost:3000** and the Tauri
backend API on **port 4000**. A desktop window should open automatically.

### Option B: Backend API Only

```bash
cd RichdaleAccounting
JWT_SECRET="dev-secret-key-change-in-production-32b!" cargo run --bin nexus-core
```

The API server starts on **http://localhost:8080**.

### Verify It Is Running

```bash
curl http://localhost:8080/health
# {"status":"ok"}

# Or, if using the Tauri backend:
curl http://localhost:4000/health
# {"status":"ok"}
```

---

## 2. Register an Account

### Via the UI

1. Open **http://localhost:3000** in your browser (or the Tauri desktop window).
2. Click **Register** (or navigate to `/register`).
3. Fill in the form:
   - **Name**: Your name
   - **Email**: e.g., `admin@example.com`
   - **Password**: At least 8 characters
4. Click **Register**.

The first registered user is automatically assigned the **Admin** role.

### Via the API

```bash
curl -X POST http://localhost:8080/api/auth/register \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Admin User",
    "email": "admin@example.com",
    "password": "securepassword123"
  }'
```

**Expected response:**

```json
{
  "success": true,
  "data": {
    "user": {
      "id": "uuid-here",
      "email": "admin@example.com",
      "role": "Admin"
    },
    "access_token": "eyJ...",
    "refresh_token": "eyJ..."
  }
}
```

> **Tip:** Save the `access_token` — you will need it for all subsequent API
> calls. In the UI, the token is stored automatically.

---

## 3. Log In

### Via the UI

1. Navigate to **http://localhost:3000/login**.
2. Enter your email and password.
3. Click **Log In**.
4. You will be redirected to the **Dashboard**.

### Via the API

```bash
curl -X POST http://localhost:8080/api/auth/login \
  -H "Content-Type: application/json" \
  -d '{
    "email": "admin@example.com",
    "password": "securepassword123"
  }'
```

**Expected response:**

```json
{
  "success": true,
  "data": {
    "user": {
      "id": "uuid-here",
      "email": "admin@example.com",
      "role": "Admin"
    },
    "access_token": "eyJ...",
    "refresh_token": "eyJ..."
  }
}
```

For all subsequent API calls, include the access token in the `Authorization`
header:

```bash
-H "Authorization: Bearer <access_token>"
```

---

## 4. Review the Chart of Accounts

NexusLedger ships with a default chart of 20 accounts covering the standard
accounting categories. Let's verify they are available.

### Via the UI

1. From the Dashboard, click **Accounts** in the navigation bar.
2. You will see a table listing all accounts with their code, name, type
   (Asset, Liability, Equity, Revenue, Expense), and current balance.

### Via the API

```bash
curl http://localhost:8080/api/v1/accounts \
  -H "Authorization: Bearer <access_token>"
```

**Default chart of accounts:**

| Code | Account Name | Type |
|---|---|---|
| 1000 | Cash | Asset |
| 1010 | Bank Account | Asset |
| 1020 | Accounts Receivable | Asset |
| 1030 | Inventory | Asset |
| 1040 | Fixed Assets | Asset |
| 2000 | Accounts Payable | Liability |
| 2010 | Loans Payable | Liability |
| 2020 | Accrued Expenses | Liability |
| 3000 | Owner's Equity | Equity |
| 3010 | Retained Earnings | Equity |
| 4000 | Sales Revenue | Revenue |
| 4010 | Service Revenue | Revenue |
| 4020 | Interest Revenue | Revenue |
| 5000 | Cost of Goods Sold | Expense |
| 5010 | Salaries Expense | Expense |
| 5020 | Rent Expense | Expense |
| 5030 | Utilities Expense | Expense |
| 5040 | Office Supplies | Expense |

> The chart of accounts is fully customizable. See the
> [FAQ](faq.md#how-do-i-add-a-new-account) for instructions on adding accounts.

---

## 5. Create Your First Transaction

A transaction in double-entry accounting always has at least two entries
(one debit, one credit), and the total debits must equal the total credits.

Let's record a simple transaction: the owner deposits $10,000 into the business
bank account as starting capital.

### Via the UI

1. Click **Journal Entry** in the navigation bar.
2. Fill in the form:
   - **Date**: Today's date
   - **Description**: "Owner capital contribution"
   - **Reference**: "DEP-001" (optional)
3. Add the first line:
   - **Account**: Bank Account (1010)
   - **Debit**: $10,000.00
4. Add the second line:
   - **Account**: Owner's Equity (3000)
   - **Credit**: $10,000.00
5. Verify that **Total Debits = Total Credits = $10,000.00**.
6. Click **Save Transaction**.

### Via the API

```bash
curl -X POST http://localhost:8080/api/v1/transactions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <access_token>" \
  -d '{
    "date": "2026-07-01T00:00:00Z",
    "description": "Owner capital contribution",
    "reference": "DEP-001",
    "entries": [
      {
        "account_id": "<bank-account-uuid>",
        "amount": "10000.00",
        "entry_type": "Debit",
        "description": "Initial deposit to bank"
      },
      {
        "account_id": "<equity-account-uuid>",
        "amount": "10000.00",
        "entry_type": "Credit",
        "description": "Owner capital"
      }
    ]
  }'
```

> **How to find account IDs:** Call `GET /api/v1/accounts` and note the `id`
> field for each account.

**Expected response:**

```json
{
  "success": true,
  "data": {
    "id": "transaction-uuid",
    "date": "2026-07-01T00:00:00Z",
    "description": "Owner capital contribution",
    "reference": "DEP-001",
    "status": "Posted",
    "entries": [ ... ]
  }
}
```

### Understanding Double-Entry

Here is why this transaction balances:

| Account | Type | Normal Balance | Entry | Effect |
|---|---|---|---|---|
| Bank Account | Asset | Debit | Debit $10,000 | Increases the bank balance |
| Owner's Equity | Equity | Credit | Credit $10,000 | Increases equity |

Debits ($10,000) = Credits ($10,000). The books balance.

---

## 6. View the Transaction in the Ledger

### Via the UI

1. Click **Transactions** in the navigation bar.
2. You will see a list of all transactions, including the one you just created.
3. Click on a transaction to view its full detail (all entries, date,
   description, and status).

### Via the API

```bash
# List all transactions
curl http://localhost:8080/api/v1/transactions \
  -H "Authorization: Bearer <access_token>"

# Get a specific transaction
curl http://localhost:8080/api/v1/transactions/<transaction-id> \
  -H "Authorization: Bearer <access_token>"
```

---

## 7. Generate a Financial Report

Now that you have a transaction recorded, let's generate a report.

### Trial Balance

The trial balance lists all accounts and their balances. Debit balances and
credit balances should be equal.

**Via the API:**

```bash
curl http://localhost:8080/api/v1/reports/trial_balance \
  -H "Authorization: Bearer <access_token>"
```

### Balance Sheet

Shows Assets = Liabilities + Equity.

**Via the API:**

```bash
curl http://localhost:8080/api/v1/reports/balance_sheet \
  -H "Authorization: Bearer <access_token>"
```

### Income Statement (Profit & Loss)

Shows Revenue - Expenses = Net Income for a period.

**Via the API:**

```bash
curl http://localhost:8080/api/v1/reports/income_statement \
  -H "Authorization: Bearer <access_token>"
```

> For a detailed walkthrough of all reports, see the
> [Reports Guide](reports-guide.md).

---

## 8. Explore the UI

NexusLedger's desktop app has the following pages:

| Page | URL Path | What You Can Do |
|---|---|---|
| **Dashboard** | `/` | Overview of accounts, invoices, and system status |
| **Accounts** | `/accounts` | View the chart of accounts and balances |
| **Transactions** | `/transactions` | Browse and search all transactions |
| **Journal Entry** | `/journal` | Create new double-entry transactions |
| **Invoices** | `/invoices` | Create and manage customer invoices |
| **Login** | `/login` | Log in to your account |
| **Register** | `/register` | Register a new user account |

### Chat Sidebar

The desktop app includes a **Chat Sidebar** that lets you interact with
NexusLedger's AI agents. You can ask natural-language questions like:

- "What is our current cash balance?"
- "Generate a balance sheet for this month"
- "Show me all transactions from last week"

The chat uses a WebSocket connection to the backend and routes your requests
to the appropriate agent for processing.

---

## 9. Working with AI Agents

NexusLedger has 9 autonomous agents that handle accounting tasks. You can
interact with them directly through the API.

### List All Agents

```bash
curl http://localhost:8080/api/v1/agents \
  -H "Authorization: Bearer <access_token>"
```

### Submit a Task to an Agent

```bash
curl -X POST http://localhost:8080/api/v1/tasks \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <access_token>" \
  -d '{
    "task_type": "GenerateReport",
    "priority": "Normal",
    "assigned_agent_type": "ReportingAgent",
    "payload": {
      "type": "Map",
      "value": {
        "report": "balance_sheet"
      }
    }
  }'
```

### Check the Task Queue

```bash
curl http://localhost:8080/api/v1/tasks/queue \
  -H "Authorization: Bearer <access_token>"
```

### Agent Overview

| Agent | What It Does |
|---|---|
| **LedgerAgent** | Records transactions, manages the chart of accounts |
| **ReconciliationAgent** | Matches bank statements to book transactions |
| **TaxAgent** | Calculates taxes and tracks filing deadlines |
| **PayrollAgent** | Processes payroll with tax withholding |
| **InvoiceAgent** | Creates and manages customer invoices |
| **ReceiptAgent** | Processes receipts and categorizes expenses |
| **ReportingAgent** | Generates balance sheets, P&L, cash flow, trial balance |
| **AuditAgent** | Maintains audit trails and checks for anomalies |
| **DocumentAgent** | Stores, retrieves, and processes financial documents |

---

## Next Steps

Congratulations! You have created your first transaction in NexusLedger. Here
are some things to try next:

- [Import bank statements via CSV](#) — Use `POST /api/v1/import/csv`
- [Export data to OFX format](#) — Use `GET /api/v1/export/ofx`
- [Set up a budget](#) — Use `POST /api/v1/budgets`
- [Track a fixed asset](#) — Use `POST /api/v1/assets`
- [Convert between currencies](#) — Use `POST /api/v1/convert`
- [Generate a full financial report](reports-guide.md)
- [Review the FAQ](faq.md) for common questions
- [Troubleshooting guide](troubleshooting.md) if something goes wrong

---

## Quick Reference: Common API Calls

```bash
# All commands require the Authorization header:
# -H "Authorization: Bearer <access_token>"

# Auth
POST /api/auth/register     # Register new user
POST /api/auth/login        # Log in
POST /api/auth/refresh      # Refresh access token

# Accounts
GET  /api/v1/accounts       # List all accounts
GET  /api/v1/accounts/:id   # Get one account

# Transactions
GET  /api/v1/transactions       # List all transactions
POST /api/v1/transactions       # Create a transaction
GET  /api/v1/transactions/:id   # Get one transaction

# Invoices
GET  /api/v1/invoices       # List invoices
POST /api/v1/invoices       # Create an invoice

# Reports
GET  /api/v1/reports/trial_balance    # Trial balance
GET  /api/v1/reports/balance_sheet    # Balance sheet
GET  /api/v1/reports/income_statement # Income statement (P&L)
GET  /api/v1/reports/cash-flow        # Cash flow statement
GET  /api/v1/reports/ar-aging         # AR aging report

# Agents
GET  /api/v1/agents         # List all agents
POST /api/v1/tasks          # Submit a task
GET  /api/v1/tasks/queue    # View task queue

# Health
GET  /health                # Liveness check
```
