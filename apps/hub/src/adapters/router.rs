//! Route device control / state by `DeviceEntity.source`.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use super::traits::{AdapterError, AdapterHealth, DeviceAdapter};
use crate::events::EventBus;
use crate::registry::{DeviceEntity, DeviceRegistry};

/// Registry of southbound adapters keyed by `source_id`.
#[derive(Clone, Default)]
pub struct AdapterRouter {
    adapters: HashMap<String, Arc<dyn DeviceAdapter>>,
}

impl AdapterRouter {
    pub fn new() -> Self {
        Self {
            adapters: HashMap::new(),
        }
    }

    pub fn register(&mut self, adapter: Arc<dyn DeviceAdapter>) {
        let id = adapter.source_id().to_string();
        tracing::info!(source = %id, "registered device adapter");
        self.adapters.insert(id, adapter);
    }

    pub fn get(&self, source: &str) -> Option<Arc<dyn DeviceAdapter>> {
        self.adapters.get(source).cloned()
    }

    pub fn sources(&self) -> Vec<String> {
        let mut keys: Vec<_> = self.adapters.keys().cloned().collect();
        keys.sort();
        keys
    }

    pub async fn health_all(&self) -> Vec<AdapterHealth> {
        let mut out = Vec::new();
        for adapter in self.adapters.values() {
            out.push(adapter.health().await);
        }
        out.sort_by(|a, b| a.source.cmp(&b.source));
        out
    }

    /// Pick adapter for an entity_id using registry `source`, with fallbacks.
    pub async fn resolve(
        &self,
        registry: &DeviceRegistry,
        entity_id: &str,
    ) -> Result<(String, Arc<dyn DeviceAdapter>), AdapterError> {
        if let Some(entity) = registry.get(entity_id).await {
            let source = if entity.source == "faker" || entity.is_faker {
                "faker".to_string()
            } else {
                entity.source.clone()
            };
            if let Some(adapter) = self.get(&source) {
                return Ok((source, adapter));
            }
            // Unknown source on entity — try HA then faker.
        }

        if let Some(ha) = self.get("ha") {
            let h = ha.health().await;
            if h.configured {
                return Ok(("ha".into(), ha));
            }
        }
        if let Some(faker) = self.get("faker") {
            if registry
                .get(entity_id)
                .await
                .is_some_and(|e| e.source == "faker" || e.is_faker)
            {
                return Ok(("faker".into(), faker));
            }
        }

        Err(AdapterError::NoAdapter(format!(
            "no adapter for entity '{entity_id}' (known sources: {:?})",
            self.sources()
        )))
    }

    /// Control via resolved adapter; refresh registry + emit events on success.
    pub async fn control(
        &self,
        registry: &DeviceRegistry,
        events: &EventBus,
        entity_id: &str,
        action: &str,
        params: &HashMap<String, Value>,
    ) -> Result<ControlOutcome, AdapterError> {
        let (source, adapter) = self.resolve(registry, entity_id).await?;

        // Prefer faker when entity is faker-tagged even if HA is configured
        // (side-by-side demo), unless caller resolved to ha because entity is ha.
        let result = match adapter.control(entity_id, action, params).await {
            Ok(v) => v,
            Err(e) => {
                // HA down → degrade to faker stub with same id when present.
                if source == "ha" {
                    if let Some(faker) = self.get("faker") {
                        if registry
                            .get(entity_id)
                            .await
                            .is_some_and(|ent| ent.source == "faker" || ent.is_faker)
                        {
                            tracing::warn!(
                                error = %e,
                                entity_id,
                                "HA control failed — degrading to faker adapter"
                            );
                            let result = faker.control(entity_id, action, params).await?;
                            let device = registry.get(entity_id).await;
                            if let Some(ref d) = device {
                                events.device_state_changed(d);
                            }
                            return Ok(ControlOutcome {
                                ok: true,
                                source: "faker".into(),
                                entity_id: entity_id.to_string(),
                                action: action.to_string(),
                                result,
                                device,
                                degraded_from: Some(e.to_string()),
                            });
                        }
                    }
                }
                return Err(e);
            }
        };

        // Refresh state into registry when the adapter can provide it.
        let device = match adapter.get_state(entity_id).await {
            Ok(entity) => {
                events.device_state_changed(&entity);
                registry.upsert(entity.clone()).await;
                Some(entity)
            }
            Err(_) => {
                // Faker control already upserted; read cache.
                let cached = registry.get(entity_id).await;
                if let Some(ref d) = cached {
                    events.device_state_changed(d);
                }
                cached
            }
        };

        Ok(ControlOutcome {
            ok: true,
            source,
            entity_id: entity_id.to_string(),
            action: action.to_string(),
            result,
            device,
            degraded_from: None,
        })
    }

    /// Live get_state via adapter when possible, else registry cache.
    pub async fn get_state(
        &self,
        registry: &DeviceRegistry,
        entity_id: &str,
    ) -> Result<DeviceEntity, AdapterError> {
        match self.resolve(registry, entity_id).await {
            Ok((_source, adapter)) => match adapter.get_state(entity_id).await {
                Ok(entity) => {
                    registry.upsert(entity.clone()).await;
                    Ok(entity)
                }
                Err(AdapterError::NotFound(_)) => registry
                    .get(entity_id)
                    .await
                    .ok_or_else(|| AdapterError::NotFound(entity_id.to_string())),
                Err(e) => {
                    if let Some(cached) = registry.get(entity_id).await {
                        tracing::warn!(
                            error = %e,
                            entity_id,
                            "adapter get_state failed — serving registry cache"
                        );
                        Ok(cached)
                    } else {
                        Err(e)
                    }
                }
            },
            Err(_) => registry
                .get(entity_id)
                .await
                .ok_or_else(|| AdapterError::NotFound(entity_id.to_string())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ControlOutcome {
    pub ok: bool,
    pub source: String,
    pub entity_id: String,
    pub action: String,
    pub result: Value,
    pub device: Option<DeviceEntity>,
    pub degraded_from: Option<String>,
}

impl ControlOutcome {
    pub fn to_json(&self) -> Value {
        let mut body = json!({
            "ok": self.ok,
            "entity_id": self.entity_id,
            "action": self.action,
            "source": self.source,
            "result": self.result,
            "device": self.device,
        });
        if self.source == "faker" {
            body["faker"] = json!(true);
        }
        if let Some(ref d) = self.degraded_from {
            body["ha_error"] = json!(d);
            body["degraded"] = json!(true);
        }
        body
    }
}
