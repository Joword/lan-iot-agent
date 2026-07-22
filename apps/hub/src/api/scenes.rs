//! Scene REST API — list / create-upsert / run via SceneEngine + HA adapter.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::api::devices::ErrorBody;
use crate::scene::{RunResult, Scene, SceneAction};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/scenes", get(list_scenes).post(upsert_scene))
        .route("/api/v1/scenes/{id}/run", post(run_scene))
}

#[derive(Serialize)]
pub struct SceneListResponse {
    pub scenes: Vec<Scene>,
    pub count: usize,
}

#[derive(Deserialize)]
pub struct UpsertSceneRequest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Ordered device actions (API field name; stored as scene `actions`).
    #[serde(default)]
    pub steps: Vec<SceneAction>,
}

/// GET /api/v1/scenes
pub async fn list_scenes(State(state): State<Arc<AppState>>) -> Json<SceneListResponse> {
    let scenes = state.scenes.list().await;
    let count = scenes.len();
    Json(SceneListResponse { scenes, count })
}

/// POST /api/v1/scenes — create or update a scene.
///
/// Body: `{"id","name","steps":[{entity_id,action,params?}]}` (+ optional `description`).
/// Persists via [`SceneEngine::upsert`] (Mongo when available; otherwise memory only).
pub async fn upsert_scene(
    State(state): State<Arc<AppState>>,
    Json(body): Json<UpsertSceneRequest>,
) -> Result<(StatusCode, Json<Scene>), (StatusCode, Json<ErrorBody>)> {
    let scene = Scene {
        id: body.id,
        name: body.name,
        description: body.description,
        actions: body.steps,
    };
    match state.scenes.upsert(scene.clone()).await {
        Ok(()) => Ok((StatusCode::OK, Json(scene))),
        Err(crate::scene::SceneError::Invalid(msg)) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "invalid_scene".into(),
                detail: Some(msg),
            }),
        )),
        Err(crate::scene::SceneError::Unavailable) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorBody {
                error: "scene_store_unavailable".into(),
                detail: Some("MongoDB scene store unavailable".into()),
            }),
        )),
        Err(crate::scene::SceneError::NotFound(sid)) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "scene_not_found".into(),
                detail: Some(format!("scene '{sid}' not found")),
            }),
        )),
    }
}

/// POST /api/v1/scenes/:id/run
///
/// Executes actions sequentially. Missing entities are skipped (no-op).
/// Partial failure returns HTTP 200 with `ok: false` and `failed` step indices.
pub async fn run_scene(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<RunResult>, (StatusCode, Json<ErrorBody>)> {
    match state
        .scenes
        .run(&id, &state.adapters, &state.registry, &state.events)
        .await
    {
        Ok(result) => Ok(Json(result)),
        Err(crate::scene::SceneError::NotFound(sid)) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "scene_not_found".into(),
                detail: Some(format!("scene '{sid}' not found")),
            }),
        )),
        Err(crate::scene::SceneError::Unavailable) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorBody {
                error: "scene_store_unavailable".into(),
                detail: Some("MongoDB scene store unavailable".into()),
            }),
        )),
        Err(crate::scene::SceneError::Invalid(msg)) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "invalid_scene".into(),
                detail: Some(msg),
            }),
        )),
    }
}
