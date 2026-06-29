# Frontend (Tauri + React)

## Architecture

```
nexus-ledger-tauri/
├── index.html                  ← Entry HTML
├── package.json                ← React 18, Vite 5, TypeScript 5
├── vite.config.ts              ← Port 3000, @ alias
├── tsconfig.json               ← Strict mode, ES2020, react-jsx
├── tsconfig.node.json          ← For vite.config.ts
├── src/
│   ├── main.tsx                ← ReactDOM.createRoot
│   ├── App.tsx                 ← Dashboard component
│   └── index.css               ← Dark theme styles
└── backend/
    ├── Cargo.toml              ← axum, tokio, tower-http (cors)
    └── src/main.rs             ← Stub HTTP server on :4000
```

## Frontend (React)

### App.tsx

A single-page dashboard with:
- **Accounts table:** Name, Type, Balance — fetched from `GET /api/accounts`
- **Invoices table:** Customer, Amount, Status — fetched from `GET /api/invoices`
- **"Create Test Invoice" button:** POSTs a hardcoded invoice to `POST /api/invoices`
- Loading and error states

```typescript
// Data flow:
useEffect → fetch("http://localhost:4000/api/accounts")  → setAccounts()
          → fetch("http://localhost:4000/api/invoices")  → setInvoices()

handleCreateInvoice → fetch("http://localhost:4000/api/invoices", { method: "POST", ... })
```

### Tech Choices

| Choice | Rationale |
|---|---|
| React 18 | Current stable React |
| Vite 5 | Fast dev server, ESM-native |
| TypeScript 5 strict | Full type safety |
| No router | Single-page app currently |
| No state management library | Only `useState` + `useEffect` |
| Dark theme CSS | Custom CSS variables, no framework |

### TypeScript Config Highlights

```json
{
  "compilerOptions": {
    "target": "ES2020",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true,
    "jsx": "react-jsx",
    "baseUrl": ".",
    "paths": { "@/*": ["src/*"] }
  }
}
```

Strictest reasonable TypeScript configuration — good foundation.

## Backend (Rust stub)

### Routes

| Method | Path | Response |
|---|---|---|
| GET | `/api/accounts` | 2 hardcoded accounts: Cash ($1,000), Revenue ($5,000) |
| GET | `/api/invoices` | 1 hardcoded invoice: Acme Corp, $1,500, Paid |
| POST | `/api/invoices` | Creates invoice with provided data, sets status "Pending" |
| GET | `/api/ledger` | 2 string transactions |
| GET | `/api/reconcile` | "Reconciliation completed" |

### CORS

```rust
let cors = CorsLayer::new()
    .allow_origin(Any)
    .allow_methods(GET)
    .allow_methods(POST)
    .allow_headers([CONTENT_TYPE]);
```

Permissive CORS for development.

## Critical Disconnect

**The Tauri backend (`nexus-ledger-tauri/backend/`) does NOT depend on `nexus-core`.** It is a standalone, trivial HTTP server with hardcoded data. The frontend fetches from this stub, not from the real accounting engine.

The `nexus-core` crate in `RichdaleAccounting/` has no HTTP server of its own (the `ApiServer` struct never binds a socket). These two halves of the project have never been connected.

### What Needs to Happen

1. Replace the Tauri backend with one that imports `nexus-core`
2. Start `ApiServer` (after fixing the `Database` type) inside the Tauri backend's `main.rs`
3. Route frontend API calls through the real agent/accounting pipeline
4. Use Tauri's IPC for desktop-native features (file dialogs, system tray, etc.)
