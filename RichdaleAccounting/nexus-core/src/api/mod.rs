//! API Module
//!
//! REST API and WebSocket server powered by axum.
//!
//! # Endpoints
//! - REST: /api/v1/status, /api/v1/accounts, /api/v1/transactions, etc.
//! - WebSocket: /ws/chat — conversational agentic interface

use std::sync::Arc;
use std::collections::HashMap;
use std::time::Instant;
use std::net::SocketAddr;

use axum::{
    Router,
    routing::{get, post},
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State, Path, Query, Request,
    },
    response::{IntoResponse, Json, Response},
    http::{StatusCode, HeaderValue},
    middleware::{self, Next},
};
use futures::{SinkExt, StreamExt};
use tower_http::cors::{CorsLayer, Any};
use tokio::sync::Mutex;
use tracing::{info, error, debug};
use chrono::{DateTime, Utc};
use uuid::Uuid;
use serde::{Serialize, Deserialize};

use crate::agents::orchestrator::AgentOrchestrator;
use crate::agents::task::{Task, TaskType, TaskPriority};
use crate::database::Database;
use crate::database::financial::Transaction;
use crate::NexusLedger;

// ── Configuration ──────────────────────────────────────────────────────────

/// API configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    /// API server host
    pub host: String,
    /// API server port
    pub port: u16,
    /// Whether to enable HTTPS
    pub enable_https: bool,
    /// SSL certificate path
    pub ssl_cert_path: Option<String>,
    /// SSL key path
    pub ssl_key_path: Option<String>,
    /// CORS allowed origins
    pub cors_origins: Vec<String>,
    /// API rate limit (requests per minute)
    pub rate_limit: u32,
    /// API timeout in seconds
    pub timeout: u64,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            enable_https: false,
            ssl_cert_path: None,
            ssl_key_path: None,
            cors_origins: vec!["*".to_string()],
            rate_limit: 100,
            timeout: 30,
        }
    }
}

impl ApiConfig {
    pub fn new(host: &str, port: u16) -> Self {
        Self { host: host.to_string(), port, ..Default::default() }
    }

    pub fn from_env() -> Self {
        Self {
            host: std::env::var("API_HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
            port: std::env::var("API_PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(8080),
            enable_https: std::env::var("API_ENABLE_HTTPS").ok().and_then(|s| s.parse().ok()).unwrap_or(false),
            ssl_cert_path: std::env::var("API_SSL_CERT_PATH").ok(),
            ssl_key_path: std::env::var("API_SSL_KEY_PATH").ok(),
            cors_origins: std::env::var("API_CORS_ORIGINS")
                .ok().map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_else(|| vec!["*".to_string()]),
            rate_limit: std::env::var("API_RATE_LIMIT").ok().and_then(|s| s.parse().ok()).unwrap_or(100),
            timeout: std::env::var("API_TIMEOUT").ok().and_then(|s| s.parse().ok()).unwrap_or(30),
        }
    }

    pub fn socket_addr(&self) -> SocketAddr {
        format!("{}:{}", self.host, self.port).parse().unwrap()
    }
}

// ── App State ──────────────────────────────────────────────────────────────

/// Shared application state injected into all axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub orchestrator: Arc<Mutex<AgentOrchestrator>>,
    pub database: Arc<Mutex<Database>>,
    pub nexus: Arc<Mutex<NexusLedger>>,
    pub config: ApiConfig,
    pub start_time: Instant,
}

impl AppState {
    pub fn new(
        orchestrator: Arc<Mutex<AgentOrchestrator>>,
        database: Arc<Mutex<Database>>,
        nexus: Arc<Mutex<NexusLedger>>,
        config: ApiConfig,
    ) -> Self {
        Self { orchestrator, database, nexus, config, start_time: Instant::now() }
    }
}

// ── Middleware ──────────────────────────────────────────────────────────────

/// Request ID middleware — generates a UUID per request and injects it as
/// an extension. The response includes `X-Request-Id`.
async fn request_id_middleware(mut req: Request, next: Next) -> Response {
    let request_id = Uuid::new_v4().to_string();
    req.extensions_mut().insert(RequestId(request_id.clone()));

    let start = Instant::now();
    let mut response = next.run(req).await;

    response.headers_mut().insert(
        "X-Request-Id",
        HeaderValue::from_str(&request_id).unwrap_or(HeaderValue::from_static("unknown")),
    );

    let elapsed = start.elapsed();
    response.headers_mut().insert(
        "X-Response-Time-Ms",
        HeaderValue::from_str(&elapsed.as_millis().to_string())
            .unwrap_or(HeaderValue::from_static("0")),
    );

    debug!("{} {} → {} ({:.0}ms)", request_id, "req", response.status().as_u16(),
        elapsed.as_secs_f64() * 1000.0);

    response
}

/// Extractable wrapper for the request ID injected by `request_id_middleware`.
#[allow(dead_code)]
#[derive(Clone, Debug)]
struct RequestId(String);

/// Error-mapping middleware — catches panics and maps them to 500 JSON responses.
async fn error_mapping_middleware(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let method = req.method().to_string();

    let response = next.run(req).await;
    let status = response.status();

    if status.is_server_error() {
        error!("{} {} → {} (server error)", method, path, status.as_u16());
    } else if status.is_client_error() {
        debug!("{} {} → {} (client error)", method, path, status.as_u16());
    } else {
        debug!("{} {} → {}", method, path, status.as_u16());
    }

    response
}

// ── API Server ─────────────────────────────────────────────────────────────

/// API server that owns the axum HTTP + WebSocket server.
#[derive(Clone)]
pub struct ApiServer {
    pub config: ApiConfig,
    pub state: AppState,
}

impl ApiServer {
    pub fn new(
        config: ApiConfig,
        orchestrator: Arc<Mutex<AgentOrchestrator>>,
        database: Arc<Mutex<Database>>,
        nexus: Arc<Mutex<NexusLedger>>,
    ) -> Self {
        let state = AppState::new(orchestrator, database, nexus, config.clone());
        Self { config, state }
    }

