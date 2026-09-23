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
    Approve {
        client_msg_id: String,
        request_id: Uuid,
    },
    Reject {
        client_msg_id: String,
        request_id: Uuid,
        reason: String,
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
        sequence: u64,
    },
    Ack {
        client_msg_id: String,
    },
    TextDelta {
        session_id: Uuid,
        text: String,
        sequence: u64,
    },
    Completed {
        session_id: Uuid,
        sequence: u64,
    },
    Cancelled {
        session_id: Uuid,
        sequence: u64,
    },
    ToolApprovalRequested {
        session_id: Uuid,
        request_id: Uuid,
        tool: String,
        summary: String,
        sequence: u64,
    },
    ApprovalResolved {
        session_id: Uuid,
        request_id: Uuid,
        approved: bool,
        sequence: u64,
    },
    Audit {
        session_id: Uuid,
        action: String,
        outcome: String,
        sequence: u64,
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct AuditEntry {
    pub action: String,
    pub outcome: String,
    pub sequence: u64,
}

#[derive(Default)]
struct SessionState {
    messages: Vec<TimelineMessage>,
    audit: Vec<AuditEntry>,
    sequence: u64,
    pending_approval: Option<Uuid>,
}

#[derive(Default)]
struct State {
    sessions: HashMap<Uuid, SessionState>,
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
        let client_msg_id = match &message {
            ClientMessage::CreateSession { client_msg_id }
            | ClientMessage::Resume { client_msg_id, .. }
            | ClientMessage::Submit { client_msg_id, .. }
            | ClientMessage::Cancel { client_msg_id, .. }
            | ClientMessage::Snapshot { client_msg_id, .. }
            | ClientMessage::Approve { client_msg_id, .. }
            | ClientMessage::Reject { client_msg_id, .. } => client_msg_id.clone(),
        };
        let mut state = self.state.lock().expect("engine state lock");
        if !state.seen_client_messages.insert(client_msg_id.clone()) {
            return vec![ServerMessage::Ack { client_msg_id }];
        }
        let approval_is_approved = matches!(&message, ClientMessage::Approve { .. });
        let result = match message {
            ClientMessage::CreateSession { .. } => {
                let id = Uuid::new_v4();
                state.sessions.insert(id, SessionState::default());
                vec![ServerMessage::SessionCreated { session_id: id }]
            }
            ClientMessage::Resume { session_id, .. }
            | ClientMessage::Snapshot { session_id, .. } => match state.sessions.get(&session_id) {
                Some(session) => vec![ServerMessage::SessionSnapshot {
                    session_id,
                    messages: session.messages.clone(),
                    sequence: session.sequence,
                }],
                None => vec![Self::error("session_not_found", "会话不存在")],
            },
            ClientMessage::Submit {
                client_msg_id,
                session_id,
                prompt,
            } => {
                let Some(session) = state.sessions.get_mut(&session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                if session.pending_approval.is_some() {
                    return vec![Self::error("approval_pending", "请先处理待审批操作")];
                }
                session.messages.push(TimelineMessage {
                    role: "user".into(),
                    text: prompt.clone(),
                });
                let response = format!("演示响应：{prompt}");
                session.messages.push(TimelineMessage {
                    role: "assistant".into(),
                    text: response.clone(),
                });
                let mut events = vec![ServerMessage::Ack { client_msg_id }];
                for chunk in response.as_bytes().chunks(DELTA_SIZE) {
                    session.sequence += 1;
                    events.push(ServerMessage::TextDelta {
                        session_id,
                        text: String::from_utf8_lossy(chunk).into(),
                        sequence: session.sequence,
                    });
                }
                session.sequence += 1;
                events.push(ServerMessage::Completed {
                    session_id,
                    sequence: session.sequence,
                });
                events
            }
            ClientMessage::Cancel {
                client_msg_id,
                session_id,
            } => {
                let Some(session) = state.sessions.get_mut(&session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                session.sequence += 1;
                session.audit.push(AuditEntry {
                    action: "cancel".into(),
                    outcome: "accepted".into(),
                    sequence: session.sequence,
                });
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::Cancelled {
                        session_id,
                        sequence: session.sequence,
                    },
                    ServerMessage::Audit {
                        session_id,
                        action: "cancel".into(),
                        outcome: "accepted".into(),
                        sequence: session.sequence,
                    },
                ]
            }
            ClientMessage::Approve {
                client_msg_id,
                request_id,
            } => {
                let Some((session_id, session)) = state
                    .sessions
                    .iter_mut()
                    .find(|(_, session)| session.pending_approval == Some(request_id))
                else {
                    return vec![Self::error("approval_not_found", "审批请求不存在或已处理")];
                };
                let approved = approval_is_approved;
                session.pending_approval = None;
                session.sequence += 1;
                let outcome = if approved { "approved" } else { "rejected" };
                session.audit.push(AuditEntry {
                    action: "tool_approval".into(),
                    outcome: outcome.into(),
                    sequence: session.sequence,
                });
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::ApprovalResolved {
                        session_id: *session_id,
                        request_id,
                        approved,
                        sequence: session.sequence,
                    },
                    ServerMessage::Audit {
                        session_id: *session_id,
                        action: "tool_approval".into(),
                        outcome: outcome.into(),
                        sequence: session.sequence,
                    },
                ]
            }
            ClientMessage::Reject {
                client_msg_id,
                request_id,
                ..
            } => {
                let Some((session_id, session)) = state
                    .sessions
                    .iter_mut()
                    .find(|(_, session)| session.pending_approval == Some(request_id))
                else {
                    return vec![Self::error("approval_not_found", "审批请求不存在或已处理")];
                };
                session.pending_approval = None;
                session.sequence += 1;
                session.audit.push(AuditEntry {
                    action: "tool_approval".into(),
                    outcome: "rejected".into(),
                    sequence: session.sequence,
                });
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::ApprovalResolved {
                        session_id: *session_id,
                        request_id,
                        approved: false,
                        sequence: session.sequence,
                    },
                    ServerMessage::Audit {
                        session_id: *session_id,
                        action: "tool_approval".into(),
                        outcome: "rejected".into(),
                        sequence: session.sequence,
                    },
                ]
            }
        };
        drop(state);
        for event in &result {
            let _ = self.events.send(event.clone());
        }
        result
    }

    fn error(code: &str, message: &str) -> ServerMessage {
        ServerMessage::Error {
            code: code.into(),
            message: message.into(),
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
    fn real_session_flow_supports_resume_deduplication_and_sequence() {
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
                .any(|e| matches!(e, ServerMessage::TextDelta { sequence: 1, .. }))
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
            matches!(&snapshot[0], ServerMessage::SessionSnapshot { messages, sequence: 4, .. } if messages.len() == 2)
        );
    }
    #[test]
    fn cancel_writes_audit_and_is_idempotent() {
        let engine = Engine::new();
        let id = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "c".into(),
        })[0]
        {
            ServerMessage::SessionCreated { session_id } => session_id,
            _ => panic!(),
        };
        let events = engine.handle(ClientMessage::Cancel {
            client_msg_id: "x".into(),
            session_id: id,
        });
        assert!(
            events.iter().any(
                |e| matches!(e, ServerMessage::Audit { outcome, .. } if outcome == "accepted")
            )
        );
        assert_eq!(
            engine.handle(ClientMessage::Cancel {
                client_msg_id: "x".into(),
                session_id: id
            }),
            vec![ServerMessage::Ack {
                client_msg_id: "x".into()
            }]
        );
    }
}
