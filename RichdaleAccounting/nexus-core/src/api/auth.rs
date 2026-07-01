//! Auth Module — JWT creation, validation, middleware, and extractors.
//!
//! Provides the authentication layer for the API. Uses HS256 symmetric JWT
//! with a configurable secret. Access tokens (30 min) and refresh tokens (7 days)
//! carry a `token_type` claim to prevent cross-use. The middleware skips paths
//! under `/auth/` and `/health`.

use axum::{
    extract::Request,
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use async_trait::async_trait;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::database::models::UserRole;

// ── JWT Claims ─────────────────────────────────────────────────────────────

/// Claims embedded in every JWT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject — user UUID as string
    pub sub: String,
    /// User role
    pub role: String,
    /// Token type: "access" or "refresh"
    pub typ: String,
    /// Issued-at (Unix timestamp)
    pub iat: usize,
    /// Expiration (Unix timestamp)
    pub exp: usize,
}

// ── Auth User ──────────────────────────────────────────────────────────────

/// The authenticated user, extracted from a validated JWT.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: Uuid,
    pub role: UserRole,
}

impl AuthUser {
    pub fn user_id(&self) -> Uuid {
        self.user_id
    }

    pub fn role(&self) -> &UserRole {
        &self.role
    }
}

// ── Role Guard Extractors ──────────────────────────────────────────────────

/// Extractor that requires the caller to have at least `Viewer` role.
#[derive(Debug)]
pub struct RequireViewer(pub AuthUser);

#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for RequireViewer {
    type Rejection = (StatusCode, Json<serde_json::Value>);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let auth = parts.extensions.get::<AuthUser>().ok_or_else(|| {
            (StatusCode::UNAUTHORIZED, Json(serde_json::json!({
                "success": false, "error": "Authentication required",
            })))
        })?;

        if !auth.role.can_read() {
            return Err((StatusCode::FORBIDDEN, Json(serde_json::json!({
                "success": false, "error": "Insufficient permissions — Viewer role required",
            }))));
        }
        Ok(Self(auth.clone()))
    }
}

/// Extractor that requires the caller to have at least `User` role (can write).
#[derive(Debug)]
pub struct RequireUser(pub AuthUser);

#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for RequireUser {
    type Rejection = (StatusCode, Json<serde_json::Value>);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let auth = parts.extensions.get::<AuthUser>().ok_or_else(|| {
            (StatusCode::UNAUTHORIZED, Json(serde_json::json!({
                "success": false, "error": "Authentication required",
            })))
        })?;

        if !auth.role.can_write() {
            return Err((StatusCode::FORBIDDEN, Json(serde_json::json!({
                "success": false, "error": "Insufficient permissions — User role required",
            }))));
        }
        Ok(Self(auth.clone()))
    }
}

/// Extractor that requires the caller to have at least `Manager` role.
#[derive(Debug)]
pub struct RequireManager(pub AuthUser);

#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for RequireManager {
    type Rejection = (StatusCode, Json<serde_json::Value>);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let auth = parts.extensions.get::<AuthUser>().ok_or_else(|| {
            (StatusCode::UNAUTHORIZED, Json(serde_json::json!({
                "success": false, "error": "Authentication required",
            })))
        })?;

        if !auth.role.can_manage() {
            return Err((StatusCode::FORBIDDEN, Json(serde_json::json!({
                "success": false, "error": "Insufficient permissions — Manager role required",
            }))));
        }
        Ok(Self(auth.clone()))
    }
}

/// Extractor that requires `Admin` role.
#[derive(Debug)]
pub struct RequireAdmin(pub AuthUser);

#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for RequireAdmin {
    type Rejection = (StatusCode, Json<serde_json::Value>);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let auth = parts.extensions.get::<AuthUser>().ok_or_else(|| {
            (StatusCode::UNAUTHORIZED, Json(serde_json::json!({
                "success": false, "error": "Authentication required",
            })))
        })?;

        if !auth.role.can_administer() {
            return Err((StatusCode::FORBIDDEN, Json(serde_json::json!({
                "success": false, "error": "Insufficient permissions — Admin role required",
            }))));
        }
        Ok(Self(auth.clone()))
    }
}

// ── Token Factory ──────────────────────────────────────────────────────────

/// Default access token TTL: 30 minutes.
pub const ACCESS_TOKEN_TTL: usize = 1_800;

/// Refresh token TTL: 7 days.
pub const REFRESH_TOKEN_TTL: usize = 604_800;

/// The default JWT secret placeholder — server refuses to start with this.
pub const DEFAULT_SECRET: &str = "CHANGE-ME-in-production-use-openssl-rand-base64-32";

/// Check if the configured secret is still the insecure default.
pub fn is_default_secret(secret: &str) -> bool {
    secret == DEFAULT_SECRET
}

/// Create an access token (30 min TTL, typ="access").
pub fn create_token(
    user_id: Uuid,
    role: &UserRole,
    secret: &str,
    ttl_seconds: usize,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = now_unix();
    let claims = Claims {
        sub: user_id.to_string(),
        role: role_to_str(role).to_string(),
        typ: "access".to_string(),
        iat: now,
        exp: now + ttl_seconds,
    };
    encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_bytes()))
}

/// Create a refresh token (7 day TTL, typ="refresh").
pub fn create_refresh_token(
    user_id: Uuid,
    role: &UserRole,
    secret: &str,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = now_unix();
    let claims = Claims {
        sub: user_id.to_string(),
        role: role_to_str(role).to_string(),
        typ: "refresh".to_string(),
        iat: now,
        exp: now + REFRESH_TOKEN_TTL,
    };
    encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_bytes()))
}

/// Validate any JWT and return its claims (no token_type check).
pub fn validate_token(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let mut validation = Validation::default();
    validation.leeway = 0;
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )?;
    Ok(token_data.claims)
}

