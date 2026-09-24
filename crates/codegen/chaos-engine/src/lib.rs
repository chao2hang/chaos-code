use base64::Engine as Base64Engine;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;
use uuid::Uuid;

pub mod protocol_schema;
pub mod pty;
pub mod remote;

pub const PROTOCOL_VERSION: u16 = 1;
pub const STATE_SCHEMA_VERSION: u16 = 1;
const SQLITE_SCHEMA_VERSION: u16 = 1;
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
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DiffPreview {
    pub proposal_id: String,
    pub path: String,
    pub before: Option<String>,
    pub after: String,
}

pub trait DiffAdapter: Send + Sync {
    fn accept(&self, proposal_id: &str, summary: &str) -> Result<(), String>;
    fn rollback(&self, proposal_id: &str) -> Result<(), String>;
    fn preview(&self, _proposal_id: &str) -> Result<DiffPreview, String> {
        Err("Diff adapter 不支持预览".into())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalResult {
    pub output: String,
    pub exit_code: i32,
}

pub trait TerminalAdapter: Send + Sync {
    fn run(&self, command: &str) -> Result<TerminalResult, String>;
}

pub trait GitAdapter: Send + Sync {
    fn stage(&self, path: &str) -> Result<(), String>;
    fn unstage(&self, path: &str) -> Result<(), String>;
    fn commit(&self, message: &str) -> Result<String, String>;
    fn checkout_branch(&self, branch: &str) -> Result<(), String>;
    fn discard(&self, path: &str) -> Result<(), String>;
}

pub struct ProcessGitAdapter {
    cwd: PathBuf,
}

impl ProcessGitAdapter {
    pub fn new(cwd: impl AsRef<Path>) -> std::io::Result<Self> {
        let cwd = std::fs::canonicalize(cwd)?;
        if !cwd.is_dir() {
            return Err(std::io::Error::other("git cwd is not a directory"));
        }
        Ok(Self { cwd })
    }

    fn run(&self, args: &[&str]) -> Result<String, String> {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&self.cwd)
            .args(args)
            .output()
            .map_err(|error| format!("无法启动 git: {error}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if stderr.is_empty() {
                format!("git exited with {}", output.status)
            } else {
                stderr
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn safe_path(path: &str) -> bool {
        !path.is_empty()
            && !path.contains('\0')
            && !Path::new(path).is_absolute()
            && !Path::new(path)
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
    }

    fn safe_branch(branch: &str) -> bool {
        !branch.is_empty()
            && !branch.starts_with('-')
            && !branch.contains('\0')
            && !branch.chars().any(char::is_whitespace)
            && !branch.contains("..")
    }
}

impl GitAdapter for ProcessGitAdapter {
    fn stage(&self, path: &str) -> Result<(), String> {
        if !Self::safe_path(path) {
            return Err("git path rejected".into());
        }
        self.run(&["add", "--", path]).map(|_| ())
    }

    fn unstage(&self, path: &str) -> Result<(), String> {
        if !Self::safe_path(path) {
            return Err("git path rejected".into());
        }
        self.run(&["restore", "--staged", "--", path]).map(|_| ())
    }

    fn commit(&self, message: &str) -> Result<String, String> {
        if message.trim().is_empty() || message.contains('\0') {
            return Err("git commit message rejected".into());
        }
        self.run(&["commit", "-m", message])?;
        self.run(&["rev-parse", "HEAD"])
    }

    fn checkout_branch(&self, branch: &str) -> Result<(), String> {
        if !Self::safe_branch(branch) {
            return Err("git branch rejected".into());
        }
        self.run(&["switch", branch]).map(|_| ())
    }

    fn discard(&self, path: &str) -> Result<(), String> {
        if !Self::safe_path(path) {
            return Err("git path rejected".into());
        }
        self.run(&["restore", "--worktree", "--", path]).map(|_| ())
    }
}

struct AttachmentUpload {
    #[allow(dead_code)]
    session_id: Uuid,
    #[allow(dead_code)]
    filename: String,
    #[allow(dead_code)]
    content_type: String,
    expected: u64,
    chunks: Vec<Vec<u8>>,
    received: u64,
}

pub struct AttachmentStager {
    root: Arc<PathBuf>,
    max_bytes: u64,
}

impl AttachmentStager {
    pub fn validate_name_type_size(
        filename: &str,
        content_type: &str,
        byte_len: u64,
    ) -> Result<(), String> {
        let allowed = [
            (".txt", "text/plain"),
            (".md", "text/markdown"),
            (".json", "application/json"),
            (".png", "image/png"),
            (".jpg", "image/jpeg"),
            (".jpeg", "image/jpeg"),
            (".pdf", "application/pdf"),
        ];
        let valid_name = Path::new(filename)
            .file_name()
            .and_then(|name| name.to_str())
            == Some(filename)
            && !filename.is_empty();
        let valid_type = allowed.iter().any(|(ext, mime)| {
            filename.to_ascii_lowercase().ends_with(ext) && *mime == content_type
        });
        if !valid_name || !valid_type || byte_len == 0 || byte_len > 10 * 1024 * 1024 {
            return Err("附件类型、名称或大小不符合允许策略".into());
        }
        Ok(())
    }

    pub fn new(root: impl AsRef<Path>, max_bytes: u64) -> std::io::Result<Self> {
        let root = std::fs::canonicalize(root)?;
        std::fs::create_dir_all(root.join(".chaos-staging"))?;
        Ok(Self {
            root: Arc::new(root),
            max_bytes,
        })
    }

    pub fn stage_chunks<I>(
        &self,
        filename: &str,
        content_type: &str,
        chunks: I,
    ) -> Result<PathBuf, String>
    where
        I: IntoIterator<Item = Result<Vec<u8>, String>>,
    {
        let safe_name = Path::new(filename)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "attachment filename is invalid".to_string())?;
        if safe_name != filename || safe_name.is_empty() {
            return Err("attachment path escape rejected".into());
        }
        Self::validate_name_type_size(safe_name, content_type, 1)?;
        let target = self
            .root
            .join(".chaos-staging")
            .join(format!("{safe_name}.part-{}", Uuid::new_v4()));
        let mut file = std::fs::File::create(&target).map_err(|e| e.to_string())?;
        let mut total = 0u64;
        for chunk in chunks {
            let chunk = chunk?;
            total = total.saturating_add(chunk.len() as u64);
            if total > self.max_bytes {
                let _ = std::fs::remove_file(&target);
                return Err("attachment exceeds size limit".into());
            }
            use std::io::Write;
            file.write_all(&chunk).map_err(|e| {
                let _ = std::fs::remove_file(&target);
                e.to_string()
            })?;
        }
        file.sync_all().map_err(|e| {
            let _ = std::fs::remove_file(&target);
            e.to_string()
        })?;
        Ok(target)
    }
}

pub struct ProcessTerminalAdapter {
    cwd: PathBuf,
    max_output_bytes: usize,
}

impl ProcessTerminalAdapter {
    pub fn new(cwd: impl AsRef<Path>, max_output_bytes: usize) -> std::io::Result<Self> {
        let cwd = std::fs::canonicalize(cwd)?;
        if !cwd.is_dir() {
            return Err(std::io::Error::other("terminal cwd is not a directory"));
        }
        Ok(Self {
            cwd,
            max_output_bytes,
        })
    }
}

impl TerminalAdapter for ProcessTerminalAdapter {
    fn run(&self, command: &str) -> Result<TerminalResult, String> {
        if command.trim().is_empty() || command.contains('\0') {
            return Err("terminal command is empty or invalid".into());
        }
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(&self.cwd)
            .output()
            .map_err(|error| format!("无法启动终端命令: {error}"))?;
        let mut text = String::from_utf8_lossy(&output.stdout).to_string();
        if text.len() > self.max_output_bytes {
            text.truncate(self.max_output_bytes);
            text.push_str("\n[output truncated]");
        }
        if !output.status.success() && text.is_empty() {
            text = String::from_utf8_lossy(&output.stderr).to_string();
        }
        Ok(TerminalResult {
            output: text,
            exit_code: output.status.code().unwrap_or(-1),
        })
    }
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
        workspace_id: Option<Uuid>,
    },
    CreateWorkspace {
        client_msg_id: String,
        name: String,
    },
    ListWorkspaces {
        client_msg_id: String,
    },
    ArchiveWorkspace {
        client_msg_id: String,
        workspace_id: Uuid,
    },
    SwitchWorkspace {
        client_msg_id: String,
        workspace_id: Uuid,
    },
    Resume {
        client_msg_id: String,
        session_id: Uuid,
        #[serde(default)]
        workspace_id: Option<Uuid>,
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
        #[serde(default)]
        workspace_id: Option<Uuid>,
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
    ProposeTerminal {
        client_msg_id: String,
        session_id: Uuid,
        command: String,
    },
    ProposeGitMutation {
        client_msg_id: String,
        session_id: Uuid,
        operation: String,
        argument: String,
    },
    GetSettings {
        client_msg_id: String,
    },
    UpdateSettings {
        client_msg_id: String,
        base_url: Option<String>,
        model: Option<String>,
    },
    GetGitStatus {
        client_msg_id: String,
    },
    ValidateAttachment {
        client_msg_id: String,
        filename: String,
        byte_len: u64,
        content_type: String,
    },
    BeginAttachment {
        client_msg_id: String,
        session_id: Uuid,
        filename: String,
        content_type: String,
        byte_len: u64,
    },
    AttachmentChunk {
        client_msg_id: String,
        upload_id: Uuid,
        chunk: String,
    },
    CancelAttachment {
        client_msg_id: String,
        upload_id: Uuid,
    },
    FinalizeAttachment {
        client_msg_id: String,
        upload_id: Uuid,
        relative_path: String,
    },
    ImportTuiSession {
        client_msg_id: String,
        root: String,
        session_id: String,
    },
    ValidateProvider {
        client_msg_id: String,
        base_url: String,
        model: String,
    },
    ScanMarketplace {
        client_msg_id: String,
        root: String,
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
    PreviewDiff {
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
        workspace_id: Uuid,
    },
    Workspaces {
        active_workspace_id: Uuid,
        workspaces: Vec<WorkspaceInfo>,
    },
    WorkspaceArchived {
        workspace_id: Uuid,
    },
    WorkspaceSwitched {
        workspace_id: Uuid,
    },
    SessionSnapshot {
        session_id: Uuid,
        workspace_id: Option<Uuid>,
        messages: Vec<TimelineMessage>,
        sequence: u64,
        pending_approval: Option<PendingApprovalSnapshot>,
        pending_question: Option<QuestionSnapshot>,
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
    DiffPreview {
        session_id: Uuid,
        preview: DiffPreview,
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
    TerminalResult {
        session_id: Uuid,
        output: String,
        exit_code: i32,
    },
    GitMutationResult {
        session_id: Uuid,
        operation: String,
        result: String,
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
    GitStatus {
        branch: Option<String>,
        entries: Vec<String>,
    },
    AttachmentValidated {
        filename: String,
        byte_len: u64,
        content_type: String,
    },
    AttachmentStarted {
        session_id: Uuid,
        upload_id: Uuid,
        filename: String,
    },
    AttachmentProgress {
        upload_id: Uuid,
        received: u64,
    },
    AttachmentCompleted {
        session_id: Uuid,
        upload_id: Uuid,
        path: String,
        bytes: u64,
    },
    AttachmentCancelled {
        upload_id: Uuid,
    },
    ProviderValidation {
        base_url: String,
        model: String,
        reachable: bool,
        error_code: Option<String>,
    },
    MarketplaceScan {
        entries: Vec<serde_json::Value>,
        catalog_loaded: bool,
    },
    TuiSessionImport {
        session_id: String,
        cwd: String,
        title: Option<String>,
        message_count: usize,
        source_unchanged: bool,
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

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct WorkspaceInfo {
    pub id: Uuid,
    pub name: String,
    pub archived: bool,
    pub last_used_sequence: u64,
    #[serde(default)]
    pub last_session_id: Option<Uuid>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct AuditEntry {
    pub action: String,
    pub outcome: String,
    pub sequence: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct QuestionSnapshot {
    pub question_id: Uuid,
    pub prompt: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PendingApprovalSnapshot {
    pub request_id: Uuid,
    pub tool: String,
    pub summary: String,
    pub confirmations_required: u8,
    pub confirmations: u8,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct PendingApproval {
    request_id: Uuid,
    tool: String,
    summary: String,
    #[serde(default)]
    confirmations_required: u8,
    #[serde(default)]
    confirmations: u8,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct SessionState {
    #[serde(default)]
    workspace_id: Option<Uuid>,
    messages: Vec<TimelineMessage>,
    audit: Vec<AuditEntry>,
    sequence: u64,
    pending_approval: Option<PendingApproval>,
    pending_question: Option<Uuid>,
    #[serde(default)]
    pending_question_prompt: Option<String>,
    pending_file_writes: HashMap<Uuid, (Uuid, String, String)>,
    pending_attachment_moves: HashMap<Uuid, (Uuid, Uuid, String)>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct State {
    sessions: HashMap<Uuid, SessionState>,
    seen_client_messages: HashSet<String>,
    workspaces: HashMap<Uuid, WorkspaceInfo>,
    active_workspace_id: Option<Uuid>,
    #[serde(default)]
    settings: GuiSettings,
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
        std::fs::create_dir_all(root.join(".chaos-staging"))?;
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
            .filter(|entry| entry.file_name() != ".chaos-staging")
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

    fn git_status(&self) -> Result<(Option<String>, Vec<String>), ServerMessage> {
        let output = std::process::Command::new("git")
            .args([
                "-C",
                self.root.to_string_lossy().as_ref(),
                "status",
                "--porcelain=v1",
                "--branch",
            ])
            .output()
            .map_err(|_| ServerMessage::Error {
                code: "git_failed".into(),
                message: "无法启动 git".into(),
            })?;
        if !output.status.success() {
            return Err(ServerMessage::Error {
                code: "git_failed".into(),
                message: "workspace 不是可用 Git 仓库".into(),
            });
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let mut lines = text.lines();
        let branch = lines
            .next()
            .and_then(|line| line.strip_prefix("## "))
            .map(str::to_string);
        Ok((branch, lines.map(str::to_string).collect()))
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
pub struct SqliteSessionStore {
    path: Arc<PathBuf>,
}

impl SqliteSessionStore {
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mode = xai_sqlite_journal::JournalMode::for_db_path(&path);
        let conn = mode.open(&path)?;
        conn.execute_batch("CREATE TABLE IF NOT EXISTS gui_meta (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS gui_sessions (id TEXT PRIMARY KEY NOT NULL, payload TEXT NOT NULL);")?;
        let current: Option<String> = conn
            .query_row(
                "SELECT value FROM gui_meta WHERE key = 'schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let current = current
            .map(|version| {
                version
                    .parse::<u16>()
                    .map_err(|_| rusqlite::Error::InvalidParameterName("invalid GUI schema".into()))
            })
            .transpose()?;
        match current {
            Some(version) if version > SQLITE_SCHEMA_VERSION => {
                return Err(rusqlite::Error::InvalidParameterName(
                    "newer GUI schema".into(),
                ));
            }
            Some(version) if version < SQLITE_SCHEMA_VERSION => {
                let backup = path.with_extension(format!("schema-v{version}.bak"));
                drop(conn);
                std::fs::copy(&path, &backup)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
                let conn = mode.open(&path)?;
                if let Err(error) = (|| {
                    let transaction = conn.unchecked_transaction()?;
                    transaction.execute(
                        "UPDATE gui_meta SET value = ?1 WHERE key = 'schema_version'",
                        [SQLITE_SCHEMA_VERSION.to_string()],
                    )?;
                    transaction.commit()
                })() {
                    drop(conn);
                    std::fs::copy(&backup, &path).map_err(|restore_error| {
                        rusqlite::Error::ToSqlConversionFailure(Box::new(restore_error))
                    })?;
                    return Err(error);
                }
            }
            None => {
                conn.execute(
                    "INSERT INTO gui_meta(key,value) VALUES('schema_version', ?1)",
                    [SQLITE_SCHEMA_VERSION.to_string()],
                )?;
            }
            Some(_) => {}
        }
        Ok(Self {
            path: Arc::new(path),
        })
    }

    pub fn save(&self, session_id: Uuid, payload: &str) -> rusqlite::Result<()> {
        let mode = xai_sqlite_journal::JournalMode::for_db_path(self.path.as_path());
        let conn = mode.open(self.path.as_path())?;
        conn.execute("INSERT INTO gui_sessions(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload", rusqlite::params![session_id.to_string(), payload])?;
        Ok(())
    }

    fn load_state(&self) -> rusqlite::Result<State> {
        let mode = xai_sqlite_journal::JournalMode::for_db_path(self.path.as_path());
        let conn = mode.open_readonly(self.path.as_path())?;
        let payload: Option<String> = conn
            .query_row(
                "SELECT payload FROM gui_sessions ORDER BY rowid DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        match payload {
            Some(payload) => {
                let envelope: StateEnvelope = serde_json::from_str(&payload)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
                if envelope.schema_version != STATE_SCHEMA_VERSION {
                    return Err(rusqlite::Error::InvalidParameterName(
                        "unsupported GUI state schema".into(),
                    ));
                }
                Ok(envelope.state)
            }
            None => Ok(State::default()),
        }
    }

    pub fn load(&self, session_id: Uuid) -> rusqlite::Result<Option<String>> {
        let mode = xai_sqlite_journal::JournalMode::for_db_path(self.path.as_path());
        let conn = mode.open_readonly(self.path.as_path())?;
        conn.query_row(
            "SELECT payload FROM gui_sessions WHERE id = ?1",
            [session_id.to_string()],
            |row| row.get(0),
        )
        .optional()
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
    sqlite_store: Option<Arc<SqliteSessionStore>>,
    attachments: Arc<Mutex<HashMap<Uuid, AttachmentUpload>>>,
    settings: Arc<Mutex<GuiSettings>>,
    terminal_adapter: Option<Arc<dyn TerminalAdapter>>,
    git_adapter: Option<Arc<dyn GitAdapter>>,
    marketplace_roots: Arc<Vec<PathBuf>>,
    tui_session_roots: Arc<Vec<PathBuf>>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct GuiSettings {
    base_url: Option<String>,
    model: Option<String>,
    has_api_key: bool,
}

impl Engine {
    pub fn new() -> Self {
        Self::with_state(State::default(), None, None, None, None, None, None)
    }

    pub fn with_terminal_adapter(mut self, adapter: impl TerminalAdapter + 'static) -> Self {
        self.terminal_adapter = Some(Arc::new(adapter));
        self
    }

    pub fn with_git_adapter(mut self, adapter: impl GitAdapter + 'static) -> Self {
        self.git_adapter = Some(Arc::new(adapter));
        self
    }

    pub fn with_sqlite_store(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let store = Arc::new(SqliteSessionStore::open(path)?);
        let state = store.load_state()?;
        Ok(Self::with_state(
            state,
            None,
            None,
            None,
            None,
            None,
            Some(store),
        ))
    }

    pub fn with_adapter(adapter: impl PromptAdapter + 'static) -> Self {
        Self::with_state(
            State::default(),
            None,
            Some(Arc::new(adapter)),
            None,
            None,
            None,
            None,
        )
    }

    pub fn with_adapter_arc(adapter: Arc<dyn PromptAdapter>) -> Self {
        Self::with_state(
            State::default(),
            None,
            Some(adapter),
            None,
            None,
            None,
            None,
        )
    }

    pub fn with_tool_adapter(adapter: impl ToolAdapter + 'static) -> Self {
        Self::with_state(
            State::default(),
            None,
            None,
            Some(Arc::new(adapter)),
            None,
            None,
            None,
        )
    }

    pub fn with_adapters(
        prompt: Option<Arc<dyn PromptAdapter>>,
        tool: Option<Arc<dyn ToolAdapter>>,
    ) -> Self {
        Self::with_state(State::default(), None, prompt, tool, None, None, None)
    }

    pub fn with_diff_adapter(adapter: impl DiffAdapter + 'static) -> Self {
        Self::with_state(
            State::default(),
            None,
            None,
            None,
            Some(Arc::new(adapter)),
            None,
            None,
        )
    }

    pub fn with_workspace(root: impl AsRef<Path>) -> std::io::Result<Self> {
        Self::with_workspace_and_adapter(root, None)
    }

    pub fn with_workspace_and_diff_adapter(
        root: impl AsRef<Path>,
        adapter: impl DiffAdapter + 'static,
    ) -> std::io::Result<Self> {
        let workspace = Arc::new(WorkspaceAdapter::new(root)?);
        Ok(Self::with_state(
            State::default(),
            None,
            None,
            None,
            Some(Arc::new(adapter)),
            Some(workspace),
            None,
        ))
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
            None,
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
        sqlite_store: Option<Arc<SqliteSessionStore>>,
    ) -> Self {
        let initial_settings = state.settings.clone();
        let (events, _) = broadcast::channel(256);
        Self {
            events,
            state: Arc::new(Mutex::new(state)),
            store_path: path.map(Arc::new),
            adapter,
            tool_adapter,
            diff_adapter,
            workspace,
            sqlite_store,
            attachments: Arc::new(Mutex::new(HashMap::new())),
            settings: Arc::new(Mutex::new(initial_settings)),
            terminal_adapter: None,
            git_adapter: None,
            marketplace_roots: Arc::new(Vec::new()),
            tui_session_roots: Arc::new(Vec::new()),
        }
    }

    pub fn with_tui_session_root(mut self, root: impl AsRef<Path>) -> std::io::Result<Self> {
        let root = std::fs::canonicalize(root)?;
        if !root.is_dir() {
            return Err(std::io::Error::other("TUI session root is not a directory"));
        }
        let mut roots = (*self.tui_session_roots).clone();
        if !roots.contains(&root) {
            roots.push(root);
        }
        self.tui_session_roots = Arc::new(roots);
        Ok(self)
    }

    pub fn with_marketplace_root(mut self, root: impl AsRef<Path>) -> std::io::Result<Self> {
        let root = std::fs::canonicalize(root)?;
        if !root.is_dir() {
            return Err(std::io::Error::other("marketplace root is not a directory"));
        }
        let mut roots = (*self.marketplace_roots).clone();
        if !roots.contains(&root) {
            roots.push(root);
        }
        self.marketplace_roots = Arc::new(roots);
        Ok(self)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ServerMessage> {
        self.events.subscribe()
    }

    fn persist(&self, state: &State) -> Result<(), ServerMessage> {
        if let Some(store) = &self.sqlite_store {
            let payload = serde_json::to_string(&StateEnvelope {
                schema_version: STATE_SCHEMA_VERSION,
                state: state.clone(),
            })
            .map_err(|_| Self::error("persistence_failed", "无法编码会话状态"))?;
            let session_id = state
                .sessions
                .keys()
                .next()
                .copied()
                .unwrap_or_else(Uuid::nil);
            store
                .save(session_id, &payload)
                .map_err(|_| Self::error("persistence_failed", "无法写入 SQLite 会话状态"))?;
            return Ok(());
        }
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
            ClientMessage::CreateSession { client_msg_id, .. }
            | ClientMessage::CreateWorkspace { client_msg_id, .. }
            | ClientMessage::ListWorkspaces { client_msg_id }
            | ClientMessage::ArchiveWorkspace { client_msg_id, .. }
            | ClientMessage::SwitchWorkspace { client_msg_id, .. }
            | ClientMessage::Resume { client_msg_id, .. }
            | ClientMessage::Submit { client_msg_id, .. }
            | ClientMessage::Cancel { client_msg_id, .. }
            | ClientMessage::Snapshot { client_msg_id, .. }
            | ClientMessage::Approve { client_msg_id, .. }
            | ClientMessage::Reject { client_msg_id, .. }
            | ClientMessage::RespondQuestion { client_msg_id, .. }
            | ClientMessage::AcceptDiff { client_msg_id, .. }
            | ClientMessage::RollbackDiff { client_msg_id, .. }
            | ClientMessage::PreviewDiff { client_msg_id, .. }
            | ClientMessage::ListFiles { client_msg_id, .. }
            | ClientMessage::ReadFile { client_msg_id, .. }
            | ClientMessage::SearchFiles { client_msg_id, .. }
            | ClientMessage::ProposeFileWrite { client_msg_id, .. }
            | ClientMessage::ProposeTerminal { client_msg_id, .. }
            | ClientMessage::ProposeGitMutation { client_msg_id, .. }
            | ClientMessage::GetSettings { client_msg_id }
            | ClientMessage::UpdateSettings { client_msg_id, .. }
            | ClientMessage::GetGitStatus { client_msg_id }
            | ClientMessage::ValidateAttachment { client_msg_id, .. }
            | ClientMessage::BeginAttachment { client_msg_id, .. }
            | ClientMessage::AttachmentChunk { client_msg_id, .. }
            | ClientMessage::CancelAttachment { client_msg_id, .. }
            | ClientMessage::FinalizeAttachment { client_msg_id, .. }
            | ClientMessage::ImportTuiSession { client_msg_id, .. }
            | ClientMessage::ValidateProvider { client_msg_id, .. }
            | ClientMessage::ScanMarketplace { client_msg_id, .. } => client_msg_id.clone(),
        };
        let mut state = self.state.lock().expect("engine state lock");
        if !state.seen_client_messages.insert(client_msg_id.clone()) {
            return vec![ServerMessage::Ack { client_msg_id }];
        }
        let result = match message {
            ClientMessage::CreateWorkspace { name, .. } => {
                let id = Uuid::new_v4();
                state.workspaces.insert(
                    id,
                    WorkspaceInfo {
                        id,
                        name,
                        archived: false,
                        last_used_sequence: 0,
                        last_session_id: None,
                    },
                );
                state.active_workspace_id = Some(id);
                let session_id = Uuid::new_v4();
                state.sessions.insert(
                    session_id,
                    SessionState {
                        workspace_id: Some(id),
                        ..SessionState::default()
                    },
                );
                if let Some(workspace) = state.workspaces.get_mut(&id) {
                    workspace.last_session_id = Some(session_id);
                    workspace.last_used_sequence = 1;
                }
                let mut events = vec![
                    ServerMessage::WorkspaceSwitched { workspace_id: id },
                    ServerMessage::SessionCreated {
                        session_id,
                        workspace_id: id,
                    },
                ];
                let workspaces = state.workspaces.values().cloned().collect::<Vec<_>>();
                events.push(ServerMessage::Workspaces {
                    active_workspace_id: id,
                    workspaces,
                });
                events
            }
            ClientMessage::CreateSession { workspace_id, .. } => {
                let workspace_id =
                    workspace_id
                        .or(state.active_workspace_id)
                        .unwrap_or_else(|| {
                            let id = Uuid::new_v4();
                            state.workspaces.insert(
                                id,
                                WorkspaceInfo {
                                    id,
                                    name: "默认工作区".into(),
                                    archived: false,
                                    last_used_sequence: 0,
                                    last_session_id: None,
                                },
                            );
                            state.active_workspace_id = Some(id);
                            id
                        });
                if !state.workspaces.contains_key(&workspace_id)
                    || state
                        .workspaces
                        .get(&workspace_id)
                        .is_some_and(|workspace| workspace.archived)
                {
                    return vec![Self::error("workspace_unavailable", "工作区不存在或已归档")];
                }
                let id = Uuid::new_v4();
                state.sessions.insert(
                    id,
                    SessionState {
                        workspace_id: Some(workspace_id),
                        ..SessionState::default()
                    },
                );
                if let Some(workspace) = state.workspaces.get_mut(&workspace_id) {
                    workspace.last_session_id = Some(id);
                    workspace.last_used_sequence = workspace.last_used_sequence.saturating_add(1);
                }
                vec![ServerMessage::SessionCreated {
                    session_id: id,
                    workspace_id,
                }]
            }
            ClientMessage::ListWorkspaces { .. } => {
                let mut workspaces = state.workspaces.values().cloned().collect::<Vec<_>>();
                workspaces.sort_by_key(|workspace| std::cmp::Reverse(workspace.last_used_sequence));
                vec![ServerMessage::Workspaces {
                    active_workspace_id: state.active_workspace_id.unwrap_or(Uuid::nil()),
                    workspaces,
                }]
            }
            ClientMessage::SwitchWorkspace { workspace_id, .. } => {
                if !state
                    .workspaces
                    .get(&workspace_id)
                    .is_some_and(|workspace| !workspace.archived)
                {
                    return vec![Self::error("workspace_unavailable", "工作区不存在或已归档")];
                }
                state.active_workspace_id = Some(workspace_id);
                let session_id = state
                    .workspaces
                    .get(&workspace_id)
                    .and_then(|workspace| workspace.last_session_id);
                let mut events = vec![ServerMessage::WorkspaceSwitched { workspace_id }];
                let session_id = match session_id {
                    Some(session_id) => session_id,
                    None => {
                        let session_id = Uuid::new_v4();
                        state.sessions.insert(
                            session_id,
                            SessionState {
                                workspace_id: Some(workspace_id),
                                ..SessionState::default()
                            },
                        );
                        if let Some(workspace) = state.workspaces.get_mut(&workspace_id) {
                            workspace.last_session_id = Some(session_id);
                            workspace.last_used_sequence =
                                workspace.last_used_sequence.saturating_add(1);
                        }
                        session_id
                    }
                };
                if let Some(session) = state.sessions.get(&session_id) {
                    events.push(ServerMessage::SessionSnapshot {
                        session_id,
                        workspace_id: session.workspace_id,
                        messages: session.messages.clone(),
                        sequence: session.sequence,
                        pending_approval: session.pending_approval.as_ref().map(|approval| {
                            PendingApprovalSnapshot {
                                request_id: approval.request_id,
                                tool: approval.tool.clone(),
                                summary: approval.summary.clone(),
                                confirmations_required: approval.confirmations_required,
                                confirmations: approval.confirmations,
                            }
                        }),
                        pending_question: session.pending_question.map(|question_id| {
                            QuestionSnapshot {
                                question_id,
                                prompt: session.pending_question_prompt.clone().unwrap_or_default(),
                            }
                        }),
                    });
                }
                events
            }
            ClientMessage::ArchiveWorkspace { workspace_id, .. } => {
                let Some(workspace) = state.workspaces.get_mut(&workspace_id) else {
                    return vec![Self::error("workspace_unavailable", "工作区不存在")];
                };
                workspace.archived = true;
                if state.active_workspace_id == Some(workspace_id) {
                    state.active_workspace_id = state
                        .workspaces
                        .values()
                        .find(|candidate| !candidate.archived && candidate.id != workspace_id)
                        .map(|candidate| candidate.id);
                }
                let workspaces = state.workspaces.values().cloned().collect::<Vec<_>>();
                vec![
                    ServerMessage::WorkspaceArchived { workspace_id },
                    ServerMessage::Workspaces {
                        active_workspace_id: state.active_workspace_id.unwrap_or(Uuid::nil()),
                        workspaces,
                    },
                ]
            }
            ClientMessage::Resume {
                session_id,
                workspace_id,
                ..
            }
            | ClientMessage::Snapshot {
                session_id,
                workspace_id,
                ..
            } => match state.sessions.get(&session_id) {
                Some(session) if workspace_id.is_none() || workspace_id == session.workspace_id => {
                    vec![ServerMessage::SessionSnapshot {
                        session_id,
                        workspace_id: session.workspace_id,
                        messages: session.messages.clone(),
                        sequence: session.sequence,
                        pending_approval: session.pending_approval.as_ref().map(|approval| {
                            PendingApprovalSnapshot {
                                request_id: approval.request_id,
                                tool: approval.tool.clone(),
                                summary: approval.summary.clone(),
                                confirmations_required: approval.confirmations_required,
                                confirmations: approval.confirmations,
                            }
                        }),
                        pending_question: session.pending_question.map(|question_id| {
                            QuestionSnapshot {
                                question_id,
                                prompt: session.pending_question_prompt.clone().unwrap_or_default(),
                            }
                        }),
                    }]
                }
                Some(_) => vec![Self::error(
                    "workspace_session_mismatch",
                    "会话不属于请求的工作区",
                )],
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
                    session.pending_question_prompt = Some(question.to_owned());
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
                        confirmations_required: 1,
                        confirmations: 0,
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
            ClientMessage::PreviewDiff {
                client_msg_id,
                session_id,
                proposal_id,
            } => self.preview_diff(&mut state, client_msg_id, session_id, proposal_id),
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
            ClientMessage::GetGitStatus { .. } => match &self.workspace {
                Some(workspace) => workspace
                    .git_status()
                    .map(|(branch, entries)| vec![ServerMessage::GitStatus { branch, entries }])
                    .unwrap_or_else(|error| vec![error]),
                None => vec![Self::error("workspace_unavailable", "没有配置 workspace")],
            },
            ClientMessage::ValidateProvider {
                base_url, model, ..
            } => {
                let valid_url = base_url.starts_with("https://")
                    && !base_url.contains('@')
                    && !base_url.contains('#');
                let valid_model = !model.trim().is_empty() && model.len() <= 200;
                vec![ServerMessage::ProviderValidation {
                    base_url,
                    model,
                    reachable: false,
                    error_code: Some(
                        if !valid_url {
                            "invalid_base_url"
                        } else if !valid_model {
                            "invalid_model"
                        } else {
                            "network_not_attempted"
                        }
                        .into(),
                    ),
                }]
            }
            ClientMessage::ValidateAttachment {
                filename,
                byte_len,
                content_type,
                ..
            } => {
                let extension_allowed = [
                    "png", "jpg", "jpeg", "gif", "webp", "pdf", "txt", "md", "json",
                ]
                .iter()
                .any(|extension| {
                    filename
                        .to_ascii_lowercase()
                        .ends_with(&format!(".{extension}"))
                });
                let mime_allowed = [
                    "image/png",
                    "image/jpeg",
                    "image/gif",
                    "image/webp",
                    "application/pdf",
                    "text/plain",
                    "text/markdown",
                    "application/json",
                ]
                .contains(&content_type.as_str());
                if filename.contains('/')
                    || filename.contains('\\')
                    || filename == "."
                    || filename == ".."
                    || !extension_allowed
                    || !mime_allowed
                    || byte_len == 0
                    || byte_len > 10 * 1024 * 1024
                {
                    vec![Self::error(
                        "attachment_rejected",
                        "附件类型、名称或大小不符合允许策略",
                    )]
                } else {
                    vec![ServerMessage::AttachmentValidated {
                        filename,
                        byte_len,
                        content_type,
                    }]
                }
            }
            ClientMessage::BeginAttachment {
                client_msg_id,
                session_id,
                filename,
                content_type,
                byte_len,
            } => {
                if let Err(error) =
                    AttachmentStager::validate_name_type_size(&filename, &content_type, byte_len)
                {
                    return vec![Self::error("attachment_rejected", &error)];
                }
                let upload_id = Uuid::new_v4();
                self.attachments.lock().unwrap().insert(
                    upload_id,
                    AttachmentUpload {
                        session_id,
                        filename: filename.clone(),
                        content_type,
                        expected: byte_len,
                        chunks: Vec::new(),
                        received: 0,
                    },
                );
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::AttachmentStarted {
                        session_id,
                        upload_id,
                        filename,
                    },
                ]
            }
            ClientMessage::AttachmentChunk {
                client_msg_id,
                upload_id,
                chunk,
            } => {
                let bytes = match Base64Engine::decode(
                    &base64::engine::general_purpose::STANDARD,
                    chunk.as_bytes(),
                ) {
                    Ok(bytes) => bytes,
                    Err(_) => {
                        return vec![Self::error(
                            "attachment_chunk_invalid",
                            "附件分块不是有效 base64",
                        )];
                    }
                };
                let mut uploads = self.attachments.lock().unwrap();
                let Some(upload) = uploads.get_mut(&upload_id) else {
                    return vec![Self::error("attachment_not_found", "附件上传不存在")];
                };
                upload.received = upload.received.saturating_add(bytes.len() as u64);
                if upload.received > upload.expected {
                    uploads.remove(&upload_id);
                    return vec![Self::error("attachment_quota_exceeded", "附件超过声明大小")];
                }
                upload.chunks.push(bytes);
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::AttachmentProgress {
                        upload_id,
                        received: upload.received,
                    },
                ]
            }
            ClientMessage::CancelAttachment {
                client_msg_id,
                upload_id,
            } => {
                let removed = self
                    .attachments
                    .lock()
                    .unwrap()
                    .remove(&upload_id)
                    .is_some();
                if !removed {
                    return vec![Self::error("attachment_not_found", "附件上传不存在")];
                }
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::AttachmentCancelled { upload_id },
                ]
            }
            ClientMessage::FinalizeAttachment {
                client_msg_id,
                upload_id,
                relative_path,
            } => {
                let upload_session_id = self
                    .attachments
                    .lock()
                    .unwrap()
                    .get(&upload_id)
                    .map(|upload| upload.session_id);
                let Some(upload_session_id) = upload_session_id else {
                    return vec![Self::error("attachment_not_found", "附件上传不存在")];
                };
                let request_id = Uuid::new_v4();
                let Some(session) = state.sessions.get_mut(&upload_session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                session.pending_approval = Some(PendingApproval {
                    request_id,
                    tool: "workspace.attach_attachment".into(),
                    summary: format!("写入附件 {relative_path}"),
                    confirmations_required: 1,
                    confirmations: 0,
                });
                session
                    .pending_attachment_moves
                    .insert(request_id, (upload_session_id, upload_id, relative_path));
                session.sequence += 1;
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::ToolApprovalRequested {
                        session_id: upload_session_id,
                        request_id,
                        tool: "workspace.attach_attachment".into(),
                        summary: "请求将已上传附件写入 workspace".into(),
                        sequence: session.sequence,
                    },
                ]
            }
            ClientMessage::ImportTuiSession {
                root, session_id, ..
            } => {
                let requested = std::fs::canonicalize(&root).ok();
                let allowed = requested.as_ref().is_some_and(|requested| {
                    self.tui_session_roots
                        .iter()
                        .any(|configured| requested == configured)
                });
                if !allowed {
                    vec![Self::error(
                        "tui_session_root_not_allowed",
                        "TUI session root is not configured",
                    )]
                } else {
                    Self::import_tui_session(
                        requested
                            .as_deref()
                            .expect("allowed TUI session root is canonical"),
                        &session_id,
                    )
                }
            }
            ClientMessage::ScanMarketplace { root, .. } => {
                let requested = std::fs::canonicalize(&root).ok();
                let allowed = requested.as_ref().is_some_and(|requested| {
                    self.marketplace_roots
                        .iter()
                        .any(|configured| requested == configured)
                });
                if !allowed {
                    return vec![Self::error(
                        "marketplace_root_not_allowed",
                        "marketplace root is not configured",
                    )];
                }
                let scan = xai_grok_plugin_marketplace::scan_marketplace(
                    requested
                        .as_deref()
                        .expect("allowed marketplace root is canonical"),
                );
                vec![ServerMessage::MarketplaceScan {
                    entries: scan
                        .entries
                        .into_iter()
                        .filter_map(|entry| serde_json::to_value(entry).ok())
                        .collect(),
                    catalog_loaded: scan.catalog_loaded,
                }]
            }
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
                state.settings = settings.clone();
                if let Err(error) = self.persist(&state) {
                    return vec![error];
                }
                vec![ServerMessage::SettingsUpdated { base_url, model }]
            }
            ClientMessage::ProposeGitMutation {
                client_msg_id,
                session_id,
                operation,
                argument,
            } => {
                let allowed = matches!(
                    operation.as_str(),
                    "stage" | "unstage" | "commit" | "checkout_branch" | "discard"
                );
                if !allowed || argument.contains('\0') || argument.contains("..") {
                    return vec![Self::error(
                        "git_operation_rejected",
                        "Git 操作或参数不在允许范围",
                    )];
                }
                let Some(session) = state.sessions.get_mut(&session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                let request_id = Uuid::new_v4();
                let confirmations_required = u8::from(matches!(
                    operation.as_str(),
                    "commit" | "checkout_branch" | "discard"
                )) + 1;
                session.pending_approval = Some(PendingApproval {
                    request_id,
                    tool: format!("git.{operation}"),
                    summary: argument,
                    confirmations_required,
                    confirmations: 0,
                });
                session.sequence += 1;
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::ToolApprovalRequested {
                        session_id,
                        request_id,
                        tool: format!("git.{operation}"),
                        summary: "请求执行受限 Git 操作".into(),
                        sequence: session.sequence,
                    },
                ]
            }
            ClientMessage::ProposeTerminal {
                client_msg_id,
                session_id,
                command,
            } => {
                let Some(session) = state.sessions.get_mut(&session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                let request_id = Uuid::new_v4();
                session.pending_approval = Some(PendingApproval {
                    request_id,
                    tool: "terminal.execute".into(),
                    summary: command,
                    confirmations_required: 1,
                    confirmations: 0,
                });
                session.sequence += 1;
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::ToolApprovalRequested {
                        session_id,
                        request_id,
                        tool: "terminal.execute".into(),
                        summary: "请求执行工作区内终端命令".into(),
                        sequence: session.sequence,
                    },
                ]
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
                    confirmations_required: 1,
                    confirmations: 0,
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
        if approved && pending.confirmations < pending.confirmations_required {
            let mut pending = pending.clone();
            pending.confirmations += 1;
            if pending.confirmations < pending.confirmations_required {
                session.pending_approval = Some(pending);
                session.sequence += 1;
                return vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::ToolApprovalRequested {
                        session_id: *session_id,
                        request_id,
                        tool: session
                            .pending_approval
                            .as_ref()
                            .map(|approval| approval.tool.clone())
                            .unwrap_or_default(),
                        summary: "破坏性 Git 操作需要再次确认".into(),
                        sequence: session.sequence,
                    },
                ];
            }
        }
        let pending_file_write = session.pending_file_writes.remove(&request_id);
        let pending_attachment_move = session.pending_attachment_moves.remove(&request_id);
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
                        events.push(ServerMessage::ToolStarted {
                            session_id: write_session_id,
                            tool: "workspace.write_file".into(),
                            sequence: session.sequence,
                        });
                        session.sequence += 1;
                        events.push(ServerMessage::FileChanged {
                            session_id: write_session_id,
                            path: relative_path.clone(),
                            operation: "write".into(),
                            sequence: session.sequence,
                        });
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
        } else if let Some((move_session_id, upload_id, relative_path)) = pending_attachment_move {
            if !approved {
                "rejected"
            } else {
                let upload = self.attachments.lock().unwrap().remove(&upload_id);
                if let Some(upload) = upload {
                    if upload.received != upload.expected {
                        session.sequence += 1;
                        events.push(ServerMessage::Error {
                            code: "attachment_incomplete".into(),
                            message: "附件仍未接收完整".into(),
                        });
                        "failed"
                    } else if let Some(workspace) = &self.workspace {
                        let staging = workspace
                            .root
                            .join(".chaos-staging")
                            .join(format!("upload-{upload_id}.part"));
                        let result = (|| {
                            let mut file =
                                std::fs::File::create(&staging).map_err(|e| e.to_string())?;
                            use std::io::Write;
                            for chunk in upload.chunks {
                                file.write_all(&chunk).map_err(|e| e.to_string())?;
                            }
                            file.sync_all().map_err(|e| e.to_string())?;
                            let bytes = upload.received;
                            let destination = workspace.root.join(&relative_path);
                            let parent = destination
                                .parent()
                                .ok_or_else(|| "invalid attachment path".to_string())?;
                            let canonical_parent =
                                std::fs::canonicalize(parent).map_err(|e| e.to_string())?;
                            if !canonical_parent.starts_with(workspace.root.as_path()) {
                                return Err("attachment path escapes workspace".into());
                            }
                            let target = canonical_parent.join(
                                destination
                                    .file_name()
                                    .ok_or_else(|| "invalid attachment path".to_string())?,
                            );
                            std::fs::rename(&staging, &target).map_err(|e| e.to_string())?;
                            Ok(bytes)
                        })();
                        match result {
                            Ok(bytes) => {
                                session.sequence += 1;
                                events.push(ServerMessage::FileChanged {
                                    session_id: move_session_id,
                                    path: relative_path.clone(),
                                    operation: "attachment_write".into(),
                                    sequence: session.sequence,
                                });
                                events.push(ServerMessage::AttachmentCompleted {
                                    session_id: move_session_id,
                                    upload_id,
                                    path: relative_path,
                                    bytes,
                                });
                                "executed"
                            }
                            Err(message) => {
                                session.sequence += 1;
                                events.push(ServerMessage::Error {
                                    code: "attachment_write_failed".into(),
                                    message,
                                });
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
                } else {
                    session.sequence += 1;
                    events.push(ServerMessage::Error {
                        code: "attachment_not_found".into(),
                        message: "附件上传不存在".into(),
                    });
                    "failed"
                }
            }
        } else if approved {
            session.sequence += 1;
            events.push(ServerMessage::ToolStarted {
                session_id: *session_id,
                tool: pending.tool.clone(),
                sequence: session.sequence,
            });
            if pending.tool.starts_with("git.") {
                let result = match &self.git_adapter {
                    Some(adapter) => {
                        let operation = pending.tool.strip_prefix("git.").unwrap_or_default();
                        match operation {
                            "stage" => adapter.stage(&pending.summary).map(|_| "staged".into()),
                            "unstage" => {
                                adapter.unstage(&pending.summary).map(|_| "unstaged".into())
                            }
                            "commit" => adapter.commit(&pending.summary),
                            "checkout_branch" => adapter
                                .checkout_branch(&pending.summary)
                                .map(|_| "checked out".into()),
                            "discard" => adapter
                                .discard(&pending.summary)
                                .map(|_| "discarded".into()),
                            _ => Err("git operation unavailable".into()),
                        }
                    }
                    None => Err("没有配置 Git adapter".into()),
                };
                match result {
                    Ok(result) => {
                        session.sequence += 1;
                        events.push(ServerMessage::GitMutationResult {
                            session_id: *session_id,
                            operation: pending.tool.clone(),
                            result,
                        });
                        "executed"
                    }
                    Err(error) => {
                        session.sequence += 1;
                        events.push(ServerMessage::Error {
                            code: "git_failed".into(),
                            message: error,
                        });
                        "failed"
                    }
                }
            } else if pending.tool == "terminal.execute" {
                match &self.terminal_adapter {
                    Some(adapter) => match adapter.run(&pending.summary) {
                        Ok(result) => {
                            session.sequence += 1;
                            events.push(ServerMessage::TerminalResult {
                                session_id: *session_id,
                                output: result.output,
                                exit_code: result.exit_code,
                            });
                            "executed"
                        }
                        Err(error) => {
                            session.sequence += 1;
                            events.push(ServerMessage::Error {
                                code: "terminal_failed".into(),
                                message: error,
                            });
                            "failed"
                        }
                    },
                    None => {
                        session.sequence += 1;
                        events.push(ServerMessage::Error {
                            code: "terminal_unavailable".into(),
                            message: "没有配置终端 adapter".into(),
                        });
                        "unavailable"
                    }
                }
            } else {
                match &self.tool_adapter {
                    Some(adapter) => match adapter.execute(&pending.tool, &pending.summary) {
                        Ok(result) => {
                            session.messages.push(TimelineMessage {
                                role: "tool".into(),
                                text: result.clone(),
                            });
                            session.sequence += 1;
                            events.push(ServerMessage::ToolProgress {
                                session_id: *session_id,
                                tool: pending.tool.clone(),
                                progress: "completed".into(),
                                sequence: session.sequence,
                            });
                            session.sequence += 1;
                            events.push(ServerMessage::ToolResult {
                                session_id: *session_id,
                                tool: pending.tool.clone(),
                                result,
                                sequence: session.sequence,
                            });
                            session.sequence += 1;
                            events.push(ServerMessage::Usage {
                                session_id: *session_id,
                                input_tokens: pending.summary.len() as u64,
                                output_tokens: 1,
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
        session.pending_question_prompt = None;
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

    fn import_tui_session(root: &Path, session_id: &str) -> Vec<ServerMessage> {
        let session_root = root.join(session_id);
        let summary_path = session_root.join("summary.json");
        let updates_path = session_root.join("updates.jsonl");
        let summary = match std::fs::read(&summary_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        {
            Some(summary) => summary,
            None => {
                return vec![Self::error(
                    "tui_session_invalid",
                    "TUI summary.json 不可读取或格式无效",
                )];
            }
        };
        let info = summary.get("info").and_then(serde_json::Value::as_object);
        let cwd = info
            .and_then(|info| info.get("cwd"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let Some(cwd) = cwd else {
            return vec![Self::error(
                "tui_session_invalid",
                "TUI summary 缺少 info.cwd",
            )];
        };
        let title = summary
            .get("session_summary")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let message_count = summary
            .get("num_messages")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_else(|| {
                std::fs::read_to_string(&updates_path)
                    .map(|contents| {
                        contents
                            .lines()
                            .filter(|line| !line.trim().is_empty())
                            .count()
                    })
                    .unwrap_or(0) as u64
            }) as usize;
        let source_unchanged =
            std::fs::metadata(&summary_path).is_ok() && std::fs::metadata(&updates_path).is_ok();
        vec![ServerMessage::TuiSessionImport {
            session_id: session_id.to_owned(),
            cwd,
            title,
            message_count,
            source_unchanged,
        }]
    }

    fn preview_diff(
        &self,
        state: &mut State,
        client_msg_id: String,
        session_id: Uuid,
        proposal_id: String,
    ) -> Vec<ServerMessage> {
        if !state.sessions.contains_key(&session_id) {
            return vec![Self::error("session_not_found", "会话不存在")];
        }
        let Some(adapter) = &self.diff_adapter else {
            return vec![Self::error("diff_failed", "没有配置 Diff adapter")];
        };
        match adapter.preview(&proposal_id) {
            Ok(preview) => vec![
                ServerMessage::Ack { client_msg_id },
                ServerMessage::DiffPreview {
                    session_id,
                    preview,
                },
            ],
            Err(message) => vec![Self::error("diff_failed", &message)],
        }
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
    fn workspace_registry_creates_switches_archives_and_lists_recent() {
        let engine = Engine::new();
        let first = engine.handle(ClientMessage::CreateSession {
            client_msg_id: "workspace-session".into(),
            workspace_id: None,
        });
        let (session_id, workspace_id) = match first[0] {
            ServerMessage::SessionCreated {
                session_id,
                workspace_id,
            } => (session_id, workspace_id),
            _ => panic!(),
        };
        let listed = engine.handle(ClientMessage::ListWorkspaces {
            client_msg_id: "workspaces".into(),
        });
        assert!(
            matches!(&listed[0], ServerMessage::Workspaces { workspaces, active_workspace_id } if *active_workspace_id == workspace_id && workspaces.iter().any(|workspace| workspace.id == workspace_id && !workspace.archived))
        );
        assert!(matches!(
            engine.handle(ClientMessage::SwitchWorkspace {
                client_msg_id: "switch".into(),
                workspace_id
            })[0],
            ServerMessage::WorkspaceSwitched { .. }
        ));
        assert!(matches!(
            engine.handle(ClientMessage::ArchiveWorkspace {
                client_msg_id: "archive".into(),
                workspace_id
            })[0],
            ServerMessage::WorkspaceArchived { .. }
        ));
        assert!(matches!(
            engine.handle(ClientMessage::Resume {
                client_msg_id: "resume".into(),
                session_id,
                workspace_id: None
            })[0],
            ServerMessage::SessionSnapshot { .. }
        ));
    }

    #[test]
    fn real_session_flow_supports_resume_deduplication_and_sequence() {
        let engine = Engine::new();
        let created = engine.handle(ClientMessage::CreateSession {
            client_msg_id: "create".into(),
            workspace_id: None,
        });
        let id = match created[0] {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
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
        assert!(matches!(
            engine.handle(ClientMessage::Submit {
                client_msg_id: "submit".into(),
                session_id: id,
                prompt: "重复".into()
            }).as_slice(),
            [ServerMessage::Ack { client_msg_id }] if client_msg_id == "submit"
        ));
        let snapshot = engine.handle(ClientMessage::Resume {
            client_msg_id: "resume".into(),
            session_id: id,
            workspace_id: None,
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
            workspace_id: None,
        })[0]
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
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
        assert!(matches!(
            engine.handle(ClientMessage::Cancel { client_msg_id: "x".into(), session_id: id }).as_slice(),
            [ServerMessage::Ack { client_msg_id }] if client_msg_id == "x"
        ));
    }
    #[test]
    fn persistent_state_has_schema_and_rejects_newer_versions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sessions.json");
        let engine = Engine::with_persistence(&path).unwrap();
        engine.handle(ClientMessage::CreateSession {
            client_msg_id: "schema-create".into(),
            workspace_id: None,
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
            workspace_id: None,
        })[0]
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
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
            workspace_id: None,
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
            workspace_id: None,
        })[0]
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
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
    fn approved_tool_emits_started_progress_result_sequence() {
        let engine = Engine::with_tool_adapter(FixtureTool);
        let session_id = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "sequence-create".into(),
            workspace_id: None,
        })[0]
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
            _ => panic!(),
        };
        let requested = engine.handle(ClientMessage::Submit {
            client_msg_id: "sequence-submit".into(),
            session_id,
            prompt: "/approve-tool write".into(),
        });
        let request_id = match requested[1] {
            ServerMessage::ToolApprovalRequested { request_id, .. } => request_id,
            _ => panic!(),
        };
        let events = engine.handle(ClientMessage::Approve {
            client_msg_id: "sequence-approve".into(),
            request_id,
        });
        assert!(matches!(events[1], ServerMessage::ToolStarted { .. }));
        assert!(matches!(events[2], ServerMessage::ToolProgress { .. }));
        assert!(matches!(events[3], ServerMessage::ToolResult { .. }));
    }

    #[test]
    fn approved_tool_runs_only_through_tool_adapter() {
        let engine = Engine::with_tool_adapter(FixtureTool);
        let id = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "create-tool".into(),
            workspace_id: None,
        })[0]
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
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
        assert!(resolved.iter().any(|event| matches!(event, ServerMessage::ToolResult { result, .. } if result == "demo.tool:write")));
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
            workspace_id: None,
        })[0]
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
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

        fn preview(&self, proposal_id: &str) -> Result<DiffPreview, String> {
            Ok(DiffPreview {
                proposal_id: proposal_id.into(),
                path: "hello.txt".into(),
                before: Some("before".into()),
                after: "after".into(),
            })
        }
    }

    #[test]
    fn diff_accept_and_rollback_use_session_scoped_adapter() {
        let engine = Engine::with_diff_adapter(FixtureDiff);
        let session_id = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "diff-create".into(),
            workspace_id: None,
        })[0]
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
            _ => panic!(),
        };
        let previewed = engine.handle(ClientMessage::PreviewDiff {
            client_msg_id: "diff-preview".into(),
            session_id,
            proposal_id: "p1".into(),
        });
        assert!(previewed.iter().any(|event| matches!(
            event,
            ServerMessage::DiffPreview { preview, .. } if preview.path == "hello.txt" && preview.after == "after"
        )));
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
    fn sqlite_store_rejects_corrupt_database_and_missing_parent() {
        let directory = tempfile::tempdir().unwrap();
        let corrupt = directory.path().join("corrupt.db");
        std::fs::write(&corrupt, b"not sqlite").unwrap();
        assert!(SqliteSessionStore::open(&corrupt).is_err());
        let missing_parent = directory.path().join("missing").join("gui.db");
        assert!(SqliteSessionStore::open(&missing_parent).is_err());
    }

    #[test]
    fn sqlite_store_upgrades_older_schema_with_backup_and_rejects_bad_version() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("gui.db");
        {
            let connection = rusqlite::Connection::open(&path).unwrap();
            connection.execute_batch("CREATE TABLE gui_meta (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL); CREATE TABLE gui_sessions (id TEXT PRIMARY KEY NOT NULL, payload TEXT NOT NULL); INSERT INTO gui_meta(key,value) VALUES('schema_version','0');").unwrap();
        }
        let backup = directory.path().join("gui.schema-v0.bak");
        let store = SqliteSessionStore::open(&path).unwrap();
        assert!(backup.is_file());
        assert_eq!(store.load(Uuid::new_v4()).unwrap(), None);
        let connection = rusqlite::Connection::open(&path).unwrap();
        let version: String = connection
            .query_row(
                "SELECT value FROM gui_meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, SQLITE_SCHEMA_VERSION.to_string());
        connection
            .execute(
                "UPDATE gui_meta SET value = 'not-a-version' WHERE key = 'schema_version'",
                [],
            )
            .unwrap();
        assert!(SqliteSessionStore::open(&path).is_err());
    }

    #[test]
    fn sqlite_store_round_trips_and_rejects_newer_schema() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("gui.db");
        let store = SqliteSessionStore::open(&path).unwrap();
        let session = Uuid::new_v4();
        store.save(session, "snapshot").unwrap();
        assert_eq!(store.load(session).unwrap().as_deref(), Some("snapshot"));
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute(
                "UPDATE gui_meta SET value = '99' WHERE key = 'schema_version'",
                [],
            )
            .unwrap();
        assert!(SqliteSessionStore::open(&path).is_err());
    }

    #[test]
    fn provider_validation_never_attempts_network_or_handles_credentials() {
        let engine = Engine::new();
        let result = engine.handle(ClientMessage::ValidateProvider {
            client_msg_id: "provider".into(),
            base_url: "http://user:pass@example.test".into(),
            model: "demo".into(),
        });
        assert!(
            matches!(&result[0], ServerMessage::ProviderValidation { reachable: false, error_code: Some(code), .. } if code == "invalid_base_url")
        );
        let valid_shape = engine.handle(ClientMessage::ValidateProvider {
            client_msg_id: "provider-ok".into(),
            base_url: "https://api.example.test/v1".into(),
            model: "demo".into(),
        });
        assert!(
            matches!(&valid_shape[0], ServerMessage::ProviderValidation { reachable: false, error_code: Some(code), .. } if code == "network_not_attempted")
        );
    }

    struct FixtureGit;
    impl GitAdapter for FixtureGit {
        fn stage(&self, path: &str) -> Result<(), String> {
            if path == "src/main.rs" {
                Ok(())
            } else {
                Err("bad path".into())
            }
        }
        fn unstage(&self, path: &str) -> Result<(), String> {
            self.stage(path)
        }
        fn commit(&self, message: &str) -> Result<String, String> {
            Ok(format!("commit:{message}"))
        }
        fn checkout_branch(&self, branch: &str) -> Result<(), String> {
            if branch == "feature" {
                Ok(())
            } else {
                Err("bad branch".into())
            }
        }
        fn discard(&self, path: &str) -> Result<(), String> {
            self.stage(path)
        }
    }

    #[test]
    fn attachment_stager_writes_chunks_and_cleans_failed_uploads() {
        let directory = tempfile::tempdir().unwrap();
        let stager = AttachmentStager::new(directory.path(), 8).unwrap();
        let staged = stager
            .stage_chunks(
                "note.txt",
                "text/plain",
                vec![Ok(b"abc".to_vec()), Ok(b"def".to_vec())],
            )
            .unwrap();
        assert_eq!(std::fs::read_to_string(&staged).unwrap(), "abcdef");
        let rejected =
            stager.stage_chunks("too.txt", "text/plain", vec![Ok(b"123456789".to_vec())]);
        assert!(rejected.is_err());
        assert_eq!(
            std::fs::read_dir(directory.path().join(".chaos-staging"))
                .unwrap()
                .count(),
            1
        );
        assert!(
            stager
                .stage_chunks("../escape.txt", "text/plain", vec![Ok(b"x".to_vec())])
                .is_err()
        );
    }

    #[test]
    fn process_git_adapter_executes_only_fixed_git_arguments() {
        let directory = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(["-C", directory.path().to_str().unwrap()])
                .args(args)
                .output()
                .unwrap()
        };
        assert!(run(&["init", "-q"]).status.success());
        std::fs::write(directory.path().join("note.txt"), "hello").unwrap();
        let adapter = ProcessGitAdapter::new(directory.path()).unwrap();
        adapter.stage("note.txt").unwrap();
        let commit = adapter.commit("first").unwrap();
        assert_eq!(commit.len(), 40);
        assert!(adapter.stage("../outside").is_err());
        assert!(adapter.checkout_branch("--orphan").is_err());
    }

    #[test]
    fn git_mutation_requires_approval_and_allows_only_fixed_operations() {
        let engine = Engine::new().with_git_adapter(FixtureGit);
        let session_id = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "git-session".into(),
            workspace_id: None,
        })[0]
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
            _ => panic!(),
        };
        let requested = engine.handle(ClientMessage::ProposeGitMutation {
            client_msg_id: "git-propose".into(),
            session_id,
            operation: "commit".into(),
            argument: "safe".into(),
        });
        let request_id = match &requested[1] {
            ServerMessage::ToolApprovalRequested {
                request_id, tool, ..
            } if tool == "git.commit" => *request_id,
            other => panic!("{other:?}"),
        };
        let result = engine.handle(ClientMessage::Approve {
            client_msg_id: "git-approve".into(),
            request_id,
        });
        let request_id = match &result[1] {
            ServerMessage::ToolApprovalRequested { request_id, .. } => *request_id,
            other => panic!("unexpected {other:?}"),
        };
        let result = engine.handle(ClientMessage::Approve {
            client_msg_id: "git-approve-again".into(),
            request_id,
        });
        assert!(result.iter().any(|event| matches!(event, ServerMessage::GitMutationResult { result, .. } if result == "commit:safe")));
        let rejected = engine.handle(ClientMessage::ProposeGitMutation {
            client_msg_id: "git-bad".into(),
            session_id,
            operation: "push".into(),
            argument: "origin".into(),
        });
        assert!(
            matches!(&rejected[0], ServerMessage::Error { code, .. } if code == "git_operation_rejected")
        );
    }

    #[test]
    fn terminal_process_adapter_runs_in_fixed_cwd_and_truncates_output() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("marker.txt"), "cwd").unwrap();
        let adapter = ProcessTerminalAdapter::new(directory.path(), 8).unwrap();
        let result = adapter.run("pwd; printf 123456789").unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.output.len() <= 8 + "\n[output truncated]".len());
        assert!(result.output.contains("output truncated") || result.output.contains("marker"));
    }

    #[test]
    fn terminal_command_requires_approval_before_execution() {
        let directory = tempfile::tempdir().unwrap();
        let adapter = ProcessTerminalAdapter::new(directory.path(), 1024).unwrap();
        let engine = Engine::new().with_terminal_adapter(adapter);
        let session_id = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "term-session".into(),
            workspace_id: None,
        })[0]
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
            _ => panic!(),
        };
        let requested = engine.handle(ClientMessage::ProposeTerminal {
            client_msg_id: "term-propose".into(),
            session_id,
            command: "printf approved".into(),
        });
        let request_id = match &requested[1] {
            ServerMessage::ToolApprovalRequested {
                request_id, tool, ..
            } if tool == "terminal.execute" => *request_id,
            other => panic!("unexpected {other:?}"),
        };
        let result = engine.handle(ClientMessage::Approve {
            client_msg_id: "term-approve".into(),
            request_id,
        });
        assert!(result.iter().any(|event| matches!(event, ServerMessage::TerminalResult { output, exit_code: 0, .. } if output.contains("approved"))));
    }

    #[test]
    fn terminal_process_adapter_returns_nonzero_exit_code() {
        let directory = tempfile::tempdir().unwrap();
        let adapter = ProcessTerminalAdapter::new(directory.path(), 1024).unwrap();
        let result = adapter.run("exit 7").unwrap();
        assert_eq!(result.exit_code, 7);
    }

    #[test]
    fn attachment_policy_rejects_path_traversal_unknown_types_and_large_files() {
        let engine = Engine::new();
        let accepted = engine.handle(ClientMessage::ValidateAttachment {
            client_msg_id: "attachment-ok".into(),
            filename: "note.txt".into(),
            byte_len: 5,
            content_type: "text/plain".into(),
        });
        assert!(matches!(
            accepted[0],
            ServerMessage::AttachmentValidated { .. }
        ));
        let rejected = engine.handle(ClientMessage::ValidateAttachment {
            client_msg_id: "attachment-bad".into(),
            filename: "../secret.exe".into(),
            byte_len: 11,
            content_type: "application/octet-stream".into(),
        });
        assert!(
            matches!(&rejected[0], ServerMessage::Error { code, .. } if code == "attachment_rejected")
        );
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
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let persisted = Engine::with_persistence(&path).unwrap();
        let updated = persisted.handle(ClientMessage::UpdateSettings {
            client_msg_id: "persist-settings".into(),
            base_url: Some("https://persisted.example.test/v1".into()),
            model: Some("persisted-model".into()),
        });
        assert!(matches!(updated[0], ServerMessage::SettingsUpdated { .. }));
        drop(persisted);
        let reopened = Engine::with_persistence(&path).unwrap();
        let restored = reopened.handle(ClientMessage::GetSettings {
            client_msg_id: "restore-settings".into(),
        });
        assert!(
            matches!(&restored[0], ServerMessage::Settings { base_url, model, .. } if base_url.as_deref() == Some("https://persisted.example.test/v1") && model.as_deref() == Some("persisted-model"))
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
            workspace_id: None,
        })[0]
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
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
            workspace_id: None,
        })[0]
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
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
            workspace_id: None,
        })[0]
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
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
            workspace_id: None,
        })[0]
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
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
