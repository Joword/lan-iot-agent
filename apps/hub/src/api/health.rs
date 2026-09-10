//! Health check endpoint.

use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;
use std::sync::Arc;

use crate::adapters::ha::HaConnectionStatus;
use crate::AppState;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub service: &'static str,
    pub ha: HaHealth,
    pub mongodb: MongoHealth,
    pub devices_cached: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adapters: Vec<String>,
}

#[derive(Serialize)]
pub struct HaHealth {
    pub configured: bool,
    pub connection: HaConnectionStatus,
}

#[derive(Serialize)]
pub struct MongoHealth {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/api/v1/health", get(health))
}

/// GET /api/v1/health
pub async fn health(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let connection = state.ha_status.read().await.clone();
    let devices_cached = state.registry.len().await;
    let mongodb = match &state.mongo {
        Some(store) => {
            let ok = store.ping().await.is_ok();
            MongoHealth {
                ok,
                database: Some(store.database_name().to_string()),
            }
        }
        None => MongoHealth {
            ok: false,
            database: None,
        },
    };
    Json(HealthResponse {
        status: "ok",
        service: "hub",
        ha: HaHealth {
            configured: state.adapters.is_configured("ha"),
            connection,
        },
        mongodb,
        devices_cached,
        adapters: state.adapters.sources(),
    })
}
