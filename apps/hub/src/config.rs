//! Hub configuration from `hub.toml` + environment overrides.

use serde::Deserialize;
use std::env;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct HubConfig {
    pub bind: String,
    pub port: u16,
    pub ha_url: String,
    pub ha_token: String,
    pub agent_url: String,
    /// How often to REST-refresh the device registry when WS is unavailable (seconds).
    pub registry_poll_secs: u64,
    /// When true, REST/WS/MCP require `Authorization: Bearer` (except health + pair).
    pub auth_required: bool,
    /// Optional secret mixed into opaque tokens (stub HMAC). Empty = random-ish tokens.
    pub auth_secret: String,
    /// MongoDB connection URI (auth + scenes + companions).
    pub mongodb_uri: String,
    /// MongoDB database name (collections: `pairing_codes`, `tokens`, `scenes`, `companions`).
    pub mongodb_database: String,
}

#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    #[serde(default)]
    hub: HubSection,
    #[serde(default)]
    ha: HaSection,
    #[serde(default)]
    agent: AgentSection,
    #[serde(default)]
    mongodb: MongodbSection,
}

#[derive(Debug, Deserialize)]
struct HubSection {
    #[serde(default = "default_bind")]
    bind: String,
    #[serde(default = "default_port")]
    port: u16,
    #[serde(default = "default_poll")]
    registry_poll_secs: u64,
}

impl Default for HubSection {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            port: default_port(),
            registry_poll_secs: default_poll(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct HaSection {
    #[serde(default = "default_ha_url")]
    url: String,
    #[serde(default)]
    token: String,
}

impl Default for HaSection {
    fn default() -> Self {
        Self {
            url: default_ha_url(),
            token: String::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct AgentSection {
    #[serde(default = "default_agent_url")]
    url: String,
}

impl Default for AgentSection {
    fn default() -> Self {
        Self {
            url: default_agent_url(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct MongodbSection {
    #[serde(default = "default_mongodb_uri")]
    uri: String,
    #[serde(default = "default_mongodb_database")]
    database: String,
}

impl Default for MongodbSection {
    fn default() -> Self {
        Self {
            uri: default_mongodb_uri(),
            database: default_mongodb_database(),
        }
    }
}

fn default_bind() -> String {
    "0.0.0.0".into()
}
fn default_port() -> u16 {
    3000
}
fn default_poll() -> u64 {
    30
}
fn default_ha_url() -> String {
    "http://homeassistant:8123".into()
}
fn default_agent_url() -> String {
    "http://agent:8000".into()
}
fn default_mongodb_uri() -> String {
    "mongodb://mongo:27017".into()
}
fn default_mongodb_database() -> String {
    "lan_iot".into()
}

impl HubConfig {
    /// Load config: optional TOML file, then env overrides (`HA_URL`, `HA_TOKEN`, …).
    pub fn load() -> Self {
        let path = env::var("HUB_CONFIG")
            .unwrap_or_else(|_| "/etc/lan-iot/hub.toml".into());
        let file = Self::load_file(&path).unwrap_or_default();

        let mut cfg = HubConfig {
            bind: file.hub.bind,
            port: file.hub.port,
            ha_url: file.ha.url,
            ha_token: file.ha.token,
            agent_url: file.agent.url,
            registry_poll_secs: file.hub.registry_poll_secs,
            auth_required: false,
            auth_secret: String::new(),
            mongodb_uri: file.mongodb.uri,
            mongodb_database: file.mongodb.database,
        };

        if let Ok(v) = env::var("HA_URL") {
            if !v.is_empty() {
                cfg.ha_url = v;
            }
        }
        if let Ok(v) = env::var("HA_TOKEN") {
            cfg.ha_token = v;
        }
        if let Ok(v) = env::var("AGENT_URL") {
            if !v.is_empty() {
                cfg.agent_url = v;
            }
        }
        if let Ok(v) = env::var("HUB_PORT") {
            if let Ok(p) = v.parse() {
                cfg.port = p;
            }
        }
        if let Ok(v) = env::var("HUB_BIND") {
            if !v.is_empty() {
                cfg.bind = v;
            }
        }
        if let Ok(v) = env::var("REGISTRY_POLL_SECS") {
            if let Ok(p) = v.parse() {
                cfg.registry_poll_secs = p;
            }
        }
        if let Ok(v) = env::var("AUTH_REQUIRED") {
            cfg.auth_required = matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            );
        }
        if let Ok(v) = env::var("AUTH_SECRET") {
            cfg.auth_secret = v;
        }
        if let Ok(v) = env::var("MONGODB_URI") {
            if !v.is_empty() {
                cfg.mongodb_uri = v;
            }
        }
        if let Ok(v) = env::var("MONGODB_DATABASE") {
            if !v.is_empty() {
                cfg.mongodb_database = v;
            }
        }

        // Local-dev convenience: also try ../../config/hub.toml relative to CWD.
        if cfg.ha_token.is_empty() && !Path::new(&path).exists() {
            if let Some(local) = Self::load_file("../../config/hub.toml")
                .or_else(|| Self::load_file("config/hub.toml"))
            {
                if cfg.ha_url == default_ha_url() {
                    cfg.ha_url = local.ha.url;
                }
                if cfg.ha_token.is_empty() {
                    cfg.ha_token = local.ha.token;
                }
                if cfg.agent_url == default_agent_url() {
                    cfg.agent_url = local.agent.url;
                }
                if cfg.mongodb_uri == default_mongodb_uri() && !local.mongodb.uri.is_empty() {
                    cfg.mongodb_uri = local.mongodb.uri;
                }
                if cfg.mongodb_database == default_mongodb_database()
                    && !local.mongodb.database.is_empty()
                {
                    cfg.mongodb_database = local.mongodb.database;
                }
            }
        }

        cfg
    }

    fn load_file(path: &str) -> Option<FileConfig> {
        let raw = std::fs::read_to_string(path).ok()?;
        match toml::from_str::<FileConfig>(&raw) {
            Ok(c) => {
                tracing::info!(path, "loaded hub config");
                Some(c)
            }
            Err(e) => {
                tracing::warn!(path, error = %e, "failed to parse hub config");
                None
            }
        }
    }

    pub fn ha_configured(&self) -> bool {
        !self.ha_url.is_empty() && !self.ha_token.is_empty()
    }
}
