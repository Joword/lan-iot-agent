//! Machine-readable device capabilities for Agent / MCP tool planning.
//!
//! Derived from entity type + HA-style attributes (supported_features,
//! hvac_modes, min/max temp, brightness, …). Keeps the Hub northbound
//! contract independent of any single southbound backend.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

use super::EntityType;

/// High-level capability class an entity exposes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    OnOff,
    Brightness,
    ColorTemp,
    Color,
    Thermostat,
    HvacMode,
    OpenClose,
    FanSpeed,
    SensorRead,
}

/// One parameter accepted by an action.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParamSpec {
    pub name: String,
    /// JSON-schema-ish: number | integer | string | boolean
    #[serde(rename = "type")]
    pub param_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#enum: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// One controllable action (e.g. `turn_on`, `set_temperature`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActionSpec {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub params: Vec<ParamSpec>,
}

/// A capability cluster: kind + the actions that implement it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Capability {
    pub kind: CapabilityKind,
    #[serde(default)]
    pub actions: Vec<ActionSpec>,
}

impl Capability {
    /// Flatten all action names across capabilities (for MCP describe).
    pub fn action_names(caps: &[Capability]) -> Vec<String> {
        let mut names: Vec<String> = caps
            .iter()
            .flat_map(|c| c.actions.iter().map(|a| a.name.clone()))
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Compact JSON summary for tool responses.
    pub fn summarize(caps: &[Capability]) -> Value {
        let kinds: Vec<&str> = caps
            .iter()
            .map(|c| match c.kind {
                CapabilityKind::OnOff => "on_off",
                CapabilityKind::Brightness => "brightness",
                CapabilityKind::ColorTemp => "color_temp",
                CapabilityKind::Color => "color",
                CapabilityKind::Thermostat => "thermostat",
                CapabilityKind::HvacMode => "hvac_mode",
                CapabilityKind::OpenClose => "open_close",
                CapabilityKind::FanSpeed => "fan_speed",
                CapabilityKind::SensorRead => "sensor_read",
            })
            .collect();
        json!({
            "kinds": kinds,
            "actions": Self::action_names(caps),
            "capabilities": caps,
        })
    }
}

/// Infer capabilities from entity domain + HA-like attributes.
pub fn derive_capabilities(
    entity_type: &EntityType,
    attributes: &HashMap<String, Value>,
) -> Vec<Capability> {
    match entity_type {
        EntityType::Light => derive_light(attributes),
        EntityType::Climate => derive_climate(attributes),
        EntityType::Switch | EntityType::Fan => derive_on_off_switch(entity_type, attributes),
        EntityType::Cover => derive_cover(),
        EntityType::Sensor | EntityType::BinarySensor => vec![Capability {
            kind: CapabilityKind::SensorRead,
            actions: vec![],
        }],
        EntityType::Other => derive_generic_on_off(),
    }
}

fn on_off_actions() -> Vec<ActionSpec> {
    vec![
        ActionSpec {
            name: "turn_on".into(),
            description: Some("Turn the device on".into()),
            params: vec![],
        },
        ActionSpec {
            name: "turn_off".into(),
            description: Some("Turn the device off".into()),
            params: vec![],
        },
        ActionSpec {
            name: "toggle".into(),
            description: Some("Toggle on/off".into()),
            params: vec![],
        },
    ]
}

fn derive_generic_on_off() -> Vec<Capability> {
    vec![Capability {
        kind: CapabilityKind::OnOff,
        actions: on_off_actions(),
    }]
}

fn derive_on_off_switch(entity_type: &EntityType, attributes: &HashMap<String, Value>) -> Vec<Capability> {
    let mut caps = vec![Capability {
        kind: CapabilityKind::OnOff,
        actions: on_off_actions(),
    }];
    if matches!(entity_type, EntityType::Fan) {
        if let Some(pct) = attributes.get("percentage").or_else(|| attributes.get("speed")) {
            let _ = pct;
            caps.push(Capability {
                kind: CapabilityKind::FanSpeed,
                actions: vec![ActionSpec {
                    name: "set_percentage".into(),
                    description: Some("Set fan speed percentage 0–100".into()),
                    params: vec![ParamSpec {
                        name: "percentage".into(),
                        param_type: "integer".into(),
                        required: true,
                        minimum: Some(0.0),
                        maximum: Some(100.0),
                        r#enum: None,
                        description: Some("Fan speed percent".into()),
                    }],
                }],
            });
        }
    }
    caps
}

fn derive_light(attributes: &HashMap<String, Value>) -> Vec<Capability> {
    let mut caps = vec![Capability {
        kind: CapabilityKind::OnOff,
        actions: on_off_actions(),
    }];

    let has_brightness = attributes.contains_key("brightness")
        || attributes.contains_key("brightness_pct")
        || color_modes_include(attributes, &["brightness", "color_temp", "hs", "xy", "rgb"])
        || supported_feature_bit(attributes, 1); // SUPPORT_BRIGHTNESS historically

    if has_brightness {
        caps.push(Capability {
            kind: CapabilityKind::Brightness,
            actions: vec![ActionSpec {
                name: "set_brightness".into(),
                description: Some("Set brightness (0–255) and turn on".into()),
                params: vec![ParamSpec {
                    name: "brightness".into(),
                    param_type: "integer".into(),
                    required: true,
                    minimum: Some(0.0),
                    maximum: Some(255.0),
                    r#enum: None,
                    description: Some("HA brightness scale 0–255".into()),
                }],
            }],
        });
    }

    if attributes.contains_key("color_temp")
        || attributes.contains_key("color_temp_kelvin")
        || color_modes_include(attributes, &["color_temp"])
    {
        caps.push(Capability {
            kind: CapabilityKind::ColorTemp,
            actions: vec![ActionSpec {
                name: "set_color_temp".into(),
                description: Some("Set color temperature (mireds or kelvin via params)".into()),
                params: vec![ParamSpec {
                    name: "color_temp".into(),
                    param_type: "integer".into(),
                    required: false,
                    minimum: None,
                    maximum: None,
                    r#enum: None,
                    description: Some("Color temperature in mireds".into()),
                }],
            }],
        });
    }

    if color_modes_include(attributes, &["hs", "xy", "rgb", "rgbw", "rgbww"])
        || attributes.contains_key("hs_color")
        || attributes.contains_key("rgb_color")
    {
        caps.push(Capability {
            kind: CapabilityKind::Color,
            actions: vec![ActionSpec {
                name: "turn_on".into(),
                description: Some("Turn on with optional rgb_color / hs_color in params".into()),
                params: vec![],
            }],
        });
    }

    caps
}

fn derive_climate(attributes: &HashMap<String, Value>) -> Vec<Capability> {
    let min_t = num_attr(attributes, &["min_temp", "min_temperature"]).unwrap_or(16.0);
    let max_t = num_attr(attributes, &["max_temp", "max_temperature"]).unwrap_or(30.0);

    let mut caps = vec![
        Capability {
            kind: CapabilityKind::OnOff,
            actions: on_off_actions(),
        },
        Capability {
            kind: CapabilityKind::Thermostat,
            actions: vec![ActionSpec {
                name: "set_temperature".into(),
                description: Some("Set target temperature".into()),
                params: vec![ParamSpec {
                    name: "temperature".into(),
                    param_type: "number".into(),
                    required: true,
                    minimum: Some(min_t),
                    maximum: Some(max_t),
                    r#enum: None,
                    description: Some(format!("Target °C ({min_t}–{max_t})")),
                }],
            }],
        },
    ];

    let modes = string_list_attr(attributes, "hvac_modes");
    if !modes.is_empty() {
        caps.push(Capability {
            kind: CapabilityKind::HvacMode,
            actions: vec![ActionSpec {
                name: "set_hvac_mode".into(),
                description: Some("Set HVAC mode".into()),
                params: vec![ParamSpec {
                    name: "mode".into(),
                    param_type: "string".into(),
                    required: true,
                    minimum: None,
                    maximum: None,
                    r#enum: Some(modes.clone()),
                    description: Some("HVAC mode".into()),
                }],
            }],
        });
    }

    caps
}

fn derive_cover() -> Vec<Capability> {
    vec![Capability {
        kind: CapabilityKind::OpenClose,
        actions: vec![
            ActionSpec {
                name: "open_cover".into(),
                description: Some("Open the cover".into()),
                params: vec![],
            },
            ActionSpec {
                name: "close_cover".into(),
                description: Some("Close the cover".into()),
                params: vec![],
            },
            ActionSpec {
                name: "stop_cover".into(),
                description: Some("Stop cover motion".into()),
                params: vec![],
            },
        ],
    }]
}

fn color_modes_include(attributes: &HashMap<String, Value>, needles: &[&str]) -> bool {
    let Some(modes) = attributes.get("supported_color_modes") else {
        return false;
    };
    match modes {
        Value::Array(arr) => arr.iter().any(|v| {
            v.as_str()
                .is_some_and(|s| needles.iter().any(|n| s.eq_ignore_ascii_case(n)))
        }),
        Value::String(s) => needles.iter().any(|n| s.to_ascii_lowercase().contains(n)),
        _ => false,
    }
}

fn supported_feature_bit(attributes: &HashMap<String, Value>, bit: u64) -> bool {
    attributes
        .get("supported_features")
        .and_then(|v| v.as_u64())
        .is_some_and(|f| f & bit != 0)
}

fn num_attr(attributes: &HashMap<String, Value>, keys: &[&str]) -> Option<f64> {
    for k in keys {
        if let Some(v) = attributes.get(*k) {
            if let Some(n) = v.as_f64() {
                return Some(n);
            }
            if let Some(n) = v.as_i64() {
                return Some(n as f64);
            }
        }
    }
    None
}

fn string_list_attr(attributes: &HashMap<String, Value>, key: &str) -> Vec<String> {
    match attributes.get(key) {
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        Some(Value::String(s)) => s.split(',').map(|p| p.trim().to_string()).collect(),
        _ => Vec::new(),
    }
}

/// Build an OpenAI-ish JSON Schema object from ParamSpec list (for dynamic tools).
#[allow(dead_code)]
pub fn params_to_json_schema(params: &[ParamSpec]) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for p in params {
        let mut prop = Map::new();
        prop.insert("type".into(), json!(p.param_type));
        if let Some(d) = &p.description {
            prop.insert("description".into(), json!(d));
        }
        if let Some(min) = p.minimum {
            prop.insert("minimum".into(), json!(min));
        }
        if let Some(max) = p.maximum {
            prop.insert("maximum".into(), json!(max));
        }
        if let Some(en) = &p.r#enum {
            prop.insert("enum".into(), json!(en));
        }
        properties.insert(p.name.clone(), Value::Object(prop));
        if p.required {
            required.push(p.name.clone());
        }
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn climate_derives_thermostat_range() {
        let attrs = HashMap::from([
            ("min_temp".into(), json!(16.0)),
            ("max_temp".into(), json!(30.0)),
            ("hvac_modes".into(), json!(["off", "cool", "heat"])),
        ]);
        let caps = derive_capabilities(&EntityType::Climate, &attrs);
        assert!(caps.iter().any(|c| c.kind == CapabilityKind::Thermostat));
        assert!(caps.iter().any(|c| c.kind == CapabilityKind::HvacMode));
        let thermo = caps
            .iter()
            .find(|c| c.kind == CapabilityKind::Thermostat)
            .unwrap();
        let p = &thermo.actions[0].params[0];
        assert_eq!(p.minimum, Some(16.0));
        assert_eq!(p.maximum, Some(30.0));
    }

    #[test]
    fn light_with_brightness_attr() {
        let attrs = HashMap::from([("brightness".into(), json!(128))]);
        let caps = derive_capabilities(&EntityType::Light, &attrs);
        assert!(caps.iter().any(|c| c.kind == CapabilityKind::Brightness));
        assert!(Capability::action_names(&caps).contains(&"set_brightness".into()));
    }
}
