# NexusLedger Load Tests

Phase 7 Task 7.13 — Load testing infrastructure for the NexusLedger REST API.

## Overview

This directory contains three complementary load testing tools for the
NexusLedger API:

| Tool | File | Description |
|------|------|-------------|
| **Rust** | `load_test.rs` | tokio-based integration test: 100 concurrent tasks x 10 requests each |
| **wrk** | `wrk_bench.sh` | Shell script driving wrk against each endpoint individually |
| **hey** | `hey_bench.sh` | Shell script driving hey against each endpoint individually |

All tests exercise the same API surface:

| Method | Endpoint | Auth | Role |
|--------|----------|------|------|
| GET | `/health` | None | — |
| GET | `/api/v1/status` | Bearer JWT | Viewer+ |
| GET | `/api/v1/accounts` | Bearer JWT | Viewer+ |
| GET | `/api/v1/transactions` | Bearer JWT | Viewer+ |
| POST | `/api/v1/transactions` | Bearer JWT | User+ |
| GET | `/api/v1/agents` | Bearer JWT | Viewer+ |
| GET | `/api/v1/tasks/queue` | Bearer JWT | Viewer+ |
| POST | `/api/auth/register` | None | — |
| POST | `/api/auth/login` | None | — |

## Prerequisites

### 1. Start the NexusLedger API server

The API must be running and seeded with at least two accounts (for balanced
POST /transactions requests). From the project root:

```sh
cd nexus-core
# Set a secure JWT secret (>= 32 bytes)
export JWT_SECRET="$(openssl rand -base64 32)"
# Raise the rate limit for load testing (default is 100 req/min)
export API_RATE_LIMIT=10000
# Build in release mode for accurate performance numbers
cargo build --release
# Start the server (seeds accounts on startup)
./target/release/nexus-core
```

Verify the server is up:

```sh
curl http://localhost:8080/health
```

### 2. Install load testing tools

**Rust** (required for `load_test.rs`):
- Rust toolchain (rustc + cargo) — already installed for building NexusLedger

