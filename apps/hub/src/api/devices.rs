//! Device REST API — list / detail / actions via AdapterRouter + registry.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use crate::adapters::AdapterError;
use crate::registry::DeviceEntity;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/devices", get(list_devices))
        .route("/api/v1/devices/{id}", get(get_device))
        .route("/api/v1/devices/{id}/actions", post(device_action))
}

#[derive(Serialize)]
pub struct DeviceListResponse {
    pub devices: Vec<DeviceEntity>,
    pub ha_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adapters: Vec<String>,
}

#[derive(Serialize)]
pub struct ErrorBody {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Deserialize)]
pub struct ActionRequest {
    pub action: String,
    #[serde(default)]
    pub params: HashMap<String, Value>,
}

#[derive(Serialize)]
pub struct ActionResponse {
    pub ok: bool,
    pub entity_id: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
}

/// GET /api/v1/devices
pub async fn list_devices(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<DeviceListResponse>) {
    // Best-effort refresh if cache empty and HA configured.
    if state.registry.len().await == 0 && state.adapters.is_configured("ha") {
        if let Err(e) = state.adapters.sync_into("ha", &state.registry).await {
            tracing::warn!(error = %e, "on-demand HA sync failed");
            return (
                StatusCode::OK,
                Json(DeviceListResponse {
                    devices: vec![],
                    ha_available: false,
                    warning: Some(e.to_string()),
                    adapters: state.adapters.sources(),
                }),
            );
        }
    }

    let devices = state.registry.list_devices().await;
    let ha_available = matches!(
        *state.ha_status.read().await,
        crate::adapters::ha::HaConnectionStatus::Connected
    ) || (!devices.is_empty() && state.adapters.is_configured("ha"));

    let warning = if !state.adapters.is_configured("ha") {
        if devices.iter().any(|d| d.is_faker) {
            Some(
                "HA not configured — serving in-memory faker brand devices (SEED_FAKER_DEVICES). Swap with real HA entities later."
                    .into(),
            )
        } else {
            Some("HA not configured — set HA_URL and HA_TOKEN".into())
        }
    } else if !ha_available && devices.is_empty() {
        Some("Home Assistant unreachable or empty — returning empty device list".into())
    } else {
        None
    };

    (
        StatusCode::OK,
        Json(DeviceListResponse {
            devices,
            ha_available,
            warning,
            adapters: state.adapters.sources(),
        }),
    )
}

/// GET /api/v1/devices/:id  (id = entity_id, e.g. light.faker_esp32_light)
pub async fn get_device(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<DeviceEntity>, (StatusCode, Json<ErrorBody>)> {
    match state.adapters.get_state(&state.registry, &id).await {
        Ok(entity) => Ok(Json(entity)),
        Err(e) => Err(adapter_http_err(e)),
    }
}

/// POST /api/v1/devices/:id/actions
pub async fn device_action(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<ActionRequest>,
) -> Result<Json<ActionResponse>, (StatusCode, Json<ErrorBody>)> {
    match state
        .adapters
        .control(
            &state.registry,
            &state.events,
            &id,
            &body.action,
            &body.params,
        )
        .await
    {
        Ok(outcome) => Ok(Json(ActionResponse {
            ok: outcome.ok,
            entity_id: outcome.entity_id,
            action: outcome.action,
            source: Some(outcome.source),
            result: Some(outcome.result),
        })),
        Err(e) => Err(adapter_http_err(e)),
    }
}

fn adapter_http_err(err: AdapterError) -> (StatusCode, Json<ErrorBody>) {
    match err {
        AdapterError::NotConfigured(src) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorBody {
                error: "adapter_not_configured".into(),
                detail: Some(format!("{src} not configured")),
            }),
        ),
        AdapterError::Unreachable(msg) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorBody {
                error: "adapter_unreachable".into(),
                detail: Some(msg),
            }),
        ),
        AdapterError::Api { status, body } => {
            let code = if status == 404 {
                StatusCode::NOT_FOUND
            } else if status == 401 || status == 403 {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::BAD_GATEWAY
            };
            (
                code,
                Json(ErrorBody {
                    error: "adapter_api_error".into(),
                    detail: Some(format!("{status}: {body}")),
                }),
            )
        }
        AdapterError::NotFound(id) => (
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: "device_not_found".into(),
                detail: Some(format!("entity_id '{id}' not found")),
            }),
        ),
        AdapterError::Invalid(msg) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "invalid_action".into(),
                detail: Some(msg),
            }),
        ),
        AdapterError::NoAdapter(msg) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorBody {
                error: "no_adapter".into(),
                detail: Some(msg),
            }),
        ),
    }
}

/// Helper for unit-style docs / future tests.
#[allow(dead_code)]
pub fn example_action_payload() -> Value {
    json!({
        "action": "turn_on",
        "params": {}
    })
}
