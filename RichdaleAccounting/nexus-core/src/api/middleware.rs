//! Middleware — Rate Limiting & CSRF Protection
//!
//! # Rate Limiting
//!
//! Token-bucket rate limiter implemented with `std::collections::HashMap`
//! guarded by `tokio::sync::Mutex`. Each key (user UUID or client IP) gets
//! an independent bucket with per-role or per-endpoint limits.
//!
//! ## Per-role limits (requests / minute)
//! | Role    | Steady-state | Burst (2×) |
//! |---------|-------------|------------|
//! | Admin   | 1000        | 2000       |
//! | Manager | 500         | 1000       |
//! | User    | 100         | 200        |
//! | Viewer  | 50          | 100        |
//! | Guest   | 20          | 40         |
//!
//! ## Per-endpoint overrides
//! - `/api/auth/login`    — 10/min (burst 20), keyed by IP
//! - `/api/auth/register` — 5/min  (burst 10), keyed by IP
//!
//! ## Response headers (all responses)
//! - `X-RateLimit-Limit`     — steady-state limit (req/min)
//! - `X-RateLimit-Remaining` — tokens remaining in the bucket
//! - `X-RateLimit-Reset`     — Unix timestamp when the bucket refills to capacity
//!
//! On limit exceeded: HTTP 429 with `Retry-After` header and body
//! `{"success":false,"error":"Rate limit exceeded","retry_after":N}`.
//!
//! # CSRF Protection
//!
//! Double-submit cookie pattern for state-changing requests (POST/PUT/DELETE).
//! Reads the `XSRF-TOKEN` cookie and compares it with the `X-XSRF-TOKEN`
//! header. Returns 403 if either is missing or they don't match.
//! Skips `/api/auth/*` paths and GET/OPTIONS methods.
//!
//! # Wiring
//!
//! ```ignore
//! use nexus_core::api::middleware::{init_rate_limiter, RateLimitConfig, rate_limit_middleware};
//! use axum::middleware;
//!
//! init_rate_limiter(RateLimitConfig::default());
//!
//! let app = Router::new()
//!     // ... routes ...
//!     .layer(middleware::from_fn(rate_limit_middleware));
//! ```

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use axum::{
    extract::Request,
    http::{header, HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::api::auth::AuthUser;
use crate::database::models::UserRole;

// ── Configuration ──────────────────────────────────────────────────────────

/// Per-role and per-endpoint rate limit configuration (requests per minute).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum requests per minute for Admin role.
    pub admin_limit: u32,
    /// Maximum requests per minute for Manager role.
    pub manager_limit: u32,
    /// Maximum requests per minute for User role.
    pub user_limit: u32,
    /// Maximum requests per minute for Viewer role.
    pub viewer_limit: u32,
    /// Maximum requests per minute for Guest / unauthenticated requests.
    pub guest_limit: u32,
    /// Maximum requests per minute for the `/api/auth/login` endpoint.
    pub login_limit: u32,
    /// Maximum requests per minute for the `/api/auth/register` endpoint.
    pub register_limit: u32,
    /// Paths exempt from rate limiting (matched exactly).
    pub exempt_paths: Vec<String>,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            admin_limit: 1000,
            manager_limit: 500,
            user_limit: 100,
            viewer_limit: 50,
            guest_limit: 20,
            login_limit: 10,
            register_limit: 5,
            exempt_paths: vec![
                "/health".to_string(),
                "/ready".to_string(),
                "/metrics".to_string(),
            ],
        }
    }
}

impl RateLimitConfig {
    /// Create a new config from environment variables, falling back to defaults.
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Some(v) = std::env::var("RATE_LIMIT_ADMIN").ok().and_then(|s| s.parse().ok()) {
            config.admin_limit = v;
        }
        if let Some(v) = std::env::var("RATE_LIMIT_MANAGER").ok().and_then(|s| s.parse().ok()) {
            config.manager_limit = v;
        }
        if let Some(v) = std::env::var("RATE_LIMIT_USER").ok().and_then(|s| s.parse().ok()) {
            config.user_limit = v;
        }
        if let Some(v) = std::env::var("RATE_LIMIT_VIEWER").ok().and_then(|s| s.parse().ok()) {
            config.viewer_limit = v;
        }
        if let Some(v) = std::env::var("RATE_LIMIT_GUEST").ok().and_then(|s| s.parse().ok()) {
            config.guest_limit = v;
        }
        if let Some(v) = std::env::var("RATE_LIMIT_LOGIN").ok().and_then(|s| s.parse().ok()) {
            config.login_limit = v;
        }
        if let Some(v) = std::env::var("RATE_LIMIT_REGISTER").ok().and_then(|s| s.parse().ok()) {
            config.register_limit = v;
        }

        config
    }

    /// Get the rate limit (requests per minute) for a given role.
    pub fn limit_for_role(&self, role: &UserRole) -> u32 {
        match role {
            UserRole::Admin => self.admin_limit,
            UserRole::Manager => self.manager_limit,
            UserRole::User => self.user_limit,
            UserRole::Viewer => self.viewer_limit,
            UserRole::Guest => self.guest_limit,
        }
    }

    /// Check if a path is exempt from rate limiting.
    ///
    /// Matches the path exactly against the configured exempt paths.
    pub fn is_exempt(&self, path: &str) -> bool {
        self.exempt_paths.iter().any(|p| path == p)
    }

    /// Get the per-endpoint override limit for a path, if any.
    ///
    /// Returns `Some(limit)` for paths with endpoint-specific overrides:
    /// - `/api/auth/login`    → `login_limit`
    /// - `/api/auth/register` → `register_limit`
    /// Returns `None` for all other paths.
    fn endpoint_limit(&self, path: &str) -> Option<u32> {
        if path.starts_with("/api/auth/login") {
            Some(self.login_limit)
        } else if path.starts_with("/api/auth/register") {
            Some(self.register_limit)
        } else {
            None
        }
    }
}

// ── Token Bucket ───────────────────────────────────────────────────────────

