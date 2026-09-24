#![forbid(unsafe_code)]

pub mod action;
pub mod app;
pub mod artwork;
pub mod format;
pub mod render;
pub mod service;
pub mod session_state;
pub mod shortcuts;
pub mod state;
pub mod updater;

pub use action::{Action, Effect, HitMap, HitTarget, dispatch_key, dispatch_mouse};
pub use app::entry;
pub use shortcuts::{KeyBinding, ShortcutAction, ShortcutRegistry};
pub use state::*;
