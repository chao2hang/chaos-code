use base64::Engine as Base64Engine;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;
use uuid::Uuid;

pub mod protocol_schema;
pub mod provider;
pub mod pty;
pub mod remote;

pub const PROTOCOL_VERSION: u16 = 1;
pub const STATE_SCHEMA_VERSION: u16 = 1;
const SQLITE_SCHEMA_VERSION: u16 = 1;
const DELTA_SIZE: usize = 8;
const WORKSPACE_READ_LIMIT: usize = 1024 * 1024;

fn read_workspace_bytes(reader: impl std::io::Read) -> std::io::Result<Vec<u8>> {
    let mut contents = Vec::new();
    let mut limited = std::io::Read::take(reader, WORKSPACE_READ_LIMIT as u64 + 1);
    std::io::Read::read_to_end(&mut limited, &mut contents)?;
    Ok(contents)
}

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
    /// The diff the next commit would record, bounded to
    /// [`COMMIT_DIFF_LIMIT`] bytes. Adapters that cannot read a diff say so;
    /// they do not pretend an empty staged area means the same thing.
    fn staged_diff(&self) -> Result<StagedDiff, String> {
        Err("Git adapter 不支持读取暂存差异".into())
    }
}

/// What [`GitAdapter::staged_diff`] hands back: the diff text the commit-message
/// request may carry, and whether it stopped short of the real staged content.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StagedDiff {
    pub text: String,
    pub truncated: bool,
}

/// Bytes of staged diff one commit-message request may send to the Provider.
/// The browser can stage a generated file of any size, and the diff goes into a
/// prompt, so the read itself has to stop at a limit rather than at whatever the
/// staging area happens to hold.
pub const COMMIT_DIFF_LIMIT: usize = 24 * 1024;

/// Longest suggestion accepted from a Provider, cut on a character boundary.
const COMMIT_MESSAGE_LIMIT: usize = 600;

/// The instruction a commit-message suggestion is asked for. The staged diff is
/// quoted last and inside delimiters so that diff content cannot read as
/// instructions to the model that follows it.
fn commit_message_prompt(branch: Option<&str>, diff: &StagedDiff) -> String {
    let mut prompt = String::from(
        "根据下面暂存的 Git 差异写一条提交信息。第一行是不超过 72 个字符的摘要，\
         使用祈使句，不要以句号结尾；只有在差异确实需要解释时才补充正文段落。\
         只输出提交信息本身，不要输出解释、前缀或代码块标记。\n\n",
    );
    if let Some(branch) = branch {
        prompt.push_str("当前分支：");
        prompt.push_str(branch);
        prompt.push('\n');
    }
    if diff.truncated {
        prompt.push_str("（差异过长，以下内容在限制处截断）\n");
    }
    prompt.push_str("---START STAGED DIFF---\n");
    prompt.push_str(&diff.text);
    if !diff.text.ends_with('\n') {
        prompt.push('\n');
    }
    prompt.push_str("---END STAGED DIFF---");
    prompt
}

/// What the browser is offered as the commit message.
///
/// Models wrap the answer in code fences, bullet it, or open with a label like
/// 「这是建议：」. All of that would be pasted verbatim into `git commit -m`, so it
/// is removed here. Everything the model wrote after that is kept, body included:
/// dropping a paragraph the Provider judged worth writing is a worse failure than
/// leaving one explanatory line the user can delete in the form.
fn normalize_commit_message(raw: &str) -> String {
    let mut lines: Vec<String> = raw
        .trim()
        .lines()
        .map(|line| line.trim().to_string())
        .collect();
    while lines.first().is_some_and(|line| line.starts_with("```")) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.starts_with("```")) {
        lines.pop();
    }
    while lines.first().is_some_and(|line| line.is_empty()) {
        lines.remove(0);
    }
    let leading_label = lines
        .first()
        .is_some_and(|line| line.len() <= 40 && (line.ends_with(':') || line.ends_with('：')));
    if leading_label {
        lines.remove(0);
        while lines.first().is_some_and(|line| line.is_empty()) {
            lines.remove(0);
        }
    }
    for line in lines.iter_mut() {
        *line = line
            .trim_start_matches(['-', '*', '•'])
            .trim_start_matches([':', '：'])
            .trim()
            .to_string();
    }
    let message = lines
        .join("\n")
        .trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .to_string();
    if message.len() <= COMMIT_MESSAGE_LIMIT {
        return message;
    }
    let mut cut = COMMIT_MESSAGE_LIMIT;
    while !message.is_char_boundary(cut) {
        cut -= 1;
    }
    message[..cut].trim_end().to_string()
}

pub struct ProcessGitAdapter {
    cwd: PathBuf,
}

impl ProcessGitAdapter {
    pub fn new(cwd: impl AsRef<Path>) -> std::io::Result<Self> {
        let cwd = dunce::canonicalize(cwd)?;
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

    fn staged_diff(&self) -> Result<StagedDiff, String> {
        let mut builder = std::process::Command::new("git");
        builder
            .arg("-C")
            .arg(&self.cwd)
            .args(["diff", "--cached", "--no-color"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        // A `diff <driver>` entry in the repository's own config makes this read
        // spawn grandchildren, so the child runs in its own process group inside
        // a scope that is killed on the way out, as the terminal adapter does.
        xai_tty_utils::detach_std_command(&mut builder);
        let process_scope = xai_tty_utils::ProcessScope::new();
        #[allow(clippy::disallowed_methods)] // reaped by wait() below and kill_all() after it
        let mut child = builder
            .spawn()
            .map_err(|error| format!("无法启动 git: {error}"))?;
        let _process_group = match process_scope.enroll_std(&child) {
            Ok(group) => group,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("无法跟踪 git 进程: {error}"));
            }
        };
        // Both pipes are drained on their own threads: git that could not write
        // would block and the wait() behind it would never return. Only the
        // first COMMIT_DIFF_LIMIT bytes of the diff are retained.
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "git 标准输出不可用".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "git 标准错误不可用".to_string())?;
        let stdout_reader = std::thread::spawn(move || drain_output(stdout, COMMIT_DIFF_LIMIT));
        let stderr_reader = std::thread::spawn(move || drain_output(stderr, 4096));
        let status = child
            .wait()
            .map_err(|error| format!("等待 git 失败: {error}"))?;
        process_scope.kill_all();
        let (bytes, truncated) = stdout_reader
            .join()
            .map_err(|_| "读取 git 标准输出失败".to_string())?
            .map_err(|error| format!("读取 git 标准输出失败: {error}"))?;
        let (stderr, _) = stderr_reader
            .join()
            .map_err(|_| "读取 git 标准错误失败".to_string())?
            .unwrap_or_default();
        if !status.success() {
            let stderr = String::from_utf8_lossy(&stderr).trim().to_string();
            return Err(if stderr.is_empty() {
                format!("git exited with {status}")
            } else {
                stderr
            });
        }
        let mut text = String::from_utf8_lossy(&bytes).trim_end().to_string();
        if truncated {
            text.push_str("\n[diff truncated]");
        }
        Ok(StagedDiff { text, truncated })
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

fn ensure_private_staging_dir(root: &Path) -> std::io::Result<()> {
    let staging = root.join(".chaos-staging");
    match std::fs::symlink_metadata(&staging) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => return Err(std::io::Error::other("staging path is not a directory")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(&staging)?;
        }
        Err(error) => return Err(error),
    }
    let canonical = dunce::canonicalize(&staging)?;
    if canonical != staging || !canonical.starts_with(root) {
        return Err(std::io::Error::other("staging path escapes workspace root"));
    }
    Ok(())
}

impl AttachmentStager {
    pub fn validate_name_type_size(
        filename: &str,
        content_type: &str,
        byte_len: u64,
    ) -> Result<(), String> {
        if filename.contains(['/', '\\']) {
            return Err("attachment path separators are not allowed".into());
        }
        let allowed = [
            (".txt", "text/plain"),
            (".md", "text/markdown"),
            (".json", "application/json"),
            (".png", "image/png"),
            (".gif", "image/gif"),
            (".webp", "image/webp"),
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
        let root = dunce::canonicalize(root)?;
        ensure_private_staging_dir(&root)?;
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
        let cwd = dunce::canonicalize(cwd)?;
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
        let mut command_builder = std::process::Command::new("sh");
        command_builder
            .arg("-c")
            .arg(command)
            .current_dir(&self.cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        xai_tty_utils::detach_std_command(&mut command_builder);
        let process_scope = xai_tty_utils::ProcessScope::new();
        #[allow(clippy::disallowed_methods)]
        let mut child = command_builder
            .spawn()
            .map_err(|error| format!("无法启动终端命令: {error}"))?;
        let _process_group = match process_scope.enroll_std(&child) {
            Ok(group) => group,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("无法跟踪终端进程: {error}"));
            }
        };
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "终端标准输出不可用".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "终端错误输出不可用".to_string())?;
        let stdout_limit = self.max_output_bytes;
        let stderr_limit = self.max_output_bytes;
        let stdout_reader = std::thread::spawn(move || drain_output(stdout, stdout_limit));
        let stderr_reader = std::thread::spawn(move || drain_output(stderr, stderr_limit));
        let status = child
            .wait()
            .map_err(|error| format!("等待终端命令失败: {error}"))?;
        process_scope.kill_all();
        let (stdout, stdout_truncated) = stdout_reader
            .join()
            .map_err(|_| "读取终端标准输出失败".to_string())?
            .map_err(|error| format!("读取终端标准输出失败: {error}"))?;
        let (stderr, stderr_truncated) = stderr_reader
            .join()
            .map_err(|_| "读取终端错误输出失败".to_string())?
            .map_err(|error| format!("读取终端错误输出失败: {error}"))?;
        let (bytes, truncated) = if !status.success() && stdout.is_empty() {
            (stderr, stderr_truncated)
        } else {
            (stdout, stdout_truncated)
        };
        let mut text = String::from_utf8_lossy(&bytes).to_string();
        if truncated {
            while text.len() > self.max_output_bytes {
                text.pop();
            }
            text.push_str("\n[output truncated]");
        }
        Ok(TerminalResult {
            output: text,
            exit_code: status.code().unwrap_or(-1),
        })
    }
}

