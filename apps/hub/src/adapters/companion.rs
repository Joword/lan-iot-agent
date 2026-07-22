//! Companion adapter (P6).
//!
//! Registry of Companion devices (phone/PC). Hub POSTs commands to each
//! device's `base_url` — HA does not manage this path.
//!
//! On startup: load collection `companions` from the shared Mongo store. If
//! empty, seed `companion.demo_pc` → `http://127.0.0.1:9876`. When Mongo is
//! down, Hub still runs from the demo seed in memory only.
//!
//! Demo Companion may be offline; callers get a structured unreachable error
//! (Hub stays up).

use futures_util::TryStreamExt;
use mongodb::bson::doc;
use mongodb::options::IndexOptions;
use mongodb::{Collection, IndexModel};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use thiserror::Error;
use tokio::sync::RwLock;

use crate::mongo::MongoStore;

/// Registered Companion endpoint (phone / PC agent).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanionDevice {
    pub id: String,
    pub name: String,
    /// Base URL of the Companion HTTP listener (Hub POSTs `{base}/command`).
    pub base_url: String,
    /// `pc` | `phone` | …
    pub kind: String,
}

/// Result of forwarding a command to a Companion.
#[derive(Debug, Clone, Serialize)]
pub struct CommandResult {
    pub ok: bool,
    pub device_id: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Error)]
pub enum CompanionError {
    #[error("companion not found: {0}")]
    NotFound(String),
    #[error("companion unreachable: {0}")]
    Unreachable(String),
    #[error("companion API error ({status}): {body}")]
    Api { status: u16, body: String },
    #[error("invalid companion: {0}")]
    Invalid(String),
    #[error("companion store unavailable (MongoDB)")]
    Unavailable,
}

/// In-memory Companion registry + optional Mongo `companions` collection.
pub struct CompanionAdapter {
    devices: RwLock<HashMap<String, CompanionDevice>>,
    collection: Option<Collection<CompanionDevice>>,
    client: reqwest::Client,
}

impl CompanionAdapter {
    pub fn new() -> Self {
        Self {
            devices: RwLock::new(HashMap::new()),
            collection: None,
            client: http_client(),
        }
    }

    /// Load from Mongo when available; seed demo if empty; else in-memory demo.
    pub async fn load(mongo: Option<&MongoStore>) -> Self {
        match mongo {
            Some(store) => match Self::try_load_mongo(store).await {
                Ok(adapter) => adapter,
                Err(e) => {
                    tracing::error!(
                        error = %e,
                        "MongoDB companions load failed — falling back to demo seed"
                    );
                    Self::from_devices(demo_companions())
                }
            },
            None => {
                tracing::info!("MongoDB unavailable — companions loaded from demo seed only");
                Self::from_devices(demo_companions())
            }
        }
    }

    async fn try_load_mongo(store: &MongoStore) -> Result<Self, mongodb::error::Error> {
        let collection: Collection<CompanionDevice> = store.collection("companions");
        ensure_companion_indexes(&collection).await?;

        let count = collection.count_documents(doc! {}).await?;
        if count == 0 {
            let seed = demo_companions();
            for device in &seed {
                collection
                    .replace_one(doc! { "id": &device.id }, device)
                    .upsert(true)
                    .await?;
            }
            tracing::info!(
                database = store.database_name(),
                count = seed.len(),
                "seeded MongoDB companions collection"
            );
            return Ok(Self::from_devices_with_collection(seed, Some(collection)));
        }

        let mut cursor = collection.find(doc! {}).await?;
        let mut devices = Vec::new();
        while let Some(device) = cursor.try_next().await? {
            devices.push(device);
        }
        tracing::info!(
            database = store.database_name(),
            count = devices.len(),
            "loaded companions from MongoDB"
        );
        Ok(Self::from_devices_with_collection(devices, Some(collection)))
    }

    /// Seed demo companion (`companion.demo_pc` → localhost; may be offline).
    pub fn with_demo() -> Self {
        Self::from_devices(demo_companions())
    }

    pub fn from_devices(devices: Vec<CompanionDevice>) -> Self {
        Self::from_devices_with_collection(devices, None)
    }

    fn from_devices_with_collection(
        devices: Vec<CompanionDevice>,
        collection: Option<Collection<CompanionDevice>>,
    ) -> Self {
        let map = devices.into_iter().map(|d| (d.id.clone(), d)).collect();
        Self {
            devices: RwLock::new(map),
            collection,
            client: http_client(),
        }
    }

