use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;
use uuid::Uuid;

pub const PROTOCOL_VERSION: u16 = 1;
const DELTA_SIZE: usize = 8;

/// Boundary for connecting the GUI session state to a real Agent runtime.
/// Implementations return text chunks and never receive GUI credentials.
pub trait PromptAdapter: Send + Sync {
    fn run_prompt(&self, prompt: &str) -> Result<Vec<String>, String>;
}

/// Adapter that invokes the installed `chaos --headless` binary in JSON mode.
/// The binary path is explicit so Web/Desktop hosts cannot accidentally execute
/// an arbitrary command from a browser message.
pub struct HeadlessProcessAdapter {
    binary: std::path::PathBuf,
    cwd: std::path::PathBuf,
}

impl HeadlessProcessAdapter {
    pub fn new(binary: impl Into<std::path::PathBuf>, cwd: impl Into<std::path::PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            cwd: cwd.into(),
        }
    }
}

impl PromptAdapter for HeadlessProcessAdapter {
    fn run_prompt(&self, prompt: &str) -> Result<Vec<String>, String> {
        let output = std::process::Command::new(&self.binary)
            .current_dir(&self.cwd)
            .args([
                "--no-auto-update",
                "--output-format",
                "json",
                "--single",
                prompt,
            ])
            .output()
            .map_err(|error| format!("无法启动 headless Agent: {error}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        let value: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("headless Agent 返回无效 JSON: {error}"))?;
        let text = value
            .get("text")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "headless Agent JSON 缺少 text 字段".to_string())?;
        Ok(bounded_text_chunks(text, DELTA_SIZE))
    }
}

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

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct SessionState {
    messages: Vec<TimelineMessage>,
    audit: Vec<AuditEntry>,
    sequence: u64,
    pending_approval: Option<Uuid>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct State {
    sessions: HashMap<Uuid, SessionState>,
    seen_client_messages: HashSet<String>,
}

#[derive(Clone)]
pub struct Engine {
    events: broadcast::Sender<ServerMessage>,
    state: Arc<Mutex<State>>,
    store_path: Option<Arc<PathBuf>>,
    adapter: Option<Arc<dyn PromptAdapter>>,
}

impl Engine {
    pub fn new() -> Self {
        Self::with_state(State::default(), None, None)
    }

    pub fn with_adapter(adapter: impl PromptAdapter + 'static) -> Self {
        Self::with_state(State::default(), None, Some(Arc::new(adapter)))
    }

    pub fn with_adapter_arc(adapter: Arc<dyn PromptAdapter>) -> Self {
        Self::with_state(State::default(), None, Some(adapter))
    }

    pub fn with_persistence(path: impl AsRef<Path>) -> std::io::Result<Self> {
        Self::with_persistence_and_adapter(path, None)
    }

    pub fn with_persistence_and_adapter(
        path: impl AsRef<Path>,
        adapter: Option<Arc<dyn PromptAdapter>>,
    ) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let state = if path.exists() {
            serde_json::from_slice(&std::fs::read(&path)?).map_err(std::io::Error::other)?
        } else {
            State::default()
        };
        Ok(Self::with_state(state, Some(path), adapter))
    }

    fn with_state(
        state: State,
        path: Option<PathBuf>,
        adapter: Option<Arc<dyn PromptAdapter>>,
    ) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            events,
            state: Arc::new(Mutex::new(state)),
            store_path: path.map(Arc::new),
            adapter,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ServerMessage> {
        self.events.subscribe()
    }

    fn persist(&self, state: &State) -> Result<(), ServerMessage> {
        let Some(path) = &self.store_path else {
            return Ok(());
        };
        let bytes = serde_json::to_vec_pretty(state)
            .map_err(|_| Self::error("persistence_failed", "无法编码会话状态"))?;
        let temporary = path.with_extension("tmp");
        std::fs::write(&temporary, bytes)
            .map_err(|_| Self::error("persistence_failed", "无法写入会话状态"))?;
        std::fs::rename(temporary, path.as_ref())
            .map_err(|_| Self::error("persistence_failed", "无法提交会话状态"))?;
        Ok(())
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
                if let Some(summary) = prompt.strip_prefix("/approve-tool ") {
                    let request_id = Uuid::new_v4();
                    session.pending_approval = Some(request_id);
                    session.sequence += 1;
                    vec![
                        ServerMessage::Ack { client_msg_id },
                        ServerMessage::ToolApprovalRequested {
                            session_id,
                            request_id,
                            tool: "demo.tool".into(),
                            summary: summary.into(),
                            sequence: session.sequence,
                        },
                    ]
                } else {
                    let response_chunks =
                        match self.adapter.as_ref() {
                            Some(adapter) => adapter.run_prompt(&prompt).map_err(|message| {
                                ServerMessage::Error {
                                    code: "agent_failed".into(),
                                    message,
                                }
                            }),
                            None => Ok(bounded_text_chunks(
                                &format!("演示响应：{prompt}"),
                                DELTA_SIZE,
                            )),
                        };
                    let response_chunks = match response_chunks {
                        Ok(chunks) => chunks,
                        Err(error) => return vec![error],
                    };
                    let response = response_chunks.concat();
                    session.messages.push(TimelineMessage {
                        role: "user".into(),
                        text: prompt.clone(),
                    });
                    session.messages.push(TimelineMessage {
                        role: "assistant".into(),
                        text: response,
                    });
                    let mut events = vec![ServerMessage::Ack { client_msg_id }];
                    for text in response_chunks {
                        session.sequence += 1;
                        events.push(ServerMessage::TextDelta {
                            session_id,
                            text,
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
            } => self.resolve_approval(&mut state, client_msg_id, request_id, true),
            ClientMessage::Reject {
                client_msg_id,
                request_id,
                ..
            } => self.resolve_approval(&mut state, client_msg_id, request_id, false),
        };
        if let Err(error) = self.persist(&state) {
            return vec![error];
        }
        drop(state);
        for event in &result {
            let _ = self.events.send(event.clone());
        }
        result
    }

    fn resolve_approval(
        &self,
        state: &mut State,
        client_msg_id: String,
        request_id: Uuid,
        approved: bool,
    ) -> Vec<ServerMessage> {
        let Some((session_id, session)) = state
            .sessions
            .iter_mut()
            .find(|(_, session)| session.pending_approval == Some(request_id))
        else {
            return vec![Self::error("approval_not_found", "审批请求不存在或已处理")];
        };
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
    fn error(code: &str, message: &str) -> ServerMessage {
        ServerMessage::Error {
            code: code.into(),
            message: message.into(),
        }
    }
}
fn bounded_text_chunks(text: &str, max_bytes: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        if !current.is_empty() && current.len() + character.len_utf8() > max_bytes {
            chunks.push(std::mem::take(&mut current));
        }
        current.push(character);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
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
        assert!(matches!(
            &snapshot[0],
            ServerMessage::SessionSnapshot {
                messages,
                sequence,
                ..
            } if messages.len() == 2 && *sequence > 0
        ));
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
    #[test]
    fn persistent_engine_restores_snapshot_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions.json");
        let first = Engine::with_persistence(&path).unwrap();
        let id = match first.handle(ClientMessage::CreateSession {
            client_msg_id: "create".into(),
        })[0]
        {
            ServerMessage::SessionCreated { session_id } => session_id,
            _ => panic!(),
        };
        first.handle(ClientMessage::Submit {
            client_msg_id: "submit".into(),
            session_id: id,
            prompt: "持久化".into(),
        });
        drop(first);
        let second = Engine::with_persistence(&path).unwrap();
        let snapshot = second.handle(ClientMessage::Resume {
            client_msg_id: "resume".into(),
            session_id: id,
        });
        assert!(
            matches!(&snapshot[0], ServerMessage::SessionSnapshot { messages, .. } if messages[1].text.contains("持久化"))
        );
    }
    struct FixtureAdapter;

    impl PromptAdapter for FixtureAdapter {
        fn run_prompt(&self, prompt: &str) -> Result<Vec<String>, String> {
            Ok(vec![format!("真实 adapter: {prompt}")])
        }
    }

    #[cfg(unix)]
    #[test]
    fn headless_process_adapter_reads_real_json_entrypoint() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("chaos-fixture");
        std::fs::write(
            &binary,
            "#!/bin/sh\nprintf '{\"text\":\"process response\"}\\n'\n",
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        let adapter = HeadlessProcessAdapter::new(&binary, dir.path());
        assert_eq!(
            adapter.run_prompt("hello").unwrap().concat(),
            "process response"
        );
    }

    #[test]
    fn adapter_response_drives_the_same_session_events() {
        let engine = Engine::with_adapter(FixtureAdapter);
        let session_id = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "c".into(),
        })[0]
        {
            ServerMessage::SessionCreated { session_id } => session_id,
            _ => panic!(),
        };
        let events = engine.handle(ClientMessage::Submit {
            client_msg_id: "s".into(),
            session_id,
            prompt: "prompt".into(),
        });
        assert!(events.iter().any(|event| matches!(event, ServerMessage::TextDelta { text, .. } if text.contains("真实 adapter"))));
    }

    #[test]
    fn text_deltas_preserve_utf8_boundaries() {
        let chunks = bounded_text_chunks("你好 Chaos", 8);
        assert!(chunks.iter().all(|chunk| chunk.len() <= 8));
        assert_eq!(chunks.concat(), "你好 Chaos");
    }

    #[test]
    fn approval_requires_explicit_resolution() {
        let engine = Engine::new();
        let id = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "create".into(),
        })[0]
        {
            ServerMessage::SessionCreated { session_id } => session_id,
            _ => panic!(),
        };
        let events = engine.handle(ClientMessage::Submit {
            client_msg_id: "tool".into(),
            session_id: id,
            prompt: "/approve-tool write file".into(),
        });
        let request_id = match events[1] {
            ServerMessage::ToolApprovalRequested { request_id, .. } => request_id,
            _ => panic!(),
        };
        let resolved = engine.handle(ClientMessage::Reject {
            client_msg_id: "reject".into(),
            request_id,
            reason: "deny".into(),
        });
        assert!(resolved.iter().any(|event| matches!(
            event,
            ServerMessage::ApprovalResolved {
                approved: false,
                ..
            }
        )));
    }
}
