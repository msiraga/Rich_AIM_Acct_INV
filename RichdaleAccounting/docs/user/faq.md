# Frequently Asked Questions

Common questions about using NexusLedger. For troubleshooting specific errors,
see the [Troubleshooting Guide](troubleshooting.md).

---

## Table of Contents

1. [Accounts & Chart of Accounts](#accounts--chart-of-accounts)
2. [Transactions](#transactions)
3. [Bank Statements & Import/Export](#bank-statements--importexport)
4. [Multi-Currency](#multi-currency)
5. [Reports](#reports)
6. [User Roles & Access Control](#user-roles--access-control)
7. [Backup & Data](#backup--data)
8. [Offline Usage](#offline-usage)
9. [AI Agents](#ai-agents)
10. [Budgets & Fixed Assets](#budgets--fixed-assets)

---

## Accounts & Chart of Accounts

### How do I add a new account?

**Via the API:**

Currently, accounts are created through the ledger. The default chart of
accounts (20 accounts) is seeded automatically on first run. To add a custom
account, submit a task to the LedgerAgent:

```bash
curl -X POST http://localhost:8080/api/v1/tasks \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <access_token>" \
  -d '{
    "task_type": "RecordTransaction",
    "assigned_agent_type": "LedgerAgent",
    "payload": {
      "type": "Map",
      "value": {
        "action": "create_account",
        "name": "Petty Cash",
        "account_type": "Asset",
        "code": "1050",
        "description": "Small cash fund for minor expenses"
      }
    }
  }'
```

**Via the UI:**

Use the Chat Sidebar and type: "Add a new asset account called Petty Cash with
code 1050."

### What account types are supported?

NexusLedger supports the five standard accounting types:

| Type | Normal Balance | Examples |
|---|---|---|
| **Asset** | Debit | Cash, Bank, A/R, Inventory, Equipment |
| **Liability** | Credit | A/P, Loans, Accrued Expenses |
| **Equity** | Credit | Owner's Equity, Retained Earnings |
| **Revenue** | Credit | Sales, Services, Interest Income |
| **Expense** | Debit | COGS, Salaries, Rent, Utilities |

### Can I deactivate an account without deleting it?

Yes. Accounts can be set to inactive. Inactive accounts cannot be used in new
transactions, but their historical data is preserved for reporting. This is the
recommended approach rather than deleting an account that has transactions.

---

## Transactions

### How do I record a transaction?

Transactions are created via the Journal Entry page in the UI or via the API.
Every transaction requires at least two entries (one debit, one credit) with
equal totals. See the [Quick Start Guide](quick-start.md) for a step-by-step
walkthrough.

### What transaction types does NexusLedger support?

NexusLedger supports general journal entries. Additionally, the Invoice and
Receipt agents can create specialized transactions:

- **Invoice transactions**: Debit A/R, Credit Revenue (when an invoice is
  created)
- **Payment transactions**: Debit Cash/Bank, Credit A/R (when a customer pays)
- **Bill transactions**: Debit Expense, Credit A/P (when a vendor bill is
  recorded)
- **Bill payment**: Debit A/P, Credit Cash/Bank (when you pay a vendor)

### Can I edit or delete a transaction?

For audit integrity, transactions should not be deleted. Instead, create a
reversing entry (a new transaction with the debit and credit entries swapped)
to correct an error. This maintains a complete audit trail.

The AuditAgent monitors all transaction changes and maintains a tamper-evident
log.

### What is a "reference" field on a transaction?

The reference field is an optional free-text field you can use to store a
check number, deposit slip number, invoice number, or any other identifier
that helps you trace the transaction back to its source document.

---

## Bank Statements & Import/Export

### How do I import bank statements?

NexusLedger supports CSV import for bank statements and transaction data.

**Via the API:**

```bash
curl -X POST http://localhost:8080/api/v1/import/csv \
  -H "Authorization: Bearer <access_token>" \
  -H "Content-Type: multipart/form-data" \
  -F "file=@bank_statement.csv"
```

**CSV format:**

The CSV file should have the following columns:

```csv
date,description,amount,account_code,entry_type
2026-07-01,Office Supplies Purchase,-150.00,5040,Debit
2026-07-01,Customer Payment,2000.00,1010,Debit
2026-07-02,Rent Payment,-3000.00,5020,Debit
```

| Column | Description |
|---|---|
| `date` | Transaction date (YYYY-MM-DD) |
| `description` | Description of the transaction |
| `amount` | Amount (positive for inflow, negative for outflow) |
| `account_code` | The chart of accounts code to post to |
| `entry_type` | "Debit" or "Credit" |

**Via the UI:**

Use the Chat Sidebar: "Import transactions from bank_statement.csv"

### How do I export data?

NexusLedger supports two export formats:

**CSV Export** (for spreadsheets and accounting software):

```bash
curl http://localhost:8080/api/v1/export/csv \
  -H "Authorization: Bearer <access_token>" \
  -o nexus_export.csv
```

**OFX Export** (for personal finance software like Quicken):

```bash
curl http://localhost:8080/api/v1/export/ofx \
  -H "Authorization: Bearer <access_token>" \
  -o nexus_export.ofx
```

### Can I import from QuickBooks or other accounting software?

Currently, NexusLedger supports CSV import. To migrate from QuickBooks:

1. Export your chart of accounts and transactions from QuickBooks as CSV.
2. Map the columns to NexusLedger's expected format (see above).
3. Import using the CSV import endpoint.

Direct QuickBooks import is on the roadmap.

---

## Multi-Currency

### How do I set up multi-currency?

NexusLedger supports multi-currency transactions with real-time exchange rate
conversion.

**Step 1: Set exchange rates.**

```bash
curl -X POST http://localhost:8080/api/v1/exchange-rates \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <access_token>" \
  -d '{
    "from_currency": "EUR",
    "to_currency": "USD",
    "rate": "1.08"
  }'
```

**Step 2: View current rates.**

```bash
curl http://localhost:8080/api/v1/exchange-rates \
  -H "Authorization: Bearer <access_token>"
```

**Step 3: Convert an amount.**

```bash
curl -X POST http://localhost:8080/api/v1/convert \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <access_token>" \
  -d '{
    "from_currency": "EUR",
    "to_currency": "USD",
    "amount": "1000.00"
  }'
```

**Response:**

```json
{
  "success": true,
  "data": {
    "from_currency": "EUR",
    "to_currency": "USD",
    "original_amount": "1000.00",
    "converted_amount": "1080.00",
    "rate": "1.08"
  }
}
```

### What currencies are supported?

NexusLedger supports 160+ currencies. You set the exchange rates manually (as
shown above). Automatic rate fetching from external APIs is on the roadmap.

### Does the base currency have to be USD?

No. The base currency is determined by the accounts in your chart of accounts.
All account balances are stored in your base currency, and multi-currency
transactions are converted using the exchange rates you configure.

---

## Reports

### How do I generate a P&L (Income Statement) for a specific period?

**Via the API:**

```bash
curl http://localhost:8080/api/v1/reports/income_statement \
  -H "Authorization: Bearer <access_token>"
```

The income statement defaults to the trailing 365-day period. For period-specific
reporting, use the ReportingAgent:

```bash
curl -X POST http://localhost:8080/api/v1/tasks \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <access_token>" \
  -d '{
    "task_type": "GenerateReport",
    "assigned_agent_type": "ReportingAgent",
    "payload": {
      "type": "Map",
      "value": {
        "report_type": "income_statement",
        "start_date": "2026-01-01",
        "end_date": "2026-06-30"
      }
    }
  }'
```

**Via the UI:**

Use the Chat Sidebar: "Show me the income statement for Q1 2026" or "Generate a
P&L from January to June 2026."

### What reports are available?

See the [Reports Guide](reports-guide.md) for a complete walkthrough of all
available reports:

- Trial Balance
- Balance Sheet
- Income Statement (P&L)
- Cash Flow Statement
- Accounts Receivable Aging Report
- Budget Variance Report
- Accounts Payable Outstanding Report

### Can I schedule automatic report generation?

You can submit report tasks to the ReportingAgent at any time. For recurring
schedules (e.g., monthly P&L), you can set up an external cron job that calls
the API:

```bash
# Example: Generate monthly P&L on the 1st of each month
# Add to crontab:
0 0 1 * * curl -X POST http://localhost:8080/api/v1/tasks \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <token>" \
  -d '{"task_type":"GenerateReport","assigned_agent_type":"ReportingAgent","payload":{"type":"Map","value":{"report_type":"income_statement"}}}'
```

Built-in scheduled reporting is on the roadmap.

---

## User Roles & Access Control

### What are the user roles?

NexusLedger has a 5-tier role-based access control (RBAC) system:

| Role | Permissions |
|---|---|
| **Guest** | No access (must register first) |
| **Viewer** | Read-only access to accounts, transactions, and reports |
| **User** | Can create transactions and invoices; view reports |
| **Manager** | All User permissions plus budget and asset management |
| **Admin** | Full access including user management and role assignment |

The first registered user is automatically assigned the **Admin** role.

### How do I change a user's role?

Only Admins can change user roles:

**Via the API:**

```bash
curl -X POST http://localhost:8080/api/v1/users/<user-id>/role \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <admin-access-token>" \
  -d '{"role": "Manager"}'
```

**Via the UI:**

Use the Chat Sidebar (as Admin): "Make john@example.com a Manager."

### How do I list all users?

Admins can list all registered users:

```bash
curl http://localhost:8080/api/v1/users \
  -H "Authorization: Bearer <admin-access-token>"
```

---

## Backup & Data

### How do I backup my data?

**Method 1: SurrealDB Export**

If using SurrealDB in WebSocket mode, use the SurrealDB export tool:

```bash
surreal export ws://localhost:8000 --user root --pass root --ns nexus --db accounting > backup.sql
```

To restore:

```bash
surreal import ws://localhost:8000 --user root --pass root --ns nexus --db accounting backup.sql
```

**Method 2: CSV Export**

Export all transactions to CSV as a basic backup:

```bash
curl http://localhost:8080/api/v1/export/csv \
  -H "Authorization: Bearer <access_token>" \
  -o backup_$(date +%Y%m%d).csv
```

**Method 3: Data Directory Backup**

If running SurrealDB with local storage, back up the data directory:

```bash
# Stop the server first
cp -r data/ backup_$(date +%Y%m%d)/
```

### Does NexusLedger have automatic backups?

Yes. NexusLedger has automatic backup configured in `config/server.toml`:

```toml
[backup]
enable_auto_backup = true
backup_interval = 86400   # 24 hours
backup_dir = "data/backups"
max_backups = 7           # Keep the 7 most recent backups
```

Backups are stored in `data/backups/` and the oldest are automatically pruned.

### Can I use this offline?

Yes. NexusLedger is designed with offline-first capabilities. The core
accounting engine runs entirely locally — no internet connection is required
for:

- Creating transactions
- Generating reports
- Managing accounts
- AI agent processing (with local Ollama models)

Internet is only needed if you are using:
- External exchange rate APIs
- Cloud synchronization (if enabled)
- Remote SurrealDB instances

For full offline mode with local data persistence, use in-memory mode
(`mem://`) or a local SurrealDB instance.

---

## AI Agents

### What are the 9 agents and what do they do?

| Agent | Role |
|---|---|
| **LedgerAgent** | Records transactions, manages the chart of accounts, generates trial balance |
| **ReconciliationAgent** | Matches bank statements to book transactions, identifies discrepancies |
| **TaxAgent** | Calculates taxes (US Federal, state), tracks filing deadlines, generates tax forms |
| **PayrollAgent** | Processes payroll with tax withholding (Social Security, Medicare, federal/state tax) |
| **InvoiceAgent** | Creates customer invoices, tracks payment status, generates customer statements |
| **ReceiptAgent** | Processes receipts, categorizes expenses, links to transactions |
| **ReportingAgent** | Generates all financial reports (balance sheet, P&L, cash flow, trial balance) |
| **AuditAgent** | Maintains audit trails, detects anomalies, checks for fraud patterns |
| **DocumentAgent** | Stores, retrieves, and processes financial documents (invoices, receipts, statements) |

### How do I interact with agents?

You can interact with agents through:

1. **The Chat Sidebar** (UI) — type natural-language requests
2. **The REST API** — submit structured tasks
3. **WebSocket** — real-time chat at `ws://localhost:8080/ws/chat`

### Do I need Ollama/AI to use NexusLedger?

No. AI features (document OCR, smart categorization, anomaly detection) are
optional. Core accounting — double-entry transactions, reports, budgets — works
without any AI service.

To disable AI, set `enable_ai = false` in `config/server.toml`.

---

## Budgets & Fixed Assets

### How do I set up a budget?

Create a budget for a specific period (month or quarter):

```bash
curl -X POST http://localhost:8080/api/v1/budgets \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <access_token>" \
  -d '{
    "period": "2026-07",
    "items": [
      {"account_id": "<salaries-uuid>", "budgeted_amount": "20000.00"},
      {"account_id": "<rent-uuid>", "budgeted_amount": "5000.00"},
      {"account_id": "<sales-revenue-uuid>", "budgeted_amount": "50000.00"}
    ]
  }'
```

Then generate a variance report to compare actual vs. budgeted amounts:

```bash
curl "http://localhost:8080/api/v1/budgets/variance?period=2026-07" \
  -H "Authorization: Bearer <access_token>"
```

### How do I track fixed assets and depreciation?

Register a fixed asset:

```bash
curl -X POST http://localhost:8080/api/v1/assets \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <access_token>" \
  -d '{
    "name": "Delivery Van",
    "asset_code": "FA-001",
    "cost": "35000.00",
    "useful_life_years": 5,
    "salvage_value": "5000.00",
    "depreciation_method": "StraightLine"
  }'
```

Compute depreciation:

```bash
curl -X POST http://localhost:8080/api/v1/assets/depreciation \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <access_token>" \
  -d '{
    "asset_id": "<asset-uuid>",
    "method": "StraightLine",
    "period": "2026-07"
  }'
```

NexusLedger supports two depreciation methods:

| Method | Description |
|---|---|
| **Straight-Line** | Equal depreciation each year: (cost - salvage) / useful life |
| **Double-Declining Balance** | Accelerated depreciation: 2x straight-line rate on remaining book value |

---

## Still Have Questions?

- [Quick Start Guide](quick-start.md) — Step-by-step first transaction
- [Reports Guide](reports-guide.md) — All financial reports explained
- [Troubleshooting](troubleshooting.md) — Fix common errors
- [Installation Guide](installation.md) — Set up NexusLedger
- **Email**: mounir@richdaleai.com
- **GitHub Issues**: [https://github.com/msiraga/Rich_AIM_Acct_INV/issues](https://github.com/msiraga/Rich_AIM_Acct_INV/issues)
