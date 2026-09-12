//! MCP Server stub — JSON-RPC over HTTP for the Python Agent.
//!
//! # Endpoint shape (Agent integration)
//!
//! Base URL: `POST /mcp` (also `GET /mcp` for a short discovery doc).
//!
//! Hub speaks a pragmatic JSON-RPC 2.0 subset (not full MCP streamable-HTTP).
//! Agent can call tools without a full MCP SDK:
//!
//! ```json
//! // List tools
//! {"jsonrpc":"2.0","id":1,"method":"tools/list"}
//!
//! // Call a tool (MCP-style)
//! {"jsonrpc":"2.0","id":2,"method":"tools/call",
//!  "params":{"name":"devices.list","arguments":{"type":"light"}}}
//!
//! // Shorthand: method == tool name
//! {"jsonrpc":"2.0","id":3,"method":"devices.control",
//!  "params":{"entity_id":"light.demo","action":"turn_on","params":{}}}
//! ```
//!
//! Convenience (same handlers, no JSON-RPC envelope):
//! - `GET  /mcp/tools` → tool catalog
//! - `POST /mcp/call`  → `{"name":"devices.list","arguments":{...}}`
//!
//! Tool results: `{"jsonrpc":"2.0","id":…,"result":{"ok":true,"tool":"…","data":{…}}}`
//! Errors: `{"jsonrpc":"2.0","id":…,"error":{"code":-32000,"message":"…"}}`
//!
//! Tools route through Device Registry + HA adapter (no duplicated HA logic).
//! Companion tools use the CompanionAdapter registry (Hub HTTP only; Mongo-backed when available).

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use crate::adapters::AdapterError;
use crate::registry::{Capability, EntityType};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/mcp", get(mcp_info).post(mcp_jsonrpc))
        .route("/mcp/tools", get(list_tools_http))
        .route("/mcp/call", post(call_tool_http))
}

/// GET /mcp — human/agent discovery blob.
async fn mcp_info() -> Json<Value> {
    Json(json!({
        "service": "lan-iot-hub-mcp",
        "transport": "json-rpc-http",
        "endpoints": {
            "rpc": "POST /mcp",
            "tools": "GET /mcp/tools",
            "call": "POST /mcp/call"
        },
        "methods": ["initialize", "tools/list", "tools/call", "ping"],
        "tools": tool_catalog(),
        "note": "Full MCP streamable-HTTP not required; Agent may POST tools/call or /mcp/call"
    }))
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[serde(default = "default_jsonrpc")]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

fn default_jsonrpc() -> String {
    "2.0".into()
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct CallBody {
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

/// POST /mcp — JSON-RPC entry.
async fn mcp_jsonrpc(
    State(state): State<Arc<AppState>>,
    Json(req): Json<JsonRpcRequest>,
) -> (StatusCode, Json<JsonRpcResponse>) {
    let id = req.id.clone();
    if !req.jsonrpc.is_empty() && req.jsonrpc != "2.0" {
        return (
            StatusCode::OK,
            Json(JsonRpcResponse {
                jsonrpc: "2.0",
                id,
                result: None,
                error: Some(rpc_err(
                    -32600,
                    format!("unsupported jsonrpc version: {}", req.jsonrpc),
                    None,
                )),
            }),
        );
    }
    match dispatch_rpc(&state, &req.method, req.params).await {
        Ok(result) => (
            StatusCode::OK,
            Json(JsonRpcResponse {
                jsonrpc: "2.0",
                id,
                result: Some(result),
                error: None,
            }),
        ),
        Err(err) => (
            StatusCode::OK, // JSON-RPC errors still use HTTP 200
            Json(JsonRpcResponse {
                jsonrpc: "2.0",
                id,
                result: None,
                error: Some(err),
            }),
        ),
    }
}

async fn list_tools_http() -> Json<Value> {
    Json(json!({ "tools": tool_catalog() }))
}

async fn call_tool_http(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CallBody>,
) -> (StatusCode, Json<Value>) {
    match call_tool(&state, &body.name, body.arguments).await {
        Ok(data) => (
            StatusCode::OK,
            Json(json!({ "ok": true, "tool": body.name, "data": data })),
        ),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "tool": body.name,
                "error": err.message,
                "data": err.data,
            })),
        ),
    }
}