/// Internal token-bucket state for a single key (user ID or IP).
struct BucketState {
    /// Current number of tokens (fractional to allow smooth refill).
    tokens: f64,
    /// Last time tokens were refilled.
    last_refill: Instant,
}

/// Decision returned by the rate limiter for a single request.
#[derive(Debug)]
struct RateLimitDecision {
    /// Whether the request is allowed.
    allowed: bool,
    /// Steady-state limit (req/min).
    limit: u32,
    /// Tokens remaining in the bucket after this check.
    remaining: u32,
    /// Unix timestamp when the bucket refills to full capacity.
    reset: u64,
    /// Seconds until the next token is available (only meaningful if `!allowed`).
    retry_after: u64,
}

/// Token-bucket rate limiter with per-key state.
///
/// Each key (user UUID string or client IP) gets an independent bucket.
/// The bucket starts full at `2 × limit_per_min` (burst capacity) and
/// refills at `limit_per_min / 60` tokens per second.
///
/// State is stored in a `HashMap<String, BucketState>` guarded by
/// `tokio::sync::Mutex`.
pub struct RateLimiter {
    /// Configuration (limits, exempt paths).
    config: RateLimitConfig,
    /// Per-key token-bucket state.
    buckets: Arc<Mutex<HashMap<String, BucketState>>>,
}

impl RateLimiter {
    /// Create a new rate limiter with the given configuration.
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            buckets: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get a reference to the configuration.
    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }

    /// Get the configured limit (req/min) for a role.
    pub fn limit_for_role(&self, role: &UserRole) -> u32 {
        self.config.limit_for_role(role)
    }

    /// Check if a request identified by `key` is allowed under
    /// `limit_per_min` requests per minute.
    ///
    /// Uses a token-bucket algorithm:
    /// - Capacity = `2 × limit_per_min` (burst allowance)
    /// - Refill rate = `limit_per_min / 60` tokens per second
    ///
    /// Returns a `RateLimitDecision` indicating whether the request is
    /// allowed and how many tokens remain.
    pub async fn check(&self, key: &str, limit_per_min: u32) -> RateLimitDecision {
        let limit = limit_per_min.max(1);
        let capacity = (limit * 2) as f64; // burst = 2× steady-state
        let refill_rate = limit as f64 / 60.0; // tokens per second

        let mut buckets = self.buckets.lock().await;
        let now = Instant::now();

        let bucket = buckets.entry(key.to_string()).or_insert(BucketState {
            tokens: capacity, // start with a full bucket
            last_refill: now,
        });

        // Refill tokens based on elapsed time since last check
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * refill_rate).min(capacity);
        bucket.last_refill = now;

        if bucket.tokens >= 1.0 {
            // Consume one token
            bucket.tokens -= 1.0;
            let remaining = bucket.tokens.floor() as u32;
            let time_to_full = if refill_rate > 0.0 {
                (capacity - bucket.tokens) / refill_rate
            } else {
                0.0
            };
            let reset = now_unix() + time_to_full.ceil() as u64;

            RateLimitDecision {
                allowed: true,
                limit: limit_per_min,
                remaining,
                reset,
                retry_after: 0,
            }
        } else {
            // No tokens available — deny
            let retry_after = if refill_rate > 0.0 {
                ((1.0 - bucket.tokens) / refill_rate).ceil() as u64
            } else {
                60
            };
            let time_to_full = if refill_rate > 0.0 {
                (capacity - bucket.tokens) / refill_rate
            } else {
                60.0
            };
            let reset = now_unix() + time_to_full.ceil() as u64;

            RateLimitDecision {
                allowed: false,
                limit: limit_per_min,
                remaining: 0,
                reset,
                retry_after: retry_after.max(1),
            }
        }
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new(RateLimitConfig::default())
    }
}

// ── Global Singleton ───────────────────────────────────────────────────────

/// Global rate limiter instance, initialized once at startup.
static RATE_LIMITER: OnceLock<Arc<RateLimiter>> = OnceLock::new();

/// Initialize the global rate limiter with the given configuration.
///
/// Should be called once during server startup, before any requests are served.
/// Subsequent calls are no-ops (the first configuration wins).
pub fn init_rate_limiter(config: RateLimitConfig) {
    let _ = RATE_LIMITER.set(Arc::new(RateLimiter::new(config)));
}

/// Get the global rate limiter, or a default instance if not yet initialized.
fn get_rate_limiter() -> Arc<RateLimiter> {
    RATE_LIMITER
        .get()
        .cloned()
        .unwrap_or_else(|| Arc::new(RateLimiter::default()))
}

// ── IP Extraction ──────────────────────────────────────────────────────────

/// Extract the client IP address from the request.
///
/// Checks (in order):
/// 1. `X-Forwarded-For` header (first IP in the comma-separated list)
/// 2. `X-Real-IP` header
/// 3. `SocketAddr` in request extensions (set by axum's `ConnectInfo`)
/// 4. Falls back to `"unknown"`
fn extract_client_ip(req: &Request) -> String {
    // 1. X-Forwarded-For (behind a reverse proxy)
    if let Some(forwarded) = req.headers().get("x-forwarded-for") {
        if let Ok(value) = forwarded.to_str() {
            if let Some(first_ip) = value.split(',').next() {
                let ip = first_ip.trim();
                if !ip.is_empty() {
                    return ip.to_string();
                }
            }
        }
    }

    // 2. X-Real-IP
    if let Some(real_ip) = req.headers().get("x-real-ip") {
        if let Ok(value) = real_ip.to_str() {
            let ip = value.trim();
            if !ip.is_empty() {
                return ip.to_string();
            }
        }
    }

    // 3. SocketAddr from extensions (set by axum's ConnectInfo layer)
    if let Some(addr) = req.extensions().get::<std::net::SocketAddr>() {
        return addr.ip().to_string();
    }

    // 4. Fallback
    "unknown".to_string()
}

// ── Key & Limit Determination ──────────────────────────────────────────────

