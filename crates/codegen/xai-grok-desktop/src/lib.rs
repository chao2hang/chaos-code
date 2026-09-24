//! Desktop host boundary for the shared Chaos engine.
//!
//! The host intentionally has no Tauri dependency in the default workspace.
//! Tauri commands can call these functions from a platform-specific adapter
//! without creating a second session state machine.

pub use chaos_engine::{ClientMessage, Engine, ServerMessage};

#[derive(Clone)]
pub struct DesktopApp {
    pub engine: Engine,
}

impl DesktopApp {
    pub fn new(engine: Engine) -> Self {
        Self { engine }
    }
    pub fn dispatch(&self, message: ClientMessage) -> Vec<ServerMessage> {
        self.engine.handle(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_host_dispatches_shared_engine_protocol() {
        let app = DesktopApp::new(Engine::new());
        let events = app.dispatch(ClientMessage::CreateSession {
            client_msg_id: "desktop-create".into(),
        });
        assert!(matches!(events[0], ServerMessage::SessionCreated { .. }));
    }
}