async fn dispatch_rpc(
    state: &AppState,
    method: &str,
    params: Value,
) -> Result<Value, JsonRpcError> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "serverInfo": { "name": "lan-iot-hub", "version": "0.1.0" },
            "capabilities": { "tools": {} },
            "instructions": "Call tools via tools/call or shorthand method=tool name"
        })),
        "ping" => Ok(json!({ "ok": true })),
        "tools/list" => Ok(json!({ "tools": tool_catalog() })),
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| rpc_err(-32602, "params.name required", None))?;
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            let data = call_tool(state, name, arguments).await?;
            Ok(json!({
                "ok": true,
                "tool": name,
                "data": data,
                // MCP-ish content block for clients that expect it:
                "content": [{ "type": "text", "text": data.to_string() }]
            }))
        }
        // Shorthand: treat unknown method as tool name.
        other if other.contains('.') => {
            let arguments = match params {
                Value::Object(map) => {
                    // tools/call uses {name, arguments}; shorthand uses args as params.
                    if map.contains_key("arguments") {
                        map.get("arguments").cloned().unwrap_or(json!({}))
                    } else {
                        Value::Object(map)
                    }
                }
                other => other,
            };
            let data = call_tool(state, other, arguments).await?;
            Ok(json!({ "ok": true, "tool": other, "data": data }))
        }
        other => Err(rpc_err(
            -32601,
            format!("method not found: {other}"),
            None,
        )),
    }
}

fn tool_catalog() -> Vec<Value> {
    vec![
        tool_def(
            "devices.list",
            "List devices from Hub registry (any southbound adapter). Optional filters: type, room. Each device includes capabilities.",
            json!({
                "type": "object",
                "properties": {
                    "type": { "type": "string", "description": "light|climate|switch|sensor|…" },
                    "room": { "type": "string", "description": "Substring match on friendly_name" }
                }
            }),
        ),
        tool_def(
            "devices.get_state",
            "Get one device/entity state by entity_id (routed by entity.source).",
            json!({
                "type": "object",
                "required": ["entity_id"],
                "properties": {
                    "entity_id": { "type": "string" }
                }
            }),
        ),
        tool_def(
            "devices.describe",
            "Describe a device's machine-readable capabilities and allowed actions/params for Agent planning.",
            json!({
                "type": "object",
                "required": ["entity_id"],
                "properties": {
                    "entity_id": { "type": "string" }
                }
            }),
        ),
        tool_def(
            "devices.control",
            "Control a device via its southbound adapter (turn_on, turn_off, set_temperature, …). Routes by entity.source (ha|faker|…).",
            json!({
                "type": "object",
                "required": ["entity_id", "action"],
                "properties": {
                    "entity_id": { "type": "string" },
                    "action": { "type": "string" },
                    "params": { "type": "object" }
                }
            }),
        ),
        tool_def(
            "climate.set",
            "Thin wrapper: set climate temperature (and optional HVAC mode).",
            json!({
                "type": "object",
                "required": ["entity_id", "temperature"],
                "properties": {
                    "entity_id": { "type": "string" },
                    "temperature": { "type": "number" },
                    "mode": { "type": "string" }
                }
            }),
        ),
        tool_def(
            "lights.control",
            "Thin wrapper: turn light on/off with optional brightness.",
            json!({
                "type": "object",
                "required": ["entity_id", "on"],
                "properties": {
                    "entity_id": { "type": "string" },
                    "on": { "type": "boolean" },
                    "brightness": { "type": "integer", "minimum": 0, "maximum": 255 }
                }
            }),
        ),
        tool_def(
            "scenes.run",
            "Run a Hub scene by id (sequential device actions). Missing entities are skipped.",
            json!({
                "type": "object",
                "required": ["scene_id"],
                "properties": {
                    "scene_id": {
                        "type": "string",
                        "description": "Scene id, e.g. sleep_mode or away_mode"
                    }
                }
            }),
        ),
        tool_def(
            "companion.command",
            "Send a command to a Companion device (phone/PC/robot) via Hub HTTP. Demo: companion.demo_pc may be offline.",
            json!({
                "type": "object",
                "required": ["device_id", "command"],
                "properties": {
                    "device_id": {
                        "type": "string",
                        "description": "Companion id, e.g. companion.demo_pc"
                    },
                    "command": {
                        "type": "string",
                        "description": "Command string forwarded to Companion POST /command (ping, notify, lock, stop, dock, start, …)"
                    },
                    "title": {
                        "type": "string",
                        "description": "Optional notify title"
                    },
                    "body": {
                        "type": "string",
                        "description": "Optional notify body / message"
                    }
                }
            }),
        ),
    ]
}