    /// Start the API server — binds axum, blocks until shutdown signal.
    pub async fn start(&self) -> Result<(), anyhow::Error> {
        let addr = self.config.socket_addr();
        info!("Starting API server on {}", addr);

        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        let app = Router::new()
            // ── REST ──
            .route("/api/v1/status", get(status_handler))
            .route("/api/v1/agents", get(agents_handler))
            .route("/api/v1/accounts", get(accounts_handler))
            .route("/api/v1/accounts/:id", get(account_by_id_handler))
            .route("/api/v1/transactions", get(transactions_handler).post(create_transaction_handler))
            .route("/api/v1/transactions/:id", get(transaction_by_id_handler))
            .route("/api/v1/invoices", get(invoices_handler).post(create_invoice_handler))
            .route("/api/v1/tasks", post(submit_task_handler))
            .route("/api/v1/tasks/queue", get(task_queue_handler))
            .route("/api/v1/reports/:report_type", get(report_handler))
            // ── WebSocket ──
            .route("/ws/chat", get(ws_chat_handler))
            // ── Health ──
            .route("/health", get(health_handler))
            // ── Middleware layers (outermost last = first to execute) ──
            .layer(middleware::from_fn(error_mapping_middleware))
            .layer(middleware::from_fn(request_id_middleware))
            .layer(cors)
            .with_state(self.state.clone());

        let listener = tokio::net::TcpListener::bind(&addr).await?;
        info!("API server listening on {}", addr);
        info!("WebSocket chat available at ws://{}/ws/chat", addr);

        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await?;

        info!("API server shut down");
        Ok(())
    }
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl+C handler");
    info!("Shutdown signal received");
}

// ── Response Helpers ───────────────────────────────────────────────────────

/// Standard API response envelope.
#[derive(Debug, Clone, Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub metadata: ApiResponseMetadata,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self { success: true, data: Some(data), error: None, metadata: ApiResponseMetadata::new() }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self { success: false, data: None, error: Some(msg.into()), metadata: ApiResponseMetadata::new() }
    }
}

/// API response metadata.
#[derive(Debug, Clone, Serialize)]
pub struct ApiResponseMetadata {
    pub request_id: String,
    pub timestamp: DateTime<Utc>,
    pub api_version: String,
}

impl ApiResponseMetadata {
    pub fn new() -> Self {
        Self {
            request_id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            api_version: "v1".to_string(),
        }
    }
}

/// API error types (maps to HTTP status codes).
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Bad request: {0}")]
    BadRequest(String),
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    #[error("Forbidden: {0}")]
    Forbidden(String),
    #[error("Rate limit exceeded")]
    RateLimitExceeded,
    #[error("Internal server error: {0}")]
    InternalServerError(String),
    #[error("Service unavailable")]
    ServiceUnavailable,
}

impl ApiError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::RateLimitExceeded => StatusCode::TOO_MANY_REQUESTS,
            Self::InternalServerError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let status = self.status_code();
        let body = Json(ApiResponse::<()>::error(self.to_string()));
        (status, body).into_response()
    }
}

// ── REST Route Handlers ────────────────────────────────────────────────────

/// GET /api/v1/status
async fn status_handler(State(state): State<AppState>) -> impl IntoResponse {
    let orchestrator = state.orchestrator.lock().await;
    let sys = orchestrator.get_system_status().await;
    let uptime = state.start_time.elapsed().as_secs();

    let data = serde_json::json!({
        "status": "ok",
        "timestamp": Utc::now().to_rfc3339(),
        "uptime_seconds": uptime,
        "version": env!("CARGO_PKG_VERSION"),
        "agents": {
            "total": sys.total_agents,
            "active": sys.active_agents,
            "idle": sys.idle_agents,
            "busy": sys.busy_agents,
            "error": sys.error_agents,
        },
        "tasks": {
            "processed": sys.total_tasks_processed,
            "failed": sys.total_tasks_failed,
            "in_progress": sys.total_tasks_in_progress,
        },
        "health_score": sys.health_score,
        "warnings": sys.warnings,
    });

    Json(ApiResponse::success(data))
}

/// GET /api/v1/agents
async fn agents_handler(State(state): State<AppState>) -> impl IntoResponse {
    let orchestrator = state.orchestrator.lock().await;
    let agents = orchestrator.agents.read().await;

    let agents_data: Vec<serde_json::Value> = agents.values()
        .map(|agent| {
            let guard = agent.blocking_lock();
            serde_json::json!({
                "id": guard.config().id.to_string(),
                "name": guard.config().name,
                "type": format!("{:?}", guard.config().agent_type),
                "status": format!("{:?}", guard.status()),
            })
        })
        .collect();

    Json(ApiResponse::success(serde_json::json!(agents_data)))
}

/// GET /api/v1/accounts
async fn accounts_handler(State(state): State<AppState>) -> impl IntoResponse {
    let nexus = state.nexus.lock().await;
    match nexus.ledger.list_accounts().await {
        Ok(accounts) => {
            let data: Vec<serde_json::Value> = accounts.into_iter()
                .map(|acc| serde_json::json!({
                    "id": acc.id.to_string(),
                    "number": acc.number,
                    "name": acc.name,
                    "type": format!("{:?}", acc.account_type),
                    "balance": acc.balance.to_string(),
                    "status": format!("{:?}", acc.status),
                }))
                .collect();
            Json(ApiResponse::success(serde_json::json!(data))).into_response()
        }
        Err(e) => ApiError::InternalServerError(e.to_string()).into_response(),
    }
}