fn drain_output(mut reader: impl std::io::Read, limit: usize) -> std::io::Result<(Vec<u8>, bool)> {
    let mut retained = Vec::with_capacity(limit.min(8192));
    let mut truncated = false;
    let mut buffer = [0; 8192];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let remaining = limit.saturating_sub(retained.len());
        let retained_now = remaining.min(count);
        retained.extend_from_slice(&buffer[..retained_now]);
        truncated |= retained_now < count;
    }
    Ok((retained, truncated))
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
    /// Ask what the serving process actually started with. Read-only, and
    /// answerable in Safe Web Mode, because the browser is entitled to know
    /// which capabilities were withheld from it and why.
    GetHostInfo {
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
    /// Ask what commit message fits what is staged right now. Read-only: it
    /// reads the staged diff and asks the configured Provider. It proposes no
    /// commit, asks for no approval and runs no git command that writes.
    SuggestCommitMessage {
        client_msg_id: String,
        session_id: Uuid,
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
        directories: Vec<String>,
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
    HostInfo {
        info: HostInfo,
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
    /// A commit message the browser may put in its commit form. `truncated`
    /// says the staged diff the Provider saw was cut short, so a short answer
    /// may be short because the model never saw the rest of it.
    CommitMessageSuggestion {
        session_id: Uuid,
        message: String,
        truncated: bool,
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

/// How the serving process keeps what it has learned between restarts.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StateBackend {
    /// Nothing survives the process; a restart loses every session.
    #[default]
    Memory,
    /// The transitional JSON snapshot at a path the operator set at startup.
    JsonFile,
    /// SQLite.
    Sqlite,
}

/// Whether the process serving the browser replaces its own binary.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpdateMode {
    /// The process swaps its own binary and restarts.
    SelfUpdate,
    /// Something outside the process owns updates: `cargo`, `npm`, an installer.
    #[default]
    External,
}

/// Which origins the preview proxy will forward to a forwarded port.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreviewProxyState {
    /// No port is forwarded, so every `/preview` request is refused.
    #[default]
    Disabled,
    /// Ports are forwarded, and only the declared public name may reach them.
    NamedOnly,
    /// Ports are forwarded and an operator opted into loopback-only origins too.
    AnyOrigin,
}

/// One client message Safe Web Mode refuses, and what it would have done.
///
/// `message` is the wire tag rather than a paraphrase, so a client can hide the
/// control that would have sent it instead of letting the user click and be
/// refused.
/// The wire tag of a client message, matching the serde `rename_all` on the
/// enum. Exhaustive on purpose: a new variant does not compile until it is
/// classified.
pub fn safe_mode_tag(message: &ClientMessage) -> &'static str {
    match message {
        ClientMessage::CreateSession { .. } => "create_session",
        ClientMessage::CreateWorkspace { .. } => "create_workspace",
        ClientMessage::ListWorkspaces { .. } => "list_workspaces",
        ClientMessage::ArchiveWorkspace { .. } => "archive_workspace",
        ClientMessage::SwitchWorkspace { .. } => "switch_workspace",
        ClientMessage::Resume { .. } => "resume",
        ClientMessage::Submit { .. } => "submit",
        ClientMessage::Cancel { .. } => "cancel",
        ClientMessage::Snapshot { .. } => "snapshot",
        ClientMessage::Approve { .. } => "approve",
        ClientMessage::Reject { .. } => "reject",
        ClientMessage::RespondQuestion { .. } => "respond_question",
        ClientMessage::ListFiles { .. } => "list_files",
        ClientMessage::ReadFile { .. } => "read_file",
        ClientMessage::SearchFiles { .. } => "search_files",
        ClientMessage::ProposeFileWrite { .. } => "propose_file_write",
        ClientMessage::ProposeTerminal { .. } => "propose_terminal",
        ClientMessage::ProposeGitMutation { .. } => "propose_git_mutation",
        ClientMessage::GetSettings { .. } => "get_settings",
        ClientMessage::GetHostInfo { .. } => "get_host_info",
        ClientMessage::UpdateSettings { .. } => "update_settings",
        ClientMessage::GetGitStatus { .. } => "get_git_status",
        ClientMessage::SuggestCommitMessage { .. } => "suggest_commit_message",
        ClientMessage::ValidateAttachment { .. } => "validate_attachment",
        ClientMessage::BeginAttachment { .. } => "begin_attachment",
        ClientMessage::AttachmentChunk { .. } => "attachment_chunk",
        ClientMessage::CancelAttachment { .. } => "cancel_attachment",
        ClientMessage::FinalizeAttachment { .. } => "finalize_attachment",
        ClientMessage::ImportTuiSession { .. } => "import_tui_session",
        ClientMessage::ValidateProvider { .. } => "validate_provider",
        ClientMessage::ScanMarketplace { .. } => "scan_marketplace",
        ClientMessage::AcceptDiff { .. } => "accept_diff",
        ClientMessage::RollbackDiff { .. } => "rollback_diff",
        ClientMessage::PreviewDiff { .. } => "preview_diff",
    }
}

/// Every client message Safe Web Mode refuses, with what it would have done.
///
/// This is the refusal policy, not a description of it: [`safe_mode_allows`]
/// returns false for exactly these tags. The browser renders it verbatim, so a
/// refusal the panel does not list is a refusal the user finds by clicking, and
/// the panel is tested against the protocol mirror to keep that from happening.
pub const SAFE_MODE_REFUSALS: &[(&str, &str)] = &[
    ("create_workspace", "新建工作区"),
    ("list_workspaces", "列出工作区"),
    ("archive_workspace", "归档工作区"),
    ("switch_workspace", "切换工作区"),
    ("approve", "批准待审操作"),
    ("reject", "驳回待审操作"),
    ("propose_file_write", "把文件写进工作区"),
    ("propose_terminal", "在工作区执行命令"),
    ("propose_git_mutation", "执行 Git 变更"),
    ("update_settings", "修改设置"),
    ("get_git_status", "读取 Git 状态"),
    (
        "suggest_commit_message",
        "读取暂存差异并向 Provider 请求提交信息建议",
    ),
    ("validate_attachment", "校验附件"),
    ("begin_attachment", "开始上传附件"),
    ("attachment_chunk", "上传附件分片"),
    ("cancel_attachment", "取消附件上传"),
    ("finalize_attachment", "把附件落进工作区"),
    ("validate_provider", "探测 Provider 连通性"),
    ("accept_diff", "接受 Diff"),
    ("rollback_diff", "回滚 Diff"),
];

/// Whether Safe Web Mode lets a client send this message.
///
/// The line drawn here is "no workspace or host mutation, not even behind an
/// approval prompt": `propose_file_write`, `propose_terminal` and
/// `propose_git_mutation` are refused although the engine would ask the user
/// first. `finalize_attachment` used to sit on the allowlist, which made it
/// unreachable rather than permissive — an `upload_id` only comes from
/// `begin_attachment`, which is refused — so the one attachment step that writes
/// into the workspace was allowlisted while the three that merely stage bytes
/// were not. The flow is refused as a unit, from either end.
///
/// This lives beside the enum it classifies, and beside [`SAFE_MODE_REFUSALS`],
/// the list the browser renders. A host that enforced a policy without reporting
/// it produced a settings panel that promised nothing while the socket refused
/// messages the panel had never listed;
/// `the_refusal_list_matches_what_the_socket_actually_refuses` in
/// `xai-grok-web/tests/host_info_flow.rs` drives a real socket against the list
/// to keep the two from drifting apart. How many entries that is stays out of
/// prose here: the list gains one every time a message that touches the
/// workspace is added, and a number in a comment goes stale on the very commit
/// that adds it.
#[must_use]
pub fn safe_mode_allows(message: &ClientMessage) -> bool {
    !SAFE_MODE_REFUSALS
        .iter()
        .any(|(tag, _)| *tag == safe_mode_tag(message))
}

/// [`SAFE_MODE_REFUSALS`] as a wire payload.
#[must_use]
pub fn safe_mode_refusals() -> Vec<SafeModeRefusal> {
    SAFE_MODE_REFUSALS
        .iter()
        .map(|(tag, capability)| SafeModeRefusal {
            message: (*tag).to_string(),
            capability: (*capability).to_string(),
        })
        .collect()
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SafeModeRefusal {
    /// Wire tag of the refused client message, `propose_file_write`.
    pub message: String,
    /// What it would have done, in the browser's own words.
    pub capability: String,
}

/// The configuration the serving process actually started with.
///
/// Nothing here is chosen by the browser: each field is a decision the host had
/// to make before it could bind a socket, and the host fills this in once, at
/// startup. It exists because most of the settings panel's categories have no
/// editable field to show — the answer is a fact about the deployment, and the
/// honest control surface says which fact and why the browser cannot change it,
/// rather than rendering an input that goes nowhere.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct HostInfo {
    /// Version of the binary serving the browser, which is not the version of
    /// the bundle the browser loaded.
    pub host_version: String,
    /// The wire protocol the handshake agreed on.
    pub protocol_version: u16,
    /// The address the socket really bound, `127.0.0.1:8787` for the loopback
    /// host. Reachability beyond that is a property of the proxy in front.
    pub bind_addr: String,
    pub state_backend: StateBackend,
    /// Safe Web Mode is on: the refusals listed in `safe_mode_refusals` are in
    /// force right now, not merely available as a policy.
    pub safe_web_mode: bool,
    /// Absolute path of the bound workspace. `None` means the browser can read,
    /// search, write or run in no workspace at all.
    pub workspace_root: Option<String>,
    /// A token has to accompany every request.
    pub token_required: bool,
    /// The public name declared for this host, if any. The `Host`/`Origin` rules
    /// key off it, so the browser is shown the value instead of inferring it
    /// from `location.host`, which a proxy may have rewritten.
    pub public_origin: Option<String>,
    pub preview_proxy: PreviewProxyState,
    /// Ports the preview proxy forwards; empty unless it is enabled.
    pub preview_ports: Vec<u16>,
    pub update_mode: UpdateMode,
    /// What Safe Web Mode withholds, each paired with the error code it returns.
    /// Populated whether or not Safe Web Mode is on, so the panel can say what
    /// turning it on would cost.
    pub safe_mode_refusals: Vec<SafeModeRefusal>,
}

