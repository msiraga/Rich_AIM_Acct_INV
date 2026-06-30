You are continuing development of NexusLedger, an agentic accounting platform
in Rust (Tauri + React frontend, SurrealDB backend, agent-based architecture).

## Repository
c:\Users\msira\Richdale_Accounting\Rich_AIM_Acct_INV
Latest commit on main: (Phase 4 just committed — check git log)

## Read These First
1. RichdaleAccounting/Phases/TRACKER.md        ← Phase 4 done, Phase 5 next
2. RichdaleAccounting/Phases/00-strategy.md    ← methodology + principles
3. RichdaleAccounting/Phases/06-phase-5-ai-pipeline.md ← THIS PHASE'S PLAN
4. RichdaleAccounting/Phases/phase-4-handoff.md (this file)
5. RichdaleAccounting/docs/02-architecture.md  ← system diagram

## Phase 4: Auth & Accounting Completeness — COMPLETED ✅

### Auth Track
- 4.1: argon2id hashing, empty-password rejection, 8 tests
- 4.2: JWT with token_type (access vs refresh), strict 0s leeway, refresh rotation, default secret refusal, WS auth via ?token=, 8 tests
- 4.3: Register (email + strength validation), Login (timing-attack fix with dummy hash), Refresh (rotation, role re-fetched from DB)
- 4.4: RBAC — UserRole levels (Guest→Admin), RequireViewer/User/Manager/Admin extractors on all endpoints, admin user-management endpoints
- 4.5: React Login/Register, ProtectedRoute, centralized api.ts with Bearer auth + 401-refresh interceptor, role-based nav, WS auth, isLoading fix

### Accounting Track
- 4.6: AP workflow (vendor→bill→payment), partial payments, multi-line bills, ApAgent, 4 API endpoints, 11 tests
- 4.7: AR aging (4 buckets), partial payment support, as-of date, metadata-based matching
- 4.8: Cash flow — 3 GAAP sections, AccountType classification, depreciation via 1050, reconciliation assertion
- 4.9: CSV import — quoted field parser, balance validation, 15 tests
- 4.10: CSV/OFX export — account name resolution, date filters, signed OFX TRNAMT, BANKACCTFROM, LEDGERBAL, 8 tests
- 4.11: Multi-currency — ExchangeRates, TransactionEntry currency fields (currency, exchange_rate, base_currency_amount)
- 4.12: Budget — sign-based variance, period overlap logic, BudgetManager, 3 tests
- 4.13: Fixed assets — SL/DDB depreciation, post_depreciation() auto-journal, dispose_asset() with gain/loss, 8 tests
- 4.14: 20 integration tests covering all freeze token conditions

### Verification
- cargo check: 0 errors
- cargo test: 287 passed, 0 failed
- Frontend: TypeScript compiles clean
- Freeze Token 4: all 6 conditions met

## IMPORTANT: Key Changes vs Phase 3 (Breaking)
- **TransactionEntry** has 3 new fields: `currency`, `exchange_rate`, `base_currency_amount` with defaults ("USD", None, None). Always use `..Default::default()` in struct literals.
- **total_amount()** now returns debit-sum only (not sum of all entries)
- **Chart of accounts** expanded from 18→20: added 1050 (Accumulated Depreciation), 5050 (Depreciation Expense)
- **record_transaction()** reassigns transaction.number to "TRX-00000XXX" format
- **JWT access token TTL** reduced to 30 min (1800s), refresh 7 days (604800s)
- **Auth middleware** skips /auth and /health, allows /ws/ with ?token= query param

## Phase 5: AI Pipeline — NEXT

Read: RichdaleAccounting/Phases/06-phase-5-ai-pipeline.md

10 tasks in 3 tracks:

### Track A — OCR (parallel: 5.1 + 5.2)
5.1: Tesseract OCR → ai/ocr.rs (new)
5.2: PDF extraction → ai/pdf.rs (new)

### Track B — UI (parallel with A)
5.3: Document upload page with drag-and-drop

### Track C — AI Features (parallel with A, B)
5.6: Embedding storage + vector search
5.7: Transaction anomaly detection
5.8: Smart account categorization

### Wire (after A completes: 5.4 → 5.5 → 5.9 → 5.10)
5.4: Wire OCR → AI extraction
5.5: Auto-create transaction from extraction
5.9: AI health endpoint
5.10: E2E test — receipt upload → auto-transaction

## Key File Paths (Updated for Phase 4)
```
nexus-core/src/
├── api/auth.rs          ← JWT creation/validation, role extractors, auth middleware
├── api/mod.rs           ← axum server with 40+ endpoints
├── accounting/
│   ├── ap.rs            ← AP workflow (Vendor, ApBill, ApProcessor, ApAgent)
│   ├── budget.rs        ← Budget tracking (Budget, BudgetManager, variance)
│   ├── assets.rs        ← Fixed assets (FixedAsset, AssetManager, depreciation)
│   └── reporting.rs     ← AR aging (ArAgingReport, generate_ar_aging)
├── database/
│   ├── user.rs          ← hash_password(), verify_password(), SurrealUserRepository
│   ├── financial.rs     ← TransactionEntry with multi-currency fields
│   └── models.rs        ← UserRole with level()/can_read()/can_write()/etc.
├── utils/
│   ├── import.rs        ← CSV import with parse_csv_line() quoted field parser
│   └── export.rs        ← CSV/OFX export with account resolution
└── tests/
    └── e2e_test.rs      ← 20 integration tests

nexus-ledger-tauri/src/
├── lib/api.ts           ← Centralized API client with auth + refresh interceptor
├── contexts/AuthContext.tsx  ← Auth state, login/register/logout, token storage
├── components/ProtectedRoute.tsx ← Auth gate with loading spinner
├── pages/LoginPage.tsx  ← Login form
└── pages/RegisterPage.tsx ← Registration form
```

## Freeze Token 5 (all must pass)
- [ ] Receipt photo → OCR text → AI JSON → transaction created
- [ ] AI degrades gracefully when Ollama unavailable
- [ ] Anomaly detection flags test case
- [ ] Embeddings searchable
- [ ] cargo test: 0 failures

Begin by reading TRACKER.md and 06-phase-5-ai-pipeline.md, then start Task 5.1.
