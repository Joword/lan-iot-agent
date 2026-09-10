//! Scene engine with MongoDB persistence + in-memory cache.
//!
//! On startup: load collection `scenes` from the shared Mongo store. If empty,
//! seed from `config/scenes.toml` (or `SCENES_CONFIG`), else built-in demos
//! (`sleep_mode`, `away_mode`). When Mongo is down, Hub still runs from
//! TOML/demo in memory only.
//!
//! Each scene is an ordered list of device actions executed sequentially via
//! the AdapterRouter (HA / faker / future backends). Missing entities are
//! skipped as a graceful no-op.

use futures_util::TryStreamExt;
use mongodb::bson::doc;
use mongodb::options::IndexOptions;
use mongodb::{Collection, IndexModel};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::path::Path;
use tokio::sync::RwLock;

use crate::adapters::{AdapterError, AdapterRouter};
use crate::events::EventBus;
use crate::mongo::MongoStore;
use crate::registry::DeviceRegistry;

/// One device action inside a scene.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneAction {
    pub entity_id: String,
    pub action: String,
    #[serde(default)]
    pub params: HashMap<String, Value>,
}

/// A named scene: ordered list of actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub actions: Vec<SceneAction>,
}

/// Result of a single step when running a scene.
#[derive(Debug, Clone, Serialize)]
pub struct StepResult {
    pub index: usize,
    pub entity_id: String,
    pub action: String,
    pub ok: bool,
    /// Entity missing from HA/registry — treated as graceful no-op.
    pub skipped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
}

/// Aggregate result of `POST /scenes/{id}/run`.
#[derive(Debug, Clone, Serialize)]
pub struct RunResult {
    pub scene_id: String,
    /// True when every non-skipped step succeeded.
    pub ok: bool,
    pub steps: Vec<StepResult>,
    /// Indices of steps that failed (not skipped).
    pub failed: Vec<usize>,
    pub skipped_count: usize,
}

#[derive(Debug, Deserialize)]
struct ScenesFile {
    #[serde(default)]
    scenes: Vec<Scene>,
}

/// In-memory scene cache + optional Mongo `scenes` collection.
pub struct SceneEngine {
    scenes: RwLock<HashMap<String, Scene>>,
    collection: Option<Collection<Scene>>,
}

impl SceneEngine {
    pub fn new() -> Self {
        Self {
            scenes: RwLock::new(HashMap::new()),
            collection: None,
        }
    }

    /// Load from Mongo when available; seed if empty; else TOML / demos.
    pub async fn load(mongo: Option<&MongoStore>) -> Self {
        match mongo {
            Some(store) => match Self::try_load_mongo(store).await {
                Ok(engine) => engine,
                Err(e) => {
                    tracing::error!(
                        error = %e,
                        "MongoDB scenes load failed — falling back to config/demo"
                    );
                    Self::from_scenes(load_seed_scenes())
                }
            },
            None => {
                tracing::info!("MongoDB unavailable — scenes loaded from config/demo only");
                Self::from_scenes(load_seed_scenes())
            }
        }
    }

    async fn try_load_mongo(store: &MongoStore) -> Result<Self, mongodb::error::Error> {
        let collection: Collection<Scene> = store.collection("scenes");
        ensure_scene_indexes(&collection).await?;

        // Scenes persist after the first boot, so edits to scenes.toml are
        // invisible to an existing collection. SCENES_RESEED=1 overwrites the
        // seeded ids once (hand-authored scenes with other ids are untouched).
        let count = collection.count_documents(doc! {}).await?;
        if count == 0 || reseed_requested() {
            let seed = load_seed_scenes();
            for scene in &seed {
                collection
                    .replace_one(doc! { "id": &scene.id }, scene)
                    .upsert(true)
                    .await?;
            }
            tracing::info!(
                database = store.database_name(),
                count = seed.len(),
                reseed = count > 0,
                "seeded MongoDB scenes collection"
            );
            if count == 0 {
                return Ok(Self::from_scenes_with_collection(seed, Some(collection)));
            }
            // Re-seed over an existing collection: fall through so hand-authored
            // scenes outside the seed ids stay loaded.
        }

        let mut cursor = collection.find(doc! {}).await?;
        let mut scenes = Vec::new();
        while let Some(scene) = cursor.try_next().await? {
            scenes.push(scene);
        }
        tracing::info!(
            database = store.database_name(),
            count = scenes.len(),
            "loaded scenes from MongoDB"
        );
        Ok(Self::from_scenes_with_collection(scenes, Some(collection)))
    }

    pub fn from_scenes(scenes: Vec<Scene>) -> Self {
        Self::from_scenes_with_collection(scenes, None)
    }