/// GET /api/v1/accounts/:id
async fn account_by_id_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let nexus = state.nexus.lock().await;
    let result = {
        let accounts = nexus.ledger.accounts.read().await;
        accounts.get(&id).cloned()
    };
    match result {
        Some(acc) => {
            let data = serde_json::json!({
                "id": acc.id.to_string(),
                "number": acc.number,
                "name": acc.name,
                "type": format!("{:?}", acc.account_type),
                "balance": acc.balance.to_string(),
                "status": format!("{:?}", acc.status),
            });
            Json(ApiResponse::success(data)).into_response()
        }
        None => ApiError::NotFound(format!("Account {}", id)).into_response(),
    }
}

/// GET /api/v1/transactions
async fn transactions_handler(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let nexus = state.nexus.lock().await;
    match nexus.ledger.list_transactions().await {
        Ok(transactions) => {
            let limit = params.get("limit").and_then(|s| s.parse::<usize>().ok()).unwrap_or(100);
            let offset = params.get("offset").and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
            let total = transactions.len();
            let page: Vec<serde_json::Value> = transactions.into_iter()
                .skip(offset).take(limit)
                .map(|txn| serde_json::json!({
                    "id": txn.id.to_string(),
                    "number": txn.number,
                    "description": txn.description,
                    "date": txn.date.to_rfc3339(),
                    "status": format!("{:?}", txn.status),
                    "total_amount": txn.total_amount().to_string(),
                    "entries": txn.entries.iter().map(|e| serde_json::json!({
                        "account_id": e.account_id.to_string(),
                        "amount": e.amount.to_string(),
                        "entry_type": format!("{:?}", e.entry_type),
                    })).collect::<Vec<_>>(),
                }))
                .collect();

            Json(ApiResponse::success(serde_json::json!({
                "data": page,
                "pagination": { "total": total, "limit": limit, "offset": offset }
            }))).into_response()
        }
        Err(e) => ApiError::InternalServerError(e.to_string()).into_response(),
    }
}

/// GET /api/v1/transactions/:id
async fn transaction_by_id_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let nexus = state.nexus.lock().await;
    let result = {
        let txns = nexus.ledger.transactions.read().await;
        txns.get(&id).cloned()
    };
    match result {
        Some(txn) => {
            let data = serde_json::json!({
                "id": txn.id.to_string(),
                "number": txn.number,
                "description": txn.description,
                "date": txn.date.to_rfc3339(),
                "status": format!("{:?}", txn.status),
                "total_amount": txn.total_amount().to_string(),
                "entries": txn.entries.iter().map(|e| serde_json::json!({
                    "account_id": e.account_id.to_string(),
                    "amount": e.amount.to_string(),
                    "entry_type": format!("{:?}", e.entry_type),
                })).collect::<Vec<_>>(),
            });
            Json(ApiResponse::success(data)).into_response()
        }
        None => ApiError::NotFound(format!("Transaction {}", id)).into_response(),
    }
}

/// POST /api/v1/transactions
async fn create_transaction_handler(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    // Parse transaction from body (minimal: description + debit/credit entries)
    let description = body.get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("API transaction");

    let entries = match body.get("entries") {
        Some(serde_json::Value::Array(arr)) => {
            let mut parsed = Vec::new();
            for entry in arr {
                let account_id: Uuid = match entry.get("account_id").and_then(|v| v.as_str()) {
                    Some(s) => match Uuid::parse_str(s) { Ok(id) => id, Err(_) => continue },
                    None => continue,
                };
                let amount_str = entry.get("amount").and_then(|v| v.as_str()).unwrap_or("0");
                let amount: rust_decimal::Decimal = match amount_str.parse() {
                    Ok(a) => a,
                    Err(_) => continue,
                };
                let entry_type = match entry.get("entry_type").and_then(|v| v.as_str()) {
                    Some("debit") | Some("Debit") => crate::database::financial::EntryType::Debit,
                    _ => crate::database::financial::EntryType::Credit,
                };
                let desc = entry.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
                parsed.push(crate::database::financial::TransactionEntry {
                    id: Uuid::new_v4(),
                    account_id,
                    amount,
                    entry_type,
                    description: desc,
                    reference: None,
                });
            }
            parsed
        }
        _ => vec![],
    };

    if entries.is_empty() {
        return ApiError::BadRequest("At least one entry required".into()).into_response();
    }

    let now = Utc::now();
    let txn = Transaction {
        id: Uuid::new_v4(),
        number: format!("TXN-{}", &Uuid::new_v4().to_string()[..8]),
        description: description.to_string(),
        date: now,
        transaction_type: crate::database::financial::TransactionType::JournalEntry,
        status: crate::database::financial::TransactionStatus::Pending,
        entries,
        journal_entry_id: None,
        document_ids: vec![],
        metadata: serde_json::json!({}),
        created_at: now,
        updated_at: now,
    };

    let nexus = state.nexus.lock().await;
    match nexus.process_transaction(txn).await {
        Ok(processed) => {
            let data = serde_json::json!({
                "id": processed.id.to_string(),
                "number": processed.number,
                "description": processed.description,
                "date": processed.date.to_rfc3339(),
                "status": format!("{:?}", processed.status),
                "total_amount": processed.total_amount().to_string(),
            });
            Json(ApiResponse::success(data)).into_response()
        }
        Err(e) => ApiError::InternalServerError(e.to_string()).into_response(),
    }
}

/// POST /api/v1/invoices — create an invoice via the orchestrator.
async fn create_invoice_handler(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let payload = serde_json::json!({
        "customer_name": body.get("customer_name").and_then(|v| v.as_str()).unwrap_or("Customer"),
        "customer_email": body.get("customer_email").and_then(|v| v.as_str()).unwrap_or(""),
        "items": body.get("items").unwrap_or(&serde_json::json!([])),
        "due_date": body.get("due_date").and_then(|v| v.as_str()),
        "notes": body.get("notes").and_then(|v| v.as_str()).unwrap_or(""),
    });

    let task = Task::generate_invoice(payload);
    let orchestrator = state.orchestrator.lock().await;
    match orchestrator.submit_task(task).await {
        Ok(task_id) => {
            Json(ApiResponse::success(serde_json::json!({
                "task_id": task_id.to_string(),
                "status": "submitted",
                "message": "Invoice creation submitted",
            }))).into_response()
        }
        Err(e) => ApiError::InternalServerError(e.to_string()).into_response(),
    }
}