/// Validate an access token — rejects refresh tokens.
pub fn validate_access_token(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let claims = validate_token(token, secret)?;
    if claims.typ != "access" {
        return Err(jsonwebtoken::errors::Error::from(
            jsonwebtoken::errors::ErrorKind::InvalidToken,
        ));
    }
    Ok(claims)
}

/// Validate a refresh token — rejects access tokens.
pub fn validate_refresh_token(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let claims = validate_token(token, secret)?;
    if claims.typ != "refresh" {
        return Err(jsonwebtoken::errors::Error::from(
            jsonwebtoken::errors::ErrorKind::InvalidToken,
        ));
    }
    Ok(claims)
}

// ── Auth Middleware ────────────────────────────────────────────────────────

pub async fn auth_middleware(
    mut req: Request,
    next: Next,
    secret: String,
) -> Response {
    let path = req.uri().path().to_string();

    // Public paths
    if path.starts_with("/auth") || path == "/health" || path == "/ready" || path == "/metrics" {
        return next.run(req).await;
    }

    // Also allow WebSocket upgrade with ?token= query param
    if path.starts_with("/ws/") {
        if let Some(token) = extract_token_from_query(&req) {
            return process_token(&token, &secret, req, next).await;
        }
    }

    // Extract Bearer token from Authorization header
    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let token = auth_header.strip_prefix("Bearer ").unwrap_or("").to_string();

    if token.is_empty() {
        return unauthorized("Missing or invalid Authorization header");
    }

    process_token(&token, &secret, req, next).await
}

/// Process a token: validate as access token, inject AuthUser.
async fn process_token(token: &str, secret: &str, mut req: Request, next: Next) -> Response {
    match validate_access_token(token, secret) {
        Ok(claims) => {
            let user_id = match Uuid::parse_str(&claims.sub) {
                Ok(id) => id,
                Err(_) => return unauthorized("Invalid user ID in token"),
            };
            let role = str_to_role(&claims.role);
            req.extensions_mut().insert(AuthUser { user_id, role });
            next.run(req).await
        }
        Err(e) => {
            let msg = match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => "Token expired",
                _ => "Invalid token",
            };
            unauthorized(msg)
        }
    }
}

fn extract_token_from_query(req: &Request) -> Option<String> {
    let query = req.uri().query()?;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=')?;
        if k == "token" {
            return Some(v.to_string());
        }
    }
    None
}

fn unauthorized(msg: &str) -> Response {
    (StatusCode::UNAUTHORIZED, Json(serde_json::json!({
        "success": false, "error": msg,
    }))).into_response()
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn now_unix() -> usize {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as usize
}

fn role_to_str(role: &UserRole) -> &str {
    match role {
        UserRole::Admin => "admin",
        UserRole::Manager => "manager",
        UserRole::User => "user",
        UserRole::Viewer => "viewer",
        UserRole::Guest => "guest",
    }
}

fn str_to_role(s: &str) -> UserRole {
    match s {
        "admin" => UserRole::Admin,
        "manager" => UserRole::Manager,
        "user" => UserRole::User,
        "viewer" => UserRole::Viewer,
        _ => UserRole::Guest,
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET: &str = "test-secret-key-for-jwt-signing-at-least-32-bytes!!";

    #[test]
    fn test_create_and_validate_access_token() {
        let user_id = Uuid::new_v4();
        let token = create_token(user_id, &UserRole::User, TEST_SECRET, 3600).unwrap();
        let claims = validate_access_token(&token, TEST_SECRET).unwrap();
        assert_eq!(claims.sub, user_id.to_string());
        assert_eq!(claims.role, "user");
        assert_eq!(claims.typ, "access");
    }

    #[test]
    fn test_refresh_token_rejected_as_access() {
        let user_id = Uuid::new_v4();
        let token = create_refresh_token(user_id, &UserRole::User, TEST_SECRET).unwrap();
        let result = validate_access_token(&token, TEST_SECRET);
        assert!(result.is_err());
    }

    #[test]
    fn test_access_token_rejected_as_refresh() {
        let user_id = Uuid::new_v4();
        let token = create_token(user_id, &UserRole::User, TEST_SECRET, 3600).unwrap();
        let result = validate_refresh_token(&token, TEST_SECRET);
        assert!(result.is_err());
    }

    #[test]
    fn test_refresh_token_validates() {
        let user_id = Uuid::new_v4();
        let token = create_refresh_token(user_id, &UserRole::Manager, TEST_SECRET).unwrap();
        let claims = validate_refresh_token(&token, TEST_SECRET).unwrap();
        assert_eq!(claims.typ, "refresh");
        assert_eq!(claims.role, "manager");
    }

    #[test]
    fn test_expired_token_fails() {
        let user_id = Uuid::new_v4();
        let token = create_token(user_id, &UserRole::User, TEST_SECRET, 0).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
        assert!(validate_access_token(&token, TEST_SECRET).is_err());
    }

    #[test]
    fn test_wrong_secret_fails() {
        let user_id = Uuid::new_v4();
        let token = create_token(user_id, &UserRole::User, TEST_SECRET, 3600).unwrap();
        assert!(validate_access_token(&token, "wrong-secret-key-that-is-different!!").is_err());
    }

    #[test]
    fn test_is_default_secret() {
        assert!(is_default_secret(DEFAULT_SECRET));
        assert!(!is_default_secret("a-real-secret-key-with-32-bytes-min!!!"));
    }

    #[test]
    fn test_role_conversions() {
        assert_eq!(role_to_str(&UserRole::Admin), "admin");
        assert_eq!(str_to_role("admin"), UserRole::Admin);
        assert_eq!(str_to_role("unknown"), UserRole::Guest);
    }
}