/// Determine the rate-limit key and per-minute limit for a request.
///
/// - **Per-endpoint overrides** (login/register): keyed by `endpoint:{path}:{ip}`,
///   uses the endpoint-specific limit. These are unauthenticated routes, so
///   the IP address is used as the key.
/// - **Authenticated**: keyed by `user:{uuid}`, uses the role-specific limit.
/// - **Unauthenticated**: keyed by `ip:{addr}`, uses the Guest limit.
fn determine_key_and_limit(req: &Request, config: &RateLimitConfig) -> (String, u32) {
    let path = req.uri().path();

    // Per-endpoint overrides for unauthenticated auth routes
    if let Some(endpoint_limit) = config.endpoint_limit(path) {
        let ip = extract_client_ip(req);
        return (format!("endpoint:{}:{}", path, ip), endpoint_limit);
    }

    // Authenticated: keyed by user ID, limited by role
    if let Some(auth_user) = req.extensions().get::<AuthUser>() {
        let limit = config.limit_for_role(&auth_user.role);
        return (format!("user:{}", auth_user.user_id), limit);
    }

    // Unauthenticated: keyed by IP, Guest limit
    let ip = extract_client_ip(req);
    (format!("ip:{}", ip), config.guest_limit)
}

// ── Rate Limit Check ───────────────────────────────────────────────────────

/// Core rate-limit check logic, extracted for testability.
///
/// Returns a `RateLimitDecision` indicating whether the request is allowed
/// and the rate limit metadata for headers.
///
/// This function does **not** consume the request — it only reads from it.
/// The caller is responsible for forwarding the request to `next.run(req)`.
async fn check_rate_limit(req: &Request, limiter: &RateLimiter) -> RateLimitDecision {
    let config = limiter.config();
    let path = req.uri().path();

    // Exempt paths bypass rate limiting entirely
    if config.is_exempt(path) {
        debug!("Rate limit bypassed for exempt path: {}", path);
        return RateLimitDecision {
            allowed: true,
            limit: 0,
            remaining: 0,
            reset: 0,
            retry_after: 0,
        };
    }

    let (key, limit_per_min) = determine_key_and_limit(req, config);
    limiter.check(&key, limit_per_min).await
}

// ── Response Builders ──────────────────────────────────────────────────────

/// Add `X-RateLimit-*` headers to a response.
///
/// Sets:
/// - `X-RateLimit-Limit`     — steady-state limit (req/min)
/// - `X-RateLimit-Remaining` — tokens remaining
/// - `X-RateLimit-Reset`     — Unix timestamp when bucket refills to capacity
fn add_rate_limit_headers(response: &mut Response, decision: &RateLimitDecision) {
    response.headers_mut().insert(
        "x-ratelimit-limit",
        HeaderValue::from_str(&decision.limit.to_string())
            .unwrap_or(HeaderValue::from_static("0")),
    );
    response.headers_mut().insert(
        "x-ratelimit-remaining",
        HeaderValue::from_str(&decision.remaining.to_string())
            .unwrap_or(HeaderValue::from_static("0")),
    );
    response.headers_mut().insert(
        "x-ratelimit-reset",
        HeaderValue::from_str(&decision.reset.to_string())
            .unwrap_or(HeaderValue::from_static("0")),
    );
}

/// Build a 429 Too Many Requests response with rate limit information.
///
/// Sets the `Retry-After` header and returns a JSON body:
/// `{"success":false,"error":"Rate limit exceeded","retry_after":N}`
fn build_rate_limited_response(decision: &RateLimitDecision) -> Response {
    let body = serde_json::json!({
        "success": false,
        "error": "Rate limit exceeded",
        "retry_after": decision.retry_after,
    });

    let mut response = Json(body).into_response();
    *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;

    // Rate limit headers
    add_rate_limit_headers(&mut response, decision);

    // Retry-After header
    response.headers_mut().insert(
        header::RETRY_AFTER,
        HeaderValue::from_str(&decision.retry_after.to_string())
            .unwrap_or(HeaderValue::from_static("1")),
    );

    response
}

// ── Rate Limit Middleware ──────────────────────────────────────────────────

/// Rate limiting middleware.
///
/// Extracts the user role from request extensions (set by `auth_middleware`)
/// and applies per-role token-bucket limits. Unauthenticated requests to
/// non-auth endpoints are limited by IP address under the Guest quota.
/// Auth endpoints (`/api/auth/login`, `/api/auth/register`) have their own
/// stricter per-endpoint limits, also keyed by IP. Exempt paths bypass
/// the limiter entirely.
///
/// On rate limit exceeded, returns HTTP 429 with a `Retry-After` header
/// and a JSON body: `{"success":false,"error":"Rate limit exceeded","retry_after":N}`.
///
/// All non-exempt responses include `X-RateLimit-Limit`,
/// `X-RateLimit-Remaining`, and `X-RateLimit-Reset` headers.
///
/// # Layer ordering
///
/// This middleware should run **after** the auth middleware (so it can read
/// the `AuthUser` extension). In axum, outer layers execute first:
///
/// ```ignore
/// // Layers listed last execute first (outermost):
/// .layer(middleware::from_fn(rate_limit_middleware)) // innermost
/// .layer(auth_layer)                                  // runs before rate_limit
/// .layer(middleware::from_fn(request_id_middleware))  // runs before auth
/// ```
pub async fn rate_limit_middleware(req: Request, next: Next) -> Response {
    let limiter = get_rate_limiter();
    let decision = check_rate_limit(&req, &limiter).await;

    if !decision.allowed {
        warn!(
            "Rate limit exceeded — path={}, retry_after={}s, limit={}/min",
            req.uri().path(),
            decision.retry_after,
            decision.limit
        );
        return build_rate_limited_response(&decision);
    }

    if decision.limit > 0 {
        debug!(
            "Rate limit OK — path={}, limit={}/min, remaining={}",
            req.uri().path(),
            decision.limit,
            decision.remaining
        );

        let mut response = next.run(req).await;
        add_rate_limit_headers(&mut response, &decision);
        response
    } else {
        // Exempt path (limit == 0) — no rate limit headers
        next.run(req).await
    }
}

