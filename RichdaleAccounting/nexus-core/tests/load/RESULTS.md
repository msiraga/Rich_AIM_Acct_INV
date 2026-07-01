# Load Test Results — NexusLedger Phase 7 Task 7.13

## Overview

This directory contains load-testing scripts for the NexusLedger API server.
The tests verify that the API can handle concurrent read, write, and WebSocket
traffic within acceptable latency and error thresholds.

### Freeze Token 7 Criteria

> 100 concurrent requests: zero errors, p99 < 500ms

---

## Prerequisites

### Tools

| Tool      | Purpose                | Install                                   |
|-----------|------------------------|-------------------------------------------|
| `wrk`     | HTTP load generator    | `apt install wrk` / `brew install wrk`    |
| `websocat`| WebSocket client       | `cargo install websocat` / `apt install websocat` |
| `curl`    | HTTP client (token)    | pre-installed on most systems             |
| `jq`      | JSON parser            | `apt install jq` / `brew install jq`      |

### Server

The NexusLedger API server must be running and accessible at `http://localhost:4000`
(or override with `API_BASE=http://host:port`).

The server must have:
- A valid JWT secret configured (`JWT_SECRET` env var, >= 32 bytes)
- An initialised chart of accounts (accounts 1000 = Cash, 4000 = Sales Revenue)
- User registration enabled (no DB connectivity issues)

### Make Scripts Executable

```bash
chmod +x generate_token.sh load_get.sh load_post.sh load_concurrent_rw.sh load_ws.sh
```

---

## Scripts

| Script                  | Description                                              |
|-------------------------|----------------------------------------------------------|
| `generate_token.sh`     | Registers a test user and exports `$TOKEN` (JWT). Must be sourced. |
| `load_get.sh`           | wrk GET load test against 3 read endpoints.              |
| `load_post.sh`          | wrk POST load test using `post_script.lua` to create transactions. |
| `post_script.lua`       | wrk Lua script generating balanced double-entry POST bodies. |
| `load_concurrent_rw.sh` | Runs readers (100) and writers (10) simultaneously.      |
| `load_ws.sh`            | Opens 50 concurrent WebSocket connections for 60s.       |

### Usage

```bash
# 1. Start the API server (with a valid JWT secret)
JWT_SECRET=$(openssl rand -base64 32) ./nexus-ledger

# 2. Run individual tests
./load_get.sh               # GET endpoints, 100 conns, 60s
./load_post.sh              # POST transactions, 50 conns, 60s
./load_concurrent_rw.sh     # Concurrent reads + writes
./load_ws.sh                # WebSocket, 50 conns, 60s

# 3. Override defaults
API_BASE=http://localhost:9090 ./load_get.sh 8 200 30s
./load_post.sh 10 60s       # 10 writer connections (for concurrent_rw)
./load_ws.sh 100 120 1      # 100 WS connections, 120s, 1s interval
```

---

## Pass Criteria

| Test             | Errors | p99 Latency | Notes                           |
|------------------|--------|-------------|---------------------------------|
| GET endpoints    | 0      | < 500 ms    | 100 concurrent connections      |
| POST transactions| 0      | < 1000 ms   | 50 concurrent connections       |
| Concurrent R/W   | 0      | < 500 ms (GET) / < 1000 ms (POST) | 100 readers + 10 writers |
| WebSocket        | 0 drops| 90%+ response rate | 50 concurrent connections |

---

## Results

Run the scripts and fill in the table below with the measured values.

### GET Load Test (load_get.sh)

| Test | Endpoint                        | Concurrent | Duration | RPS | p50 | p99 | Errors | Status |
|------|---------------------------------|------------|----------|-----|-----|-----|--------|--------|
| GET  | /api/v1/accounts                | 100        | 60s      | TBD | TBD | TBD | TBD    | TBD — run scripts to populate |
| GET  | /api/v1/transactions            | 100        | 60s      | TBD | TBD | TBD | TBD    | TBD — run scripts to populate |
| GET  | /api/v1/reports/balance_sheet   | 100        | 60s      | TBD | TBD | TBD | TBD    | TBD — run scripts to populate |

### POST Load Test (load_post.sh)

| Test | Endpoint                  | Concurrent | Duration | RPS | p50 | p99 | Errors | Status |
|------|---------------------------|------------|----------|-----|-----|-----|--------|--------|
| POST | /api/v1/transactions      | 50         | 60s      | TBD | TBD | TBD | TBD    | TBD — run scripts to populate |

### Concurrent Read/Write Test (load_concurrent_rw.sh)

| Test | Endpoint(s)                          | Concurrent       | Duration | RPS | p50 | p99 | Errors | Status |
|------|--------------------------------------|------------------|----------|-----|-----|-----|--------|--------|
| R/W  | GET /api/v1/accounts + POST /txns    | 100 readers + 10 writers | 60s | TBD | TBD | TBD | TBD | TBD — run scripts to populate |
| R/W  | GET /api/v1/transactions + POST /txns| 100 readers + 10 writers | 60s | TBD | TBD | TBD | TBD | TBD — run scripts to populate |
| R/W  | GET /api/v1/reports/balance_sheet + POST /txns | 100 readers + 10 writers | 60s | TBD | TBD | TBD | TBD | TBD — run scripts to populate |

### WebSocket Load Test (load_ws.sh)

| Test | Endpoint          | Concurrent | Duration | Messages Sent | Responses Received | Response Rate | Errors | Status |
|------|-------------------|------------|----------|---------------|---------------------|---------------|--------|--------|
| WS   | /ws/chat?token=…  | 50         | 60s      | TBD           | TBD                 | TBD           | TBD    | TBD — run scripts to populate |

---

## How to Read wrk Output

wrk produces output like:

```
Running 1m test @ http://localhost:4000/api/v1/accounts
  4 threads and 100 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency   123.45ms  45.67ms   1.23s   85.67%
    Req/Sec   2.00k     0.50k     3.00k    75.00%
  Latency Distribution
     50%  100.00ms
     75%  150.00ms
     90%  200.00ms
     99%  450.00ms
  12345 requests in 60.00s, 12.34MB read
  Non-2xx or 3xx responses: 0
```

- **RPS**: `requests / duration` (e.g., 12345 / 60 = ~206 RPS)
- **p50**: `50%` line in Latency Distribution
- **p99**: `99%` line in Latency Distribution
- **Errors**: `Non-2xx or 3xx responses` count (0 if line absent)

---

## Notes

- `generate_token.sh` creates a unique test user per run (timestamp-based suffix)
  so it can be executed repeatedly without "username already taken" errors.
- `load_post.sh` fetches real account UUIDs from the running server before
  starting the wrk run. The Lua script receives them via `wrk -- args`.
- `load_concurrent_rw.sh` captures reader and writer output to separate temp
  files to avoid interleaved output, then prints both after completion.
- `load_ws.sh` staggers connection establishment by 50ms to avoid a thundering
  herd at startup.
- All scripts respect the `API_BASE` environment variable (default
  `http://localhost:4000`) for pointing at a non-default host/port.
