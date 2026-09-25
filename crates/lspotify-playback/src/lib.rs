#![forbid(unsafe_code)]

mod ipc;
mod librespot_backend;
mod target;

pub use ipc::{BRIDGE_PROTOCOL_VERSION, BridgeEvent, Envelope, parse_event};
pub use librespot_backend::LibrespotLocalPlayer;
pub use target::{FallbackReason, PlaybackCoordinator, SelectedTarget};
