//! LanIoT Hub — control plane entry point.
//!
//! Provides the unified API gateway, auth, scenes, and MCP server.
//! Device I/O goes through Home Assistant (REST/WS); Companion is Hub HTTP only.

mod adapters;
mod api;
mod auth;
mod config;
mod events;
mod lan;
mod mcp;
mod mongo;
mod registry;
mod scene;

use adapters::agent::AgentClient;
use adapters::companion::CompanionAdapter;
use adapters::faker::FakerAdapter;
use adapters::ha::{spawn_ha_sync, HaAdapter, HaConnectionStatus};
use adapters::AdapterRouter;
use auth::AuthService;
use config::HubConfig;
use events::EventBus;
use mongo::MongoStore;
use registry::{should_seed_faker, DeviceRegistry};
use scene::SceneEngine;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Shared application state for Axum handlers.
pub struct AppState {
    pub config: HubConfig,
    /// Southbound device backends keyed by source (`ha`, `faker`, …).
    pub adapters: AdapterRouter,
    pub agent: AgentClient,
    pub registry: Arc<DeviceRegistry>,
    pub scenes: Arc<SceneEngine>,
    pub companions: Arc<CompanionAdapter>,
    pub auth: AuthService,
    /// Shared Mongo handle when connect succeeded at boot (may still fail later pings).
    pub mongo: Option<MongoStore>,
    pub ha_status: Arc<RwLock<HaConnectionStatus>>,
    pub events: EventBus,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "lan_iot_hub=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = HubConfig::load();
    let ha = HaAdapter::new(&config.ha_url, &config.ha_token);
    let agent = AgentClient::new(&config.agent_url);
    let registry = Arc::new(DeviceRegistry::new());
    let events = EventBus::new();
    let ha_status = Arc::new(RwLock::new(if config.ha_configured() {
        HaConnectionStatus::Unknown
    } else {
        HaConnectionStatus::NotConfigured
    }));

    if config.ha_configured() {
        tracing::info!(ha_url = %config.ha_url, "HA adapter configured");
        spawn_ha_sync(
            ha.clone(),
            Arc::clone(&registry),
            Arc::clone(&ha_status),
            events.clone(),
            config.registry_poll_secs,
        );
    } else {
        tracing::warn!(
            "HA_URL/HA_TOKEN not set — device endpoints use faker seed until HA is configured"
        );
    }

    // Offline / no-HA: seed branded faker devices (xiaomi / gree / esp32) so the
    // stack stays demoable. Set SEED_FAKER_DEVICES=0 to disable when HA is down.
    // When HA is up, default is off unless SEED_FAKER_DEVICES=1 (side-by-side).
    let seed_faker = match std::env::var("SEED_FAKER_DEVICES") {
        Ok(v) => {
            let t = v.trim().to_ascii_lowercase();
            if matches!(t.as_str(), "0" | "false" | "no" | "off") {
                false
            } else if matches!(t.as_str(), "1" | "true" | "yes" | "on") {
                true
            } else {
                should_seed_faker(config.ha_configured())
            }
        }
        Err(_) => should_seed_faker(config.ha_configured()),
    };
    if seed_faker {
        registry.seed_faker_catalog().await;
    }

    let mut adapters = AdapterRouter::new();
    adapters.register(Arc::new(ha.clone()));
    adapters.register(Arc::new(FakerAdapter::new(Arc::clone(&registry))));

    let mongo =
        MongoStore::connect_optional(&config.mongodb_uri, &config.mongodb_database).await;
    let scenes = Arc::new(SceneEngine::load(mongo.as_ref()).await);
    tracing::info!(
        scene_count = scenes.list().await.len(),
        scenes_mongo = scenes.mongo_backed(),
        "scene engine ready"
    );
    let companions = Arc::new(CompanionAdapter::load(mongo.as_ref()).await);
    tracing::info!(
        companion_count = companions.list().await.len(),
        companions_mongo = companions.mongo_backed(),
        "companion adapter ready"
    );
    tracing::info!(agent_url = %config.agent_url, "Agent forward URL");
    let auth = AuthService::from_store(mongo.as_ref(), config.auth_secret.clone()).await;
    tracing::info!(
        auth_required = config.auth_required,
        mongodb_ready = auth.available(),
        mongodb_database = %config.mongodb_database,
        "pairing auth ready (AUTH_REQUIRED / MongoDB)"
    );

    let state = Arc::new(AppState {
        config: config.clone(),
        adapters,
        agent,
        registry,
        scenes,
        companions,
        auth,
        mongo,
        ha_status,
        events,
    });

    let app = api::router(Arc::clone(&state))
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&state),
            auth::require_bearer,
        ))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = format!("{}:{}", config.bind, config.port)
        .parse()
        .expect("invalid bind address");
    tracing::info!("LanIoT Hub listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind Hub port");

    // Hub must start even if Agent / HA are not ready yet.
    axum::serve(listener, app)
        .await
        .expect("server error");
}
