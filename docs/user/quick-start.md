# Quick Start Guide

This guide walks you through creating your first transaction in NexusLedger —
from registration to viewing the balance sheet — with expected output at each
step. If you have not yet installed NexusLedger, see the
[Installation Guide](install.md) first.

You can interact with NexusLedger through the **desktop UI** or the **REST API**.
Both paths are documented below. The examples use `http://localhost:8080` as the
API base URL.

---

## Table of Contents

1. [Start the Server](#1-start-the-server)
2. [Register an Admin Account](#2-register-an-admin-account)
3. [Review the Seeded Chart of Accounts](#3-review-the-seeded-chart-of-accounts)
4. [Create Your First Journal Entry](#4-create-your-first-journal-entry)
5. [View the Trial Balance](#5-view-the-trial-balance)
6. [Create an Invoice](#6-create-an-invoice)
7. [Upload a Receipt (if AI is available)](#7-upload-a-receipt-if-ai-is-available)
8. [View the Balance Sheet](#8-view-the-balance-sheet)

---

## 1. Start the Server

### Desktop app (recommended)

```bash
cd nexus-ledger-tauri
npm install
export JWT_SECRET="your-secret-key-at-least-32-bytes-long!"
cargo tauri dev
```

A desktop window opens automatically. The API server runs on port 8080.

### API-only mode

```bash
cd RichdaleAccounting
export JWT_SECRET="your-secret-key-at-least-32-bytes-long!"
cargo run --bin nexus-core
```

### Verify it is running

```bash
curl http://localhost:8080/health
```

**Expected output:**

```json
{"status":"ok","uptime_seconds":3,"timestamp":"2026-07-01T12:00:00.000Z"}
```

---

## 2. Register an Admin Account

The first registered user is automatically assigned the **Admin** role.

### Via the UI

1. Open the NexusLedger desktop window (or navigate to `http://localhost:8080`
   in your browser if using API-only mode with the Vite dev server).
2. Click **Register**.
3. Fill in:
   - **Username**: `admin`
   - **Email**: `admin@example.com`
   - **Password**: `SecurePass123` (must be 8+ characters with at least one letter and one number)
4. Click **Register**.

### Via the API

```bash
curl -X POST http://localhost:8080/api/auth/register \
  -H "Content-Type: application/json" \
  -d '{
    "username": "admin",
    "email": "admin@example.com",
    "password": "SecurePass123"
  }'
```

**Expected output:**

```json
{
  "success": true,
  "data": {
    "user_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
    "username": "admin",
    "role": "user",
    "access_token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...",
    "refresh_token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...",
    "expires_in": 1800
  },
  "metadata": {
    "request_id": "...",
    "timestamp": "2026-07-01T12:00:01.000Z",
    "api_version": "v1"
  }
}
```

> **Important:** Save the `access_token` value. You will need it in the
> `Authorization: Bearer <token>` header for all subsequent API calls. In the
> UI, the token is stored automatically.

For the remaining steps, set a shell variable for convenience:

```bash
TOKEN="eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."
```

---

## 3. Review the Seeded Chart of Accounts

NexusLedger ships with a default chart of 20 accounts, seeded automatically on
first run.

### Via the UI

Navigate to **Accounts** in the sidebar. You will see a table of all accounts.

### Via the API

```bash
curl http://localhost:8080/api/v1/accounts \
  -H "Authorization: Bearer $TOKEN"
```

**Expected output (abbreviated):**

```json
{
  "success": true,
  "data": [
    {"id": "...", "number": "1000", "name": "Cash", "type": "Asset", "balance": "0", "status": "Active"},
    {"id": "...", "number": "1010", "name": "Bank Account", "type": "Asset", "balance": "0", "status": "Active"},
    {"id": "...", "number": "1020", "name": "Accounts Receivable", "type": "Asset", "balance": "0", "status": "Active"},
    {"id": "...", "number": "1030", "name": "Inventory", "type": "Asset", "balance": "0", "status": "Active"},
    {"id": "...", "number": "1040", "name": "Fixed Assets", "type": "Asset", "balance": "0", "status": "Active"},
    {"id": "...", "number": "1050", "name": "Accumulated Depreciation", "type": "Asset", "balance": "0", "status": "Active"},
    {"id": "...", "number": "2000", "name": "Accounts Payable", "type": "Liability", "balance": "0", "status": "Active"},
    {"id": "...", "number": "2010", "name": "Loans Payable", "type": "Liability", "balance": "0", "status": "Active"},
    {"id": "...", "number": "2020", "name": "Accrued Expenses", "type": "Liability", "balance": "0", "status": "Active"},
    {"id": "...", "number": "3000", "name": "Owner's Equity", "type": "Equity", "balance": "0", "status": "Active"},
    {"id": "...", "number": "3010", "name": "Retained Earnings", "type": "Equity", "balance": "0", "status": "Active"},
    {"id": "...", "number": "4000", "name": "Sales Revenue", "type": "Revenue", "balance": "0", "status": "Active"},
    {"id": "...", "number": "4010", "name": "Service Revenue", "type": "Revenue", "balance": "0", "status": "Active"},
    {"id": "...", "number": "4020", "name": "Interest Revenue", "type": "Revenue", "balance": "0", "status": "Active"},
    {"id": "...", "number": "5000", "name": "Cost of Goods Sold", "type": "Expense", "balance": "0", "status": "Active"},
    {"id": "...", "number": "5010", "name": "Salaries Expense", "type": "Expense", "balance": "0", "status": "Active"},
    {"id": "...", "number": "5020", "name": "Rent Expense", "type": "Expense", "balance": "0", "status": "Active"},
    {"id": "...", "number": "5030", "name": "Utilities Expense", "type": "Expense", "balance": "0", "status": "Active"},
    {"id": "...", "number": "5040", "name": "Office Supplies", "type": "Expense", "balance": "0", "status": "Active"},
    {"id": "...", "number": "5050", "name": "Depreciation Expense", "type": "Expense", "balance": "0", "status": "Active"}
  ],
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

> **Note:** You will need the `id` values for Cash (1000) and Sales Revenue
> (4000) in the next step. Copy them from the response.

---

## 4. Create Your First Journal Entry

Record a simple revenue transaction: receive $1,000 in cash for a sale.

**Double-entry logic:**

| Account | Type | Normal Balance | Entry | Effect |
|---|---|---|---|---|
| Cash (1000) | Asset | Debit | Debit $1,000 | Increases cash |
| Sales Revenue (4000) | Revenue | Credit | Credit $1,000 | Increases revenue |

Debits ($1,000) = Credits ($1,000). The books balance.

### Via the UI

1. Click **Journal Entry** in the sidebar.
2. Fill in:
   - **Description**: `Cash sale — services rendered`
3. Add line 1:
   - **Account**: Cash (1000)
   - **Entry Type**: Debit
   - **Amount**: `1000.00`
4. Add line 2:
   - **Account**: Sales Revenue (4000)
   - **Entry Type**: Credit
   - **Amount**: `1000.00`
5. Verify **Total Debits = Total Credits = $1,000.00**.
6. Click **Save Transaction**.

### Via the API

Replace `<cash-id>` and `<revenue-id>` with the UUIDs from Step 3.

```bash
curl -X POST http://localhost:8080/api/v1/transactions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "description": "Cash sale — services rendered",
    "entries": [
      {
        "account_id": "<cash-id>",
        "amount": "1000.00",
        "entry_type": "debit",
        "description": "Cash received"
      },
      {
        "account_id": "<revenue-id>",
        "amount": "1000.00",
        "entry_type": "credit",
        "description": "Service revenue earned"
      }
    ]
  }'
```

**Expected output:**

```json
{
  "success": true,
  "data": {
    "id": "f7e6d5c4-b3a2-1098-7654-3210fedcba98",
    "number": "TRX-0000001",
    "description": "Cash sale — services rendered",
    "date": "2026-07-01T12:00:05.000Z",
    "status": "Posted",
    "total_amount": "1000.00"
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

---

## 5. View the Trial Balance

The trial balance lists all accounts and their balances. Debit balances and
credit balances should be equal.

### Via the API

```bash
curl http://localhost:8080/api/v1/reports/trial_balance \
  -H "Authorization: Bearer $TOKEN"
```

**Expected output (abbreviated):**

```json
{
  "success": true,
  "data": {
    "report_type": "trial_balance",
    "balances": [
      {"account_id": "<cash-id>", "balance": "1000.00"},
      {"account_id": "<revenue-id>", "balance": "-1000.00"}
    ]
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

> Cash shows a positive (debit) balance of $1,000. Sales Revenue shows a
> negative (credit) balance of $1,000. The sum is zero — the books balance.

---

## 6. Create an Invoice

Create a customer invoice for $2,500.

### Via the API

```bash
curl -X POST http://localhost:8080/api/v1/invoices \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "customer_name": "Acme Corporation",
    "customer_email": "billing@acme.com",
    "items": [
      {"description": "Consulting services — July", "quantity": 1, "unit_price": "2000.00"},
      {"description": "Expense reimbursement", "quantity": 1, "unit_price": "500.00"}
    ],
    "due_date": "2026-08-01",
    "notes": "Net 30"
  }'
```

**Expected output:**

```json
{
  "success": true,
  "data": {
    "task_id": "b8c7d6e5-a4b3-2109-8765-4321fedcba09",
    "status": "submitted",
    "message": "Invoice creation submitted"
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

The invoice is processed asynchronously by the **InvoiceAgent**. To view
created invoices:

```bash
curl http://localhost:8080/api/v1/invoices \
  -H "Authorization: Bearer $TOKEN"
```

---

## 7. Upload a Receipt (if AI is available)

If the AI pipeline (Mistral OCR4 or local GGUF models) is configured, you can
upload a receipt photo or PDF and NexusLedger will automatically extract the
vendor, amount, date, and line items, then create a matching transaction.

> **If AI is not available:** Skip this step. All core accounting features work
> without AI. The AI features degrade gracefully.

### Via the UI

1. Navigate to **Documents** in the sidebar.
2. Drag and drop a receipt image (PNG, JPG) or PDF onto the upload zone.
3. The system will:
   - Run OCR to extract text from the receipt.
   - Use the AI extraction model to parse vendor, date, amount, and line items.
   - Auto-create a transaction (e.g., Debit Office Supplies, Credit Cash).
4. Review the extracted data and confirm the transaction.

### Via the API (WebSocket)

You can also use the conversational WebSocket interface:

```bash
# Connect to the chat WebSocket:
wscat -c "ws://localhost:8080/ws/chat?token=$TOKEN"

# Then type:
# "log a receipt from Staples for $45.99"
```

**Expected response:**

```json
{
  "type": "response",
  "intent": "record_receipt",
  "message": "I've recorded a receipt from Staples for $45.99. Debited Office Supplies (5040) and credited Cash (1000). Transaction TRX-0000002 has been posted."
}
```

---

## 8. View the Balance Sheet

The balance sheet shows Assets = Liabilities + Equity.

### Via the API

```bash
curl http://localhost:8080/api/v1/reports/balance_sheet \
  -H "Authorization: Bearer $TOKEN"
```

**Expected output:**

```json
{
  "success": true,
  "data": {
    "report_type": "balance_sheet",
    "assets": "1000.00",
    "liabilities": "0",
    "equity": "0",
    "total_assets": "1000.00",
    "total_liabilities_plus_equity": "1000.00"
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

> Total Assets ($1,000) = Total Liabilities + Equity ($1,000). The balance
> sheet balances.

### Other reports available

| Report | Endpoint |
|---|---|
| Trial Balance | `GET /api/v1/reports/trial_balance` |
| Balance Sheet | `GET /api/v1/reports/balance_sheet` |
| Income Statement (P&L) | `GET /api/v1/reports/income_statement` |
| Cash Flow Statement | `GET /api/v1/reports/cash-flow` |
| AR Aging | `GET /api/v1/reports/ar-aging` |

---

## Next Steps

Congratulations! You have created your first transaction, generated an invoice,
and viewed financial reports in NexusLedger. Here are some things to try next:

- **Import bank statements** — `POST /api/v1/import/csv` with a CSV file
- **Export data** — `GET /api/v1/export/csv` or `GET /api/v1/export/ofx`
- **Set up a budget** — `POST /api/v1/budgets` then `GET /api/v1/budgets/variance`
- **Track a fixed asset** — `POST /api/v1/assets` then `POST /api/v1/assets/depreciation`
- **Convert currencies** — `POST /api/v1/convert`
- **Pay a vendor bill** — `POST /api/v1/ap/bills` then `POST /api/v1/ap/bills/:id/pay`
- **Explore the API** — See the [API Reference](../api/reference.md) for every endpoint
- **Developer setup** — See [Developer Setup](../developer/setup.md) to contribute
