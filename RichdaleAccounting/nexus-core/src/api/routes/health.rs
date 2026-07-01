//! Health Check & Metrics Endpoints
//!
//! Provides Kubernetes-style liveness/readiness probes and a Prometheus
//! metrics endpoint.
//!
//! # Endpoints
//! - `GET /health`  — liveness probe (always 200 if the process is alive)
//! - `GET /ready`   — readiness probe (200 if DB connected + agents initialized, 503 otherwise)
//! - `GET /metrics` — Prometheus text-format metrics (404 if Prometheus is not enabled)

use axum::{
    Router,
    routing::get,
    extract::State,
    response::{IntoResponse, Json},
    http::{HeaderValue, StatusCode, header},
};
use chrono::Utc;
use tracing::debug;

use crate::api::AppState;
use crate::agents::status::SystemStatus;

// ── Handlers ───────────────────────────────────────────────────────────────

/// GET /health — liveness probe.
///
/// Always returns HTTP 200 as long as the process is alive.
/// This endpoint must NEVER fail — it does not touch the database
/// or any other external dependency.
pub async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    let uptime = state.start_time.elapsed().as_secs();
    debug!("Liveness check: uptime={}s", uptime);

    let body = serde_json::json!({
        "status": "ok",
        "uptime_seconds": uptime,
        "timestamp": Utc::now().to_rfc3339(),
    });

    (StatusCode::OK, Json(body))
}

/// GET /ready — readiness probe.
///
/// Returns 200 if the database is connected AND the orchestrator has
/// at least one agent registered.  Returns 503 otherwise with a
/// human-readable `reason` field explaining what is not ready.
pub async fn ready_handler(State(state): State<AppState>) -> impl IntoResponse {
    let timestamp = Utc::now().to_rfc3339();

    // Check database connection (releases the lock immediately).
    let db_connected = {
        let db = state.database.lock().await;
        db.is_connected().await
    };

    // Check orchestrator agent count (releases the lock immediately).
    let agent_count = {
        let orchestrator = state.orchestrator.lock().await;
        orchestrator.get_system_status().await.total_agents
    };

    let db_str = if db_connected { "connected" } else { "disconnected" };

    debug!("Readiness check: db={}, agents={}", db_str, agent_count);

    let (status_code, body) = if db_connected && agent_count > 0 {
        (
            StatusCode::OK,
            serde_json::json!({
                "status": "ready",
                "db": db_str,
                "agents": agent_count,
                "timestamp": timestamp,
            }),
        )
    } else {
        let reason = if !db_connected && agent_count == 0 {
            "Database not connected and no agents registered"
        } else if !db_connected {
            "Database not connected"
        } else {
            "No agents registered"
        };

        (
            StatusCode::SERVICE_UNAVAILABLE,
            serde_json::json!({
                "status": "not_ready",
                "db": db_str,
                "agents": agent_count,
                "reason": reason,
                "timestamp": timestamp,
            }),
        )
    };

    (status_code, Json(body))
}

/// GET /metrics — Prometheus metrics endpoint.
///
/// Returns Prometheus text-format metrics with
/// `Content-Type: text/plain; version=0.0.4; charset=utf-8`.
/// If Prometheus is not enabled (controlled by the
/// `MONITOR_ENABLE_PROMETHEUS` environment variable), returns HTTP 404.
///
/// Metrics are generated from the orchestrator's system status and
/// database connection state.  When `SystemMonitor::render_prometheus_metrics()`
/// becomes available in `AppState`, this handler can delegate to it instead.
pub async fn metrics_handler(State(state): State<AppState>) -> impl IntoResponse {
    let prometheus_enabled = std::env::var("MONITOR_ENABLE_PROMETHEUS")
        .ok()
        .and_then(|s| s.parse::<bool>().ok())
        .unwrap_or(false);

    if !prometheus_enabled {
        debug!("Metrics request: Prometheus not enabled, returning 404");
        return (StatusCode::NOT_FOUND, "Prometheus metrics not enabled\n").into_response();
    }

    // Gather system status from the orchestrator.
    let sys = {
        let orchestrator = state.orchestrator.lock().await;
        orchestrator.get_system_status().await
    };

    // Check database connectivity.
    let db_connected = {
        let db = state.database.lock().await;
        db.is_connected().await
    };

    let uptime = state.start_time.elapsed().as_secs();
    let metrics_text = render_prometheus_metrics(&sys, db_connected, uptime);

    debug!("Metrics request: rendered {} bytes", metrics_text.len());

    let mut response = (StatusCode::OK, metrics_text).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
    );
    response
}