fn tool_def(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema
    })
}

fn rpc_err(code: i32, message: impl Into<String>, data: Option<Value>) -> JsonRpcError {
    JsonRpcError {
        code,
        message: message.into(),
        data,
    }
}

async fn call_tool(state: &AppState, name: &str, arguments: Value) -> Result<Value, JsonRpcError> {
    match name {
        "devices.list" => tool_devices_list(state, &arguments).await,
        "devices.get_state" => tool_devices_get_state(state, &arguments).await,
        "devices.describe" => tool_devices_describe(state, &arguments).await,
        "devices.control" => tool_devices_control(state, &arguments).await,
        "climate.set" => tool_climate_set(state, &arguments).await,
        "lights.control" => tool_lights_control(state, &arguments).await,
        "scenes.run" => tool_scenes_run(state, &arguments).await,
        "companion.command" => tool_companion_command(state, &arguments).await,
        other => Err(rpc_err(-32601, format!("unknown tool: {other}"), None)),
    }
}

async fn tool_devices_list(state: &AppState, args: &Value) -> Result<Value, JsonRpcError> {
    if state.registry.len().await == 0 && state.adapters.is_configured("ha") {
        if let Err(e) = state.adapters.sync_into("ha", &state.registry).await {
            tracing::warn!(error = %e, "MCP devices.list: HA sync failed");
            return Ok(json!({
                "devices": [],
                "ha_available": false,
                "warning": e.to_string()
            }));
        }
    }

    let type_filter = args.get("type").and_then(|v| v.as_str());
    let room_filter = args
        .get("room")
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase());

    let mut devices = state.registry.list_devices().await;
    if let Some(t) = type_filter {
        let want = parse_entity_type(t);
        devices.retain(|d| match &want {
            Some(et) => &d.entity_type == et,
            None => d
                .entity_id
                .split_once('.')
                .map(|(dom, _)| dom.eq_ignore_ascii_case(t))
                .unwrap_or(false),
        });
    }
    if let Some(ref room) = room_filter {
        devices.retain(|d| d.friendly_name.to_lowercase().contains(room));
    }

    let ha_available = matches!(
        *state.ha_status.read().await,
        crate::adapters::ha::HaConnectionStatus::Connected
    ) || (!devices.is_empty() && state.adapters.is_configured("ha"));

    Ok(json!({
        "devices": devices,
        "ha_available": ha_available,
        "count": devices.len()
    }))
}