// ── CSRF Protection ────────────────────────────────────────────────────────

/// Extract a cookie value by name from the `Cookie` header.
///
/// Parses the `Cookie` header (semicolon-separated `name=value` pairs)
/// and returns the value for the first cookie matching `name`
/// (case-insensitive). Returns `None` if the cookie is not found or
/// the `Cookie` header is absent.
fn extract_cookie_value(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;

    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some((k, v)) = cookie.split_once('=') {
            if k.trim().eq_ignore_ascii_case(name) {
                return Some(v.trim().to_string());
            }
        }
    }

    None
}

/// Core CSRF verification logic, extracted for testability.
///
/// Implements the double-submit cookie pattern:
/// 1. Read the `XSRF-TOKEN` cookie value.
/// 2. Read the `X-XSRF-TOKEN` header value.
/// 3. If both are present and equal, the request passes (`None`).
/// 4. Otherwise, return a 403 response.
///
/// Skips verification for:
/// - GET and OPTIONS methods (safe, read-only)
/// - Paths starting with `/api/auth/` (login/register don't have a CSRF token yet)
///
/// Returns `Some(403 response)` if CSRF verification fails,
/// or `None` if the request should be allowed through.
fn check_csrf(req: &Request) -> Option<Response> {
    let method = req.method();
    let path = req.uri().path();

    // Skip safe methods (GET, OPTIONS)
    if method == &Method::GET || method == &Method::OPTIONS {
        return None;
    }

    // Skip /api/auth/* paths (login/register don't have CSRF token yet)
    if path.starts_with("/api/auth/") {
        return None;
    }

    // Double-submit cookie pattern: compare cookie with header
    let cookie_token = extract_cookie_value(req.headers(), "XSRF-TOKEN");
    let header_token = req
        .headers()
        .get("x-xsrf-token")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string());

    match (cookie_token, header_token) {
        (Some(cookie), Some(header)) if cookie == header => {
            // Tokens match — request is allowed
            None
        }
        _ => {
            // Missing or mismatched tokens — deny
            warn!(
                "CSRF validation failed — path={}, method={}, cookie_present={}, header_present={}",
                path,
                method,
                req.headers().get(header::COOKIE).is_some(),
                req.headers().get("x-xsrf-token").is_some()
            );
            Some(build_csrf_failure_response())
        }
    }
}

/// Build a 403 Forbidden response for CSRF validation failure.
///
/// Returns a JSON body: `{"success":false,"error":"CSRF token validation failed"}`
fn build_csrf_failure_response() -> Response {
    let body = serde_json::json!({
        "success": false,
        "error": "CSRF token validation failed",
    });
    (StatusCode::FORBIDDEN, Json(body)).into_response()
}