**wrk** (for `wrk_bench.sh`):
- macOS: `brew install wrk`
- Ubuntu/Debian: `apt install wrk` or build from [source](https://github.com/wg/wrk)
- Also requires: `curl`, `jq`

**hey** (for `hey_bench.sh`):
- `go install github.com/rakyll/hey@latest`
- Or download a binary from [releases](https://github.com/rakyll/hey/releases)
- Also requires: `curl`, `jq`

## Running the Tests

### Option A: Rust Load Test (Recommended)

The Rust load test is the most comprehensive: it runs a realistic request mix
(8 GET + 2 POST per task, 100 tasks = 1000 total requests), measures p50/p90/p99
latency separately for GET and POST, and asserts pass/fail criteria.

#### Setup: Register the test as a cargo integration test

The test file lives at `tests/load/load_test.rs`. Cargo discovers integration
tests only from the top level of a crate's `tests/` directory, so you need to
either:

**Option 1 — Symlink (recommended):**

```sh
cd nexus-core
ln -s ../tests/load/load_test.rs tests/load_test.rs
```

**Option 2 — Add a `[[test]]` entry to `nexus-core/Cargo.toml`:**

```toml
[[test]]
name = "load_test"
path = "../tests/load/load_test.rs"

[dev-dependencies]
# These are already workspace dependencies; add as dev-deps for the test:
reqwest = { workspace = true }
tokio = { workspace = true }
serde_json = { workspace = true }
```

#### Run the test

```sh
# From the nexus-core directory:
cargo test --test load_test -- --nocapture --ignored
```

The test is marked `#[ignore]` because it requires a running server. The
`--nocapture` flag ensures stdout (the results summary) is printed.

#### Adjusting parameters

```sh
# 200 concurrent tasks, 20 requests each (4000 total)
LOAD_CONCURRENT=200 LOAD_REQUESTS_PER_TASK=20 \
  cargo test --test load_test -- --nocapture --ignored
```

#### Expected output

```
═══════════════════════════════════════════════════════════════
  NexusLedger Load Test
  Concurrent tasks : 100
  Requests per task: 10
  Total requests   : 1000
  API base URL     : http://localhost:8080
═══════════════════════════════════════════════════════════════

[1/3] Authenticating test user...
      ✓ JWT token acquired
[2/3] Fetching account IDs for transaction creation...
      ✓ Got 2 account IDs
[3/3] Spawning 100 concurrent tasks (10 requests each)...

╔═══════════════════════════════════════════════════════════════╗
║                    LOAD TEST RESULTS                          ║
╠═══════════════════════════════════════════════════════════════╣
║  Tasks completed :    100                                      ║
║  Total requests  :   1000                                      ║
║  Total errors    :      0                                      ║
║  Total duration  :     1.23s                                   ║
║  Requests/sec    :    813.0                                    ║
╠═══════════════════════════════════════════════════════════════╣
║  GET  latency    p50=1.2ms   p90=3.5ms   p99=12.0ms           ║
║  POST latency    p50=5.0ms   p90=10.0ms  p99=45.0ms           ║
╠═══════════════════════════════════════════════════════════════╣
║  Per-endpoint breakdown:                                      ║
║    /health               100 reqs,   0 errors                 ║
║    /status               200 reqs,   0 errors                 ║
║    /accounts             100 reqs,   0 errors                 ║
║    /transactions         300 reqs,   0 errors                 ║
║    /agents               100 reqs,   0 errors                 ║
║    /create/transaction   200 reqs,   0 errors                 ║
║    /tasks/queue          100 reqs,   0 errors                 ║
╚═══════════════════════════════════════════════════════════════╝

✓ GET  p99 (12.0ms) < 500.0ms — PASS
✓ POST p99 (45.0ms) < 1000.0ms — PASS

✓ ALL LOAD TEST ASSERTIONS PASSED
```

(Actual numbers will vary based on hardware, database backend, and server load.)

### Option B: wrk Benchmark

```sh
cd tests/load
chmod +x wrk_bench.sh

# Default: 30s, 100 connections
./wrk_bench.sh

# Custom: 60s, 200 connections
./wrk_bench.sh 60s 200

# Quick smoke test
./wrk_bench.sh 10s 50
```

wrk runs each endpoint separately for the specified duration. Results are
saved to `wrk_results_<timestamp>.txt` in the current directory.

### Option C: hey Benchmark

```sh
cd tests/load
chmod +x hey_bench.sh

# Default: 30s, 100 concurrent workers
./hey_bench.sh

# Custom: 60s, 200 workers
./hey_bench.sh 60 200

# Quick smoke test
./hey_bench.sh 10 50
```

hey runs each endpoint separately for the specified duration. Results are
saved to `hey_results_<timestamp>.txt` in the current directory.

## Pass / Fail Criteria

| Criterion          | Threshold   | Applies to |
|--------------------|-------------|------------|
| Error count        | 0 errors    | All tests  |
| GET p99 latency    | < 500 ms    | All GET endpoints |
| POST p99 latency   | < 1000 ms   | POST /api/v1/transactions |

The Rust load test enforces these criteria with `assert!` statements. The
wrk and hey scripts print the criteria at the end but do not assert
programmatically — review the output manually.

## What Each Tool Measures

### Rust load test (`load_test.rs`)

- 100 concurrent tokio tasks, each making 10 sequential requests
- Mixed workload: 8 GET + 2 POST per task (800 GET + 200 POST = 1000 total)
- Auth flow: registers a unique test user, logs in, uses the JWT for all
  authenticated endpoints
- Fetches real account IDs from the API for balanced POST transactions
- Measures: total time, requests/sec, error count, p50/p90/p99 (separately
  for GET and POST), per-endpoint request counts and errors
- Percentiles calculated manually (sort latencies, index into sorted array)

### wrk (`wrk_bench.sh`)

- Runs each endpoint individually (not mixed workload)
- Uses Lua scripts to inject JWT auth headers and POST bodies
- Reports: requests/sec, latency distribution (p50/p75/p90/p99), transfer/sec
- Configurable duration and connection count

### hey (`hey_bench.sh`)

- Runs each endpoint individually
- Sends auth headers via `-H` flag, POST body via `-D` flag
- Reports: requests/sec, latency distribution (p50/p75/p90/p95/p99),
  status code distribution, histogram
- Configurable duration and concurrency

## Auth Flow

All three tools follow the same authentication sequence:

1. **Register** a test user via `POST /api/auth/register` with a unique
   username/email and the password `LoadTest123!` (meets the policy: 8+ chars,
   at least one letter and one digit)
2. Extract `access_token` from the response JSON (`data.access_token`)
3. If registration fails (user already exists from a prior run), fall back to
   `POST /api/auth/login` with the same credentials
4. Use the token as `Authorization: Bearer <token>` for all authenticated
   endpoints

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `API_BASE` | `http://localhost:8080` | Base URL of the API server |
| `JWT_SECRET` | _(server refuses to start without it)_ | Must match the server's secret |
| `API_RATE_LIMIT` | `100` | Server-side rate limit (req/min); raise for load testing |
| `LOAD_CONCURRENT` | `100` | Number of concurrent tasks (Rust test only) |
| `LOAD_REQUESTS_PER_TASK` | `10` | Requests per task (Rust test only) |
| `THREADS` | `$(nproc)` | Number of wrk threads (wrk script only) |

## Files

```
tests/load/
├── load_test.rs    # Rust integration test (tokio + reqwest)
├── wrk_bench.sh    # wrk load test script
├── hey_bench.sh    # hey load test script
├── RESULTS.md      # Results template — fill in after running
└── README.md       # This file
```

## Troubleshooting

### "authentication failed: register returned non-201"

The API server may not be running, or the JWT secret environment variable
isn't set. Verify:

```sh
curl -X POST http://localhost:8080/api/auth/register \
  -H "Content-Type: application/json" \
  -d '{"username":"testuser","email":"test@test.com","password":"Test1234!"}'
```

### "No account IDs found — POST requests will be skipped"

The server started but the database wasn't seeded. The NexusLedger server
seeds accounts on startup when connected to a fresh database. Ensure the
database is in-memory or freshly initialized.

### "401 Unauthorized" on authenticated endpoints

The JWT token may have expired (30-minute TTL) or the `JWT_SECRET` env var
differs between the server and the test. Ensure they match.

### "429 Too Many Requests"

The server's default rate limit is 100 req/min. For load testing, raise it:

```sh
export API_RATE_LIMIT=10000
```

Then restart the server.

### wrk/hey: "command not found"

Install the tools (see Prerequisites above). On Windows, use WSL or Git Bash
to run the shell scripts.
