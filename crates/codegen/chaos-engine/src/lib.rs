use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;
use uuid::Uuid;

pub const PROTOCOL_VERSION: u16 = 1;
const DELTA_SIZE: usize = 8;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    CreateSession {
        client_msg_id: String,
    },
    Resume {
        client_msg_id: String,
        session_id: Uuid,
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
    Snapshot {
        client_msg_id: String,
        session_id: Uuid,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Handshake {
        protocol_version: u16,
    },
    SessionCreated {
        session_id: Uuid,
    },
    SessionSnapshot {
        session_id: Uuid,
        messages: Vec<TimelineMessage>,
    },
    Ack {
        client_msg_id: String,
    },
    TextDelta {
        session_id: Uuid,
        text: String,
    },
    Completed {
        session_id: Uuid,
    },
    Cancelled {
        session_id: Uuid,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct TimelineMessage {
    pub role: String,
    pub text: String,
}

#[derive(Default)]
struct State {
    sessions: HashMap<Uuid, Vec<TimelineMessage>>,
    seen_client_messages: HashSet<String>,
}

#[derive(Clone)]
pub struct Engine {
    events: broadcast::Sender<ServerMessage>,
    state: Arc<Mutex<State>>,
}

impl Engine {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            events,
            state: Arc::new(Mutex::new(State::default())),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ServerMessage> {
        self.events.subscribe()
    }

    pub fn handle(&self, message: ClientMessage) -> Vec<ServerMessage> {
        let (client_msg_id, _session_id) = match &message {
            ClientMessage::CreateSession { client_msg_id } => (client_msg_id, None),
            ClientMessage::Resume {
                client_msg_id,
                session_id,
            }
            | ClientMessage::Submit {
                client_msg_id,
                session_id,
                ..
            }
            | ClientMessage::Cancel {
                client_msg_id,
                session_id,
            }
            | ClientMessage::Snapshot {
                client_msg_id,
                session_id,
            } => (client_msg_id, Some(*session_id)),
        };
        let mut state = self.state.lock().expect("engine state lock");
        if !state.seen_client_messages.insert(client_msg_id.clone()) {
            return vec![ServerMessage::Ack {
                client_msg_id: client_msg_id.clone(),
            }];
        }
        let result = match message {
            ClientMessage::CreateSession { .. } => {
                let id = Uuid::new_v4();
                state.sessions.insert(id, Vec::new());
                vec![ServerMessage::SessionCreated { session_id: id }]
            }
            ClientMessage::Resume { session_id, .. }
            | ClientMessage::Snapshot { session_id, .. } => match state.sessions.get(&session_id) {
                Some(messages) => vec![ServerMessage::SessionSnapshot {
                    session_id,
                    messages: messages.clone(),
                }],
                None => vec![ServerMessage::Error {
                    code: "session_not_found".into(),
                    message: "会话不存在".into(),
                }],
            },
            ClientMessage::Submit {
                client_msg_id,
                session_id,
                prompt,
            } => {
                if !state.sessions.contains_key(&session_id) {
                    vec![ServerMessage::Error {
                        code: "session_not_found".into(),
                        message: "会话不存在".into(),
                    }]
                } else {
                    let response = format!("演示响应：{prompt}");
                    state.sessions.get_mut(&session_id).unwrap().extend([
                        TimelineMessage {
                            role: "user".into(),
                            text: prompt,
                        },
                        TimelineMessage {
                            role: "assistant".into(),
                            text: response.clone(),
                        },
                    ]);
                    let mut events = vec![ServerMessage::Ack { client_msg_id }];
                    events.extend(response.as_bytes().chunks(DELTA_SIZE).map(|chunk| {
                        ServerMessage::TextDelta {
                            session_id,
                            text: String::from_utf8_lossy(chunk).into(),
                        }
                    }));
                    events.push(ServerMessage::Completed { session_id });
                    events
                }
            }
            ClientMessage::Cancel {
                client_msg_id,
                session_id,
            } => vec![
                ServerMessage::Ack { client_msg_id },
                ServerMessage::Cancelled { session_id },
            ],
        };
        drop(state);
        for event in &result {
            let _ = self.events.send(event.clone());
        }
        result
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
    fn real_session_flow_supports_resume_and_deduplication() {
        let engine = Engine::new();
        let created = engine.handle(ClientMessage::CreateSession {
            client_msg_id: "create".into(),
        });
        let id = match created[0] {
            ServerMessage::SessionCreated { session_id } => session_id,
            _ => panic!(),
        };
        let first = engine.handle(ClientMessage::Submit {
            client_msg_id: "submit".into(),
            session_id: id,
            prompt: "你好".into(),
        });
        assert!(
            first
                .iter()
                .any(|e| matches!(e, ServerMessage::TextDelta { .. }))
        );
        assert_eq!(
            engine.handle(ClientMessage::Submit {
                client_msg_id: "submit".into(),
                session_id: id,
                prompt: "重复".into()
            }),
            vec![ServerMessage::Ack {
                client_msg_id: "submit".into()
            }]
        );
        let snapshot = engine.handle(ClientMessage::Resume {
            client_msg_id: "resume".into(),
            session_id: id,
        });
        assert!(
            matches!(&snapshot[0], ServerMessage::SessionSnapshot { messages, .. } if messages.len() == 2)
        );
    }
}
