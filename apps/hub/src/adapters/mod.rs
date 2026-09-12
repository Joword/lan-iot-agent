//! Device adapters.
//!
//! Southbound backends implement [`traits::DeviceAdapter`] and register on
//! [`router::AdapterRouter`]. Northbound REST / WS / MCP route by
//! `DeviceEntity.source` — HA is one backend, not the Hub's identity.
//!
//! - `ha`: Home Assistant REST/WS
//! - `faker`: in-memory brand stubs for offline demos
//! - `agent`: Python Agent HTTP (chat forward from Hub WS)
//! - `companion`: Companion devices (PC / phone / robot) via Hub HTTP (not a DeviceAdapter)

pub mod agent;
pub mod companion;
pub mod faker;
pub mod ha;
pub mod router;
pub mod traits;

pub use router::AdapterRouter;
pub use traits::AdapterError;
