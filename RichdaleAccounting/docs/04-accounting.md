# Accounting Engine

## Double-Entry System

NexusLedger implements a proper double-entry accounting system with the following core concepts:

### Account Types and Normal Balances

| Account Type | Normal Balance | Debit Effect | Credit Effect |
|---|---|---|---|
| **Asset** | Debit | Increase (+) | Decrease (−) |
| **Liability** | Credit | Decrease (−) | Increase (+) |
| **Equity** | Credit | Decrease (−) | Increase (+) |
| **Revenue** | Credit | Decrease (−) | Increase (+) |
| **Expense** | Debit | Increase (+) | Decrease (−) |

This is correctly implemented in `Account::update_balance()`:

```rust
pub fn update_balance(&mut self, amount: Decimal, entry_type: EntryType) {
    let multiplier = match (self.normal_balance(), entry_type) {
        (BalanceType::Debit,  EntryType::Debit)  =>  1,
        (BalanceType::Debit,  EntryType::Credit) => -1,
        (BalanceType::Credit, EntryType::Debit)  => -1,
        (BalanceType::Credit, EntryType::Credit) =>  1,
    };
    self.balance += multiplier * amount;
}
```

### Default Chart of Accounts

The ledger initializes with a standard chart of accounts (20 accounts):

| Range | Type | Examples |
|---|---|---|
| 1000–1040 | **Assets** | Cash (1000), Bank Account (1010), A/R (1020), Inventory (1030), Fixed Assets (1040) |
| 2000–2020 | **Liabilities** | A/P (2000), Loans Payable (2010), Accrued Expenses (2020) |
| 3000–3010 | **Equity** | Owner's Equity (3000), Retained Earnings (3010) |
| 4000–4020 | **Revenue** | Sales Revenue (4000), Service Revenue (4010), Interest Revenue (4020) |
| 5000–5040 | **Expenses** | COGS (5000), Salaries (5010), Rent (5020), Utilities (5030), Office Supplies (5040) |

### Transaction Validation

Before recording, every transaction is validated:

1. **Balance check:** Sum of debits must equal sum of credits
2. **Account existence:** All referenced accounts must exist
3. **Account status:** All accounts must be active (not frozen/closed)

### Financial Statements

The ledger can generate:

| Statement | Method | Description |
|---|---|---|
| **Trial Balance** | `get_trial_balance()` | All account balances at a point in time |
| **Balance Sheet** | `get_balance_sheet()` | Assets = Liabilities + Equity |
| **Income Statement** | `get_income_statement(start, end)` | Revenue − Expenses = Net Income (for a period) |

**Missing:** Cash Flow Statement, Statement of Changes in Equity, departmental/class breakdowns.

---

## Reconciliation Module

### StatementTransaction

Models a line item from a bank statement:

```rust
pub struct StatementTransaction {
    pub date: NaiveDate,
    pub description: String,
    pub amount: Decimal,
    pub transaction_type: StatementTransactionType,  // Debit or Credit
    pub reference: String,
    pub is_matched: bool,
    pub matched_transaction_id: Option<Uuid>,
}
```

### Matching Algorithm

The `match_transactions()` method:

1. Builds a map of book transactions keyed by `(amount, description)`
2. For each statement transaction, looks up matching book transactions
3. Marks matched pairs, collects unmatched on both sides

**Limitation:** The matching is exact-match only on amount+description. A real reconciliation system needs fuzzy matching, date proximity, and reference number matching.

### Reconciliation Status

```
InProgress → Completed   (difference = 0)
           → NeedsReview (difference ≠ 0)
           → Cancelled
```

---

## Tax Module

### Tax Jurisdictions

Pre-configured with three US jurisdictions as examples:

| Jurisdiction | Code | Type | Income | Sales | Filing |
|---|---|---|---|---|---|
| US Federal | `US-FED` | Federal | 21% | — | Annual |
| California | `US-CA` | State | 9.3% | 7.25% | Quarterly |
| New York | `US-NY` | State | 6% | 4% | Quarterly |

### Tax Types

```rust
pub enum TaxType {
    Income, Payroll, Sales, Property, VAT, GST, Other(String)
}
```

### Tax Filing Lifecycle

```
NotStarted → InProgress → ReadyToFile → Filed → Paid
                ↓                         ↓
            Cancelled                 Cancelled
                                         ↓
                                     Overdue
```

### Tax Calculation

```rust
tax_amount = taxable_amount * tax_rate / 100
```

This is a flat-rate calculation. Real tax systems need progressive brackets, deductions, credits, and multi-factor formulas. The `TaxAgent::process_calculate_taxes()` ignores any task payload and hardcodes a US-FED income tax calculation on $10,000.

---

## Payroll Module

### Employee Model

```rust
pub struct Employee {
    pub id: Uuid,
    pub number: String,
    pub first_name: String,
    pub last_name: String,
    pub hire_date: NaiveDate,
    pub status: EmploymentStatus,      // Active, OnLeave, Terminated, Retired
    pub employment_type: EmploymentType, // FullTime, PartTime, Contractor, etc.
    pub pay_rate: Decimal,
    pub pay_frequency: PayFrequency,   // Weekly, BiWeekly, SemiMonthly, Monthly
    pub tax_info: TaxInformation,      // Filing status, allowances, exemptions
    pub direct_deposit: Option<DirectDeposit>,
}
```

### Payroll Calculation Breakdown

```
Gross Pay = Regular Pay + Overtime Pay (1.5× rate)

Deductions:
  Federal Tax      (simplified: rate based on filing status)
  State Tax        (simplified: rate based on filing status)
  Local Tax        (simplified: rate based on filing status)
  Social Security  (6.2% up to $160,200 wage base — 2023 limit)
  Medicare         (1.45% + 0.9% additional above $200,000)
  Retirement       (5% default)

Net Pay = Gross Pay − Total Deductions

Employer Cost = Gross Pay + Employer Contributions
  (matching Social Security, Medicare, Retirement)
```

### Hardcoded Tax Parameters

The payroll module hardcodes US 2023 values:
- Social Security rate: 6.2%, wage base: $160,200
- Medicare rate: 1.45%, additional: 0.9% above $200,000
- Tax brackets by filing status (simplified flat rates)

**Missing:** State-specific tax tables, local tax jurisdictions, pre-tax deductions (401k, HSA, FSA), garnishments, PTO accrual, year-end W-2/1099 generation.