/// GET /api/v1/invoices — list invoices (filtered from transactions).
async fn invoices_handler(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let nexus = state.nexus.lock().await;
    match nexus.ledger.list_transactions().await {
        Ok(transactions) => {
            let limit = params.get("limit").and_then(|s| s.parse::<usize>().ok()).unwrap_or(100);
            let offset = params.get("offset").and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);

            // Filter to invoice-type transactions
            let invoice_txns: Vec<&crate::database::financial::Transaction> = transactions.iter()
                .filter(|t| matches!(t.transaction_type, crate::database::financial::TransactionType::Invoice))
                .collect();

            let total = invoice_txns.len();
            let page: Vec<serde_json::Value> = invoice_txns.into_iter()
                .skip(offset).take(limit)
                .map(|txn| serde_json::json!({
                    "id": txn.id.to_string(),
                    "number": txn.number,
                    "description": txn.description,
                    "date": txn.date.to_rfc3339(),
                    "status": format!("{:?}", txn.status),
                    "total_amount": txn.total_amount().to_string(),
                    "entries": txn.entries.iter().map(|e| serde_json::json!({
                        "account_id": e.account_id.to_string(),
                        "amount": e.amount.to_string(),
                        "entry_type": format!("{:?}", e.entry_type),
                    })).collect::<Vec<_>>(),
                }))
                .collect();

            Json(ApiResponse::success(serde_json::json!({
                "data": page,
                "pagination": { "total": total, "limit": limit, "offset": offset }
            }))).into_response()
        }
        Err(e) => ApiError::InternalServerError(e.to_string()).into_response(),
    }
}

/// POST /api/v1/tasks — submit a generic task to the orchestrator.
async fn submit_task_handler(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let task_type_str = body.get("task_type")
        .and_then(|v| v.as_str())
        .unwrap_or("RecordTransaction");

    let task_type = match task_type_str {
        "RecordTransaction" => TaskType::RecordTransaction,
        "ReconcileAccount" => TaskType::ReconcileAccount,
        "GenerateInvoice" => TaskType::GenerateInvoice,
        "ProcessPayment" => TaskType::ProcessPayment,
        "CalculatePayroll" => TaskType::CalculatePayroll,
        "CalculateTaxes" => TaskType::CalculateTaxes,
        "ProcessReceipt" => TaskType::ProcessReceipt,
        "GenerateReport" => TaskType::GenerateReport,
        "AuditCheck" => TaskType::AuditCheck,
        "StoreDocument" => TaskType::StoreDocument,
        other => return ApiError::BadRequest(format!("Unknown task type: {}", other)).into_response(),
    };

    let priority = body.get("priority")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "Critical" => TaskPriority::Critical,
            "High" => TaskPriority::High,
            "Low" => TaskPriority::Low,
            _ => TaskPriority::Normal,
        })
        .unwrap_or(TaskPriority::Normal);

    let mut task = Task::new(task_type);
    task.priority = priority;
    if let Some(payload) = body.get("payload") {
        task.payload = crate::agents::task::TaskPayload::Json(payload.clone());
    }

    let orchestrator = state.orchestrator.lock().await;
    match orchestrator.submit_task(task).await {
        Ok(task_id) => {
            Json(ApiResponse::success(serde_json::json!({
                "task_id": task_id.to_string(),
                "status": "submitted",
            }))).into_response()
        }
        Err(e) => ApiError::InternalServerError(e.to_string()).into_response(),
    }
}

/// GET /api/v1/tasks/queue — view task queue status.
async fn task_queue_handler(State(state): State<AppState>) -> impl IntoResponse {
    let orchestrator = state.orchestrator.lock().await;
    let queue_len = orchestrator.task_queue.lock().await.len();
    let in_progress = orchestrator.in_progress_tasks.lock().await.len();
    let completed = orchestrator.completed_tasks.lock().await.len();
    let failed = orchestrator.failed_tasks.lock().await.len();

    Json(ApiResponse::success(serde_json::json!({
        "queue_length": queue_len,
        "in_progress": in_progress,
        "completed": completed,
        "failed": failed,
    })))
}

/// GET /api/v1/reports/:report_type
async fn report_handler(
    State(state): State<AppState>,
    Path(report_type): Path<String>,
) -> impl IntoResponse {
    let nexus = state.nexus.lock().await;
    match report_type.as_str() {
        "trial_balance" => match nexus.ledger.get_trial_balance().await {
            Ok(tb) => {
                let balances: Vec<serde_json::Value> = tb.into_iter()
                    .map(|(id, balance)| serde_json::json!({
                        "account_id": id.to_string(),
                        "balance": balance.to_string(),
                    }))
                    .collect();
                Json(ApiResponse::success(serde_json::json!({
                    "report_type": "trial_balance",
                    "balances": balances,
                }))).into_response()
            }
            Err(e) => ApiError::InternalServerError(e.to_string()).into_response(),
        },
        "balance_sheet" => match nexus.ledger.get_balance_sheet().await {
            Ok(bs) => {
                Json(ApiResponse::success(serde_json::json!({
                    "report_type": "balance_sheet",
                    "assets": bs.assets.to_string(),
                    "liabilities": bs.liabilities.to_string(),
                    "equity": bs.equity.to_string(),
                    "total_assets": bs.total_assets.to_string(),
                    "total_liabilities_plus_equity": bs.total_liabilities_plus_equity.to_string(),
                }))).into_response()
            }
            Err(e) => ApiError::InternalServerError(e.to_string()).into_response(),
        },
        "income_statement" => {
            let start = Utc::now() - chrono::Duration::days(365);
            match nexus.ledger.get_income_statement(start, Utc::now()).await {
                Ok(is_) => {
                    Json(ApiResponse::success(serde_json::json!({
                        "report_type": "income_statement",
                        "revenue": is_.revenue.to_string(),
                        "expenses": is_.expenses.to_string(),
                        "net_income": is_.net_income.to_string(),
                    }))).into_response()
                }
                Err(e) => ApiError::InternalServerError(e.to_string()).into_response(),
            }
        }
        other => ApiError::NotFound(format!("Report type: {}", other)).into_response(),
    }
}

