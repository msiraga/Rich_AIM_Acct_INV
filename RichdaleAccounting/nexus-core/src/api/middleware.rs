//! Rate Limiting Middleware
//!
//! Token-bucket rate limiter using the `governor` crate, with configurable
//! per-role limits and IP-based fallback for unauthenticated requests.
//!
//! # Behavior
//! - Each role gets its own keyed governor instance with an independent quota.
//! - Authenticated requests are keyed by user UUID; unauthenticated requests
//!   are keyed by client IP address (from `X-Forwarded-For`, `X-Real-IP`,
//!   or the connection's socket address).
//! - Exempt paths bypass the limiter entirely.
//! - On rate limit exceeded, returns HTTP 429 with a `Retry-After` header
//!   and a JSON body: `{"error": "rate_limit_exceeded", "retry_after": N,
//!   "limit": N, "remaining": N}`.
//!
//! # Wiring
//!
//! Initialize the global limiter before serving, then apply as an axum layer:
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

use std::num::NonZeroU32;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use axum::{
    extract::Request,
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use governor::{
    clock::DefaultClock,
    state::keyed::DefaultKeyedStateStore,
    Jitter, Quota, RateLimiter as GovernorRateLimiter,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::api::auth::AuthUser;
use crate::database::models::UserRole;

// ── Configuration ──────────────────────────────────────────────────────────

/// Per-role rate limit configuration (requests per minute).
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
            exempt_paths: vec![
                "/health".to_string(),
                "/ready".to_string(),
                "/metrics".to_string(),
                "/api/auth/login".to_string(),
                "/api/auth/register".to_string(),
                "/api/auth/refresh".to_string(),
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
}

// ── Rate Limiter ────────────────────────────────────────────────────────────

/// A keyed governor rate limiter keyed by `String` (user ID or IP address).
///
/// Uses `DefaultClock` and `Jitter::default()` as specified by the
/// `governor` crate's keyed-with-clock constructor.
type KeyedLimiter =
    GovernorRateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>;

/// Token-bucket rate limiter with per-role governor instances.
///
/// Each role gets its own keyed rate limiter, allowing independent quotas.
/// Keys are either the authenticated user's UUID (as a string) or the client
/// IP address for unauthenticated requests.
///
/// # Example
///
/// ```ignore
/// use nexus_core::api::middleware::{RateLimiter, RateLimitConfig};
/// use nexus_core::database::models::UserRole;
///
/// let limiter = RateLimiter::new(RateLimitConfig::default());
///
/// // Check a Guest request from IP 1.2.3.4
/// assert!(limiter.check("1.2.3.4", &UserRole::Guest).is_ok());
/// ```
pub struct RateLimiter {
    /// Configuration (limits, exempt paths).
    config: RateLimitConfig,
    /// Governor instance for Admin role.
    admin: KeyedLimiter,
    /// Governor instance for Manager role.
    manager: KeyedLimiter,
    /// Governor instance for User role.
    user: KeyedLimiter,
    /// Governor instance for Viewer role.
    viewer: KeyedLimiter,
    /// Governor instance for Guest / unauthenticated.
    guest: KeyedLimiter,
}

impl RateLimiter {
    /// Create a new rate limiter with the given configuration.
    ///
    /// Each role gets a separate `governor` keyed rate limiter with its own
    /// `Quota::per_minute` limit. The limiters use `DefaultClock` and
    /// `Jitter::default()` for timing.
    pub fn new(config: RateLimitConfig) -> Self {
        let make_limiter = |limit: u32| -> KeyedLimiter {
            let quota = Quota::per_minute(
                NonZeroU32::new(limit.max(1)).unwrap(),
            );
            GovernorRateLimiter::keyed(quota)
        };

        Self {
            admin: make_limiter(config.admin_limit),
            manager: make_limiter(config.manager_limit),
            user: make_limiter(config.user_limit),
            viewer: make_limiter(config.viewer_limit),
            guest: make_limiter(config.guest_limit),
            config,
        }
    }

    /// Get the governor limiter for a given role.
    fn limiter_for_role(&self, role: &UserRole) -> &KeyedLimiter {
        match role {
            UserRole::Admin => &self.admin,
            UserRole::Manager => &self.manager,
            UserRole::User => &self.user,
            UserRole::Viewer => &self.viewer,
            UserRole::Guest => &self.guest,
        }
    }

    /// Check if a request identified by `key` is allowed under the rate limit
    /// for `role`.
    ///
    /// Returns `Ok(())` if the request is allowed (a token was consumed from
    /// the bucket), or `Err(retry_after)` with the `Duration` until the next
    /// allowed request.
    pub fn check(&self, key: &str, role: &UserRole) -> Result<(), Duration> {
        let limiter = self.limiter_for_role(role);
        limiter.check_key(&key.to_string()).map_err(|_e| Duration::from_secs(60))
    }

    /// Get the configured limit (req/min) for a role.
    pub fn limit_for_role(&self, role: &UserRole) -> u32 {
        self.config.limit_for_role(role)
    }

    /// Get a reference to the configuration.
    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new(RateLimitConfig::default())
    }
}

// ── Global Singleton ────────────────────────────────────────────────────────

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

// ── IP Extraction ───────────────────────────────────────────────────────────

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

// ── 429 Response Builder ────────────────────────────────────────────────────

/// Build a 429 Too Many Requests response with rate limit information.
///
/// Sets the `Retry-After` header (in seconds) and returns a JSON body:
/// `{"error": "rate_limit_exceeded", "retry_after": N, "limit": N, "remaining": 0}`
fn build_rate_limited_response(retry_after: Duration, limit: u32) -> Response {
    let retry_secs = retry_after.as_secs().max(1);

    let body = serde_json::json!({
        "error": "rate_limit_exceeded",
        "retry_after": retry_secs,
        "limit": limit,
        "remaining": 0u32,
    });

    let mut response = Json(body).into_response();
    *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
    response.headers_mut().insert(
        header::RETRY_AFTER,
        HeaderValue::from_str(&retry_secs.to_string())
            .unwrap_or(HeaderValue::from_static("1")),
    );

    response
}

// ── Core Rate Limit Logic ───────────────────────────────────────────────────

/// Core rate-limit check logic, extracted for testability.
///
/// Returns `Some(429 response)` if the request should be rate-limited,
/// or `None` if the request is allowed (and should be passed to the next
/// handler).
///
/// This function does **not** consume the request — it only reads from it.
/// The caller is responsible for forwarding the request to `next.run(req)`.
fn check_rate_limit(req: &Request, limiter: &RateLimiter) -> Option<Response> {
    let config = limiter.config();
    let path = req.uri().path();

    // Exempt paths bypass rate limiting entirely
    if config.is_exempt(path) {
        debug!("Rate limit bypassed for exempt path: {}", path);
        return None;
    }

    // Determine the key and role:
    // - Authenticated: key = user UUID, role = JWT role
    // - Unauthenticated: key = client IP, role = Guest
    let (key, role) = match req.extensions().get::<AuthUser>() {
        Some(auth_user) => {
            (auth_user.user_id.to_string(), auth_user.role.clone())
        }
        None => {
            let ip = extract_client_ip(req);
            (ip, UserRole::Guest)
        }
    };

    let limit = config.limit_for_role(&role);

    // Check the rate limit
    match limiter.check(&key, &role) {
        Ok(()) => {
            debug!(
                "Rate limit OK — role={:?}, key={}, limit={}/min",
                role, key, limit
            );
            None
        }
        Err(retry_after) => {
            let retry_secs = retry_after.as_secs().max(1);

            warn!(
                "Rate limit exceeded — role={:?}, key={}, limit={}/min, retry_after={}s",
                role, key, limit, retry_secs
            );

            Some(build_rate_limited_response(retry_after, limit))
        }
    }
}

// ── Axum Middleware ─────────────────────────────────────────────────────────

/// Rate limiting middleware.
///
/// Extracts the user role from request extensions (set by `auth_middleware`)
/// and applies per-role token-bucket limits. Unauthenticated requests are
/// limited by IP address under the Guest quota. Exempt paths bypass the
/// limiter entirely.
///
/// On rate limit exceeded, returns HTTP 429 with a `Retry-After` header
/// and a JSON body describing the limit.
///
/// # Layer ordering
///
/// This middleware should run **after** the auth middleware (so it can read
/// the `AuthUser` extension) and **after** the request-id middleware (so
/// logs are correlated). In axum, outer layers execute first, so:
///
/// ```ignore
/// // Layers listed last execute first (outermost):
/// .layer(middleware::from_fn(rate_limit_middleware))
/// .layer(auth_layer)                    // runs before rate_limit
/// .layer(middleware::from_fn(request_id_middleware)) // runs before auth
/// ```
pub async fn rate_limit_middleware(req: Request, next: Next) -> Response {
    let limiter = get_rate_limiter();

    if let Some(response) = check_rate_limit(&req, &limiter) {
        return response;
    }

    next.run(req).await
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use uuid::Uuid;

    // ── Config tests ────────────────────────────────────────────────────────

    #[test]
    fn test_default_config_limits() {
        let config = RateLimitConfig::default();
        assert_eq!(config.admin_limit, 1000);
        assert_eq!(config.manager_limit, 500);
        assert_eq!(config.user_limit, 100);
        assert_eq!(config.viewer_limit, 50);
        assert_eq!(config.guest_limit, 20);
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

        // All specified exempt paths should be exempt
        assert!(config.is_exempt("/health"));
        assert!(config.is_exempt("/ready"));
        assert!(config.is_exempt("/metrics"));
        assert!(config.is_exempt("/api/auth/login"));
        assert!(config.is_exempt("/api/auth/register"));
        assert!(config.is_exempt("/api/auth/refresh"));
    }

    #[test]
    fn test_config_non_exempt_paths() {
        let config = RateLimitConfig::default();

        // Regular API paths should NOT be exempt
        assert!(!config.is_exempt("/api/v1/accounts"));
        assert!(!config.is_exempt("/api/v1/transactions"));
        assert!(!config.is_exempt("/ws/chat"));
        assert!(!config.is_exempt("/"));
        assert!(!config.is_exempt("/api/auth/logout"));
    }

    #[test]
    fn test_config_custom_exempt_paths() {
        let config = RateLimitConfig {
            exempt_paths: vec!["/custom".to_string()],
            ..Default::default()
        };

        assert!(config.is_exempt("/custom"));
        assert!(!config.is_exempt("/health")); // overridden
    }

    // ── RateLimiter per-role tests ──────────────────────────────────────────

    #[test]
    fn test_rate_limiter_admin_allows_up_to_limit() {
        let limiter = RateLimiter::new(RateLimitConfig {
            admin_limit: 5,
            ..Default::default()
        });
        let key = "test-admin-key";

        // First 5 requests should be allowed
        for i in 0..5 {
            assert!(
                limiter.check(key, &UserRole::Admin).is_ok(),
                "Admin request {} should be allowed",
                i + 1
            );
        }

        // 6th request should be throttled
        assert!(
            limiter.check(key, &UserRole::Admin).is_err(),
            "6th Admin request should be rate limited"
        );
    }

    #[test]
    fn test_rate_limiter_manager_allows_up_to_limit() {
        let limiter = RateLimiter::new(RateLimitConfig {
            manager_limit: 5,
            ..Default::default()
        });
        let key = "test-manager-key";

        for _ in 0..5 {
            assert!(limiter.check(key, &UserRole::Manager).is_ok());
        }
        assert!(limiter.check(key, &UserRole::Manager).is_err());
    }

    #[test]
    fn test_rate_limiter_user_allows_up_to_limit() {
        let limiter = RateLimiter::new(RateLimitConfig {
            user_limit: 5,
            ..Default::default()
        });
        let key = "test-user-key";

        for _ in 0..5 {
            assert!(limiter.check(key, &UserRole::User).is_ok());
        }
        assert!(limiter.check(key, &UserRole::User).is_err());
    }

    #[test]
    fn test_rate_limiter_viewer_allows_up_to_limit() {
        let limiter = RateLimiter::new(RateLimitConfig {
            viewer_limit: 5,
            ..Default::default()
        });
        let key = "test-viewer-key";

        for _ in 0..5 {
            assert!(limiter.check(key, &UserRole::Viewer).is_ok());
        }
        assert!(limiter.check(key, &UserRole::Viewer).is_err());
    }

    #[test]
    fn test_rate_limiter_guest_allows_up_to_limit() {
        let limiter = RateLimiter::new(RateLimitConfig {
            guest_limit: 5,
            ..Default::default()
        });
        let key = "test-guest-key";

        for _ in 0..5 {
            assert!(limiter.check(key, &UserRole::Guest).is_ok());
        }
        assert!(limiter.check(key, &UserRole::Guest).is_err());
    }

    // ── RateLimiter isolation tests ─────────────────────────────────────────

    #[test]
    fn test_rate_limiter_independent_keys() {
        // Different keys under the same role have independent quotas
        let limiter = RateLimiter::new(RateLimitConfig {
            user_limit: 3,
            ..Default::default()
        });

        // Exhaust key A
        for _ in 0..3 {
            assert!(limiter.check("key-a", &UserRole::User).is_ok());
        }
        assert!(limiter.check("key-a", &UserRole::User).is_err());

        // Key B should still be allowed
        assert!(
            limiter.check("key-b", &UserRole::User).is_ok(),
            "Different keys should have independent quotas"
        );
    }

    #[test]
    fn test_rate_limiter_independent_roles() {
        // Same key under different roles has independent quotas
        let limiter = RateLimiter::new(RateLimitConfig {
            admin_limit: 3,
            user_limit: 3,
            ..Default::default()
        });
        let key = "same-user-id";

        // Exhaust User quota for this key
        for _ in 0..3 {
            assert!(limiter.check(key, &UserRole::User).is_ok());
        }
        assert!(limiter.check(key, &UserRole::User).is_err());

        // Same key under Admin should still be allowed (separate governor)
        assert!(
            limiter.check(key, &UserRole::Admin).is_ok(),
            "Different roles should have independent quotas even for the same key"
        );
    }

    // ── RateLimiter retry duration test ─────────────────────────────────────

    #[test]
    fn test_rate_limiter_returns_retry_duration() {
        let limiter = RateLimiter::new(RateLimitConfig {
            guest_limit: 2,
            ..Default::default()
        });
        let key = "test-retry-duration";

        // Exhaust the limit
        limiter.check(key, &UserRole::Guest).unwrap();
        limiter.check(key, &UserRole::Guest).unwrap();

        // The third request should return an error with a retry duration
        let result = limiter.check(key, &UserRole::Guest);
        assert!(result.is_err());
        let retry_after = result.unwrap_err();

        // Retry-After should be positive but at most 60 seconds
        // (2 req/min => ~30s per token refill)
        assert!(
            retry_after > Duration::ZERO,
            "Retry duration should be positive"
        );
        assert!(
            retry_after <= Duration::from_secs(60),
            "Retry duration should be at most 60s, got {:?}",
            retry_after
        );
    }

    // ── IP extraction tests ─────────────────────────────────────────────────

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
        // Should take the first IP in the comma-separated list
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
        // X-Forwarded-For should take precedence over X-Real-IP
        let req = Request::builder()
            .header("x-forwarded-for", "1.2.3.4")
            .header("x-real-ip", "5.6.7.8")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_client_ip(&req), "1.2.3.4");
    }

    #[test]
    fn test_extract_ip_fallback_unknown() {
        // No headers and no SocketAddr extension => "unknown"
        let req = Request::builder()
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_client_ip(&req), "unknown");
    }

    #[test]
    fn test_extract_ip_from_socket_addr() {
        // SocketAddr in extensions should be used as a fallback
        let mut req = Request::builder()
            .body(Body::empty())
            .unwrap();
        let addr: std::net::SocketAddr = "192.168.1.100:12345".parse().unwrap();
        req.extensions_mut().insert(addr);
        assert_eq!(extract_client_ip(&req), "192.168.1.100");
    }

    // ── 429 response tests ──────────────────────────────────────────────────

    #[tokio::test]
    async fn test_429_response_status_and_headers() {
        let response = build_rate_limited_response(Duration::from_secs(30), 100);

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            response.headers().get(header::RETRY_AFTER),
            Some(&HeaderValue::from_static("30"))
        );
    }

    #[tokio::test]
    async fn test_429_response_body() {
        let response = build_rate_limited_response(Duration::from_secs(45), 50);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["error"], "rate_limit_exceeded");
        assert_eq!(json["retry_after"], 45);
        assert_eq!(json["limit"], 50);
        assert_eq!(json["remaining"], 0);
    }

    #[test]
    fn test_429_response_minimum_retry_after() {
        // A sub-second retry should be rounded up to 1 second
        let response = build_rate_limited_response(Duration::from_millis(500), 10);

        assert_eq!(
            response.headers().get(header::RETRY_AFTER),
            Some(&HeaderValue::from_static("1"))
        );
    }

    // ── check_rate_limit: exempt paths ──────────────────────────────────────

    #[test]
    fn test_check_rate_limit_exempt_path_bypasses() {
        // Even with a tiny guest_limit, exempt paths should bypass
        let limiter = RateLimiter::new(RateLimitConfig {
            guest_limit: 1,
            ..Default::default()
        });

        // Make many requests to /health — should all bypass
        for _ in 0..10 {
            let req = Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap();
            assert!(
                check_rate_limit(&req, &limiter).is_none(),
                "Exempt path /health should bypass rate limiting"
            );
        }
    }

    #[test]
    fn test_check_rate_limit_all_exempt_paths_bypass() {
        let limiter = RateLimiter::new(RateLimitConfig {
            guest_limit: 1,
            ..Default::default()
        });

        for path in &[
            "/health",
            "/ready",
            "/metrics",
            "/api/auth/login",
            "/api/auth/register",
            "/api/auth/refresh",
        ] {
            let req = Request::builder()
                .uri(*path)
                .body(Body::empty())
                .unwrap();
            assert!(
                check_rate_limit(&req, &limiter).is_none(),
                "Exempt path {} should bypass rate limiting",
                path
            );
        }
    }

    // ── check_rate_limit: unauthenticated (IP-keyed) ────────────────────────

    #[test]
    fn test_check_rate_limit_unauthenticated_uses_ip_as_key() {
        let limiter = RateLimiter::new(RateLimitConfig {
            guest_limit: 2,
            ..Default::default()
        });

        // Requests from the same IP should share the Guest quota
        let make_req = || {
            Request::builder()
                .uri("/api/v1/accounts")
                .header("x-forwarded-for", "1.2.3.4")
                .body(Body::empty())
                .unwrap()
        };

        // First 2 requests are allowed
        assert!(check_rate_limit(&make_req(), &limiter).is_none());
        assert!(check_rate_limit(&make_req(), &limiter).is_none());

        // Third request is rate limited
        let result = check_rate_limit(&make_req(), &limiter);
        assert!(result.is_some(), "Third request from same IP should be rate limited");

        let response = result.unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn test_check_rate_limit_different_ips_independent() {
        let limiter = RateLimiter::new(RateLimitConfig {
            guest_limit: 1,
            ..Default::default()
        });

        // IP A exhausts quota
        let req_a = Request::builder()
            .uri("/api/v1/accounts")
            .header("x-forwarded-for", "1.1.1.1")
            .body(Body::empty())
            .unwrap();
        assert!(check_rate_limit(&req_a, &limiter).is_none());
        assert!(check_rate_limit(&req_a, &limiter).is_some());

        // IP B should still be allowed
        let req_b = Request::builder()
            .uri("/api/v1/accounts")
            .header("x-forwarded-for", "2.2.2.2")
            .body(Body::empty())
            .unwrap();
        assert!(
            check_rate_limit(&req_b, &limiter).is_none(),
            "Different IP should have independent quota"
        );
    }

    // ── check_rate_limit: authenticated (user-ID-keyed) ─────────────────────

    #[test]
    fn test_check_rate_limit_authenticated_uses_user_id_as_key() {
        let limiter = RateLimiter::new(RateLimitConfig {
            user_limit: 2,
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

        // First 2 requests are allowed
        assert!(check_rate_limit(&make_req(), &limiter).is_none());
        assert!(check_rate_limit(&make_req(), &limiter).is_none());

        // Third request is rate limited
        assert!(
            check_rate_limit(&make_req(), &limiter).is_some(),
            "Third request from same user should be rate limited"
        );
    }

    #[test]
    fn test_check_rate_limit_different_users_independent() {
        let limiter = RateLimiter::new(RateLimitConfig {
            user_limit: 2,
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

        // User A exhausts their quota
        let req_a = make_req("00000000-0000-0000-0000-00000000000a");
        assert!(check_rate_limit(&req_a, &limiter).is_none());
        assert!(check_rate_limit(&req_a, &limiter).is_none());
        assert!(check_rate_limit(&req_a, &limiter).is_some());

        // User B should still be allowed
        let req_b = make_req("00000000-0000-0000-0000-00000000000b");
        assert!(
            check_rate_limit(&req_b, &limiter).is_none(),
            "Different user should have independent quota"
        );
    }

    #[test]
    fn test_check_rate_limit_admin_role_uses_admin_quota() {
        // Admin should get 1000 req/min, not the Guest 20
        let limiter = RateLimiter::new(RateLimitConfig::default());

        let user_id = Uuid::new_v4();
        let req = {
            let mut r = Request::builder()
                .uri("/api/v1/accounts")
                .body(Body::empty())
                .unwrap();
            r.extensions_mut().insert(AuthUser {
                user_id,
                role: UserRole::Admin,
            });
            r
        };

        // Should be allowed well beyond the Guest limit of 20
        // (testing a few to confirm Admin quota is used, not Guest)
        for _ in 0..25 {
            assert!(
                check_rate_limit(&req, &limiter).is_none(),
                "Admin should have 1000 req/min, not 20"
            );
        }
    }

    // ── check_rate_limit: 429 response correctness ──────────────────────────

    #[test]
    fn test_check_rate_limit_429_has_correct_status() {
        let limiter = RateLimiter::new(RateLimitConfig {
            viewer_limit: 1,
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
                role: UserRole::Viewer,
            });
            req
        };

        // First request is allowed
        assert!(check_rate_limit(&make_req(), &limiter).is_none());

        // Second request is rate limited with 429
        let response = check_rate_limit(&make_req(), &limiter).unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn test_check_rate_limit_429_has_retry_after_header() {
        let limiter = RateLimiter::new(RateLimitConfig {
            guest_limit: 1,
            ..Default::default()
        });

        let make_req = || {
            Request::builder()
                .uri("/api/v1/accounts")
                .header("x-forwarded-for", "7.7.7.7")
                .body(Body::empty())
                .unwrap()
        };

        // First request is allowed
        assert!(check_rate_limit(&make_req(), &limiter).is_none());

        // Second request is rate limited
        let response = check_rate_limit(&make_req(), &limiter).unwrap();

        // Must have Retry-After header
        let retry_after = response.headers().get(header::RETRY_AFTER);
        assert!(retry_after.is_some(), "429 response must have Retry-After header");

        // Retry-After should be a valid integer >= 1
        let value = retry_after.unwrap().to_str().unwrap();
        let secs: u64 = value.parse().unwrap();
        assert!(secs >= 1, "Retry-After should be at least 1 second");
    }

    #[tokio::test]
    async fn test_check_rate_limit_429_body_is_correct_json() {
        let limiter = RateLimiter::new(RateLimitConfig {
            guest_limit: 1,
            ..Default::default()
        });

        let make_req = || {
            Request::builder()
                .uri("/api/v1/accounts")
                .header("x-forwarded-for", "8.8.8.8")
                .body(Body::empty())
                .unwrap()
        };

        // First request is allowed
        assert!(check_rate_limit(&make_req(), &limiter).is_none());

        // Second request returns 429
        let response = check_rate_limit(&make_req(), &limiter).unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

        // Verify JSON body
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["error"], "rate_limit_exceeded");
        assert!(json["retry_after"].is_u64());
        assert!(json["retry_after"].as_u64().unwrap() >= 1);
        assert_eq!(json["limit"], 1); // guest_limit = 1
        assert_eq!(json["remaining"], 0);
    }
}
