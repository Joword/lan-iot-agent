//! Hub client WebSocket — UI / Agent-facing realtime channel.
//!
//! Endpoint: `GET /api/v1/ws` (upgrade) — also mounted at `/ws`.
//! Optional auth: `?token=lit_…` when `AUTH_REQUIRED=true`.
//!
//! ## Client → Hub frames
//! ```json
//! {"type":"device:command","entity_id":"light.demo","action":"turn_on","params":{}}
//! {"type":"agent:message","content":"客厅有点热","confirm":false,"pending_action":null,"context":{…}}
//! {"type":"ping"}
//! ```
//!
//! ## Hub → Client frames
//! ```json
//! {"type":"device:state_changed","device":{…}}
//! {"type":"agent:stream","reply":"…","status":"ok", …}
//! {"type":"device:command_result","ok":true,"entity_id":"…","action":"…"}
//! {"type":"error","code":"agent_unreachable","message":"…"}
//! {"type":"pong"}
//! {"type":"hello","service":"hub"}
//! ```

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use crate::adapters::agent::{AgentError, ChatOptions};
use crate::adapters::AdapterError;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/ws", get(ws_upgrade))
        .route("/ws", get(ws_upgrade))
}

#[derive(Debug, Deserialize, Default)]
struct WsQuery {
    /// Bearer token when AUTH_REQUIRED (browsers cannot set WS Authorization easily).
    #[serde(default)]
    token: Option<String>,
}