    /// True when a Mongo `companions` collection is bound (persistence enabled).
    pub fn mongo_backed(&self) -> bool {
        self.collection.is_some()
    }

    pub async fn list(&self) -> Vec<CompanionDevice> {
        let guard = self.devices.read().await;
        let mut out: Vec<_> = guard.values().cloned().collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    pub async fn get(&self, id: &str) -> Option<CompanionDevice> {
        self.devices.read().await.get(id).cloned()
    }

    /// Create or update a companion in memory and (when available) MongoDB.
    pub async fn upsert(&self, device: CompanionDevice) -> Result<(), CompanionError> {
        if device.id.trim().is_empty() {
            return Err(CompanionError::Invalid("companion id required".into()));
        }
        if device.base_url.trim().is_empty() {
            return Err(CompanionError::Invalid("companion base_url required".into()));
        }
        if let Some(coll) = &self.collection {
            coll.replace_one(doc! { "id": &device.id }, &device)
                .upsert(true)
                .await
                .map_err(|e| {
                    tracing::error!(
                        error = %e,
                        companion_id = %device.id,
                        "MongoDB companion upsert failed"
                    );
                    CompanionError::Unavailable
                })?;
        }
        self.devices
            .write()
            .await
            .insert(device.id.clone(), device);
        Ok(())
    }

    /// Remove a companion from memory and (when available) MongoDB.
    pub async fn remove(&self, id: &str) -> Result<(), CompanionError> {
        let id = id.trim();
        if id.is_empty() {
            return Err(CompanionError::Invalid("companion id required".into()));
        }
        if let Some(coll) = &self.collection {
            coll.delete_one(doc! { "id": id }).await.map_err(|e| {
                tracing::error!(error = %e, companion_id = %id, "MongoDB companion delete failed");
                CompanionError::Unavailable
            })?;
        }
        let removed = self.devices.write().await.remove(id).is_some();
        if !removed {
            return Err(CompanionError::NotFound(id.to_string()));
        }
        Ok(())
    }

    /// POST `{base_url}/command` with `{"command":…}`. Graceful if Companion is down.
    pub async fn send_command(
        &self,
        device_id: &str,
        command: &str,
    ) -> Result<CommandResult, CompanionError> {
        let device = self
            .get(device_id)
            .await
            .ok_or_else(|| CompanionError::NotFound(device_id.to_string()))?;

        let url = format!(
            "{}/command",
            device.base_url.trim_end_matches('/')
        );
        let body = json!({ "command": command });

        match self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(resp) => {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                if !status.is_success() {
                    return Err(CompanionError::Api {
                        status: status.as_u16(),
                        body: text,
                    });
                }
                let response = if text.trim().is_empty() {
                    Some(json!({ "accepted": true }))
                } else {
                    serde_json::from_str(&text).ok().or(Some(json!({ "raw": text })))
                };
                Ok(CommandResult {
                    ok: true,
                    device_id: device_id.to_string(),
                    command: command.to_string(),
                    response,
                    error: None,
                })
            }
            Err(e) => {
                tracing::warn!(
                    device_id,
                    url = %url,
                    error = %e,
                    "companion command failed (device may be offline)"
                );
                Err(CompanionError::Unreachable(e.to_string()))
            }
        }
    }
}

impl Default for CompanionAdapter {
    fn default() -> Self {
        Self::with_demo()
    }
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("reqwest client")
}

fn demo_companions() -> Vec<CompanionDevice> {
    vec![CompanionDevice {
        id: "companion.demo_pc".into(),
        name: "Demo PC Companion".into(),
        // Intentionally may be down — stub for Agent/Hub wiring tests.
        base_url: "http://127.0.0.1:9876".into(),
        kind: "pc".into(),
    }]
}

async fn ensure_companion_indexes(
    coll: &Collection<CompanionDevice>,
) -> Result<(), mongodb::error::Error> {
    let mut unique_opts = IndexOptions::default();
    unique_opts.unique = Some(true);
    let id_unique = IndexModel::builder()
        .keys(doc! { "id": 1 })
        .options(Some(unique_opts))
        .build();
    coll.create_indexes(vec![id_unique]).await?;
    Ok(())
}
