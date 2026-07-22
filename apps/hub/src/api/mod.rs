//! REST + WebSocket API handlers.

pub mod companions;
pub mod devices;
pub mod health;
pub mod scenes;
pub mod ws;

use axum::Router;
use std::sync::Arc;

use crate::AppState;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .merge(health::routes())
        .merge(devices::routes())
        .merge(companions::routes())
        .merge(scenes::routes())
        .merge(crate::auth::routes())
        .merge(ws::routes())
        .merge(crate::mcp::routes())
        .with_state(state)
}
