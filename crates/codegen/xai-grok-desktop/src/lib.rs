//! Desktop host boundary for the shared Chaos engine.
//!
//! Tauri integration is intentionally isolated here; the first walking
//! skeleton exposes the same engine handle that a Tauri command layer will use.

pub use chaos_engine::{ClientMessage, Engine, ServerMessage};

#[derive(Clone)]
pub struct DesktopApp {
    pub engine: Engine,
}

impl DesktopApp {
    pub fn new(engine: Engine) -> Self {
        Self { engine }
    }
}