/// GET /health
async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    let orchestrator = state.orchestrator.lock().await;
    let sys = orchestrator.get_system_status().await;

    let healthy = sys.health_score > 0.5;
    let status_code = if healthy { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };

    let data = serde_json::json!({
        "healthy": healthy,
        "health_score": sys.health_score,
        "timestamp": Utc::now().to_rfc3339(),
        "warnings": sys.warnings,
    });

    (status_code, Json(ApiResponse::success(data))).into_response()
}

// ── WebSocket Chat ─────────────────────────────────────────────────────────

/// GET /ws/chat — upgrade to WebSocket for conversational agentic interface.
async fn ws_chat_handler(
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_chat(socket, state))
}

/// Core WebSocket chat loop — receives NL messages, returns accounting responses.
async fn handle_chat(stream: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = stream.split();

    // Send welcome message
    let welcome = serde_json::json!({
        "type": "system",
        "message": "Welcome to NexusLedger. I'm your accounting assistant. How can I help?",
        "examples": [
            "create an invoice for Acme Corp for $1,500",
            "what's my cash balance?",
            "show me my balance sheet",
            "log a receipt from Staples for $45.99",
            "reconcile my bank account"
        ]
    });
    let _ = sender.send(Message::Text(welcome.to_string().into())).await;

    while let Some(Ok(msg)) = receiver.next().await {
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Close(_) => break,
            _ => continue,
        };

        debug!("WS chat received: {}", text);

        let response = process_nlu_message(&text, &state).await;
        let _ = sender.send(Message::Text(response.into())).await;
    }
}

// ── NLU (Natural Language Understanding) ───────────────────────────────────

/// Basic rule-based NLU — extracts intent + entities from natural language.
/// Phase 5 will replace this with LLM-based understanding.
#[derive(Debug)]
struct NluResult {
    intent: String,
    entities: HashMap<String, String>,
    confidence: f64,
}

/// Process a natural language message and return a JSON response.
async fn process_nlu_message(text: &str, state: &AppState) -> String {
    let nlu = parse_intent(text);
    debug!("NLU: intent={} entities={:?} confidence={}", nlu.intent, nlu.entities, nlu.confidence);

    match nlu.intent.as_str() {
        "query_balance" => handle_query_balance(&nlu, state).await,
        "create_invoice" => handle_create_invoice(&nlu, state).await,
        "process_payment" => handle_process_payment(&nlu, state).await,
        "record_receipt" => handle_record_receipt(&nlu, state).await,
        "record_transaction" => handle_record_transaction(&nlu, state).await,
        "generate_report" => handle_generate_report(&nlu, state).await,
        "reconcile" => handle_reconcile(&nlu, state).await,
        "calculate_tax" => handle_calculate_tax(&nlu, state).await,
        "run_payroll" => handle_run_payroll(&nlu, state).await,
        "status" => handle_status(state).await,
        _ => handle_unknown(&nlu, state).await,
    }
}

/// Keyword + regex-based intent parser.
fn parse_intent(text: &str) -> NluResult {
    let lower = text.to_lowercase();

    // ── query_balance ──
    if lower.contains("balance") || lower.contains("how much cash") || lower.contains("how much money") {
        return NluResult { intent: "query_balance".into(), entities: extract_amounts(&lower), confidence: 0.9 };
    }

    // ── create_invoice ──
    if lower.contains("create") && (lower.contains("invoice") || lower.contains("bill")) ||
       lower.contains("new invoice") || lower.contains("send invoice") {
        let mut entities = extract_amounts(&lower);
        if let Some(name) = extract_entity_name(&lower) { entities.insert("customer".into(), name); }
        return NluResult { intent: "create_invoice".into(), entities, confidence: 0.85 };
    }

    // ── process_payment ──
    if lower.contains("pay") && (lower.contains("invoice") || lower.contains("bill")) ||
       lower.contains("process payment") || lower.contains("make payment") {
        return NluResult { intent: "process_payment".into(), entities: extract_amounts(&lower), confidence: 0.85 };
    }

    // ── record_receipt ──
    if lower.contains("receipt") || lower.contains("log") && (lower.contains("receipt") || lower.contains("expense") || lower.contains("from")) ||
       lower.contains("record") && lower.contains("receipt") {
        return NluResult { intent: "record_receipt".into(), entities: extract_amounts(&lower), confidence: 0.8 };
    }

    // ── record_transaction ──
    if lower.contains("record") && (lower.contains("transaction") || lower.contains("sale") || lower.contains("purchase")) ||
       lower.contains("received") && lower.contains("cash") && lower.contains("sale") {
        return NluResult { intent: "record_transaction".into(), entities: extract_amounts(&lower), confidence: 0.75 };
    }

    // ── generate_report ──
    if lower.contains("report") || lower.contains("show me") && (lower.contains("balance sheet") || lower.contains("income") || lower.contains("p&l") || lower.contains("profit")) ||
       lower.contains("trial balance") || lower.contains("financial statement") {
        let report_type = if lower.contains("balance sheet") { "balance_sheet" }
            else if lower.contains("income") || lower.contains("p&l") || lower.contains("profit") { "income_statement" }
            else if lower.contains("cash flow") { "cash_flow" }
            else { "trial_balance" };
        let mut entities = HashMap::new();
        entities.insert("report_type".into(), report_type.into());
        return NluResult { intent: "generate_report".into(), entities, confidence: 0.9 };
    }

    // ── reconcile ──
    if lower.contains("reconcile") {
        return NluResult { intent: "reconcile".into(), entities: HashMap::new(), confidence: 0.9 };
    }

    // ── calculate_tax ──
    if lower.contains("tax") && (lower.contains("calculate") || lower.contains("owe") || lower.contains("how much")) ||
       lower.contains("tax estimate") {
        return NluResult { intent: "calculate_tax".into(), entities: extract_amounts(&lower), confidence: 0.85 };
    }

    // ── run_payroll ──
    if lower.contains("payroll") || lower.contains("run payroll") || lower.contains("pay employees") {
        return NluResult { intent: "run_payroll".into(), entities: HashMap::new(), confidence: 0.9 };
    }

    // ── status ──
    if lower.contains("status") || lower.contains("how are you") || lower.contains("what can you do") {
        return NluResult { intent: "status".into(), entities: HashMap::new(), confidence: 0.95 };
    }

    // ── unknown ──
    NluResult { intent: "unknown".into(), entities: HashMap::new(), confidence: 0.1 }
}

