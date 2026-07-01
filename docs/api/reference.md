# API Reference

Complete reference for the NexusLedger REST API. The API server runs on
**port 8080** by default (override with the `API_PORT` environment variable).

## Table of Contents

- [Authentication](#authentication)
- [Auth](#auth)
  - [POST /api/auth/register](#post-apiauthregister)
  - [POST /api/auth/login](#post-apiauthlogin)
  - [POST /api/auth/refresh](#post-apiauthrefresh)
- [Accounts](#accounts)
  - [GET /api/v1/accounts](#get-apiv1accounts)
  - [GET /api/v1/accounts/:id](#get-apiv1accountsid)
- [Transactions](#transactions)
  - [GET /api/v1/transactions](#get-apiv1transactions)
  - [POST /api/v1/transactions](#post-apiv1transactions)
- [Invoices](#invoices)
  - [GET /api/v1/invoices](#get-apiv1invoices)
  - [POST /api/v1/invoices](#post-apiv1invoices)
- [Accounts Payable](#accounts-payable)
  - [GET /api/v1/ap/bills](#get-apiv1apbills)
  - [POST /api/v1/ap/bills](#post-apiv1apbills)
  - [POST /api/v1/ap/bills/:id/pay](#post-apiv1apbillsidpay)
  - [GET /api/v1/ap/outstanding](#get-apiv1apoutstanding)
- [Reports](#reports)
  - [GET /api/v1/reports/:type](#get-apiv1reportstype)
  - [GET /api/v1/reports/ar-aging](#get-apiv1reportsar-aging)
- [Import / Export](#import--export)
  - [POST /api/v1/import/csv](#post-apiv1importcsv)
  - [GET /api/v1/export/csv](#get-apiv1exportcsv)
  - [GET /api/v1/export/ofx](#get-apiv1exportofx)
- [Budget](#budget)
  - [POST /api/v1/budgets](#post-apiv1budgets)
  - [GET /api/v1/budgets/variance](#get-apiv1budgetsvariance)
- [Fixed Assets](#fixed-assets)
  - [GET /api/v1/assets](#get-apiv1assets)
  - [POST /api/v1/assets](#post-apiv1assets)
  - [POST /api/v1/assets/depreciation](#post-apiv1assetsdepreciation)
- [Multi-Currency](#multi-currency)
  - [GET /api/v1/exchange-rates](#get-apiv1exchange-rates)
  - [POST /api/v1/exchange-rates](#post-apiv1exchange-rates)
  - [POST /api/v1/convert](#post-apiv1convert)
- [Users](#users)
  - [GET /api/v1/users](#get-apiv1users)
  - [POST /api/v1/users/:id/role](#post-apiv1usersidrole)
- [Edge / Sync](#edge--sync)
  - [GET /api/v1/edge/status](#get-apiv1edgestatus)
  - [POST /api/v1/edge/sync](#post-apiv1edgesync)
- [Health](#health)
  - [GET /health](#get-health)
  - [GET /ready](#get-ready)
  - [GET /metrics](#get-metrics)
- [Response Format](#response-format)
- [Role Requirements](#role-requirements)

---

## Authentication

All endpoints except `/api/auth/*`, `/health`, `/ready`, `/metrics`, and
`/ws/chat` require a valid JWT in the `Authorization` header:

```
Authorization: Bearer <access_token>
```

Access tokens expire after 30 minutes. Use the refresh token to obtain a new
access token without re-entering credentials (see
[POST /api/auth/refresh](#post-apiauthrefresh)).

WebSocket connections authenticate via a `?token=<access_token>` query
parameter: `ws://localhost:8080/ws/chat?token=eyJ...`.

---

## Auth

### POST /api/auth/register

Create a new user account. The first registered user is assigned the Admin
role; subsequent users receive the User role.

**Auth:** None (public endpoint)

**Request body:**

```json
{
  "username": "admin",
  "email": "admin@example.com",
  "password": "SecurePass123",
  "display_name": "Admin User"
}
```

| Field | Type | Required | Validation |
|---|---|---|---|
| `username` | string | Yes | Non-empty, must be unique |
| `email` | string | Yes | Valid email format, must be unique |
| `password` | string | Yes | 8–128 chars, at least one letter and one number |
| `display_name` | string | No | Defaults to username |

**curl:**

```bash
curl -X POST http://localhost:8080/api/auth/register \
  -H "Content-Type: application/json" \
  -d '{
    "username": "admin",
    "email": "admin@example.com",
    "password": "SecurePass123"
  }'
```

**Response (201 Created):**

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
    "request_id": "uuid",
    "timestamp": "2026-07-01T12:00:00.000Z",
    "api_version": "v1"
  }
}
```

**Errors:**

| Status | Condition |
|---|---|
| 400 | Missing fields, weak password, invalid email, duplicate username/email |

---

### POST /api/auth/login

Authenticate with username and password. Returns JWT access and refresh tokens.

**Auth:** None (public endpoint)

**Request body:**

```json
{
  "username": "admin",
  "password": "SecurePass123"
}
```

**curl:**

```bash
curl -X POST http://localhost:8080/api/auth/login \
  -H "Content-Type: application/json" \
  -d '{
    "username": "admin",
    "password": "SecurePass123"
  }'
```

**Response (200 OK):**

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
    "request_id": "uuid",
    "timestamp": "2026-07-01T12:00:00.000Z",
    "api_version": "v1"
  }
}
```

**Errors:**

| Status | Condition |
|---|---|
| 401 | Invalid username or password, account deactivated |

> The login endpoint uses a timing-attack mitigation: when the username is not
> found, a dummy argon2id hash verification is performed so that "user not
> found" and "wrong password" take the same time.

---

### POST /api/auth/refresh

Exchange a refresh token for a new access token and a rotated refresh token.

**Auth:** None (refresh token in body)

**Request body:**

```json
{
  "refresh_token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."
}
```

**curl:**

```bash
curl -X POST http://localhost:8080/api/auth/refresh \
  -H "Content-Type: application/json" \
  -d '{
    "refresh_token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."
  }'
```

**Response (200 OK):**

```json
{
  "success": true,
  "data": {
    "user_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
    "username": "admin",
    "role": "user",
    "access_token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...new...",
    "refresh_token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...rotated...",
    "expires_in": 1800
  },
  "metadata": {
    "request_id": "uuid",
    "timestamp": "2026-07-01T12:00:00.000Z",
    "api_version": "v1"
  }
}
```

**Errors:**

| Status | Condition |
|---|---|
| 401 | Invalid, expired, or wrong token_type (access tokens are rejected) |

> The refresh handler re-fetches the user's current role from the database,
> so role changes take effect on the next token refresh.

---

## Accounts

### GET /api/v1/accounts

List all accounts in the chart of accounts.

**Auth:** Viewer+

**curl:**

```bash
curl http://localhost:8080/api/v1/accounts \
  -H "Authorization: Bearer $TOKEN"
```

**Response (200 OK):**

```json
{
  "success": true,
  "data": [
    {
      "id": "uuid-here",
      "number": "1000",
      "name": "Cash",
      "type": "Asset",
      "balance": "1000.00",
      "status": "Active"
    }
  ],
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

---

### GET /api/v1/accounts/:id

Retrieve a single account by its UUID.

**Auth:** Viewer+

| Parameter | Location | Description |
|---|---|---|
| `id` | path | Account UUID |

**curl:**

```bash
curl http://localhost:8080/api/v1/accounts/a1b2c3d4-e5f6-7890-abcd-ef1234567890 \
  -H "Authorization: Bearer $TOKEN"
```

**Response (200 OK):**

```json
{
  "success": true,
  "data": {
    "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
    "number": "1000",
    "name": "Cash",
    "type": "Asset",
    "balance": "1000.00",
    "status": "Active"
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

**Errors:**

| Status | Condition |
|---|---|
| 404 | Account not found |

---

## Transactions

### GET /api/v1/transactions

List all transactions with pagination.

**Auth:** Viewer+

| Parameter | Location | Default | Description |
|---|---|---|---|
| `limit` | query | 100 | Maximum number of transactions to return |
| `offset` | query | 0 | Number of transactions to skip |

**curl:**

```bash
curl "http://localhost:8080/api/v1/transactions?limit=10&offset=0" \
  -H "Authorization: Bearer $TOKEN"
```

**Response (200 OK):**

```json
{
  "success": true,
  "data": {
    "data": [
      {
        "id": "uuid",
        "number": "TRX-0000001",
        "description": "Cash sale — services rendered",
        "date": "2026-07-01T12:00:05.000Z",
        "status": "Posted",
        "total_amount": "1000.00",
        "entries": [
          {
            "account_id": "uuid",
            "amount": "1000.00",
            "entry_type": "Debit"
          },
          {
            "account_id": "uuid",
            "amount": "1000.00",
            "entry_type": "Credit"
          }
        ]
      }
    ],
    "pagination": { "total": 1, "limit": 10, "offset": 0 }
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

---

### POST /api/v1/transactions

Create a new double-entry transaction. Debits and credits must balance.

**Auth:** User+

**Request body:**

```json
{
  "description": "Cash sale — services rendered",
  "entries": [
    {
      "account_id": "uuid-of-cash-account",
      "amount": "1000.00",
      "entry_type": "debit",
      "description": "Cash received"
    },
    {
      "account_id": "uuid-of-revenue-account",
      "amount": "1000.00",
      "entry_type": "credit",
      "description": "Service revenue earned"
    }
  ]
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `description` | string | No | Transaction description (defaults to "API transaction") |
| `entries` | array | Yes | At least one entry required |
| `entries[].account_id` | string (UUID) | Yes | Account UUID from the chart of accounts |
| `entries[].amount` | string | Yes | Decimal amount as string (e.g., "1000.00") |
| `entries[].entry_type` | string | Yes | "debit" or "credit" |
| `entries[].description` | string | No | Per-entry description |

**curl:**

```bash
curl -X POST http://localhost:8080/api/v1/transactions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "description": "Cash sale — services rendered",
    "entries": [
      {"account_id": "<cash-id>", "amount": "1000.00", "entry_type": "debit"},
      {"account_id": "<revenue-id>", "amount": "1000.00", "entry_type": "credit"}
    ]
  }'
```

**Response (200 OK):**

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

**Errors:**

| Status | Condition |
|---|---|
| 400 | No entries provided |
| 500 | Ledger processing error (e.g., unbalanced entries) |

---

## Invoices

### GET /api/v1/invoices

List all invoice-type transactions with pagination.

**Auth:** Viewer+

| Parameter | Location | Default | Description |
|---|---|---|---|
| `limit` | query | 100 | Maximum invoices to return |
| `offset` | query | 0 | Number of invoices to skip |

**curl:**

```bash
curl http://localhost:8080/api/v1/invoices \
  -H "Authorization: Bearer $TOKEN"
```

**Response (200 OK):**

```json
{
  "success": true,
  "data": {
    "data": [
      {
        "id": "uuid",
        "number": "TRX-0000002",
        "description": "Invoice: Acme Corporation",
        "date": "2026-07-01T12:00:10.000Z",
        "status": "Posted",
        "total_amount": "2500.00",
        "entries": [ ... ]
      }
    ],
    "pagination": { "total": 1, "limit": 100, "offset": 0 }
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

---

### POST /api/v1/invoices

Create a customer invoice. The request is submitted to the InvoiceAgent for
asynchronous processing.

**Auth:** User+

**Request body:**

```json
{
  "customer_name": "Acme Corporation",
  "customer_email": "billing@acme.com",
  "items": [
    {"description": "Consulting services — July", "quantity": 1, "unit_price": "2000.00"},
    {"description": "Expense reimbursement", "quantity": 1, "unit_price": "500.00"}
  ],
  "due_date": "2026-08-01",
  "notes": "Net 30"
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `customer_name` | string | No | Customer name (defaults to "Customer") |
| `customer_email` | string | No | Customer email |
| `items` | array | No | Line items with description, quantity, unit_price |
| `due_date` | string | No | ISO date (YYYY-MM-DD) |
| `notes` | string | No | Free-text notes |

**curl:**

```bash
curl -X POST http://localhost:8080/api/v1/invoices \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "customer_name": "Acme Corporation",
    "customer_email": "billing@acme.com",
    "items": [{"description": "Consulting", "quantity": 1, "unit_price": "2000.00"}],
    "due_date": "2026-08-01"
  }'
```

**Response (200 OK):**

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

---

## Accounts Payable

### GET /api/v1/ap/bills

List all outstanding AP bills (transactions with descriptions starting with "AP Bill:").

**Auth:** Viewer+

**curl:**

```bash
curl http://localhost:8080/api/v1/ap/bills \
  -H "Authorization: Bearer $TOKEN"
```

**Response (200 OK):**

```json
{
  "success": true,
  "data": {
    "bills": [
      {
        "id": "uuid",
        "number": "AP-BILL-a1b2c3d4",
        "description": "AP Bill: BILL-a1b2c3d4 — Office supplies [Vendor: Staples]",
        "amount": "150.00",
        "date": "2026-07-01T12:00:00.000Z",
        "status": "Posted"
      }
    ]
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

---

### POST /api/v1/ap/bills

Create a vendor bill. Posts a debit to the expense account and a credit to
Accounts Payable (2000).

**Auth:** User+

**Request body:**

```json
{
  "vendor_name": "Staples",
  "description": "Office supplies",
  "amount": "150.00",
  "expense_account": "5040"
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `vendor_name` | string | No | Vendor name (defaults to "Unknown Vendor") |
| `description` | string | No | Bill description |
| `amount` | string | Yes | Positive decimal amount |
| `expense_account` | string | No | Account number to debit (defaults to "5000" — COGS) |

**curl:**

```bash
curl -X POST http://localhost:8080/api/v1/ap/bills \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "vendor_name": "Staples",
    "description": "Office supplies",
    "amount": "150.00",
    "expense_account": "5040"
  }'
```

**Response (201 Created):**

```json
{
  "success": true,
  "data": {
    "transaction_id": "uuid",
    "bill_number": "BILL-a1b2c3d4",
    "vendor": "Staples",
    "amount": "150.00",
    "status": "Approved"
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

**Errors:**

| Status | Condition |
|---|---|
| 400 | Invalid or non-positive amount |
| 500 | AP or Expense account not found in chart of accounts |

---

### POST /api/v1/ap/bills/:id/pay

Pay an outstanding AP bill. Posts a debit to Accounts Payable (2000) and a
credit to Cash (1000) for the full bill amount.

**Auth:** User+

| Parameter | Location | Description |
|---|---|---|
| `id` | path | Bill transaction UUID |

**curl:**

```bash
curl -X POST http://localhost:8080/api/v1/ap/bills/a1b2c3d4-e5f6-7890-abcd-ef1234567890/pay \
  -H "Authorization: Bearer $TOKEN"
```

**Response (200 OK):**

```json
{
  "success": true,
  "data": {
    "payment_transaction_id": "uuid",
    "bill_transaction_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
    "amount_paid": "150.00",
    "status": "Paid"
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

**Errors:**

| Status | Condition |
|---|---|
| 404 | Bill transaction not found |
| 500 | AP or Cash account not found |

---

### GET /api/v1/ap/outstanding

Get the total outstanding Accounts Payable balance.

**Auth:** Viewer+

**curl:**

```bash
curl http://localhost:8080/api/v1/ap/outstanding \
  -H "Authorization: Bearer $TOKEN"
```

**Response (200 OK):**

```json
{
  "success": true,
  "data": {
    "outstanding_ap": "150.00"
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

---

## Reports

### GET /api/v1/reports/:type

Generate a financial report. Supported types: `trial_balance`, `balance_sheet`,
`income_statement`.

**Auth:** Viewer+

| Parameter | Location | Description |
|---|---|---|
| `type` | path | Report type: `trial_balance`, `balance_sheet`, `income_statement` |

**curl (trial balance):**

```bash
curl http://localhost:8080/api/v1/reports/trial_balance \
  -H "Authorization: Bearer $TOKEN"
```

**Response (trial_balance):**

```json
{
  "success": true,
  "data": {
    "report_type": "trial_balance",
    "balances": [
      {"account_id": "uuid", "balance": "1000.00"},
      {"account_id": "uuid", "balance": "-1000.00"}
    ]
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

**curl (balance sheet):**

```bash
curl http://localhost:8080/api/v1/reports/balance_sheet \
  -H "Authorization: Bearer $TOKEN"
```

**Response (balance_sheet):**

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

**curl (income statement):**

```bash
curl http://localhost:8080/api/v1/reports/income_statement \
  -H "Authorization: Bearer $TOKEN"
```

**Response (income_statement):**

```json
{
  "success": true,
  "data": {
    "report_type": "income_statement",
    "revenue": "1000.00",
    "expenses": "0",
    "net_income": "1000.00"
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

> The income statement covers the trailing 365-day period.

---

### GET /api/v1/reports/ar-aging

Generate an Accounts Receivable aging report with 4 buckets (current, 31-60,
61-90, 90+ days).

**Auth:** Viewer+

| Parameter | Location | Required | Description |
|---|---|---|---|
| `as_of_date` | query | No | As-of date (YYYY-MM-DD); defaults to now |

**curl:**

```bash
curl "http://localhost:8080/api/v1/reports/ar-aging?as_of_date=2026-07-01" \
  -H "Authorization: Bearer $TOKEN"
```

**Response (200 OK):**

```json
{
  "success": true,
  "data": {
    "current": {
      "amount": "2500.00",
      "count": 1,
      "invoices": [
        {
          "invoice_id": "uuid",
          "invoice_number": "TRX-0000002",
          "customer": "Acme Corporation",
          "amount": "2500.00",
          "days_outstanding": 5
        }
      ]
    },
    "days_31_60": { "amount": "0", "count": 0 },
    "days_61_90": { "amount": "0", "count": 0 },
    "days_90_plus": { "amount": "0", "count": 0 },
    "total_outstanding": "2500.00"
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

---

## Import / Export

### POST /api/v1/import/csv

Import transactions from CSV content.

**Auth:** User+

**Request body:**

```json
{
  "csv": "date,description,amount,account_code,entry_type\n2026-07-01,Office Supplies,-150.00,5040,Debit\n2026-07-01,Cash Payment,150.00,1000,Credit"
}
```

**CSV column format:**

```csv
date,description,amount,account_code,entry_type
2026-07-01,Office Supplies Purchase,-150.00,5040,Debit
2026-07-01,Customer Payment,2000.00,1010,Debit
```

| Column | Description |
|---|---|
| `date` | Transaction date (YYYY-MM-DD) |
| `description` | Transaction description |
| `amount` | Amount (positive for inflow, negative for outflow) |
| `account_code` | Chart of accounts code to post to |
| `entry_type` | "Debit" or "Credit" |

**curl:**

```bash
curl -X POST http://localhost:8080/api/v1/import/csv \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "csv": "date,description,amount,account_code,entry_type\n2026-07-01,Test Sale,500.00,1000,Debit\n2026-07-01,Test Sale,500.00,4000,Credit"
  }'
```

**Response (201 Created):**

```json
{
  "success": true,
  "data": {
    "imported": 1,
    "transactions": [
      {
        "id": "uuid",
        "number": "TRX-0000003",
        "description": "Test Sale",
        "amount": "500.00"
      }
    ]
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

**Errors:**

| Status | Condition |
|---|---|
| 400 | Missing or empty CSV content, parse error, unbalanced entries |

---

### GET /api/v1/export/csv

Export all transactions to CSV format.

**Auth:** Viewer+

| Parameter | Location | Required | Description |
|---|---|---|---|
| `start_date` | query | No | Filter start date (YYYY-MM-DD) |
| `end_date` | query | No | Filter end date (YYYY-MM-DD) |

**curl:**

```bash
curl "http://localhost:8080/api/v1/export/csv?start_date=2026-01-01&end_date=2026-12-31" \
  -H "Authorization: Bearer $TOKEN" \
  -o export.csv
```

**Response (200 OK):**

```
Content-Type: text/csv

Account Number,Account Name,Date,Description,Debit,Credit
1000,Cash,2026-07-01,Cash sale,1000.00,
4000,Sales Revenue,2026-07-01,Cash sale,,1000.00
```

---

### GET /api/v1/export/ofx

Export transactions in OFX (Open Financial Exchange) format for import into
personal finance software (Quicken, GnuCash, etc.).

**Auth:** Viewer+

| Parameter | Location | Required | Description |
|---|---|---|---|
| `start_date` | query | No | Filter start date (YYYY-MM-DD) |
| `end_date` | query | No | Filter end date (YYYY-MM-DD) |

**curl:**

```bash
curl "http://localhost:8080/api/v1/export/ofx" \
  -H "Authorization: Bearer $TOKEN" \
  -o export.ofx
```

**Response (200 OK):**

```
Content-Type: application/x-ofx

OFXHEADER:100
DATA:OFXSGML
...
```

---

## Budget

### POST /api/v1/budgets

Create a budget for a specific account.

**Auth:** User+

**Request body:**

```json
{
  "account_number": "5010",
  "name": "July Salaries Budget",
  "amount": "20000.00"
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `account_number` | string | Yes | Chart of accounts code (e.g., "5010") |
| `name` | string | No | Budget name (defaults to "Budget") |
| `amount` | string | Yes | Budgeted amount as decimal string |

**curl:**

```bash
curl -X POST http://localhost:8080/api/v1/budgets \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "account_number": "5010",
    "name": "July Salaries Budget",
    "amount": "20000.00"
  }'
```

**Response (200 OK):**

```json
{
  "success": true,
  "data": {
    "budget_id": "uuid",
    "account_number": "5010",
    "amount": "20000.00"
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

**Errors:**

| Status | Condition |
|---|---|
| 400 | Missing account_number or invalid amount |
| 404 | Account not found |

---

### GET /api/v1/budgets/variance

Generate a budget variance report comparing actual balances to budgeted amounts
for expense accounts.

**Auth:** Viewer+

**curl:**

```bash
curl http://localhost:8080/api/v1/budgets/variance \
  -H "Authorization: Bearer $TOKEN"
```

**Response (200 OK):**

```json
{
  "success": true,
  "data": {
    "lines": [
      {
        "account_number": "5000",
        "account_name": "Cost of Goods Sold",
        "budgeted": "0",
        "actual": "500.00",
        "variance": "500.00"
      },
      {
        "account_number": "5010",
        "account_name": "Salaries Expense",
        "budgeted": "0",
        "actual": "20000.00",
        "variance": "20000.00"
      }
    ]
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

---

## Fixed Assets

### GET /api/v1/assets

List all registered fixed assets.

**Auth:** Viewer+

**curl:**

```bash
curl http://localhost:8080/api/v1/assets \
  -H "Authorization: Bearer $TOKEN"
```

**Response (200 OK):**

```json
{
  "success": true,
  "data": {
    "assets": []
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

---

### POST /api/v1/assets

Register a new fixed asset.

**Auth:** User+

**Request body:**

```json
{
  "name": "Delivery Van",
  "cost": "35000.00",
  "salvage_value": "5000.00",
  "useful_life_months": "60"
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `name` | string | No | Asset name (defaults to "Asset") |
| `cost` | string | Yes | Acquisition cost as decimal string |
| `salvage_value` | string | No | Salvage/residual value (defaults to "0") |
| `useful_life_months` | string | No | Useful life in months (defaults to "60") |

**curl:**

```bash
curl -X POST http://localhost:8080/api/v1/assets \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "name": "Delivery Van",
    "cost": "35000.00",
    "salvage_value": "5000.00",
    "useful_life_months": "60"
  }'
```

**Response (200 OK):**

```json
{
  "success": true,
  "data": {
    "asset_id": "uuid",
    "name": "Delivery Van",
    "cost": "35000.00",
    "salvage_value": "5000.00",
    "useful_life_months": 60,
    "monthly_depreciation": "500.00"
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

---

### POST /api/v1/assets/depreciation

Compute monthly depreciation for an asset.

**Auth:** User+

**Request body:**

```json
{
  "cost": "35000.00",
  "salvage_value": "5000.00",
  "useful_life_months": "60"
}
```

**curl:**

```bash
curl -X POST http://localhost:8080/api/v1/assets/depreciation \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "cost": "35000.00",
    "salvage_value": "5000.00",
    "useful_life_months": "60"
  }'
```

**Response (200 OK):**

```json
{
  "success": true,
  "data": {
    "cost": "35000.00",
    "salvage_value": "5000.00",
    "useful_life_months": 60,
    "depreciation_method": "StraightLine",
    "monthly_depreciation": "500.00"
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

---

## Multi-Currency

### GET /api/v1/exchange-rates

List current exchange rates. The base currency is USD.

**Auth:** Viewer+

**curl:**

```bash
curl http://localhost:8080/api/v1/exchange-rates \
  -H "Authorization: Bearer $TOKEN"
```

**Response (200 OK):**

```json
{
  "success": true,
  "data": {
    "base_currency": "USD",
    "rates": [
      {"currency": "EUR", "rate": "1.09"},
      {"currency": "GBP", "rate": "1.27"}
    ]
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

---

### POST /api/v1/exchange-rates

Set or update an exchange rate for a currency.

**Auth:** User+

**Request body:**

```json
{
  "currency": "EUR",
  "rate": "1.08"
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `currency` | string | Yes | Currency code (e.g., "EUR", "GBP") — auto-uppercased |
| `rate` | string | Yes | Exchange rate as decimal string |

**curl:**

```bash
curl -X POST http://localhost:8080/api/v1/exchange-rates \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{"currency": "EUR", "rate": "1.08"}'
```

**Response (200 OK):**

```json
{
  "success": true,
  "data": {
    "currency": "EUR",
    "rate": "1.08"
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

---

### POST /api/v1/convert

Convert an amount from one currency to another via the base currency (USD).

**Auth:** Viewer+

**Request body:**

```json
{
  "amount": "1000.00",
  "from": "EUR",
  "to": "USD"
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `amount` | string | Yes | Amount to convert |
| `from` | string | No | Source currency (defaults to "USD") |
| `to` | string | No | Target currency (defaults to "USD") |

**curl:**

```bash
curl -X POST http://localhost:8080/api/v1/convert \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{"amount": "1000.00", "from": "EUR", "to": "USD"}'
```

**Response (200 OK):**

```json
{
  "success": true,
  "data": {
    "from": "EUR",
    "to": "USD",
    "amount": "1000.00",
    "converted": "1080.00",
    "rate": "1080.00"
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

**Errors:**

| Status | Condition |
|---|---|
| 400 | Invalid amount or unknown currency code |

---

## Users

### GET /api/v1/users

List all registered users with their roles and status.

**Auth:** Admin only

**curl:**

```bash
curl http://localhost:8080/api/v1/users \
  -H "Authorization: Bearer $ADMIN_TOKEN"
```

**Response (200 OK):**

```json
{
  "success": true,
  "data": {
    "users": [
      {
        "id": "uuid",
        "username": "admin",
        "email": "admin@example.com",
        "display_name": "Admin User",
        "role": "admin",
        "is_active": true,
        "last_login": "2026-07-01T12:00:00.000Z"
      }
    ]
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

---

### POST /api/v1/users/:id/role

Update a user's role. Only Admins can change roles.

**Auth:** Admin only

| Parameter | Location | Description |
|---|---|---|
| `id` | path | User UUID |

**Request body:**

```json
{
  "role": "manager"
}
```

| Value | Description |
|---|---|
| `admin` | Full access including user management |
| `manager` | All User permissions plus budget and asset management |
| `user` | Can create transactions and invoices; view reports |
| `viewer` | Read-only access |
| `guest` | No access (must register first) |

**curl:**

```bash
curl -X POST http://localhost:8080/api/v1/users/a1b2c3d4-e5f6-7890-abcd-ef1234567890/role \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -d '{"role": "manager"}'
```

**Response (200 OK):**

```json
{
  "success": true,
  "data": {
    "user_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
    "username": "jane",
    "role": "manager"
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

**Errors:**

| Status | Condition |
|---|---|
| 400 | Missing role or unknown role name |
| 404 | User not found |

---

## Edge / Sync

### GET /api/v1/edge/status

Get the current edge/sync status, including offline mode, last sync time, and
pending changes.

**Auth:** User+

**curl:**

```bash
curl http://localhost:8080/api/v1/edge/status \
  -H "Authorization: Bearer $TOKEN"
```

**Response (200 OK, edge enabled):**

```json
{
  "success": true,
  "data": {
    "enabled": true,
    "offline_mode": false,
    "is_online": true,
    "last_sync": "2026-07-01T12:00:00.000Z",
    "sync_in_progress": false,
    "pending_changes": 0,
    "storage_used_mb": 12,
    "storage_max_mb": 1024
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

**Response (200 OK, edge disabled):**

```json
{
  "success": true,
  "data": {
    "enabled": false,
    "offline_mode": false,
    "is_online": true,
    "last_sync": null,
    "sync_in_progress": false,
    "pending_changes": 0,
    "storage_used_mb": 0,
    "storage_max_mb": 1024
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

---

### POST /api/v1/edge/sync

Manually trigger a sync cycle. Pushes dirty local records to SurrealDB and
pulls remote changes to the local SQLite store.

**Auth:** User+

**curl:**

```bash
curl -X POST http://localhost:8080/api/v1/edge/sync \
  -H "Authorization: Bearer $TOKEN"
```

**Response (200 OK):**

```json
{
  "success": true,
  "data": {
    "synced": true,
    "errors": 0,
    "status": {
      "enabled": true,
      "offline_mode": false,
      "is_online": true,
      "last_sync": "2026-07-01T12:01:00.000Z",
      "sync_in_progress": false
    }
  },
  "metadata": { "request_id": "...", "timestamp": "...", "api_version": "v1" }
}
```

**Errors:**

| Status | Condition |
|---|---|
| 503 | Edge mode is not enabled |
| 500 | Sync failed |

---

## Health

### GET /health

Liveness probe. Always returns HTTP 200 as long as the process is alive. Does
not touch the database or any external dependency.

**Auth:** None (public)

**curl:**

```bash
curl http://localhost:8080/health
```

**Response (200 OK):**

```json
{
  "status": "ok",
  "uptime_seconds": 3600,
  "timestamp": "2026-07-01T12:00:00.000Z"
}
```

---

### GET /ready

Readiness probe. Returns 200 if the database is connected AND at least one
agent is registered. Returns 503 otherwise with a `reason` field.

**Auth:** None (public)

**curl:**

```bash
curl http://localhost:8080/ready
```

**Response (200 OK — ready):**

```json
{
  "status": "ready",
  "db": "connected",
  "agents": 9,
  "timestamp": "2026-07-01T12:00:00.000Z"
}
```

**Response (503 Service Unavailable — not ready):**

```json
{
  "status": "not_ready",
  "db": "disconnected",
  "agents": 0,
  "reason": "Database not connected and no agents registered",
  "timestamp": "2026-07-01T12:00:00.000Z"
}
```

---

### GET /metrics

Prometheus metrics endpoint. Returns text exposition format. Requires the
`MONITOR_ENABLE_PROMETHEUS=true` environment variable; returns 404 otherwise.

**Auth:** None (public)

**curl:**

```bash
MONITOR_ENABLE_PROMETHEUS=true curl http://localhost:8080/metrics
```

**Response (200 OK):**

```
Content-Type: text/plain; version=0.0.4; charset=utf-8

# HELP nexus_agents_total Total number of registered agents
# TYPE nexus_agents_total gauge
nexus_agents_total 9

# HELP nexus_agents_active Number of active agents
# TYPE nexus_agents_active gauge
nexus_agents_active 9

# HELP nexus_agents_idle Number of idle agents
# TYPE nexus_agents_idle gauge
nexus_agents_idle 9

# HELP nexus_agents_busy Number of busy agents
# TYPE nexus_agents_busy gauge
nexus_agents_busy 0

# HELP nexus_agents_error Number of agents in error state
# TYPE nexus_agents_error gauge
nexus_agents_error 0

# HELP nexus_tasks_processed_total Total number of tasks processed
# TYPE nexus_tasks_processed_total counter
nexus_tasks_processed_total 42

# HELP nexus_tasks_failed_total Total number of tasks that failed
# TYPE nexus_tasks_failed_total counter
nexus_tasks_failed_total 0

# HELP nexus_tasks_in_progress Number of tasks currently in progress
# TYPE nexus_tasks_in_progress gauge
nexus_tasks_in_progress 0

# HELP nexus_health_score System health score (0.0 to 1.0)
# TYPE nexus_health_score gauge
nexus_health_score 1

# HELP nexus_db_connected Whether the database is connected (1 = yes, 0 = no)
# TYPE nexus_db_connected gauge
nexus_db_connected 1

# HELP nexus_uptime_seconds Process uptime in seconds
# TYPE nexus_uptime_seconds gauge
nexus_uptime_seconds 3600
```

**Response (404 Not Found — Prometheus not enabled):**

```
Prometheus metrics not enabled
```

---

## Response Format

All API responses (except `/metrics` and file-export endpoints) use a standard
envelope:

```json
{
  "success": true,
  "data": { ... },
  "error": null,
  "metadata": {
    "request_id": "uuid",
    "timestamp": "2026-07-01T12:00:00.000Z",
    "api_version": "v1"
  }
}
```

Error responses set `success: false`, omit `data`, and populate `error`:

```json
{
  "success": false,
  "error": "Not found: Account 12345",
  "metadata": {
    "request_id": "uuid",
    "timestamp": "2026-07-01T12:00:00.000Z",
    "api_version": "v1"
  }
}
```

Every response includes the `X-Request-Id` and `X-Response-Time-Ms` headers.

---

## Role Requirements

| Role | Can Access |
|---|---|
| **Guest** | No API access (must register first) |
| **Viewer** | All GET endpoints (accounts, transactions, invoices, reports, AP, budgets, assets, exchange rates, edge status, convert) |
| **User** | All Viewer endpoints + POST endpoints (transactions, invoices, bills, payments, CSV import, budgets, assets, exchange-rate updates, edge sync) |
| **Manager** | All User endpoints (same access level in current implementation) |
| **Admin** | All endpoints including user management (list users, change roles) |

| Endpoint | Minimum Role |
|---|---|
| `POST /api/auth/register` | None (public) |
| `POST /api/auth/login` | None (public) |
| `POST /api/auth/refresh` | None (refresh token) |
| `GET /health` | None (public) |
| `GET /ready` | None (public) |
| `GET /metrics` | None (public) |
| `GET /api/v1/*` (read) | Viewer |
| `POST /api/v1/*` (write) | User |
| `GET /api/v1/users` | Admin |
| `POST /api/v1/users/:id/role` | Admin |
