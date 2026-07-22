//! Brand / faker tagging for device entities.
//!
//! Real brand protocols live in Home Assistant. Hub only labels entities so UI
//! and Agent can tell stub (`faker` / `demo`) gear from production hardware.
//! Swap path: enable a real HA integration → new entity_ids appear → remove the
//! matching MQTT faker entity (or leave it; Hub will list both until deleted).

use serde::{Deserialize, Serialize};

/// Known brand labels (lowercase). Unknown → `None` / `"unknown"`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrandHint {
    Xiaomi,
    Gree,
    Esp32,
    Companion,
    Unknown,
}

impl BrandHint {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Xiaomi => "xiaomi",
            Self::Gree => "gree",
            Self::Esp32 => "esp32",
            Self::Companion => "companion",
            Self::Unknown => "unknown",
        }
    }
}

/// Infer brand + faker flag from HA entity_id / friendly_name.
///
/// Conventions (replace later with real HA entities — no Hub core change):
/// - `*.faker_*` or friendly name containing `[faker]` → `is_faker = true`
/// - `*.demo_*` → also treated as faker (legacy demo ids)
/// - brand tokens in id/name: xiaomi, gree, esp32
pub fn infer_brand_meta(entity_id: &str, friendly_name: &str) -> (Option<String>, bool) {
    let id_l = entity_id.to_ascii_lowercase();
    let name_l = friendly_name.to_ascii_lowercase();
    let blob = format!("{id_l} {name_l}");

    let is_faker = id_l.contains(".faker_")
        || id_l.contains("_faker_")
        || id_l.contains(".demo_")
        || id_l.contains("_demo_")
        || name_l.contains("[faker]")
        || name_l.contains("(faker)");

    let brand = if blob.contains("xiaomi") || blob.contains("miot") || blob.contains("miio") {
        Some(BrandHint::Xiaomi.as_str().to_string())
    } else if blob.contains("gree") {
        Some(BrandHint::Gree.as_str().to_string())
    } else if blob.contains("esp32") {
        Some(BrandHint::Esp32.as_str().to_string())
    } else if id_l.starts_with("companion.") {
        Some(BrandHint::Companion.as_str().to_string())
    } else {
        None
    };

    (brand, is_faker)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faker_xiaomi_id() {
        let (b, f) = infer_brand_meta("light.faker_xiaomi_bulb", "Faker Xiaomi Bulb");
        assert_eq!(b.as_deref(), Some("xiaomi"));
        assert!(f);
    }

    #[test]
    fn demo_gree_is_faker() {
        let (b, f) = infer_brand_meta("climate.demo_gree_ac", "Demo Gree AC [faker]");
        assert_eq!(b.as_deref(), Some("gree"));
        assert!(f);
    }

    #[test]
    fn real_looking_entity_not_faker() {
        let (b, f) = infer_brand_meta("light.living_room", "Living Room");
        assert!(b.is_none());
        assert!(!f);
    }
}
