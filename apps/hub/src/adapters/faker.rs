//! In-memory faker brand adapter — offline / no-HA southbound backend.
//!
//! Device catalogue is seeded into [`DeviceRegistry`] at boot; this adapter
//! only implements control + get_state against those stubs.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use super::traits::{AdapterError, AdapterHealth, DeviceAdapter};
use crate::registry::{DeviceEntity, DeviceRegistry};

pub struct FakerAdapter {
    registry: Arc<DeviceRegistry>,
}

impl FakerAdapter {
    pub fn new(registry: Arc<DeviceRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl DeviceAdapter for FakerAdapter {
    fn source_id(&self) -> &str {
        "faker"
    }

    async fn health(&self) -> AdapterHealth {
        let n = self
            .registry
            .list()
            .await
            .into_iter()
            .filter(|e| e.source == "faker" || e.is_faker)
            .count();
        AdapterHealth {
            source: "faker".into(),
            configured: true,
            ok: true,
            detail: Some(format!("{n} faker entities in registry")),
        }
    }

    async fn sync(&self) -> Result<Vec<DeviceEntity>, AdapterError> {
        // Seeding is done at boot via DeviceRegistry::seed_faker_catalog.
        Ok(self
            .registry
            .list()
            .await
            .into_iter()
            .filter(|e| e.source == "faker" || e.is_faker)
            .collect())
    }

    async fn get_state(&self, entity_id: &str) -> Result<DeviceEntity, AdapterError> {
        let entity = self
            .registry
            .get(entity_id)
            .await
            .ok_or_else(|| AdapterError::NotFound(entity_id.to_string()))?;
        if entity.source != "faker" && !entity.is_faker {
            return Err(AdapterError::Invalid(format!(
                "entity '{entity_id}' is not a faker stub (source={})",
                entity.source
            )));
        }
        Ok(entity)
    }

    async fn control(
        &self,
        entity_id: &str,
        action: &str,
        params: &HashMap<String, Value>,
    ) -> Result<Value, AdapterError> {
        match self
            .registry
            .apply_faker_action(entity_id, action, params)
            .await
        {
            Ok(entity) => Ok(json!({
                "faker": true,
                "state": entity.state,
                "attributes": entity.attributes,
                "capabilities": entity.capabilities,
            })),
            Err(msg) => Err(AdapterError::Invalid(msg)),
        }
    }
}
