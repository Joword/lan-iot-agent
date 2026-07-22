//! Hub → client event bus (fan-out for WebSocket subscribers).

use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::registry::DeviceEntity;

const CAPACITY: usize = 256;

/// Cloneable handle that publishes JSON frames to all Hub WS clients.
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<Value>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(CAPACITY);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.tx.subscribe()
    }

    pub fn publish(&self, event: Value) {
        // Ignore lagging / no-subscriber errors — hub must keep running.
        let _ = self.tx.send(event);
    }

    pub fn device_state_changed(&self, device: &DeviceEntity) {
        self.publish(json!({
            "type": "device:state_changed",
            "device": device,
        }));
    }

    pub fn error(&self, code: &str, message: impl Serialize) {
        self.publish(json!({
            "type": "error",
            "code": code,
            "message": message,
        }));
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
