//! LAN scan / adopt — Companion HTTP already listening (PC / phone / robot).
//! `kind=chip` on port 9878 is an R&D firmware hook, not a home device class.

use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::adapters::companion::CompanionDevice;
use crate::api::devices::ErrorBody;
use crate::lan::{self, LanEndpoint};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/lan/scan", post(scan_lan))
        .route("/api/v1/lan/adopt", post(adopt_lan))
}

#[derive(Serialize)]
pub struct ScanResponse {
    pub devices: Vec<LanEndpoint>,
    pub count: usize,
}

/// POST /api/v1/lan/scan — probe Companion health on the LAN (robot = product; chip = R&D).
pub async fn scan_lan(State(state): State<Arc<AppState>>) -> Json<ScanResponse> {
    let adopted = state.companions.list().await;
    let devices = lan::scan(&adopted).await;
    let count = devices.len();
    Json(ScanResponse { devices, count })
}

#[derive(Deserialize)]
pub struct AdoptRequest {
    pub base_url: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
}

/// POST /api/v1/lan/adopt — probe then upsert into the Companion registry.
pub async fn adopt_lan(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AdoptRequest>,
) -> Result<(StatusCode, Json<CompanionDevice>), (StatusCode, Json<ErrorBody>)> {
    let base = body.base_url.trim().trim_end_matches('/').to_string();
    if base.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "invalid_request".into(),
                detail: Some("base_url required".into()),
            }),
        ));
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .no_proxy()
        .build()
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorBody {
                    error: "scan_failed".into(),
                    detail: Some(e.to_string()),
                }),
            )
        })?;

    let probed = lan::probe(&client, &base).await.ok_or_else(|| {
        (
            StatusCode::BAD_GATEWAY,
            Json(ErrorBody {
                error: "unreachable".into(),
                detail: Some(format!("no LanIoT health at {base}")),
            }),
        )
    })?;

    let device = CompanionDevice {
        id: body.id.filter(|s| !s.is_empty()).unwrap_or(probed.id),
        name: body.name.filter(|s| !s.is_empty()).unwrap_or(probed.name),
        base_url: base,
        kind: body.kind.filter(|s| !s.is_empty()).unwrap_or(probed.kind),
    };

    state
        .companions
        .upsert(device.clone())
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(ErrorBody {
                    error: "adopt_failed".into(),
                    detail: Some(e.to_string()),
                }),
            )
        })?;

    Ok((StatusCode::CREATED, Json(device)))
}
