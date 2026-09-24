use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;
use uuid::Uuid;

pub const PROTOCOL_VERSION: u16 = 1;
pub const STATE_SCHEMA_VERSION: u16 = 1;
const DELTA_SIZE: usize = 8;

/// Boundary for connecting the GUI session state to a real Agent runtime.
/// Implementations return text chunks and never receive GUI credentials.
pub trait PromptAdapter: Send + Sync {
    fn run_prompt(&self, prompt: &str) -> Result<Vec<String>, String>;
}

/// Boundary for tools that require an explicit approval before execution.
/// The adapter receives only the declared tool and summary, never browser
/// transport objects or raw credentials.
pub trait ToolAdapter: Send + Sync {
    fn execute(&self, tool: &str, summary: &str) -> Result<String, String>;
}

/// Boundary for applying a proposed file change. Implementations own the
/// actual workspace and must make accept/rollback atomic for their backend.
pub trait DiffAdapter: Send + Sync {
    fn accept(&self, proposal_id: &str, summary: &str) -> Result<(), String>;
    fn rollback(&self, proposal_id: &str) -> Result<(), String>;
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
    RespondQuestion {
        client_msg_id: String,
        question_id: Uuid,
        answer: String,
    },
    ListFiles {
        client_msg_id: String,
        relative_path: String,
    },
    ReadFile {
        client_msg_id: String,
        relative_path: String,
    },
    SearchFiles {
        client_msg_id: String,
        query: String,
    },
    ProposeFileWrite {
        client_msg_id: String,
        session_id: Uuid,
        relative_path: String,
        contents: String,
    },
    GetSettings {
        client_msg_id: String,
    },
    UpdateSettings {
        client_msg_id: String,
        base_url: Option<String>,
        model: Option<String>,
    },
    AcceptDiff {
        client_msg_id: String,
        session_id: Uuid,
        proposal_id: String,
        summary: String,
    },
    RollbackDiff {
        client_msg_id: String,
        session_id: Uuid,
        proposal_id: String,
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
    DiffResolved {
        proposal_id: String,
        action: String,
        sequence: u64,
    },
    QuestionRequested {
        session_id: Uuid,
        question_id: Uuid,
        prompt: String,
        sequence: u64,
    },
    QuestionResolved {
        session_id: Uuid,
        question_id: Uuid,
        answer: String,
        sequence: u64,
    },
    ToolStarted {
        session_id: Uuid,
        tool: String,
        sequence: u64,
    },
    ToolProgress {
        session_id: Uuid,
        tool: String,
        progress: String,
        sequence: u64,
    },
    ToolResult {
        session_id: Uuid,
        tool: String,
        result: String,
        sequence: u64,
    },
    FileChanged {
        session_id: Uuid,
        path: String,
        operation: String,
        sequence: u64,
    },
    Usage {
        session_id: Uuid,
        input_tokens: u64,
        output_tokens: u64,
        sequence: u64,
    },
    FilesListed {
        path: String,
        entries: Vec<String>,
    },
    FileContents {
        path: String,
        contents: String,
    },
    SearchResults {
        query: String,
        matches: Vec<String>,
    },
    FileWritten {
        session_id: Uuid,
        path: String,
        bytes: usize,
    },
    Settings {
        base_url: Option<String>,
        model: Option<String>,
        has_api_key: bool,
    },
    SettingsUpdated {
        base_url: Option<String>,
        model: Option<String>,
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
struct PendingApproval {
    request_id: Uuid,
    tool: String,
    summary: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct SessionState {
    messages: Vec<TimelineMessage>,
    audit: Vec<AuditEntry>,
    sequence: u64,
    pending_approval: Option<PendingApproval>,
    pending_question: Option<Uuid>,
    pending_file_writes: HashMap<Uuid, (Uuid, String, String)>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct State {
    sessions: HashMap<Uuid, SessionState>,
    seen_client_messages: HashSet<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StateEnvelope {
    schema_version: u16,
    state: State,
}

#[derive(Clone)]
pub struct WorkspaceAdapter {
    root: Arc<PathBuf>,
}

impl WorkspaceAdapter {
    pub fn new(root: impl AsRef<Path>) -> std::io::Result<Self> {
        let root = std::fs::canonicalize(root)?;
        if !root.is_dir() {
            return Err(std::io::Error::other("workspace root is not a directory"));
        }
        Ok(Self {
            root: Arc::new(root),
        })
    }

    fn confined(&self, relative: &str) -> Result<PathBuf, ServerMessage> {
        let candidate = self.root.join(relative);
        let canonical = std::fs::canonicalize(&candidate).map_err(|_| ServerMessage::Error {
            code: "path_invalid".into(),
            message: "路径不存在或无法解析".into(),
        })?;
        if !canonical.starts_with(self.root.as_path()) {
            return Err(ServerMessage::Error {
                code: "path_escape".into(),
                message: "路径超出 workspace 范围".into(),
            });
        }
        Ok(canonical)
    }

    fn list(&self, relative: &str) -> Result<Vec<String>, ServerMessage> {
        let path = self.confined(relative)?;
        let mut entries = std::fs::read_dir(path)
            .map_err(|_| ServerMessage::Error {
                code: "list_failed".into(),
                message: "无法读取目录".into(),
            })?
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect::<Vec<_>>();
        entries.sort();
        Ok(entries)
    }

    fn read(&self, relative: &str) -> Result<String, ServerMessage> {
        let path = self.confined(relative)?;
        let metadata = std::fs::metadata(&path).map_err(|_| ServerMessage::Error {
            code: "read_failed".into(),
            message: "无法读取文件".into(),
        })?;
        if metadata.len() > 1024 * 1024 {
            return Err(ServerMessage::Error {
                code: "file_too_large".into(),
                message: "文件超过 1 MiB 限制".into(),
            });
        }
        std::fs::read_to_string(path).map_err(|_| ServerMessage::Error {
            code: "read_failed".into(),
            message: "文件不是可读文本".into(),
        })
    }

    fn write(&self, relative: &str, contents: &str) -> Result<usize, ServerMessage> {
        if contents.len() > 1024 * 1024 {
            return Err(ServerMessage::Error {
                code: "file_too_large".into(),
                message: "文件超过 1 MiB 限制".into(),
            });
        }
        let relative_path = Path::new(relative);
        if relative_path.is_absolute() {
            return Err(Self::path_escape());
        }
        let candidate = self.root.join(relative_path);
        let parent = candidate.parent().ok_or_else(|| ServerMessage::Error {
            code: "write_failed".into(),
            message: "无效父目录".into(),
        })?;
        let canonical_parent = std::fs::canonicalize(parent).map_err(|_| ServerMessage::Error {
            code: "write_failed".into(),
            message: "父目录不存在".into(),
        })?;
        if !canonical_parent.starts_with(self.root.as_path()) {
            return Err(Self::path_escape());
        }
        if candidate.exists() {
            let canonical = std::fs::canonicalize(&candidate).map_err(|_| Self::path_escape())?;
            if !canonical.starts_with(self.root.as_path()) {
                return Err(Self::path_escape());
            }
        }
        let path = canonical_parent.join(candidate.file_name().ok_or_else(Self::path_escape)?);
        std::fs::write(&path, contents).map_err(|_| ServerMessage::Error {
            code: "write_failed".into(),
            message: "无法写入文件".into(),
        })?;
        Ok(contents.len())
    }

    fn path_escape() -> ServerMessage {
        ServerMessage::Error {
            code: "path_escape".into(),
            message: "路径超出 workspace 范围".into(),
        }
    }

    fn search(&self, query: &str) -> Result<Vec<String>, ServerMessage> {
        let mut matches = Vec::new();
        for entry in walkdir::WalkDir::new(self.root.as_path())
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(self.root.as_path())
                .unwrap_or(entry.path())
                .display()
                .to_string();
            if let Ok(contents) = std::fs::read_to_string(entry.path()) {
                if contents.contains(query) {
                    matches.push(relative);
                }
            }
            if matches.len() >= 100 {
                break;
            }
        }
        Ok(matches)
    }
}

#[derive(Clone)]
pub struct Engine {
    events: broadcast::Sender<ServerMessage>,
    state: Arc<Mutex<State>>,
    store_path: Option<Arc<PathBuf>>,
    adapter: Option<Arc<dyn PromptAdapter>>,
    tool_adapter: Option<Arc<dyn ToolAdapter>>,
    diff_adapter: Option<Arc<dyn DiffAdapter>>,
    workspace: Option<Arc<WorkspaceAdapter>>,
    settings: Arc<Mutex<GuiSettings>>,
}

#[derive(Clone, Debug, Default)]
struct GuiSettings {
    base_url: Option<String>,
    model: Option<String>,
    has_api_key: bool,
}

impl Engine {
    pub fn new() -> Self {
        Self::with_state(State::default(), None, None, None, None, None)
    }

    pub fn with_adapter(adapter: impl PromptAdapter + 'static) -> Self {
        Self::with_state(
            State::default(),
            None,
            Some(Arc::new(adapter)),
            None,
            None,
            None,
        )
    }

    pub fn with_adapter_arc(adapter: Arc<dyn PromptAdapter>) -> Self {
        Self::with_state(State::default(), None, Some(adapter), None, None, None)
    }

    pub fn with_tool_adapter(adapter: impl ToolAdapter + 'static) -> Self {
        Self::with_state(
            State::default(),
            None,
            None,
            Some(Arc::new(adapter)),
            None,
            None,
        )
    }

    pub fn with_adapters(
        prompt: Option<Arc<dyn PromptAdapter>>,
        tool: Option<Arc<dyn ToolAdapter>>,
    ) -> Self {
        Self::with_state(State::default(), None, prompt, tool, None, None)
    }

    pub fn with_diff_adapter(adapter: impl DiffAdapter + 'static) -> Self {
        Self::with_state(
            State::default(),
            None,
            None,
            None,
            Some(Arc::new(adapter)),
            None,
        )
    }

    pub fn with_workspace(root: impl AsRef<Path>) -> std::io::Result<Self> {
        Self::with_workspace_and_adapter(root, None)
    }

    pub fn with_workspace_and_adapter(
        root: impl AsRef<Path>,
        adapter: Option<Arc<dyn PromptAdapter>>,
    ) -> std::io::Result<Self> {
        let workspace = Arc::new(WorkspaceAdapter::new(root)?);
        Ok(Self::with_state(
            State::default(),
            None,
            adapter,
            None,
            None,
            Some(workspace),
        ))
    }

    pub fn with_persistence(path: impl AsRef<Path>) -> std::io::Result<Self> {
        Self::with_persistence_and_adapters(path, None, None, None)
    }

    pub fn with_persistence_and_adapter(
        path: impl AsRef<Path>,
        adapter: Option<Arc<dyn PromptAdapter>>,
    ) -> std::io::Result<Self> {
        Self::with_persistence_and_adapters(path, adapter, None, None)
    }

    pub fn with_persistence_and_adapters(
        path: impl AsRef<Path>,
        adapter: Option<Arc<dyn PromptAdapter>>,
        tool_adapter: Option<Arc<dyn ToolAdapter>>,
        diff_adapter: Option<Arc<dyn DiffAdapter>>,
    ) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let state = if path.exists() {
            let bytes = std::fs::read(&path)?;
            match serde_json::from_slice::<StateEnvelope>(&bytes) {
                Ok(envelope) if envelope.schema_version == STATE_SCHEMA_VERSION => envelope.state,
                Ok(envelope) if envelope.schema_version > STATE_SCHEMA_VERSION => {
                    return Err(std::io::Error::other(
                        "session state was written by a newer version",
                    ));
                }
                Ok(_) => return Err(std::io::Error::other("unsupported session state schema")),
                Err(_) => {
                    let legacy: State =
                        serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
                    let backup = path.with_extension("json.legacy.bak");
                    std::fs::copy(&path, backup)?;
                    legacy
                }
            }
        } else {
            State::default()
        };
        Ok(Self::with_state(
            state,
            Some(path),
            adapter,
            tool_adapter,
            diff_adapter,
            None,
        ))
    }

    fn with_state(
        state: State,
        path: Option<PathBuf>,
        adapter: Option<Arc<dyn PromptAdapter>>,
        tool_adapter: Option<Arc<dyn ToolAdapter>>,
        diff_adapter: Option<Arc<dyn DiffAdapter>>,
        workspace: Option<Arc<WorkspaceAdapter>>,
    ) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            events,
            state: Arc::new(Mutex::new(state)),
            store_path: path.map(Arc::new),
            adapter,
            tool_adapter,
            diff_adapter,
            workspace,
            settings: Arc::new(Mutex::new(GuiSettings::default())),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ServerMessage> {
        self.events.subscribe()
    }

    fn persist(&self, state: &State) -> Result<(), ServerMessage> {
        let Some(path) = &self.store_path else {
            return Ok(());
        };
        let bytes = serde_json::to_vec_pretty(&StateEnvelope {
            schema_version: STATE_SCHEMA_VERSION,
            state: state.clone(),
        })
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
            | ClientMessage::Reject { client_msg_id, .. }
            | ClientMessage::RespondQuestion { client_msg_id, .. }
            | ClientMessage::AcceptDiff { client_msg_id, .. }
            | ClientMessage::RollbackDiff { client_msg_id, .. }
            | ClientMessage::ListFiles { client_msg_id, .. }
            | ClientMessage::ReadFile { client_msg_id, .. }
            | ClientMessage::SearchFiles { client_msg_id, .. }
            | ClientMessage::ProposeFileWrite { client_msg_id, .. }
            | ClientMessage::GetSettings { client_msg_id }
            | ClientMessage::UpdateSettings { client_msg_id, .. } => client_msg_id.clone(),
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
                if session.pending_question.is_some() {
                    return vec![Self::error("question_pending", "请先回答待处理问题")];
                }
                if let Some(question) = prompt.strip_prefix("/ask ") {
                    let question_id = Uuid::new_v4();
                    session.pending_question = Some(question_id);
                    session.sequence += 1;
                    vec![
                        ServerMessage::Ack { client_msg_id },
                        ServerMessage::QuestionRequested {
                            session_id,
                            question_id,
                            prompt: question.into(),
                            sequence: session.sequence,
                        },
                    ]
                } else if let Some(summary) = prompt.strip_prefix("/approve-tool ") {
                    let request_id = Uuid::new_v4();
                    session.pending_approval = Some(PendingApproval {
                        request_id,
                        tool: "demo.tool".into(),
                        summary: summary.to_string(),
                    });
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
            ClientMessage::RespondQuestion {
                client_msg_id,
                question_id,
                answer,
            } => self.resolve_question(&mut state, client_msg_id, question_id, answer),
            ClientMessage::AcceptDiff {
                client_msg_id,
                session_id,
                proposal_id,
                summary,
            } => self.resolve_diff(
                &mut state,
                client_msg_id,
                session_id,
                proposal_id,
                summary,
                true,
            ),
            ClientMessage::RollbackDiff {
                client_msg_id,
                session_id,
                proposal_id,
            } => self.resolve_diff(
                &mut state,
                client_msg_id,
                session_id,
                proposal_id,
                String::new(),
                false,
            ),
            ClientMessage::ListFiles { relative_path, .. } => match &self.workspace {
                Some(workspace) => workspace
                    .list(&relative_path)
                    .map(|entries| {
                        vec![ServerMessage::FilesListed {
                            path: relative_path,
                            entries,
                        }]
                    })
                    .unwrap_or_else(|error| vec![error]),
                None => vec![Self::error("workspace_unavailable", "没有配置 workspace")],
            },
            ClientMessage::ReadFile { relative_path, .. } => match &self.workspace {
                Some(workspace) => workspace
                    .read(&relative_path)
                    .map(|contents| {
                        vec![ServerMessage::FileContents {
                            path: relative_path,
                            contents,
                        }]
                    })
                    .unwrap_or_else(|error| vec![error]),
                None => vec![Self::error("workspace_unavailable", "没有配置 workspace")],
            },
            ClientMessage::SearchFiles { query, .. } => match &self.workspace {
                Some(workspace) => workspace
                    .search(&query)
                    .map(|matches| vec![ServerMessage::SearchResults { query, matches }])
                    .unwrap_or_else(|error| vec![error]),
                None => vec![Self::error("workspace_unavailable", "没有配置 workspace")],
            },
            ClientMessage::GetSettings { .. } => {
                let settings = self.settings.lock().expect("settings lock").clone();
                vec![ServerMessage::Settings {
                    base_url: settings.base_url,
                    model: settings.model,
                    has_api_key: settings.has_api_key,
                }]
            }
            ClientMessage::UpdateSettings {
                base_url, model, ..
            } => {
                if base_url
                    .as_ref()
                    .is_some_and(|url| url.contains('@') || !url.starts_with("https://"))
                {
                    return vec![Self::error(
                        "invalid_base_url",
                        "Base URL 必须是 https URL 且不能包含凭据",
                    )];
                }
                let mut settings = self.settings.lock().expect("settings lock");
                settings.base_url = base_url.clone();
                settings.model = model.clone();
                vec![ServerMessage::SettingsUpdated { base_url, model }]
            }
            ClientMessage::ProposeFileWrite {
                client_msg_id,
                session_id,
                relative_path,
                contents,
            } => {
                let Some(session) = state.sessions.get_mut(&session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                if contents.len() > 1024 * 1024 {
                    return vec![Self::error("file_too_large", "文件超过 1 MiB 限制")];
                }
                let request_id = Uuid::new_v4();
                session.pending_approval = Some(PendingApproval {
                    request_id,
                    tool: "workspace.write_file".into(),
                    summary: format!("写入 {relative_path}"),
                });
                session
                    .pending_file_writes
                    .insert(request_id, (session_id, relative_path, contents));
                session.sequence += 1;
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::ToolApprovalRequested {
                        session_id,
                        request_id,
                        tool: "workspace.write_file".into(),
                        summary: "请求写入 workspace 文件".into(),
                        sequence: session.sequence,
                    },
                ]
            }
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
        let Some((session_id, session)) = state.sessions.iter_mut().find(|(_, session)| {
            session
                .pending_approval
                .as_ref()
                .is_some_and(|pending| pending.request_id == request_id)
        }) else {
            return vec![Self::error("approval_not_found", "审批请求不存在或已处理")];
        };
        let pending = session
            .pending_approval
            .take()
            .expect("matched pending approval");
        let pending_file_write = session.pending_file_writes.remove(&request_id);
        let mut events = vec![ServerMessage::Ack { client_msg_id }];
        let outcome = if let Some((write_session_id, relative_path, contents)) = pending_file_write
        {
            if !approved {
                "rejected"
            } else if let Some(workspace) = &self.workspace {
                match workspace.write(&relative_path, &contents) {
                    Ok(bytes) => {
                        session.messages.push(TimelineMessage {
                            role: "tool".into(),
                            text: format!("wrote {relative_path}"),
                        });
                        session.sequence += 1;
                        events.push(ServerMessage::FileWritten {
                            session_id: write_session_id,
                            path: relative_path,
                            bytes,
                        });
                        "executed"
                    }
                    Err(error) => {
                        session.sequence += 1;
                        events.push(error);
                        "failed"
                    }
                }
            } else {
                session.sequence += 1;
                events.push(ServerMessage::Error {
                    code: "workspace_unavailable".into(),
                    message: "没有配置 workspace".into(),
                });
                "unavailable"
            }
        } else if approved {
            match &self.tool_adapter {
                Some(adapter) => match adapter.execute(&pending.tool, &pending.summary) {
                    Ok(result) => {
                        session.messages.push(TimelineMessage {
                            role: "tool".into(),
                            text: result.clone(),
                        });
                        session.sequence += 1;
                        events.push(ServerMessage::TextDelta {
                            session_id: *session_id,
                            text: result,
                            sequence: session.sequence,
                        });
                        "executed"
                    }
                    Err(error) => {
                        session.sequence += 1;
                        events.push(ServerMessage::Error {
                            code: "tool_failed".into(),
                            message: error,
                        });
                        "failed"
                    }
                },
                None => {
                    session.sequence += 1;
                    events.push(ServerMessage::Error {
                        code: "tool_unavailable".into(),
                        message: "没有配置获准的工具 adapter".into(),
                    });
                    "unavailable"
                }
            }
        } else {
            "rejected"
        };
        session.sequence += 1;
        session.audit.push(AuditEntry {
            action: "tool_approval".into(),
            outcome: outcome.into(),
            sequence: session.sequence,
        });
        events.push(ServerMessage::ApprovalResolved {
            session_id: *session_id,
            request_id,
            approved: outcome == "executed",
            sequence: session.sequence,
        });
        events.push(ServerMessage::Audit {
            session_id: *session_id,
            action: "tool_approval".into(),
            outcome: outcome.into(),
            sequence: session.sequence,
        });
        events
    }
    fn resolve_question(
        &self,
        state: &mut State,
        client_msg_id: String,
        question_id: Uuid,
        answer: String,
    ) -> Vec<ServerMessage> {
        let Some((session_id, session)) = state
            .sessions
            .iter_mut()
            .find(|(_, session)| session.pending_question == Some(question_id))
        else {
            return vec![Self::error("question_not_found", "问题不存在或已回答")];
        };
        session.pending_question = None;
        session.sequence += 1;
        session.audit.push(AuditEntry {
            action: "question".into(),
            outcome: "answered".into(),
            sequence: session.sequence,
        });
        vec![
            ServerMessage::Ack { client_msg_id },
            ServerMessage::QuestionResolved {
                session_id: *session_id,
                question_id,
                answer,
                sequence: session.sequence,
            },
            ServerMessage::Audit {
                session_id: *session_id,
                action: "question".into(),
                outcome: "answered".into(),
                sequence: session.sequence,
            },
        ]
    }

    fn resolve_diff(
        &self,
        state: &mut State,
        client_msg_id: String,
        session_id: Uuid,
        proposal_id: String,
        summary: String,
        accept: bool,
    ) -> Vec<ServerMessage> {
        let Some(session) = state.sessions.get_mut(&session_id) else {
            return vec![Self::error("session_not_found", "会话不存在")];
        };
        let result = match &self.diff_adapter {
            Some(adapter) => {
                if accept {
                    adapter.accept(&proposal_id, &summary)
                } else {
                    adapter.rollback(&proposal_id)
                }
            }
            None => Err("没有配置 Diff adapter".into()),
        };
        session.sequence += 1;
        let action = if accept {
            "accept_diff"
        } else {
            "rollback_diff"
        };
        let outcome = if result.is_ok() {
            "applied"
        } else {
            "rejected"
        };
        let mut events = vec![ServerMessage::Ack { client_msg_id }];
        if let Err(message) = result {
            events.push(ServerMessage::Error {
                code: "diff_failed".into(),
                message,
            });
        }
        events.push(ServerMessage::DiffResolved {
            proposal_id,
            action: action.into(),
            sequence: session.sequence,
        });
        session.audit.push(AuditEntry {
            action: action.into(),
            outcome: outcome.into(),
            sequence: session.sequence,
        });
        events.push(ServerMessage::Audit {
            session_id,
            action: action.into(),
            outcome: outcome.into(),
            sequence: session.sequence,
        });
        events
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
    fn persistent_state_has_schema_and_rejects_newer_versions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sessions.json");
        let engine = Engine::with_persistence(&path).unwrap();
        engine.handle(ClientMessage::CreateSession {
            client_msg_id: "schema-create".into(),
        });
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["schema_version"], STATE_SCHEMA_VERSION);
        value.as_object().unwrap();
        std::fs::write(
            &path,
            serde_json::json!({ "schema_version": STATE_SCHEMA_VERSION + 1, "state": {} })
                .to_string(),
        )
        .unwrap();
        assert!(Engine::with_persistence(&path).is_err());
    }

    #[test]
    fn legacy_state_is_backed_up_before_loading() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sessions.json");
        std::fs::write(&path, serde_json::to_vec(&State::default()).unwrap()).unwrap();
        Engine::with_persistence(&path).unwrap();
        assert!(path.with_extension("json.legacy.bak").exists());
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

    struct FixtureTool;

    impl ToolAdapter for FixtureTool {
        fn execute(&self, tool: &str, summary: &str) -> Result<String, String> {
            Ok(format!("{tool}:{summary}"))
        }
    }

    #[test]
    fn approved_tool_runs_only_through_tool_adapter() {
        let engine = Engine::with_tool_adapter(FixtureTool);
        let id = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "create-tool".into(),
        })[0]
        {
            ServerMessage::SessionCreated { session_id } => session_id,
            _ => panic!(),
        };
        let requested = engine.handle(ClientMessage::Submit {
            client_msg_id: "request-tool".into(),
            session_id: id,
            prompt: "/approve-tool write".into(),
        });
        let request_id = match requested[1] {
            ServerMessage::ToolApprovalRequested { request_id, .. } => request_id,
            _ => panic!(),
        };
        let resolved = engine.handle(ClientMessage::Approve {
            client_msg_id: "approve-tool".into(),
            request_id,
        });
        assert!(resolved.iter().any(|event| matches!(event, ServerMessage::TextDelta { text, .. } if text == "demo.tool:write")));
        assert!(resolved.iter().any(|event| matches!(
            event,
            ServerMessage::ApprovalResolved { approved: true, .. }
        )));
    }

    #[test]
    fn approval_without_tool_adapter_fails_closed() {
        let engine = Engine::new();
        let id = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "create-tool".into(),
        })[0]
        {
            ServerMessage::SessionCreated { session_id } => session_id,
            _ => panic!(),
        };
        let requested = engine.handle(ClientMessage::Submit {
            client_msg_id: "request-tool".into(),
            session_id: id,
            prompt: "/approve-tool write".into(),
        });
        let request_id = match requested[1] {
            ServerMessage::ToolApprovalRequested { request_id, .. } => request_id,
            _ => panic!(),
        };
        let resolved = engine.handle(ClientMessage::Approve {
            client_msg_id: "approve-tool".into(),
            request_id,
        });
        assert!(resolved.iter().any(
            |event| matches!(event, ServerMessage::Error { code, .. } if code == "tool_unavailable")
        ));
    }