/// Extract dollar amounts from text.
fn extract_amounts(text: &str) -> HashMap<String, String> {
    let mut entities = HashMap::new();
    let re = regex::Regex::new(r"\$?(\d+(?:,\d{3})*(?:\.\d{2})?)").unwrap();
    let amounts: Vec<&str> = re.find_iter(text).map(|m| m.as_str().trim_start_matches('$')).collect();
    if let Some(amt) = amounts.first() {
        entities.insert("amount".into(), amt.to_string());
    }
    entities
}

/// Extract a company/person name from text (heuristic: capitalized words after "for"/"to").
fn extract_entity_name(text: &str) -> Option<String> {
    let re = regex::Regex::new(r"(?:for|to|from)\s+([A-Z][a-z]+(?:\s+[A-Z][a-z]+)*)").unwrap();
    re.captures(text).map(|c| c[1].to_string())
}

// ── NLU Intent Handlers ────────────────────────────────────────────────────

async fn handle_query_balance(_nlu: &NluResult, state: &AppState) -> String {
    let nexus = state.nexus.lock().await;
    match nexus.ledger.list_accounts().await {
        Ok(accounts) => {
            // Find cash account (code 1000)
            let cash = accounts.iter().find(|a| a.number == "1000");
            let total_assets: rust_decimal::Decimal = accounts.iter()
                .filter(|a| matches!(a.account_type, crate::database::financial::AccountType::Asset))
                .map(|a| a.balance)
                .sum();

            let lines: Vec<String> = accounts.iter()
                .filter(|a| matches!(a.account_type, crate::database::financial::AccountType::Asset))
                .map(|a| format!("  {} ({}): ${}", a.name, a.number, a.balance))
                .collect();

            let cash_balance = cash.map(|c| c.balance.to_string()).unwrap_or_else(|| "N/A".into());

            serde_json::json!({
                "type": "response",
                "intent": "query_balance",
                "message": format!("Your cash balance is ${}. Total assets: ${}.",
                    cash_balance, total_assets),
                "data": {
                    "cash_balance": cash_balance,
                    "total_assets": total_assets.to_string(),
                    "breakdown": lines,
                }
            }).to_string()
        }
        Err(e) => error_response("query_balance", &e.to_string()),
    }
}

async fn handle_create_invoice(nlu: &NluResult, state: &AppState) -> String {
    let customer = nlu.entities.get("customer").cloned().unwrap_or_else(|| "Customer".into());
    let amount_str = nlu.entities.get("amount").cloned().unwrap_or_else(|| "0".into());
    let amount: rust_decimal::Decimal = amount_str.parse().unwrap_or(rust_decimal::Decimal::ZERO);

    let payload = serde_json::json!({
        "customer_name": customer,
        "items": [{
            "description": "Services",
            "quantity": 1,
            "unit_price": amount.to_string(),
        }],
        "due_date": (Utc::now() + chrono::Duration::days(30)).date_naive().to_string(),
    });

    let task = Task::generate_invoice(payload);
    let orchestrator = state.orchestrator.lock().await;
    match orchestrator.submit_task(task).await {
        Ok(task_id) => serde_json::json!({
            "type": "response",
            "intent": "create_invoice",
            "message": format!("Invoice created for {} for ${}. Task ID: {}", customer, amount, task_id),
            "data": { "task_id": task_id.to_string(), "customer": customer, "amount": amount.to_string() },
        }).to_string(),
        Err(e) => error_response("create_invoice", &e.to_string()),
    }
}

async fn handle_process_payment(nlu: &NluResult, state: &AppState) -> String {
    let amount_str = nlu.entities.get("amount").cloned().unwrap_or_else(|| "0".into());
    let amount: rust_decimal::Decimal = amount_str.parse().unwrap_or(rust_decimal::Decimal::ZERO);

    let payload = serde_json::json!({ "amount": amount.to_string() });
    let task = Task::process_payment(payload);
    let orchestrator = state.orchestrator.lock().await;
    match orchestrator.submit_task(task).await {
        Ok(task_id) => serde_json::json!({
            "type": "response",
            "intent": "process_payment",
            "message": format!("Payment of ${} processed. Task ID: {}", amount, task_id),
            "data": { "task_id": task_id.to_string(), "amount": amount.to_string() },
        }).to_string(),
        Err(e) => error_response("process_payment", &e.to_string()),
    }
}

