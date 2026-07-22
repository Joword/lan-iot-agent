//! In-memory faker brand devices for offline / no-HA development.
//!
//! Enabled when `SEED_FAKER_DEVICES=1` (or `true` / `yes` / `on`), or when HA
//! is not configured at boot. These stand in for real Xiaomi / Gree / ESP32
//! entities until HA integrations are wired — entity_ids use the `faker_` /
//! `demo_` convention so they are easy to find and replace.

use serde_json::{json, Map, Value};
use std::collections::HashMap;

use super::{DeviceEntity, DeviceRegistry, EntityType};

fn attrs(pairs: &[(&str, Value)]) -> HashMap<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

/// Canonical faker catalogue. Keep ids stable; swap later by deleting MQTT
/// stubs and letting real HA entities appear under production entity_ids.
pub fn faker_catalog() -> Vec<DeviceEntity> {
    vec![
        DeviceEntity::new(
            "light.demo_esp32_light",
            EntityType::Light,
            "ESP32 Light [faker]",
            "on",
            attrs(&[
                ("brightness", json!(180)),
                ("brand_note", json!("Replace with real ESP32 MQTT / firmware entity")),
            ]),
            true,
            "faker",
        ),
        DeviceEntity::new(
            "climate.demo_gree_ac",
            EntityType::Climate,
            "Gree AC [faker]",
            "cool",
            attrs(&[
                ("temperature", json!(26.0)),
                ("current_temperature", json!(28.5)),
                ("hvac_modes", json!(["off", "auto", "cool", "heat", "dry", "fan_only"])),
                ("brand_note", json!("Replace with HA gree integration entity")),
            ]),
            true,
            "faker",
        ),
        DeviceEntity::new(
            "light.faker_xiaomi_bulb",
            EntityType::Light,
            "Xiaomi Bulb [faker]",
            "off",
            attrs(&[
                ("brightness", json!(128)),
                ("brand_note", json!("Replace with xiaomi_miot / xiaomi_miio light entity")),
            ]),
            true,
            "faker",
        ),
        DeviceEntity::new(
            "switch.faker_xiaomi_plug",
            EntityType::Switch,
            "Xiaomi Plug [faker]",
            "on",
            attrs(&[
                ("brand_note", json!("Replace with xiaomi_miot switch / outlet entity")),
            ]),
            true,
            "faker",
        ),
        DeviceEntity::new(
            "sensor.demo_esp32_temperature",
            EntityType::Sensor,
            "ESP32 Temperature [faker]",
            "26.5",
            attrs(&[
                ("unit_of_measurement", json!("°C")),
                ("device_class", json!("temperature")),
                ("brand_note", json!("Replace with real ESP32 sensor entity")),
            ]),
            true,
            "faker",
        ),
    ]
}

impl DeviceRegistry {
    /// Upsert faker catalogue without wiping HA-sourced entities.
    pub async fn seed_faker_catalog(&self) {
        let catalog = faker_catalog();
        let n = catalog.len();
        for entity in catalog {
            // Do not overwrite a live HA entity with the same id.
            if let Some(existing) = self.get(&entity.entity_id).await {
                if existing.source == "ha" {
                    continue;
                }
            }
            self.upsert(entity).await;
        }
        tracing::info!(count = n, "seeded faker brand devices (source=faker)");
    }

    /// Apply a simple action to an in-memory faker entity (no HA).
    pub async fn apply_faker_action(
        &self,
        entity_id: &str,
        action: &str,
        params: &HashMap<String, Value>,
    ) -> Result<DeviceEntity, String> {
        let mut entity = self
            .get(entity_id)
            .await
            .ok_or_else(|| format!("faker entity '{entity_id}' not in registry"))?;
        if entity.source != "faker" && !entity.is_faker {
            return Err(format!(
                "entity '{entity_id}' is not a faker stub (source={})",
                entity.source
            ));
        }

        match action {
            "turn_on" => {
                entity.state = "on".into();
                if let Some(b) = params.get("brightness").and_then(|v| v.as_u64()) {
                    entity.attributes.insert("brightness".into(), json!(b));
                }
            }
            "turn_off" => {
                entity.state = "off".into();
            }
            "toggle" => {
                entity.state = if entity.state == "on" || entity.state == "cool" {
                    "off".into()
                } else {
                    "on".into()
                };
            }
            "set_brightness" => {
                let b = params
                    .get("brightness")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| "params.brightness required".to_string())?;
                entity.state = "on".into();
                entity.attributes.insert("brightness".into(), json!(b));
            }
            "set_temperature" => {
                let t = params
                    .get("temperature")
                    .and_then(|v| v.as_f64())
                    .ok_or_else(|| "params.temperature required".to_string())?;
                if entity.state == "off" {
                    entity.state = "cool".into();
                }
                entity.attributes.insert("temperature".into(), json!(t));
            }
            "set_hvac_mode" => {
                let mode = params
                    .get("hvac_mode")
                    .or_else(|| params.get("mode"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "params.hvac_mode required".to_string())?;
                entity.state = mode.to_string();
            }
            other => {
                return Err(format!("unsupported faker action: {other}"));
            }
        }

        entity.available = true;
        self.upsert(entity.clone()).await;
        Ok(entity)
    }
}

/// Env gate: SEED_FAKER_DEVICES truthy, or force when HA is not configured.
pub fn should_seed_faker(ha_configured: bool) -> bool {
    match std::env::var("SEED_FAKER_DEVICES") {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => !ha_configured,
    }
}

/// Tiny helper kept for docs — builds a JSON snapshot of the catalogue.
#[allow(dead_code)]
pub fn catalog_json() -> Value {
    let list: Vec<Value> = faker_catalog()
        .into_iter()
        .map(|e| {
            let mut m = Map::new();
            m.insert("entity_id".into(), json!(e.entity_id));
            m.insert("brand".into(), json!(e.brand));
            m.insert("is_faker".into(), json!(e.is_faker));
            m.insert("friendly_name".into(), json!(e.friendly_name));
            Value::Object(m)
        })
        .collect();
    json!(list)
}
