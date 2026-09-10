//! Home Assistant adapter — Hub ↔ HA via REST (+ optional WebSocket).
//!
//! Brand IoT and ESP32 (MQTT) are driven by HA; this module never talks MQTT
//! to devices directly.

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;

use super::traits::{AdapterError, AdapterHealth, DeviceAdapter};
use crate::events::EventBus;
use crate::registry::{DeviceEntity, DeviceRegistry, EntityType};
use async_trait::async_trait;

#[derive(Debug, Error)]
pub enum HaError {
    #[error("Home Assistant is not configured (set HA_URL and HA_TOKEN)")]
    NotConfigured,
    #[error("Home Assistant unreachable: {0}")]
    Unreachable(String),
    #[error("Home Assistant API error ({status}): {body}")]
    Api { status: u16, body: String },
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error("request error: {0}")]
    Request(#[from] reqwest::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaState {
    pub entity_id: String,
    pub state: String,
    #[serde(default)]
    pub attributes: HashMap<String, Value>,
    #[serde(default)]
    pub last_changed: Option<String>,
    #[serde(default)]
    pub last_updated: Option<String>,
}

#[derive(Clone)]
pub struct HaAdapter {
    client: reqwest::Client,
    base_url: String,
    token: String,
}

impl HaAdapter {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("reqwest client"),
            base_url,
            token: token.into(),
        }
    }

    pub fn is_configured(&self) -> bool {
        !self.base_url.is_empty() && !self.token.is_empty()
    }

    fn ensure_configured(&self) -> Result<(), HaError> {
        if self.is_configured() {
            Ok(())
        } else {
            Err(HaError::NotConfigured)
        }
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.token)
    }

    /// GET /api/ — HA health / version ping.
    pub async fn ping(&self) -> Result<Value, HaError> {
        self.ensure_configured()?;
        let url = format!("{}/api/", self.base_url);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| HaError::Unreachable(e.to_string()))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(HaError::Api {
                status: status.as_u16(),
                body,
            });
        }
        serde_json::from_str(&body).map_err(|e| HaError::InvalidResponse(e.to_string()))
    }

    /// GET /api/states — list all entities.
    pub async fn list_states(&self) -> Result<Vec<HaState>, HaError> {
        self.ensure_configured()?;
        let url = format!("{}/api/states", self.base_url);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| HaError::Unreachable(e.to_string()))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(HaError::Api {
                status: status.as_u16(),
                body,
            });
        }
        serde_json::from_str(&body).map_err(|e| HaError::InvalidResponse(e.to_string()))
    }

    /// GET /api/states/{entity_id}
    pub async fn fetch_state(&self, entity_id: &str) -> Result<HaState, HaError> {
        self.ensure_configured()?;
        let url = format!("{}/api/states/{}", self.base_url, entity_id);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| HaError::Unreachable(e.to_string()))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if status.as_u16() == 404 {
            return Err(HaError::Api {
                status: 404,
                body: format!("entity not found: {entity_id}"),
            });
        }
        if !status.is_success() {
            return Err(HaError::Api {
                status: status.as_u16(),
                body,
            });
        }
        serde_json::from_str(&body).map_err(|e| HaError::InvalidResponse(e.to_string()))
    }

    /// POST /api/services/{domain}/{service}
    pub async fn call_service(
        &self,
        domain: &str,
        service: &str,
        payload: Value,
    ) -> Result<Value, HaError> {
        self.ensure_configured()?;
        let url = format!("{}/api/services/{domain}/{service}", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", self.auth_header())
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| HaError::Unreachable(e.to_string()))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(HaError::Api {
                status: status.as_u16(),
                body,
            });
        }
        if body.trim().is_empty() {
            return Ok(json!([]));
        }
        serde_json::from_str(&body).map_err(|e| HaError::InvalidResponse(e.to_string()))
    }

    /// Map a simple Hub action to an HA service call.
    pub async fn perform_action(
        &self,
        entity_id: &str,
        action: &str,
        params: &HashMap<String, Value>,
    ) -> Result<Value, HaError> {
        let domain = entity_id
            .split_once('.')
            .map(|(d, _)| d)
            .unwrap_or("homeassistant");

        let (service, mut data) = match action {
            "turn_on" => ("turn_on", json!({ "entity_id": entity_id })),
            "turn_off" => ("turn_off", json!({ "entity_id": entity_id })),
            "toggle" => ("toggle", json!({ "entity_id": entity_id })),
            "set_temperature" => {
                let temp = params
                    .get("temperature")
                    .cloned()
                    .unwrap_or(json!(24.0));
                (
                    "set_temperature",
                    json!({ "entity_id": entity_id, "temperature": temp }),
                )
            }
            "set_brightness" => {
                let brightness = params
                    .get("brightness")
                    .cloned()
                    .unwrap_or(json!(255));
                (
                    "turn_on",
                    json!({ "entity_id": entity_id, "brightness": brightness }),
                )
            }
            "set_hvac_mode" => {
                let mode = params
                    .get("mode")
                    .cloned()
                    .unwrap_or(json!("cool"));
                (
                    "set_hvac_mode",
                    json!({ "entity_id": entity_id, "hvac_mode": mode }),
                )
            }
            other => {
                return Err(HaError::InvalidResponse(format!(
                    "unsupported action: {other} (try turn_on, turn_off, set_temperature, set_brightness)"
                )));
            }
        };

        if let Some(obj) = data.as_object_mut() {
            for (k, v) in params {
                if k != "temperature" && k != "brightness" && k != "mode" {
                    obj.insert(k.clone(), v.clone());
                }
            }
        }

        // climate.set_temperature uses climate domain; light brightness uses light.turn_on
        let service_domain = match action {
            "set_temperature" | "set_hvac_mode" => "climate",
            "turn_on" | "turn_off" | "toggle" | "set_brightness" => domain,
            _ => domain,
        };

        self.call_service(service_domain, service, data).await
    }

    /// Convert HA state into our registry entity model.
    pub fn state_to_entity(state: &HaState) -> DeviceEntity {
        let entity_type = EntityType::from_entity_id(&state.entity_id);
        let friendly_name = state
            .attributes
            .get("friendly_name")
            .and_then(|v| v.as_str())
            .unwrap_or(&state.entity_id)
            .to_string();

        DeviceEntity::new(
            state.entity_id.clone(),
            entity_type,
            friendly_name,
            state.state.clone(),
            state.attributes.clone(),
            state.state != "unavailable" && state.state != "unknown",
            "ha",
        )
    }

    /// Pull all HA states into the in-memory registry. Returns count synced,
    /// or an error (caller should degrade gracefully).
    pub async fn sync_registry(&self, registry: &DeviceRegistry) -> Result<usize, HaError> {
        let states = self.list_states().await?;
        let entities: Vec<DeviceEntity> = states.iter().map(Self::state_to_entity).collect();
        let n = entities.len();
        registry.replace_ha_entities(entities).await;
        Ok(n)
    }
}

