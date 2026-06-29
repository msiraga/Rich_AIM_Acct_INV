# API, Edge, and Monitor Modules

---

## API Module (`api/mod.rs`)

### Design

The API layer defines a complete REST API framework:

```rust
ApiServer {
    config: ApiConfig,                          // host, port, HTTPS, CORS, rate limit
    orchestrator: Arc<Mutex<AgentOrchestrator>>, // Agent coordination
    database: Arc<Mutex<Database>>,              // ← Database TYPE DOES NOT EXIST
    nexus: Arc<Mutex<NexusLedger>>,              // Core system
}
```

### Endpoint Design

| Method | Path | Handler |
|---|---|---|
| GET | `/api/v1/status` | System health, agent counts, health score |
| GET | `/api/v1/agents` | List all agents with status |
| GET | `/api/v1/accounts` | List chart of accounts |
| GET | `/api/v1/transactions` | List all transactions |
| POST | `/api/v1/transactions` | Create a transaction (returns mock) |
| POST | `/api/v1/agents/tasks` | Create a task for an agent (returns mock) |

### Response Format

```json
{
    "success": true,
    "data": { ... },
    "error": null,
    "metadata": {
        "request_id": "uuid",
        "timestamp": "2024-...",
        "response_time_ms": 0,
        "api_version": "v1"
    }
}
```

### API Handler Trait

```rust
#[async_trait]
pub trait ApiHandler: Send + Sync {
    async fn handle_get(&self, path, params) → Result<ApiResponse<Value>, ApiError>;
    async fn handle_post(&self, path, body) → Result<ApiResponse<Value>, ApiError>;
    async fn handle_put(&self, path, body) → Result<ApiResponse<Value>, ApiError>;
    async fn handle_delete(&self, path) → Result<ApiResponse<Value>, ApiError>;
    async fn handle_patch(&self, path, body) → Result<ApiResponse<Value>, ApiError>;
}
```

### Current State

- `ApiServer::start()` only **logs** the configuration — it never binds a socket or starts an HTTP server
- The `DefaultApiHandler` has partial implementations: GET endpoints route to real data, POST endpoints return mock data, PUT/DELETE/PATCH all return 404
- The actual axum HTTP server is in the Tauri backend (`nexus-ledger-tauri/backend/src/main.rs`), which is a completely separate, standalone stub that does not import `nexus-core`

---

## Edge Module (`edge/mod.rs`)

### Purpose

Enable offline-first accounting on edge devices (laptops, tablets) with periodic cloud synchronization.

### Components

```
EdgeManager
├── config: EdgeConfig
│   ├── enabled: bool               ← Default: false
│   ├── storage_path: String        ← Default: "./data/edge"
│   ├── sync_interval: u64          ← Default: 300s (5 min)
│   ├── offline_mode: bool          ← Default: false
│   ├── max_storage_size_mb: u64    ← Default: 1024 (1 GB)
│   ├── compress_data: bool
│   └── encrypt_data: bool
├── database: Arc<Mutex<Database>>  ← Same missing type issue
├── nexus: Arc<Mutex<NexusLedger>>
├── last_sync: Arc<Mutex<Option<DateTime<Utc>>>>
└── sync_in_progress: Arc<Mutex<bool>>
```

### Local Storage Layout

```
./data/edge/
├── accounts/
├── transactions/
├── documents/
├── audit/
└── temp/
```

### OfflineDataManager

Provides `save_for_offline()`, `load_offline_data()`, `clear_offline_data()` for storing data locally when disconnected.

### Sync Status

```rust
EdgeSyncStatus {
    enabled, offline_mode, is_online,
    last_sync, sync_in_progress,
    storage_used_mb, storage_max_mb,
}
```

### Current State

- All methods are stubs — they log what they *would* do but perform no real I/O
- No actual local database (SQLite, etc.) is set up
- No network connectivity checking
- No conflict resolution logic
- Default configuration has `enabled: false`

---

## Monitor Module (`monitor/mod.rs`)

### Purpose

System-wide observability: metrics collection, alerting, and health scoring.

### Components

```
SystemMonitor
├── config: MonitorConfig
│   ├── collection_interval: 60s
│   ├── retention_hours: 24
│   ├── enable_prometheus: false
│   └── prometheus_port: 9090
├── metrics: Arc<RwLock<HashMap<String, Metric>>>
├── historical_metrics: Arc<RwLock<Vec<SystemMetrics>>>
└── alerts: Arc<RwLock<Vec<Alert>>>
```

### Metric Types

| Type | Use |
|---|---|
| `Counter` | Monotonically increasing (tasks processed) |
| `Gauge` | Current value (active agents) |
| `Histogram` | Distribution (processing time) |
| `Summary` | Quantile data |

### Alert System

Alerts are generated when:
- Health score drops below 0.5 → **Critical** alert
- Health score drops below 0.8 → **Warning** alert
- Any agent in error state → **Error** alert
- Task failure rate exceeds 10% → **Warning** alert

Alert severity: Low, Medium, High, Critical.

### Health Scoring

```
health_score = 1.0
  − (error_agent_ratio × 0.4)     // Up to 40% penalty for error agents
  − (task_failure_ratio × 0.3)    // Up to 30% penalty for task failures

Clamped to [0.0, 1.0]
```

### SystemMetrics Snapshot

```rust
SystemMetrics {
    timestamp, total_agents, active_agents,
    idle_agents, busy_agents, error_agents,
    total_tasks_processed, total_tasks_failed,
    total_tasks_in_progress, health_score,
}
```

Historical metrics are retained for `retention_hours` (default 24h) and auto-pruned.

### Current State

- Metrics collection is started but `collect_metrics()` only runs once at initialization — no periodic loop is set up
- Prometheus integration is configured but not implemented
- All metrics are purely in-memory with no export/persistence