async fn tool_devices_get_state(state: &AppState, args: &Value) -> Result<Value, JsonRpcError> {
    let entity_id = args
        .get("entity_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| rpc_err(-32602, "entity_id required", None))?;

    match state.adapters.get_state(&state.registry, entity_id).await {
        Ok(entity) => Ok(serde_json::to_value(entity).unwrap_or(json!({}))),
        Err(e) => Err(adapter_to_rpc(e)),
    }
}

async fn tool_devices_describe(state: &AppState, args: &Value) -> Result<Value, JsonRpcError> {
    let entity_id = args
        .get("entity_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| rpc_err(-32602, "entity_id required", None))?;

    let entity = state
        .adapters
        .get_state(&state.registry, entity_id)
        .await
        .map_err(adapter_to_rpc)?;

    Ok(json!({
        "entity_id": entity.entity_id,
        "friendly_name": entity.friendly_name,
        "entity_type": entity.entity_type,
        "source": entity.source,
        "brand": entity.brand,
        "state": entity.state,
        "available": entity.available,
        "is_faker": entity.is_faker,
        "summary": Capability::summarize(&entity.capabilities),
        "capabilities": entity.capabilities,
        "adapters": state.adapters.sources(),
    }))
}

async fn tool_devices_control(state: &AppState, args: &Value) -> Result<Value, JsonRpcError> {
    let entity_id = args
        .get("entity_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| rpc_err(-32602, "entity_id required", None))?;
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or_else(|| rpc_err(-32602, "action required", None))?;
    let params = value_to_params(args.get("params").cloned().unwrap_or(json!({})));

    perform_and_refresh(state, entity_id, action, &params).await
}

async fn tool_climate_set(state: &AppState, args: &Value) -> Result<Value, JsonRpcError> {
    let entity_id = args
        .get("entity_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| rpc_err(-32602, "entity_id required", None))?;
    let temperature = args
        .get("temperature")
        .cloned()
        .ok_or_else(|| rpc_err(-32602, "temperature required", None))?;

    let mut params = HashMap::new();
    params.insert("temperature".into(), temperature);
    let mut result =
        perform_and_refresh(state, entity_id, "set_temperature", &params).await?;

    if let Some(mode) = args.get("mode").cloned() {
        let mut mode_params = HashMap::new();
        mode_params.insert("mode".into(), mode);
        let mode_result =
            perform_and_refresh(state, entity_id, "set_hvac_mode", &mode_params).await?;
        if let Some(obj) = result.as_object_mut() {
            obj.insert("hvac_mode_result".into(), mode_result);
        }
    }

    Ok(result)
}

async fn tool_lights_control(state: &AppState, args: &Value) -> Result<Value, JsonRpcError> {
    let entity_id = args
        .get("entity_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| rpc_err(-32602, "entity_id required", None))?;
    let on = args
        .get("on")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| rpc_err(-32602, "on (boolean) required", None))?;

    let mut params = HashMap::new();
    if let Some(b) = args.get("brightness").cloned() {
        params.insert("brightness".into(), b);
        if on {
            return perform_and_refresh(state, entity_id, "set_brightness", &params).await;
        }
    }

    let action = if on { "turn_on" } else { "turn_off" };
    perform_and_refresh(state, entity_id, action, &params).await
}

async fn tool_scenes_run(state: &AppState, args: &Value) -> Result<Value, JsonRpcError> {
    let scene_id = args
        .get("scene_id")
        .and_then(|v| v.as_str())
        .or_else(|| args.get("id").and_then(|v| v.as_str()))
        .ok_or_else(|| rpc_err(-32602, "scene_id required", None))?;

    match state
        .scenes
        .run(scene_id, &state.adapters, &state.registry, &state.events)
        .await
    {
        Ok(result) => Ok(serde_json::to_value(result).unwrap_or(json!({}))),
        Err(crate::scene::SceneError::NotFound(id)) => Err(rpc_err(
            -32004,
            format!("scene not found: {id}"),
            None,
        )),
        Err(crate::scene::SceneError::Unavailable) => Err(rpc_err(
            -32003,
            "scene store unavailable (MongoDB)",
            None,
        )),
        Err(crate::scene::SceneError::Invalid(msg)) => {
            Err(rpc_err(-32602, format!("invalid scene: {msg}"), None))
        }
    }
}

async fn tool_companion_command(state: &AppState, args: &Value) -> Result<Value, JsonRpcError> {
    let device_id = args
        .get("device_id")
        .and_then(|v| v.as_str())
        .or_else(|| args.get("id").and_then(|v| v.as_str()))
        .ok_or_else(|| rpc_err(-32602, "device_id required", None))?;
    let command = args
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| rpc_err(-32602, "command required", None))?;

    match state
        .companions
        .send_command(device_id, command, Some(args))
        .await {
        Ok(result) => Ok(serde_json::to_value(result).unwrap_or(json!({}))),
        Err(crate::adapters::companion::CompanionError::NotFound(id)) => Err(rpc_err(
            -32004,
            format!("companion not found: {id}"),
            None,
        )),
        // Graceful: demo Companion may be offline — return structured failure, not a hard crash.
        Err(crate::adapters::companion::CompanionError::Unreachable(msg)) => Ok(json!({
            "ok": false,
            "device_id": device_id,
            "command": command,
            "companion_available": false,
            "error": format!("companion unreachable: {msg}"),
        })),
        Err(crate::adapters::companion::CompanionError::Api { status, body }) => Ok(json!({
            "ok": false,
            "device_id": device_id,
            "command": command,
            "companion_available": true,
            "error": format!("companion API {status}: {body}"),
        })),
        Err(crate::adapters::companion::CompanionError::Invalid(msg)) => Err(rpc_err(
            -32602,
            format!("invalid companion: {msg}"),
            None,
        )),
        Err(crate::adapters::companion::CompanionError::Unavailable) => Err(rpc_err(
            -32003,
            "companion store unavailable (MongoDB)",
            None,
        )),
    }
}

async fn perform_and_refresh(
    state: &AppState,
    entity_id: &str,
    action: &str,
    params: &HashMap<String, Value>,
) -> Result<Value, JsonRpcError> {
    match state
        .adapters
        .control(&state.registry, &state.events, entity_id, action, params)
        .await
    {
        Ok(outcome) => Ok(outcome.to_json()),
        Err(e) => Err(adapter_to_rpc(e)),
    }
}

fn adapter_to_rpc(err: AdapterError) -> JsonRpcError {
    match err {
        AdapterError::NotConfigured(src) => {
            rpc_err(-32002, format!("{src} adapter not configured"), None)
        }
        AdapterError::Unreachable(msg) => rpc_err(-32003, format!("adapter unreachable: {msg}"), None),
        AdapterError::Api { status, body } => rpc_err(
            -32003,
            format!("adapter API {status}"),
            Some(json!({ "body": body, "status": status })),
        ),
        AdapterError::NotFound(id) => rpc_err(-32004, format!("device not found: {id}"), None),
        AdapterError::Invalid(msg) => rpc_err(-32602, msg, None),
        AdapterError::NoAdapter(msg) => rpc_err(-32002, msg, None),
    }
}

fn value_to_params(v: Value) -> HashMap<String, Value> {
    match v {
        Value::Object(map) => map.into_iter().collect(),
        _ => HashMap::new(),
    }
}

fn parse_entity_type(s: &str) -> Option<EntityType> {
    match s.to_ascii_lowercase().as_str() {
        "light" | "lights" => Some(EntityType::Light),
        "climate" => Some(EntityType::Climate),
        "switch" | "switches" => Some(EntityType::Switch),
        "sensor" | "sensors" => Some(EntityType::Sensor),
        "binary_sensor" => Some(EntityType::BinarySensor),
        "cover" => Some(EntityType::Cover),
        "fan" => Some(EntityType::Fan),
        "other" => Some(EntityType::Other),
        _ => None,
    }
}

/// Kept for potential internal use / tests.
#[allow(dead_code)]
pub struct McpServer;

#[allow(dead_code)]
impl McpServer {
    pub fn new() -> Self {
        Self
    }
}

#[allow(dead_code)]
impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}