    struct FixtureDiff;

    impl DiffAdapter for FixtureDiff {
        fn accept(&self, proposal_id: &str, summary: &str) -> Result<(), String> {
            if proposal_id == "p1" && summary == "safe" {
                Ok(())
            } else {
                Err("unexpected proposal".into())
            }
        }
        fn rollback(&self, proposal_id: &str) -> Result<(), String> {
            if proposal_id == "p1" {
                Ok(())
            } else {
                Err("unknown proposal".into())
            }
        }
    }

    #[test]
    fn diff_accept_and_rollback_use_session_scoped_adapter() {
        let engine = Engine::with_diff_adapter(FixtureDiff);
        let session_id = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "diff-create".into(),
        })[0]
        {
            ServerMessage::SessionCreated { session_id } => session_id,
            _ => panic!(),
        };
        let accepted = engine.handle(ClientMessage::AcceptDiff {
            client_msg_id: "diff-accept".into(),
            session_id,
            proposal_id: "p1".into(),
            summary: "safe".into(),
        });
        assert!(accepted.iter().any(|event| matches!(event, ServerMessage::DiffResolved { action, .. } if action == "accept_diff")));
        let rolled_back = engine.handle(ClientMessage::RollbackDiff {
            client_msg_id: "diff-rollback".into(),
            session_id,
            proposal_id: "p1".into(),
        });
        assert!(rolled_back.iter().any(|event| matches!(event, ServerMessage::DiffResolved { action, .. } if action == "rollback_diff")));
    }

    #[cfg(unix)]
    #[test]
    fn workspace_symlink_escape_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.txt"),
            directory.path().join("link.txt"),
        )
        .unwrap();
        let engine = Engine::with_workspace(directory.path()).unwrap();
        let result = engine.handle(ClientMessage::ReadFile {
            client_msg_id: "symlink".into(),
            relative_path: "link.txt".into(),
        });
        assert!(matches!(&result[0], ServerMessage::Error { code, .. } if code == "path_escape"));
    }

    #[test]
    fn settings_never_return_api_key_and_reject_unsafe_base_urls() {
        let engine = Engine::new();
        let updated = engine.handle(ClientMessage::UpdateSettings {
            client_msg_id: "settings".into(),
            base_url: Some("https://api.example.test/v1".into()),
            model: Some("demo".into()),
        });
        assert!(matches!(updated[0], ServerMessage::SettingsUpdated { .. }));
        let settings = engine.handle(ClientMessage::GetSettings {
            client_msg_id: "get-settings".into(),
        });
        assert!(
            matches!(&settings[0], ServerMessage::Settings { has_api_key: false, model, .. } if model.as_deref() == Some("demo"))
        );
        let unsafe_url = engine.handle(ClientMessage::UpdateSettings {
            client_msg_id: "unsafe-settings".into(),
            base_url: Some("http://user:pass@example.test".into()),
            model: None,
        });
        assert!(
            matches!(&unsafe_url[0], ServerMessage::Error { code, .. } if code == "invalid_base_url")
        );
    }

    #[test]
    fn workspace_root_confinement_rejects_escape_and_reads_files() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("hello.txt"), "needle").unwrap();
        let engine = Engine::with_workspace(directory.path()).unwrap();
        let listed = engine.handle(ClientMessage::ListFiles {
            client_msg_id: "list".into(),
            relative_path: ".".into(),
        });
        assert!(
            matches!(&listed[0], ServerMessage::FilesListed { entries, .. } if entries == &["hello.txt"])
        );
        let read = engine.handle(ClientMessage::ReadFile {
            client_msg_id: "read".into(),
            relative_path: "hello.txt".into(),
        });
        assert!(
            matches!(&read[0], ServerMessage::FileContents { contents, .. } if contents == "needle")
        );
        let escaped = engine.handle(ClientMessage::ReadFile {
            client_msg_id: "escape".into(),
            relative_path: "../outside".into(),
        });
        assert!(
            matches!(&escaped[0], ServerMessage::Error { code, .. } if code == "path_invalid" || code == "path_escape")
        );
        let session_id = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "write-session".into(),
        })[0]
        {
            ServerMessage::SessionCreated { session_id } => session_id,
            _ => panic!(),
        };
        let proposed = engine.handle(ClientMessage::ProposeFileWrite {
            client_msg_id: "write".into(),
            session_id,
            relative_path: "new.txt".into(),
            contents: "written".into(),
        });
        let request_id = match proposed[1] {
            ServerMessage::ToolApprovalRequested { request_id, .. } => request_id,
            _ => panic!(),
        };
        let written = engine.handle(ClientMessage::Approve {
            client_msg_id: "write-approve".into(),
            request_id,
        });
        assert!(written.iter().any(
            |event| matches!(event, ServerMessage::FileWritten { path, .. } if path == "new.txt")
        ));
        assert_eq!(
            std::fs::read_to_string(directory.path().join("new.txt")).unwrap(),
            "written"
        );
        let escaped_write = engine.handle(ClientMessage::ProposeFileWrite {
            client_msg_id: "escape-write".into(),
            session_id,
            relative_path: "../outside".into(),
            contents: "bad".into(),
        });
        let escape_request = match escaped_write[1] {
            ServerMessage::ToolApprovalRequested { request_id, .. } => request_id,
            _ => panic!(),
        };
        let rejected = engine.handle(ClientMessage::Reject {
            client_msg_id: "escape-reject".into(),
            request_id: escape_request,
            reason: "deny".into(),
        });
        assert!(rejected.iter().any(|event| matches!(
            event,
            ServerMessage::ApprovalResolved {
                approved: false,
                ..
            }
        )));
        assert!(!directory.path().join("../outside").exists());
        let search = engine.handle(ClientMessage::SearchFiles {
            client_msg_id: "search".into(),
            query: "needle".into(),
        });
        assert!(
            matches!(&search[0], ServerMessage::SearchResults { matches, .. } if matches.iter().any(|path| path == "hello.txt"))
        );
    }

    #[test]
    fn question_requires_and_resolves_explicit_answer() {
        let engine = Engine::new();
        let session_id = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "q-create".into(),
        })[0]
        {
            ServerMessage::SessionCreated { session_id } => session_id,
            _ => panic!(),
        };
        let requested = engine.handle(ClientMessage::Submit {
            client_msg_id: "q-submit".into(),
            session_id,
            prompt: "/ask choose".into(),
        });
        let question_id = match requested[1] {
            ServerMessage::QuestionRequested { question_id, .. } => question_id,
            _ => panic!(),
        };
        let resolved = engine.handle(ClientMessage::RespondQuestion {
            client_msg_id: "q-answer".into(),
            question_id,
            answer: "yes".into(),
        });
        assert!(resolved.iter().any(|event| matches!(event, ServerMessage::QuestionResolved { answer, .. } if answer == "yes")));
    }

    #[test]
    fn diff_without_adapter_fails_closed() {
        let engine = Engine::new();
        let session_id = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "diff-create".into(),
        })[0]
        {
            ServerMessage::SessionCreated { session_id } => session_id,
            _ => panic!(),
        };
        let events = engine.handle(ClientMessage::AcceptDiff {
            client_msg_id: "diff-accept".into(),
            session_id,
            proposal_id: "p1".into(),
            summary: "safe".into(),
        });
        assert!(events.iter().any(
            |event| matches!(event, ServerMessage::Error { code, .. } if code == "diff_failed")
        ));
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