    fn from_scenes_with_collection(
        scenes: Vec<Scene>,
        collection: Option<Collection<Scene>>,
    ) -> Self {
        let map = scenes.into_iter().map(|s| (s.id.clone(), s)).collect();
        Self {
            scenes: RwLock::new(map),
            collection,
        }
    }

    /// True when a Mongo `scenes` collection is bound (persistence enabled).
    pub fn mongo_backed(&self) -> bool {
        self.collection.is_some()
    }

    pub async fn list(&self) -> Vec<Scene> {
        let guard = self.scenes.read().await;
        let mut list: Vec<_> = guard.values().cloned().collect();
        list.sort_by(|a, b| a.id.cmp(&b.id));
        list
    }

    pub async fn list_ids(&self) -> Vec<String> {
        self.list().await.into_iter().map(|s| s.id).collect()
    }

    pub async fn get(&self, id: &str) -> Option<Scene> {
        self.scenes.read().await.get(id).cloned()
    }

    /// Create or update a scene in memory and (when available) MongoDB.
    pub async fn upsert(&self, scene: Scene) -> Result<(), SceneError> {
        if scene.id.trim().is_empty() {
            return Err(SceneError::Invalid("scene id required".into()));
        }
        if let Some(coll) = &self.collection {
            coll.replace_one(doc! { "id": &scene.id }, &scene)
                .upsert(true)
                .await
                .map_err(|e| {
                    tracing::error!(error = %e, scene_id = %scene.id, "MongoDB scene upsert failed");
                    SceneError::Unavailable
                })?;
        }
        self.scenes
            .write()
            .await
            .insert(scene.id.clone(), scene);
        Ok(())
    }

    /// Run scene actions sequentially via AdapterRouter. Continues on failure;
    /// missing entities are skipped (no-op).
    pub async fn run(
        &self,
        id: &str,
        adapters: &AdapterRouter,
        registry: &DeviceRegistry,
        events: &EventBus,
    ) -> Result<RunResult, SceneError> {
        let scene = self
            .get(id)
            .await
            .ok_or_else(|| SceneError::NotFound(id.to_string()))?;

        let mut steps = Vec::with_capacity(scene.actions.len());
        let mut failed = Vec::new();
        let mut skipped_count = 0usize;

        for (index, step) in scene.actions.iter().enumerate() {
            let outcome = execute_step(adapters, registry, events, index, step).await;
            if outcome.skipped {
                skipped_count += 1;
            } else if !outcome.ok {
                failed.push(index);
            }
            steps.push(outcome);
        }

        tracing::info!(
            scene_id = %scene.id,
            ok = failed.is_empty(),
            failed = failed.len(),
            skipped = skipped_count,
            "scene run finished"
        );

        Ok(RunResult {
            scene_id: scene.id,
            ok: failed.is_empty(),
            steps,
            failed,
            skipped_count,
        })
    }
}

