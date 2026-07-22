//! Device Registry — in-memory aggregation of HA entities (+ Companion later).

mod brand;
mod capability;
mod faker;

pub use brand::infer_brand_meta;
pub use capability::{derive_capabilities, Capability};
pub use faker::should_seed_faker;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    Light,
    Climate,
    Switch,
    Sensor,
    BinarySensor,
    Cover,
    Fan,
    Other,
}

impl EntityType {
    pub fn from_entity_id(entity_id: &str) -> Self {
        match entity_id.split_once('.').map(|(d, _)| d).unwrap_or("") {
            "light" => Self::Light,
            "climate" => Self::Climate,
            "switch" => Self::Switch,
            "sensor" => Self::Sensor,
            "binary_sensor" => Self::BinarySensor,
            "cover" => Self::Cover,
            "fan" => Self::Fan,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceEntity {
    pub entity_id: String,
    pub entity_type: EntityType,
    pub friendly_name: String,
    pub state: String,
    pub attributes: HashMap<String, Value>,
    pub available: bool,
    /// Origin of this entity (`ha` | `faker` | …).
    pub source: String,
    /// Inferred brand label (`xiaomi`, `gree`, `esp32`, …). None if unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brand: Option<String>,
    /// True for stub / MQTT demo gear (`faker_*` / `demo_*`). Swap with real HA entities later.
    #[serde(default)]
    pub is_faker: bool,
    /// Machine-readable capabilities for Agent / MCP (derived on upsert).
    #[serde(default)]
    pub capabilities: Vec<Capability>,
}

impl DeviceEntity {
    /// Build a registry entity and fill brand / faker / capability metadata.
    pub fn new(
        entity_id: impl Into<String>,
        entity_type: EntityType,
        friendly_name: impl Into<String>,
        state: impl Into<String>,
        attributes: HashMap<String, Value>,
        available: bool,
        source: impl Into<String>,
    ) -> Self {
        let entity_id = entity_id.into();
        let friendly_name = friendly_name.into();
        let (brand, is_faker) = infer_brand_meta(&entity_id, &friendly_name);
        let capabilities = derive_capabilities(&entity_type, &attributes);
        Self {
            entity_id,
            entity_type,
            friendly_name,
            state: state.into(),
            attributes,
            available,
            source: source.into(),
            brand,
            is_faker,
            capabilities,
        }
    }

    /// Recompute capabilities from current attributes (after faker mutation).
    pub fn refresh_capabilities(&mut self) {
        self.capabilities = derive_capabilities(&self.entity_type, &self.attributes);
    }
}

pub struct DeviceRegistry {
    /// Keyed by entity_id.
    entities: RwLock<HashMap<String, DeviceEntity>>,
}

impl DeviceRegistry {
    pub fn new() -> Self {
        Self {
            entities: RwLock::new(HashMap::new()),
        }
    }

    pub async fn list(&self) -> Vec<DeviceEntity> {
        let guard = self.entities.read().await;
        let mut list: Vec<_> = guard.values().cloned().collect();
        list.sort_by(|a, b| a.entity_id.cmp(&b.entity_id));
        list
    }

    /// Controllable / interesting domains for the Hub device API (filters noise).
    pub async fn list_devices(&self) -> Vec<DeviceEntity> {
        self.list()
            .await
            .into_iter()
            .filter(|e| {
                matches!(
                    e.entity_type,
                    EntityType::Light
                        | EntityType::Climate
                        | EntityType::Switch
                        | EntityType::Cover
                        | EntityType::Fan
                        | EntityType::Sensor
                        | EntityType::BinarySensor
                )
            })
            .collect()
    }

    pub async fn get(&self, entity_id: &str) -> Option<DeviceEntity> {
        self.entities.read().await.get(entity_id).cloned()
    }

    pub async fn upsert(&self, entity: DeviceEntity) {
        self.entities
            .write()
            .await
            .insert(entity.entity_id.clone(), entity);
    }

    /// Replace all HA-sourced entities; keep non-HA (e.g. companion) entries.
    pub async fn replace_ha_entities(&self, entities: Vec<DeviceEntity>) {
        let mut guard = self.entities.write().await;
        guard.retain(|_, e| e.source != "ha");
        for entity in entities {
            guard.insert(entity.entity_id.clone(), entity);
        }
    }

    pub async fn len(&self) -> usize {
        self.entities.read().await.len()
    }
}

impl Default for DeviceRegistry {
    fn default() -> Self {
        Self::new()
    }
}
