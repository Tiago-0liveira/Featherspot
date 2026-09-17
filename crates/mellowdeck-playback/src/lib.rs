#![forbid(unsafe_code)]

mod ipc;
mod target;

pub use ipc::{BRIDGE_PROTOCOL_VERSION, BridgeEvent, Envelope, parse_event};
pub use target::{FallbackReason, PlaybackCoordinator, SelectedTarget};