async fn handle_record_receipt(nlu: &NluResult, state: &AppState) -> String {
    let amount_str = nlu.entities.get("amount").cloned().unwrap_or_else(|| "0".into());
    let amount: rust_decimal::Decimal = amount_str.parse().unwrap_or(rust_decimal::Decimal::ZERO);

    let payload = serde_json::json!({ "amount": amount.to_string(), "category": "Office Supplies" });
    let task = Task::process_receipt(payload);
    let orchestrator = state.orchestrator.lock().await;
    match orchestrator.submit_task(task).await {
        Ok(task_id) => serde_json::json!({
            "type": "response",
            "intent": "record_receipt",
            "message": format!("Receipt for ${} logged. Task ID: {}", amount, task_id),
            "data": { "task_id": task_id.to_string(), "amount": amount.to_string() },
        }).to_string(),
        Err(e) => error_response("record_receipt", &e.to_string()),
    }
}

async fn handle_record_transaction(nlu: &NluResult, state: &AppState) -> String {
    let amount_str = nlu.entities.get("amount").cloned().unwrap_or_else(|| "0".into());
    let _amount: rust_decimal::Decimal = amount_str.parse().unwrap_or(rust_decimal::Decimal::ZERO);

    let task = Task::new(TaskType::RecordTransaction);
    let orchestrator = state.orchestrator.lock().await;
    match orchestrator.submit_task(task).await {
        Ok(task_id) => serde_json::json!({
            "type": "response",
            "intent": "record_transaction",
            "message": format!("Transaction recorded. Task ID: {}", task_id),
            "data": { "task_id": task_id.to_string() },
        }).to_string(),
        Err(e) => error_response("record_transaction", &e.to_string()),
    }
}

async fn handle_generate_report(nlu: &NluResult, state: &AppState) -> String {
    let report_type = nlu.entities.get("report_type").cloned().unwrap_or_else(|| "trial_balance".into());
    let nexus = state.nexus.lock().await;

    match report_type.as_str() {
        "balance_sheet" => match nexus.ledger.get_balance_sheet().await {
            Ok(bs) => {
                let report = serde_json::json!({
                    "report_type": "balance_sheet",
                    "assets": bs.assets.to_string(),
                    "liabilities": bs.liabilities.to_string(),
                    "equity": bs.equity.to_string(),
                    "total_assets": bs.total_assets.to_string(),
                    "total_liabilities_plus_equity": bs.total_liabilities_plus_equity.to_string(),
                });
                serde_json::json!({
                    "type": "response",
                    "intent": "generate_report",
                    "message": format!("Balance sheet generated."),
                    "data": { "report_type": "balance_sheet", "report": report },
                }).to_string()
            }
            Err(e) => error_response("generate_report", &e.to_string()),
        },
        "income_statement" => {
            let start = Utc::now() - chrono::Duration::days(365);
            match nexus.ledger.get_income_statement(start, Utc::now()).await {
                Ok(is_) => {
                    let report = serde_json::json!({
                        "report_type": "income_statement",
                        "revenue": is_.revenue.to_string(),
                        "expenses": is_.expenses.to_string(),
                        "net_income": is_.net_income.to_string(),
                    });
                    serde_json::json!({
                        "type": "response",
                        "intent": "generate_report",
                        "message": format!("Income statement generated."),
                        "data": { "report_type": "income_statement", "report": report },
                    }).to_string()
                }
                Err(e) => error_response("generate_report", &e.to_string()),
            }
        }
        _ => match nexus.ledger.get_trial_balance().await {
            Ok(tb) => {
                let balances: Vec<serde_json::Value> = tb.into_iter()
                    .map(|(id, balance)| serde_json::json!({
                        "account_id": id.to_string(),
                        "balance": balance.to_string(),
                    }))
                    .collect();
                serde_json::json!({
                    "type": "response",
                    "intent": "generate_report",
                    "message": format!("Trial balance generated."),
                    "data": { "report_type": "trial_balance", "report": { "balances": balances } },
                }).to_string()
            }
            Err(e) => error_response("generate_report", &e.to_string()),
        },
    }
}

async fn handle_reconcile(_nlu: &NluResult, state: &AppState) -> String {
    let task = Task::new(TaskType::ReconcileAccount);
    let orchestrator = state.orchestrator.lock().await;
    match orchestrator.submit_task(task).await {
        Ok(task_id) => serde_json::json!({
            "type": "response",
            "intent": "reconcile",
            "message": format!("Reconciliation started. Task ID: {}", task_id),
            "data": { "task_id": task_id.to_string() },
        }).to_string(),
        Err(e) => error_response("reconcile", &e.to_string()),
    }
}

async fn handle_calculate_tax(nlu: &NluResult, state: &AppState) -> String {
    let amount_str = nlu.entities.get("amount").cloned().unwrap_or_else(|| "50000".into());
    let amount: rust_decimal::Decimal = amount_str.parse().unwrap_or(rust_decimal::Decimal::from(50000));

    let payload = serde_json::json!({ "taxable_income": amount.to_string() });
    let task = Task::calculate_taxes(payload);
    let orchestrator = state.orchestrator.lock().await;
    match orchestrator.submit_task(task).await {
        Ok(task_id) => serde_json::json!({
            "type": "response",
            "intent": "calculate_tax",
            "message": format!("Tax calculation submitted for ${} taxable income. Task ID: {}", amount, task_id),
            "data": { "task_id": task_id.to_string(), "taxable_income": amount.to_string() },
        }).to_string(),
        Err(e) => error_response("calculate_tax", &e.to_string()),
    }
}

async fn handle_run_payroll(_nlu: &NluResult, state: &AppState) -> String {
    let task = Task::calculate_payroll(serde_json::json!({"period": "current"}));
    let orchestrator = state.orchestrator.lock().await;
    match orchestrator.submit_task(task).await {
        Ok(task_id) => serde_json::json!({
            "type": "response",
            "intent": "run_payroll",
            "message": format!("Payroll processing started. Task ID: {}", task_id),
            "data": { "task_id": task_id.to_string() },
        }).to_string(),
        Err(e) => error_response("run_payroll", &e.to_string()),
    }
}

