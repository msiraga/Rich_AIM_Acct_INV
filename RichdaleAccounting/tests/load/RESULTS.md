# NexusLedger Load Test Results

> Fill in the sections below after running the load tests. Use this document
> as the canonical record of performance baselines for Phase 7 Task 7.13.

---

## 1. Test Environment

| Field            | Value |
|-----------------|-------|
| **Date**         | _(fill in)_ |
| **OS**           | _(e.g., macOS 14.4 / Ubuntu 22.04 / Windows 11)_ |
| **CPU**          | _(e.g., Apple M2 Pro 10-core / Intel i7-12700K)_ |
| **RAM**          | _(e.g., 32 GB)_ |
| **Database**     | _(e.g., SurrealDB in-memory / SurrealDB RocksDB on NVMe)_ |
| **Rust version** | _(e.g., rustc 1.78.0)_ |
| **API build**    | _(e.g., release `cargo build --release`)_ |
| **Tool used**    | _(Rust load_test / wrk / hey)_ |

### Server configuration

| Setting        | Value |
|---------------|-------|
| API host:port | `127.0.0.1:8080` |
| JWT secret    | _(set via `JWT_SECRET` env var)_ |
| Rate limit    | `100` req/min (default) |
| Timeout       | `30s` (default) |

> **Note:** The default rate limit is 100 req/min. For load testing with 100+
> concurrent connections, you must either raise `API_RATE_LIMIT` in the config
> or set the `API_RATE_LIMIT` environment variable to a higher value (e.g.,
> `10000`) before starting the server.

---

## 2. Test Parameters

| Parameter              | Value |
|-----------------------|-------|
| Concurrent tasks/connections | 100 |
| Requests per task           | 10 |
| Total requests              | 1000 |
| Duration (wrk/hey)          | 60s per endpoint |
| Threads (wrk)               | _(number of CPU cores)_ |

### Request mix (Rust load test)

Each of the 100 tasks makes 10 sequential requests in this order:

| # | Method | Endpoint                    |
|---|--------|-----------------------------|
| 1 | GET    | /health                     |
| 2 | GET    | /api/v1/status              |
| 3 | GET    | /api/v1/accounts            |
| 4 | GET    | /api/v1/transactions        |
| 5 | POST   | /api/v1/transactions        |
| 6 | GET    | /api/v1/transactions        |
| 7 | GET    | /api/v1/agents              |
| 8 | GET    | /api/v1/tasks/queue         |
| 9 | POST   | /api/v1/transactions        |
| 10| GET    | /api/v1/status              |

**Totals:** 800 GET + 200 POST = 1000 requests

---

## 3. Pass / Fail Criteria

| Criterion                  | Threshold     | Status |
|---------------------------|---------------|--------|
| Error count               | 0 errors      | ☐ PASS / ☐ FAIL |
| GET p99 latency           | < 500 ms      | ☐ PASS / ☐ FAIL |
| POST p99 latency          | < 1000 ms     | ☐ PASS / ☐ FAIL |
| All tasks completed       | 100/100       | ☐ PASS / ☐ FAIL |

> A test run **passes** only if all four criteria are met simultaneously.

---

## 4. Results — Rust Load Test (`load_test.rs`)

### Overall summary

| Metric             | Value |
|--------------------|-------|
| Tasks completed    |       |
| Total requests     |       |
| Total errors       |       |
| Total duration     |       |
| Requests/sec       |       |

### Latency by method

| Method | p50 | p90 | p99 | min | max |
|--------|-----|-----|-----|-----|-----|
| GET    |     |     |     |     |     |
| POST   |     |     |     |     |     |

### Per-endpoint breakdown

| Endpoint                        | Requests | Errors |
|---------------------------------|----------|--------|
| GET /health                     |          |        |
| GET /api/v1/status              |          |        |
| GET /api/v1/accounts            |          |        |
| GET /api/v1/transactions        |          |        |
| POST /api/v1/transactions       |          |        |
| GET /api/v1/agents              |          |        |
| GET /api/v1/tasks/queue         |          |        |

### Assertion results

| Assertion                          | Result |
|------------------------------------|--------|
| `total_errors == 0`                | ☐ PASS / ☐ FAIL |
| `get_p99 < 500ms`                  | ☐ PASS / ☐ FAIL |
| `post_p99 < 1000ms`                | ☐ PASS / ☐ FAIL |

### Raw output

```
(paste cargo test output here)
```

---

## 5. Results — wrk Benchmark (`wrk_bench.sh`)

| Endpoint                        | RPS     | Latency p50 | p90 | p99 | Errors | Status |
|---------------------------------|---------|-------------|-----|-----|--------|--------|
| GET /health                     |         |             |     |     |        |        |
| GET /api/v1/status              |         |             |     |     |        |        |
| GET /api/v1/accounts            |         |             |     |     |        |        |
| GET /api/v1/transactions        |         |             |     |     |        |        |
| GET /api/v1/agents              |         |             |     |     |        |        |
| GET /api/v1/tasks/queue         |         |             |     |     |        |        |
| POST /api/v1/transactions       |         |             |     |     |        |        |

> wrk reports latency as: `Latency Distribution: 50% Xms, 75% Yms, 90% Zms, 99% Wms`

### Raw output

```
(paste wrk output here)
```

---

## 6. Results — hey Benchmark (`hey_bench.sh`)

| Endpoint                        | RPS     | Latency p50 | p90 | p95 | p99 | Errors | Status |
|---------------------------------|---------|-------------|-----|-----|-----|--------|--------|
| GET /health                     |         |             |     |     |     |        |        |
| GET /api/v1/status              |         |             |     |     |     |        |        |
| GET /api/v1/accounts            |         |             |     |     |     |        |        |
| GET /api/v1/transactions        |         |             |     |     |     |        |        |
| GET /api/v1/agents              |         |             |     |     |     |        |        |
| GET /api/v1/tasks/queue         |         |             |     |     |     |        |        |
| POST /api/v1/transactions       |         |             |     |     |     |        |        |

> hey reports: `Latency distribution: 50% Xms, 75% Yms, 90% Zms, 95% Wms, 99% Vms`

### Raw output

```
(paste hey output here)
```

---

## 7. Analysis

### Bottlenecks identified

_(Describe any endpoints or operations that showed high latency or errors.
Consider: database contention, mutex lock contention on AppState, JWT
validation overhead, serialization cost, connection pool exhaustion, etc.)_

### Comparison across tools

_(Note any significant differences between the Rust load test, wrk, and hey
results. The Rust test uses a single shared reqwest client with connection
pooling; wrk and hey may behave differently depending on their connection
models.)_

### Recommendations

_(Based on the results, note any performance improvements to pursue: e.g.,
reduce lock hold times, add connection pooling, implement caching, switch
from mutex to RwLock for read-heavy endpoints, etc.)_

---

## 8. Sign-off

| Role        | Name | Date | Signature |
|-------------|------|------|-----------|
| Test runner |      |      |           |
| Reviewer    |      |      |           |
