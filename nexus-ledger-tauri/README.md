# NexusLedger Template

A **functioning template** for NexusLedger, an agentic accounting platform (QuickBooks replacement).

## Architecture

- **Backend**: Rust (Axum) - HTTP API server
- **Frontend**: React + TypeScript + Vite

## Project Structure

```
nexus-ledger-tauri/
├── backend/               # Rust HTTP API server
│   ├── Cargo.toml
│   └── src/
│       └── main.rs       # API endpoints
├── src/                  # React frontend
│   ├── App.tsx           # Main app component
│   ├── main.tsx
│   ├── index.css
│   └── ...
├── package.json           # Frontend dependencies
├── Cargo.toml            # Rust workspace
└── README.md
```

## Quick Start

### 1. Run the Rust Backend

```bash
cd backend
cargo run
```

The backend will start on `http://localhost:3001` with the following endpoints:

- `GET /api/accounts` - List accounts
- `GET /api/invoices` - List invoices
- `POST /api/invoices` - Create an invoice
- `GET /api/ledger` - Get ledger transactions
- `GET /api/reconcile` - Run reconciliation

### 2. Run the React Frontend

```bash
npm install
npm run dev
```

The frontend will start on `http://localhost:3000` and connect to the backend API.

### 3. Run Both (Backend + Frontend)

```bash
npm install
npm run start
```

This uses `concurrently` to run both the backend and frontend simultaneously.

## API Endpoints

| Method | Endpoint          | Description                     |
|--------|-------------------|---------------------------------|
| GET    | `/api/accounts`   | Get list of accounts            |
| GET    | `/api/invoices`   | Get list of invoices            |
| POST   | `/api/invoices`   | Create a new invoice            |
| GET    | `/api/ledger`     | Get ledger transactions         |
| GET    | `/api/reconcile`  | Run reconciliation              |

## Next Steps

1. **Integrate `nexus-core`**: Replace the mock backend with the actual `nexus-core` Rust logic.
2. **Add Database**: Connect to SurrealDB or another database.
3. **Add Authentication**: Implement user authentication.
4. **Expand UI**: Add more pages (e.g., reports, settings).

## License

Apache-2.0

## Author

Mounir Siraji <mounir@richdaleai.com>
