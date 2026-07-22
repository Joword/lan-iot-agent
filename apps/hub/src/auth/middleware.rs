//! Optional Bearer auth middleware.
//!
//! When `AUTH_REQUIRED` is false (default), all requests pass through.
//! When true, protected routes need `Authorization: Bearer <token>` from
//! `POST /api/v1/auth/pair` (validated against MongoDB). Health and pair
//! stay open. If MongoDB is down while auth is required, protected routes
//! return 503.

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::sync::Arc;

use crate::AppState;

/// Axum middleware: enforce Bearer token when `auth_required` is set.
pub async fn require_bearer(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    if !state.config.auth_required {
        return next.run(request).await;
    }

    let path = request.uri().path();
    if is_public_path(path) {
        return next.run(request).await;
    }

    if !state.auth.available() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "auth_unavailable",
                "message": "MongoDB auth store unavailable (AUTH_REQUIRED=true)"
            })),
        )
            .into_response();
    }

    let Some(token) = bearer_token(request.headers()) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "missing_token",
                "message": "Authorization: Bearer <token> required (AUTH_REQUIRED=true)"
            })),
        )
            .into_response();
    };

    match state.auth.validate_token(&token).await {
        Ok(true) => next.run(request).await,
        Ok(false) => (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "invalid_token",
                "message": "token not recognized or expired; call POST /api/v1/auth/pair"
            })),
        )
            .into_response(),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "auth_unavailable",
                "message": "MongoDB auth store unavailable (AUTH_REQUIRED=true)"
            })),
        )
            .into_response(),
    }
}

fn is_public_path(path: &str) -> bool {
    matches!(
        path,
        "/api/v1/health"
            | "/api/v1/auth/pair"
            | "/api/v1/auth/logout"
            | "/health"
            | "/api/v1/ws"
            | "/ws"
    )
}

fn bearer_token(headers: &axum::http::HeaderMap) -> Option<String> {
    let value = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    let token = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))?
        .trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}