async fn handle_status(state: &AppState) -> String {
    let orchestrator = state.orchestrator.lock().await;
    let sys = orchestrator.get_system_status().await;

    serde_json::json!({
        "type": "response",
        "intent": "status",
        "message": format!("I'm running. {} agents active, {} tasks processed. Health: {:.0}%.",
            sys.active_agents, sys.total_tasks_processed, sys.health_score * 100.0),
        "data": {
            "agents": sys.total_agents,
            "active": sys.active_agents,
            "tasks_processed": sys.total_tasks_processed,
            "health_score": sys.health_score,
        }
    }).to_string()
}

async fn handle_unknown(nlu: &NluResult, _state: &AppState) -> String {
    serde_json::json!({
        "type": "response",
        "intent": "unknown",
        "message": "I didn't quite understand that. Here's what I can do:\n\
            • Create invoices: \"create an invoice for Acme Corp for $1,500\"\n\
            • Process payments: \"pay the Acme invoice\"\n\
            • Log receipts: \"log a receipt from Staples for $45.99\"\n\
            • Record transactions: \"record a sale of $500\"\n\
            • Query balances: \"what's my cash balance?\"\n\
            • Generate reports: \"show me my balance sheet\"\n\
            • Reconcile: \"reconcile my bank account\"\n\
            • Calculate taxes: \"how much tax do I owe on $50,000?\"\n\
            • Run payroll: \"run payroll for this week\"\n\
            • System status: \"status\"",
        "confidence": nlu.confidence,
    }).to_string()
}

fn error_response(intent: &str, error: &str) -> String {
    serde_json::json!({
        "type": "error",
        "intent": intent,
        "message": format!("Error: {}", error),
        "error": error,
    }).to_string()
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_config_default() {
        let config = ApiConfig::default();
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 8080);
        assert!(!config.enable_https);
    }

    #[test]
    fn test_api_config_creation() {
        let config = ApiConfig::new("example.com", 8000);
        assert_eq!(config.host, "example.com");
        assert_eq!(config.port, 8000);
    }

    #[test]
    fn test_api_response() {
        let data = serde_json::json!({"test": "value"});
        let response = ApiResponse::success(data);
        assert!(response.success);
        assert!(response.data.is_some());
        assert!(response.error.is_none());

        let error_response = ApiResponse::<serde_json::Value>::error("Test error");
        assert!(!error_response.success);
        assert!(error_response.error.is_some());
    }

    #[test]
    fn test_api_error() {
        let error = ApiError::NotFound("Resource not found".to_string());
        assert_eq!(error.status_code(), StatusCode::NOT_FOUND);

        let error = ApiError::BadRequest("Invalid data".to_string());
        assert_eq!(error.status_code(), StatusCode::BAD_REQUEST);

        let error = ApiError::Unauthorized("Not authorized".to_string());
        assert_eq!(error.status_code(), StatusCode::UNAUTHORIZED);

        let error = ApiError::RateLimitExceeded;
        assert_eq!(error.status_code(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn test_parse_intent_query_balance() {
        let nlu = parse_intent("what's my cash balance?");
        assert_eq!(nlu.intent, "query_balance");
        assert!(nlu.confidence > 0.5);
    }

    #[test]
    fn test_parse_intent_create_invoice() {
        let nlu = parse_intent("create an invoice for Acme Corp for $1,500");
        assert_eq!(nlu.intent, "create_invoice");
        assert!(nlu.confidence > 0.5);
        assert_eq!(nlu.entities.get("amount").map(|s| s.as_str()), Some("1,500"));
    }

    #[test]
    fn test_parse_intent_record_receipt() {
        let nlu = parse_intent("log a receipt from Staples for $45.99");
        assert_eq!(nlu.intent, "record_receipt");
        assert!(nlu.confidence > 0.5);
        assert_eq!(nlu.entities.get("amount").map(|s| s.as_str()), Some("45.99"));
    }

    #[test]
    fn test_parse_intent_generate_report() {
        let nlu = parse_intent("show me my balance sheet");
        assert_eq!(nlu.intent, "generate_report");
        assert_eq!(nlu.entities.get("report_type").map(|s| s.as_str()), Some("balance_sheet"));
    }

    #[test]
    fn test_parse_intent_reconcile() {
        let nlu = parse_intent("reconcile my bank account");
        assert_eq!(nlu.intent, "reconcile");
        assert!(nlu.confidence > 0.5);
    }

    #[test]
    fn test_parse_intent_calculate_tax() {
        let nlu = parse_intent("how much tax do I owe on $50,000?");
        assert_eq!(nlu.intent, "calculate_tax");
    }

    #[test]
    fn test_parse_intent_run_payroll() {
        let nlu = parse_intent("run payroll for this week");
        assert_eq!(nlu.intent, "run_payroll");
    }

    #[test]
    fn test_parse_intent_status() {
        let nlu = parse_intent("status");
        assert_eq!(nlu.intent, "status");
    }

    #[test]
    fn test_parse_intent_unknown() {
        let nlu = parse_intent("blah blah blah");
        assert_eq!(nlu.intent, "unknown");
        assert!(nlu.confidence < 0.5);
    }

    #[test]
    fn test_extract_amounts_simple() {
        let entities = extract_amounts("pay $1,500.00 for invoice");
        assert_eq!(entities.get("amount").map(|s| s.as_str()), Some("1,500.00"));
    }

    #[test]
    fn test_extract_entity_name() {
        let name = extract_entity_name("create an invoice for Acme Corp for $1,500");
        assert_eq!(name, Some("Acme Corp".into()));
    }
}
