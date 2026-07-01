# Financial Reports Guide

NexusLedger generates standard financial statements and operational reports.
This guide explains each report, what it shows, and how to generate it through
the API or the desktop UI.

---

## Table of Contents

1. [Trial Balance](#1-trial-balance)
2. [Balance Sheet](#2-balance-sheet)
3. [Income Statement (Profit & Loss)](#3-income-statement-profit--loss)
4. [Cash Flow Statement](#4-cash-flow-statement)
5. [Accounts Receivable Aging Report](#5-accounts-receivable-aging-report)
6. [Budget Variance Report](#6-budget-variance-report)
7. [Accounts Payable Outstanding Report](#7-accounts-payable-outstanding-report)
8. [Exporting Reports](#8-exporting-reports)
9. [Report Generation via AI Agents](#9-report-generation-via-ai-agents)

---

## Prerequisites

All API examples assume you have:

1. A running NexusLedger server (see [Installation](installation.md)).
2. An active JWT access token (see [Quick Start](quick-start.md)).

All requests require the Authorization header:

```
Authorization: Bearer <your-access-token>
```

---

## 1. Trial Balance

### What It Shows

The trial balance lists every account in the chart of accounts with its current
balance. It is the first report you should run to verify that your books are in
balance — total debits must equal total credits.

| Column | Description |
|---|---|
| Account ID | Unique identifier for the account |
| Account Code | The account number (e.g., 1000 for Cash) |
| Account Name | Human-readable name |
| Balance | Current balance as a decimal string |

### When to Use It

- At month-end close, before generating financial statements
- When investigating a posting error
- As a quick snapshot of all account balances

### How to Generate

**Via the API:**

```bash
curl http://localhost:8080/api/v1/reports/trial_balance \
  -H "Authorization: Bearer <access_token>"
```

**Response format:**

```json
{
  "success": true,
  "data": {
    "report_type": "trial_balance",
    "balances": [
      {
        "account_id": "uuid",
        "balance": "10000.00"
      },
      ...
    ]
  }
}
```

**Via the UI:**

1. Open the desktop app.
2. Navigate to the **Dashboard** or use the **Chat Sidebar**.
3. Type: "Show me the trial balance" — the ReportingAgent will generate it.

### Reading the Report

- **Positive balances** for Asset and Expense accounts are normal (debit
  balances).
- **Positive balances** for Liability, Equity, and Revenue accounts are normal
  (credit balances). Internally, these may show as negative in the raw output
  because NexusLedger uses signed balances — a negative number for a credit
  account is a normal credit balance.
- The sum of all balances should be **zero** if the books are in balance.

---

## 2. Balance Sheet

### What It Shows

The balance sheet is a snapshot of your business's financial position at a
point in time. It follows the fundamental accounting equation:

```
Assets = Liabilities + Equity
```

| Field | Description |
|---|---|
| `assets` | Total value of all asset accounts (cash, bank, A/R, inventory, fixed assets) |
| `liabilities` | Total value of all liability accounts (A/P, loans, accrued expenses) |
| `equity` | Total value of all equity accounts (owner's equity, retained earnings) |
| `total_assets` | Sum of all asset balances |
| `total_liabilities_plus_equity` | Sum of liabilities + equity — must equal `total_assets` |

### When to Use It

- End of month, quarter, or year reporting
- Applying for a loan or line of credit
- Sharing financial position with investors or partners
- Verifying that Assets = Liabilities + Equity after recording transactions

### How to Generate

**Via the API:**

```bash
curl http://localhost:8080/api/v1/reports/balance_sheet \
  -H "Authorization: Bearer <access_token>"
```

**Response format:**

```json
{
  "success": true,
  "data": {
    "report_type": "balance_sheet",
    "assets": "25000.00",
    "liabilities": "5000.00",
    "equity": "20000.00",
    "total_assets": "25000.00",
    "total_liabilities_plus_equity": "25000.00"
  }
}
```

**Via the UI:**

Use the **Chat Sidebar** and type: "Generate a balance sheet" or "Show me the
balance sheet."

### Verifying It Balances

Check that `total_assets` equals `total_liabilities_plus_equity`. If they do
not match, there is a posting error — run the [Trial Balance](#1-trial-balance)
to identify which account is off.

---

## 3. Income Statement (Profit & Loss)

### What It Shows

The income statement (also called Profit & Loss or P&L) shows your business's
financial performance over a period of time:

```
Revenue - Expenses = Net Income
```

| Field | Description |
|---|---|
| `revenue` | Total revenue earned during the period (sales, services, interest) |
| `expenses` | Total expenses incurred during the period (COGS, salaries, rent, utilities) |
| `net_income` | Revenue minus expenses — positive means profit, negative means loss |

### When to Use It

- Monthly, quarterly, and annual financial reporting
- Tracking profitability trends
- Tax preparation
- Budgeting and forecasting

### How to Generate

**Via the API:**

```bash
curl http://localhost:8080/api/v1/reports/income_statement \
  -H "Authorization: Bearer <access_token>"
```

The report defaults to the trailing 365-day period.

**Response format:**

```json
{
  "success": true,
  "data": {
    "report_type": "income_statement",
    "revenue": "50000.00",
    "expenses": "35000.00",
    "net_income": "15000.00"
  }
}
```

**Via the UI:**

Use the **Chat Sidebar** and type: "Show me the income statement" or "What is
my profit and loss?"

### Interpreting Results

- **Positive net income**: Your business is profitable for the period.
- **Negative net income**: Your business is operating at a loss.
- Compare across periods to identify trends (revenue growth, cost
  increases, margin changes).

---

## 4. Cash Flow Statement

### What It Shows

The cash flow statement tracks how cash moves through your business during a
period. It categorizes cash movements into three sections:

1. **Operating Activities** — Cash from day-to-day business operations
2. **Investing Activities** — Cash from buying or selling assets
3. **Financing Activities** — Cash from loans, owner contributions, or
   distributions

### When to Use It

- Understanding why your bank balance changed
- Assessing liquidity and cash runway
- Identifying whether operations generate or consume cash

### How to Generate

**Via the API:**

```bash
curl "http://localhost:8080/api/v1/reports/cash-flow?start=2026-01-01&end=2026-06-30" \
  -H "Authorization: Bearer <access_token>"
```

**Response format:**

```json
{
  "success": true,
  "data": {
    "operating_activities": { ... },
    "investing_activities": { ... },
    "financing_activities": { ... },
    "net_cash_flow": "5000.00"
  }
}
```

**Via the UI:**

Use the **Chat Sidebar**: "Generate a cash flow statement for Q2."

---

## 5. Accounts Receivable Aging Report

### What It Shows

The AR aging report breaks down outstanding customer invoices by how long they
have been unpaid. It helps you identify which customers are late on payment and
how far past due they are.

| Aging Bucket | Description |
|---|---|
| Current | Not yet due |
| 1-30 days | 1 to 30 days past due |
| 31-60 days | 31 to 60 days past due |
| 61-90 days | 61 to 90 days past due |
| 90+ days | More than 90 days past due |

### When to Use It

- Weekly or monthly collections review
- Estimating bad debt allowance
- Identifying customers who need payment reminders
- Month-end close

### How to Generate

**Via the API:**

```bash
curl http://localhost:8080/api/v1/reports/ar-aging \
  -H "Authorization: Bearer <access_token>"
```

**Response format:**

```json
{
  "success": true,
  "data": {
    "total_outstanding": "15000.00",
    "buckets": {
      "current": "8000.00",
      "1_30_days": "4000.00",
      "31_60_days": "2000.00",
      "61_90_days": "1000.00",
      "90_plus_days": "0.00"
    }
  }
}
```

**Via the UI:**

Use the **Chat Sidebar**: "Show me the AR aging report" or "Which customers are
past due?"

### Acting on the Report

- **Current bucket**: Send statements as a courtesy reminder.
- **1-30 days**: Send a friendly payment reminder email.
- **31-60 days**: Follow up with a phone call and a past-due notice.
- **61-90 days**: Consider suspending services or credit terms.
- **90+ days**: Escalate to collections or write off as bad debt.

---

## 6. Budget Variance Report

### What It Shows

The budget variance report compares your actual financial performance against
a budget you have set. For each budgeted account, it shows:

| Field | Description |
|---|---|
| Budgeted amount | What you planned to spend or earn |
| Actual amount | What actually happened |
| Variance | The difference (actual - budget) |
| Variance % | Variance as a percentage of the budget |

### Prerequisites

Before you can generate a variance report, you must create a budget:

```bash
curl -X POST http://localhost:8080/api/v1/budgets \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <access_token>" \
  -d '{
    "period": "2026-07",
    "items": [
      {
        "account_id": "<salaries-expense-uuid>",
        "budgeted_amount": "20000.00"
      },
      {
        "account_id": "<rent-expense-uuid>",
        "budgeted_amount": "5000.00"
      },
      {
        "account_id": "<sales-revenue-uuid>",
        "budgeted_amount": "50000.00"
      }
    ]
  }'
```

### How to Generate the Variance Report

**Via the API:**

```bash
curl "http://localhost:8080/api/v1/budgets/variance?period=2026-07" \
  -H "Authorization: Bearer <access_token>"
```

**Response format:**

```json
{
  "success": true,
  "data": {
    "period": "2026-07",
    "items": [
      {
        "account_id": "uuid",
        "account_name": "Salaries Expense",
        "budgeted": "20000.00",
        "actual": "22000.00",
        "variance": "-2000.00",
        "variance_pct": "-10.0"
      },
      ...
    ]
  }
}
```

### Interpreting Variance

- **Negative variance on expenses**: You overspent (actual > budget).
- **Positive variance on expenses**: You underspent (actual < budget).
- **Negative variance on revenue**: You earned less than expected.
- **Positive variance on revenue**: You exceeded your revenue target.

A variance of more than 10% (positive or negative) typically warrants
investigation.

---

## 7. Accounts Payable Outstanding Report

### What It Shows

The AP outstanding report shows all unpaid vendor bills, their due dates, and
total amounts owed. It is the payable-side counterpart of the AR aging report.

### When to Use It

- Planning cash disbursements
- Avoiding late payment penalties
- Taking advantage of early-payment discounts
- Month-end close

### How to Generate

**Via the API:**

```bash
curl http://localhost:8080/api/v1/ap/outstanding \
  -H "Authorization: Bearer <access_token>"
```

**Response format:**

```json
{
  "success": true,
  "data": {
    "total_outstanding": "12000.00",
    "bills": [
      {
        "bill_id": "uuid",
        "vendor": "Acme Supplies",
        "amount": "3000.00",
        "due_date": "2026-07-15",
        "days_until_due": 14
      },
      ...
    ]
  }
}
```

### Paying a Bill

To record a payment against an outstanding bill:

```bash
curl -X POST http://localhost:8080/api/v1/ap/bills/<bill-id>/pay \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <access_token>" \
  -d '{
    "payment_account_id": "<bank-account-uuid>",
    "amount": "3000.00",
    "payment_date": "2026-07-01"
  }'
```

This creates a double-entry transaction that debits Accounts Payable and
credits the bank account, and marks the bill as paid.

---

## 8. Exporting Reports

NexusLedger supports exporting data in two formats:

### CSV Export

Export transaction data to a CSV file:

```bash
curl http://localhost:8080/api/v1/export/csv \
  -H "Authorization: Bearer <access_token>" \
  -o transactions.csv
```

The CSV file can be opened in Excel, Google Sheets, or any spreadsheet
application.

### OFX Export

Export data in OFX (Open Financial Exchange) format, compatible with personal
finance software:

```bash
curl http://localhost:8080/api/v1/export/ofx \
  -H "Authorization: Bearer <access_token>" \
  -o transactions.ofx
```

---

## 9. Report Generation via AI Agents

Instead of calling specific API endpoints, you can ask the **ReportingAgent**
to generate any report. This is especially useful through the Chat Sidebar.

### Submit a Report Task

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
        "report_type": "balance_sheet"
      }
    }
  }'
```

### Available Report Types for Agent Tasks

| Report Type | `report_type` value |
|---|---|
| Trial Balance | `trial_balance` |
| Balance Sheet | `balance_sheet` |
| Income Statement | `income_statement` |
| Cash Flow | `cash_flow` |
| AR Aging | `ar_aging` |

### Check Task Status

```bash
curl http://localhost:8080/api/v1/tasks/queue \
  -H "Authorization: Bearer <access_token>"
```

---

## Report Summary

| Report | Endpoint | What It Answers |
|---|---|---|
| Trial Balance | `GET /api/v1/reports/trial_balance` | Are my books in balance? |
| Balance Sheet | `GET /api/v1/reports/balance_sheet` | What is my business worth right now? |
| Income Statement | `GET /api/v1/reports/income_statement` | Did I make a profit this period? |
| Cash Flow | `GET /api/v1/reports/cash-flow` | Where did my cash go? |
| AR Aging | `GET /api/v1/reports/ar-aging` | Who owes me money and how late are they? |
| Budget Variance | `GET /api/v1/budgets/variance` | Am I on track with my budget? |
| AP Outstanding | `GET /api/v1/ap/outstanding` | Who do I owe and when is it due? |

---

## Related Documentation

- [Quick Start Guide](quick-start.md) — Create your first transaction
- [FAQ](faq.md) — Common questions about reports
- [Troubleshooting](troubleshooting.md) — Fix report generation issues
