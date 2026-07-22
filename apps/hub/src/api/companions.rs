//! Companion REST API — list / register / command via CompanionAdapter.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::adapters::companion::{CommandResult, CompanionDevice, CompanionError};
use crate::api::devices::ErrorBody;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/v1/companions",
            get(list_companions).post(register_companion),
        )
        .route(
            "/api/v1/companions/{id}",
            delete(unregister_companion),
        )
        .route("/api/v1/companions/{id}/command", post(companion_command))
}

#[derive(Serialize)]
pub struct CompanionListResponse {
    pub companions: Vec<CompanionDevice>,
    pub count: usize,
}

#[derive(Deserialize)]
pub struct CommandRequest {
    pub command: String,
}

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub id: String,
    pub name: String,
    pub base_url: String,
    #[serde(default = "default_kind")]
    pub kind: String,
}

fn default_kind() -> String {
    "pc".into()
}

/// GET /api/v1/companions
pub async fn list_companions(
    State(state): State<Arc<AppState>>,
) -> Json<CompanionListResponse> {
    let companions = state.companions.list().await;
    let count = companions.len();
    Json(CompanionListResponse { companions, count })
}

/// POST /api/v1/companions — register or update a Companion endpoint.
///
/// Body: `{"id","name","base_url","kind"?}`. Persists to Mongo when available.
pub async fn register_companion(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<CompanionDevice>), (StatusCode, Json<ErrorBody>)> {
    let device = CompanionDevice {
        id: body.id,
        name: body.name,
        base_url: body.base_url,
        kind: body.kind,
    };
    match state.companions.upsert(device.clone()).await {
        Ok(()) => Ok((StatusCode::OK, Json(device))),
        Err(CompanionError::Invalid(msg)) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "invalid_companion".into(),
                detail: Some(msg),
            }),
        )),
        Err(CompanionError::Unavailable) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorBody {
                error: "companion_store_unavailable".into(),
                detail: Some("MongoDB companion upsert failed".into()),
            }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: "companion_register_failed".into(),
                detail: Some(e.to_string()),
            }),
        )),
    }
}

/// DELETE /api/v1/companions/:id — unregister companion.
pub async fn unregister_companion(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorBody>)> {
    match state.companions.remove(&id).await {
        Ok(()) => Ok(Json(serde_json::json!({ "ok": true, "id": id }))),
        Err(CompanionError::NotFound(cid)) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "companion_not_found".into(),
                detail: Some(format!("companion '{cid}' not registered")),
            }),
        )),
        Err(CompanionError::Unavailable) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorBody {
                error: "companion_store_unavailable".into(),
                detail: Some("MongoDB companion delete failed".into()),
            }),
        )),
        Err(CompanionError::Invalid(msg)) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "invalid_companion".into(),
                detail: Some(msg),
            }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: "companion_unregister_failed".into(),
                detail: Some(e.to_string()),
            }),
        )),
    }
}

/// POST /api/v1/companions/:id/command
///
/// Body: `{"command":"…"}`. Unknown id → 404. Offline Companion → 503 with detail
/// (Hub stays up; demo `companion.demo_pc` may be unreachable).
pub async fn companion_command(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<CommandRequest>,
) -> Result<Json<CommandResult>, (StatusCode, Json<ErrorBody>)> {
    match state.companions.send_command(&id, &body.command).await {
        Ok(result) => Ok(Json(result)),
        Err(CompanionError::NotFound(cid)) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "companion_not_found".into(),
                detail: Some(format!("companion '{cid}' not registered")),
            }),
        )),
        Err(CompanionError::Unreachable(msg)) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorBody {
                error: "companion_unreachable".into(),
                detail: Some(msg),
            }),
        )),
        Err(CompanionError::Api { status, body }) => Err((
            StatusCode::BAD_GATEWAY,
            Json(ErrorBody {
                error: "companion_api_error".into(),
                detail: Some(format!("Companion {status}: {body}")),
            }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: "companion_command_failed".into(),
                detail: Some(e.to_string()),
            }),
        )),
    }
}
