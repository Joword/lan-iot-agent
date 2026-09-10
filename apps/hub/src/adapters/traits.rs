//! Southbound device adapter contract.
//!
//! Hub northbound APIs (REST / WS / MCP) talk only to [`AdapterRouter`], which
//! routes by `DeviceEntity.source` to a concrete [`DeviceAdapter`]
//! (`ha`, `faker`, future `z2m` / `matter`, …).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;

use crate::registry::DeviceEntity;

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("adapter '{0}' is not configured")]
    NotConfigured(String),
    #[error("adapter unreachable: {0}")]
    Unreachable(String),
    #[error("adapter API error ({status}): {body}")]
    Api { status: u16, body: String },
    #[error("entity not found: {0}")]
    NotFound(String),
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error("no adapter registered for source '{0}'")]
    NoAdapter(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterHealth {
    pub source: String,
    pub configured: bool,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Pluggable southbound backend for device list / state / control.
#[async_trait]
pub trait DeviceAdapter: Send + Sync {
    /// Stable source key written onto `DeviceEntity.source` (`ha`, `faker`, …).
    fn source_id(&self) -> &str;

    /// Whether this backend is configured (credentials / URL present). Default true.
    fn is_configured(&self) -> bool {
        true
    }

    async fn health(&self) -> AdapterHealth;

    /// Pull entities from the backend (HA sync). Faker may return empty —
    /// seeding is handled at boot via the registry.
    async fn sync(&self) -> Result<Vec<DeviceEntity>, AdapterError>;

    async fn get_state(&self, entity_id: &str) -> Result<DeviceEntity, AdapterError>;

    async fn control(
        &self,
        entity_id: &str,
        action: &str,
        params: &HashMap<String, Value>,
    ) -> Result<Value, AdapterError>;
}