impl From<HaError> for AdapterError {
    fn from(err: HaError) -> Self {
        match err {
            HaError::NotConfigured => AdapterError::NotConfigured("ha".into()),
            HaError::Unreachable(msg) => AdapterError::Unreachable(msg),
            HaError::Api { status, body } => AdapterError::Api { status, body },
            HaError::InvalidResponse(msg) => AdapterError::Invalid(msg),
            HaError::Request(e) => AdapterError::Unreachable(e.to_string()),
        }
    }
}

#[async_trait]
impl DeviceAdapter for HaAdapter {
    fn source_id(&self) -> &str {
        "ha"
    }

    fn is_configured(&self) -> bool {
        !self.base_url.is_empty() && !self.token.is_empty()
    }

    async fn health(&self) -> AdapterHealth {
        if !self.is_configured() {
            return AdapterHealth {
                source: "ha".into(),
                configured: false,
                ok: false,
                detail: Some("HA_URL / HA_TOKEN not set".into()),
            };
        }
        match self.ping().await {
            Ok(_) => AdapterHealth {
                source: "ha".into(),
                configured: true,
                ok: true,
                detail: None,
            },
            Err(e) => AdapterHealth {
                source: "ha".into(),
                configured: true,
                ok: false,
                detail: Some(e.to_string()),
            },
        }
    }

    async fn sync(&self) -> Result<Vec<DeviceEntity>, AdapterError> {
        let states = self.list_states().await?;
        Ok(states.iter().map(Self::state_to_entity).collect())
    }