impl Default for SceneEngine {
    fn default() -> Self {
        Self::from_scenes(load_seed_scenes())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SceneError {
    #[error("scene not found: {0}")]
    NotFound(String),
    #[error("invalid scene: {0}")]
    Invalid(String),
    #[error("scene store unavailable (MongoDB)")]
    Unavailable,
}

async fn ensure_scene_indexes(coll: &Collection<Scene>) -> Result<(), mongodb::error::Error> {
    let mut unique_opts = IndexOptions::default();
    unique_opts.unique = Some(true);
    let id_unique = IndexModel::builder()
        .keys(doc! { "id": 1 })
        .options(Some(unique_opts))
        .build();
    coll.create_indexes(vec![id_unique]).await?;
    Ok(())
}

/// Seed source: TOML if present, else built-in demos.
fn load_seed_scenes() -> Vec<Scene> {
    let candidates: Vec<String> = {
        let mut v = Vec::new();
        if let Ok(p) = env::var("SCENES_CONFIG") {
            if !p.is_empty() {
                v.push(p);
            }
        }
        v.push("../../config/scenes.toml".into());
        v.push("config/scenes.toml".into());
        v.push("/etc/lan-iot/scenes.toml".into());
        v
    };

    for path in &candidates {
        if !Path::new(path).exists() {
            continue;
        }
        match std::fs::read_to_string(path) {
            Ok(raw) => match toml::from_str::<ScenesFile>(&raw) {
                Ok(file) if !file.scenes.is_empty() => {
                    tracing::info!(
                        path,
                        count = file.scenes.len(),
                        "loaded seed scenes from config"
                    );
                    return file.scenes;
                }
                Ok(_) => {
                    tracing::warn!(path, "scenes file empty — using demo scenes");
                }
                Err(e) => {
                    tracing::warn!(path, error = %e, "failed to parse scenes.toml");
                }
            },
            Err(e) => {
                tracing::warn!(path, error = %e, "failed to read scenes.toml");
            }
        }
    }

    tracing::info!("using built-in demo scenes (sleep_mode, away_mode)");
    demo_scenes()
}

/// `SCENES_RESEED` truthy → overwrite seeded scene ids in Mongo at boot.
fn reseed_requested() -> bool {
    std::env::var("SCENES_RESEED")
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

/// Lights/plug shared by both demo scenes, in both id families.
///
/// `*.faker_*` are in-memory stubs, `*.demo_*` are the HA MQTT fixtures. Only
/// one family is usually present; absent entities are reported as skipped.
fn demo_off_steps() -> Vec<SceneAction> {
    [
        "light.faker_esp32_light",
        "light.demo_esp32_light",
        "light.faker_xiaomi_bulb",
        "light.demo_xiaomi_bulb",
        "switch.faker_xiaomi_plug",
        "switch.demo_xiaomi_plug",
    ]
    .iter()
    .map(|entity_id| SceneAction {
        entity_id: (*entity_id).into(),
        action: "turn_off".into(),
        params: HashMap::new(),
    })
    .collect()
}

fn demo_climate_steps(action: &str, params: HashMap<String, Value>) -> Vec<SceneAction> {
    ["climate.faker_gree_ac", "climate.demo_gree_ac"]
        .iter()
        .map(|entity_id| SceneAction {
            entity_id: (*entity_id).into(),
            action: action.into(),
            params: params.clone(),
        })
        .collect()
}

fn demo_scenes() -> Vec<Scene> {
    let mut sleep = demo_off_steps();
    sleep.extend(demo_climate_steps(
        "set_hvac_mode",
        HashMap::from([("hvac_mode".into(), json!("off"))]),
    ));

    let mut away = demo_off_steps();
    away.extend(demo_climate_steps(
        "set_temperature",
        HashMap::from([("temperature".into(), json!(26.0))]),
    ));

    vec![
        Scene {
            id: "sleep_mode".into(),
            name: "Sleep Mode".into(),
            description: Some(
                "Multi-brand bedtime: ESP32 + Xiaomi lights off, Gree AC off".into(),
            ),
            actions: sleep,
        },
        Scene {
            id: "away_mode".into(),
            name: "Away Mode".into(),
            description: Some(
                "Leaving home: turn off lights/plug and set Gree cool 26°C".into(),
            ),
            actions: away,
        },
    ]
}

async fn execute_step(
    adapters: &AdapterRouter,
    registry: &DeviceRegistry,
    events: &EventBus,
    index: usize,
    step: &SceneAction,
) -> StepResult {
    let entity_id = &step.entity_id;
    let action = &step.action;

    // Graceful no-op when the target entity is absent.
    if !entity_present(adapters, registry, entity_id).await {
        tracing::info!(
            entity_id,
            action,
            index,
            "scene step skipped — entity not present"
        );
        return StepResult {
            index,
            entity_id: entity_id.clone(),
            action: action.clone(),
            ok: true,
            skipped: true,
            error: Some(format!("entity '{entity_id}' not found — skipped")),
            result: Some(json!({ "skipped": true })),
        };
    }

    match adapters
        .control(registry, events, entity_id, action, &step.params)
        .await
    {
        Ok(outcome) => {
            let degraded = outcome.degraded_from.clone();
            StepResult {
                index,
                entity_id: entity_id.clone(),
                action: action.clone(),
                ok: true,
                skipped: false,
                error: degraded.map(|e| format!("degraded: {e}")),
                result: Some(outcome.to_json()),
            }
        }
        Err(AdapterError::NotFound(_)) | Err(AdapterError::Api { status: 404, .. }) => {
            StepResult {
                index,
                entity_id: entity_id.clone(),
                action: action.clone(),
                ok: true,
                skipped: true,
                error: Some(format!("entity '{entity_id}' not found — skipped")),
                result: Some(json!({ "skipped": true })),
            }
        }
        Err(e) => {
            tracing::warn!(
                entity_id,
                action,
                index,
                error = %e,
                "scene step failed"
            );
            StepResult {
                index,
                entity_id: entity_id.clone(),
                action: action.clone(),
                ok: false,
                skipped: false,
                error: Some(e.to_string()),
                result: None,
            }
        }
    }
}

async fn entity_present(
    adapters: &AdapterRouter,
    registry: &DeviceRegistry,
    entity_id: &str,
) -> bool {
    if registry.get(entity_id).await.is_some() {
        return true;
    }
    match adapters.get_state(registry, entity_id).await {
        Ok(_) => true,
        Err(AdapterError::NotFound(_)) | Err(AdapterError::Api { status: 404, .. }) => false,
        Err(e) => {
            tracing::debug!(
                entity_id,
                error = %e,
                "entity presence check inconclusive — will attempt action"
            );
            true
        }
    }
}
