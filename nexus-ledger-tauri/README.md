# NexusLedger Template

A functioning template for NexusLedger, an agentic accounting platform.

## Quick Start

### 1. Run the Rust Backend
```bash
cd backend
cargo run
```
Backend starts on `http://localhost:4000`.

### 2. Run the React Frontend
```bash
npm install
npm run dev
```
Frontend starts on `http://localhost:3000`.

### 3. Run Both
```bash
npm install
npm run start
```

## API Endpoints
- `GET /api/accounts` → List accounts
- `GET /api/invoices` → List invoices
- `POST /api/invoices` → Create an invoice
- `GET /api/ledger` → Get ledger transactions
- `GET /api/reconcile` → Run reconciliation

## Ports
- **Backend**: `4000` (Rust HTTP API)
- **Frontend**: `3000` (React UI)