// ── Router ─────────────────────────────────────────────────────────────────

/// Build a [`Router`] with all health-check routes.
///
/// Merge this into the main application router:
/// ```ignore
/// let app = Router::new()
///     .merge(health_routes())
///     // ... other routes ...
///     .with_state(state);
/// ```
pub fn health_routes() -> Router<AppState> {
    Router::new()
        .route("/health", get(health_handler))
        .route("/ready", get(ready_handler))
        .route("/metrics", get(metrics_handler))
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Render system metrics in Prometheus text exposition format.
///
/// Produces standard `# HELP` / `# TYPE` comment lines followed by
/// `metric_name value` pairs, as expected by the Prometheus scrape format.
fn render_prometheus_metrics(
    sys: &SystemStatus,
    db_connected: bool,
    uptime_seconds: u64,
) -> String {
    let db_flag = if db_connected { 1 } else { 0 };

    format!(
        "# HELP nexus_agents_total Total number of registered agents\n\
         # TYPE nexus_agents_total gauge\n\
         nexus_agents_total {total_agents}\n\
         \n\
         # HELP nexus_agents_active Number of active agents\n\
         # TYPE nexus_agents_active gauge\n\
         nexus_agents_active {active_agents}\n\
         \n\
         # HELP nexus_agents_idle Number of idle agents\n\
         # TYPE nexus_agents_idle gauge\n\
         nexus_agents_idle {idle_agents}\n\
         \n\
         # HELP nexus_agents_busy Number of busy agents\n\
         # TYPE nexus_agents_busy gauge\n\
         nexus_agents_busy {busy_agents}\n\
         \n\
         # HELP nexus_agents_error Number of agents in error state\n\
         # TYPE nexus_agents_error gauge\n\
         nexus_agents_error {error_agents}\n\
         \n\
         # HELP nexus_tasks_processed_total Total number of tasks processed\n\
         # TYPE nexus_tasks_processed_total counter\n\
         nexus_tasks_processed_total {total_tasks_processed}\n\
         \n\
         # HELP nexus_tasks_failed_total Total number of tasks that failed\n\
         # TYPE nexus_tasks_failed_total counter\n\
         nexus_tasks_failed_total {total_tasks_failed}\n\
         \n\
         # HELP nexus_tasks_in_progress Number of tasks currently in progress\n\
         # TYPE nexus_tasks_in_progress gauge\n\
         nexus_tasks_in_progress {total_tasks_in_progress}\n\
         \n\
         # HELP nexus_health_score System health score (0.0 to 1.0)\n\
         # TYPE nexus_health_score gauge\n\
         nexus_health_score {health_score}\n\
         \n\
         # HELP nexus_db_connected Whether the database is connected (1 = yes, 0 = no)\n\
         # TYPE nexus_db_connected gauge\n\
         nexus_db_connected {db_flag}\n\
         \n\
         # HELP nexus_uptime_seconds Process uptime in seconds\n\
         # TYPE nexus_uptime_seconds gauge\n\
         nexus_uptime_seconds {uptime_seconds}\n",
        total_agents = sys.total_agents,
        active_agents = sys.active_agents,
        idle_agents = sys.idle_agents,
        busy_agents = sys.busy_agents,
        error_agents = sys.error_agents,
        total_tasks_processed = sys.total_tasks_processed,
        total_tasks_failed = sys.total_tasks_failed,
        total_tasks_in_progress = sys.total_tasks_in_progress,
        health_score = sys.health_score,
        db_flag = db_flag,
        uptime_seconds = uptime_seconds,
    )
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ApiConfig;
    use crate::agents::orchestrator::AgentOrchestrator;
    use crate::database::user::SurrealUserRepository;
    use crate::database::Database;
    use crate::NexusLedger;
    use axum::body::to_bytes;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    /// Construct a minimal `AppState` for testing.
    ///
    /// The database is **not** connected and the orchestrator has **zero**
    /// agents — i.e. the system is in a cold-start / not-ready state.
    fn make_test_state() -> AppState {
        let orchestrator = Arc::new(Mutex::new(AgentOrchestrator::new()));
        let database = Arc::new(Mutex::new(Database::new()));
        let nexus = Arc::new(Mutex::new(NexusLedger::new()));
        let user_repo = Arc::new(SurrealUserRepository::new(None));
        let config = ApiConfig::default();
        AppState::new(orchestrator, database, nexus, user_repo, config)
    }

    #[tokio::test]
    async fn test_health_always_returns_200() {
        let state = make_test_state();
        let response = health_handler(State(state)).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);

        // Verify response body shape.
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["status"], "ok");
        assert!(json["uptime_seconds"].as_u64().is_some());
        assert!(json["timestamp"].as_str().is_some());
    }

    #[tokio::test]
    async fn test_ready_returns_503_when_db_not_connected() {
        let state = make_test_state();
        let response = ready_handler(State(state)).await.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        // Verify the body reports the DB as disconnected.
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["status"], "not_ready");
        assert_eq!(json["db"], "disconnected");
        assert_eq!(json["agents"], 0);
        assert!(json["reason"].as_str().is_some());
        assert!(json["timestamp"].as_str().is_some());
    }

    #[test]
    fn test_render_prometheus_metrics_format() {
        let mut sys = SystemStatus::new();
        sys.total_agents = 5;
        sys.active_agents = 3;
        sys.idle_agents = 2;
        sys.busy_agents = 1;
        sys.error_agents = 0;
        sys.total_tasks_processed = 100;
        sys.total_tasks_failed = 5;
        sys.total_tasks_in_progress = 2;
        sys.health_score = 0.95;

        let output = render_prometheus_metrics(&sys, true, 3600);

        // HELP / TYPE comments
        assert!(output.contains("# HELP nexus_agents_total"));
        assert!(output.contains("# TYPE nexus_agents_total gauge"));
        assert!(output.contains("# TYPE nexus_tasks_processed_total counter"));

        // Metric values
        assert!(output.contains("nexus_agents_total 5"));
        assert!(output.contains("nexus_agents_active 3"));
        assert!(output.contains("nexus_agents_idle 2"));
        assert!(output.contains("nexus_agents_busy 1"));
        assert!(output.contains("nexus_agents_error 0"));
        assert!(output.contains("nexus_tasks_processed_total 100"));
        assert!(output.contains("nexus_tasks_failed_total 5"));
        assert!(output.contains("nexus_tasks_in_progress 2"));
        assert!(output.contains("nexus_health_score 0.95"));
        assert!(output.contains("nexus_db_connected 1"));
        assert!(output.contains("nexus_uptime_seconds 3600"));
    }

    #[test]
    fn test_render_prometheus_metrics_db_disconnected() {
        let sys = SystemStatus::new();
        let output = render_prometheus_metrics(&sys, false, 0);
        assert!(output.contains("nexus_db_connected 0"));
        assert!(output.contains("nexus_agents_total 0"));
    }

    #[test]
    fn test_health_routes_builds_without_panic() {
        let _router = health_routes();
    }
}
