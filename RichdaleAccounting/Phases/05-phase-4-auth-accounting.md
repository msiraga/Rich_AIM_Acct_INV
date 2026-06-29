# Phase 4: Auth & Accounting Completeness

**Objective:** Add authentication, authorization, and fill in every missing accounting feature to reach QuickBooks-parity. This phase makes the platform usable for a real business.  
**Duration:** 3–4 weeks  
**Depends on:** Phase 3 (freeze token satisfied)  
**Blocks:** Phase 5  

---

## Why This Phase Exists

After Phase 3, the app works end-to-end but has no security (anyone can do anything) and is missing critical accounting features: AP/AR workflows, cash flow statements, AR aging reports, CSV import/export, multi-currency, budgets, and fixed asset tracking. A real business cannot use the platform without these.

---

## Task List

### Auth Track

| ID | Task | Primary File(s) | Depends On | Parallel With |
|---|---|---|---|---|
| 4.1 | **Password hashing** — add argon2 (or bcrypt) to hash passwords on registration, verify on login. Never store plaintext. | `database/user.rs`, `Cargo.toml` | P3 | 4.2 |
| 4.2 | **JWT middleware** — issue JWT on login, validate on every API request, extract user ID + role from token. | `api/auth.rs` (new) | 4.1 | 4.1 |
| 4.3 | **Login/Register endpoints** — `POST /api/auth/register`, `POST /api/auth/login`, `POST /api/auth/refresh`. | `api/routes/auth.rs` (new) | 4.1, 4.2 | Nothing |
| 4.4 | **Role-based access control** — middleware checks `UserRole` against endpoint requirements. Viewer=read-only, User=CRUD own data, Manager=CRUD all, Admin=everything. | `api/auth.rs` | 4.2 | 4.5 |
| 4.5 | **Login UI** — registration form, login form, JWT storage in frontend (localStorage or cookie), protected routes (redirect to login if no token). | `nexus-ledger-tauri/src/pages/Login.tsx` (new) | 4.3 | 4.4 |

### Accounting Track

| ID | Task | Primary File(s) | Depends On | Parallel With |
|---|---|---|---|---|
| 4.6 | **Accounts Payable workflow** — enter vendor bill, schedule payment date, mark as paid, auto-create journal entries (debit expense, credit AP → debit AP, credit cash). | `accounting/ap.rs` (new) | P3 | 4.7, 4.8, 4.11 |
| 4.7 | **Accounts Receivable aging report** — outstanding invoices grouped by 0-30, 31-60, 61-90, 90+ days. Query real invoice data. | `accounting/reporting.rs` | P3 | 4.6, 4.8 |
| 4.8 | **Cash Flow Statement** — operating activities (net income + adjustments), investing activities (asset purchases), financing activities (loans, equity). Three-section format per GAAP. | `accounting/reporting.rs` | P3 | 4.6, 4.7 |
| 4.9 | **CSV import** — upload CSV file with transactions (date, description, amount, account). Parse, validate, create transactions via `LedgerAgent`. | `utils/import.rs` (new) | P3 | 4.10 |
| 4.10 | **CSV/OFX export** — download transactions as CSV. Optionally support OFX format for bank import compatibility. | `utils/export.rs` (new) | P3 | 4.9 |
| 4.11 | **Multi-currency** — add `currency` field to transactions, store exchange rate, convert to base currency for reporting. Base currency set per `Organization`. | `database/financial.rs`, `accounting/ledger.rs` | P3 | 4.6 |
| 4.12 | **Budget tracking** — create budgets per account per period. Budget vs. actual report showing variance. | `accounting/budget.rs` (new) | P3 | 4.8 |
| 4.13 | **Fixed asset tracking** — register assets with cost, useful life, depreciation method (straight-line). Auto-generate monthly depreciation journal entries. | `accounting/assets.rs` (new) | P3 | 4.6 |

### Final Gate

| ID | Task | Depends On |
|---|---|---|
| 4.14 | **Integration tests** — (a) unauthorized user cannot access admin endpoints, (b) AP bill → payment → journal entry, (c) AR aging shows correct groupings, (d) CSV import creates valid transactions | 4.4, 4.6, 4.7, 4.9 |

---

## Dependency Graph

```
                    P3 (freeze token)
                         │
         ┌───────────────┼───────────────┐
         │               │               │
    Track A (auth)   Track B (acct)  Track C (import)
         │               │               │
    4.1 ─→ 4.2       4.6 ─┐          4.9 ─→ 4.10
         │  │            4.7 ─┤
         │  └→ 4.3      4.8 ─┤
         │       │           4.11─┤
         │    4.4 ─┐        4.12─┤
         │         │        4.13─┘
         └──→ 4.5  │
                   │
                   └────────────┬──→ 4.14 (integration tests)
```

---

## Parallel Execution Strategy

**Session 1 (Three parallel tracks start simultaneously):**
- Track A: 4.1 → 4.2 → 4.3 → 4.4 → 4.5
- Track B: 4.6 + 4.7 + 4.8 + 4.11 + 4.12 + 4.13 (all independent after P3)
- Track C: 4.9 → 4.10

**Session 2 (Integration, after all tracks):**
- 4.14

---

## Freeze Token 4 🔒

All conditions must be true:

- [ ] User registration hashes password with argon2/bcrypt
- [ ] Login returns valid JWT token
- [ ] All API endpoints require valid JWT (except `/auth/*`)
- [ ] Viewer role cannot create/update/delete
- [ ] Admin role can access everything
- [ ] AP workflow: enter bill → schedule payment → mark paid → two journal entries created (bill + payment)
- [ ] AR aging report: shows outstanding invoices grouped by 30/60/90 days with correct amounts
- [ ] Cash flow statement: three sections (operating/investing/financing) with correct totals
- [ ] CSV import: upload file → transactions created → appear in ledger
- [ ] CSV export: download all transactions as valid CSV
- [ ] Multi-currency: create EUR transaction → reported in USD base currency with exchange rate
- [ ] Budget: create budget for expense account → budget vs. actual shows variance
- [ ] Fixed asset: register $10K asset with 5-year straight-line → monthly depreciation = $166.67
- [ ] Login UI works: register → login → access protected pages → logout
- [ ] `cargo test` passes (unit + integration)

---

## Notes for Reviewer

- This phase is the largest in task count (14 tasks) but tasks are well-isolated
- Auth track (A) is sequential — can't parallelize within it
- Accounting track (B) is fully parallel — 6 independent features
- Multi-currency (4.11) requires exchange rate data — start with manual rate entry, defer API feeds
- This phase makes the platform **business-viable** — a real small business could start using it
