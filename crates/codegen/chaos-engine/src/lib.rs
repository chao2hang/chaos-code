use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    CreateSession {
        client_msg_id: String,
    },
    Submit {
        client_msg_id: String,
        session_id: Uuid,
        prompt: String,
    },
    Cancel {
        client_msg_id: String,
        session_id: Uuid,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Handshake { protocol_version: u16 },
    SessionCreated { session_id: Uuid },
    Ack { client_msg_id: String },
    TextDelta { session_id: Uuid, text: String },
    Completed { session_id: Uuid },
    Cancelled { session_id: Uuid },
    Error { code: String, message: String },
}

#[derive(Clone)]
pub struct Engine {
    events: broadcast::Sender<ServerMessage>,
}

impl Engine {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(128);
        Self { events }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ServerMessage> {
        self.events.subscribe()
    }

    pub fn handle(&self, message: ClientMessage) -> Vec<ServerMessage> {
        match message {
            ClientMessage::CreateSession { .. } => {
                vec![ServerMessage::SessionCreated {
                    session_id: Uuid::new_v4(),
                }]
            }
            ClientMessage::Submit {
                client_msg_id,
                session_id,
                prompt,
            } => {
                let text = format!("演示响应：{prompt}");
                let result = vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::TextDelta { session_id, text },
                    ServerMessage::Completed { session_id },
                ];
                for event in &result {
                    let _ = self.events.send(event.clone());
                }
                result
            }
            ClientMessage::Cancel {
                client_msg_id,
                session_id,
            } => {
                let result = vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::Cancelled { session_id },
                ];
                for event in &result {
                    let _ = self.events.send(event.clone());
                }
                result
            }
        }
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_acknowledges_and_completes() {
        let engine = Engine::new();
        let session_id = Uuid::new_v4();
        let events = engine.handle(ClientMessage::Submit {
            client_msg_id: "m1".into(),
            session_id,
            prompt: "你好".into(),
        });
        assert!(matches!(events[0], ServerMessage::Ack { .. }));
        assert!(matches!(events[2], ServerMessage::Completed { .. }));
    }
}