impl Default for HostInfo {
    fn default() -> Self {
        Self {
            host_version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: PROTOCOL_VERSION,
            bind_addr: String::new(),
            state_backend: StateBackend::default(),
            safe_web_mode: false,
            workspace_root: None,
            token_required: false,
            public_origin: None,
            preview_proxy: PreviewProxyState::default(),
            preview_ports: Vec::new(),
            update_mode: UpdateMode::default(),
            safe_mode_refusals: safe_mode_refusals(),
        }
    }
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
        let root = dunce::canonicalize(root)?;
        if !root.is_dir() {
            return Err(std::io::Error::other("workspace root is not a directory"));
        }
        ensure_private_staging_dir(&root)?;
        Ok(Self {
            root: Arc::new(root),
        })
    }

    /// The canonical path this adapter is confined to. Reported to the browser
    /// so the settings panel can name the workspace it is talking about instead
    /// of the workspace it assumes.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn contains_staging_component(path: &Path) -> bool {
        path.components()
            .any(|component| component.as_os_str() == ".chaos-staging")
    }

    fn reject_staging_path(relative: &str) -> Result<(), ServerMessage> {
        if Self::contains_staging_component(Path::new(relative)) {
            return Err(Self::path_escape());
        }
        Ok(())
    }

    fn confined(&self, relative: &str) -> Result<PathBuf, ServerMessage> {
        Self::reject_staging_path(relative)?;
        let candidate = self.root.join(relative);
        let canonical = dunce::canonicalize(&candidate).map_err(|_| ServerMessage::Error {
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

    fn list(&self, relative: &str) -> Result<(Vec<String>, Vec<String>), ServerMessage> {
        let path = self.confined(relative)?;
        let mut listed = std::fs::read_dir(path)
            .map_err(|_| ServerMessage::Error {
                code: "list_failed".into(),
                message: "无法读取目录".into(),
            })?
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name() != ".chaos-staging")
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let is_directory = entry.file_type().is_ok_and(|kind| kind.is_dir());
                Some((name, is_directory))
            })
            .collect::<Vec<_>>();
        listed.sort_by(|left, right| left.0.cmp(&right.0));
        let directories = listed
            .iter()
            .filter(|(_, is_directory)| *is_directory)
            .map(|(name, _)| name.clone())
            .collect();
        let entries = listed.into_iter().map(|(name, _)| name).collect();
        Ok((entries, directories))
    }

    fn read(&self, relative: &str) -> Result<String, ServerMessage> {
        let path = self.confined(relative)?;
        let metadata = std::fs::metadata(&path).map_err(|_| ServerMessage::Error {
            code: "read_failed".into(),
            message: "无法读取文件".into(),
        })?;
        if !metadata.is_file() {
            return Err(ServerMessage::Error {
                code: "read_failed".into(),
                message: "目标不是普通文件".into(),
            });
        }
        if metadata.len() > WORKSPACE_READ_LIMIT as u64 {
            return Err(ServerMessage::Error {
                code: "file_too_large".into(),
                message: "文件超过 1 MiB 限制".into(),
            });
        }
        let file = std::fs::File::open(path).map_err(|_| ServerMessage::Error {
            code: "read_failed".into(),
            message: "文件不是可读文本".into(),
        })?;
        let contents = read_workspace_bytes(file).map_err(|_| ServerMessage::Error {
            code: "read_failed".into(),
            message: "文件不是可读文本".into(),
        })?;
        if contents.len() > WORKSPACE_READ_LIMIT {
            return Err(ServerMessage::Error {
                code: "file_too_large".into(),
                message: "文件超过 1 MiB 限制".into(),
            });
        }
        String::from_utf8(contents).map_err(|_| ServerMessage::Error {
            code: "read_failed".into(),
            message: "文件不是可读文本".into(),
        })
    }

    /// Where a relative path lands on disk, resolved by the same rules `write_raw`
    /// enforces. `WorkspaceDiffAdapter` needs the path itself to put bytes back,
    /// and a second copy of these rules would be a second thing to get wrong.
    fn target(&self, relative: &str) -> Result<PathBuf, ServerMessage> {
        Self::reject_staging_path(relative)?;
        let relative_path = Path::new(relative);
        if relative_path.is_absolute() {
            return Err(Self::path_escape());
        }
        let candidate = self.root.join(relative_path);
        let parent = candidate.parent().ok_or_else(|| ServerMessage::Error {
            code: "write_failed".into(),
            message: "无效父目录".into(),
        })?;
        let canonical_parent = dunce::canonicalize(parent).map_err(|_| ServerMessage::Error {
            code: "write_failed".into(),
            message: "父目录不存在".into(),
        })?;
        if !canonical_parent.starts_with(self.root.as_path()) {
            return Err(Self::path_escape());
        }
        match std::fs::symlink_metadata(&candidate) {
            Ok(_) => {
                let canonical = dunce::canonicalize(&candidate).map_err(|_| Self::path_escape())?;
                if !canonical.starts_with(self.root.as_path()) {
                    return Err(Self::path_escape());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(Self::path_escape()),
        }
        Ok(canonical_parent.join(candidate.file_name().ok_or_else(Self::path_escape)?))
    }

    /// The bytes as they sit on disk, `None` when the path is not there. Text is
    /// not required here: an undo point has to remember what a file held even
    /// when it cannot show it.
    fn read_raw(&self, relative: &str) -> Result<Option<Vec<u8>>, ServerMessage> {
        let path = self.target(relative)?;
        match std::fs::metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => {
                return Err(ServerMessage::Error {
                    code: "read_failed".into(),
                    message: "无法读取文件".into(),
                });
            }
            Ok(metadata) if !metadata.is_file() => {
                return Err(ServerMessage::Error {
                    code: "read_failed".into(),
                    message: "目标不是普通文件".into(),
                });
            }
            Ok(metadata) if metadata.len() > WORKSPACE_READ_LIMIT as u64 => {
                return Err(ServerMessage::Error {
                    code: "file_too_large".into(),
                    message: "文件超过 1 MiB 限制".into(),
                });
            }
            Ok(_) => {}
        }
        std::fs::read(path)
            .map(Some)
            .map_err(|_| ServerMessage::Error {
                code: "read_failed".into(),
                message: "无法读取文件".into(),
            })
    }

    fn write_raw(&self, relative: &str, contents: &[u8]) -> Result<usize, ServerMessage> {
        if contents.len() > WORKSPACE_READ_LIMIT {
            return Err(ServerMessage::Error {
                code: "file_too_large".into(),
                message: "文件超过 1 MiB 限制".into(),
            });
        }
        let path = self.target(relative)?;
        std::fs::write(&path, contents).map_err(|_| ServerMessage::Error {
            code: "write_failed".into(),
            message: "无法写入文件".into(),
        })?;
        Ok(contents.len())
    }

    /// Remove a confined file. Rolling back a write that created a path has to
    /// take the path away rather than leave an empty file behind.
    fn remove(&self, relative: &str) -> Result<(), ServerMessage> {
        let path = self.target(relative)?;
        std::fs::remove_file(&path).map_err(|_| ServerMessage::Error {
            code: "write_failed".into(),
            message: "无法删除文件".into(),
        })
    }

    fn write(&self, relative: &str, contents: &str) -> Result<usize, ServerMessage> {
        // This seam only ever carries text, so overwriting a file the browser
        // could not have read is refused: those bytes can be neither shown nor
        // put back, and a write that destroys them silently is not a write this
        // GUI can take responsibility for.
        if let Some(prior) = self.read_raw(relative)?
            && std::str::from_utf8(&prior).is_err()
        {
            return Err(ServerMessage::Error {
                code: "file_not_text".into(),
                message: "目标当前内容不是文本，此入口不会覆盖".into(),
            });
        }
        self.write_raw(relative, contents.as_bytes())
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
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let mut matches = Vec::new();
        for entry in walkdir::WalkDir::new(self.root.as_path())
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| entry.file_name() != ".chaos-staging")
            .filter_map(Result::ok)
        {
            if Self::contains_staging_component(entry.path()) || !entry.file_type().is_file() {
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(self.root.as_path())
                .unwrap_or(entry.path())
                .display()
                .to_string();
            if let Ok(file) = std::fs::File::open(entry.path())
                && let Ok(contents) = read_workspace_bytes(file)
                && contents.len() <= WORKSPACE_READ_LIMIT
                && let Ok(contents) = String::from_utf8(contents)
                && contents.contains(query)
            {
                matches.push(relative);
            }
            if matches.len() >= 100 {
                break;
            }
        }
        Ok(matches)
    }
}

/// How many landed writes stay undoable at once. Every entry keeps a copy of the
/// bytes it replaced, so an unbounded map would be an unbounded memory claim on a
/// host that runs for days; the oldest change is the first to stop being undoable.
pub const DIFF_PROPOSAL_LIMIT: usize = 32;

#[derive(Clone)]
struct RecordedChange {
    relative_path: String,
    before: Option<Vec<u8>>,
    after: Vec<u8>,
}

#[derive(Default)]
struct ProposalLog {
    by_id: HashMap<String, RecordedChange>,
    /// Insertion order, so eviction has a defined victim instead of whatever the
    /// hash map happens to hand back.
    order: VecDeque<String>,
}

fn workspace_message_text(message: ServerMessage) -> String {
    match message {
        ServerMessage::Error { message, .. } => message,
        _ => "workspace 操作失败".into(),
    }
}

/// `DiffAdapter` over the workspace this process already owns.
///
/// The write path records a change once its bytes have landed, so `accept` is a
/// confirmation rather than a write: the file is already what the user approved.
/// `rollback` puts the recorded bytes back, or takes the file away when it did not
/// exist, and refuses when the file has moved since -- a later edit by the user or
/// by git is not this proposal's to erase.
pub struct WorkspaceDiffAdapter {
    workspace: Arc<WorkspaceAdapter>,
    proposals: Mutex<ProposalLog>,
}

impl WorkspaceDiffAdapter {
    #[must_use]
    pub fn new(workspace: Arc<WorkspaceAdapter>) -> Self {
        Self {
            workspace,
            proposals: Mutex::new(ProposalLog::default()),
        }
    }

    /// The one place this adapter locks its log.
    ///
    /// A poisoned lock is reported as what it is: a panic happened while an undo
    /// point was being added or spent, so the log may hold half of one. Continuing
    /// over that would let a rollback act on a proposal nobody recorded.
    fn log(&self) -> std::sync::MutexGuard<'_, ProposalLog> {
        self.proposals.lock().expect("workspace undo point lock")
    }

    /// Record a write that has already landed. Called with the bytes that were on
    /// disk beforehand, because nothing can recover them afterwards.
    ///
    /// # Errors
    /// Returns a refusal when the path would escape the workspace, when either
    /// side is too large, or when the prior bytes are not text.
    pub fn record_change(
        &self,
        proposal_id: &str,
        relative_path: &str,
        before: Option<&[u8]>,
        after: &[u8],
    ) -> Result<(), String> {
        // Checked now rather than at rollback time: a proposal the workspace would
        // refuse to write again must never sit in the map looking undoable.
        self.workspace
            .target(relative_path)
            .map_err(workspace_message_text)?;
        if let Some(prior) = before
            && std::str::from_utf8(prior).is_err()
        {
            return Err(format!(
                "{relative_path} 的旧内容不是文本，无法作为差异保留"
            ));
        }
        if std::str::from_utf8(after).is_err() {
            return Err(format!(
                "{relative_path} 的新内容不是文本，无法作为差异保留"
            ));
        }
        let mut log = self.log();
        while log.order.len() >= DIFF_PROPOSAL_LIMIT {
            let Some(oldest) = log.order.pop_front() else {
                break;
            };
            log.by_id.remove(&oldest);
        }
        log.order.push_back(proposal_id.to_string());
        log.by_id.insert(
            proposal_id.to_string(),
            RecordedChange {
                relative_path: relative_path.to_string(),
                before: before.map(<[u8]>::to_vec),
                after: after.to_vec(),
            },
        );
        Ok(())
    }

    fn peek(&self, proposal_id: &str) -> Result<RecordedChange, String> {
        self.log()
            .by_id
            .get(proposal_id)
            .cloned()
            .ok_or_else(|| "提案不存在或已处理".into())
    }

    fn take(&self, proposal_id: &str) -> Result<RecordedChange, String> {
        let mut log = self.log();
        let change = log
            .by_id
            .remove(proposal_id)
            .ok_or_else(|| "提案不存在或已处理".to_string())?;
        if let Some(position) = log.order.iter().position(|id| id == proposal_id) {
            log.order.remove(position);
        }
        Ok(change)
    }
}

impl DiffAdapter for WorkspaceDiffAdapter {
    fn accept(&self, proposal_id: &str, _summary: &str) -> Result<(), String> {
        // The bytes are already on disk, so accepting must not write anything: it
        // says the change stays, which also takes the undo away.
        self.take(proposal_id).map(|_| ())
    }

    fn rollback(&self, proposal_id: &str) -> Result<(), String> {
        let change = self.peek(proposal_id)?;
        let current = self
            .workspace
            .read_raw(&change.relative_path)
            .map_err(workspace_message_text)?;
        if current.as_deref() != Some(change.after.as_slice()) {
            return Err(format!(
                "{} 在写入之后又被改过，回滚会覆盖那次修改",
                change.relative_path
            ));
        }
        match &change.before {
            Some(prior) => {
                self.workspace
                    .write_raw(&change.relative_path, prior)
                    .map_err(workspace_message_text)?;
            }
            None => {
                self.workspace
                    .remove(&change.relative_path)
                    .map_err(workspace_message_text)?;
            }
        }
        // Only consumed once the bytes are back: a failed restore should stay
        // retryable rather than leave the change unundoable with nothing shown.
        self.take(proposal_id)?;
        Ok(())
    }