/// CSRF verification middleware for state-changing requests.
///
/// Implements the double-submit cookie pattern:
/// - Reads the `XSRF-TOKEN` cookie and the `X-XSRF-TOKEN` header.
/// - If both are present and match, the request passes through.
/// - If either is missing or they don't match, returns HTTP 403.
///
/// Skips verification for:
/// - GET and OPTIONS methods (safe, read-only)
/// - Paths starting with `/api/auth/` (login/register don't have a CSRF token yet)
///
/// # Layer ordering
///
/// This middleware should run **after** the auth middleware and **before**
/// the route handler. In axum:
///
/// ```ignore
/// .layer(middleware::from_fn(csrf_middleware)) // innermost
/// .layer(middleware::from_fn(rate_limit_middleware))
/// .layer(auth_layer)
/// ```
pub async fn csrf_middleware(req: Request, next: Next) -> Response {
    if let Some(response) = check_csrf(&req) {
        return response;
    }
    next.run(req).await
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Current Unix timestamp in seconds.
fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use uuid::Uuid;

    // ═══════════════════════════════════════════════════════════════════════
    //  Config tests
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_default_config_limits() {
        let config = RateLimitConfig::default();
        assert_eq!(config.admin_limit, 1000);
        assert_eq!(config.manager_limit, 500);
        assert_eq!(config.user_limit, 100);
        assert_eq!(config.viewer_limit, 50);
        assert_eq!(config.guest_limit, 20);
        assert_eq!(config.login_limit, 10);
        assert_eq!(config.register_limit, 5);
    }

    #[test]
    fn test_config_limit_for_each_role() {
        let config = RateLimitConfig::default();
        assert_eq!(config.limit_for_role(&UserRole::Admin), 1000);
        assert_eq!(config.limit_for_role(&UserRole::Manager), 500);
        assert_eq!(config.limit_for_role(&UserRole::User), 100);
        assert_eq!(config.limit_for_role(&UserRole::Viewer), 50);
        assert_eq!(config.limit_for_role(&UserRole::Guest), 20);
    }

    #[test]
    fn test_config_exempt_paths() {
        let config = RateLimitConfig::default();

        // Health endpoints should be exempt
        assert!(config.is_exempt("/health"));
        assert!(config.is_exempt("/ready"));
        assert!(config.is_exempt("/metrics"));

        // Auth endpoints should NOT be exempt (they have per-endpoint limits)
        assert!(!config.is_exempt("/api/auth/login"));
        assert!(!config.is_exempt("/api/auth/register"));
    }

    #[test]
    fn test_config_endpoint_overrides() {
        let config = RateLimitConfig::default();

        assert_eq!(config.endpoint_limit("/api/auth/login"), Some(10));
        assert_eq!(config.endpoint_limit("/api/auth/register"), Some(5));
        assert_eq!(config.endpoint_limit("/api/auth/refresh"), None);
        assert_eq!(config.endpoint_limit("/api/v1/accounts"), None);
        assert_eq!(config.endpoint_limit("/health"), None);
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  Rate limiter: token-bucket tests
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_rate_limiter_burst_capacity_is_2x() {
        // limit = 5/min → burst capacity = 10
        let limiter = RateLimiter::new(RateLimitConfig {
            guest_limit: 5,
            ..Default::default()
        });

        // First 10 requests (burst) should be allowed
        for i in 0..10 {
            let d = limiter.check("test-key", 5).await;
            assert!(
                d.allowed,
                "Request {} should be allowed (burst capacity = 10)",
                i + 1
            );
        }

        // 11th request should be denied
        let d = limiter.check("test-key", 5).await;
        assert!(!d.allowed, "11th request should be rate limited");
        assert_eq!(d.limit, 5);
    }

    #[tokio::test]
    async fn test_rate_limiter_independent_keys() {
        // Different keys under the same limit have independent buckets
        let limiter = RateLimiter::new(RateLimitConfig {
            user_limit: 3,
            ..Default::default()
        });

        // Exhaust key A (burst = 6)
        for _ in 0..6 {
            assert!(limiter.check("key-a", 3).await.allowed);
        }
        assert!(!limiter.check("key-a", 3).await.allowed);

        // Key B should still be allowed
        assert!(
            limiter.check("key-b", 3).await.allowed,
            "Different keys should have independent buckets"
        );
    }

    #[tokio::test]
    async fn test_rate_limiter_returns_retry_after() {
        let limiter = RateLimiter::new(RateLimitConfig {
            guest_limit: 1,
            ..Default::default()
        });
        let key = "test-retry";

        // Burst capacity = 2, so first 2 are allowed
        limiter.check(key, 1).await;
        limiter.check(key, 1).await;

        // Third request is denied
        let d = limiter.check(key, 1).await;
        assert!(!d.allowed);
        assert!(d.retry_after >= 1, "retry_after should be at least 1 second");
    }

    #[tokio::test]
    async fn test_rate_limiter_remaining_decrements() {
        let limiter = RateLimiter::new(RateLimitConfig {
            guest_limit: 10,
            ..Default::default()
        });
        let key = "test-remaining";

        // Burst capacity = 20, first request leaves 19
        let d1 = limiter.check(key, 10).await;
        assert!(d1.allowed);
        assert_eq!(d1.remaining, 19);

        let d2 = limiter.check(key, 10).await;
        assert!(d2.allowed);
        assert_eq!(d2.remaining, 18);
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  REQUIRED TEST 1: Rate limit exceeded returns 429
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_rate_limit_exceeded_returns_429() {
        let limiter = RateLimiter::new(RateLimitConfig {
            guest_limit: 1, // burst = 2
            ..Default::default()
        });

        let make_req = || {
            Request::builder()
                .uri("/api/v1/accounts")
                .header("x-forwarded-for", "1.2.3.4")
                .body(Body::empty())
                .unwrap()
        };

        // First 2 requests (burst capacity) are allowed
        assert!(check_rate_limit(&make_req(), &limiter).await.allowed);
        assert!(check_rate_limit(&make_req(), &limiter).await.allowed);

        // Third request is denied
        let decision = check_rate_limit(&make_req(), &limiter).await;
        assert!(!decision.allowed, "Third request should be rate limited");

        // Build 429 response and verify
        let response = build_rate_limited_response(&decision);
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

        // Verify headers
        assert!(response.headers().get("x-ratelimit-limit").is_some());
        assert!(response.headers().get("x-ratelimit-remaining").is_some());
        assert!(response.headers().get("x-ratelimit-reset").is_some());
        assert!(response.headers().get(header::RETRY_AFTER).is_some());
    }

    #[tokio::test]
    async fn test_rate_limit_429_body_format() {
        let limiter = RateLimiter::new(RateLimitConfig {
            guest_limit: 1,
            ..Default::default()
        });

        let make_req = || {
            Request::builder()
                .uri("/api/v1/accounts")
                .header("x-forwarded-for", "5.5.5.5")
                .body(Body::empty())
                .unwrap()
        };

        // Exhaust burst
        check_rate_limit(&make_req(), &limiter).await;
        check_rate_limit(&make_req(), &limiter).await;

        // Get denied decision
        let decision = check_rate_limit(&make_req(), &limiter).await;
        assert!(!decision.allowed);

        let response = build_rate_limited_response(&decision);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["success"], false);
        assert_eq!(json["error"], "Rate limit exceeded");
        assert!(json["retry_after"].as_u64().unwrap() >= 1);
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  REQUIRED TEST 2: Different roles get different limits
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_different_roles_get_different_limits() {
        let limiter = RateLimiter::new(RateLimitConfig::default());
        // admin_limit = 1000 (burst 2000), viewer_limit = 50 (burst 100)

        // ── Admin: 101 requests should all be allowed ──
        let admin_id = Uuid::new_v4();
        let make_admin_req = || {
            let mut req = Request::builder()
                .uri("/api/v1/accounts")
                .body(Body::empty())
                .unwrap();
            req.extensions_mut().insert(AuthUser {
                user_id: admin_id,
                role: UserRole::Admin,
            });
            req
        };

        for _ in 0..101 {
            let d = check_rate_limit(&make_admin_req(), &limiter).await;
            assert!(
                d.allowed,
                "Admin should be allowed up to burst capacity 2000"
            );
        }

        // ── Viewer: 100 requests allowed (burst 100), 101st denied ──
        let viewer_id = Uuid::new_v4();
        let make_viewer_req = || {
            let mut req = Request::builder()
                .uri("/api/v1/accounts")
                .body(Body::empty())
                .unwrap();
            req.extensions_mut().insert(AuthUser {
                user_id: viewer_id,
                role: UserRole::Viewer,
            });
            req
        };

        for _ in 0..100 {
            let d = check_rate_limit(&make_viewer_req(), &limiter).await;
            assert!(
                d.allowed,
                "Viewer should be allowed up to burst capacity 100"
            );
        }

        let d = check_rate_limit(&make_viewer_req(), &limiter).await;
        assert!(!d.allowed, "Viewer's 101st request should be rate limited");
        assert_eq!(d.limit, 50, "Viewer limit should be 50/min");
    }

    #[tokio::test]
    async fn test_admin_limit_is_higher_than_viewer() {
        // Verify the limits are different by exhausting viewer but not admin
        let limiter = RateLimiter::new(RateLimitConfig::default());

        let viewer_id = Uuid::new_v4();
        let admin_id = Uuid::new_v4();

        // Exhaust viewer (burst = 100)
        for _ in 0..100 {
            let mut req = Request::builder()
                .uri("/api/v1/accounts")
                .body(Body::empty())
                .unwrap();
            req.extensions_mut().insert(AuthUser {
                user_id: viewer_id,
                role: UserRole::Viewer,
            });
            assert!(check_rate_limit(&req, &limiter).await.allowed);
        }

        // Viewer is now exhausted
        let mut req = Request::builder()
            .uri("/api/v1/accounts")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(AuthUser {
            user_id: viewer_id,
            role: UserRole::Viewer,
        });
        assert!(!check_rate_limit(&req, &limiter).await.allowed);

        // Admin should still have plenty of quota (burst = 2000)
        let mut req = Request::builder()
            .uri("/api/v1/accounts")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(AuthUser {
            user_id: admin_id,
            role: UserRole::Admin,
        });
        assert!(
            check_rate_limit(&req, &limiter).await.allowed,
            "Admin should have separate, higher limit"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  REQUIRED TEST 3: Login endpoint has stricter limit
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_login_endpoint_has_stricter_limit() {
        let limiter = RateLimiter::new(RateLimitConfig::default());
        // login_limit = 10 (burst = 20)

        let make_login_req = || {
            Request::builder()
                .uri("/api/auth/login")
                .header("x-forwarded-for", "1.2.3.4")
                .body(Body::empty())
                .unwrap()
        };

        // First 20 requests (burst capacity) are allowed
        for _ in 0..20 {
            let d = check_rate_limit(&make_login_req(), &limiter).await;
            assert!(d.allowed, "Login requests within burst should be allowed");
            assert_eq!(d.limit, 10, "Login endpoint limit should be 10/min");
        }

        // 21st request is denied
        let d = check_rate_limit(&make_login_req(), &limiter).await;
        assert!(!d.allowed, "21st login request should be rate limited");
        assert_eq!(d.limit, 10);
    }

    #[tokio::test]
    async fn test_register_endpoint_has_even_stricter_limit() {
        let limiter = RateLimiter::new(RateLimitConfig::default());
        // register_limit = 5 (burst = 10)

        let make_register_req = || {
            Request::builder()
                .uri("/api/auth/register")
                .header("x-forwarded-for", "1.2.3.4")
                .body(Body::empty())
                .unwrap()
        };

        // First 10 requests (burst capacity) are allowed
        for _ in 0..10 {
            let d = check_rate_limit(&make_register_req(), &limiter).await;
            assert!(d.allowed, "Register requests within burst should be allowed");
            assert_eq!(d.limit, 5, "Register endpoint limit should be 5/min");
        }

        // 11th request is denied
        let d = check_rate_limit(&make_register_req(), &limiter).await;
        assert!(!d.allowed, "11th register request should be rate limited");
    }

    #[tokio::test]
    async fn test_login_and_register_have_independent_buckets() {
        let limiter = RateLimiter::new(RateLimitConfig::default());

        // Exhaust login bucket (burst = 20)
        for _ in 0..20 {
            let req = Request::builder()
                .uri("/api/auth/login")
                .header("x-forwarded-for", "1.2.3.4")
                .body(Body::empty())
                .unwrap();
            assert!(check_rate_limit(&req, &limiter).await.allowed);
        }

        // Login is now exhausted
        let req = Request::builder()
            .uri("/api/auth/login")
            .header("x-forwarded-for", "1.2.3.4")
            .body(Body::empty())
            .unwrap();
        assert!(!check_rate_limit(&req, &limiter).await.allowed);

        // Register should still be allowed (different endpoint key)
        let req = Request::builder()
            .uri("/api/auth/register")
            .header("x-forwarded-for", "1.2.3.4")
            .body(Body::empty())
            .unwrap();
        assert!(
            check_rate_limit(&req, &limiter).await.allowed,
            "Register endpoint should have independent bucket from login"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  REQUIRED TEST 4: CSRF missing token returns 403
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_csrf_missing_token_returns_403() {
        // POST with no cookie and no header
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/accounts")
            .body(Body::empty())
            .unwrap();

        let result = check_csrf(&req);
        assert!(result.is_some(), "Missing CSRF token should return 403");

        let response = result.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn test_csrf_missing_cookie_returns_403() {
        // Header present but no cookie
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/accounts")
            .header("x-xsrf-token", "abc123")
            .body(Body::empty())
            .unwrap();

        let result = check_csrf(&req);
        assert!(result.is_some(), "Missing CSRF cookie should return 403");
        assert_eq!(result.unwrap().status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn test_csrf_missing_header_returns_403() {
        // Cookie present but no header
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/accounts")
            .header("cookie", "XSRF-TOKEN=abc123")
            .body(Body::empty())
            .unwrap();

        let result = check_csrf(&req);
        assert!(result.is_some(), "Missing CSRF header should return 403");
        assert_eq!(result.unwrap().status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn test_csrf_mismatched_tokens_return_403() {
        // Both present but different values
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/accounts")
            .header("cookie", "XSRF-TOKEN=abc123")
            .header("x-xsrf-token", "xyz789")
            .body(Body::empty())
            .unwrap();

        let result = check_csrf(&req);
        assert!(result.is_some(), "Mismatched CSRF tokens should return 403");
        assert_eq!(result.unwrap().status(), StatusCode::FORBIDDEN);
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  REQUIRED TEST 5: CSRF matching tokens pass through
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_csrf_matching_tokens_pass_through() {
        // Both cookie and header present with matching values
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/accounts")
            .header("cookie", "XSRF-TOKEN=abc123")
            .header("x-xsrf-token", "abc123")
            .body(Body::empty())
            .unwrap();

        let result = check_csrf(&req);
        assert!(
            result.is_none(),
            "Matching CSRF tokens should pass through (return None)"
        );
    }

    #[test]
    fn test_csrf_matching_tokens_with_other_cookies_pass_through() {
        // Cookie header has multiple cookies, XSRF-TOKEN is one of them
        let req = Request::builder()
            .method("PUT")
            .uri("/api/v1/accounts/123")
            .header("cookie", "session=xyz; XSRF-TOKEN=secret-token; theme=dark")
            .header("x-xsrf-token", "secret-token")
            .body(Body::empty())
            .unwrap();

        let result = check_csrf(&req);
        assert!(
            result.is_none(),
            "Matching CSRF tokens with other cookies should pass through"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  CSRF: skip conditions
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_csrf_skips_get_method() {
        // GET request without CSRF tokens should pass through
        let req = Request::builder()
            .method("GET")
            .uri("/api/v1/accounts")
            .body(Body::empty())
            .unwrap();

        let result = check_csrf(&req);
        assert!(
            result.is_none(),
            "GET requests should skip CSRF verification"
        );
    }

    #[test]
    fn test_csrf_skips_options_method() {
        let req = Request::builder()
            .method("OPTIONS")
            .uri("/api/v1/accounts")
            .body(Body::empty())
            .unwrap();

        let result = check_csrf(&req);
        assert!(
            result.is_none(),
            "OPTIONS requests should skip CSRF verification"
        );
    }

    #[test]
    fn test_csrf_skips_auth_paths() {
        // /api/auth/* paths should skip CSRF (login/register don't have token yet)
        let req = Request::builder()
            .method("POST")
            .uri("/api/auth/login")
            .body(Body::empty())
            .unwrap();

        let result = check_csrf(&req);
        assert!(
            result.is_none(),
            "/api/auth/* paths should skip CSRF verification"
        );
    }

    #[test]
    fn test_csrf_applies_to_put_method() {
        // PUT is a state-changing method, should require CSRF
        let req = Request::builder()
            .method("PUT")
            .uri("/api/v1/accounts/123")
            .body(Body::empty())
            .unwrap();

        let result = check_csrf(&req);
        assert!(result.is_some(), "PUT requests should require CSRF token");
        assert_eq!(result.unwrap().status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn test_csrf_applies_to_delete_method() {
        // DELETE is a state-changing method, should require CSRF
        let req = Request::builder()
            .method("DELETE")
            .uri("/api/v1/accounts/123")
            .body(Body::empty())
            .unwrap();

        let result = check_csrf(&req);
        assert!(result.is_some(), "DELETE requests should require CSRF token");
        assert_eq!(result.unwrap().status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn test_csrf_failure_response_status() {
        let response = build_csrf_failure_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_csrf_failure_response_body_content() {
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/accounts")
            .body(Body::empty())
            .unwrap();

        let response = check_csrf(&req).unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["success"], false);
        assert_eq!(json["error"], "CSRF token validation failed");
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  IP extraction tests
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_extract_ip_x_forwarded_for_single() {
        let req = Request::builder()
            .header("x-forwarded-for", "1.2.3.4")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_client_ip(&req), "1.2.3.4");
    }

    #[test]
    fn test_extract_ip_x_forwarded_for_multiple() {
        let req = Request::builder()
            .header("x-forwarded-for", "1.2.3.4, 5.6.7.8, 9.10.11.12")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_client_ip(&req), "1.2.3.4");
    }

    #[test]
    fn test_extract_ip_x_real_ip() {
        let req = Request::builder()
            .header("x-real-ip", "9.8.7.6")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_client_ip(&req), "9.8.7.6");
    }

    #[test]
    fn test_extract_ip_x_forwarded_for_takes_precedence() {
        let req = Request::builder()
            .header("x-forwarded-for", "1.2.3.4")
            .header("x-real-ip", "5.6.7.8")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_client_ip(&req), "1.2.3.4");
    }

    #[test]
    fn test_extract_ip_fallback_unknown() {
        let req = Request::builder()
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_client_ip(&req), "unknown");
    }

    #[test]
    fn test_extract_ip_from_socket_addr() {
        let mut req = Request::builder()
            .body(Body::empty())
            .unwrap();
        let addr: std::net::SocketAddr = "192.168.1.100:12345".parse().unwrap();
        req.extensions_mut().insert(addr);
        assert_eq!(extract_client_ip(&req), "192.168.1.100");
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  Cookie extraction tests
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_extract_cookie_single() {
        let req = Request::builder()
            .header("cookie", "XSRF-TOKEN=abc123")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            extract_cookie_value(req.headers(), "XSRF-TOKEN"),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn test_extract_cookie_multiple() {
        let req = Request::builder()
            .header("cookie", "session=xyz; XSRF-TOKEN=abc123; theme=dark")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            extract_cookie_value(req.headers(), "XSRF-TOKEN"),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn test_extract_cookie_case_insensitive() {
        let req = Request::builder()
            .header("cookie", "xsrf-token=abc123")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            extract_cookie_value(req.headers(), "XSRF-TOKEN"),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn test_extract_cookie_not_found() {
        let req = Request::builder()
            .header("cookie", "session=xyz")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            extract_cookie_value(req.headers(), "XSRF-TOKEN"),
            None
        );
    }

    #[test]
    fn test_extract_cookie_no_cookie_header() {
        let req = Request::builder()
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            extract_cookie_value(req.headers(), "XSRF-TOKEN"),
            None
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  check_rate_limit: exempt paths
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_check_rate_limit_exempt_path_bypasses() {
        let limiter = RateLimiter::new(RateLimitConfig {
            guest_limit: 1,
            ..Default::default()
        });

        // Even with a tiny limit, exempt paths should bypass
        for _ in 0..10 {
            let req = Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap();
            let d = check_rate_limit(&req, &limiter).await;
            assert!(d.allowed, "Exempt path /health should bypass rate limiting");
            assert_eq!(d.limit, 0, "Exempt path should have limit=0");
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  check_rate_limit: unauthenticated (IP-keyed)
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_check_rate_limit_unauthenticated_uses_ip_as_key() {
        let limiter = RateLimiter::new(RateLimitConfig {
            guest_limit: 1, // burst = 2
            ..Default::default()
        });

        let make_req = || {
            Request::builder()
                .uri("/api/v1/accounts")
                .header("x-forwarded-for", "1.2.3.4")
                .body(Body::empty())
                .unwrap()
        };

        // First 2 requests (burst) are allowed
        assert!(check_rate_limit(&make_req(), &limiter).await.allowed);
        assert!(check_rate_limit(&make_req(), &limiter).await.allowed);

        // Third is denied
        let d = check_rate_limit(&make_req(), &limiter).await;
        assert!(!d.allowed);
        assert_eq!(d.limit, 20); // guest_limit from default config
    }

    #[tokio::test]
    async fn test_check_rate_limit_different_ips_independent() {
        let limiter = RateLimiter::new(RateLimitConfig {
            guest_limit: 1, // burst = 2
            ..Default::default()
        });

        // IP A exhausts its bucket
        let req_a = Request::builder()
            .uri("/api/v1/accounts")
            .header("x-forwarded-for", "1.1.1.1")
            .body(Body::empty())
            .unwrap();
        assert!(check_rate_limit(&req_a, &limiter).await.allowed);
        assert!(check_rate_limit(&req_a, &limiter).await.allowed);
        assert!(!check_rate_limit(&req_a, &limiter).await.allowed);

        // IP B should still be allowed
        let req_b = Request::builder()
            .uri("/api/v1/accounts")
            .header("x-forwarded-for", "2.2.2.2")
            .body(Body::empty())
            .unwrap();
        assert!(
            check_rate_limit(&req_b, &limiter).await.allowed,
            "Different IP should have independent bucket"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  check_rate_limit: authenticated (user-ID-keyed)
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_check_rate_limit_authenticated_uses_user_id_as_key() {
        let limiter = RateLimiter::new(RateLimitConfig {
            user_limit: 2, // burst = 4
            ..Default::default()
        });

        let user_id = Uuid::new_v4();
        let make_req = || {
            let mut req = Request::builder()
                .uri("/api/v1/accounts")
                .body(Body::empty())
                .unwrap();
            req.extensions_mut().insert(AuthUser {
                user_id,
                role: UserRole::User,
            });
            req
        };

        // First 4 requests (burst) are allowed
        for _ in 0..4 {
            assert!(check_rate_limit(&make_req(), &limiter).await.allowed);
        }

        // 5th is denied
        let d = check_rate_limit(&make_req(), &limiter).await;
        assert!(!d.allowed);
        assert_eq!(d.limit, 2); // user_limit
    }

    #[tokio::test]
    async fn test_check_rate_limit_different_users_independent() {
        let limiter = RateLimiter::new(RateLimitConfig {
            user_limit: 2, // burst = 4
            ..Default::default()
        });

        let make_req = |id_str: &str| {
            let mut req = Request::builder()
                .uri("/api/v1/accounts")
                .body(Body::empty())
                .unwrap();
            req.extensions_mut().insert(AuthUser {
                user_id: Uuid::parse_str(id_str).unwrap(),
                role: UserRole::User,
            });
            req
        };

        // User A exhausts their bucket
        let req_a = make_req("00000000-0000-0000-0000-00000000000a");
        for _ in 0..4 {
            assert!(check_rate_limit(&req_a, &limiter).await.allowed);
        }
        assert!(!check_rate_limit(&req_a, &limiter).await.allowed);

        // User B should still be allowed
        let req_b = make_req("00000000-0000-0000-0000-00000000000b");
        assert!(
            check_rate_limit(&req_b, &limiter).await.allowed,
            "Different user should have independent bucket"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  Rate limit headers tests
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_rate_limit_headers_on_allowed_request() {
        let limiter = RateLimiter::new(RateLimitConfig {
            guest_limit: 10,
            ..Default::default()
        });

        let req = Request::builder()
            .uri("/api/v1/accounts")
            .header("x-forwarded-for", "1.2.3.4")
            .body(Body::empty())
            .unwrap();

        let d = check_rate_limit(&req, &limiter).await;
        assert!(d.allowed);
        assert_eq!(d.limit, 10);
        assert_eq!(d.remaining, 19); // burst=20, one consumed
        assert!(d.reset > 0, "Reset should be a valid Unix timestamp");
    }

    #[tokio::test]
    async fn test_rate_limit_headers_on_denied_request() {
        let limiter = RateLimiter::new(RateLimitConfig {
            guest_limit: 1, // burst = 2
            ..Default::default()
        });

        let make_req = || {
            Request::builder()
                .uri("/api/v1/accounts")
                .header("x-forwarded-for", "1.2.3.4")
                .body(Body::empty())
                .unwrap()
        };

        // Exhaust burst
        check_rate_limit(&make_req(), &limiter).await;
        check_rate_limit(&make_req(), &limiter).await;

        let d = check_rate_limit(&make_req(), &limiter).await;
        assert!(!d.allowed);
        assert_eq!(d.limit, 1);
        assert_eq!(d.remaining, 0);
        assert!(d.retry_after >= 1);
        assert!(d.reset > 0);

        // Build response and check headers
        let response = build_rate_limited_response(&d);
        assert_eq!(
            response.headers().get("x-ratelimit-limit"),
            Some(&HeaderValue::from_static("1"))
        );
        assert_eq!(
            response.headers().get("x-ratelimit-remaining"),
            Some(&HeaderValue::from_static("0"))
        );
        assert!(response.headers().get("x-ratelimit-reset").is_some());
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  now_unix helper test
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn test_now_unix_is_recent() {
        let ts = now_unix();
        // Should be after 2024-01-01 (1704067200) and before 2100-01-01 (4102444800)
        assert!(ts > 1704067200, "Timestamp should be after 2024-01-01");
        assert!(ts < 4102444800, "Timestamp should be before 2100-01-01");
    }
}