    async fn get_state(&self, entity_id: &str) -> Result<DeviceEntity, AdapterError> {
        match self.fetch_state(entity_id).await {
            Ok(ha_state) => Ok(Self::state_to_entity(&ha_state)),
            Err(HaError::Api { status: 404, .. }) => {
                Err(AdapterError::NotFound(entity_id.to_string()))
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn control(
        &self,
        entity_id: &str,
        action: &str,
        params: &HashMap<String, Value>,
    ) -> Result<Value, AdapterError> {
        self.perform_action(entity_id, action, params)
            .await
            .map_err(Into::into)
    }
}

/// Background sync: try WebSocket `state_changed` subscription; on failure,
/// fall back to REST polling. Individual WS updates are pushed to Hub clients
/// as `device:state_changed`; full REST polls refresh the cache without flooding.
pub fn spawn_ha_sync(
    ha: HaAdapter,
    registry: Arc<DeviceRegistry>,
    ha_status: Arc<RwLock<HaConnectionStatus>>,
    events: EventBus,
    poll_secs: u64,
) {
    tokio::spawn(async move {
        // Initial REST sync (best-effort).
        match ha.sync_registry(&registry).await {
            Ok(n) => {
                tracing::info!(count = n, "HA registry initial sync ok");
                *ha_status.write().await = HaConnectionStatus::Connected;
            }
            Err(e) => {
                tracing::warn!(error = %e, "HA initial sync failed — Hub stays up with empty device list");
                *ha_status.write().await = HaConnectionStatus::Degraded(e.to_string());
            }
        }

        // Prefer WS; if it exits, REST-poll until WS can be retried.
        loop {
            if ha.is_configured() {
                match run_ha_websocket(&ha, &registry, &ha_status, &events).await {
                    Ok(()) => tracing::info!("HA WebSocket closed cleanly; reconnecting"),
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "HA WebSocket unavailable — falling back to REST polling"
                        );
                        *ha_status.write().await =
                            HaConnectionStatus::Degraded(format!("ws: {e}"));
                    }
                }
            }

            // REST polling fallback (cache only — no per-entity client flood).
            let interval = std::time::Duration::from_secs(poll_secs.max(5));
            for _ in 0..6 {
                tokio::time::sleep(interval).await;
                match ha.sync_registry(&registry).await {
                    Ok(n) => {
                        tracing::debug!(count = n, "HA REST poll sync ok");
                        *ha_status.write().await = HaConnectionStatus::Connected;
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "HA REST poll failed");
                        *ha_status.write().await = HaConnectionStatus::Degraded(e.to_string());
                    }
                }
            }
            // After a few poll cycles, retry WebSocket.
        }
    });
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", content = "detail")]
pub enum HaConnectionStatus {
    Unknown,
    Connected,
    Degraded(String),
    NotConfigured,
}

impl Default for HaConnectionStatus {
    fn default() -> Self {
        Self::Unknown
    }
}

async fn run_ha_websocket(
    ha: &HaAdapter,
    registry: &DeviceRegistry,
    ha_status: &RwLock<HaConnectionStatus>,
    events: &EventBus,
) -> Result<(), HaError> {
    ha.ensure_configured()?;

    let http = Url::parse(&ha.base_url)
        .map_err(|e| HaError::InvalidResponse(format!("bad HA_URL: {e}")))?;
    let ws_scheme = if http.scheme() == "https" {
        "wss"
    } else {
        "ws"
    };
    let mut ws_url = http.clone();
    ws_url
        .set_scheme(ws_scheme)
        .map_err(|_| HaError::InvalidResponse("cannot set ws scheme".into()))?;
    ws_url.set_path("/api/websocket");

    let (ws, _) = connect_async(ws_url.as_str())
        .await
        .map_err(|e| HaError::Unreachable(format!("ws connect: {e}")))?;

    let (mut write, mut read) = ws.split();
    let mut msg_id: u64 = 1;

    // Expect auth_required → send auth → auth_ok → subscribe_events
    while let Some(msg) = read.next().await {
        let msg = msg.map_err(|e| HaError::Unreachable(format!("ws read: {e}")))?;
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Ping(p) => {
                let _ = write.send(Message::Pong(p)).await;
                continue;
            }
            Message::Close(_) => break,
            _ => continue,
        };

        let v: Value = serde_json::from_str(&text)
            .map_err(|e| HaError::InvalidResponse(e.to_string()))?;
        let msg_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");

        match msg_type {
            "auth_required" => {
                let auth = json!({
                    "type": "auth",
                    "access_token": ha.token,
                });
                write
                    .send(Message::Text(auth.to_string().into()))
                    .await
                    .map_err(|e| HaError::Unreachable(format!("ws auth send: {e}")))?;
            }
            "auth_ok" => {
                tracing::info!("HA WebSocket authenticated");
                *ha_status.write().await = HaConnectionStatus::Connected;
                let sub = json!({
                    "id": msg_id,
                    "type": "subscribe_events",
                    "event_type": "state_changed",
                });
                msg_id += 1;
                write
                    .send(Message::Text(sub.to_string().into()))
                    .await
                    .map_err(|e| HaError::Unreachable(format!("ws subscribe: {e}")))?;
            }
            "auth_invalid" => {
                return Err(HaError::Api {
                    status: 401,
                    body: text,
                });
            }
            "event" => {
                if let Some(new_state) = v
                    .pointer("/event/data/new_state")
                    .cloned()
                    .filter(|s| !s.is_null())
                {
                    if let Ok(state) = serde_json::from_value::<HaState>(new_state) {
                        let entity = HaAdapter::state_to_entity(&state);
                        events.device_state_changed(&entity);
                        registry.upsert(entity).await;
                    }
                }
            }
            "result" => {
                if v.get("success") == Some(&json!(true)) {
                    tracing::debug!(id = ?v.get("id"), "HA WS command ok");
                } else {
                    tracing::warn!(payload = %text, "HA WS command failed");
                }
            }
            _ => {}
        }
    }

    Ok(())
}
