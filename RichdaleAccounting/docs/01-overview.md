# NexusLedger — Project Overview

**Author:** Mounir Siraji <mounir@richdaleai.com>  
**Organization:** RichdaleAI  
**License:** Apache-2.0  
**Tagline:** Fully Agentic Accounting Platform — a QuickBooks replacement  
**Version:** 0.1.0 (pre-alpha skeleton)

---

## What Is NexusLedger?

NexusLedger is an ambitious, ground-up rewrite of accounting software. Instead of a monolithic architecture (like QuickBooks, Xero, or Sage), it models each accounting function as an **autonomous agent** — a ledger agent, a reconciliation agent, a tax agent, a payroll agent, and so on — coordinated by a central orchestrator. The system targets self-hosted deployment with edge/offline support, AI-augmented document processing, and a Tauri-based desktop frontend.

---

## Repository Layout

```
Rich_AIM_Acct_INV/
├── nexus-ledger-tauri/          ← Tauri desktop app (template)
│   ├── backend/                 ← Stub axum backend (NOT nexus-core)
│   │   ├── Cargo.toml
│   │   └── src/main.rs          ← Standalone HTTP server, port 4000
│   ├── src/                     ← React + TypeScript frontend
│   │   ├── App.tsx              ← Dashboard with accounts/invoices
│   │   ├── main.tsx             ← React entry point
│   │   └── index.css            ← Dark-themed styles
│   ├── index.html
│   ├── package.json             ← React 18, Vite 5, TypeScript 5
│   ├── vite.config.ts
│   ├── tsconfig.json            ← Strict TS, path alias "@/*"
│   ├── tsconfig.node.json
│   └── Cargo.toml               ← Workspace root
│
└── RichdaleAccounting/          ← The real core library
    ├── Cargo.toml               ← Workspace root
    └── nexus-core/              ← Core crate (~10,000+ lines)
        ├── Cargo.toml
        └── src/
            ├── main.rs          ← Entry point (no-op atm)
            ├── lib.rs           ← Crate root, NexusLedger struct
            ├── agents/          ← Agent trait, orchestrator, task system
            ├── accounting/      ← Ledger, reconciliation, tax, payroll
            ├── database/        ← Models, financial types, repos
            ├── ai/              ← Ollama integration
            ├── api/             ← REST/GraphQL/WS API design
            ├── audit/           ← Audit agent & logging
            ├── edge/            ← Offline mode, sync
            ├── monitor/         ← Metrics, alerts, health
            ├── models/          ← Re-exports
            └── utils/           ← Dates, files, validation
```

---

## Technology Stack

| Layer | Technology |
|---|---|
| Core language | Rust (edition 2021) |
| Async runtime | Tokio (full features) |
| Web framework | Axum 0.7 |
| Database | SurrealDB 1.0 (document-graph DB) |
| AI inference | Ollama (local LLM) |
| Serialization | Serde + serde_json |
| Decimal math | rust_decimal 1.32 |
| Date/time | Chrono 0.4 |
| Frontend | React 18 + TypeScript 5 + Vite 5 |
| Desktop shell | Tauri (configured, not implemented) |
| Logging | Tracing + tracing-subscriber |
| CLI | Clap 4.0 |
| Configuration | config crate 0.13 |
| File watching | notify 6.0 |
| HTTP client | reqwest 0.11 |

---

## Current Status

The project is a **well-architected skeleton** at approximately 10–15% completion. Every module has the correct structure, types, and interfaces — but nearly all runtime logic returns mock data, the database is never connected, the API server never binds a socket, and the two subprojects (Tauri template vs. nexus-core) are completely disconnected.

### What compiles and runs

- All unit tests pass
- The stub backend (`nexus-ledger-tauri/backend/`) starts and serves mock data
- The React frontend starts and displays mock data from the stub backend

### What does NOT work

- `nexus-core` does not compile cleanly (type mismatches in orchestrator agent factory)
- No database persistence
- No real agent processing
- No API server
- No authentication
- No reporting output
- No document processing

---

## Key Dependencies (nexus-core Cargo.toml)

```
tokio, axum (not in nexus-core — only in Tauri backend),
serde, serde_json, uuid, chrono, rust-decimal,
thiserror, anyhow, tracing, surrealdb, ollama-rs,
reqwest, sha2, hex, dashmap, clap, config, async-trait,
path-absolutize, base64, regex, notify
```