async fn ws_upgrade(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    if state.config.auth_required {
        let token = query
            .token
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty());
        let allowed = match token {
            Some(t) if state.auth.available() => state.auth.validate_token(t).await.unwrap_or(false),
            Some(_) => false,
            None => false,
        };
        if !allowed {
            return (
                axum::http::StatusCode::UNAUTHORIZED,
                "Authorization required: connect with ?token=<lit_…>",
            )
                .into_response();
        }
    }
    ws.on_upgrade(move |socket| handle_socket(socket, state))
        .into_response()
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sink, mut stream) = socket.split();
    let mut events = state.events.subscribe();

    // Greeting so clients know the channel is live.
    if sink
        .send(Message::Text(
            json!({
                "type": "hello",
                "service": "hub",
                "ha_configured": state.ha.is_configured(),
                "agent_url": state.config.agent_url,
                "auth_required": state.config.auth_required,
            })
            .to_string()
            .into(),
        ))
        .await
        .is_err()
    {
        return;
    }

    loop {
        tokio::select! {
            biased;

            evt = events.recv() => {
                match evt {
                    Ok(v) => {
                        if sink
                            .send(Message::Text(v.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        let warn = json!({
                            "type": "error",
                            "code": "event_lagged",
                            "message": format!("dropped {n} events"),
                        });
                        if sink
                            .send(Message::Text(warn.to_string().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }

            msg = stream.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let replies = handle_client_text(&state, &text).await;
                        for reply in replies {
                            if sink
                                .send(Message::Text(reply.to_string().into()))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                    Some(Ok(Message::Ping(p))) => {
                        if sink.send(Message::Pong(p)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        tracing::debug!(error = %e, "client websocket error");
                        break;
                    }
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct ClientFrame {
    #[serde(rename = "type")]
    typ: String,
    #[serde(default)]
    entity_id: Option<String>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    params: HashMap<String, Value>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    context: Option<Value>,
    /// P5 danger-confirm: re-send after requires_confirmation.
    #[serde(default)]
    confirm: bool,
    #[serde(default)]
    pending_action: Option<String>,
}

async fn handle_client_text(state: &AppState, text: &str) -> Vec<Value> {
    let frame: ClientFrame = match serde_json::from_str(text) {
        Ok(f) => f,
        Err(e) => {
            return vec![json!({
                "type": "error",
                "code": "bad_frame",
                "message": format!("invalid JSON frame: {e}"),
            })];
        }
    };

    match frame.typ.as_str() {
        "ping" => vec![json!({ "type": "pong" })],
        "device:command" => handle_device_command(state, &frame).await,
        "agent:message" => handle_agent_message(state, &frame).await,
        other => vec![json!({
            "type": "error",
            "code": "unknown_type",
            "message": format!("unsupported frame type: {other}"),
        })],
    }
}

async fn handle_device_command(state: &AppState, frame: &ClientFrame) -> Vec<Value> {
    let entity_id = match frame.entity_id.as_deref() {
        Some(id) if !id.is_empty() => id,
        _ => {
            return vec![json!({
                "type": "error",
                "code": "missing_entity_id",
                "message": "device:command requires entity_id",
            })];
        }
    };
    let action = match frame.action.as_deref() {
        Some(a) if !a.is_empty() => a,
        _ => {
            return vec![json!({
                "type": "error",
                "code": "missing_action",
                "message": "device:command requires action",
            })];
        }
    };

    match state
        .adapters
        .control(
            &state.registry,
            &state.events,
            entity_id,
            action,
            &frame.params,
        )
        .await
    {
        Ok(outcome) => {
            let mut body = json!({
                "type": "device:command_result",
                "ok": true,
                "entity_id": entity_id,
                "action": action,
                "source": outcome.source,
                "result": outcome.result,
            });
            if outcome.source == "faker" {
                body["faker"] = json!(true);
            }
            if let Some(err) = outcome.degraded_from {
                body["ha_error"] = json!(err);
                body["degraded"] = json!(true);
            }
            vec![body]
        }
        Err(AdapterError::NotConfigured(src)) => vec![json!({
            "type": "error",
            "code": "adapter_not_configured",
            "message": format!("{src} not configured — set HA_URL/HA_TOKEN or use faker entity_ids"),
        })],
        Err(AdapterError::Unreachable(msg)) => vec![json!({
            "type": "error",
            "code": "adapter_unreachable",
            "message": msg,
        })],
        Err(AdapterError::NotFound(id)) => vec![json!({
            "type": "error",
            "code": "device_not_found",
            "message": format!("entity '{id}' not found"),
        })],
        Err(AdapterError::Invalid(msg)) => vec![json!({
            "type": "error",
            "code": "invalid_action",
            "message": msg,
        })],
        Err(e) => vec![json!({
            "type": "error",
            "code": "adapter_error",
            "message": e.to_string(),
        })],
    }
}

async fn handle_agent_message(state: &AppState, frame: &ClientFrame) -> Vec<Value> {
    let content = frame
        .content
        .as_deref()
        .or(frame.message.as_deref())
        .unwrap_or("")
        .trim();
    if content.is_empty() {
        return vec![json!({
            "type": "error",
            "code": "empty_message",
            "message": "agent:message requires content (or message)",
        })];
    }

    // Enrich context with registry snapshot when client omitted devices.
    let mut context = match &frame.context {
        Some(Value::Object(map)) if map.contains_key("devices") => frame.context.clone(),
        Some(other) => {
            let devices = state.registry.list_devices().await;
            let scenes = state.scenes.list_ids().await;
            Some(json!({
                "devices": devices,
                "scenes": scenes,
                "extra": other,
            }))
        }
        None => {
            let devices = state.registry.list_devices().await;
            let scenes = state.scenes.list_ids().await;
            Some(json!({ "devices": devices, "scenes": scenes }))
        }
    };

    // Pull conversation_history from frame.context if present for ChatOptions.
    let history = frame
        .context
        .as_ref()
        .and_then(|c| c.get("conversation_history"))
        .cloned();
    if let (Some(hist), Some(Value::Object(ref mut map))) = (history.clone(), context.as_mut()) {
        map.insert("conversation_history".into(), hist);
    }

    let options = ChatOptions {
        confirm: frame.confirm,
        pending_action: frame.pending_action.clone(),
        conversation_history: history,
    };

    match state.agent.chat(content, context, options).await {
        Ok(resp) => {
            let reply = resp.get("reply").cloned().unwrap_or_else(|| json!(""));
            vec![json!({
                "type": "agent:stream",
                "reply": reply,
                "status": resp.get("status").cloned().unwrap_or_else(|| json!("ok")),
                "errors": resp.get("errors").cloned().unwrap_or_else(|| json!([])),
                "used_llm": resp.get("used_llm").cloned().unwrap_or_else(|| json!(false)),
                "used_tools": resp.get("used_tools").cloned().unwrap_or_else(|| json!(false)),
                "meta": resp.get("meta").cloned().unwrap_or_else(|| json!({})),
                "requires_confirmation": resp.get("requires_confirmation").cloned().unwrap_or_else(|| json!(false)),
                "pending_action": resp.get("pending_action").cloned().unwrap_or(Value::Null),
            })]
        }
        Err(AgentError::NotConfigured) => vec![json!({
            "type": "error",
            "code": "agent_not_configured",
            "message": "AGENT_URL is empty",
        })],
        Err(AgentError::Unreachable(msg)) => vec![json!({
            "type": "error",
            "code": "agent_unreachable",
            "message": msg,
        })],
        Err(AgentError::Api { status, body }) => vec![json!({
            "type": "error",
            "code": "agent_error",
            "message": format!("Agent HTTP {status}"),
            "detail": body,
        })],
    }
}