    fn preview(&self, proposal_id: &str) -> Result<DiffPreview, String> {
        let change = self.peek(proposal_id)?;
        let as_text = |bytes: &[u8]| -> Result<String, String> {
            std::str::from_utf8(bytes)
                .map(str::to_string)
                .map_err(|_| "内容不是 UTF-8 文本，无法给出文本差异".into())
        };
        Ok(DiffPreview {
            proposal_id: proposal_id.to_string(),
            path: change.relative_path,
            before: change.before.as_deref().map(as_text).transpose()?,
            after: as_text(&change.after)?,
        })
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
    /// The workspace's own adapter, when the host asked for one. Held concretely
    /// and separately from `diff_adapter` because the write path records into it:
    /// a host-supplied `dyn DiffAdapter` that only resolves proposals it was handed
    /// has nothing to record, and must not be told to.
    workspace_diff: Option<Arc<WorkspaceDiffAdapter>>,
    sqlite_store: Option<Arc<SqliteSessionStore>>,
    attachments: Arc<Mutex<HashMap<Uuid, AttachmentUpload>>>,
    settings: Arc<Mutex<GuiSettings>>,
    terminal_adapter: Option<Arc<dyn TerminalAdapter>>,
    git_adapter: Option<Arc<dyn GitAdapter>>,
    marketplace_roots: Arc<Vec<PathBuf>>,
    tui_session_roots: Arc<Vec<PathBuf>>,
    host_info: Arc<HostInfo>,
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

    /// Attach the workspace's own [`WorkspaceDiffAdapter`], so an approved write
    /// can be previewed and taken back instead of only reported as done. The same
    /// adapter answers `preview_diff`/`accept_diff`/`rollback_diff`.
    ///
    /// # Errors
    /// Returns an error when no workspace is configured, since there would be
    /// nothing to diff, or when another diff adapter is already installed, since
    /// silently replacing the one the host chose would hide that decision.
    pub fn with_workspace_diff_adapter(mut self) -> std::io::Result<Self> {
        let Some(workspace) = self.workspace.clone() else {
            return Err(std::io::Error::other("未配置 workspace，无法登记差异提案"));
        };
        if self.diff_adapter.is_some() {
            return Err(std::io::Error::other(
                "已配置 Diff adapter，不再装 workspace 自带的那个",
            ));
        }
        let adapter = Arc::new(WorkspaceDiffAdapter::new(workspace));
        self.workspace_diff = Some(Arc::clone(&adapter));
        self.diff_adapter = Some(adapter);
        Ok(self)
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

    /// [`Self::with_sqlite_store`] plus an explicit prompt adapter, so a host
    /// that persists sessions in SQLite still answers with the provider its
    /// operator configured instead of the development responder.
    pub fn with_sqlite_store_and_adapter(
        path: impl AsRef<Path>,
        adapter: Option<Arc<dyn PromptAdapter>>,
    ) -> rusqlite::Result<Self> {
        let store = Arc::new(SqliteSessionStore::open(path)?);
        let state = store.load_state()?;
        Ok(Self::with_state(
            state,
            None,
            adapter,
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
        // Derived before `path` and `sqlite_store` are moved into the struct.
        let state_backend = if sqlite_store.is_some() {
            StateBackend::Sqlite
        } else if path.is_some() {
            StateBackend::JsonFile
        } else {
            StateBackend::Memory
        };
        let workspace_root = workspace
            .as_ref()
            .map(|adapter| adapter.root().display().to_string());
        Self {
            events,
            state: Arc::new(Mutex::new(state)),
            store_path: path.map(Arc::new),
            adapter,
            tool_adapter,
            diff_adapter,
            workspace,
            workspace_diff: None,
            sqlite_store,
            attachments: Arc::new(Mutex::new(HashMap::new())),
            settings: Arc::new(Mutex::new(initial_settings)),
            terminal_adapter: None,
            git_adapter: None,
            marketplace_roots: Arc::new(Vec::new()),
            tui_session_roots: Arc::new(Vec::new()),
            host_info: Arc::new(HostInfo {
                state_backend,
                workspace_root,
                ..HostInfo::default()
            }),
        }
    }

    /// Fill in the parts of [`HostInfo`] that only the serving process knows.
    ///
    /// `state_backend` and `workspace_root` are derived by the engine from the
    /// store and workspace it was actually handed, and any value the host sets
    /// for them is ignored: a panel claiming SQLite, or naming a workspace the
    /// engine cannot reach, would be worse than no panel at all.
    #[must_use]
    pub fn with_host_info(mut self, configure: impl FnOnce(&mut HostInfo)) -> Self {
        let mut info = (*self.host_info).clone();
        configure(&mut info);
        info.state_backend = if self.sqlite_store.is_some() {
            StateBackend::Sqlite
        } else if self.store_path.is_some() {
            StateBackend::JsonFile
        } else {
            StateBackend::Memory
        };
        info.workspace_root = self
            .workspace
            .as_ref()
            .map(|workspace| workspace.root().display().to_string());
        // Same reason: the refusal list is what the transport enforces, and a
        // host that cleared it would show a Safe Web Mode user an empty panel.
        info.safe_mode_refusals = safe_mode_refusals();
        self.host_info = Arc::new(info);
        self
    }

    pub fn with_tui_session_root(mut self, root: impl AsRef<Path>) -> std::io::Result<Self> {
        let root = dunce::canonicalize(root)?;
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
        let root = dunce::canonicalize(root)?;
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
            | ClientMessage::GetHostInfo { client_msg_id }
            | ClientMessage::UpdateSettings { client_msg_id, .. }
            | ClientMessage::GetGitStatus { client_msg_id }
            | ClientMessage::SuggestCommitMessage { client_msg_id, .. }
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
                    ServerMessage::Ack { client_msg_id },
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
                let workspace_id = Self::selected_workspace(workspace_id)
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
                if state
                    .workspaces
                    .get(&workspace_id)
                    .is_none_or(|workspace| workspace.archived)
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
                    events.push(Self::session_snapshot(session_id, session));
                }
                events
            }
            ClientMessage::ArchiveWorkspace { workspace_id, .. } => {
                let Some(workspace) = state.workspaces.get_mut(&workspace_id) else {
                    return vec![Self::error("workspace_unavailable", "工作区不存在")];
                };
                workspace.archived = true;
                let switch_to_fallback = state.active_workspace_id == Some(workspace_id);
                if switch_to_fallback {
                    let fallback = state
                        .workspaces
                        .values()
                        .filter(|candidate| !candidate.archived && candidate.id != workspace_id)
                        .max_by_key(|candidate| (candidate.last_used_sequence, candidate.id))
                        .map(|candidate| candidate.id);
                    state.active_workspace_id = Some(fallback.unwrap_or_else(|| {
                        let fallback_id = Uuid::new_v4();
                        state.workspaces.insert(
                            fallback_id,
                            WorkspaceInfo {
                                id: fallback_id,
                                name: "默认工作区".into(),
                                archived: false,
                                last_used_sequence: 0,
                                last_session_id: None,
                            },
                        );
                        fallback_id
                    }));
                }
                let active_workspace_id = state.active_workspace_id;
                let mut events = vec![ServerMessage::WorkspaceArchived { workspace_id }];
                if let Some(active_workspace_id) = active_workspace_id {
                    if switch_to_fallback {
                        let session_id = match state
                            .workspaces
                            .get(&active_workspace_id)
                            .and_then(|workspace| workspace.last_session_id)
                        {
                            Some(session_id) => session_id,
                            None => {
                                let session_id = Uuid::new_v4();
                                state.sessions.insert(
                                    session_id,
                                    SessionState {
                                        workspace_id: Some(active_workspace_id),
                                        ..SessionState::default()
                                    },
                                );
                                if let Some(workspace) =
                                    state.workspaces.get_mut(&active_workspace_id)
                                {
                                    workspace.last_session_id = Some(session_id);
                                    workspace.last_used_sequence =
                                        workspace.last_used_sequence.saturating_add(1);
                                }
                                session_id
                            }
                        };
                        events.push(ServerMessage::WorkspaceSwitched {
                            workspace_id: active_workspace_id,
                        });
                        if let Some(session) = state.sessions.get(&session_id) {
                            events.push(Self::session_snapshot(session_id, session));
                        }
                    }
                    let mut workspaces = state.workspaces.values().cloned().collect::<Vec<_>>();
                    workspaces
                        .sort_by_key(|workspace| std::cmp::Reverse(workspace.last_used_sequence));
                    events.push(ServerMessage::Workspaces {
                        active_workspace_id,
                        workspaces,
                    });
                }
                events
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
                Some(session)
                    if session.workspace_id.is_some_and(|id| {
                        state
                            .workspaces
                            .get(&id)
                            .is_some_and(|workspace| workspace.archived)
                    }) =>
                {
                    vec![Self::error("workspace_unavailable", "会话工作区已归档")]
                }
                Some(session)
                    if Self::selected_workspace(workspace_id).is_none()
                        || workspace_id == session.workspace_id =>
                {
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
                let Some(workspace_id) = state
                    .sessions
                    .get(&session_id)
                    .and_then(|session| session.workspace_id)
                else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                if state
                    .workspaces
                    .get(&workspace_id)
                    .is_some_and(|workspace| workspace.archived)
                {
                    return vec![Self::error("workspace_unavailable", "会话工作区已归档")];
                }
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
                    .map(|(entries, directories)| {
                        vec![ServerMessage::FilesListed {
                            path: relative_path,
                            entries,
                            directories,
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
            ClientMessage::SuggestCommitMessage {
                client_msg_id,
                session_id,
            } => {
                let Some(workspace_id) = state
                    .sessions
                    .get(&session_id)
                    .and_then(|session| session.workspace_id)
                else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                if state
                    .workspaces
                    .get(&workspace_id)
                    .is_some_and(|workspace| workspace.archived)
                {
                    return vec![Self::error("workspace_unavailable", "会话工作区已归档")];
                }
                let (Some(workspace), Some(git)) = (&self.workspace, &self.git_adapter) else {
                    return vec![Self::error(
                        "workspace_unavailable",
                        "没有配置 workspace 或 Git adapter",
                    )];
                };
                let diff = match git.staged_diff() {
                    Ok(diff) => diff,
                    Err(message) => return vec![Self::error("git_failed", &message)],
                };
                // An empty staged area is not a suggestion the model can make,
                // and sending the empty diff would cost a call to say so.
                if diff.text.trim().is_empty() {
                    return vec![Self::error(
                        "nothing_staged",
                        "暂存区为空，先 stage 文件再请求提交信息建议",
                    )];
                }
                // The suggestion is the Provider's words, so without one there
                // is nothing to offer. The commit form still takes a typed
                // message; this says which half is missing here.
                let Some(adapter) = &self.adapter else {
                    return vec![Self::error(
                        "commit_suggestion_unavailable",
                        "没有配置 Provider，无法生成提交信息建议；提交信息仍可手动填写",
                    )];
                };
                let branch = workspace.git_status().ok().and_then(|(branch, _)| branch);
                let chunks =
                    match adapter.run_prompt(&commit_message_prompt(branch.as_deref(), &diff)) {
                        Ok(chunks) => chunks,
                        Err(message) => return vec![Self::error("agent_failed", &message)],
                    };
                let message = normalize_commit_message(&chunks.concat());
                if message.is_empty() {
                    return vec![Self::error(
                        "commit_suggestion_empty",
                        "Provider 没有返回可用的提交信息",
                    )];
                }
                let Some(session) = state.sessions.get_mut(&session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                session.sequence += 1;
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::CommitMessageSuggestion {
                        session_id,
                        message,
                        truncated: diff.truncated,
                    },
                ]
            }
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
                if AttachmentStager::validate_name_type_size(&filename, &content_type, byte_len)
                    .is_err()
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
                if let Err(error) = WorkspaceAdapter::reject_staging_path(&relative_path) {
                    return vec![error];
                }
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
                let requested = dunce::canonicalize(&root).ok();
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
                let requested = dunce::canonicalize(&root).ok();
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
            ClientMessage::GetHostInfo { .. } => {
                vec![ServerMessage::HostInfo {
                    info: (*self.host_info).clone(),
                }]
            }
            ClientMessage::UpdateSettings {
                base_url, model, ..
            } => {
                if base_url.as_ref().is_some_and(|url| {
                    url.contains('@') || url.contains('#') || !url.starts_with("https://")
                }) {
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
                let Some(workspace_id) = state
                    .sessions
                    .get(&session_id)
                    .and_then(|session| session.workspace_id)
                else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                if state
                    .workspaces
                    .get(&workspace_id)
                    .is_some_and(|workspace| workspace.archived)
                {
                    return vec![Self::error("workspace_unavailable", "会话工作区已归档")];
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
                let Some(workspace_id) = state
                    .sessions
                    .get(&session_id)
                    .and_then(|session| session.workspace_id)
                else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                if state
                    .workspaces
                    .get(&workspace_id)
                    .is_some_and(|workspace| workspace.archived)
                {
                    return vec![Self::error("workspace_unavailable", "会话工作区已归档")];
                }
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
                let Some(workspace_id) = state
                    .sessions
                    .get(&session_id)
                    .and_then(|session| session.workspace_id)
                else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                if state
                    .workspaces
                    .get(&workspace_id)
                    .is_some_and(|workspace| workspace.archived)
                {
                    return vec![Self::error("workspace_unavailable", "会话工作区已归档")];
                }
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
        let Some(session_id) = state.sessions.iter().find_map(|(session_id, session)| {
            session
                .pending_approval
                .as_ref()
                .is_some_and(|pending| pending.request_id == request_id)
                .then_some(*session_id)
        }) else {
            return vec![Self::error("approval_not_found", "审批请求不存在或已处理")];
        };
        let archived = state
            .sessions
            .get(&session_id)
            .and_then(|session| session.workspace_id)
            .is_some_and(|workspace_id| {
                state
                    .workspaces
                    .get(&workspace_id)
                    .is_some_and(|workspace| workspace.archived)
            });
        if archived {
            return vec![Self::error("workspace_unavailable", "会话工作区已归档")];
        }
        let session = state.sessions.get_mut(&session_id).expect("session found");
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
                        session_id,
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
                // What the path holds right now. A rollback restores these bytes
                // and nothing can recover them after the write, so they are read
                // here or the change is not undoable. `None` means no undo point:
                // either the host has no diff seam, or the current bytes could not
                // be read at all and a half-known undo point would be worse than none.
                let mut prior: Option<Option<Vec<u8>>> = None;
                if self.workspace_diff.is_some() {
                    prior = workspace.read_raw(&relative_path).ok();
                }
                match workspace.write(&relative_path, &contents) {
                    Ok(bytes) => {
                        session.messages.push(TimelineMessage {
                            role: "tool".into(),
                            text: format!("wrote {relative_path}"),
                        });
                        // Only once the bytes have landed: an undo point for bytes
                        // that never arrived would offer to take back whatever is
                        // really in the file. A refusal here therefore costs the undo
                        // point and nothing else, because the write did happen.
                        let undo = self.workspace_diff.as_ref().and_then(|diff| {
                            let recorded = prior.as_ref().map(Option::as_deref).unwrap_or(None);
                            let proposal_id = Uuid::new_v4().to_string();
                            diff.record_change(
                                &proposal_id,
                                &relative_path,
                                recorded,
                                contents.as_bytes(),
                            )
                            .ok()?;
                            diff.preview(&proposal_id).ok()
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
                        if let Some(preview) = undo {
                            // The browser's diff tab reads this: without it the
                            // proposal exists but nothing can name it back.
                            session.sequence += 1;
                            events.push(ServerMessage::DiffPreview {
                                session_id: write_session_id,
                                preview,
                            });
                        }
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
                                dunce::canonicalize(parent).map_err(|e| e.to_string())?;
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
                session_id,
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
                            session_id,
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
                                session_id,
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
                                session_id,
                                tool: pending.tool.clone(),
                                progress: "completed".into(),
                                sequence: session.sequence,
                            });
                            session.sequence += 1;
                            events.push(ServerMessage::ToolResult {
                                session_id,
                                tool: pending.tool.clone(),
                                result,
                                sequence: session.sequence,
                            });
                            session.sequence += 1;
                            events.push(ServerMessage::Usage {
                                session_id,
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
            session_id,
            request_id,
            approved: outcome == "executed",
            sequence: session.sequence,
        });
        events.push(ServerMessage::Audit {
            session_id,
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
        let Some(session_id) = state.sessions.iter().find_map(|(session_id, session)| {
            (session.pending_question == Some(question_id)).then_some(*session_id)
        }) else {
            return vec![Self::error("question_not_found", "问题不存在或已回答")];
        };
        let archived = state
            .sessions
            .get(&session_id)
            .and_then(|session| session.workspace_id)
            .is_some_and(|workspace_id| {
                state
                    .workspaces
                    .get(&workspace_id)
                    .is_some_and(|workspace| workspace.archived)
            });
        if archived {
            return vec![Self::error("workspace_unavailable", "会话工作区已归档")];
        }
        let session = state.sessions.get_mut(&session_id).expect("session found");
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
                session_id,
                question_id,
                answer,
                sequence: session.sequence,
            },
            ServerMessage::Audit {
                session_id,
                action: "question".into(),
                outcome: "answered".into(),
                sequence: session.sequence,
            },
        ]
    }

    fn session_snapshot(session_id: Uuid, session: &SessionState) -> ServerMessage {
        ServerMessage::SessionSnapshot {
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
            pending_question: session
                .pending_question
                .map(|question_id| QuestionSnapshot {
                    question_id,
                    prompt: session.pending_question_prompt.clone().unwrap_or_default(),
                }),
        }
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
        if state
            .workspaces
            .get(&session.workspace_id.unwrap_or_default())
            .is_some_and(|workspace| workspace.archived)
        {
            return vec![Self::error("workspace_unavailable", "会话工作区已归档")];
        }
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
        // A refusal is not a resolution: `diff_resolved` clears the browser's
        // preview, so emitting it after a failed rollback would hide the refusal and
        // leave the file looking settled when nothing moved.
        let resolved = result.is_ok();
        if let Err(message) = result {
            events.push(ServerMessage::Error {
                code: "diff_failed".into(),
                message,
            });
        }
        if resolved {
            events.push(ServerMessage::DiffResolved {
                proposal_id,
                action: action.into(),
                sequence: session.sequence,
            });
        }
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

    /// `Workspaces` reports `active_workspace_id` as the nil UUID when there is no
    /// active workspace, because that wire field is not optional. A client that
    /// echoes the placeholder back therefore means "none selected", not "a
    /// workspace that happens to be missing"; reading it literally would answer
    /// `create_session` with `workspace_unavailable`.
    fn selected_workspace(workspace_id: Option<Uuid>) -> Option<Uuid> {
        workspace_id.filter(|id| !id.is_nil())
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
        let resumed = engine.handle(ClientMessage::Resume {
            client_msg_id: "resume".into(),
            session_id,
            workspace_id: None,
        });
        assert!(matches!(
            &resumed[0],
            ServerMessage::Error { code, .. } if code == "workspace_unavailable"
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

    #[cfg(unix)]
    #[test]
    fn approved_write_rejects_dangling_symlink_escape() {
        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_target = outside.path().join("created-outside.txt");
        std::os::unix::fs::symlink(&outside_target, directory.path().join("dangling.txt")).unwrap();
        let engine = Engine::with_workspace(directory.path()).unwrap();
        let session_id = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "dangling-create".into(),
            workspace_id: None,
        })[0]
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
            _ => panic!("expected session"),
        };
        let proposed = engine.handle(ClientMessage::ProposeFileWrite {
            client_msg_id: "dangling-propose".into(),
            session_id,
            relative_path: "dangling.txt".into(),
            contents: "must stay inside workspace".into(),
        });
        let request_id = match proposed.as_slice() {
            [
                ServerMessage::Ack { .. },
                ServerMessage::ToolApprovalRequested { request_id, .. },
            ] => *request_id,
            other => panic!("expected approval request, got {other:?}"),
        };
        let resolved = engine.handle(ClientMessage::Approve {
            client_msg_id: "dangling-approve".into(),
            request_id,
        });
        assert!(resolved.iter().any(
            |event| matches!(event, ServerMessage::Error { code, .. } if code == "path_escape")
        ));
        assert!(!outside_target.exists());
    }

    #[cfg(unix)]
    #[test]
    fn workspace_and_attachment_stagers_reject_staging_symlink_escape() {
        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), directory.path().join(".chaos-staging"))
            .unwrap();

        assert!(Engine::with_workspace(directory.path()).is_err());
        assert!(AttachmentStager::new(directory.path(), 1024).is_err());

        let missing_outside_target = outside.path().join("should-not-exist.part");
        assert!(!missing_outside_target.exists());
        assert!(std::fs::read_dir(outside.path()).unwrap().next().is_none());
    }

    #[test]
    fn workspace_file_access_rejects_attachment_staging_paths() {
        let directory = tempfile::tempdir().unwrap();
        let nested = directory.path().join("repo");
        std::fs::create_dir(&nested).unwrap();
        std::fs::create_dir(directory.path().join(".chaos-staging")).unwrap();
        std::fs::create_dir(nested.join(".chaos-staging")).unwrap();
        std::fs::write(nested.join(".chaos-staging/nested-secret.part"), "staged").unwrap();
        std::fs::write(
            directory.path().join(".chaos-staging/root-secret.part"),
            "staged",
        )
        .unwrap();
        std::fs::write(nested.join("visible.txt"), "visible match").unwrap();
        let engine = Engine::with_workspace(directory.path()).unwrap();
        let session_id = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "staging-access-create".into(),
            workspace_id: None,
        })[0]
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
            _ => panic!("expected session"),
        };

        let read = engine.handle(ClientMessage::ReadFile {
            client_msg_id: "staging-access-read".into(),
            relative_path: ".chaos-staging/upload-secret.part".into(),
        });
        assert!(matches!(
            read.as_slice(),
            [ServerMessage::Error { code, .. }] if code == "path_escape"
        ));
        let list = engine.handle(ClientMessage::ListFiles {
            client_msg_id: "staging-access-list".into(),
            relative_path: ".chaos-staging".into(),
        });
        assert!(matches!(
            list.as_slice(),
            [ServerMessage::Error { code, .. }] if code == "path_escape"
        ));

        let nested_read = engine.handle(ClientMessage::ReadFile {
            client_msg_id: "nested-staging-read".into(),
            relative_path: "repo/.chaos-staging/nested-secret.part".into(),
        });
        assert!(matches!(
            nested_read.as_slice(),
            [ServerMessage::Error { code, .. }] if code == "path_escape"
        ));
        let search = engine.handle(ClientMessage::SearchFiles {
            client_msg_id: "staging-access-search".into(),
            query: "staged".into(),
        });
        assert!(matches!(
            search.as_slice(),
            [ServerMessage::SearchResults { matches, .. }] if matches.is_empty()
        ));

        let proposed = engine.handle(ClientMessage::ProposeFileWrite {
            client_msg_id: "staging-access-propose".into(),
            session_id,
            relative_path: ".chaos-staging/injected.part".into(),
            contents: "must remain private".into(),
        });
        let approval_id = match proposed.as_slice() {
            [
                ServerMessage::Ack { .. },
                ServerMessage::ToolApprovalRequested { request_id, .. },
            ] => *request_id,
            other => panic!("expected approval request, got {other:?}"),
        };
        let written = engine.handle(ClientMessage::Approve {
            client_msg_id: "staging-access-approve".into(),
            request_id: approval_id,
        });
        assert!(written.iter().any(
            |event| matches!(event, ServerMessage::Error { code, .. } if code == "path_escape")
        ));
        assert!(
            !directory
                .path()
                .join(".chaos-staging/injected.part")
                .exists()
        );

        let session_id = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "staging-finalize-create".into(),
            workspace_id: None,
        })[0]
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
            _ => panic!("expected session"),
        };
        let upload_id = match engine.handle(ClientMessage::BeginAttachment {
            client_msg_id: "staging-finalize-begin".into(),
            session_id,
            filename: "private.txt".into(),
            content_type: "text/plain".into(),
            byte_len: 5,
        })[1]
        {
            ServerMessage::AttachmentStarted { upload_id, .. } => upload_id,
            _ => panic!("expected upload"),
        };
        let chunked = engine.handle(ClientMessage::AttachmentChunk {
            client_msg_id: "staging-finalize-chunk".into(),
            upload_id,
            chunk: Base64Engine::encode(&base64::engine::general_purpose::STANDARD, b"hello"),
        });
        assert!(
            chunked.iter().any(|event| matches!(
                event,
                ServerMessage::AttachmentProgress { received: 5, .. }
            ))
        );
        let finalize_rejected = engine.handle(ClientMessage::FinalizeAttachment {
            client_msg_id: "staging-finalize-rejected".into(),
            upload_id,
            relative_path: "repo/.chaos-staging/exfiltrated.txt".into(),
        });
        assert!(matches!(
            finalize_rejected.as_slice(),
            [ServerMessage::Error { code, .. }] if code == "path_escape"
        ));
        assert!(!nested.join(".chaos-staging/exfiltrated.txt").exists());
        let finalize_after_reject = engine.handle(ClientMessage::FinalizeAttachment {
            client_msg_id: "staging-finalize-retry".into(),
            upload_id,
            relative_path: "repo/restored.txt".into(),
        });
        let approval_id = match finalize_after_reject.as_slice() {
            [
                ServerMessage::Ack { .. },
                ServerMessage::ToolApprovalRequested { request_id, .. },
            ] => *request_id,
            other => panic!("expected retry approval, got {other:?}"),
        };
        let finalized = engine.handle(ClientMessage::Approve {
            client_msg_id: "staging-finalize-approve".into(),
            request_id: approval_id,
        });
        assert!(finalized.iter().any(|event| matches!(event, ServerMessage::AttachmentCompleted { path, .. } if path == "repo/restored.txt")));
        assert_eq!(
            std::fs::read(nested.join("restored.txt")).unwrap(),
            b"hello"
        );
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
        assert!(
            stager
                .stage_chunks(r"nested\escape.txt", "text/plain", vec![Ok(b"x".to_vec())])
                .is_err()
        );
        assert_eq!(
            std::fs::read_dir(directory.path().join(".chaos-staging"))
                .unwrap()
                .count(),
            1,
            "rejected separator paths must not leave partial files"
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
        assert!(run(&["config", "user.name", "Chaos Test"]).status.success());
        assert!(
            run(&["config", "user.email", "chaos-test@example.invalid"])
                .status
                .success()
        );
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

    /// One commit plus one staged edit: a suggestion has something to be about,
    /// and the returned head is the value the read-only tests compare against.
    fn repo_with_staged_edit(staged_contents: &str) -> (tempfile::TempDir, String) {
        let directory = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .args(["-C", directory.path().to_str().unwrap()])
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        };
        git(&["init", "-q"]);
        git(&["config", "user.name", "Chaos Test"]);
        git(&["config", "user.email", "chaos-test@example.invalid"]);
        std::fs::write(directory.path().join("note.txt"), "one\n").unwrap();
        git(&["add", "--", "note.txt"]);
        git(&["commit", "-q", "-m", "base"]);
        let head = git(&["rev-parse", "HEAD"]);
        std::fs::write(directory.path().join("note.txt"), staged_contents).unwrap();
        git(&["add", "--", "note.txt"]);
        (directory, head)
    }

    fn git_head(root: &Path) -> String {
        let output = std::process::Command::new("git")
            .args(["-C", root.to_str().unwrap(), "rev-parse", "HEAD"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    struct RecordingPromptAdapter {
        reply: &'static str,
        seen: Mutex<Vec<String>>,
    }

    impl PromptAdapter for RecordingPromptAdapter {
        fn run_prompt(&self, prompt: &str) -> Result<Vec<String>, String> {
            self.seen.lock().unwrap().push(prompt.to_string());
            Ok(vec![self.reply.to_string()])
        }
    }

    /// The host shape the Web binary assembles for a workspace root: the same
    /// workspace adapter, the same `ProcessGitAdapter`, and a prompt adapter.
    fn engine_for_repo(root: &Path, reply: &'static str) -> (Engine, Arc<RecordingPromptAdapter>) {
        let recorder = Arc::new(RecordingPromptAdapter {
            reply,
            seen: Mutex::new(Vec::new()),
        });
        let adapter: Arc<dyn PromptAdapter> = recorder.clone();
        let engine = Engine::with_workspace_and_adapter(root, Some(adapter))
            .unwrap()
            .with_git_adapter(ProcessGitAdapter::new(root).unwrap());
        (engine, recorder)
    }

    fn new_session(engine: &Engine, client_msg_id: &str) -> Uuid {
        let events = engine.handle(ClientMessage::CreateSession {
            client_msg_id: client_msg_id.into(),
            workspace_id: None,
        });
        match &events[0] {
            ServerMessage::SessionCreated { session_id, .. } => *session_id,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn staged_diff_reports_only_what_is_staged() {
        let (directory, _) = repo_with_staged_edit("two\n");
        // The same file edited again without staging: the next commit would not
        // record it, so the diff the Provider sees must not either.
        std::fs::write(directory.path().join("note.txt"), "two\nthree\n").unwrap();
        let diff = ProcessGitAdapter::new(directory.path())
            .unwrap()
            .staged_diff()
            .unwrap();
        assert!(diff.text.contains("+two"), "{}", diff.text);
        assert!(
            !diff.text.contains("+three"),
            "unstaged content leaked into the staged diff: {}",
            diff.text
        );
        assert!(!diff.truncated);
    }

    #[test]
    fn staged_diff_stops_at_the_limit_and_says_so() {
        let big = "y".repeat(COMMIT_DIFF_LIMIT * 2);
        let (directory, _) = repo_with_staged_edit(&format!("first line\n{big}\n"));
        let diff = ProcessGitAdapter::new(directory.path())
            .unwrap()
            .staged_diff()
            .unwrap();
        assert!(diff.truncated, "a 48 KiB diff cannot fit the limit");
        assert!(diff.text.ends_with("[diff truncated]"), "{}", &diff.text);
        assert!(
            diff.text.len() <= COMMIT_DIFF_LIMIT + "\n[diff truncated]".len(),
            "the retained diff was {} bytes over the limit",
            diff.text.len().saturating_sub(COMMIT_DIFF_LIMIT)
        );
    }

    #[cfg(target_os = "linux")]
    fn git_in(root: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(["-C", root.to_str().unwrap()])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// `/proc` answers this without a libc call: a process is gone once its
    /// entry disappears, and a zombie or a just-reaped one is not running.
    #[cfg(target_os = "linux")]
    fn process_is_live(pid: u32) -> bool {
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            return false;
        };
        // The line is `pid (comm) state …`, and comm may itself hold spaces and
        // parentheses, so the state is the word after the last `)`.
        let Some(state) = stat
            .rsplit(')')
            .next()
            .and_then(|rest| rest.split_whitespace().next())
        else {
            return false;
        };
        !matches!(state, "Z" | "X")
    }

    /// A repository's own config can turn this read into a process tree: an
    /// external diff driver runs as git's child, and a driver of the repository's
    /// choosing may leave a background process behind. `staged_diff()` starts git
    /// in a process group of its own inside a `ProcessScope` and kills that group
    /// on the way out, so the driver's background process dies with the read.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_diff_driver_background_process_does_not_outlive_the_staged_diff_read() {
        let (directory, _) = repo_with_staged_edit("two\n");
        let root = directory.path();
        // Relative paths only: git runs an external diff driver from the top
        // level of the worktree, so neither the config nor the script has to
        // survive a temp directory whose name needs shell quoting. The redirect
        // matters: a background process that keeps the inherited stdout open
        // would hold the diff pipe, and then this read would be bounded by that
        // process's lifetime instead of by the reaping this test is about.
        std::fs::write(
            root.join("driver.sh"),
            "#!/bin/sh\nsleep 120 >/dev/null 2>&1 &\necho $! > driver-child.pid\nexit 0\n",
        )
        .unwrap();
        git_in(root, &["config", "diff.chaos.command", "sh driver.sh"]);
        std::fs::write(root.join(".gitattributes"), "*.txt diff=chaos\n").unwrap();
        git_in(root, &["add", "--", "driver.sh", ".gitattributes"]);

        ProcessGitAdapter::new(root)
            .unwrap()
            .staged_diff()
            .expect("the driver exits successfully, so the read succeeds");

        let recorded = std::fs::read_to_string(root.join("driver-child.pid"))
            .expect("the driver recorded its background process");
        let pid: u32 = recorded.trim().parse().expect("a pid was recorded");
        // The recorded process is a `sleep 120`, so it cannot have finished on
        // its own inside this test; anything that reaps it reaped it on purpose.
        // Death is not something this waits for either: kill_all() ran before
        // staged_diff() returned, so the loop only waits out the kernel dropping
        // the /proc entry.
        for _ in 0..200 {
            if !process_is_live(pid) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        panic!(
            "pid {pid} outlived the staged-diff read; nothing reaped the driver's process group"
        );
    }

    #[test]
    fn commit_message_suggestion_asks_the_provider_about_the_staged_diff() {
        let (directory, head) = repo_with_staged_edit("two\n");
        let (engine, recorder) = engine_for_repo(
            directory.path(),
            "改写说明段落\n\n因为旧措辞描述的是上一个版本。",
        );
        let session_id = new_session(&engine, "suggest-session");
        let events = engine.handle(ClientMessage::SuggestCommitMessage {
            client_msg_id: "suggest-1".into(),
            session_id,
        });
        assert!(matches!(&events[0], ServerMessage::Ack { .. }));
        match &events[1] {
            ServerMessage::CommitMessageSuggestion {
                session_id: event_session,
                message,
                truncated,
            } => {
                assert_eq!(*event_session, session_id);
                assert_eq!(message, "改写说明段落\n\n因为旧措辞描述的是上一个版本。");
                assert!(!truncated);
            }
            other => panic!("{other:?}"),
        }

        let prompt = recorder.seen.lock().unwrap()[0].clone();
        assert!(
            prompt.contains("+two"),
            "the staged hunk never reached the prompt: {prompt}"
        );
        assert!(
            prompt.contains("---START STAGED DIFF---") && prompt.contains("---END STAGED DIFF---"),
            "the diff has to be delimited from the instruction: {prompt}"
        );
        assert!(
            prompt.contains("master") || prompt.contains("main"),
            "the branch was not offered as context: {prompt}"
        );

        // Read-only: no approval was requested, nothing was executed, and the
        // repository still points at the commit made before the request.
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, ServerMessage::ToolApprovalRequested { .. })),
            "{events:?}"
        );
        assert_eq!(git_head(directory.path()), head);
    }

    #[test]
    fn commit_message_suggestion_names_the_missing_half() {
        // No Provider: the wording has to come from somewhere, and the commit
        // form still takes what the user types.
        let (directory, _) = repo_with_staged_edit("two\n");
        let engine = Engine::with_workspace(directory.path())
            .unwrap()
            .with_git_adapter(ProcessGitAdapter::new(directory.path()).unwrap());
        let session_id = new_session(&engine, "suggest-no-provider");
        let events = engine.handle(ClientMessage::SuggestCommitMessage {
            client_msg_id: "suggest-no-provider-1".into(),
            session_id,
        });
        assert!(
            matches!(&events[0], ServerMessage::Error { code, .. } if code == "commit_suggestion_unavailable")
        );
    }

    #[test]
    fn empty_stage_is_named_before_the_provider_is_asked() {
        let (directory, _) = repo_with_staged_edit("two\n");
        let git = |args: &[&str]| {
            assert!(
                std::process::Command::new("git")
                    .args(["-C", directory.path().to_str().unwrap()])
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        };
        // Unstage everything: the staged area is now genuinely empty.
        git(&["restore", "--staged", "--", "note.txt"]);
        let (engine, recorder) = engine_for_repo(directory.path(), "不该被调用");
        let session_id = new_session(&engine, "suggest-blank");
        let events = engine.handle(ClientMessage::SuggestCommitMessage {
            client_msg_id: "suggest-blank-1".into(),
            session_id,
        });
        assert!(
            matches!(&events[0], ServerMessage::Error { code, .. } if code == "nothing_staged"),
            "{events:?}"
        );
        assert!(
            recorder.seen.lock().unwrap().is_empty(),
            "an empty stage must not cost a Provider call"
        );
    }

    #[test]
    fn a_provider_failure_is_reported_instead_of_a_blank_suggestion() {
        struct FailingPromptAdapter;
        impl PromptAdapter for FailingPromptAdapter {
            fn run_prompt(&self, _prompt: &str) -> Result<Vec<String>, String> {
                Err("provider 返回 500".into())
            }
        }
        let (directory, head) = repo_with_staged_edit("two\n");
        let engine = Engine::with_workspace_and_adapter(
            directory.path(),
            Some(Arc::new(FailingPromptAdapter) as Arc<dyn PromptAdapter>),
        )
        .unwrap()
        .with_git_adapter(ProcessGitAdapter::new(directory.path()).unwrap());
        let session_id = new_session(&engine, "suggest-failing");
        let events = engine.handle(ClientMessage::SuggestCommitMessage {
            client_msg_id: "suggest-failing-1".into(),
            session_id,
        });
        assert!(
            matches!(&events[0], ServerMessage::Error { code, message } if code == "agent_failed" && message == "provider 返回 500"),
            "{events:?}"
        );
        assert_eq!(git_head(directory.path()), head);
    }

    #[test]
    fn a_session_the_host_no_longer_holds_is_refused_before_anything_is_read() {
        // Sessions live in the host process, so a page talking to a restarted host
        // holds an id nothing answers. That has to be the named reason, and it must
        // not cost a Provider call or touch the repository.
        let (directory, head) = repo_with_staged_edit("two\n");
        let (engine, recorder) = engine_for_repo(directory.path(), "不该被调用");
        let events = engine.handle(ClientMessage::SuggestCommitMessage {
            client_msg_id: "suggest-gone".into(),
            session_id: Uuid::new_v4(),
        });
        assert!(
            matches!(&events[0], ServerMessage::Error { code, .. } if code == "session_not_found"),
            "{events:?}"
        );
        assert!(
            recorder.seen.lock().unwrap().is_empty(),
            "a refused session id must not reach the Provider"
        );
        assert_eq!(git_head(directory.path()), head);
    }

    #[test]
    fn a_host_without_a_git_adapter_refuses_rather_than_inventing_wording() {
        // The Web binary wires a Git adapter; a host built without one cannot read
        // the staged area at all, and must say which half is missing instead of
        // asking the Provider about an empty diff.
        let (directory, head) = repo_with_staged_edit("two\n");
        let recorder = Arc::new(RecordingPromptAdapter {
            reply: "不该被调用",
            seen: Mutex::new(Vec::new()),
        });
        let adapter: Arc<dyn PromptAdapter> = recorder.clone();
        let engine = Engine::with_workspace_and_adapter(directory.path(), Some(adapter)).unwrap();
        let session_id = new_session(&engine, "suggest-no-git");
        let events = engine.handle(ClientMessage::SuggestCommitMessage {
            client_msg_id: "suggest-no-git-1".into(),
            session_id,
        });
        assert!(
            matches!(&events[0], ServerMessage::Error { code, .. } if code == "workspace_unavailable"),
            "{events:?}"
        );
        assert!(
            recorder.seen.lock().unwrap().is_empty(),
            "without a diff there is nothing to send"
        );
        assert_eq!(git_head(directory.path()), head);
    }

    #[test]
    fn a_staged_diff_that_cannot_be_read_names_git_as_the_failure() {
        // A workspace directory removed underneath a running host: the session and
        // the workspace are still known, so the failure surfaces at the read itself.
        // It has to be reported as a failed read, not as an empty stage, which would
        // tell the user to stage files when the real problem is the repository.
        let (directory, _) = repo_with_staged_edit("two\n");
        let (engine, recorder) = engine_for_repo(directory.path(), "不该被调用");
        let session_id = new_session(&engine, "suggest-no-repo");
        std::fs::remove_dir_all(directory.path().join(".git")).unwrap();
        let events = engine.handle(ClientMessage::SuggestCommitMessage {
            client_msg_id: "suggest-no-repo-1".into(),
            session_id,
        });
        let ServerMessage::Error { code, message } = &events[0] else {
            panic!("expected a refusal, got {events:?}");
        };
        assert_eq!(code, "git_failed");
        assert!(
            !message.trim().is_empty(),
            "the reason git gave has to reach the commit form: {message:?}"
        );
        assert!(
            recorder.seen.lock().unwrap().is_empty(),
            "a failed read must not be sent to the Provider as if it were a diff"
        );
    }

    #[test]
    fn normalize_commit_message_removes_what_a_model_wraps_around_an_answer() {
        assert_eq!(
            normalize_commit_message("```\nfeat: 支持暂存差异\n\n因为需要上下文\n```"),
            "feat: 支持暂存差异\n\n因为需要上下文"
        );
        assert_eq!(
            normalize_commit_message("- 修复按钮无响应\n"),
            "修复按钮无响应"
        );
        assert_eq!(
            normalize_commit_message("这是建议：\n\n修复按钮无响应"),
            "修复按钮无响应"
        );
        assert_eq!(
            normalize_commit_message("  \"带引号的措辞\"  "),
            "带引号的措辞"
        );
        let long = normalize_commit_message(&"补".repeat(500));
        assert!(long.len() <= COMMIT_MESSAGE_LIMIT);
        assert!(long.is_char_boundary(long.len()));
        assert!(
            long.chars().all(|c| c == '补'),
            "a cut must not split a char"
        );
        assert_eq!(normalize_commit_message("   "), "");
    }

    #[test]
    fn commit_message_prompt_marks_a_truncated_diff() {
        let full = commit_message_prompt(
            Some("main"),
            &StagedDiff {
                text: "+one".into(),
                truncated: false,
            },
        );
        assert!(!full.contains("截断"));
        assert!(full.contains("当前分支：main"));
        let cut = commit_message_prompt(
            None,
            &StagedDiff {
                text: "+one".into(),
                truncated: true,
            },
        );
        assert!(cut.contains("截断"), "{cut}");
        assert!(!cut.contains("当前分支"));
        assert!(cut.ends_with("---END STAGED DIFF---"));
    }

    #[test]
    fn a_replayed_suggestion_request_asks_the_provider_once() {
        let (directory, _) = repo_with_staged_edit("two\n");
        let (engine, recorder) = engine_for_repo(directory.path(), "补充说明");
        let session_id = new_session(&engine, "suggest-dedup-session");
        let first = engine.handle(ClientMessage::SuggestCommitMessage {
            client_msg_id: "suggest-dedup".into(),
            session_id,
        });
        let replay = engine.handle(ClientMessage::SuggestCommitMessage {
            client_msg_id: "suggest-dedup".into(),
            session_id,
        });
        assert!(
            matches!(&first[1], ServerMessage::CommitMessageSuggestion { .. }),
            "{first:?}"
        );
        // A socket replay may not pay the Provider a second time, and may not
        // hand the browser a second answer for the same request.
        assert!(
            matches!(replay.as_slice(), [ServerMessage::Ack { .. }]),
            "{replay:?}"
        );
        assert_eq!(recorder.seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_suggestion_built_from_a_truncated_diff_says_so() {
        let big = "y".repeat(COMMIT_DIFF_LIMIT * 2);
        let (directory, _) = repo_with_staged_edit(&format!("first line\n{big}\n"));
        let (engine, recorder) = engine_for_repo(directory.path(), "补充说明");
        let session_id = new_session(&engine, "suggest-truncated-session");
        let events = engine.handle(ClientMessage::SuggestCommitMessage {
            client_msg_id: "suggest-truncated".into(),
            session_id,
        });
        match &events[1] {
            ServerMessage::CommitMessageSuggestion { truncated, .. } => {
                assert!(*truncated, "the browser was told the whole diff was read")
            }
            other => panic!("{other:?}"),
        }
        let prompt = recorder.seen.lock().unwrap()[0].clone();
        assert!(
            prompt.contains("截断"),
            "the model was not told it was reading a cut-off diff: {prompt}"
        );
    }

    #[test]
    fn terminal_process_adapter_runs_in_fixed_cwd_and_truncates_output() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("marker.txt"), "cwd").unwrap();
        let cwd_adapter = ProcessTerminalAdapter::new(directory.path(), 1024).unwrap();
        let cwd = cwd_adapter.run("pwd").unwrap();
        assert_eq!(cwd.exit_code, 0);
        assert_eq!(cwd.output.trim(), directory.path().to_str().unwrap());

        let adapter = ProcessTerminalAdapter::new(directory.path(), 8).unwrap();
        let truncated = adapter.run("printf 123456789").unwrap();
        assert_eq!(truncated.exit_code, 0);
        assert_eq!(truncated.output, "12345678\n[output truncated]");
    }

    #[test]
    fn terminal_process_adapter_drains_both_streams_and_bounds_fallback_output() {
        let directory = tempfile::tempdir().unwrap();
        let adapter = ProcessTerminalAdapter::new(directory.path(), 8).unwrap();
        let result = adapter
            .run("head -c 65536 /dev/zero | tr '\\0' x; head -c 65536 /dev/zero | tr '\\0' y >&2; exit 9")
            .unwrap();
        assert_eq!(result.exit_code, 9);
        assert_eq!(result.output, "xxxxxxxx\n[output truncated]");

        let stderr = adapter
            .run("head -c 65536 /dev/zero | tr '\\0' y >&2; exit 9")
            .unwrap();
        assert_eq!(stderr.exit_code, 9);
        assert_eq!(stderr.output, "yyyyyyyy\n[output truncated]");
    }

    #[test]
    fn terminal_process_adapter_respects_zero_limit_and_utf8_byte_boundary() {
        let directory = tempfile::tempdir().unwrap();
        let zero_limit = ProcessTerminalAdapter::new(directory.path(), 0).unwrap();
        let empty = zero_limit.run("printf output").unwrap();
        assert_eq!(empty.output, "\n[output truncated]");

        let utf8_limit = ProcessTerminalAdapter::new(directory.path(), 3).unwrap();
        let utf8 = utf8_limit.run("printf 'éé'").unwrap();
        assert_eq!(utf8.output, "é\n[output truncated]");
        assert!(utf8.output.starts_with('é'));
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
    fn attachment_validation_rejects_cross_platform_path_separators() {
        let engine = Engine::new();
        for filename in ["safe/../secret.txt", r"safe\..\secret.txt"] {
            let rejected = engine.handle(ClientMessage::ValidateAttachment {
                client_msg_id: format!("reject-{filename}"),
                filename: filename.into(),
                content_type: "text/plain".into(),
                byte_len: 4,
            });
            assert!(
                matches!(
                    rejected.as_slice(),
                    [ServerMessage::Error { code, .. }] if code == "attachment_rejected"
                ),
                "filename {filename:?} was not rejected: {rejected:?}"
            );
            let rejected_begin = engine.handle(ClientMessage::BeginAttachment {
                client_msg_id: format!("reject-begin-{filename}"),
                session_id: Uuid::new_v4(),
                filename: filename.into(),
                content_type: "text/plain".into(),
                byte_len: 4,
            });
            assert!(
                matches!(
                    rejected_begin.as_slice(),
                    [ServerMessage::Error { code, .. }] if code == "attachment_rejected"
                ),
                "BeginAttachment accepted filename {filename:?}: {rejected_begin:?}"
            );
        }

        let directory = tempfile::tempdir().unwrap();
        let stager = AttachmentStager::new(directory.path(), 1024).unwrap();
        for filename in ["safe/../secret.txt", r"safe\..\secret.txt"] {
            assert!(
                stager
                    .stage_chunks(filename, "text/plain", [Ok(b"data".to_vec())],)
                    .is_err(),
                "stager must reject filename separators: {filename:?}"
            );
        }
        assert!(
            std::fs::read_dir(directory.path().join(".chaos-staging"))
                .unwrap()
                .next()
                .is_none()
        );

        let accepted = engine.handle(ClientMessage::ValidateAttachment {
            client_msg_id: "accept-basename".into(),
            filename: "secret.txt".into(),
            content_type: "text/plain".into(),
            byte_len: 4,
        });
        assert!(matches!(
            accepted.as_slice(),
            [ServerMessage::AttachmentValidated { filename, .. }] if filename == "secret.txt"
        ));
        let accepted_image = engine.handle(ClientMessage::ValidateAttachment {
            client_msg_id: "accept-image-gif".into(),
            filename: "image.gif".into(),
            content_type: "image/gif".into(),
            byte_len: 4,
        });
        assert!(matches!(
            accepted_image.as_slice(),
            [ServerMessage::AttachmentValidated { filename, .. }] if filename == "image.gif"
        ));
        let accepted_webp = engine.handle(ClientMessage::ValidateAttachment {
            client_msg_id: "accept-image-webp".into(),
            filename: "image.webp".into(),
            content_type: "image/webp".into(),
            byte_len: 4,
        });
        assert!(matches!(
            accepted_webp.as_slice(),
            [ServerMessage::AttachmentValidated { filename, .. }] if filename == "image.webp"
        ));
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

    /// One message per wire tag, so the Safe Web Mode policy is checked against
    /// every message the socket can receive rather than a sample of them.
    fn every_client_message() -> Vec<ClientMessage> {
        let session = Uuid::new_v4();
        let workspace = Uuid::new_v4();
        let request = Uuid::new_v4();
        let upload = Uuid::new_v4();
        let id = |n: u32| format!("msg-{n}");
        vec![
            ClientMessage::CreateSession {
                client_msg_id: id(1),
                workspace_id: None,
            },
            ClientMessage::CreateWorkspace {
                client_msg_id: id(2),
                name: "workspace".into(),
            },
            ClientMessage::ListWorkspaces {
                client_msg_id: id(3),
            },
            ClientMessage::ArchiveWorkspace {
                client_msg_id: id(4),
                workspace_id: workspace,
            },
            ClientMessage::SwitchWorkspace {
                client_msg_id: id(5),
                workspace_id: workspace,
            },
            ClientMessage::Resume {
                client_msg_id: id(6),
                session_id: session,
                workspace_id: None,
            },
            ClientMessage::Submit {
                client_msg_id: id(7),
                session_id: session,
                prompt: "prompt".into(),
            },
            ClientMessage::Cancel {
                client_msg_id: id(8),
                session_id: session,
            },
            ClientMessage::Snapshot {
                client_msg_id: id(9),
                session_id: session,
                workspace_id: None,
            },
            ClientMessage::Approve {
                client_msg_id: id(10),
                request_id: request,
            },
            ClientMessage::Reject {
                client_msg_id: id(11),
                request_id: request,
                reason: "reason".into(),
            },
            ClientMessage::RespondQuestion {
                client_msg_id: id(12),
                question_id: request,
                answer: "answer".into(),
            },
            ClientMessage::ListFiles {
                client_msg_id: id(13),
                relative_path: ".".into(),
            },
            ClientMessage::ReadFile {
                client_msg_id: id(14),
                relative_path: "a.txt".into(),
            },
            ClientMessage::SearchFiles {
                client_msg_id: id(15),
                query: "query".into(),
            },
            ClientMessage::ProposeFileWrite {
                client_msg_id: id(16),
                session_id: session,
                relative_path: "a.txt".into(),
                contents: "contents".into(),
            },
            ClientMessage::ProposeTerminal {
                client_msg_id: id(17),
                session_id: session,
                command: "true".into(),
            },
            ClientMessage::ProposeGitMutation {
                client_msg_id: id(18),
                session_id: session,
                operation: "stage".into(),
                argument: ".".into(),
            },
            ClientMessage::GetSettings {
                client_msg_id: id(19),
            },
            ClientMessage::GetHostInfo {
                client_msg_id: id(20),
            },
            ClientMessage::UpdateSettings {
                client_msg_id: id(21),
                base_url: None,
                model: None,
            },
            ClientMessage::GetGitStatus {
                client_msg_id: id(22),
            },
            ClientMessage::ValidateAttachment {
                client_msg_id: id(23),
                filename: "a.txt".into(),
                byte_len: 1,
                content_type: "text/plain".into(),
            },
            ClientMessage::BeginAttachment {
                client_msg_id: id(24),
                session_id: session,
                filename: "a.txt".into(),
                content_type: "text/plain".into(),
                byte_len: 1,
            },
            ClientMessage::AttachmentChunk {
                client_msg_id: id(25),
                upload_id: upload,
                chunk: "AA==".into(),
            },
            ClientMessage::CancelAttachment {
                client_msg_id: id(26),
                upload_id: upload,
            },
            ClientMessage::FinalizeAttachment {
                client_msg_id: id(27),
                upload_id: upload,
                relative_path: "a.txt".into(),
            },
            ClientMessage::ImportTuiSession {
                client_msg_id: id(28),
                root: ".".into(),
                session_id: "tui".into(),
            },
            ClientMessage::ValidateProvider {
                client_msg_id: id(29),
                base_url: "https://api.example.test/v1".into(),
                model: "model".into(),
            },
            ClientMessage::ScanMarketplace {
                client_msg_id: id(30),
                root: ".".into(),
            },
            ClientMessage::AcceptDiff {
                client_msg_id: id(31),
                session_id: session,
                proposal_id: "proposal".into(),
                summary: "summary".into(),
            },
            ClientMessage::RollbackDiff {
                client_msg_id: id(32),
                session_id: session,
                proposal_id: "proposal".into(),
            },
            ClientMessage::PreviewDiff {
                client_msg_id: id(33),
                session_id: session,
                proposal_id: "proposal".into(),
            },
            ClientMessage::SuggestCommitMessage {
                client_msg_id: id(34),
                session_id: session,
            },
        ]
    }

    #[test]
    fn safe_mode_tags_are_the_serde_wire_tags_of_every_message() {
        use std::collections::HashSet;
        // The panel renders these tags and the refusal list is written in them, so a
        // tag that drifted from serde would silently un-refuse a message.
        let mut tags = HashSet::new();
        for message in every_client_message() {
            let encoded = serde_json::to_value(&message).expect("client message serialises");
            let wire = encoded
                .get("type")
                .and_then(serde_json::Value::as_str)
                .expect("internally tagged enum")
                .to_owned();
            assert_eq!(wire.as_str(), safe_mode_tag(&message), "tag for {wire}");
            assert!(
                tags.insert(wire.clone()),
                "two messages share the tag {wire}"
            );
        }
        assert_eq!(tags.len(), 34, "one sample per protocol message");
    }

    #[test]
    fn safe_mode_policy_never_refuses_the_read_only_surface() {
        let messages = every_client_message();
        for (tag, capability) in SAFE_MODE_REFUSALS {
            assert!(
                !capability.trim().is_empty(),
                "{tag} has no capability text"
            );
            let refused = messages
                .iter()
                .find(|message| safe_mode_tag(message) == *tag)
                .unwrap_or_else(|| {
                    panic!("{tag} is listed as refused but no client message carries that tag")
                });
            assert!(
                !safe_mode_allows(refused),
                "{tag} is listed but the policy allows it"
            );
        }
        let allowed: Vec<&'static str> = messages
            .iter()
            .filter(|message| safe_mode_allows(message))
            .map(safe_mode_tag)
            .collect();
        for expected in [
            "create_session",
            "submit",
            "cancel",
            "snapshot",
            "list_files",
            "read_file",
            "search_files",
            "get_settings",
            "get_host_info",
            "respond_question",
            "scan_marketplace",
            "import_tui_session",
            "preview_diff",
        ] {
            assert!(
                allowed.contains(&expected),
                "{expected} must stay reachable in Safe Web Mode"
            );
        }
        // The two halves have to cover the protocol exactly once, so a refusal can
        // neither overlap the allowed set nor point at nothing.
        assert_eq!(messages.len(), SAFE_MODE_REFUSALS.len() + allowed.len());
        assert_eq!(safe_mode_refusals().len(), SAFE_MODE_REFUSALS.len());
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
        let fragment_url = engine.handle(ClientMessage::UpdateSettings {
            client_msg_id: "fragment-settings".into(),
            base_url: Some("https://api.example.test/v1#fragment".into()),
            model: Some("should-not-persist".into()),
        });
        assert!(matches!(
            &fragment_url[0],
            ServerMessage::Error { code, .. } if code == "invalid_base_url"
        ));
        let settings_after_rejection = engine.handle(ClientMessage::GetSettings {
            client_msg_id: "get-settings-after-rejection".into(),
        });
        assert!(matches!(
            &settings_after_rejection[0],
            ServerMessage::Settings { base_url, model, .. }
                if base_url.as_deref() == Some("https://api.example.test/v1")
                    && model.as_deref() == Some("demo")
        ));
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
    fn workspace_search_empty_query_returns_no_matches() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("note.txt"), "ordinary content").unwrap();
        let engine = Engine::with_workspace(directory.path()).unwrap();
        let messages = engine.handle(ClientMessage::SearchFiles {
            client_msg_id: "empty-search".into(),
            query: String::new(),
        });
        assert!(
            matches!(messages.as_slice(), [ServerMessage::SearchResults { matches, .. }] if matches.is_empty())
        );
    }

    #[test]
    fn workspace_read_stops_after_the_limit_plus_one_byte() {
        struct CountingReader {
            remaining: usize,
            bytes_read: Arc<std::sync::atomic::AtomicUsize>,
        }

        impl std::io::Read for CountingReader {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                let size = buffer.len().min(self.remaining);
                buffer[..size].fill(b'x');
                self.remaining -= size;
                self.bytes_read
                    .fetch_add(size, std::sync::atomic::Ordering::Relaxed);
                Ok(size)
            }
        }

        let bytes_read = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let reader = CountingReader {
            remaining: WORKSPACE_READ_LIMIT * 4,
            bytes_read: bytes_read.clone(),
        };
        let limited = read_workspace_bytes(reader).unwrap();
        assert_eq!(limited.len(), WORKSPACE_READ_LIMIT + 1);
        assert_eq!(
            bytes_read.load(std::sync::atomic::Ordering::Relaxed),
            WORKSPACE_READ_LIMIT + 1
        );
    }

    #[test]
    fn workspace_root_confinement_rejects_escape_and_reads_files() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("hello.txt"), "needle").unwrap();
        std::fs::create_dir(directory.path().join("empty-folder")).unwrap();
        let mut large_contents = vec![b'x'; WORKSPACE_READ_LIMIT + 1];
        large_contents.extend_from_slice(b"needle");
        std::fs::write(directory.path().join("large.txt"), large_contents).unwrap();
        let engine = Engine::with_workspace(directory.path()).unwrap();
        let listed = engine.handle(ClientMessage::ListFiles {
            client_msg_id: "list".into(),
            relative_path: ".".into(),
        });
        assert!(
            matches!(&listed[0], ServerMessage::FilesListed { entries, directories, .. } if entries == &["empty-folder", "hello.txt", "large.txt"] && directories == &["empty-folder"])
        );
        let read = engine.handle(ClientMessage::ReadFile {
            client_msg_id: "read".into(),
            relative_path: "hello.txt".into(),
        });
        assert!(
            matches!(&read[0], ServerMessage::FileContents { contents, .. } if contents == "needle")
        );
        let directory_read = engine.handle(ClientMessage::ReadFile {
            client_msg_id: "read-directory".into(),
            relative_path: "empty-folder".into(),
        });
        assert!(matches!(
            directory_read.as_slice(),
            [ServerMessage::Error { code, .. }] if code == "read_failed"
        ));
        let oversized = engine.handle(ClientMessage::ReadFile {
            client_msg_id: "read-oversized".into(),
            relative_path: "large.txt".into(),
        });
        assert!(matches!(
            oversized.as_slice(),
            [ServerMessage::Error { code, .. }] if code == "file_too_large"
        ));
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
        assert!(matches!(
            &search[0],
            ServerMessage::SearchResults { matches, .. }
                if matches.as_slice() == ["hello.txt"]
        ));
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
