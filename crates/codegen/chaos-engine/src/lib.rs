use base64::Engine as Base64Engine;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use uuid::Uuid;

pub mod chaos_store;
pub mod provider;
pub mod pty;
pub mod remote;
pub mod usage;

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

/// What the session state chose for one run, as opposed to what the host
/// configured when it built the adapter.
///
/// Everything here is a *request*: an adapter that cannot honour it says so in
/// its answer rather than quietly doing something else, because the user picked
/// it in a settings panel and a silently ignored pick is a lie on screen.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ModeInfo {
    pub id: String,
    pub label: String,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ThoughtLevelInfo {
    pub id: String,
    pub label: String,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PlanStep {
    pub id: String,
    pub title: String,
    pub status: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct WorkflowRunInfo {
    pub id: String,
    pub name: String,
    pub status: String,
    pub current_phase: String,
    pub total_phases: usize,
    pub completed_phases: usize,
    pub artifacts: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SubagentInfo {
    pub id: String,
    pub parent_session_id: String,
    pub role: String,
    pub agent_type: String,
    pub status: String,
    pub depth: usize,
}

/// Headless adapter lifecycle update. It intentionally contains only display-safe metadata.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SubagentLifecycleEvent {
    pub event: String,
    pub subagent_id: String,
    pub child_session_id: String,
    pub subagent_type: String,
    pub description: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub run_in_background: Option<bool>,
    #[serde(default)]
    pub started_at_ms: Option<u64>,
    #[serde(default)]
    pub parent_session_id: Option<String>,
    #[serde(default)]
    pub workflow_run_id: Option<String>,
    #[serde(default)]
    pub agent_address: Option<String>,
    #[serde(default)]
    pub attempt_id: Option<String>,
    #[serde(default)]
    pub event_seq: Option<u64>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub parent_tool_call_id: Option<String>,
}

/// Live subagent state owned by the engine for the duration of the process.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RunningSubagentInfo {
    pub child_session_id: String,
    pub subagent_id: String,
    pub parent_session_id: String,
    pub subagent_type: String,
    pub title: String,
    pub description: String,
    pub model: Option<String>,
    pub status: String,
    pub run_in_background: bool,
    pub started_at_ms: u64,
    pub workflow_run_id: Option<String>,
    pub agent_address: Option<String>,
    pub attempt_id: Option<String>,
    pub event_seq: Option<u64>,
    pub duration_ms: Option<u64>,
    pub parent_tool_call_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SessionTargetInfo {
    pub objective: String,
    pub status: String,
    pub token_budget: Option<u64>,
    pub tokens_used: u64,
    pub time_used_seconds: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunOptions {
    /// Model the user selected in the GUI. `None` means "whatever the host was
    /// started with", which is the only case a headless host has.
    pub model: Option<String>,
    pub mode: Option<String>,
    pub thought_level: Option<String>,
    /// The directory this turn belongs to.
    ///
    /// An agent that answers about a repository has to be standing in it, and
    /// "the directory the server process happened to be started in" is not that
    /// directory once one host serves conversations from several. Adapters that
    /// run something (a CLI, a shell) take this as their working directory;
    /// those that cannot say so, do.
    pub working_dir: Option<PathBuf>,
    /// The CLI's own session id to continue, when this conversation is a view of
    /// one the terminal owns.
    ///
    /// This is what "continue the conversation" means here rather than "send the
    /// old messages again and hope": the same agent, with its own context, its
    /// own tools and its own compaction, resumes exactly this session and writes
    /// its answer back into the terminal's own transcript. An adapter with no
    /// CLI to hand falls back to [`Self::history`].
    pub resume_session: Option<String>,
    /// The conversation so far, oldest first, for an adapter that has no memory
    /// of it. The turn being answered is *not* in here — it is the `prompt` the
    /// adapter is called with.
    pub history: Vec<TimelineMessage>,
}

/// How much of a conversation an adapter without its own memory is given.
/// Bounded because the transcript of a long session is megabytes, and an
/// unbounded paste would be both slow and unreadable.
const HISTORY_MAX_MESSAGES: usize = 40;
const HISTORY_MAX_BYTES: usize = 96 * 1024;

impl RunOptions {
    /// The model to ask for, falling back to the one the adapter was built with.
    #[must_use]
    pub fn model_or<'a>(&'a self, configured: &'a str) -> &'a str {
        self.model
            .as_deref()
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .unwrap_or(configured)
    }
}

/// Provider 在一次响应里回报的 token 用量（OpenAI 兼容 `usage` 字段口径）。
///
/// 字段都是"Provider 说了才非零"的事实：聚合端不做任何估算，
/// Provider 没报用量时这就是 `None`，统计里该轮的 token 计数为 0。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProviderUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// `completion_tokens_details.reasoning_tokens`（已含在 output 内）。
    pub reasoning_tokens: u64,
    /// `prompt_tokens_details.cached_tokens`（已含在 input 内）。
    pub cached_tokens: u64,
    /// `cache_creation_input_tokens`（Anthropic 兼容口径，独立于 input）。
    pub cache_creation_tokens: u64,
    /// Provider's full prompt-token denominator for cache hit rate.
    /// OpenAI's `prompt_tokens` already includes cache reads; Anthropic's
    /// `input_tokens` does not, so this normalized total preserves both APIs.
    pub cache_total_tokens: u64,
}

/// 一次流式轮次结束时的报告：文本已通过 `emit` 交付，这里只带账目。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TurnReport {
    pub usage: Option<ProviderUsage>,
}

/// 轮次进行中的非文本进展。文本走 `emit`，这些走 `progress`：
/// 一个在后台静默重试 503 的轮次，从外面看和挂死没有区别，宿主要拿到
/// 「第几次重试、为什么」才能把它画出来。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunProgress {
    /// 上游请求失败，Agent 正在重试。`attempt` 从 1 计。
    ApiRetry {
        attempt: u32,
        max_retries: u32,
        reason: String,
    },
}

/// 流式轮次里 adapter 交给宿主的每一样东西：文本块，或一条非文本进展。
/// 文本与进展共用一个回调，宿主的「记录并广播」闭包就能独占它的输出通道，
/// 不必在两个回调之间拆分同一份可变借用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TurnOutput<'a> {
    Text(&'a str),
    /// 思考增量（`--output-format streaming-json` 的 `thought` 行，ADR-008）。
    /// 只广播事件、不进引擎转录本，与 HTTP 路径现状一致；恢复视图读终端
    /// 自己的 `updates.jsonl`。
    Thought(&'a str),
    /// CLI 流式解析出的工具卡：`tool_call` 是开始，`tool_call_update` 是进度
    /// 或终态。tool_call_id 用 CLI 的真实 id——恢复视图（updates.jsonl）与
    /// live 行同 id，resync 不重影。
    Tool(TurnTool<'a>),
    /// Structured child-session lifecycle from the headless ACP adapter.
    Subagent(SubagentLifecycleEvent),
    Progress(RunProgress),
}

/// 一张工具卡的阶段与载荷。`raw_input` 只在开始时有值，`output_text` 在更新时拼。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnTool<'a> {
    pub started: bool,
    pub tool_call_id: &'a str,
    pub tool_name: &'a str,
    /// 终态：`Some(true)`=completed，`Some(false)`=failed；进度是 `None`。
    pub completed: Option<bool>,
    pub raw_input: &'a str,
    pub output_text: &'a str,
}

/// 一条会话内的排队输入（宿主队列合同，2026-10-12 取证）。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedInput {
    pub queue_item_id: Uuid,
    /// 提交它的 V4 命令 id；渲染端用它在乐观命令表里对账。
    pub source_command_id: String,
    pub text: String,
    pub created_at_ms: u64,
    /// 客户端在信封里声明的 `requestedDelivery`（缺席 = auto）。
    /// 官方 queue item intent 的 `delivery.requested` 必须说这个事实，而不是把
    /// 准入结果抄两遍；旧持久化状态没有该字段，读作 auto。
    #[serde(default = "default_queued_requested_delivery")]
    pub requested_delivery: String,
    /// 提交它的连接的 clientId（信封必带）；旧持久化状态读作 unknown。
    #[serde(default = "default_queued_client_id")]
    pub client_id: String,
}

fn default_queued_requested_delivery() -> String {
    "auto".to_owned()
}

fn default_queued_client_id() -> String {
    "unknown".to_owned()
}

/// 一条会向用户展示的 API 重试状态：Agent 报了什么就记什么。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiRetryStatus {
    pub attempt: u32,
    pub max_retries: u32,
    pub reason: String,
}

/// Boundary for connecting the GUI session state to a real Agent runtime.
/// Implementations return text chunks and never receive GUI credentials.
pub trait PromptAdapter: Send + Sync {
    fn run_prompt(&self, prompt: &str, options: &RunOptions) -> Result<Vec<String>, String>;

    /// Streaming form of [`Self::run_prompt`]: `emit` sees each chunk the moment
    /// it exists and returns `false` once the caller has stopped listening, after
    /// which the implementation must stop producing and return. Stopping is a
    /// request to end the work, not to discard it -- the chunks already emitted
    /// stay in the transcript -- so dropping the response mid-stream is the
    /// correct reaction here, and an adapter that reads from a Provider must not
    /// keep draining a reply nobody is showing.
    ///
    /// The default runs the whole prompt first and replays its chunks, so an
    /// adapter that cannot produce incrementally still answers. `emit` returning
    /// `false` is the caller's decision, never the adapter's failure, so the
    /// default reports `Ok` for it; the caller already knows why it stopped.
    fn run_prompt_stream(
        &self,
        prompt: &str,
        options: &RunOptions,
        emit: &mut dyn FnMut(&str) -> bool,
    ) -> Result<(), String> {
        for chunk in self.run_prompt(prompt, options)? {
            if !emit(&chunk) {
                return Ok(());
            }
        }
        Ok(())
    }

    /// [`Self::run_prompt_stream`] with the turn's account: what the Provider
    /// reported in its `usage` frame, if anything. The default reports an empty
    /// account -- an adapter that never sees usage stays honest that way -- and
    /// only adapters that can read it need to override.
    fn run_prompt_turn(
        &self,
        prompt: &str,
        options: &RunOptions,
        emit: &mut dyn FnMut(&str) -> bool,
    ) -> Result<TurnReport, String> {
        self.run_prompt_stream(prompt, options, emit)?;
        Ok(TurnReport::default())
    }

    /// [`Self::run_prompt_turn`] plus non-text progress: every retry the adapter
    /// observes reaches `deliver` as [`TurnOutput::Progress`] the moment it
    /// happens, so a host can show "retrying (2/8)" while the turn is still
    /// working. Text arrives as [`TurnOutput::Text`] exactly as it did through
    /// `emit`, in order, and a `false` return means the host stopped listening
    /// and production must end. The default has nothing to report -- an adapter
    /// that cannot observe progress stays honest by never delivering Progress --
    /// and adapters that can override this instead of blocking silently.
    fn run_prompt_turn_progressing(
        &self,
        prompt: &str,
        options: &RunOptions,
        deliver: &mut dyn FnMut(TurnOutput<'_>) -> bool,
    ) -> Result<TurnReport, String> {
        self.run_prompt_turn(prompt, options, &mut |text| deliver(TurnOutput::Text(text)))
    }

    /// Which models this adapter can actually serve, asked when the client opens
    /// the model picker. `selected` is the model a run would use right now, so
    /// the answer can say whether that choice is real. The default reports
    /// "unknown" rather than an empty list, because "this adapter never lists
    /// models" and "the provider serves none" are different answers to give a user.
    fn list_models(&self, selected: Option<&str>) -> crate::provider::ProviderHealth {
        let _ = selected;
        crate::provider::ProviderHealth {
            reachable: false,
            detail: Some("该 Provider 不支持查询模型列表，可手动填写模型名".to_string()),
            ..Default::default()
        }
    }

    /// The model a run uses when the user picked none. `None` means the adapter
    /// has no model of its own to fall back to.
    fn default_model(&self) -> Option<&str> {
        None
    }

    /// The endpoint this adapter actually talks to, so a request to validate
    /// "this base URL" can be answered truthfully: an endpoint the host never
    /// configured is reported as such instead of being contacted with the
    /// host's credential.
    fn base_url(&self) -> Option<&str> {
        None
    }
}

/// Boundary for tools that require an explicit approval before execution.
/// The adapter receives only the declared tool and summary, never client
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Default)]
pub struct SessionDiffSummary {
    pub files_changed: usize,
    pub additions: usize,
    pub deletions: usize,
    pub file_paths: Vec<String>,
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
/// The client can stage a generated file of any size, and the diff goes into a
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

/// What the client is offered as the commit message.
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
/// The binary path is explicit so the Desktop host cannot accidentally execute
/// an arbitrary command from a client message.
pub struct HeadlessProcessAdapter {
    binary: std::path::PathBuf,
    cwd: std::path::PathBuf,
}

/// 流式 headless 轮次的输出上限。缓冲路径（`Command::output`）从不设限；这里
/// 的两条管道都是显式读取，总得有个边界，取一个比任何正常转录都宽的值。
const HEADLESS_MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

impl HeadlessProcessAdapter {
    pub fn new(binary: impl Into<std::path::PathBuf>, cwd: impl Into<std::path::PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            cwd: cwd.into(),
        }
    }
}

/// The argument list every headless invocation shares: options first, the prompt
/// last as `--single`'s value. Order matters against the CLI's own parser, so
/// both the buffered and the streaming runner must come from this one builder.
fn headless_args(prompt: &str, options: &RunOptions) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "--no-auto-update".to_string(),
        "--output-format".to_string(),
        "json".to_string(),
    ];
    // A model picked in the GUI has to reach the child process, or the
    // picker would only relabel the pill while the CLI kept its own default.
    // Order matters against the CLI's own parser: `--single` takes the prompt
    // as its *value*, so the prompt must stay last and everything else must
    // come before it. `--single --resume <id> <prompt>` is rejected outright
    // ("a value is required for '--single <PROMPT>'"), which is how a resumed
    // turn silently stopped reaching the CLI at all.
    if let Some(model) = options
        .model
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
    {
        args.push("--model".to_string());
        args.push(model.to_string());
    }
    // The thinking level picked in the GUI rides to the CLI the same way the
    // model does: the CLI owns the actual request, and `--reasoning-effort`
    // (alias `--effort`) is its knob. Without this the picker changed a
    // number in the engine's settings and nothing on the wire.
    if let Some(effort) = options
        .thought_level
        .as_deref()
        .map(str::trim)
        .filter(|level| !level.is_empty())
    {
        args.push("--reasoning-effort".to_string());
        args.push(effort.to_string());
    }
    // Continuing a conversation the terminal owns means asking the terminal's
    // own agent to continue it. `--resume` puts the whole prior conversation,
    // its tool results and its compaction in front of the same model that
    // produced them, and the answer is written back into the same session
    // file the terminal reads — which is the only sense in which two clients
    // can hold *one* conversation. Reconstructing the context from a copy of
    // the text here would be a different conversation that reads the same.
    if let Some(resume) = options
        .resume_session
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        args.push("--resume".to_string());
        args.push(resume.to_string());
    }
    args.push("--single".to_string());
    args.push(prompt.to_string());
    args
}

/// The directory a headless turn runs in: the conversation's own when it still
/// exists, the host's cwd otherwise. A session in `~/project-a` asked from a
/// server started in `~/project-b` would otherwise answer about the wrong
/// repository while showing the right conversation.
fn headless_working_dir<'a>(options: &'a RunOptions, fallback: &'a std::path::Path) -> &'a Path {
    options
        .working_dir
        .as_ref()
        .filter(|dir| dir.is_dir())
        .map(PathBuf::as_path)
        .unwrap_or(fallback)
}

impl PromptAdapter for HeadlessProcessAdapter {
    fn run_prompt(&self, prompt: &str, options: &RunOptions) -> Result<Vec<String>, String> {
        let args = headless_args(prompt, options);
        // The conversation's directory, when it has one that still exists.
        let working_dir = headless_working_dir(options, &self.cwd);
        let mut attempts = 0;
        let output = loop {
            match std::process::Command::new(&self.binary)
                .current_dir(working_dir)
                .args(&args)
                .output()
            {
                Ok(output) => break output,
                Err(error) => {
                    #[cfg(unix)]
                    if error.raw_os_error() == Some(26) && attempts < 5 {
                        attempts += 1;
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        continue;
                    }
                    return Err(format!("无法启动 headless Agent: {error}"));
                }
            }
        };
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

    /// Streaming form (ADR-008): the child runs `--output-format streaming-json`
    /// and talks NDJSON on stdout — one ACP session update per line. The main
    /// thread reads those lines as they arrive (text/thought deltas stream to
    /// the panel live; tool cards become `Tool*` events with the CLI's real
    /// `toolCallId`); stderr moves to a helper thread and keeps feeding
    /// `api_retry` progress lines and failure diagnostics. A host that stops
    /// listening (cancel) is answered by killing the child: a CLI turn now
    /// actually stops mid-run instead of at its end.
    fn run_prompt_turn_progressing(
        &self,
        prompt: &str,
        options: &RunOptions,
        deliver: &mut dyn FnMut(TurnOutput<'_>) -> bool,
    ) -> Result<TurnReport, String> {
        use std::io::BufRead;
        let mut args = headless_args(prompt, options);
        // ACP 原生 NDJSON（reducer 的 AcpLine 枚举是唯一合同来源，见 ADR-008）。
        // headless_args 已带 `--output-format json`（位置 1-2）：这里原位换成
        // streaming-json，而不是追加第二对——CLI 的解析器拒绝重复标志。
        debug_assert!(args.len() >= 3 && args[1] == "--output-format");
        args[2] = "streaming-json".to_string();
        let working_dir = headless_working_dir(options, &self.cwd);
        let mut attempts = 0;
        // 与终端适配器同一套纪律：先 detach，再 enroll 进本次轮次的
        // ProcessScope——一个没被登记的子进程会活得比这轮对话更久。
        let mut command_builder = std::process::Command::new(&self.binary);
        command_builder
            .current_dir(working_dir)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        xai_tty_utils::detach_std_command(&mut command_builder);
        let process_scope = xai_tty_utils::ProcessScope::new();
        let mut child = loop {
            #[allow(clippy::disallowed_methods)] // 紧接着 enroll_std，见上
            match command_builder.spawn() {
                Ok(child) => break child,
                Err(error) => {
                    #[cfg(unix)]
                    if error.raw_os_error() == Some(26) && attempts < 5 {
                        attempts += 1;
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        continue;
                    }
                    return Err(format!("无法启动 headless Agent: {error}"));
                }
            }
        };
        let _process_group = match process_scope.enroll_std(&child) {
            Ok(group) => group,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("无法跟踪 headless Agent 进程: {error}"));
            }
        };
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "headless Agent 标准输出不可用".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "headless Agent 标准错误不可用".to_string())?;
        // 两条管道各自归一个解析线程，解析结果走同一条通道由本线程投递：
        // stdout 讲 ACP 会话更新（ADR-008 词表），stderr 继续喂 api_retry 与
        // 诊断。deliver 是 &mut，不能进线程；跨线程的只能是解析结果。
        enum Parsed {
            Text(String),
            Thought(String),
            Tool(TurnToolEnv),
            Subagent(SubagentLifecycleEvent),
            Progress(RunProgress),
            Usage(ProviderUsage),
            End { stop_reason: String },
            Error(String),
        }
        /// 拥有所有权的工具卡载荷（线程边界）。
        struct TurnToolEnv {
            started: bool,
            tool_call_id: String,
            tool_name: String,
            completed: Option<bool>,
            raw_input: String,
            output_text: String,
        }
        let (line_tx, line_rx) = std::sync::mpsc::channel::<Parsed>();
        let stderr_tx = line_tx.clone();
        // tool_call 的名字表：`tool_call_update` 行在 CLI 的 streaming-json 词表里
        // 不带名字（reducer 的 ToolCallUpdate 只有 id/status/content/rawOutput），
        // 更新事件的名字从开始的那个 tool_call 记来。
        let tool_names: std::sync::Arc<
            std::sync::Mutex<std::collections::HashMap<String, String>>,
        > = std::sync::Arc::default();
        let stdout_reader = std::thread::spawn(move || {
            let tool_names = std::sync::Arc::clone(&tool_names);
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else {
                    break;
                };
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
                    continue;
                };
                match value.get("type").and_then(serde_json::Value::as_str) {
                    Some("text") => {
                        if let Some(data) = value.get("data").and_then(serde_json::Value::as_str) {
                            let _ = line_tx.send(Parsed::Text(data.to_string()));
                        }
                    }
                    Some("thought") => {
                        if let Some(data) = value.get("data").and_then(serde_json::Value::as_str) {
                            let _ = line_tx.send(Parsed::Thought(data.to_string()));
                        }
                    }
                    Some("subagent_spawned") | Some("subagent_progress") | Some("subagent_finished") => {
                        let kind = value.get("type").and_then(serde_json::Value::as_str).unwrap_or_default();
                        let event = match kind {
                            "subagent_spawned" => "spawned",
                            "subagent_progress" => "progress",
                            _ => "finished",
                        };
                        let _ = line_tx.send(Parsed::Subagent(SubagentLifecycleEvent {
                            event: event.to_owned(),
                            subagent_id: value.get("subagentId").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned(),
                            child_session_id: value.get("childSessionId").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned(),
                            subagent_type: value.get("subagentType").and_then(serde_json::Value::as_str).unwrap_or("subagent").to_owned(),
                            description: value.get("description").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned(),
                            status: value.get("status").and_then(serde_json::Value::as_str).map(str::to_owned),
                            model: value.get("model").and_then(serde_json::Value::as_str).map(str::to_owned),
                            run_in_background: value.get("runInBackground").and_then(serde_json::Value::as_bool),
                            started_at_ms: value.get("startedAtMs").and_then(serde_json::Value::as_u64),
                            parent_session_id: value.get("parentSessionId").and_then(serde_json::Value::as_str).map(str::to_owned),
                            workflow_run_id: value.get("workflowRunId").and_then(serde_json::Value::as_str).map(str::to_owned),
                            agent_address: value.get("agentAddress").and_then(serde_json::Value::as_str).map(str::to_owned),
                            attempt_id: value.get("attemptId").and_then(serde_json::Value::as_str).map(str::to_owned),
                            event_seq: value.get("eventSeq").and_then(serde_json::Value::as_u64),
                            duration_ms: value.get("durationMs").and_then(serde_json::Value::as_u64),
                            parent_tool_call_id: value.get("parentToolCallId").and_then(serde_json::Value::as_str).map(str::to_owned),
                        }));
                    }
                    Some("tool_call") => {
                        if let Some(tool_call_id) =
                            value.get("toolCallId").and_then(serde_json::Value::as_str)
                        {
                            let tool_name = value
                                .get("toolName")
                                .and_then(serde_json::Value::as_str)
                                .or_else(|| value.get("title").and_then(serde_json::Value::as_str))
                                .unwrap_or("tool")
                                .to_string();
                            tool_names
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner())
                                .insert(tool_call_id.to_string(), tool_name.clone());
                            let _ = line_tx.send(Parsed::Tool(TurnToolEnv {
                                started: true,
                                tool_call_id: tool_call_id.to_string(),
                                tool_name: tool_name.to_string(),
                                completed: None,
                                raw_input: value
                                    .get("rawInput")
                                    .map(|raw| raw.to_string())
                                    .unwrap_or_default(),
                                output_text: String::new(),
                            }));
                        }
                    }
                    Some("tool_call_update") => {
                        if let Some(tool_call_id) =
                            value.get("toolCallId").and_then(serde_json::Value::as_str)
                        {
                            let completed =
                                match value.get("status").and_then(serde_json::Value::as_str) {
                                    Some("completed") => Some(true),
                                    Some("failed") => Some(false),
                                    _ => None,
                                };
                            let mut output_text = String::new();
                            if let Some(parts) =
                                value.get("content").and_then(serde_json::Value::as_array)
                            {
                                for part in parts {
                                    if let Some(text) = part
                                        .get("content")
                                        .and_then(|content| content.get("text"))
                                        .and_then(serde_json::Value::as_str)
                                    {
                                        if !output_text.is_empty() {
                                            output_text.push('\n');
                                        }
                                        output_text.push_str(text);
                                    }
                                }
                            }
                            let tool_name = tool_names
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner())
                                .get(tool_call_id)
                                .cloned()
                                .unwrap_or_else(|| "tool".to_string());
                            let _ = line_tx.send(Parsed::Tool(TurnToolEnv {
                                started: false,
                                tool_call_id: tool_call_id.to_string(),
                                tool_name: tool_name.to_string(),
                                completed,
                                raw_input: String::new(),
                                output_text,
                            }));
                        }
                    }
                    Some("usage") => {
                        if let Some(usage) = usage_from_wire(value.get("usage")) {
                            let _ = line_tx.send(Parsed::Usage(usage));
                        }
                    }
                    Some("end") => {
                        let stop_reason = value
                            .get("stopReason")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("end_turn")
                            .to_string();
                        if let Some(usage) = usage_from_wire(value.get("usage")) {
                            let _ = line_tx.send(Parsed::Usage(usage));
                        }
                        let _ = line_tx.send(Parsed::End { stop_reason });
                        break;
                    }
                    Some("error") => {
                        let message = value
                            .get("message")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("headless Agent 报告错误")
                            .to_string();
                        let _ = line_tx.send(Parsed::Error(message));
                    }
                    // available_commands/lifecycle 等其余行：适配器这一版不消费。
                    _ => {}
                }
            }
        });
        let stderr_reader = std::thread::spawn(move || {
            let line_tx = stderr_tx;
            let mut diagnostics = String::new();
            for line in std::io::BufReader::new(stderr).lines() {
                let Ok(line) = line else {
                    break;
                };
                let trimmed = line.trim();
                // 只透传「正在重试」：exhausted/failed 是轮次终局，跟着的退出
                // 状态与错误文本会以既有方式报告，不必变成一条提示。
                if trimmed.starts_with(r#"{"type":"api_retry""#)
                    && let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed)
                    && let Some(state) = value.get("state")
                    && state.get("type").and_then(serde_json::Value::as_str) == Some("retrying")
                {
                    let _ = line_tx.send(Parsed::Progress(RunProgress::ApiRetry {
                        attempt: state
                            .get("attempt")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0) as u32,
                        max_retries: state
                            .get("max_retries")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0) as u32,
                        reason: state
                            .get("reason")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    }));
                }
                if diagnostics.len() < HEADLESS_MAX_OUTPUT_BYTES {
                    diagnostics.push_str(&line);
                    diagnostics.push('\n');
                }
            }
            diagnostics
        });

        let mut report = TurnReport::default();
        let mut last_error: Option<String> = None;
        let mut cancelled = false;
        while let Ok(parsed) = line_rx.recv() {
            let deliver_result = match parsed {
                Parsed::Text(chunk) => deliver(TurnOutput::Text(&chunk)),
                Parsed::Thought(chunk) => deliver(TurnOutput::Thought(&chunk)),
                Parsed::Tool(tool) => deliver(TurnOutput::Tool(TurnTool {
                    started: tool.started,
                    tool_call_id: &tool.tool_call_id,
                    tool_name: &tool.tool_name,
                    completed: tool.completed,
                    raw_input: &tool.raw_input,
                    output_text: &tool.output_text,
                })),
                Parsed::Subagent(event) => deliver(TurnOutput::Subagent(event)),
                Parsed::Progress(progress) => deliver(TurnOutput::Progress(progress)),
                Parsed::Usage(usage) => {
                    report.usage = Some(usage);
                    true
                }
                Parsed::End { stop_reason } => {
                    let _ = stop_reason; // 终态由进程退出码兜底，这里只记账
                    break;
                }
                Parsed::Error(message) => {
                    last_error = Some(message);
                    true
                }
            };
            if !deliver_result {
                cancelled = true;
                break;
            }
        }
        drop(line_rx);
        if cancelled {
            // 宿主已经不听了（取消）：杀掉子进程，已交付的内容留在会话里。
            let _ = child.kill();
            let _ = child.wait();
            process_scope.kill_all();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Ok(report);
        }
        let status = child
            .wait()
            .map_err(|error| format!("等待 headless Agent 失败: {error}"))?;
        process_scope.kill_all();
        let diagnostics = stderr_reader.join().unwrap_or_default();
        if !status.success() {
            let detail = if diagnostics.trim().is_empty() {
                "headless Agent 退出失败".to_string()
            } else {
                diagnostics.trim().to_string()
            };
            return Err(detail);
        }
        if let Some(message) = last_error {
            return Err(message);
        }
        Ok(report)
    }
}

/// streaming-json 的 usage 对象 → 引擎的 ProviderUsage；对象不存在就是 `None`。
///
/// 字段名两套词表并存（2026-10-12 真实 CLI 实测）：usage 行是 snake_case
/// （`input_tokens`/`cache_read_input_tokens`，与 `ResponseUsage` 的 serde 名
/// 一致），而 end 行的 `modelUsage` 子对象是 camelCase（`inputTokens`）。逐
/// 字段两读，先 camelCase 后 snake_case，谁有值用谁。
fn usage_field(usage: &serde_json::Value, camel: &str, snake: &str) -> u64 {
    usage
        .get(camel)
        .and_then(serde_json::Value::as_u64)
        .or_else(|| usage.get(snake).and_then(serde_json::Value::as_u64))
        .unwrap_or(0)
}

fn usage_from_wire(usage: Option<&serde_json::Value>) -> Option<ProviderUsage> {
    let usage = usage?;
    Some(ProviderUsage {
        input_tokens: usage_field(usage, "inputTokens", "input_tokens"),
        output_tokens: usage_field(usage, "outputTokens", "output_tokens"),
        reasoning_tokens: usage_field(usage, "reasoningTokens", "reasoning_tokens"),
        cached_tokens: usage_field(usage, "cachedReadTokens", "cache_read_input_tokens"),
        cache_creation_tokens: usage_field(
            usage,
            "cacheCreationTokens",
            "cache_creation_input_tokens",
        ),
        // Headless `ResponseUsage` reports prompt usage as disjoint uncached,
        // cache-read and cache-creation buckets. Normalize the hit-rate
        // denominator from all three unless the wire supplied an explicit total.
        cache_total_tokens: usage_field(usage, "cacheTotalTokens", "cache_total_tokens")
            .max(
                usage_field(usage, "inputTokens", "input_tokens")
                    .saturating_add(usage_field(
                        usage,
                        "cachedReadTokens",
                        "cache_read_input_tokens",
                    ))
                    .saturating_add(usage_field(
                        usage,
                        "cacheCreationTokens",
                        "cache_creation_input_tokens",
                    )),
            )
            .max(usage_field(usage, "prompt_tokens", "input_tokens")),
    })
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
    ApproveRemembered {
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
    /// Asks the host's own Provider which models it serves, so the model picker
    /// shows real choices. Deliberately carries no base URL: the client cannot
    /// point this request at an endpoint the operator did not configure, which
    /// would otherwise turn a "list models" click into a way to make the host
    /// send its credential somewhere else.
    ListProviderModels {
        client_msg_id: String,
    },
    ListModes {
        client_msg_id: String,
    },
    SetMode {
        client_msg_id: String,
        mode: String,
    },
    SetThoughtLevel {
        client_msg_id: String,
        thought_level: String,
    },
    /// The UI picked a different model; later runs ask the provider for it.
    SetModel {
        client_msg_id: String,
        model: String,
    },
    ListWorkflows {
        client_msg_id: String,
        session_id: Uuid,
    },
    ListSubagents {
        client_msg_id: String,
        session_id: Uuid,
    },
    GetSessionTarget {
        client_msg_id: String,
        session_id: Uuid,
    },
    SetSessionTarget {
        client_msg_id: String,
        session_id: Uuid,
        objective: String,
        token_budget: Option<u64>,
    },
    /// 只改目标状态（pauseGoal/resumeGoal 合同）。目标本身不动。
    SetSessionTargetStatus {
        client_msg_id: String,
        session_id: Uuid,
        status: String,
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
    GetSessionDiffSummary {
        client_msg_id: String,
        session_id: Uuid,
    },
    ListSessions {
        client_msg_id: String,
        #[serde(default)]
        workspace_id: Option<Uuid>,
    },
    RenameSession {
        client_msg_id: String,
        session_id: Uuid,
        title: String,
    },
    PinSession {
        client_msg_id: String,
        session_id: Uuid,
        pinned: bool,
    },
    /// 宿主把某个会话展示给了用户：清掉侧栏的未读时间戳。幂等。
    MarkSessionSeen {
        client_msg_id: String,
        session_id: Uuid,
    },
    /// 运行中到达的输入：进队列（`requestedDelivery:"queue"` 合同）。
    QueueInput {
        client_msg_id: String,
        session_id: Uuid,
        text: String,
        source_command_id: String,
        /// 信封 `requestedDelivery` 的原值（缺席 = auto）；只作事实存档进
        /// queue item intent，不改变准入（忙碌一律进队列）。
        requested_delivery: String,
        /// 信封 `clientId`；queue item intent 必带非空 client id。
        client_id: String,
    },
    EditQueueItem {
        client_msg_id: String,
        session_id: Uuid,
        queue_item_id: Uuid,
        new_text: String,
    },
    DeleteQueueItem {
        client_msg_id: String,
        session_id: Uuid,
        queue_item_id: Uuid,
    },
    ReorderQueueItem {
        client_msg_id: String,
        session_id: Uuid,
        queue_item_id: Uuid,
        before_queue_item_id: Option<Uuid>,
    },
    SendQueuedNow {
        client_msg_id: String,
        session_id: Uuid,
        queue_item_id: Uuid,
    },
    SetQueueAutoDrain {
        client_msg_id: String,
        session_id: Uuid,
        auto_drain: bool,
    },
    ArchiveSession {
        client_msg_id: String,
        session_id: Uuid,
        archived: bool,
    },
    DeleteSession {
        client_msg_id: String,
        session_id: Uuid,
    },
    ForkSession {
        client_msg_id: String,
        session_id: Uuid,
    },
    /// 只读的 App Usage 查询：按区间与时区汇总本引擎记录的真实轮次用量。
    UsageStats {
        client_msg_id: String,
        /// "all" | "7d" | "30d"；其余按 "all" 处理。
        range: String,
        /// 客户端的 IANA 时区；解析不了就回退服务进程本地时区。
        #[serde(default)]
        time_zone: Option<String>,
    },
}

impl ClientMessage {
    /// The session a request is about, when it names one.
    ///
    /// This is how a connection declares that it follows a session: the engine
    /// routes that session's events back to it (see [`Engine::follow`]). Every
    /// request that operates on a session names it, so a client does not have to
    /// do anything extra to keep receiving what it already asked for -- opening a
    /// snapshot or cancelling a run is the following gesture. Requests with no
    /// session (`ListWorkspaces`, `GetSettings`, the attachment chunk/validate
    /// family) name none and register nothing; `ImportTuiSession`'s `session_id`
    /// is a directory name from the terminal's on-disk sessions, not this
    /// process's session id, so it is deliberately not counted here.
    #[must_use]
    pub fn session_id(&self) -> Option<Uuid> {
        match self {
            Self::Resume { session_id, .. }
            | Self::Submit { session_id, .. }
            | Self::Cancel { session_id, .. }
            | Self::Snapshot { session_id, .. }
            | Self::ProposeFileWrite { session_id, .. }
            | Self::ProposeTerminal { session_id, .. }
            | Self::ProposeGitMutation { session_id, .. }
            | Self::SuggestCommitMessage { session_id, .. }
            | Self::BeginAttachment { session_id, .. }
            | Self::AcceptDiff { session_id, .. }
            | Self::RollbackDiff { session_id, .. }
            | Self::PreviewDiff { session_id, .. }
            | Self::ListWorkflows { session_id, .. }
            | Self::ListSubagents { session_id, .. }
            | Self::GetSessionTarget { session_id, .. }
            | Self::SetSessionTarget { session_id, .. }
            | Self::SetSessionTargetStatus { session_id, .. }
            | Self::GetSessionDiffSummary { session_id, .. }
            | Self::RenameSession { session_id, .. }
            | Self::PinSession { session_id, .. }
            | Self::MarkSessionSeen { session_id, .. }
            | Self::QueueInput { session_id, .. }
            | Self::EditQueueItem { session_id, .. }
            | Self::DeleteQueueItem { session_id, .. }
            | Self::ReorderQueueItem { session_id, .. }
            | Self::SendQueuedNow { session_id, .. }
            | Self::SetQueueAutoDrain { session_id, .. }
            | Self::ArchiveSession { session_id, .. }
            | Self::DeleteSession { session_id, .. }
            | Self::ForkSession { session_id, .. } => Some(*session_id),
            Self::CreateSession { .. }
            | Self::ListSessions { .. }
            | Self::CreateWorkspace { .. }
            | Self::ListWorkspaces { .. }
            | Self::ArchiveWorkspace { .. }
            | Self::SwitchWorkspace { .. }
            | Self::Approve { .. }
            | Self::ApproveRemembered { .. }
            | Self::Reject { .. }
            | Self::RespondQuestion { .. }
            | Self::ListFiles { .. }
            | Self::ReadFile { .. }
            | Self::SearchFiles { .. }
            | Self::GetSettings { .. }
            | Self::UpdateSettings { .. }
            | Self::GetGitStatus { .. }
            | Self::ValidateAttachment { .. }
            | Self::AttachmentChunk { .. }
            | Self::CancelAttachment { .. }
            | Self::FinalizeAttachment { .. }
            | Self::ImportTuiSession { .. }
            | Self::ValidateProvider { .. }
            | Self::ListProviderModels { .. }
            | Self::ListModes { .. }
            | Self::SetMode { .. }
            | Self::SetThoughtLevel { .. }
            | Self::SetModel { .. }
            | Self::ScanMarketplace { .. }
            | Self::UsageStats { .. } => None,
        }
    }
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
        /// 本会话当前正在进行的 API 重试（只在轮次进行中可能非空），
        /// 让刷新页面/重开面板的客户端也能把「重试中」画出来。
        #[serde(default)]
        api_retry: Option<ApiRetryStatus>,
    },
    Ack {
        client_msg_id: String,
    },
    TextDelta {
        session_id: Uuid,
        text: String,
        sequence: u64,
    },
    ReasoningDelta {
        session_id: Uuid,
        text: String,
        sequence: u64,
    },
    /// 一次 API 重试状态变化。`retry` 携带「第几次重试、为什么」；轮次终局时
    /// `finish_run` 会先清掉会话上的状态，终局事件本身（Completed/Cancelled/
    /// Error）就是客户端清掉重试提示的信号。
    /// 一轮回答开始。`prompt` 是这轮的用户输入文本：运行中入队的输入在自动
    /// 排水时由引擎自己开新一轮，宿主没有发过 Submit——这一事件就是宿主
    /// 画出新用户行与新回合的依据。
    TurnStarted {
        session_id: Uuid,
        prompt: String,
        sequence: u64,
    },
    /// 队列变了（入队/编辑/删除/重排/暂停开关）。带全量队列视图，桥原样
    /// 铺进任务的快照补丁。
    QueueUpdated {
        session_id: Uuid,
        items: Vec<QueuedInput>,
        auto_drain: bool,
        paused: bool,
        sequence: u64,
    },
    RetryUpdated {
        session_id: Uuid,
        retry: Option<ApiRetryStatus>,
        sequence: u64,
    },
    PlanUpdated {
        session_id: Uuid,
        steps: Vec<PlanStep>,
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
        #[serde(default)]
        raw_input: Option<String>,
        /// CLI 给的真实工具调用 id（streaming-json 的 `toolCallId`）；引擎内部
        /// 产生的工具事件没有这个东西，桥会按会话序号合成。带真实 id 时恢复
        /// 视图（updates.jsonl）与 live 行同 id，resync 不重影。
        #[serde(default)]
        tool_call_id: Option<String>,
        sequence: u64,
    },
    ToolProgress {
        session_id: Uuid,
        tool: String,
        progress: String,
        #[serde(default)]
        tool_call_id: Option<String>,
        sequence: u64,
    },
    ToolResult {
        session_id: Uuid,
        tool: String,
        result: String,
        /// Provider/CLI 明确报告的工具终态。内部已批准 adapter 的成功继续用 None。
        #[serde(default)]
        succeeded: Option<bool>,
        #[serde(default)]
        tool_call_id: Option<String>,
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
        /// Provider cache-read tokens, when reported.
        #[serde(default)]
        cached_tokens: u64,
        /// Normalized prompt-token denominator for cache hit rate.
        #[serde(default)]
        cache_total_tokens: u64,
        /// Provider cache-write tokens, when reported.
        #[serde(default)]
        cache_creation_tokens: u64,
        /// Whole model-call duration in milliseconds.
        #[serde(default)]
        duration_ms: Option<u64>,
        /// Time to first token in milliseconds.
        #[serde(default)]
        ttft_ms: Option<u64>,
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
    /// The host Provider's model catalog, read from the endpoint the operator
    /// configured. `configured_model` is the slug a run would use if the user
    /// changed nothing, so the picker can mark the current choice rather than
    /// guess; `detail` carries why `reachable` is false, credential-free.
    ProviderModels {
        models: Vec<String>,
        configured_model: Option<String>,
        reachable: bool,
        detail: Option<String>,
    },
    Modes {
        modes: Vec<ModeInfo>,
        current_mode: String,
        thought_levels: Vec<ThoughtLevelInfo>,
        current_thought_level: String,
    },
    MarketplaceScan {
        entries: Vec<serde_json::Value>,
        catalog_loaded: bool,
    },
    WorkflowRuns {
        session_id: Uuid,
        workflows: Vec<WorkflowRunInfo>,
    },
    Subagents {
        session_id: Uuid,
        subagents: Vec<SubagentInfo>,
    },
    SubagentsUpdated {
        session_id: Uuid,
        revision: u64,
        child_session_ids: Vec<String>,
        running: Vec<RunningSubagentInfo>,
        ended_total: usize,
    },
    SessionTarget {
        session_id: Uuid,
        target: Option<SessionTargetInfo>,
    },
    SessionDiffSummary {
        session_id: Uuid,
        summary: SessionDiffSummary,
    },
    Sessions {
        workspace_id: Option<Uuid>,
        sessions: Vec<SessionInfo>,
    },
    SessionUpdated {
        session: SessionInfo,
    },
    SessionDeleted {
        session_id: Uuid,
    },
    TuiSessionImport {
        session_id: String,
        cwd: String,
        title: Option<String>,
        message_count: usize,
        source_unchanged: bool,
    },
    /// A commit message the client may put in its commit form. `truncated`
    /// says the staged diff the Provider saw was cut short, so a short answer
    /// may be short because the model never saw the rest of it.
    CommitMessageSuggestion {
        session_id: Uuid,
        message: String,
        truncated: bool,
    },
    /// App Usage 快照（`usage` 模块聚合好的 JSON 文本）。只答给提问的连接。
    UsageStatsSnapshot {
        client_msg_id: String,
        snapshot: String,
    },
    Error {
        code: String,
        message: String,
    },
}

impl ServerMessage {
    /// Whether this message reports something that *happened* in a session, as
    /// opposed to answering one client's request.
    ///
    /// The distinction decides who receives the message on a host with several
    /// connections: an event goes to the connections following its session, so a
    /// second tab that opened the same session follows a turn another tab or
    /// another machine started, while an answer stays with the connection that
    /// asked. Two messages look like events and are not: `session_created` and
    /// `session_snapshot` are addressed, because a client
    /// that did not ask would otherwise switch to a session it never chose or
    /// replace its transcript with somebody else's.
    #[must_use]
    pub fn is_session_event(&self) -> bool {
        match self {
            Self::TextDelta { .. }
            | Self::Completed { .. }
            | Self::Cancelled { .. }
            | Self::ToolApprovalRequested { .. }
            | Self::ApprovalResolved { .. }
            | Self::Audit { .. }
            | Self::DiffResolved { .. }
            | Self::QuestionRequested { .. }
            | Self::QuestionResolved { .. }
            | Self::ToolStarted { .. }
            | Self::ToolProgress { .. }
            | Self::ToolResult { .. }
            | Self::FileChanged { .. }
            | Self::FileWritten { .. }
            | Self::Usage { .. }
            | Self::Workspaces { .. }
            | Self::WorkspaceArchived { .. }
            | Self::Modes { .. }
            | Self::ReasoningDelta { .. }
            | Self::RetryUpdated { .. }
            | Self::QueueUpdated { .. }
            | Self::TurnStarted { .. }
            | Self::PlanUpdated { .. }
            | Self::SubagentsUpdated { .. }
            | Self::SessionTarget { .. }
            | Self::Sessions { .. }
            | Self::SessionUpdated { .. }
            | Self::SessionDeleted { .. } => true,
            Self::SessionCreated { .. }
            | Self::SessionSnapshot { .. }
            | Self::WorkspaceSwitched { .. }
            | Self::Handshake { .. }
            | Self::Ack { .. }
            | Self::Error { .. }
            | Self::UsageStatsSnapshot { .. }
            | Self::FilesListed { .. }
            | Self::FileContents { .. }
            | Self::SearchResults { .. }
            | Self::Settings { .. }
            | Self::SettingsUpdated { .. }
            | Self::GitStatus { .. }
            | Self::TerminalResult { .. }
            | Self::GitMutationResult { .. }
            | Self::CommitMessageSuggestion { .. }
            | Self::AttachmentValidated { .. }
            | Self::AttachmentStarted { .. }
            | Self::AttachmentProgress { .. }
            | Self::AttachmentCompleted { .. }
            | Self::AttachmentCancelled { .. }
            | Self::ProviderValidation { .. }
            | Self::ProviderModels { .. }
            | Self::WorkflowRuns { .. }
            | Self::Subagents { .. }
            | Self::MarketplaceScan { .. }
            | Self::SessionDiffSummary { .. }
            | Self::TuiSessionImport { .. }
            | Self::DiffPreview { .. } => false,
        }
    }

    /// The session a message belongs to, when it names one.
    ///
    /// [`Self::is_session_event`] decides *whether* a message is broadcast; this
    /// decides *whose* broadcast it is. The two differ because some events are
    /// not about one session: the workspace list changes for every connection at
    /// once, and a resolved diff names a proposal rather than a session. Those
    /// report `None` and reach every connection, which is what makes them useful;
    /// everything that does name a session reaches only the connections that
    /// named it back (see [`Engine::publish`]).
    #[must_use]
    pub fn session_id(&self) -> Option<Uuid> {
        match self {
            Self::SessionCreated { session_id, .. }
            | Self::SessionSnapshot { session_id, .. }
            | Self::TextDelta { session_id, .. }
            | Self::Completed { session_id, .. }
            | Self::Cancelled { session_id, .. }
            | Self::ToolApprovalRequested { session_id, .. }
            | Self::ApprovalResolved { session_id, .. }
            | Self::Audit { session_id, .. }
            | Self::DiffPreview { session_id, .. }
            | Self::QuestionRequested { session_id, .. }
            | Self::QuestionResolved { session_id, .. }
            | Self::ToolStarted { session_id, .. }
            | Self::ToolProgress { session_id, .. }
            | Self::ToolResult { session_id, .. }
            | Self::FileChanged { session_id, .. }
            | Self::Usage { session_id, .. }
            | Self::FileWritten { session_id, .. }
            | Self::TerminalResult { session_id, .. }
            | Self::GitMutationResult { session_id, .. }
            | Self::AttachmentStarted { session_id, .. }
            | Self::AttachmentCompleted { session_id, .. }
            | Self::ReasoningDelta { session_id, .. }
            | Self::RetryUpdated { session_id, .. }
            | Self::QueueUpdated { session_id, .. }
            | Self::TurnStarted { session_id, .. }
            | Self::PlanUpdated { session_id, .. }
            | Self::WorkflowRuns { session_id, .. }
            | Self::Subagents { session_id, .. }
            | Self::SubagentsUpdated { session_id, .. }
            | Self::SessionTarget { session_id, .. }
            | Self::SessionDiffSummary { session_id, .. }
            | Self::CommitMessageSuggestion { session_id, .. } => Some(*session_id),
            Self::Handshake { .. }
            | Self::Workspaces { .. }
            | Self::WorkspaceArchived { .. }
            | Self::WorkspaceSwitched { .. }
            | Self::Sessions { .. }
            | Self::SessionUpdated { .. }
            | Self::SessionDeleted { .. }
            | Self::Ack { .. }
            | Self::UsageStatsSnapshot { .. }
            | Self::DiffResolved { .. }
            | Self::FilesListed { .. }
            | Self::FileContents { .. }
            | Self::SearchResults { .. }
            | Self::Settings { .. }
            | Self::SettingsUpdated { .. }
            | Self::GitStatus { .. }
            | Self::AttachmentValidated { .. }
            | Self::AttachmentProgress { .. }
            | Self::AttachmentCancelled { .. }
            | Self::ProviderValidation { .. }
            | Self::ProviderModels { .. }
            | Self::Modes { .. }
            | Self::MarketplaceScan { .. }
            | Self::TuiSessionImport { .. }
            | Self::Error { .. } => None,
        }
    }
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
    /// The directory this workspace *is*, absolute.
    ///
    /// A workspace used to be only a name the client typed, which is why every
    /// front end that read it had to guess a directory: the host bridge
    /// reported `std::env::current_dir()` for every task, and a conversation the
    /// terminal had in `~/project-a` appeared in the sidebar as though it had
    /// happened in whatever directory the server happened to be started from.
    /// The CLI already keys its whole store by directory; a workspace that
    /// cannot name its own directory cannot show those conversations, run a
    /// prompt in them, or tell two of them apart.
    ///
    /// `None` is the legacy state: a workspace created before this existed, or
    /// one a client created with only a name. It behaves as it always did.
    #[serde(default)]
    pub path: Option<String>,
}

/// Which conversation in the Chaos CLI's own store a session was brought over
/// from, and where its files live.
///
/// This is what makes a brought-over conversation *continuable* rather than a
/// screenshot: the CLI is asked to resume exactly this session, in exactly this
/// directory, so the answer arrives with the terminal's own context and is
/// written back into the terminal's own transcript.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ImportedSession {
    /// The directory the conversation belongs to, as the CLI's summary states it.
    pub cwd: String,
    /// The CLI's session id (UUIDv7 string), the argument to `--resume`.
    pub native_id: String,
    /// The session's directory under `$CHAOS_HOME/sessions`.
    pub directory: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct SessionInfo {
    pub session_id: Uuid,
    #[serde(default)]
    pub workspace_id: Option<Uuid>,
    pub title: String,
    pub pinned: bool,
    pub archived: bool,
    /// 侧栏未读时间戳；`None` 是「没有未读」。
    #[serde(default)]
    pub unread_at: Option<u64>,
    pub message_count: usize,
    pub updated_at_sequence: u64,
    /// The directory of the workspace this conversation belongs to. A list row
    /// that cannot name its directory cannot be grouped by one.
    #[serde(default)]
    pub workspace_path: Option<String>,
    /// Wall-clock facts. `updated_at_sequence` orders sessions inside one
    /// process's lifetime only, so every front end that sorted by "recent" had
    /// to invent a timestamp; imported conversations inherited the clock of the
    /// moment they were listed, which made last month read as just now.
    /// `0` means unknown, never "the epoch".
    #[serde(default)]
    pub created_at_ms: u64,
    #[serde(default)]
    pub updated_at_ms: u64,
    /// Set when this session is a view of a conversation the CLI owns.
    #[serde(default)]
    pub imported: Option<ImportedSession>,
    /// The last thing said, shortened.
    ///
    /// A listing wants one line per conversation. Reading every conversation to
    /// produce that line is the difference between listing a dozen and listing
    /// the few hundred a working machine holds, so the line is recorded when a
    /// turn ends rather than reconstructed each time the list is drawn.
    #[serde(default)]
    pub preview: Option<String>,
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
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    pinned: bool,
    #[serde(default)]
    archived: bool,
    messages: Vec<TimelineMessage>,
    audit: Vec<AuditEntry>,
    sequence: u64,
    pending_approval: Option<PendingApproval>,
    pending_question: Option<Uuid>,
    #[serde(default)]
    pending_question_prompt: Option<String>,
    pending_file_writes: HashMap<Uuid, (Uuid, String, String)>,
    pending_attachment_moves: HashMap<Uuid, (Uuid, Uuid, String)>,
    #[serde(default)]
    pending_tool_inputs: HashMap<String, String>,
    #[serde(default)]
    target: Option<SessionTargetInfo>,
    #[serde(default)]
    remembered_permissions: HashSet<String>,
    /// 每轮的真实 token 用量（`usage` 模块），只增不减，超上限丢最旧。
    #[serde(default)]
    turns: Vec<usage::TurnUsage>,
    /// 每个工具的累计执行计数（无时间戳，"all" 区间用它）。
    #[serde(default)]
    tool_stats: BTreeMap<String, usage::ToolUsageStat>,
    /// 最近工具执行事件（有时间戳，供热力图按天计数；超上限丢最旧）。
    #[serde(default)]
    tool_events: Vec<usage::ToolEvent>,
    /// Wall clock of the first turn and the last activity, epoch milliseconds.
    /// `sequence` orders one process's lifetime; these order conversations
    /// across restarts and across the terminal's own store, which is what a
    /// sidebar sorted by "recent" actually needs. `0` means unknown.
    #[serde(default)]
    created_at_ms: u64,
    #[serde(default)]
    updated_at_ms: u64,
    /// Set when this session is the GUI's view of a conversation the CLI owns.
    #[serde(default)]
    imported: Option<ImportedSession>,
    /// How many messages the store said this conversation has, before its
    /// transcript was read. An imported conversation is listed with its real
    /// size long before anything has opened it.
    #[serde(default)]
    stored_message_count: usize,
    /// The title as the CLI's own summary gave it, kept so a rename made here
    /// is not overwritten by the next rescan while a title the CLI changed on
    /// its own still follows the CLI.
    #[serde(default)]
    cli_title: Option<String>,
    /// The last thing said, kept so a listing can show it without reading the
    /// conversation back. See [`SessionInfo::preview`].
    #[serde(default)]
    last_preview: Option<String>,
    /// 本会话正在进行的 API 重试（「第几次、为什么」）。轮次一终局就清掉；
    /// 落盘随状态走，让重启后的快照也能如实说「正在重试」或什么都不说。
    #[serde(default)]
    api_retry: Option<ApiRetryStatus>,
    /// 侧栏的未读时间戳：这轮对话产生了用户可能没看过的内容（首个文本增量），
    /// 宿主打开会话时用 MarkSessionSeen 清掉。`None` 就是「没有未读」。
    #[serde(default)]
    unread_at: Option<u64>,
    /// 运行中到达的输入排队（宿主队列合同，2026-10-12 取证）。FIFO；自动
    /// 排水在轮次完成后把队头交给下一轮。
    #[serde(default)]
    queued_inputs: Vec<QueuedInput>,
    /// 轮次完成后是否自动排空队列。默认开；「停止」会把当前轮打断并置
    /// `queue_paused`——用户按停止是要暂停，不是要连发下一条。
    #[serde(default = "queue_auto_drain_default")]
    queue_auto_drain: bool,
    #[serde(default)]
    queue_paused: bool,
}

fn queue_auto_drain_default() -> bool {
    true
}

impl SessionState {
    /// 队列的只读视图：排队项、自动排水开关、是否暂停。
    fn queue_snapshot(&self) -> (Vec<QueuedInput>, bool, bool) {
        (
            self.queued_inputs.clone(),
            self.queue_auto_drain,
            self.queue_paused,
        )
    }

    /// A session as this process starts one: bound to a workspace and stamped
    /// with the clock it was created on, so a list sorted by "recent" has a real
    /// time to sort by from the moment the conversation exists.
    fn fresh(workspace_id: Option<Uuid>) -> Self {
        let mut session = Self {
            workspace_id,
            // 队列的自动排水默认开（serde default 只在反序列化时生效，
            // 内存构造要走这里）。
            queue_auto_drain: true,
            ..Self::default()
        };
        session.touch();
        session
    }

    /// Stamp the wall clock on a session that just changed.
    fn touch(&mut self) {
        let now = usage::now_ms();
        if self.created_at_ms == 0 {
            self.created_at_ms = now;
        }
        self.updated_at_ms = now;
    }

    fn info(&self, session_id: Uuid, workspace_path: Option<&str>) -> SessionInfo {
        let title = self.title.clone().unwrap_or_else(|| {
            self.messages
                .iter()
                .find(|m| m.role == "user")
                .map(|m| {
                    let first_line = m.text.lines().next().unwrap_or(&m.text);
                    if first_line.chars().count() > 30 {
                        format!("{}...", first_line.chars().take(28).collect::<String>())
                    } else {
                        first_line.to_string()
                    }
                })
                .unwrap_or_else(|| "新会话".to_string())
        });
        SessionInfo {
            session_id,
            workspace_id: self.workspace_id,
            title,
            pinned: self.pinned,
            archived: self.archived,
            unread_at: self.unread_at,
            // Before a transcript is read, the count the CLI's own summary gave
            // is the honest number; after it is read, the transcript is.
            message_count: if self.messages.is_empty() {
                self.stored_message_count
            } else {
                self.messages.len()
            },
            updated_at_sequence: self.sequence,
            workspace_path: workspace_path.map(str::to_owned),
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
            imported: self.imported.clone(),
            preview: self.last_preview.clone(),
        }
    }

    /// Records the one line a listing shows for this conversation.
    fn record_preview(&mut self) {
        let last = self
            .messages
            .iter()
            .rev()
            .find(|message| message.role == "assistant" && !message.text.trim().is_empty())
            .or_else(|| {
                self.messages
                    .iter()
                    .rev()
                    .find(|message| message.role == "user" && !message.text.trim().is_empty())
            });
        self.last_preview = last.map(|message| {
            let flat = message.text.lines().next().unwrap_or(&message.text).trim();
            let mut cut: String = flat.chars().take(120).collect();
            if flat.chars().count() > 120 {
                cut.push('…');
            }
            cut
        });
    }
}

/// The directory a workspace names, if it names one.
///
/// A list row carries the directory because a front end that groups
/// conversations by directory has nothing else to group by, and because the
/// alternative — the host guessing "the directory the server was started in" —
/// is the bug this replaces.
fn workspace_path_of(state: &State, workspace_id: Option<Uuid>) -> Option<String> {
    workspace_id
        .and_then(|id| state.workspaces.get(&id))
        .and_then(|workspace| workspace.path.clone())
}

/// A session's list row, with its directory resolved.
fn session_info(state: &State, session_id: Uuid) -> Option<SessionInfo> {
    let session = state.sessions.get(&session_id)?;
    let path = workspace_path_of(state, session.workspace_id);
    Some(session.info(session_id, path.as_deref()))
}

#[derive(Clone, Debug, Default)]
struct RuntimeSubagentProjection {
    revision: u64,
    children: HashMap<String, RunningSubagentInfo>,
    ended_child_session_ids: HashSet<String>,
}

impl RuntimeSubagentProjection {
    fn snapshot(&self) -> (u64, Vec<String>, Vec<RunningSubagentInfo>, usize) {
        let mut running: Vec<_> = self.children.values().cloned().collect();
        running.sort_by(|a, b| a.started_at_ms.cmp(&b.started_at_ms).then(a.child_session_id.cmp(&b.child_session_id)));
        let mut child_session_ids: Vec<_> = self.children.keys().cloned().collect();
        child_session_ids.sort();
        let ended_total = self.ended_child_session_ids.len();
        for child in &self.ended_child_session_ids {
            child_session_ids.push(child.clone());
        }
        child_session_ids.sort();
        (self.revision, child_session_ids, running, ended_total)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct State {
    sessions: HashMap<Uuid, SessionState>,
    seen_client_messages: HashSet<String>,
    workspaces: HashMap<Uuid, WorkspaceInfo>,
    active_workspace_id: Option<Uuid>,
    #[serde(default)]
    settings: GuiSettings,
    /// Imported conversations the user deleted from this GUI.
    ///
    /// An import is a projection of a store that still holds the conversation,
    /// so without a tombstone the next listing would resurrect something the user
    /// had just removed. Only the projection is deleted; the terminal's own file
    /// is untouched, because this host does not own it.
    #[serde(default)]
    dismissed_imports: HashSet<Uuid>,
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

    /// The canonical path this adapter is confined to. Reported to the client
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
            .filter(|entry| {
                let name = entry.file_name();
                name != ".chaos-staging" && name != ".git"
            })
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
        // This seam only ever carries text, so overwriting a file the client
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
            .filter_entry(|entry| {
                let name = entry.file_name().to_string_lossy();
                name != ".chaos-staging"
                    && name != ".git"
                    && name != "target"
                    && name != "node_modules"
            })
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
            let mut matched = relative.contains(query);
            if !matched
                && let Ok(file) = std::fs::File::open(entry.path())
                && let Ok(contents) = read_workspace_bytes(file)
                && contents.len() <= WORKSPACE_READ_LIMIT
                && let Ok(contents) = String::from_utf8(contents)
            {
                matched = contents.contains(query);
            }
            if matched {
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

    #[must_use]
    pub fn summary(&self) -> SessionDiffSummary {
        let log = self.log();
        let mut paths = HashSet::new();
        let mut additions = 0;
        let mut deletions = 0;
        for change in log.by_id.values() {
            paths.insert(change.relative_path.clone());
            let before_str = change
                .before
                .as_deref()
                .and_then(|b| std::str::from_utf8(b).ok())
                .unwrap_or_default();
            let after_str = std::str::from_utf8(&change.after).unwrap_or_default();
            let before_lines = before_str.lines().count();
            let after_lines = after_str.lines().count();
            if after_lines >= before_lines {
                additions += after_lines - before_lines;
            } else {
                deletions += before_lines - after_lines;
            }
        }
        let mut file_paths = paths.into_iter().collect::<Vec<_>>();
        file_paths.sort();
        SessionDiffSummary {
            files_changed: file_paths.len(),
            additions,
            deletions,
            file_paths,
        }
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

/// The row the engine's whole state lives in. See [`SqliteSessionStore::save_state`].
const STATE_ROW: &str = "__state__";

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
        self.save_row(&session_id.to_string(), payload)
    }

    /// Writes the engine's whole state, under one fixed row.
    ///
    /// The state is a single blob — every session and every workspace in the
    /// process — so it has one home. It used to be written under
    /// `state.sessions.keys().next()`, whichever session a HashMap happened to
    /// yield first: as sessions came and went, successive writes landed on
    /// different rows, rows were never deleted, and the restart read
    /// `ORDER BY rowid DESC LIMIT 1` — whichever snapshot had been *inserted*
    /// last, which after enough churn was an older one. A host listing a
    /// machine's conversations writes often enough to make that the normal
    /// path, so the state now has a name of its own and is updated in place.
    pub fn save_state(&self, payload: &str) -> rusqlite::Result<()> {
        self.save_row(STATE_ROW, payload)
    }

    fn save_row(&self, key: &str, payload: &str) -> rusqlite::Result<()> {
        let mode = xai_sqlite_journal::JournalMode::for_db_path(self.path.as_path());
        let conn = mode.open(self.path.as_path())?;
        conn.execute("INSERT INTO gui_sessions(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload", rusqlite::params![key, payload])?;
        Ok(())
    }

    fn load_state(&self) -> rusqlite::Result<State> {
        let mode = xai_sqlite_journal::JournalMode::for_db_path(self.path.as_path());
        let conn = mode.open_readonly(self.path.as_path())?;
        let payload: Option<String> = conn
            .query_row(
                "SELECT payload FROM gui_sessions WHERE id = ?1",
                [STATE_ROW],
                |row| row.get(0),
            )
            .optional()?;
        // A database written before the state had a row of its own keeps it
        // under a session id. Read the newest such row once; the next write puts
        // it where it belongs, and nothing reads that branch again.
        let payload = match payload {
            Some(payload) => Some(payload),
            None => conn
                .query_row(
                    "SELECT payload FROM gui_sessions ORDER BY rowid DESC LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .optional()?,
        };
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

/// `Mutex::lock` without the second panic.
///
/// The rule `xai-grok-shell`'s `LockOrRecover` trait states, restated here because
/// `chaos-engine` sits below that crate and cannot import it. A poisoned engine lock
/// means an earlier handler died holding it; what sits behind these locks is a
/// `State` struct, a subscriber map, a run registry and the `GuiSettings` snapshot,
/// none of whose invariants depend on that holder finishing. Panicking instead is
/// not merely noisy: `Drop for EventStream` takes the subscriber lock, and a panic
/// raised while the process is already unwinding aborts it. Recovery returns the data
/// and leaves the lock poisoned, so a panicking caller elsewhere still reports that a
/// holder died.
fn lock_or_recover<T>(lock: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    lock.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// How many events one host connection may fall behind before the engine stops
/// handing it events at all. The channel is bounded so a connection that stops
/// reading cannot grow memory without limit, and lossless up to that bound so a
/// transcript that arrived is never missing a piece of itself: an overflowing
/// connection is closed instead of trimmed, and the host reconnects for a fresh
/// snapshot rather than continuing on a stream with a hole in it.
const EVENT_CHANNEL_CAPACITY: usize = 256;

/// What a client is told when the thread producing its answer died.
///
/// A transcript that simply stops is what a dropped event looks like too, and a
/// connection cannot tell the two apart from the inside; the death is therefore
/// reported as the turn's outcome rather than left to be inferred.
const RUN_LOST: &str = "Agent 处理线程已终止";

/// How many prompt runs one engine produces at the same time.
///
/// A run is a thread parked on a Provider round trip or a subprocess, and turns
/// now run off their connection, so the number of threads is no longer bounded by
/// how fast one client can send frames. The bound therefore moves to where the
/// threads are: past this many, a further prompt is refused by name rather than
/// answered by a thread nobody is waiting for.
const MAX_CONCURRENT_RUNS: usize = 32;

/// What a client is told when every run slot is taken.
const RUNS_FULL: &str = "同时生成的回复已达上限，请等待其中一些结束";

/// The delivery end of one host connection's event stream.
///
/// Separate from [`EventStream`] and cheap to clone because a host has to address
/// answers to a connection while also reading from it, and holding the read end
/// borrowed across both is not expressible.
#[derive(Clone)]
pub struct EventSink {
    id: u64,
    subscribers: Arc<Mutex<HashMap<u64, mpsc::Sender<ServerMessage>>>>,
    session_watchers: Arc<Mutex<HashMap<Uuid, HashSet<u64>>>>,
}

impl EventSink {
    /// Whether the engine still delivers here. A sink stops being live when its
    /// buffer overflows, which is the host's cue to close the connection.
    pub fn is_live(&self) -> bool {
        lock_or_recover(&self.subscribers).contains_key(&self.id)
    }
}

/// One host connection's view of everything the engine produces.
///
/// Dropping it unregisters the connection, so a socket that went away stops
/// costing the engine a send per event.
pub struct EventStream {
    sink: EventSink,
    receiver: mpsc::Receiver<ServerMessage>,
}

impl EventStream {
    /// Where answers addressed to this connection -- as opposed to events every
    /// connection observes -- are delivered.
    pub fn sink(&self) -> EventSink {
        self.sink.clone()
    }

    /// The next event, or `None` once the engine has stopped delivering here:
    /// either the engine is gone or this connection overflowed.
    pub async fn recv(&mut self) -> Option<ServerMessage> {
        self.receiver.recv().await
    }
}

impl Drop for EventStream {
    fn drop(&mut self) {
        lock_or_recover(&self.sink.subscribers).remove(&self.sink.id);
        let mut watchers = lock_or_recover(&self.sink.session_watchers);
        watchers.retain(|_, ids| {
            ids.remove(&self.sink.id);
            !ids.is_empty()
        });
    }
}

/// What opening a prompt turn produced: either a run to stream, or the whole
/// answer, because the message was refused or was a command that ends the turn
/// without calling a Prompt at all.
enum SubmitStart {
    Running(Arc<RunControl>),
    Answered(Vec<ServerMessage>),
}

/// The knob a prompt run leaves where a later `cancel` can reach it. The run
/// itself is deliberately not joinable: the point of registering it is that
/// `cancel` no longer has to wait for the thing it is stopping.
#[derive(Default)]
struct RunControl {
    cancelled: AtomicBool,
    /// Whether this run has already been given an ending. A run that dies in a
    /// panic is closed by whoever noticed, and a run that had already closed must
    /// not be shown a second terminal event.
    finished: AtomicBool,
}

/// What one pass over the CLI's session store did.
///
/// Returned rather than logged because the host has to say, in its startup
/// banner, whether it found the terminal's conversations at all: "0 imported"
/// and "no store on this machine" are different facts a operator needs to see.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChaosImportReport {
    /// The store that was read, or `None` when there is nothing to read.
    pub root: Option<String>,
    /// Conversations the scan saw.
    pub scanned: usize,
    /// Conversations that were not in the state before this pass.
    pub imported: usize,
    /// Conversations already listed whose metadata was refreshed.
    pub refreshed: usize,
    /// Conversations skipped because the user deleted them from this host.
    pub dismissed: usize,
    /// Workspaces (directories) this pass created.
    pub workspaces: usize,
    /// Mirrored conversations this pass dropped because the terminal no longer
    /// lists them. See the reconciliation in [`Self::import_chaos_sessions`].
    pub retired: usize,
    /// Set when the store was read moments ago and nothing was done.
    pub skipped_recent: bool,
    /// The state changed but could not be written. The listing still shows what
    /// was read; it just will not survive the restart.
    pub persistence_error: Option<String>,
}

#[derive(Clone)]
pub struct Engine {
    /// Process-local runtime subagent projection. It is deliberately separate from persisted
    /// conversation state: child work belongs to the live agent runtime and never survives restart.
    subagents: Arc<Mutex<HashMap<Uuid, RuntimeSubagentProjection>>>,
    /// Live host connections, keyed by an id unique within this process.
    subscribers: Arc<Mutex<HashMap<u64, mpsc::Sender<ServerMessage>>>>,
    /// Which live connections follow which session, by session.
    ///
    /// A connection joins a set by naming that session in a request, and leaves it
    /// when its stream is dropped or its buffer overflowed. This is what keeps two
    /// tabs apart: without it, a session event would reach every socket on the
    /// host, so a second tab would watch a conversation it never opened.
    session_watchers: Arc<Mutex<HashMap<Uuid, HashSet<u64>>>>,
    next_subscriber: Arc<AtomicU64>,
    /// Prompt runs in flight, by session. Deliberately not part of `State`: a run
    /// lives in this process and a restart ends it, so persisting it would
    /// resurrect a turn that nothing is producing any more.
    runs: Arc<Mutex<HashMap<Uuid, Arc<RunControl>>>>,
    /// Slots a prompt run must hold to be producing. Shared by every clone of the
    /// engine, because the thing being bounded is this process's threads.
    run_slots: Arc<tokio::sync::Semaphore>,
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
    /// The CLI's session store this host mirrors, when the host gave it one.
    chaos_store_root: Option<Arc<PathBuf>>,
    /// When the CLI session store was last folded into the state, epoch ms.
    /// `0` means never, which is what makes the first listing do the work
    /// instead of skipping it.
    chaos_store_scan_ms: Arc<AtomicU64>,
    /// How long the built-in stand-in responder waits between chunks. A real
    /// adapter is paced by whatever produces it, so this only shapes the answer a
    /// host without a provider gives.
    demo_chunk_gap: Duration,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct GuiSettings {
    base_url: Option<String>,
    model: Option<String>,
    has_api_key: bool,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    thought_level: Option<String>,
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

    pub fn attach_tool_adapter(mut self, adapter: impl ToolAdapter + 'static) -> Self {
        self.tool_adapter = Some(Arc::new(adapter));
        self
    }

    #[must_use]
    pub fn diff_summary(&self) -> SessionDiffSummary {
        self.workspace_diff
            .as_ref()
            .map(|d| d.summary())
            .unwrap_or_default()
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

    /// What the next run should ask for, read at the moment the run starts.
    ///
    /// Reading it here rather than when the adapter was built is the point: the
    /// user changes the model in the settings panel long after the host started,
    /// and a pick that only changed the label on the composer pill was no pick
    /// at all.
    /// What the next run should ask for, including what the conversation itself
    /// brings: the directory it belongs to, the CLI session it is a view of, and
    /// enough of its own transcript for an adapter that remembers nothing.
    ///
    /// Read at the moment the run starts, like [`Self::run_options`], so a
    /// conversation opened in a directory the host was not started in is still
    /// answered in that directory.
    fn run_options_for(&self, session_id: Uuid) -> RunOptions {
        let mut options = self.run_options();
        let state = lock_or_recover(&self.state);
        let Some(session) = state.sessions.get(&session_id) else {
            return options;
        };
        options.working_dir = workspace_path_of(&state, session.workspace_id).map(PathBuf::from);
        if let Some(imported) = session.imported.as_ref() {
            options.resume_session = Some(imported.native_id.clone());
            if options.working_dir.is_none() {
                options.working_dir = Some(PathBuf::from(&imported.cwd));
            }
        }
        // `begin_submit` has already appended this turn's prompt and its empty
        // assistant placeholder by the time a run starts, so the transcript ends
        // with the very question the adapter is being handed. Drop it: it arrives
        // as `prompt`, and twice in one request is not more context, it is a
        // question asked twice.
        let mut history: Vec<TimelineMessage> = session
            .messages
            .iter()
            .filter(|message| {
                (message.role == "user" || message.role == "assistant")
                    && !message.text.trim().is_empty()
            })
            .cloned()
            .collect();
        if history.last().is_some_and(|message| message.role == "user") {
            history.pop();
        }
        let mut kept: Vec<TimelineMessage> = Vec::new();
        let mut bytes = 0usize;
        for message in history.into_iter().rev() {
            if kept.len() >= HISTORY_MAX_MESSAGES {
                break;
            }
            bytes += message.text.len();
            kept.push(message);
            if bytes > HISTORY_MAX_BYTES {
                break;
            }
        }
        kept.reverse();
        options.history = kept;
        options
    }

    fn run_options(&self) -> RunOptions {
        let settings = lock_or_recover(&self.settings);
        let model = settings
            .model
            .as_deref()
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .map(str::to_string);
        RunOptions {
            model,
            mode: settings.mode.clone(),
            thought_level: settings.thought_level.clone(),
            ..RunOptions::default()
        }
    }

    fn modes_message(&self) -> ServerMessage {
        let settings = lock_or_recover(&self.settings);
        ServerMessage::Modes {
            modes: vec![
                ModeInfo {
                    id: "build".into(),
                    label: "构建模式".into(),
                    description: "执行代码生成、工具调用与修改".into(),
                },
                ModeInfo {
                    id: "plan".into(),
                    label: "规划模式".into(),
                    description: "只读分析、架构设计与步骤拆解".into(),
                },
                ModeInfo {
                    id: "chat".into(),
                    label: "问答模式".into(),
                    description: "普通会话问答，不执行工具".into(),
                },
            ],
            current_mode: settings.mode.clone().unwrap_or_else(|| "build".into()),
            thought_levels: vec![
                ThoughtLevelInfo {
                    id: "none".into(),
                    label: "关闭思考".into(),
                    description: "直接输出最终结果，不展开推理过程".into(),
                },
                ThoughtLevelInfo {
                    id: "low".into(),
                    label: "简要思考".into(),
                    description: "快速简短的思路要点推理".into(),
                },
                ThoughtLevelInfo {
                    id: "medium".into(),
                    label: "标准思考".into(),
                    description: "适度展开关键步骤与权衡分析".into(),
                },
                ThoughtLevelInfo {
                    id: "high".into(),
                    label: "深度思考".into(),
                    description: "长链推理、边界假设与详尽验证分析".into(),
                },
            ],
            current_thought_level: settings
                .thought_level
                .clone()
                .unwrap_or_else(|| "medium".into()),
        }
    }

    /// The model the user chose, trimmed; empty or blank counts as "no choice".
    fn picked_model(&self) -> Option<String> {
        lock_or_recover(&self.settings)
            .model
            .as_deref()
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .map(str::to_string)
    }

    /// The model a run would use right now: the user's pick if there is one,
    /// otherwise whatever the adapter falls back to.
    fn effective_model(&self) -> Option<String> {
        self.picked_model().or_else(|| {
            self.adapter
                .as_ref()
                .and_then(|adapter| adapter.default_model().map(str::to_string))
        })
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
        Self {
            subagents: Arc::new(Mutex::new(HashMap::new())),
            subscribers: Arc::new(Mutex::new(HashMap::new())),
            session_watchers: Arc::new(Mutex::new(HashMap::new())),
            next_subscriber: Arc::new(AtomicU64::new(1)),
            runs: Arc::new(Mutex::new(HashMap::new())),
            run_slots: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_RUNS)),
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
            chaos_store_root: None,
            chaos_store_scan_ms: Arc::new(AtomicU64::new(0)),
            demo_chunk_gap: Duration::ZERO,
        }
    }

    /// Whether the engine currently owns an active turn for this session.
    pub fn session_is_running(&self, session_id: Uuid) -> bool {
        lock_or_recover(&self.runs).contains_key(&session_id)
    }

    /// 本会话正在进行的 API 重试状态，没有就是 `None`。桥用它在快照里如实
    /// 回答「正在重试」，让刷新过的客户端不丢这个事实。
    #[must_use]
    pub fn session_api_retry(&self, session_id: Uuid) -> Option<ApiRetryStatus> {
        lock_or_recover(&self.state)
            .sessions
            .get(&session_id)
            .and_then(|session| session.api_retry.clone())
    }

    /// Current process-local subagent projection for a parent session.
    #[must_use]
    pub fn session_subagents(
        &self,
        session_id: Uuid,
    ) -> (u64, Vec<String>, Vec<RunningSubagentInfo>, usize) {
        lock_or_recover(&self.subagents)
            .get(&session_id)
            .map(RuntimeSubagentProjection::snapshot)
            .unwrap_or_default()
    }

    /// Apply one structured subagent lifecycle event and broadcast the resulting snapshot.
    /// Duplicate sequence/attempt events do not bump revision or notify clients.
    pub fn update_subagent(
        &self,
        parent_session_id: Uuid,
        event: SubagentLifecycleEvent,
        sink: Option<&EventSink>,
        emit: &mut dyn FnMut(ServerMessage),
    ) -> bool {
        if event.event == "spawned"
            && (event.workflow_run_id.is_some() || event.run_in_background != Some(true))
        {
            return false;
        }
        let child_key = if event.child_session_id.is_empty() {
            event.subagent_id.clone()
        } else {
            event.child_session_id.clone()
        };
        if event.event == "finished"
            && lock_or_recover(&self.subagents)
                .get(&parent_session_id)
                .and_then(|projection| projection.children.get(&child_key))
                .is_none()
        {
            return false;
        }
        let now = usage::now_ms();
        let updated = {
            let mut projections = lock_or_recover(&self.subagents);
            let projection = projections.entry(parent_session_id).or_default();
            match event.event.as_str() {
                "spawned" => {
                    let child = RunningSubagentInfo {
                        child_session_id: child_key.clone(),
                        subagent_id: event.subagent_id.clone(),
                        parent_session_id: parent_session_id.to_string(),
                        subagent_type: event.subagent_type.clone(),
                        title: if event.description.trim().is_empty() {
                            event.subagent_type.clone()
                        } else {
                            event.description.clone()
                        },
                        description: event.description.clone(),
                        model: event.model.clone(),
                        status: "running".into(),
                        run_in_background: event.run_in_background.unwrap_or(true),
                        started_at_ms: event.started_at_ms.unwrap_or(now),
                        workflow_run_id: event.workflow_run_id.clone(),
                        agent_address: event.agent_address.clone(),
                        attempt_id: event.attempt_id.clone(),
                        event_seq: event.event_seq,
                        duration_ms: event.duration_ms,
                        parent_tool_call_id: event.parent_tool_call_id.clone(),
                    };
                    if projection.children.get(&child_key) == Some(&child) { return false; }
                    projection.ended_child_session_ids.remove(&child_key);
                    projection.children.insert(child_key, child);
                    true
                }
                "progress" => {
                    let Some(child) = projection.children.get_mut(&child_key) else { return false };
                    let status = event.status.as_deref().unwrap_or("running");
                    let mut changed = false;
                    if child.status != status { child.status = status.to_owned(); changed = true; }
                    if event.duration_ms.is_some() && child.duration_ms != event.duration_ms {
                        child.duration_ms = event.duration_ms;
                        changed = true;
                    }
                    if event.event_seq.is_some() && child.event_seq != event.event_seq {
                        child.event_seq = event.event_seq;
                        changed = true;
                    }
                    changed
                }
                "finished" => {
                    let Some(mut child) = projection.children.remove(&child_key) else { return false };
                    child.status = event.status.clone().unwrap_or_else(|| "completed".into());
                    child.duration_ms = event.duration_ms;
                    child.event_seq = event.event_seq.or(child.event_seq);
                    projection.ended_child_session_ids.insert(child_key);
                    true
                }
                _ => false,
            }
            .then(|| {
                projection.revision = projection.revision.saturating_add(1);
                projection.snapshot()
            })
        };
        let Some((revision, child_session_ids, running, ended_total)) = updated else {
            return false;
        };
        self.broadcast_or_emit(
            ServerMessage::SubagentsUpdated {
                session_id: parent_session_id,
                revision,
                child_session_ids,
                running,
                ended_total,
            },
            sink,
            emit,
        );
        true
    }

    /// 一个会话的队列视图：排队项、自动排水开关、是否暂停。
    #[must_use]
    pub fn session_queue(&self, session_id: Uuid) -> (Vec<QueuedInput>, bool, bool) {
        lock_or_recover(&self.state)
            .sessions
            .get(&session_id)
            .map(|session| session.queue_snapshot())
            .unwrap_or_default()
    }

    /// Pending interactions of one session, counted for the sidebar badge.
    ///
    /// 客户端会话索引可用 `pendingInteractionSummary` 展示待处理交互；这格数字
    /// 不能写死为零，因为审批与提问确实会停在会话里等人。返回 `(approvals, questions)`；
    /// 会话不存在或没有 pending 时是 `(0, 0)`——「没有交互」是事实，不是省略。
    #[must_use]
    pub fn pending_interaction_counts(&self, session_id: Uuid) -> (usize, usize) {
        let state = lock_or_recover(&self.state);
        let Some(session) = state.sessions.get(&session_id) else {
            return (0, 0);
        };
        (
            usize::from(session.pending_approval.is_some()),
            usize::from(session.pending_question.is_some()),
        )
    }

    /// The thinking level the next run will carry, when the user picked one.
    ///
    /// `None` means "provider default" — the honest display for a knob nobody
    /// has turned, not a made-up tier to fill the pill.
    #[must_use]
    pub fn current_thought_level(&self) -> Option<String> {
        lock_or_recover(&self.settings)
            .thought_level
            .as_deref()
            .map(str::trim)
            .filter(|level| !level.is_empty())
            .map(str::to_owned)
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

    /// The namespace every identity derived from the CLI's store is derived
    /// from, so the same conversation gets the same session id in every process,
    /// on every restart, and on a host that has never seen it before. Random ids
    /// would make each rescan a fresh set of duplicates.
    const CHAOS_STORE_NAMESPACE: Uuid = Uuid::from_u128(0x8f3a_2c51_4b7e_9d04_a1c6_0e5b_7f28_d390);

    /// A directory as a workspace key: canonical when it exists, so `~/proj`,
    /// `~/proj/` and a symlink to it are one workspace, and the trimmed literal
    /// when it does not (a conversation can outlive its directory).
    fn directory_key(path: &str) -> String {
        let trimmed = path.trim_end_matches('/');
        let trimmed = if trimmed.is_empty() { "/" } else { trimmed };
        match dunce::canonicalize(trimmed) {
            Ok(resolved) if resolved.is_dir() => resolved.to_string_lossy().into_owned(),
            _ => trimmed.to_string(),
        }
    }

    /// The session id this engine gives a conversation from the CLI's store.
    fn imported_session_id(cwd: &str, native_id: &str) -> Uuid {
        Uuid::new_v5(
            &Self::CHAOS_STORE_NAMESPACE,
            format!("{cwd}\u{0}{native_id}").as_bytes(),
        )
    }

    /// Find — or create — the workspace that *is* this directory.
    ///
    /// The point is that a conversation created in the client in `~/project-a`
    /// and one the terminal had in `~/project-a` land in the same workspace,
    /// under the same id, rather than in a name the client invented and a
    /// directory the host guessed.
    pub fn workspace_for_directory(&self, path: &str) -> Uuid {
        let key = Self::directory_key(path);
        let id = Uuid::new_v5(&Self::CHAOS_STORE_NAMESPACE, key.as_bytes());
        let mut state = lock_or_recover(&self.state);
        if let std::collections::hash_map::Entry::Vacant(slot) = state.workspaces.entry(id) {
            let name = Path::new(key.as_str())
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| key.clone());
            slot.insert(WorkspaceInfo {
                id,
                name,
                archived: false,
                last_used_sequence: 0,
                last_session_id: None,
                path: Some(key),
            });
        }
        id
    }

    /// Folds the Chaos CLI's own conversation store into this engine's state, so
    /// every conversation the terminal had — in whichever directory it had it —
    /// is a session here: listed, opened, and continuable.
    ///
    /// This is deliberately *not* a client message. The root is the machine's
    /// own Chaos home, decided by the operator's environment, and a client that
    /// could name an arbitrary root would be naming arbitrary directories.
    /// [`ClientMessage::ImportTuiSession`] stays the allowlisted, one-at-a-time
    /// path for a client that was handed a root; this is the host saying "the
    /// store is there, show me what is in it".
    ///
    /// What lands in the state is metadata and provenance only. Transcripts are
    /// read when a conversation is opened ([`Self::hydrate_imported_transcript`]),
    /// not here, because the whole state is re-serialized on every write and a
    /// store holding hundreds of conversations would make every keystroke in the
    /// GUI rewrite all of them.
    ///
    /// Idempotent, and re-running it is how a conversation the terminal added a
    /// minute ago shows up without restarting the host. A conversation the user
    /// deleted here stays deleted: this host projects the CLI's store, it does
    /// not own it.
    pub fn import_chaos_sessions(&self, root: &Path) -> ChaosImportReport {
        let found = chaos_store::scan_store(root, chaos_store::SCAN_MAX_SESSIONS);
        self.chaos_store_scan_ms
            .store(usage::now_ms(), Ordering::SeqCst);
        let mut report = ChaosImportReport {
            root: Some(root.display().to_string()),
            scanned: found.len(),
            ..ChaosImportReport::default()
        };
        // A store that is not there at all says nothing, and an empty listing
        // must not be read as "delete everything mirrored". An empty store that
        // does exist is a fact: nothing is left to list.
        if found.is_empty() && !root.is_dir() {
            return report;
        }
        let mut state = lock_or_recover(&self.state);
        // A directory is canonicalised once per scan rather than once per
        // conversation, and the canonical form is the workspace key: `~/proj`,
        // `~/proj/` and a symlink to it are one directory, and two workspaces
        // for one directory is two sidebars for one project.
        let mut keys: HashMap<String, String> = HashMap::new();
        // Which conversation is the newest one seen for each workspace, so
        // switching to a directory opens the conversation the user was last in.
        let mut newest: HashMap<Uuid, (u64, Uuid)> = HashMap::new();
        // Every conversation the store still vouches for, by the id this host
        // gives it. Anything mirrored that is not in here is stale.
        let mut listed: HashSet<Uuid> = HashSet::new();
        for meta in found {
            let cwd = match keys.get(&meta.cwd) {
                Some(key) => key.clone(),
                None => {
                    let key = Self::directory_key(&meta.cwd);
                    keys.insert(meta.cwd.clone(), key.clone());
                    key
                }
            };
            let workspace_id = Uuid::new_v5(&Self::CHAOS_STORE_NAMESPACE, cwd.as_bytes());
            let session_id = Self::imported_session_id(&cwd, &meta.native_id);
            if state.dismissed_imports.contains(&session_id) {
                report.dismissed += 1;
                continue;
            }
            listed.insert(session_id);
            if let std::collections::hash_map::Entry::Vacant(slot) =
                state.workspaces.entry(workspace_id)
            {
                let name = Path::new(cwd.as_str())
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| cwd.clone());
                slot.insert(WorkspaceInfo {
                    id: workspace_id,
                    name,
                    archived: false,
                    last_used_sequence: 0,
                    last_session_id: None,
                    path: Some(cwd.clone()),
                });
                report.workspaces += 1;
            } else if state
                .workspaces
                .get(&workspace_id)
                .is_some_and(|workspace| workspace.path.is_none())
            {
                // A workspace that predates paths, or that a client made with a
                // name only: this conversation says which directory it is.
                if let Some(workspace) = state.workspaces.get_mut(&workspace_id) {
                    workspace.path = Some(cwd.clone());
                }
            }

            let title = meta.display_title();
            if let Some(existing) = state.sessions.get_mut(&session_id) {
                // The terminal kept talking: the conversation is newer, and maybe
                // longer, than what this host last saw. A title the user renamed
                // here stays theirs — only a title still equal to what the CLI
                // said follows the CLI.
                let mut changed = false;
                if existing.title == existing.cli_title && existing.title != title {
                    existing.cli_title = title.clone();
                    existing.title = title;
                    changed = true;
                }
                if existing.stored_message_count != meta.message_count {
                    // A transcript already read into this session is the better
                    // count, so the stored number only follows the CLI when
                    // nothing has been read yet.
                    existing.stored_message_count = meta.message_count;
                    changed = true;
                }
                if meta.updated_at_ms > existing.updated_at_ms {
                    existing.updated_at_ms = meta.updated_at_ms;
                    changed = true;
                }
                if existing.created_at_ms == 0 && meta.created_at_ms != 0 {
                    existing.created_at_ms = meta.created_at_ms;
                    changed = true;
                }
                // Counted only when something actually moved, so a report saying
                // "refreshed" means the terminal changed something and not that
                // this host looked.
                if changed {
                    report.refreshed += 1;
                }
            } else {
                state.sessions.insert(
                    session_id,
                    SessionState {
                        workspace_id: Some(workspace_id),
                        title: title.clone(),
                        cli_title: title,
                        stored_message_count: meta.message_count,
                        created_at_ms: meta.created_at_ms,
                        updated_at_ms: meta.updated_at_ms,
                        imported: Some(ImportedSession {
                            cwd: cwd.clone(),
                            native_id: meta.native_id.clone(),
                            directory: meta.directory.display().to_string(),
                        }),
                        ..SessionState::default()
                    },
                );
                report.imported += 1;
            }
            if let Some(workspace) = state.workspaces.get_mut(&workspace_id) {
                workspace.last_used_sequence = workspace.last_used_sequence.saturating_add(1);
            }
            match newest.get(&workspace_id) {
                Some((best, _)) if *best >= meta.updated_at_ms => {}
                _ => {
                    newest.insert(workspace_id, (meta.updated_at_ms, session_id));
                }
            }
        }
        for (workspace_id, (_, session_id)) in newest {
            if let Some(workspace) = state.workspaces.get_mut(&workspace_id) {
                workspace.last_session_id = Some(session_id);
            }
        }
        // A mirror, not an archive. A conversation the terminal stopped listing —
        // deleted, hidden, or one that turns out to be a nameless husk under the
        // rules the picker applies — has to leave this listing too, or the sidebar
        // keeps rows forever that its own client has already forgotten.
        //
        // A row this host has since written into stays: it holds a turn the
        // terminal's file does not, and dropping it would throw away the answer
        // the user just got.
        let retired: Vec<(Uuid, Option<Uuid>)> = state
            .sessions
            .iter()
            .filter(|(id, session)| {
                session.imported.is_some() && !listed.contains(id) && session.messages.is_empty()
            })
            .map(|(id, session)| (*id, session.workspace_id))
            .collect();
        for (session_id, workspace_id) in retired {
            if let Some(removed) = state.sessions.remove(&session_id) {
                if let Some(workspace_id) = removed.workspace_id.or(workspace_id) {
                    let fallback = state.sessions.iter().find_map(|(id, session)| {
                        (session.workspace_id == Some(workspace_id)).then_some(*id)
                    });
                    if let Some(workspace) = state.workspaces.get_mut(&workspace_id)
                        && workspace.last_session_id == Some(session_id)
                    {
                        workspace.last_session_id = fallback;
                    }
                }
                report.retired += 1;
            }
        }
        if let Err(ServerMessage::Error { message, .. }) = self.persist(&state) {
            report.persistence_error = Some(message);
        }
        report
    }

    /// Names the CLI's session store this host mirrors, so a listing can refresh
    /// it later without being told where it is a second time.
    ///
    /// Only the host calls this. A client is never handed a root, because a root
    /// a client chooses is a directory the client chooses.
    #[must_use]
    pub fn with_chaos_store_root(mut self, root: impl AsRef<Path>) -> Self {
        let given = root.as_ref();
        // A store that does not exist yet is still the store this host mirrors:
        // the CLI may create it after this process started, and a listing that
        // resolved the name once at startup would never notice it.
        let root = dunce::canonicalize(given).unwrap_or_else(|_| given.to_path_buf());
        self.chaos_store_root = Some(Arc::new(root));
        self
    }

    /// The store this host mirrors, if it was given one.
    #[must_use]
    pub fn chaos_store_root(&self) -> Option<PathBuf> {
        self.chaos_store_root.as_deref().cloned()
    }

    /// Re-reads the CLI's store unless it was read moments ago, so a listing can
    /// call this on every refresh without turning the sidebar into a disk walk.
    ///
    /// A host that never named a store gets an empty report: no scanning, no
    /// guessing, nothing to mirror.
    pub fn refresh_chaos_sessions(&self) -> ChaosImportReport {
        const MIN_INTERVAL_MS: u64 = 5_000;
        let Some(root) = self.chaos_store_root() else {
            return ChaosImportReport::default();
        };
        let last = self.chaos_store_scan_ms.load(Ordering::SeqCst);
        let now = usage::now_ms();
        if last != 0 && now.saturating_sub(last) < MIN_INTERVAL_MS {
            return ChaosImportReport {
                root: Some(root.display().to_string()),
                skipped_recent: true,
                ..ChaosImportReport::default()
            };
        }
        self.import_chaos_sessions(&root)
    }

    /// Where the machine's own Chaos session store lives, if it has one.
    #[must_use]
    pub fn chaos_store_dir() -> Option<PathBuf> {
        chaos_store::sessions_dir()
    }

    /// The transcript a resumed conversation has on disk, when it is one the CLI
    /// owns and nothing has been read into it yet.
    ///
    /// `None` for every conversation this process created itself, and for one
    /// that already holds a transcript — there is nothing on that disk to read.
    fn transcript_on_disk(
        state: &Mutex<State>,
        session_id: Uuid,
    ) -> Option<(Uuid, Vec<TimelineMessage>)> {
        let directory = {
            let state = lock_or_recover(state);
            let session = state.sessions.get(&session_id)?;
            if !session.messages.is_empty() {
                return None;
            }
            session.imported.as_ref()?.directory.clone()
        };
        let transcript = chaos_store::read_transcript(Path::new(&directory));
        (!transcript.is_empty()).then_some((session_id, transcript))
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

    /// Paces the built-in stand-in responder so an answer arrives over time instead
    /// of in one write.
    ///
    /// A host started without a provider has no producer to set the pace, and left
    /// unpaced it emits a whole reply inside one turn of the socket loop, which is
    /// indistinguishable on screen from a host that cannot stream at all. This only
    /// shapes that stand-in: with an adapter, the producer's own timing is the pace.
    #[must_use]
    pub fn with_demo_pacing(mut self, gap: Duration) -> Self {
        self.demo_chunk_gap = gap;
        self
    }

    /// Registers a host connection as a receiver of this engine's events: the
    /// answers to its own requests, plus the events of every session it has named
    /// in a request (see [`Engine::follow`] and [`Engine::publish`]).
    ///
    /// A host that reads this stream must not also send a request's return value
    /// to the same connection: [`Self::handle`] is for hosts with no subscription,
    /// and a host doing both would show every event twice.
    pub fn subscribe(&self) -> EventStream {
        let id = self.next_subscriber.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::channel(EVENT_CHANNEL_CAPACITY);
        lock_or_recover(&self.subscribers).insert(id, sender);
        EventStream {
            sink: EventSink {
                id,
                subscribers: Arc::clone(&self.subscribers),
                session_watchers: Arc::clone(&self.session_watchers),
            },
            receiver,
        }
    }

    /// Hands one event to the connection that asked for it, if any.
    fn deliver_to(&self, sink: &EventSink, event: ServerMessage) {
        let evicted = {
            let mut subscribers = lock_or_recover(&self.subscribers);
            let Some(sender) = subscribers.get(&sink.id) else {
                return;
            };
            if sender.try_send(event).is_ok() {
                return;
            }
            // Full means the connection stopped reading, and Closed that it
            // is gone. Either way the only honest state is unregistered:
            // keeping it would let the next event look delivered while this
            // one vanished.
            subscribers.remove(&sink.id);
            Some(sink.id)
        };
        if let Some(id) = evicted {
            self.unwatch(id);
        }
    }

    /// Forgets a connection entirely: no more events, and no more claims on any
    /// session's watcher set. Both locks are taken one at a time, never together.
    fn unwatch(&self, id: u64) {
        lock_or_recover(&self.session_watchers).retain(|_, watchers| {
            watchers.remove(&id);
            !watchers.is_empty()
        });
    }

    /// Records that the connection making this request follows its session, so the
    /// events that session goes on to produce come back to it.
    ///
    /// Registration happens before the request is handled, because a `submit`
    /// produces its first delta while it is being handled: a connection registered
    /// afterwards would miss the start of the very answer it asked for. The set
    /// grows only as connections name sessions and shrinks back to nothing when
    /// those connections go away, so it cannot outlive the clients that made it.
    fn follow(&self, session_id: Uuid, sink: &EventSink) {
        lock_or_recover(&self.session_watchers)
            .entry(session_id)
            .or_default()
            .insert(sink.id);
    }

    /// Registers the connection a transcript was just handed to as a follower of
    /// the session it now shows.
    ///
    /// A transcript is an answer, so it comes back addressed rather than broadcast,
    /// but the connection that received it is looking at that conversation and
    /// everything it goes on to produce is something that connection has to be told
    /// about. `switch_workspace` is why this is needed at all: the session it moves
    /// the tab to is chosen here, so the request that caused it names no session for
    /// [`Self::follow`] to register, and a tab that had just been given a
    /// conversation would otherwise watch it freeze.
    fn adopt(&self, event: &ServerMessage, sink: &EventSink) {
        if !matches!(
            event,
            ServerMessage::SessionSnapshot { .. } | ServerMessage::SessionCreated { .. }
        ) {
            return;
        }
        if let Some(session_id) = event.session_id() {
            self.follow(session_id, sink);
        }
    }

    /// Hands one event to the connections entitled to see it, evicting any whose
    /// buffer is full rather than dropping an event from a transcript that keeps
    /// reading.
    ///
    /// An event that names a session goes only to the connections that named that
    /// session themselves; one that names none -- the workspace list, a resolved
    /// diff -- is a fact about the host rather than a conversation, and reaches
    /// every connection. Scoping the rest is what keeps a second tab from watching
    /// a session it never opened. The watcher set is copied out before the
    /// subscriber lock is taken, so the two locks are never held at once.
    fn publish(&self, event: &ServerMessage) {
        let followers = event.session_id().map(|session_id| {
            lock_or_recover(&self.session_watchers)
                .get(&session_id)
                .cloned()
                .unwrap_or_default()
        });
        let mut evicted: Vec<u64> = Vec::new();
        lock_or_recover(&self.subscribers).retain(|id, sender| {
            if followers.as_ref().is_some_and(|set| !set.contains(id)) {
                return true;
            }
            if sender.try_send(event.clone()).is_err() {
                evicted.push(*id);
                return false;
            }
            true
        });
        for id in evicted {
            self.unwatch(id);
        }
    }

    /// Puts one produced event in front of the clients that should see it.
    ///
    /// Session events are what happened, so they go through [`Self::publish`],
    /// which hands them to the connections following that session; anything else
    /// answers one request and goes only to the connection that made it. A
    /// broadcast `file_contents` would put one tab's file into another tab's
    /// viewer, which is neither what the other tab asked for nor its data to show.
    fn broadcast_or_emit(
        &self,
        event: ServerMessage,
        sink: Option<&EventSink>,
        emit: &mut dyn FnMut(ServerMessage),
    ) {
        if !event.is_session_event() {
            match sink {
                Some(sink) => {
                    self.adopt(&event, sink);
                    self.deliver_to(sink, event);
                }
                None => emit(event),
            }
            return;
        }
        match sink {
            Some(_) => {
                // The asking connection is registered like any other, so one
                // publish reaches it too; emitting here as well would double it.
                self.publish(&event);
            }
            None => {
                self.publish(&event);
                emit(event);
            }
        }
    }

    fn persist(&self, state: &State) -> Result<(), ServerMessage> {
        if let Some(store) = &self.sqlite_store {
            let payload = serde_json::to_string(&StateEnvelope {
                schema_version: STATE_SCHEMA_VERSION,
                state: state.clone(),
            })
            .map_err(|_| Self::error("persistence_failed", "无法编码会话状态"))?;
            store
                .save_state(&payload)
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

    /// Handles one client message and returns every event it produced.
    ///
    /// This is the entry point for a host with no subscription. A host that reads
    /// [`Self::subscribe`] uses [`Self::dispatch_to`] instead, so a prompt's chunks
    /// reach the socket as the Provider produces them rather than in one burst
    /// after the last one, and so no event is delivered twice.
    pub fn handle(&self, message: ClientMessage) -> Vec<ServerMessage> {
        let mut events = Vec::new();
        self.dispatch_to(message, None, &mut |event| events.push(event));
        events
    }

    /// 汇总本引擎记录的真实用量，返回官方前端 `AppUsageSnapshot` 形状的 JSON 值。
    ///
    /// 这是引擎客户端 API 的用量快照入口。没有 sqlite / JSON 后端（纯内存）时
    /// 同样可答，只是账本只覆盖本进程的生命周期。
    /// 任何内部失败都降级为形状完整的空快照，而不是让客户端看到报错。
    #[must_use]
    pub fn app_usage_snapshot(&self, range: &str, time_zone: Option<&str>) -> serde_json::Value {
        let events = self.handle(ClientMessage::UsageStats {
            client_msg_id: format!("usage-snapshot-{}", Uuid::new_v4()),
            range: range.to_string(),
            time_zone: time_zone.map(str::to_owned),
        });
        events
            .into_iter()
            .find_map(|event| match event {
                ServerMessage::UsageStatsSnapshot { snapshot, .. } => {
                    serde_json::from_str(&snapshot).ok()
                }
                _ => None,
            })
            .unwrap_or_else(|| usage::empty_app_usage_snapshot(range, time_zone, usage::now_ms()))
    }

    /// Handles one client message, routing each event the moment it is produced:
    /// session events to the connections following that session, answers to `sink`.
    /// `sink` is `None` for a host that takes events from the return path of
    /// [`Self::handle`] rather than from a subscription.
    ///
    /// The call itself never blocks on a prompt: submitting one is routed before the
    /// state lock, onto no lock at all. Holding that lock across the adapter call is
    /// what left `cancel` unable to reach the run it was asked to stop, and every
    /// other client message queued behind it.
    pub fn dispatch_to(
        &self,
        message: ClientMessage,
        sink: Option<&EventSink>,
        emit: &mut dyn FnMut(ServerMessage),
    ) {
        // Before handling, not after: the events this request causes are produced
        // during handling, and the connection that asked has to be listening for
        // them already.
        if let (Some(session_id), Some(sink)) = (message.session_id(), &sink) {
            self.follow(session_id, sink);
        }
        if let ClientMessage::Submit {
            client_msg_id,
            session_id,
            prompt,
        } = message
        {
            if !self.remember_client_message(&client_msg_id) {
                self.broadcast_or_emit(ServerMessage::Ack { client_msg_id }, sink, emit);
                return;
            }
            self.run_submit(client_msg_id, session_id, prompt, sink, emit);
            return;
        }
        for event in self.run_message(message) {
            self.broadcast_or_emit(event, sink, emit);
        }
    }

    /// Whether this request is being seen for the first time. Deduplicated here
    /// rather than inside the big match, because the prompt path decides what to do
    /// before it takes the lock at all.
    fn remember_client_message(&self, client_msg_id: &str) -> bool {
        lock_or_recover(&self.state)
            .seen_client_messages
            .insert(client_msg_id.to_string())
    }

    /// Runs one prompt: opens the turn, streams the answer as the adapter produces
    /// it, then closes the turn with the outcome the client actually saw.
    ///
    /// Three lock scopes, none of them spanning the adapter. The first validates and
    /// registers the run, so a `cancel` cannot land between "this session is
    /// producing" and "there is something to stop". The adapter then runs with no
    /// lock held, and each chunk takes one only long enough to record and hand out.
    /// The last closes the turn and unregisters the run; because it takes the same
    /// lock `cancel` needs, a stop is either seen by the run or answered on the
    /// spot, never dropped and never counted twice.
    /// Opens a prompt turn: refuses what must not run, answers what needs no
    /// Prompt, and otherwise records the turn and registers the run.
    ///
    /// The registration happens here, under the same lock that decided the session
    /// is free, so a `cancel` cannot slip between "this session is producing" and
    /// "there is a knob to stop it".
    fn begin_submit(&self, session_id: Uuid, client_msg_id: &str, prompt: &str) -> SubmitStart {
        let mut state = lock_or_recover(&self.state);
        let refusal = |code: &str, message: &str| {
            vec![ServerMessage::Error {
                code: code.into(),
                message: message.into(),
            }]
        };
        let Some(workspace_id) = state
            .sessions
            .get(&session_id)
            .and_then(|session| session.workspace_id)
        else {
            return SubmitStart::Answered(refusal("session_not_found", "会话不存在"));
        };
        if state
            .workspaces
            .get(&workspace_id)
            .is_some_and(|workspace| workspace.archived)
        {
            return SubmitStart::Answered(refusal("workspace_unavailable", "会话工作区已归档"));
        }
        if let Some(workspace) = state.workspaces.get_mut(&workspace_id) {
            workspace.last_used_sequence = workspace.last_used_sequence.saturating_add(1);
        }
        let Some(session) = state.sessions.get_mut(&session_id) else {
            return SubmitStart::Answered(refusal("session_not_found", "会话不存在"));
        };
        if session.pending_approval.is_some() {
            return SubmitStart::Answered(refusal("approval_pending", "请先处理待审批操作"));
        }
        if session.pending_question.is_some() {
            return SubmitStart::Answered(refusal("question_pending", "请先回答待处理问题"));
        }
        if lock_or_recover(&self.runs).contains_key(&session_id) {
            // Two answers into one transcript would interleave their chunks, and the
            // second would settle the first turn as finished. Steering a run that is
            // already going is its own contract, not a second submit.
            return SubmitStart::Answered(refusal(
                "session_busy",
                "上一轮仍在进行，请等待它结束或先取消",
            ));
        }
        if let Some(question) = prompt.strip_prefix("/ask ") {
            let question_id = Uuid::new_v4();
            session.pending_question = Some(question_id);
            session.pending_question_prompt = Some(question.to_owned());
            session.sequence += 1;
            let sequence = session.sequence;
            let question = question.to_owned();
            let persisted = self.persist(&state);
            drop(state);
            if let Err(error) = persisted {
                return SubmitStart::Answered(vec![error]);
            }
            return SubmitStart::Answered(vec![
                ServerMessage::Ack {
                    client_msg_id: client_msg_id.to_string(),
                },
                ServerMessage::QuestionRequested {
                    session_id,
                    question_id,
                    prompt: question,
                    sequence,
                },
            ]);
        }
        if prompt.trim() == "/clear" {
            session.messages.clear();
            session.sequence += 1;
            let sequence = session.sequence;
            let persisted = self.persist(&state);
            drop(state);
            if let Err(error) = persisted {
                return SubmitStart::Answered(vec![error]);
            }
            return SubmitStart::Answered(vec![
                ServerMessage::Ack {
                    client_msg_id: client_msg_id.to_string(),
                },
                ServerMessage::TurnStarted {
                    session_id,
                    prompt: prompt.to_string(),
                    sequence,
                },
                ServerMessage::TextDelta {
                    session_id,
                    text: "✅ **会话上下文已清空**：当前对话已重置，已释放上下文历史。".to_string(),
                    sequence: sequence + 1,
                },
                ServerMessage::Completed {
                    session_id,
                    sequence: sequence + 2,
                },
            ]);
        }
        if let Some(summary) = prompt.strip_prefix("/approve-tool ") {
            let request_id = Uuid::new_v4();
            let summary = summary.to_owned();
            session.pending_approval = Some(PendingApproval {
                request_id,
                tool: "demo.tool".into(),
                summary: summary.clone(),
                confirmations_required: 1,
                confirmations: 0,
            });
            session.sequence += 1;
            let sequence = session.sequence;
            let persisted = self.persist(&state);
            drop(state);
            if let Err(error) = persisted {
                return SubmitStart::Answered(vec![error]);
            }
            return SubmitStart::Answered(vec![
                ServerMessage::Ack {
                    client_msg_id: client_msg_id.to_string(),
                },
                ServerMessage::ToolApprovalRequested {
                    session_id,
                    request_id,
                    tool: "demo.tool".into(),
                    summary,
                    sequence,
                },
            ]);
        }
        // The assistant message opens empty and grows with the chunks, so the
        // transcript a client watched arrive is the one that gets stored.
        session.messages.push(TimelineMessage {
            role: "user".into(),
            text: prompt.to_owned(),
        });
        session.messages.push(TimelineMessage {
            role: "assistant".into(),
            text: String::new(),
        });
        session.touch();
        let control = Arc::<RunControl>::default();
        lock_or_recover(&self.runs).insert(session_id, Arc::clone(&control));
        if let Err(error) = self.persist(&state) {
            lock_or_recover(&self.runs).remove(&session_id);
            return SubmitStart::Answered(vec![error]);
        }
        SubmitStart::Running(control)
    }

    /// Starts a prompt turn, then decides who waits for it.
    ///
    /// A host with no subscription reads events out of the return path of
    /// [`Self::handle`], and a return path cannot be handed to another thread, so
    /// that host waits for its turn to end. A host that did subscribe is a
    /// connection: the answer continues on a thread of its own and the request that
    /// started it is answered at once, so the stop button, the file panel and the
    /// next frame on that same socket keep moving while the Provider is still
    /// writing. Without this, one connection could not cancel its own turn, because
    /// the only thread able to read the `cancel` frame was busy producing it.
    fn run_submit(
        &self,
        client_msg_id: String,
        session_id: Uuid,
        prompt: String,
        sink: Option<&EventSink>,
        emit: &mut dyn FnMut(ServerMessage),
    ) {
        // Taken before the turn is opened, so a prompt that cannot be produced is
        // refused without having written anything into the transcript.
        let Ok(slot) = self.run_slots.clone().try_acquire_owned() else {
            self.broadcast_or_emit(
                ServerMessage::Error {
                    code: "too_many_runs".into(),
                    message: RUNS_FULL.into(),
                },
                sink,
                emit,
            );
            return;
        };
        let control = match self.begin_submit(session_id, &client_msg_id, &prompt) {
            SubmitStart::Answered(events) => {
                for event in events {
                    self.broadcast_or_emit(event, sink, emit);
                }
                return;
            }
            SubmitStart::Running(control) => control,
        };
        self.broadcast_or_emit(ServerMessage::Ack { client_msg_id }, sink, emit);
        let Some(sink) = sink else {
            let _slot = slot;
            self.produce_run(session_id, &control, &prompt, None, emit);
            self.drain_queue_after_run(session_id, None, emit);
            return;
        };
        let engine = self.clone();
        let control = Arc::clone(&control);
        let answer_to = sink.clone();
        std::thread::spawn(move || {
            // The slot is the run: it goes back when the answer is over, not when
            // the request that started it was answered.
            let _slot = slot;
            // A Provider that panics has to leave the session free rather than busy
            // forever with nothing producing, and the clients have to be told: a
            // transcript that stops mid-sentence with no ending at all is
            // indistinguishable from a network that dropped one event.
            let produced = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                engine.produce_run(
                    session_id,
                    &control,
                    &prompt,
                    Some(&answer_to),
                    &mut |_event| {},
                )
            }));
            if produced.is_err() {
                engine.finish_run(
                    session_id,
                    &control,
                    Some(&answer_to),
                    &mut |_event| {},
                    Some(RUN_LOST.to_string()),
                );
            }
            engine.drain_queue_after_run(session_id, Some(&answer_to), &mut |_event| {});
        });
    }

    /// 轮次终局后的队列排水（队列合同的自动排水半边）。
    ///
    /// 规则：`auto_drain` 开着、没有暂停、队列非空、会话没有挂着待答的交互时，
    /// 把队头交给下一轮。用户按过停止（`queue_paused`）之后不自动开新一轮——
    /// 按停止是要暂停，不是要连发下一条；`SendQueuedNow`/`SetQueueAutoDrain`
    /// 显式恢复。每轮跑完再取下一条，直到队列空或条件不再成立。
    fn drain_queue_after_run(
        &self,
        session_id: Uuid,
        sink: Option<&EventSink>,
        emit: &mut dyn FnMut(ServerMessage),
    ) {
        loop {
            // 会话正忙就走人。排水是「一轮结束后接着排下一轮」，恢复路径
            // （`resume_queue`）却可能和刚开起来的新轮撞上：已经取下的队头
            // 交给 run_submit 只会被 session_busy 退掉，那句话就此消失。
            // 留在队列里，等这一轮自己排完再取。
            if self.session_is_running(session_id) {
                return;
            }
            let next = {
                let mut state = lock_or_recover(&self.state);
                let Some(session) = state.sessions.get_mut(&session_id) else {
                    return;
                };
                if session.queue_paused
                    || !session.queue_auto_drain
                    || session.queued_inputs.is_empty()
                    || session.pending_approval.is_some()
                    || session.pending_question.is_some()
                {
                    return;
                }
                session.queued_inputs.remove(0)
            };
            // 排水公告：队列少一项（连接上所有人都能对账乐观命令）。
            let mut announcements = Vec::new();
            {
                let mut state = lock_or_recover(&self.state);
                if let Some(session) = state.sessions.get_mut(&session_id) {
                    session.sequence += 1;
                    let (items, auto_drain, paused) = session.queue_snapshot();
                    announcements.push(ServerMessage::QueueUpdated {
                        session_id,
                        items,
                        auto_drain,
                        paused,
                        sequence: session.sequence,
                    });
                }
            }
            for event in announcements {
                self.broadcast_or_emit(event, sink, emit);
            }
            // 真正开下一轮。run_submit 自己负责拿运行槽、发 Ack 与终局事件；
            // 有 sink 的路径在独立线程上跑，这里等它结束再取下一条。
            self.run_submit(
                format!("drain-{}", next.queue_item_id),
                session_id,
                next.text,
                sink,
                emit,
            );
            let mut wait = 0;
            while self.session_is_running(session_id) && wait < 60_000 {
                std::thread::sleep(std::time::Duration::from_millis(10));
                wait += 1;
            }
        }
    }

    /// 排水入口，给「暂停后恢复」这一路径用。
    ///
    /// 轮次终局会自动排一次；但停止把队列挂起时那一轮已经结束了，此后没有
    /// 任何一轮会再经过终局——清掉暂停只让横幅消失，队头永远等不到下一轮。
    /// 宿主在解除暂停后调它，队列当场接着排。会话正忙或队列已空时它什么都不
    /// 做：该做的事下一轮终局会做。
    pub fn resume_queue(
        &self,
        session_id: Uuid,
        sink: Option<&EventSink>,
        emit: &mut dyn FnMut(ServerMessage),
    ) {
        self.drain_queue_after_run(session_id, sink, emit);
    }

    /// Produces one answer and closes the run it belongs to.
    ///
    /// The adapter, or with no adapter the stand-in, drives `responder`; every chunk
    /// is recorded before it is handed out, so a transcript a connection saw is a
    /// transcript the store can answer for. Runs on whichever thread owns the turn:
    /// the connection's for [`Self::handle`], the run thread for a subscriber.
    fn produce_run(
        &self,
        session_id: Uuid,
        control: &RunControl,
        prompt: &str,
        sink: Option<&EventSink>,
        emit: &mut dyn FnMut(ServerMessage),
    ) {
        // One chunk in, one chunk out: recorded before it is handed to the clients,
        // so a transcript a connection saw is a transcript the store can answer for.
        let started_ms = usage::now_ms();
        let started_at = Instant::now();
        let mut first_delta_at: Option<Instant> = None;
        // 回合开场：宿主据此画新用户行（运行中入队再自动排水的回合，宿主没有
        // 发过 Submit，只有这一事件能告诉它「这轮问的是这句」）。只读序号、
        // 不消耗序号——TextDelta 的 sequence 从 1 起的历史口径不能被开场白挪动。
        {
            let state = lock_or_recover(&self.state);
            let sequence = state
                .sessions
                .get(&session_id)
                .map(|session| session.sequence)
                .unwrap_or(0);
            self.broadcast_or_emit(
                ServerMessage::TurnStarted {
                    session_id,
                    prompt: prompt.to_string(),
                    sequence,
                },
                sink,
                emit,
            );
        }
        match self.adapter.as_ref() {
            Some(adapter) => {
                let options = self.run_options_for(session_id);
                let configured = adapter.default_model().unwrap_or("");
                let picked = options.model_or(configured);
                let model = (!picked.trim().is_empty()).then(|| picked.to_string());
                // 文本与进展共用一个投递闭包：两者都「先记进会话，再在锁外广播」，
                // 正在看的人和之后刷新的人读同一份状态，锁只包住记录与取出事件。
                let deliver = &mut |output: TurnOutput<'_>| -> bool {
                    let event = match output {
                        TurnOutput::Text(text) => {
                            if control.cancelled.load(Ordering::SeqCst) {
                                return false;
                            }
                            if first_delta_at.is_none() {
                                first_delta_at = Some(Instant::now());
                            }
                            let mut state = lock_or_recover(&self.state);
                            let Some(session) = state.sessions.get_mut(&session_id) else {
                                return false;
                            };
                            session.sequence += 1;
                            // 侧栏徽标的依据：这轮有新文本到达。是否算「已读」由
                            // 宿主在打开会话时用 MarkSessionSeen 决定。
                            session.unread_at = Some(usage::now_ms());
                            match session.messages.last_mut() {
                                Some(last) if last.role == "assistant" => last.text.push_str(text),
                                _ => session.messages.push(TimelineMessage {
                                    role: "assistant".into(),
                                    text: text.to_string(),
                                }),
                            }
                            Some(ServerMessage::TextDelta {
                                session_id,
                                text: text.to_string(),
                                sequence: session.sequence,
                            })
                        }
                        TurnOutput::Progress(RunProgress::ApiRetry {
                            attempt,
                            max_retries,
                            reason,
                        }) => {
                            let mut state = lock_or_recover(&self.state);
                            let Some(session) = state.sessions.get_mut(&session_id) else {
                                return true; // 会话没了：吞掉这条进展，别中断整轮
                            };
                            session.sequence += 1;
                            let retry = ApiRetryStatus {
                                attempt,
                                max_retries,
                                reason: reason.clone(),
                            };
                            session.api_retry = Some(retry.clone());
                            Some(ServerMessage::RetryUpdated {
                                session_id,
                                retry: Some(retry),
                                sequence: session.sequence,
                            })
                        }
                        TurnOutput::Thought(text) => {
                            if control.cancelled.load(Ordering::SeqCst) {
                                return false;
                            }
                            let mut state = lock_or_recover(&self.state);
                            let Some(session) = state.sessions.get_mut(&session_id) else {
                                return false;
                            };
                            session.sequence += 1;
                            Some(ServerMessage::ReasoningDelta {
                                session_id,
                                text: text.to_string(),
                                sequence: session.sequence,
                            })
                        }
                        TurnOutput::Subagent(mut subagent) => {
                            subagent.parent_session_id.get_or_insert_with(|| session_id.to_string());
                            self.update_subagent(session_id, subagent, sink, emit);
                            return true;
                        }
                        TurnOutput::Tool(tool) => {
                            if control.cancelled.load(Ordering::SeqCst) {
                                return false;
                            }
                            let mut state = lock_or_recover(&self.state);
                            let Some(session) = state.sessions.get_mut(&session_id) else {
                                return false;
                            };
                            session.sequence += 1;
                            let tool_call_id = Some(tool.tool_call_id.to_string());
                            let raw_input_opt = if !tool.raw_input.is_empty() {
                                Some(tool.raw_input.to_string())
                            } else {
                                None
                            };
                            let event = if tool.started {
                                if let Some(input_str) = &raw_input_opt {
                                    session
                                        .pending_tool_inputs
                                        .insert(tool.tool_call_id.to_string(), input_str.clone());
                                }
                                ServerMessage::ToolStarted {
                                    session_id,
                                    tool: tool.tool_name.to_string(),
                                    raw_input: raw_input_opt,
                                    tool_call_id,
                                    sequence: session.sequence,
                                }
                            } else if let Some(ok) = tool.completed {
                                let input_str = session
                                    .pending_tool_inputs
                                    .remove(tool.tool_call_id)
                                    .unwrap_or_default();
                                if ok {
                                    session.messages.push(TimelineMessage {
                                        role: "tool".into(),
                                        text: serde_json::json!({
                                            "tool": tool.tool_name,
                                            "input": input_str,
                                            "output": tool.output_text,
                                        })
                                        .to_string(),
                                    });
                                }
                                ServerMessage::ToolResult {
                                    session_id,
                                    tool: tool.tool_name.to_string(),
                                    result: tool.output_text.to_string(),
                                    succeeded: Some(ok),
                                    tool_call_id,
                                    sequence: session.sequence,
                                }
                            } else {
                                ServerMessage::ToolProgress {
                                    session_id,
                                    tool: tool.tool_name.to_string(),
                                    progress: tool.output_text.to_string(),
                                    tool_call_id,
                                    sequence: session.sequence,
                                }
                            };
                            Some(event)
                        }
                    };
                    match event {
                        Some(event) => {
                            self.broadcast_or_emit(event, sink, emit);
                            true
                        }
                        None => false,
                    }
                };
                let result = adapter.run_prompt_turn_progressing(prompt, &options, deliver);
                // responder 已经不再使用，此刻才能读它记录的首 token 时刻。
                let ttft_ms =
                    first_delta_at.map(|first| first.duration_since(started_at).as_millis() as u64);
                let duration_ms = started_at.elapsed().as_millis() as u64;
                match result {
                    Ok(report) => {
                        let reported = report.usage;
                        self.record_turn(
                            session_id,
                            usage::TurnUsage {
                                at_ms: started_ms,
                                model,
                                input_tokens: reported.as_ref().map_or(0, |u| u.input_tokens),
                                output_tokens: reported.as_ref().map_or(0, |u| u.output_tokens),
                                reasoning_tokens: reported
                                    .as_ref()
                                    .map_or(0, |u| u.reasoning_tokens),
                                cached_tokens: reported.as_ref().map_or(0, |u| u.cached_tokens),
                                cache_creation_tokens: reported
                                    .as_ref()
                                    .map_or(0, |u| u.cache_creation_tokens),
                                duration_ms: Some(duration_ms),
                                ttft_ms,
                                ok: true,
                            },
                        );
                        // 真实轮次的账目同时作为 Usage 事件发出去：客户端可以从会话快照
                        // 与事件中读取累计值。此前只有 tool-adapter 的伪造
                        // 分支会发 Usage，真实数字只躺在账本里，会话面板永远是 0。
                        if let Some(reported_usage) = reported.as_ref() {
                            let event = {
                                let mut state = lock_or_recover(&self.state);
                                state.sessions.get_mut(&session_id).map(|session| {
                                    session.sequence += 1;
                                    ServerMessage::Usage {
                                        session_id,
                                        input_tokens: reported_usage.input_tokens,
                                        output_tokens: reported_usage.output_tokens,
                                        cached_tokens: reported_usage.cached_tokens,
                                        cache_total_tokens: reported_usage.cache_total_tokens,
                                        cache_creation_tokens: reported_usage.cache_creation_tokens,
                                        duration_ms: Some(duration_ms),
                                        ttft_ms,
                                        sequence: session.sequence,
                                    }
                                })
                            };
                            if let Some(event) = event {
                                self.broadcast_or_emit(event, sink, emit);
                            }
                        }
                    }
                    Err(message) => {
                        self.record_turn(
                            session_id,
                            usage::TurnUsage {
                                at_ms: started_ms,
                                model,
                                duration_ms: Some(duration_ms),
                                ttft_ms,
                                ok: false,
                                ..usage::TurnUsage::default()
                            },
                        );
                        self.finish_run(session_id, control, sink, emit, Some(message));
                        return;
                    }
                }
            }
            None => {
                // 演示路径没有进展可报，走纯文本的投递闭包就够。
                let deliver = &mut |output: TurnOutput<'_>| -> bool {
                    let TurnOutput::Text(text) = output else {
                        return true;
                    };
                    if control.cancelled.load(Ordering::SeqCst) {
                        return false;
                    }
                    if first_delta_at.is_none() {
                        first_delta_at = Some(Instant::now());
                    }
                    let event = {
                        let mut state = lock_or_recover(&self.state);
                        let Some(session) = state.sessions.get_mut(&session_id) else {
                            return false;
                        };
                        session.sequence += 1;
                        // demo 路径同一份未读纪律（deliver 闭包与这里各有一份
                        // 记录逻辑，改一处必须改另一处）。
                        session.unread_at = Some(usage::now_ms());
                        match session.messages.last_mut() {
                            Some(last) if last.role == "assistant" => last.text.push_str(text),
                            _ => session.messages.push(TimelineMessage {
                                role: "assistant".into(),
                                text: text.to_string(),
                            }),
                        }
                        ServerMessage::TextDelta {
                            session_id,
                            text: text.to_string(),
                            sequence: session.sequence,
                        }
                    };
                    self.broadcast_or_emit(event, sink, emit);
                    true
                };
                for (index, chunk) in
                    bounded_text_chunks(&format!("演示响应：{prompt}"), DELTA_SIZE)
                        .into_iter()
                        .enumerate()
                {
                    // Waiting between chunks, and after the wait asking again, is
                    // what a real producer does; the run thread is off the socket
                    // loop, so this waits out its own turn and nothing else.
                    if index > 0 && !self.demo_chunk_gap.is_zero() {
                        std::thread::sleep(self.demo_chunk_gap);
                    }
                    if !deliver(TurnOutput::Text(&chunk)) {
                        break;
                    }
                }
                // 演示轮次也如实记账：有轮次发生，token 为 0。
                self.record_turn(
                    session_id,
                    usage::TurnUsage {
                        at_ms: started_ms,
                        duration_ms: Some(started_at.elapsed().as_millis() as u64),
                        ttft_ms: first_delta_at
                            .map(|first| first.duration_since(started_at).as_millis() as u64),
                        ok: true,
                        ..usage::TurnUsage::default()
                    },
                );
            }
        }
        self.finish_run(session_id, control, sink, emit, None);
    }

    /// 记录一轮的真实用量并落库。轮次记录不进时间线，只进用量账本。
    fn record_turn(&self, session_id: Uuid, turn: usage::TurnUsage) {
        let mut state_guard = lock_or_recover(&self.state);
        if let Some(session) = state_guard.sessions.get_mut(&session_id) {
            session.turns.push(turn);
            let excess = session
                .turns
                .len()
                .saturating_sub(usage::MAX_TRACKED_TURNS_PER_SESSION);
            session.turns.drain(0..excess);
        }
        // 落库失败不阻塞回复：finish_run 与后续事件还会再持久化。
        let _ = self.persist(&state_guard);
    }

    /// Closes a prompt run: unregisters it, then reports the terminal event that
    /// matches why it ended. `failure` is an adapter that gave up, which is neither
    /// a completion nor a cancellation, and the text produced before it gave up stays
    /// in the transcript because the client was shown that text.
    fn finish_run(
        &self,
        session_id: Uuid,
        control: &RunControl,
        sink: Option<&EventSink>,
        emit: &mut dyn FnMut(ServerMessage),
        failure: Option<String>,
    ) {
        if control.finished.swap(true, Ordering::SeqCst) {
            // The run already has an ending: a panic noticed after the producer
            // closed the turn must not put a second one in front of the client.
            return;
        }
        let mut state = lock_or_recover(&self.state);
        lock_or_recover(&self.runs).remove(&session_id);
        let Some(session) = state.sessions.get_mut(&session_id) else {
            return;
        };
        // 轮次无论怎么结束，「正在重试」都不再成立：终局事件就是客户端清掉
        // 重试提示的信号，会话上的状态必须与之一致。
        session.api_retry = None;
        let events = match failure {
            Some(message) => {
                if let Some(last) = session.messages.last_mut() {
                    if last.role == "assistant" && last.text.is_empty() {
                        last.text = format!("请求失败：{message}");
                    }
                }
                session.sequence += 1;
                session.record_preview();
                session.touch();
                vec![ServerMessage::Error {
                    code: "agent_failed".into(),
                    message,
                }]
            }
            None => {
                session.sequence += 1;
                session.record_preview();
                session.touch();
                let sequence = session.sequence;
                if control.cancelled.load(Ordering::SeqCst) {
                    vec![ServerMessage::Cancelled {
                        session_id,
                        sequence,
                    }]
                } else {
                    vec![ServerMessage::Completed {
                        session_id,
                        sequence,
                    }]
                }
            }
        };
        if let Err(error) = self.persist(&state) {
            // The chunks are already on screen. Reporting only the failed write would
            // leave a transcript nothing wrote down, with no indication that a reload
            // will not reproduce it, so both are said.
            drop(state);
            self.broadcast_or_emit(error, sink, emit);
            return;
        }
        drop(state);
        for event in events {
            self.broadcast_or_emit(event, sink, emit);
        }
    }
    fn run_message(&self, message: ClientMessage) -> Vec<ServerMessage> {
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
            | ClientMessage::ApproveRemembered { client_msg_id, .. }
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
            | ClientMessage::SuggestCommitMessage { client_msg_id, .. }
            | ClientMessage::ValidateAttachment { client_msg_id, .. }
            | ClientMessage::BeginAttachment { client_msg_id, .. }
            | ClientMessage::AttachmentChunk { client_msg_id, .. }
            | ClientMessage::CancelAttachment { client_msg_id, .. }
            | ClientMessage::FinalizeAttachment { client_msg_id, .. }
            | ClientMessage::ImportTuiSession { client_msg_id, .. }
            | ClientMessage::ValidateProvider { client_msg_id, .. }
            | ClientMessage::ListProviderModels { client_msg_id }
            | ClientMessage::ListModes { client_msg_id }
            | ClientMessage::SetMode { client_msg_id, .. }
            | ClientMessage::SetThoughtLevel { client_msg_id, .. }
            | ClientMessage::SetModel { client_msg_id, .. }
            | ClientMessage::ListWorkflows { client_msg_id, .. }
            | ClientMessage::ListSubagents { client_msg_id, .. }
            | ClientMessage::GetSessionTarget { client_msg_id, .. }
            | ClientMessage::SetSessionTarget { client_msg_id, .. }
            | ClientMessage::SetSessionTargetStatus { client_msg_id, .. }
            | ClientMessage::GetSessionDiffSummary { client_msg_id, .. }
            | ClientMessage::ListSessions { client_msg_id, .. }
            | ClientMessage::RenameSession { client_msg_id, .. }
            | ClientMessage::PinSession { client_msg_id, .. }
            | ClientMessage::MarkSessionSeen { client_msg_id, .. }
            | ClientMessage::QueueInput { client_msg_id, .. }
            | ClientMessage::EditQueueItem { client_msg_id, .. }
            | ClientMessage::DeleteQueueItem { client_msg_id, .. }
            | ClientMessage::ReorderQueueItem { client_msg_id, .. }
            | ClientMessage::SendQueuedNow { client_msg_id, .. }
            | ClientMessage::SetQueueAutoDrain { client_msg_id, .. }
            | ClientMessage::ArchiveSession { client_msg_id, .. }
            | ClientMessage::DeleteSession { client_msg_id, .. }
            | ClientMessage::ForkSession { client_msg_id, .. }
            | ClientMessage::ScanMarketplace { client_msg_id, .. }
            | ClientMessage::UsageStats { client_msg_id, .. } => client_msg_id.clone(),
        };
        // A conversation the CLI owns keeps its transcript on disk rather than in
        // this process, so opening one means reading that file. It is read before
        // the state lock is taken: a long conversation is megabytes of text, and
        // holding the engine's one lock across that read would stall every other
        // session's answer for the length of a disk read.
        let hydration = match &message {
            ClientMessage::Resume { session_id, .. } => {
                Self::transcript_on_disk(&self.state, *session_id)
            }
            _ => None,
        };
        let mut state = self.state.lock().expect("engine state lock");
        if !state.seen_client_messages.insert(client_msg_id.clone()) {
            return vec![ServerMessage::Ack { client_msg_id }];
        }
        if let Some((session_id, transcript)) = hydration
            && let Some(session) = state.sessions.get_mut(&session_id)
            // Only a session with nothing in it is filled. Once a turn has been
            // typed here the live transcript is the one that must be shown, and
            // re-reading would either duplicate that turn or throw it away.
            && session.messages.is_empty()
        {
            session.stored_message_count = session.stored_message_count.max(transcript.len());
            let read_something = !transcript.is_empty();
            session.messages = transcript;
            if read_something && session.last_preview.is_none() {
                session.record_preview();
            }
        }
        let result = match message {
            // ---- 运行中输入的队列（宿主队列合同）。全部幂等、只动队列，
            // 不开新一轮；真正的排水发生在轮次终局或 SendQueuedNow。
            ClientMessage::QueueInput {
                client_msg_id,
                session_id,
                text,
                source_command_id,
                requested_delivery,
                client_id,
            } => {
                let Some(session) = state.sessions.get_mut(&session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                session.queued_inputs.push(QueuedInput {
                    queue_item_id: Uuid::new_v4(),
                    source_command_id: source_command_id.clone(),
                    text: text.clone(),
                    created_at_ms: usage::now_ms(),
                    requested_delivery: requested_delivery.clone(),
                    client_id: client_id.clone(),
                });
                session.sequence += 1;
                let (items, auto_drain, paused) = session.queue_snapshot();
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::QueueUpdated {
                        session_id,
                        items,
                        auto_drain,
                        paused,
                        sequence: session.sequence,
                    },
                ]
            }
            ClientMessage::EditQueueItem {
                client_msg_id,
                session_id,
                queue_item_id,
                new_text,
            } => {
                let Some(session) = state.sessions.get_mut(&session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                match session
                    .queued_inputs
                    .iter_mut()
                    .find(|item| item.queue_item_id == queue_item_id)
                {
                    Some(item) => item.text = new_text,
                    None => return vec![Self::error("queue_item_not_found", "排队项不存在")],
                }
                session.sequence += 1;
                let (items, auto_drain, paused) = session.queue_snapshot();
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::QueueUpdated {
                        session_id,
                        items,
                        auto_drain,
                        paused,
                        sequence: session.sequence,
                    },
                ]
            }
            ClientMessage::DeleteQueueItem {
                client_msg_id,
                session_id,
                queue_item_id,
            } => {
                let Some(session) = state.sessions.get_mut(&session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                session
                    .queued_inputs
                    .retain(|item| item.queue_item_id != queue_item_id);
                session.sequence += 1;
                let (items, auto_drain, paused) = session.queue_snapshot();
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::QueueUpdated {
                        session_id,
                        items,
                        auto_drain,
                        paused,
                        sequence: session.sequence,
                    },
                ]
            }
            ClientMessage::ReorderQueueItem {
                client_msg_id,
                session_id,
                queue_item_id,
                before_queue_item_id,
            } => {
                let Some(session) = state.sessions.get_mut(&session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                let Some(position) = session
                    .queued_inputs
                    .iter()
                    .position(|item| item.queue_item_id == queue_item_id)
                else {
                    return vec![Self::error("queue_item_not_found", "排队项不存在")];
                };
                let item = session.queued_inputs.remove(position);
                let insert_at = match before_queue_item_id {
                    Some(before) => session
                        .queued_inputs
                        .iter()
                        .position(|other| other.queue_item_id == before)
                        .unwrap_or(session.queued_inputs.len()),
                    None => session.queued_inputs.len(),
                };
                session.queued_inputs.insert(insert_at, item);
                session.sequence += 1;
                let (items, auto_drain, paused) = session.queue_snapshot();
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::QueueUpdated {
                        session_id,
                        items,
                        auto_drain,
                        paused,
                        sequence: session.sequence,
                    },
                ]
            }
            ClientMessage::SendQueuedNow {
                client_msg_id,
                session_id,
                queue_item_id,
            } => {
                let Some(session) = state.sessions.get_mut(&session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                // 把指定项提到队头并解除暂停：正在运行的轮次结束后它就是下一
                // 条；空闲时它立刻成为下一轮。
                let Some(position) = session
                    .queued_inputs
                    .iter()
                    .position(|item| item.queue_item_id == queue_item_id)
                else {
                    return vec![Self::error("queue_item_not_found", "排队项不存在")];
                };
                let item = session.queued_inputs.remove(position);
                session.queued_inputs.insert(0, item);
                session.queue_paused = false;
                session.sequence += 1;
                let (items, auto_drain, paused) = session.queue_snapshot();
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::QueueUpdated {
                        session_id,
                        items,
                        auto_drain,
                        paused,
                        sequence: session.sequence,
                    },
                ]
            }
            ClientMessage::SetQueueAutoDrain {
                client_msg_id,
                session_id,
                auto_drain,
            } => {
                let Some(session) = state.sessions.get_mut(&session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                session.queue_auto_drain = auto_drain;
                if auto_drain {
                    session.queue_paused = false;
                }
                session.sequence += 1;
                let (items, auto_drain, paused) = session.queue_snapshot();
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::QueueUpdated {
                        session_id,
                        items,
                        auto_drain,
                        paused,
                        sequence: session.sequence,
                    },
                ]
            }
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
                        path: None,
                    },
                );
                state.active_workspace_id = Some(id);
                let session_id = Uuid::new_v4();
                state
                    .sessions
                    .insert(session_id, SessionState::fresh(Some(id)));
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
                                path: None,
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
                state
                    .sessions
                    .insert(id, SessionState::fresh(Some(workspace_id)));
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
                if let Some(workspace) = state.workspaces.get_mut(&workspace_id) {
                    workspace.last_used_sequence = workspace.last_used_sequence.saturating_add(1);
                }
                let session_id = state
                    .workspaces
                    .get(&workspace_id)
                    .and_then(|workspace| workspace.last_session_id);
                let mut events = vec![ServerMessage::WorkspaceSwitched { workspace_id }];
                let session_id = match session_id {
                    Some(session_id) => session_id,
                    None => {
                        let session_id = Uuid::new_v4();
                        state
                            .sessions
                            .insert(session_id, SessionState::fresh(Some(workspace_id)));
                        if let Some(workspace) = state.workspaces.get_mut(&workspace_id) {
                            workspace.last_session_id = Some(session_id);
                        }
                        session_id
                    }
                };
                if let Some(session) = state.sessions.get(&session_id) {
                    events.push(Self::session_snapshot(session_id, session));
                }
                let mut workspaces = state.workspaces.values().cloned().collect::<Vec<_>>();
                workspaces.sort_by_key(|workspace| std::cmp::Reverse(workspace.last_used_sequence));
                events.push(ServerMessage::Workspaces {
                    active_workspace_id: workspace_id,
                    workspaces,
                });
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
                                path: None,
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
                                    SessionState::fresh(Some(active_workspace_id)),
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
                        api_retry: session.api_retry.clone(),
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
            // A prompt is routed by `dispatch_to` before the state lock is taken,
            // because the adapter has to be able to run while `cancel` and every
            // other message stay answerable. Reaching this arm means that routing
            // was dropped, which would put a blocking call back inside the lock.
            ClientMessage::Submit { .. } => {
                vec![Self::error("internal_error", "submit 未经流式路径进入引擎")]
            }
            ClientMessage::Cancel {
                client_msg_id,
                session_id,
            } => {
                let Some(session) = state.sessions.get_mut(&session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                // A run still in flight takes the stop signal here and reports its
                // own terminal event once the adapter stops, so the transcript can
                // never read as over while chunks were still on their way. Nothing is
                // producing anything to stop -- the usual case for this synchronous
                // entry point -- then the stop is its own answer.
                let live = lock_or_recover(&self.runs).get(&session_id).cloned();
                if let Some(control) = &live {
                    // The only thing that actually stops a run. The adapter's next
                    // chunk is refused, it returns, and `finish_run` reports the
                    // terminal event -- so the stop is felt at the producer, not just
                    // written down beside it.
                    control.cancelled.store(true, Ordering::SeqCst);
                }
                let outcome = if live.is_some() {
                    "interrupted"
                } else {
                    "accepted"
                };
                // 用户按停止：打断当前轮，同时挂起队列的自动排水——按停止是
                // 要暂停，不是要连发下一条；SendQueuedNow / SetQueueAutoDrain
                // 显式恢复。暂停随事件说出去：不公告的话客户端照旧按
                // autoDrain 画队列，既不显示暂停横幅也没有恢复入口。
                let paused_queue = if live.is_some() && !session.queued_inputs.is_empty() {
                    session.queue_paused = true;
                    session.sequence += 1;
                    let (items, auto_drain, paused) = session.queue_snapshot();
                    Some(ServerMessage::QueueUpdated {
                        session_id,
                        items,
                        auto_drain,
                        paused,
                        sequence: session.sequence,
                    })
                } else {
                    None
                };
                session.sequence += 1;
                session.audit.push(AuditEntry {
                    action: "cancel".into(),
                    outcome: outcome.into(),
                    sequence: session.sequence,
                });
                let mut events = vec![ServerMessage::Ack { client_msg_id }];
                events.extend(paused_queue);
                if live.is_none() {
                    events.push(ServerMessage::Cancelled {
                        session_id,
                        sequence: session.sequence,
                    });
                }
                events.push(ServerMessage::Audit {
                    session_id,
                    action: "cancel".into(),
                    outcome: outcome.into(),
                    sequence: session.sequence,
                });
                events
            }
            ClientMessage::Approve {
                client_msg_id,
                request_id,
            } => self.resolve_approval(&mut state, client_msg_id, request_id, true, None, false),
            ClientMessage::ApproveRemembered {
                client_msg_id,
                request_id,
            } => self.resolve_approval(&mut state, client_msg_id, request_id, true, None, true),
            ClientMessage::Reject {
                client_msg_id,
                request_id,
                reason,
            } => self.resolve_approval(
                &mut state,
                client_msg_id,
                request_id,
                false,
                Some(reason),
                false,
            ),
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
                let options = self.run_options();
                let chunks = match adapter
                    .run_prompt(&commit_message_prompt(branch.as_deref(), &diff), &options)
                {
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
                let valid_url = crate::provider::endpoint_shape_allowed(&base_url);
                let valid_model = !model.trim().is_empty() && model.len() <= 200;
                // Shape first: a malformed target must not be contacted at all.
                let shape_error = if !valid_url {
                    Some("invalid_base_url")
                } else if !valid_model {
                    Some("invalid_model")
                } else {
                    None
                };
                let answer = |reachable: bool, error_code: Option<&str>| {
                    vec![ServerMessage::ProviderValidation {
                        base_url: base_url.clone(),
                        model: model.clone(),
                        reachable,
                        error_code: error_code.map(str::to_string),
                    }]
                };
                if let Some(code) = shape_error {
                    return answer(false, Some(code));
                }
                // The probe can only be about the endpoint this host actually
                // talks to: `base_url` came from a client, and sending the
                // host's credential to whatever a page asks for is not a
                // "validation" feature.
                let Some(adapter) = &self.adapter else {
                    return answer(false, Some("provider_not_configured"));
                };
                match adapter.base_url() {
                    None => return answer(false, Some("provider_without_endpoint")),
                    Some(configured)
                        if configured.trim_end_matches('/') != base_url.trim_end_matches('/') =>
                    {
                        return answer(false, Some("endpoint_not_host_configured"));
                    }
                    Some(_) => {}
                }
                let health = adapter.list_models(Some(model.as_str()));
                if !health.reachable {
                    return answer(false, Some("provider_unreachable"));
                }
                if !health.configured_model_known {
                    return answer(false, Some("model_not_listed"));
                }
                answer(true, None)
            }
            ClientMessage::ListProviderModels { .. } => {
                let selected = self.effective_model();
                let Some(adapter) = &self.adapter else {
                    return vec![ServerMessage::ProviderModels {
                        models: Vec::new(),
                        configured_model: selected,
                        reachable: false,
                        detail: Some("没有配置 Provider，模型名需手动填写".to_string()),
                    }];
                };
                let health = adapter.list_models(selected.as_deref());
                vec![ServerMessage::ProviderModels {
                    models: health.model_ids,
                    configured_model: selected,
                    reachable: health.reachable,
                    detail: health.detail,
                }]
            }
            ClientMessage::ListModes { client_msg_id } => {
                vec![ServerMessage::Ack { client_msg_id }, self.modes_message()]
            }
            ClientMessage::SetMode {
                client_msg_id,
                mode,
            } => {
                let valid_modes = ["build", "plan", "chat"];
                let trimmed = mode.trim();
                let mode_to_set = if valid_modes.contains(&trimmed) {
                    trimmed.to_string()
                } else {
                    "build".to_string()
                };
                lock_or_recover(&self.settings).mode = Some(mode_to_set);
                vec![ServerMessage::Ack { client_msg_id }, self.modes_message()]
            }
            ClientMessage::SetThoughtLevel {
                client_msg_id,
                thought_level,
            } => {
                // 与 `ReasoningEffort::from_str`（xai-grok-sampling-types）同域：这里收
                // 下的每个值都要能原样走到 provider 或 CLI 的 --reasoning-effort。
                // 以前的 4 值清单会把客户端（默认补全最后一档）发来的
                // "max" 静默改写成 "medium"——档位越界时宁可拒收也不偷改。
                let valid_levels = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];
                let trimmed = thought_level.trim();
                let level_to_set = if valid_levels.contains(&trimmed) {
                    trimmed.to_string()
                } else {
                    return vec![
                        ServerMessage::Ack { client_msg_id },
                        ServerMessage::Error {
                            code: "invalid_thought_level".into(),
                            message: format!(
                                "思考强度必须是 none/minimal/low/medium/high/xhigh/max 之一，收到 {trimmed:?}"
                            ),
                        },
                    ];
                };
                lock_or_recover(&self.settings).thought_level = Some(level_to_set);
                vec![ServerMessage::Ack { client_msg_id }, self.modes_message()]
            }
            ClientMessage::SetModel {
                client_msg_id,
                model,
            } => {
                // The UI's model picker feeds this; run_options() reads it for
                // every later run. An unknown id still lands in settings: the
                // provider's own validation (configured_model_known) surfaces
                // the mismatch at probe time, not silently here.
                let trimmed = model.trim();
                if trimmed.is_empty() {
                    lock_or_recover(&self.settings).model = None;
                } else {
                    lock_or_recover(&self.settings).model = Some(trimmed.to_string());
                }
                vec![ServerMessage::Ack { client_msg_id }]
            }
            ClientMessage::ListWorkflows {
                client_msg_id,
                session_id,
            } => {
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::WorkflowRuns {
                        session_id,
                        workflows: Vec::new(),
                    },
                ]
            }
            ClientMessage::ListSubagents {
                client_msg_id,
                session_id,
            } => {
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::Subagents {
                        session_id,
                        subagents: Vec::new(),
                    },
                ]
            }
            ClientMessage::GetSessionTarget {
                client_msg_id,
                session_id,
            } => {
                let target = state
                    .sessions
                    .get(&session_id)
                    .and_then(|s| s.target.clone());
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::SessionTarget { session_id, target },
                ]
            }
            ClientMessage::SetSessionTarget {
                client_msg_id,
                session_id,
                objective,
                token_budget,
            } => {
                let Some(session) = state.sessions.get_mut(&session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                let target_info = SessionTargetInfo {
                    objective,
                    status: "active".into(),
                    token_budget,
                    tokens_used: 0,
                    time_used_seconds: 0,
                };
                session.target = Some(target_info.clone());
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::SessionTarget {
                        session_id,
                        target: Some(target_info),
                    },
                ]
            }
            ClientMessage::SetSessionTargetStatus {
                client_msg_id,
                session_id,
                status,
            } => {
                let Some(session) = state.sessions.get_mut(&session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                match session.target.as_mut() {
                    Some(target) => target.status = status,
                    None => {
                        return vec![Self::error("target_not_found", "会话没有目标")];
                    }
                }
                let target = session.target.clone();
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::SessionTarget { session_id, target },
                ]
            }
            ClientMessage::GetSessionDiffSummary {
                client_msg_id,
                session_id,
            } => {
                let summary = self.diff_summary();
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::SessionDiffSummary {
                        session_id,
                        summary,
                    },
                ]
            }
            ClientMessage::ListSessions {
                client_msg_id,
                workspace_id,
            } => {
                let mut sessions: Vec<SessionInfo> = state
                    .sessions
                    .iter()
                    .filter(|(_, s)| match workspace_id {
                        Some(target_ws) => s.workspace_id == Some(target_ws),
                        None => true,
                    })
                    .filter_map(|(id, _)| session_info(&state, *id))
                    .collect();
                sessions.sort_by(|a, b| {
                    b.pinned
                        .cmp(&a.pinned)
                        // A conversation's wall clock first: `sequence` restarts
                        // with the process, so sorting by it alone put a
                        // conversation from last month above one from a minute
                        // ago after every restart. The sequence stays as the
                        // tie-break for sessions that carry no timestamp.
                        .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
                        .then_with(|| b.updated_at_sequence.cmp(&a.updated_at_sequence))
                });
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::Sessions {
                        workspace_id,
                        sessions,
                    },
                ]
            }
            ClientMessage::RenameSession {
                client_msg_id,
                session_id,
                title,
            } => {
                let Some(session) = state.sessions.get_mut(&session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                session.title = Some(title);
                session.sequence += 1;
                session.touch();
                let Some(info) = session_info(&state, session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::SessionUpdated { session: info },
                ]
            }
            ClientMessage::PinSession {
                client_msg_id,
                session_id,
                pinned,
            } => {
                let Some(session) = state.sessions.get_mut(&session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                session.pinned = pinned;
                session.sequence += 1;
                session.touch();
                let Some(info) = session_info(&state, session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::SessionUpdated { session: info },
                ]
            }
            ClientMessage::MarkSessionSeen {
                client_msg_id,
                session_id,
            } => {
                if let Some(session) = state.sessions.get_mut(&session_id) {
                    session.unread_at = None;
                }
                vec![ServerMessage::Ack { client_msg_id }]
            }
            ClientMessage::ArchiveSession {
                client_msg_id,
                session_id,
                archived,
            } => {
                let Some(session) = state.sessions.get_mut(&session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                session.archived = archived;
                session.sequence += 1;
                session.touch();
                let Some(info) = session_info(&state, session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::SessionUpdated { session: info },
                ]
            }
            ClientMessage::DeleteSession {
                client_msg_id,
                session_id,
            } => {
                let Some(removed) = state.sessions.remove(&session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                // A conversation the CLI owns is still on that disk, so deleting
                // it here is a statement about this listing, not about the file.
                // Without the tombstone the next rescan would put it back.
                if removed.imported.is_some() {
                    state.dismissed_imports.insert(session_id);
                }
                if let Some(ws_id) = removed.workspace_id {
                    let fallback_session_id = state
                        .sessions
                        .iter()
                        .find_map(|(id, s)| (s.workspace_id == Some(ws_id)).then_some(*id));
                    if let Some(ws) = state.workspaces.get_mut(&ws_id)
                        && ws.last_session_id == Some(session_id)
                    {
                        ws.last_session_id = fallback_session_id;
                    }
                }
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::SessionDeleted { session_id },
                ]
            }
            ClientMessage::ForkSession {
                client_msg_id,
                session_id,
            } => {
                let Some(original) = state.sessions.get(&session_id) else {
                    return vec![Self::error("session_not_found", "会话不存在")];
                };
                let new_session_id = Uuid::new_v4();
                let orig_path = workspace_path_of(&state, original.workspace_id);
                let orig_title = original.info(session_id, orig_path.as_deref()).title;
                let mut forked = original.clone();
                forked.title = Some(format!("{orig_title} (Fork)"));
                forked.sequence = 0;
                forked.pending_approval = None;
                forked.pending_question = None;
                forked.pending_file_writes.clear();
                forked.pending_attachment_moves.clear();
                if let Some(ws_id) = forked.workspace_id
                    && let Some(ws) = state.workspaces.get_mut(&ws_id)
                {
                    ws.last_session_id = Some(new_session_id);
                    ws.last_used_sequence = ws.last_used_sequence.saturating_add(1);
                }
                let forked_path = workspace_path_of(&state, forked.workspace_id);
                let info = forked.info(new_session_id, forked_path.as_deref());
                let snapshot = Self::session_snapshot(new_session_id, &forked);
                let ws_id = forked.workspace_id.unwrap_or_default();
                state.sessions.insert(new_session_id, forked);
                vec![
                    ServerMessage::Ack { client_msg_id },
                    ServerMessage::SessionCreated {
                        session_id: new_session_id,
                        workspace_id: ws_id,
                    },
                    snapshot,
                    ServerMessage::SessionUpdated { session: info },
                ]
            }
            ClientMessage::UsageStats {
                client_msg_id,
                range,
                time_zone,
            } => {
                // 聚合全程只读：直接借 `state`，不克隆账本，锁在分支结束时释放。
                let facts: Vec<usage::UsageFacts<'_>> = state
                    .sessions
                    .values()
                    .map(|session| usage::UsageFacts {
                        turns: &session.turns,
                        tools: &session.tool_stats,
                        tool_events: &session.tool_events,
                    })
                    .collect();
                let snapshot = usage::build_app_usage_snapshot(
                    &range,
                    time_zone.as_deref(),
                    usage::now_ms(),
                    &facts,
                );
                let snapshot_text =
                    serde_json::to_string(&snapshot).unwrap_or_else(|_| "{}".to_string());
                vec![
                    ServerMessage::Ack {
                        client_msg_id: client_msg_id.clone(),
                    },
                    ServerMessage::UsageStatsSnapshot {
                        client_msg_id,
                        snapshot: snapshot_text,
                    },
                ]
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
                let settings = lock_or_recover(&self.settings).clone();
                vec![ServerMessage::Settings {
                    base_url: settings.base_url,
                    model: settings.model,
                    has_api_key: settings.has_api_key,
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
                let mut settings = lock_or_recover(&self.settings);
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
                let line_count = contents.lines().count();
                let summary = format!(
                    "写入 {relative_path}（{line_count} 行，{} 字节）",
                    contents.len()
                );
                session.pending_approval = Some(PendingApproval {
                    request_id,
                    tool: "workspace.write_file".into(),
                    summary: summary.clone(),
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
                        summary,
                        sequence: session.sequence,
                    },
                ]
            }
        };
        if let Err(error) = self.persist(&state) {
            return vec![error];
        }
        result
    }

    fn resolve_approval(
        &self,
        state: &mut State,
        client_msg_id: String,
        request_id: Uuid,
        approved: bool,
        reason: Option<String>,
        remember: bool,
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
        // 先记下是哪类执行，后面 outcome 匹配会把这些元组按值拆走。
        let is_file_write_outcome = pending_file_write.is_some();
        let is_attachment_move_outcome = pending_attachment_move.is_some();
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
                            raw_input: None,
                            tool_call_id: None,
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
                            // The client's diff tab reads this: without it the
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
                raw_input: None,
                tool_call_id: None,
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
                                tool_call_id: None,
                                sequence: session.sequence,
                            });
                            session.sequence += 1;
                            events.push(ServerMessage::ToolResult {
                                session_id,
                                tool: pending.tool.clone(),
                                result,
                                succeeded: None,
                                tool_call_id: None,
                                sequence: session.sequence,
                            });
                            session.sequence += 1;
                            events.push(ServerMessage::Usage {
                                session_id,
                                input_tokens: pending.summary.len() as u64,
                                output_tokens: 1,
                                cached_tokens: 0,
                                cache_total_tokens: pending.summary.len() as u64,
                                cache_creation_tokens: 0,
                                duration_ms: None,
                                ttft_ms: None,
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
        // 工具账目：真正执行过（成功或失败）才计数；用户拒绝与"未配置 adapter"
        // 不算工具调用。次数进无时间戳的累计表，事件（带时间戳）供按天热力图。
        // 持久化推迟到 `session` 借用结束后统一做。
        if matches!(outcome, "executed" | "failed") {
            let tool_name = if is_file_write_outcome {
                "workspace.write_file".to_string()
            } else if is_attachment_move_outcome {
                "attachment.save".to_string()
            } else {
                pending.tool.clone()
            };
            let stat = session.tool_stats.entry(tool_name.clone()).or_default();
            stat.calls += 1;
            if outcome == "failed" {
                stat.errors += 1;
            }
            session.tool_events.push(usage::ToolEvent {
                at_ms: usage::now_ms(),
                tool: tool_name,
                ok: outcome == "executed",
            });
            let excess = session
                .tool_events
                .len()
                .saturating_sub(usage::MAX_TOOL_EVENTS_PER_SESSION);
            session.tool_events.drain(0..excess);
        }
        if approved && remember {
            session.remembered_permissions.insert(pending.tool.clone());
        }
        if !approved && let Some(r) = &reason {
            tracing::info!(%session_id, %request_id, reason = %r, "工具审批已被用户拒绝");
        }
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
        // 到这里 `session` 借用已结束；把工具执行的账目与审批结果一并落库。
        if (is_file_write_outcome
            || is_attachment_move_outcome
            || matches!(outcome, "executed" | "failed"))
            && let Err(error) = self.persist(state)
        {
            events.push(error);
        }
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
            api_retry: session.api_retry.clone(),
        }
    }

    fn import_tui_session(root: &Path, session_id: &str) -> Vec<ServerMessage> {
        // The caller has named a directory *inside* a root this process was told
        // to allow. A name carrying a separator is not a name, it is a way out of
        // that root, and the allowlist is the only reason the file may be read.
        let names_one_directory = !session_id.is_empty()
            && session_id != "."
            && session_id != ".."
            && !session_id.contains('/')
            && !session_id.contains('\\')
            && !Path::new(session_id).is_absolute();
        if !names_one_directory {
            return vec![Self::error(
                "tui_session_invalid",
                "TUI 会话名不是单一目录名，已拒绝在配置的根目录之外读取",
            )];
        }
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
        // A refusal is not a resolution: `diff_resolved` clears the client's
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

    /// 一个先报 API 重试、再等测试放行、最后交付文本的 adapter：把「轮次进行中」
    /// 这个时刻钉住，才能验证进行中的会话状态与快照。`Receiver` 不是 Sync，用
    /// Mutex 包一层以满足 `PromptAdapter: Send + Sync`。
    struct BlockingRetryAdapter {
        started_tx: std::sync::mpsc::Sender<()>,
        proceed: Mutex<std::sync::mpsc::Receiver<()>>,
    }

    impl PromptAdapter for BlockingRetryAdapter {
        fn run_prompt(&self, prompt: &str, _options: &RunOptions) -> Result<Vec<String>, String> {
            Ok(vec![format!("真实 adapter: {prompt}")])
        }

        fn run_prompt_turn_progressing(
            &self,
            _prompt: &str,
            _options: &RunOptions,
            deliver: &mut dyn FnMut(TurnOutput<'_>) -> bool,
        ) -> Result<TurnReport, String> {
            let _ = deliver(TurnOutput::Progress(RunProgress::ApiRetry {
                attempt: 2,
                max_retries: 8,
                reason: "API error (status 503): no channel".to_string(),
            }));
            let _ = self.started_tx.send(());
            let _ = self.proceed.lock().unwrap().recv();
            let _ = deliver(TurnOutput::Text("done"));
            Ok(TurnReport::default())
        }
    }

    #[test]
    fn api_retry_progress_reaches_clients_and_snapshot_then_clears() {
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (proceed_tx, proceed_rx) = std::sync::mpsc::channel();
        let engine = Engine::with_adapter(BlockingRetryAdapter {
            started_tx,
            proceed: Mutex::new(proceed_rx),
        });
        let id = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "retry-create".into(),
            workspace_id: None,
        })[0]
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
            _ => panic!(),
        };
        std::thread::scope(|scope| {
            // handle 会阻塞整轮（无订阅者时 run 内联在调用线程上跑），放到独立
            // 线程，主线程才能在轮次中途看会话状态。
            let submit = scope.spawn(|| {
                engine.handle(ClientMessage::Submit {
                    client_msg_id: "retry-submit".into(),
                    session_id: id,
                    prompt: "会重试吗".into(),
                })
            });
            started_rx.recv().unwrap();
            // 中途：会话状态说「第 2/8 次重试」，刷新用的快照也如实带上。
            let mid = engine.session_api_retry(id).expect("mid-turn retry state");
            assert_eq!(
                (mid.attempt, mid.max_retries),
                (2, 8),
                "mid-turn retry state"
            );
            let resumed = engine.handle(ClientMessage::Resume {
                client_msg_id: "retry-resume".into(),
                session_id: id,
                workspace_id: None,
            });
            assert!(matches!(
                &resumed[0],
                ServerMessage::SessionSnapshot { api_retry: Some(retry), .. } if retry.attempt == 2
            ));
            proceed_tx.send(()).unwrap();
            let events = submit.join().unwrap();
            assert!(
                events
                    .iter()
                    .any(|e| matches!(e, ServerMessage::RetryUpdated { retry: Some(retry), .. } if retry.attempt == 2 && retry.max_retries == 8))
            );
            // 终局之后：状态清空，快照不再带重试。
            assert!(engine.session_api_retry(id).is_none());
            let after = engine.handle(ClientMessage::Resume {
                client_msg_id: "retry-resume-2".into(),
                session_id: id,
                workspace_id: None,
            });
            assert!(matches!(
                &after[0],
                ServerMessage::SessionSnapshot {
                    api_retry: None,
                    ..
                }
            ));
        });
    }

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

    /// 排水口只在空闲会话上下手。恢复（`resume_queue`）可能和一个刚开起来的新轮
    /// 撞上：那时把队头取下来交给 run_submit 只会被 session_busy 退掉，那句话
    /// 从此既不在队列里，也不在时间线上。跑着的会话上恢复必须原样留着队头，
    /// 等这一轮终局自己接着排。
    #[test]
    fn a_resume_while_a_turn_runs_leaves_the_queued_item_in_place() {
        let engine = Engine::new().with_demo_pacing(std::time::Duration::from_millis(20));
        let id = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "c".into(),
            workspace_id: None,
        })[0]
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
            _ => panic!("CreateSession 先给出会话"),
        };
        let runner = engine.clone();
        let turn = std::thread::spawn(move || {
            runner.handle(ClientMessage::Submit {
                client_msg_id: "slow".into(),
                session_id: id,
                prompt: "请把下面这段话慢慢复述一遍。".repeat(40),
            });
        });
        for _ in 0..300 {
            if engine.session_is_running(id) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(engine.session_is_running(id), "演示轮次要真的跑起来");

        engine.handle(ClientMessage::QueueInput {
            client_msg_id: "queued".into(),
            session_id: id,
            text: "排队的问题".into(),
            source_command_id: "cmd".into(),
            requested_delivery: "queue".into(),
            client_id: "test".into(),
        });
        engine.resume_queue(id, None, &mut |_event| {});
        let (items, _, _) = engine.session_queue(id);
        assert_eq!(items.len(), 1, "跑着的会话上排水不许动队头：{items:?}");

        turn.join().unwrap();
        let (items, _, _) = engine.session_queue(id);
        assert!(
            items.is_empty(),
            "这一轮终局后队头自己排成下一轮，队列该是空的：{items:?}"
        );
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
        fn run_prompt(&self, prompt: &str, _options: &RunOptions) -> Result<Vec<String>, String> {
            Ok(vec![format!("真实 adapter: {prompt}")])
        }
    }

    #[cfg(unix)]
    #[test]
    fn headless_process_adapter_reads_real_json_entrypoint() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("chaos-fixture");
        // The stand-in binary records the argv it was given, because "the JSON was
        // parsed" says nothing about whether the model the user picked arrived.
        let argv_file = dir.path().join("argv.txt");
        std::fs::write(
            &binary,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'.tmp\nmv '{}'.tmp '{}'\nprintf '{{\"text\":\"process response\"}}\\n'\n",
                argv_file.display(),
                argv_file.display(),
                argv_file.display(),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        let adapter = HeadlessProcessAdapter::new(&binary, dir.path());
        assert_eq!(
            adapter
                .run_prompt("hello", &RunOptions::default())
                .unwrap()
                .concat(),
            "process response"
        );
        let unasked = std::fs::read_to_string(&argv_file).unwrap();
        assert!(
            unasked.contains("--single"),
            "the stand-in never saw the headless flags: {unasked}"
        );
        assert!(
            !unasked.contains("--model"),
            "a run nobody picked a model for must not pass one: {unasked}"
        );
        adapter
            .run_prompt(
                "hello again",
                &RunOptions {
                    model: Some("picked-model".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        let dump = std::fs::read_to_string(&argv_file).unwrap();
        let args: Vec<&str> = dump.lines().collect();
        let flag = args
            .iter()
            .position(|arg| *arg == "--model")
            .unwrap_or_else(|| panic!("the picked model never reached the child: {args:?}"));
        assert_eq!(
            args.get(flag + 1).copied(),
            Some("picked-model"),
            "{args:?}"
        );
        assert_eq!(
            args.last().copied(),
            Some("hello again"),
            "the prompt has to stay the trailing positional argument: {args:?}"
        );
    }

    /// 界面上选的思考档位必须像 `--model` 一样到达子进程，否则选择器只是给
    /// 胶囊换了个字，CLI 依旧用自己的默认推理强度。没选的时候不能凭空传一个。
    #[test]
    fn a_picked_thought_level_reaches_the_headless_child_as_reasoning_effort() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("chaos-argv-fixture");
        let argv_file = dir.path().join("argv.txt");
        std::fs::write(
            &binary,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'.tmp\nmv '{}'.tmp '{}'\nprintf '{{\"text\":\"ok\"}}\\n'\n",
                argv_file.display(),
                argv_file.display(),
                argv_file.display(),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        let adapter = HeadlessProcessAdapter::new(&binary, dir.path());

        adapter
            .run_prompt(
                "hello",
                &RunOptions {
                    thought_level: Some("max".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        let dump = std::fs::read_to_string(&argv_file).unwrap();
        let args: Vec<&str> = dump.lines().collect();
        let effort = args
            .iter()
            .position(|arg| *arg == "--reasoning-effort")
            .unwrap_or_else(|| panic!("选的思考档位没传到子进程：{args:?}"));
        assert_eq!(args.get(effort + 1).copied(), Some("max"), "{args:?}");
        // 与 `--single` 的次序约定一致：选项在前，提示词永远最后。
        let single = args
            .iter()
            .position(|arg| *arg == "--single")
            .unwrap_or_else(|| panic!("headless 标志没传出去：{args:?}"));
        assert!(effort < single, "档位必须排在 --single 之前：{args:?}");

        adapter
            .run_prompt(
                "hello",
                &RunOptions {
                    thought_level: Some("  ".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        let unasked = std::fs::read_to_string(&argv_file).unwrap();
        assert!(
            !unasked.contains("--reasoning-effort"),
            "没人选档位时不得凭空传一个：{unasked}"
        );
    }

    /// 流式 headless 轮次：CLI 在 stderr 上报的 `{"type":"api_retry",…}` 行
    /// 变成 `TurnOutput::Progress`，stdout 的结果对象照旧解析成文本。
    #[cfg(unix)]
    #[test]
    fn streaming_headless_turn_reports_stderr_api_retry_and_still_delivers_text() {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::Mutex as StdMutex;
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("chaos-fixture");
        std::fs::write(
            &binary,
            // 先在 stderr 上报一次重试，再给出最终结果：真实 CLI 在 503 重试与
            // 退出之间的时序与此一致（诊断先于结果对象）。
            "#!/bin/sh\n\
             printf '%s\\n' '{\"type\":\"api_retry\",\"state\":{\"type\":\"retrying\",\"attempt\":2,\"max_retries\":8,\"reason\":\"API error (status 503): no channel\",\"error_type\":\"api\"}}' >&2\n\
             sleep 0.05\n\
             printf '%s\\n' '{\"type\":\"text\",\"data\":\"process \"}'\n\
             printf '%s\\n' '{\"type\":\"text\",\"data\":\"response\"}'\n\
             printf '%s\\n' '{\"type\":\"usage\",\"usage\":{\"inputTokens\":120,\"outputTokens\":45,\"reasoningTokens\":8}}'\n\
             printf '%s\\n' '{\"type\":\"end\",\"stopReason\":\"end_turn\",\"sessionId\":\"s\",\"requestId\":\"r\"}'\n",
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        let adapter = HeadlessProcessAdapter::new(&binary, dir.path());
        let progress: StdMutex<Vec<RunProgress>> = StdMutex::new(Vec::new());
        let mut text = String::new();
        let mut thought = String::new();
        let report =
            adapter
                .run_prompt_turn_progressing("hello", &RunOptions::default(), &mut |output| {
                    match output {
                        TurnOutput::Text(chunk) => {
                            text.push_str(chunk);
                            true
                        }
                        TurnOutput::Thought(chunk) => {
                            thought.push_str(chunk);
                            true
                        }
                        TurnOutput::Tool(_) | TurnOutput::Subagent(_) => true,
                        TurnOutput::Progress(p) => {
                            progress.lock().unwrap().push(p);
                            true
                        }
                    }
                })
                .unwrap();
        // 真实账目从 usage 行进 TurnReport，不再恒 None。
        assert_eq!(
            report.usage,
            Some(crate::ProviderUsage {
                input_tokens: 120,
                output_tokens: 45,
                reasoning_tokens: 8,
                cached_tokens: 0,
                cache_creation_tokens: 0,
                cache_total_tokens: 120,
            })
        );
        assert!(thought.is_empty(), "text 行不算思考流：{thought}");
        assert_eq!(text, "process response");
        let seen = progress.into_inner().unwrap();
        assert_eq!(
            seen,
            vec![RunProgress::ApiRetry {
                attempt: 2,
                max_retries: 8,
                reason: "API error (status 503): no channel".to_string(),
            }]
        );
    }

    /// ADR-008 验收 1：`tool_call`/`tool_call_update` 行变成带 CLI 真实
    /// toolCallId 的工具卡事件，终态与输出文本一起到。
    #[cfg(unix)]
    #[test]
    fn streaming_headless_turn_delivers_tool_cards_with_real_ids() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("chaos-fixture");
        std::fs::write(
            &binary,
            "#!/bin/sh\n\
             printf '%s\\n' '{\"type\":\"tool_call\",\"toolCallId\":\"call_x1\",\"toolName\":\"run_terminal_command\",\"rawInput\":{\"command\":\"ss -ltn\"}}'\n\
             printf '%s\\n' '{\"type\":\"tool_call_update\",\"toolCallId\":\"call_x1\",\"status\":\"failed\",\"content\":[{\"type\":\"content\",\"content\":{\"type\":\"text\",\"text\":\"Tool execution failed: task_ids is empty\"}}]}'\n\
             printf '%s\\n' '{\"type\":\"end\",\"stopReason\":\"end_turn\",\"sessionId\":\"s\",\"requestId\":\"r\"}'\n",
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        let adapter = HeadlessProcessAdapter::new(&binary, dir.path());
        // 借用与线程边界：TurnTool 借的是闭包局部的字符串，先把要断言的字段抄出来。
        let cards: std::sync::Mutex<Vec<(bool, String, String, Option<bool>, String)>> =
            std::sync::Mutex::new(Vec::new());
        adapter
            .run_prompt_turn_progressing("hello", &RunOptions::default(), &mut |output| {
                if let TurnOutput::Tool(tool) = output {
                    cards.lock().unwrap().push((
                        tool.started,
                        tool.tool_call_id.to_string(),
                        tool.tool_name.to_string(),
                        tool.completed,
                        tool.output_text.to_string(),
                    ));
                }
                true
            })
            .unwrap();
        let cards = cards.into_inner().unwrap();
        assert_eq!(cards.len(), 2);
        assert_eq!(
            cards[0],
            (
                true,
                "call_x1".into(),
                "run_terminal_command".into(),
                None,
                String::new()
            )
        );
        assert_eq!(
            cards[1],
            (
                false,
                "call_x1".into(),
                "run_terminal_command".into(),
                Some(false),
                "Tool execution failed: task_ids is empty".into()
            )
        );
    }

    /// ADR-008 验收 2：宿主停止听（取消）之后，子进程被杀掉、轮次即刻以
    /// Ok 返回——阻塞路径做不到的「跑到一半真停」。
    #[cfg(unix)]
    #[test]
    fn a_cancelled_streaming_headless_turn_kills_the_child_and_returns() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("chaos-fixture");
        std::fs::write(
            &binary,
            // 十秒的存活窗口：取消若生效，测试在毫秒级返回。
            "#!/bin/sh\n\
             for i in 1 2 3 4 5 6 7 8 9 10; do printf '%s\\n' '{\"type\":\"text\",\"data\":\"tick\"}'; sleep 1; done\n",
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        let adapter = HeadlessProcessAdapter::new(&binary, dir.path());
        let started = std::time::Instant::now();
        let chunks = std::sync::Mutex::new(0usize);
        let report = adapter
            .run_prompt_turn_progressing("hello", &RunOptions::default(), &mut |output| {
                if matches!(output, TurnOutput::Text(_)) {
                    let mut count = chunks.lock().unwrap();
                    *count += 1;
                    // 第一条文本后停止听：模拟 RunControl.cancelled。
                    return *count < 1;
                }
                true
            })
            .unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "取消必须即刻生效"
        );
        assert!(report.usage.is_none(), "被取消的轮次没有完整账目");
    }

    /// ADR-008 真实端到端（手动）：真 CLI + 真 provider 的 streaming-json 轮次。
    /// 验证三件只有真进程能证明的事：text 增量真的逐块到达（假流式已死）、
    /// usage 行（snake_case 字段）进 TurnReport、工具卡带真实 id。
    /// `cargo test -p chaos-engine --lib a_real_cli_streaming_turn -- --ignored --nocapture`
    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "真网络 + 真认证：只在操作者本机手动跑"]
    async fn a_real_cli_streaming_turn_streams_usage_and_tools() {
        let binary = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../target/debug/chaos");
        if !binary.exists() {
            eprintln!("chaos 二进制不存在，跳过");
            return;
        }
        let adapter = HeadlessProcessAdapter::new(&binary, &std::env::temp_dir());
        let mut text = String::new();
        let mut thought = String::new();
        let mut tool_ids: Vec<String> = Vec::new();
        let report = adapter
            .run_prompt_turn_progressing(
                "只回答一个字：好",
                &RunOptions::default(),
                &mut |output| match output {
                    TurnOutput::Text(chunk) => {
                        text.push_str(chunk);
                        true
                    }
                    TurnOutput::Thought(chunk) => {
                        thought.push_str(chunk);
                        true
                    }
                    TurnOutput::Tool(tool) => {
                        tool_ids.push(tool.tool_call_id.to_string());
                        true
                    }
                    TurnOutput::Progress(_) | TurnOutput::Subagent(_) => true,
                },
            )
            .expect("真实流式轮次应成功");
        eprintln!(
            "text={text:?} thought_len={} tools={tool_ids:?} usage={:?}",
            thought.len(),
            report.usage
        );
        assert!(!text.is_empty(), "流式文本必须真的到达");
        let usage = report.usage.expect("真实轮次必须带 usage 行的账目");
        assert!(usage.input_tokens > 0, "真实账目不可能是零");
    }

    /// `--single` 的**取值**就是提示词本身，不是「一个开关加一个位置参数」。
    /// 于是 `--single --resume <id> <prompt>` 会被真实 CLI 直接拒绝：
    /// 「a value is required for '--single <PROMPT>'」——这是接入 `--resume`
    /// 时踩到的第一个坑，界面那边只看得见「回答没回来」。
    #[test]
    fn a_resumed_headless_run_puts_resume_before_single_and_runs_in_the_conversations_directory() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join("chaos-fixture");
        let record = root.path().join("record.txt");
        let somewhere = root.path().join("project-a");
        std::fs::create_dir_all(&somewhere).unwrap();
        std::fs::write(
            &binary,
            format!(
                "#!/bin/sh\n{{ echo \"cwd=$PWD\"; for a in \"$@\"; do echo \"arg=$a\"; done; }} > '{}'\nprintf '{{\"text\":\"ok\"}}\\n'\n",
                record.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        // The host process was started somewhere that is *not* the conversation's
        // directory; that difference is the whole point of the assertion.
        let adapter = HeadlessProcessAdapter::new(&binary, root.path());
        adapter
            .run_prompt(
                "现在端口通了吗",
                &RunOptions {
                    resume_session: Some("01a11392-1bd4-7f82-ab78-df12c9f1028c".into()),
                    working_dir: Some(somewhere.clone()),
                    ..RunOptions::default()
                },
            )
            .unwrap();
        let dump = std::fs::read_to_string(&record).unwrap();
        let args: Vec<&str> = dump
            .lines()
            .filter_map(|l| l.strip_prefix("arg="))
            .collect();
        let resume = args
            .iter()
            .position(|arg| *arg == "--resume")
            .unwrap_or_else(|| panic!("续聊的会话 id 没传到子进程：{args:?}"));
        assert_eq!(
            args.get(resume + 1).copied(),
            Some("01a11392-1bd4-7f82-ab78-df12c9f1028c"),
            "{args:?}"
        );
        let single = args
            .iter()
            .position(|arg| *arg == "--single")
            .unwrap_or_else(|| panic!("headless 标志没传出去：{args:?}"));
        assert!(
            resume < single,
            "`--resume` 必须排在 `--single` 之前，否则提示词会被当成 `--single` 之前的悬空值：{args:?}"
        );
        assert_eq!(
            args.get(single + 1).copied(),
            Some("现在端口通了吗"),
            "`--single` 的值就是这一轮的提示词：{args:?}"
        );
        assert_eq!(args.len(), single + 2, "提示词是最后一个参数：{args:?}");
        assert!(
            dump.contains(&format!("cwd={}", somewhere.display())),
            "回答要发生在对话自己的目录里：{dump}"
        );

        // 目录不存在（被删了、或者在另一台机器上列出来的）时退回宿主目录，
        // 而不是让子进程起不来。
        adapter
            .run_prompt(
                "还在吗",
                &RunOptions {
                    working_dir: Some(root.path().join("gone")),
                    ..RunOptions::default()
                },
            )
            .unwrap();
        let dump = std::fs::read_to_string(&record).unwrap();
        assert!(
            dump.contains(&format!("cwd={}", root.path().display())),
            "目录没了就退回宿主目录：{dump}"
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
        // A well-formed URL with no Provider configured cannot be probed either;
        // saying "not configured" is only honest if nothing was contacted, which
        // is what `the_validation_request_never_contacts_a_client_supplied_url`
        // holds to against a listening endpoint.
        assert!(
            matches!(&valid_shape[0], ServerMessage::ProviderValidation { reachable: false, error_code: Some(code), .. } if code == "provider_not_configured")
        );
    }

    /// A host without a Provider has no catalog to list, which is a different
    /// answer from "the Provider serves no models" and from "the Provider is down":
    /// the client draws a plain text field for the first and an error for the others.
    #[test]
    fn a_host_without_a_provider_reports_no_model_catalog() {
        let engine = Engine::new();
        let listed = engine.handle(ClientMessage::ListProviderModels {
            client_msg_id: "models".into(),
        });
        assert!(
            matches!(
                &listed[0],
                ServerMessage::ProviderModels { models, reachable: false, detail: Some(_), .. }
                    if models.is_empty()
            ),
            "{listed:?}"
        );
    }

    /// A handler that dies holding the settings lock must not take the next turn --
    /// or the settings panel -- down with it. What sits behind that lock is a plain
    /// snapshot of two strings and a flag, so reading the value back is strictly
    /// better than a second panic. The three sites below are the only readers of that
    /// lock, and all three took it with `.expect(..)` before this behaviour existed.
    #[test]
    fn a_poisoned_settings_lock_still_answers_the_run_and_the_settings_panel() {
        let engine = Engine::new();
        let died = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _held = engine.settings.lock().unwrap();
            panic!("a handler dies holding the settings lock");
        }));
        assert!(died.is_err(), "the fixture has to poison the lock first");

        engine.handle(ClientMessage::UpdateSettings {
            client_msg_id: "after-poison".into(),
            base_url: None,
            model: Some("still-asked-model".into()),
        });
        assert_eq!(engine.picked_model().as_deref(), Some("still-asked-model"));
        let echoed = engine.handle(ClientMessage::GetSettings {
            client_msg_id: "echo".into(),
        });
        assert!(
            matches!(
                &echoed[0],
                ServerMessage::Settings { model: Some(model), .. } if model == "still-asked-model"
            ),
            "{echoed:?}"
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
        /// The model each run was asked to use. A run that dropped the settings
        /// panel's pick would look identical from the transcript, so the only way
        /// to hold the engine to forwarding it is to record what arrived.
        models: Mutex<Vec<Option<String>>>,
    }

    impl PromptAdapter for RecordingPromptAdapter {
        fn run_prompt(&self, prompt: &str, options: &RunOptions) -> Result<Vec<String>, String> {
            self.seen.lock().unwrap().push(prompt.to_string());
            self.models.lock().unwrap().push(options.model.clone());
            Ok(vec![self.reply.to_string()])
        }
    }

    /// A host configured for a workspace root with its git and prompt adapters.
    fn engine_for_repo(root: &Path, reply: &'static str) -> (Engine, Arc<RecordingPromptAdapter>) {
        let recorder = Arc::new(RecordingPromptAdapter {
            reply,
            seen: Mutex::new(Vec::new()),
            models: Mutex::new(Vec::new()),
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
            fn run_prompt(
                &self,
                _prompt: &str,
                _options: &RunOptions,
            ) -> Result<Vec<String>, String> {
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
        // A host built without a Git adapter cannot read the staged area at all,
        // and must say which half is missing instead of
        // asking the Provider about an empty diff.
        let (directory, head) = repo_with_staged_edit("two\n");
        let recorder = Arc::new(RecordingPromptAdapter {
            reply: "不该被调用",
            seen: Mutex::new(Vec::new()),
            models: Mutex::new(Vec::new()),
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
        // hand the client a second answer for the same request.
        assert!(
            matches!(replay.as_slice(), [ServerMessage::Ack { .. }]),
            "{replay:?}"
        );
        assert_eq!(recorder.seen.lock().unwrap().len(), 1);
    }

    /// A model saved in the settings panel has to be the model the next run asks
    /// for. This path looked entirely right on screen while the pick changed
    /// nothing but a label, so the assertion is on what the adapter was handed.
    #[test]
    fn a_model_saved_in_settings_is_the_model_the_next_run_asks_for() {
        let (directory, _) = repo_with_staged_edit("two\n");
        let (engine, recorder) = engine_for_repo(directory.path(), "补充说明");
        let session_id = new_session(&engine, "model-pick-session");
        let suggest = |engine: &Engine, id: &str| {
            engine.handle(ClientMessage::SuggestCommitMessage {
                client_msg_id: id.into(),
                session_id,
            })
        };
        suggest(&engine, "suggest-before-pick");
        engine.handle(ClientMessage::UpdateSettings {
            client_msg_id: "pick-a-model".into(),
            base_url: None,
            model: Some("picked-model".into()),
        });
        suggest(&engine, "suggest-after-pick");
        // A blank field is "no choice", not a model named "   ".
        engine.handle(ClientMessage::UpdateSettings {
            client_msg_id: "clear-the-model".into(),
            base_url: None,
            model: Some("   ".into()),
        });
        suggest(&engine, "suggest-after-blank");
        assert_eq!(
            *recorder.models.lock().unwrap(),
            vec![None, Some("picked-model".to_string()), None],
            "the pick was not forwarded, or a blank field was forwarded as a model"
        );
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
                assert!(*truncated, "the client was told the whole diff was read")
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

    #[test]
    fn file_write_approval_summary_carries_path_and_lines_and_custom_reject_reason() {
        let engine = Engine::new();
        let session_id = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "create".into(),
            workspace_id: None,
        })[0]
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
            _ => panic!("failed to create session"),
        };
        let events = engine.handle(ClientMessage::ProposeFileWrite {
            client_msg_id: "write-proposal".into(),
            session_id,
            relative_path: "src/config.rs".into(),
            contents: "line 1\nline 2\nline 3\n".into(),
        });
        let (request_id, summary) = match &events[1] {
            ServerMessage::ToolApprovalRequested {
                request_id,
                summary,
                tool,
                ..
            } => {
                assert_eq!(tool, "workspace.write_file");
                (*request_id, summary.clone())
            }
            other => panic!("expected ToolApprovalRequested, got {other:?}"),
        };
        assert!(
            summary.contains("src/config.rs")
                && summary.contains("3 行")
                && summary.contains("字节"),
            "summary should carry path, lines and byte count: {summary}"
        );
        let resolved = engine.handle(ClientMessage::Reject {
            client_msg_id: "reject-proposal".into(),
            request_id,
            reason: "不要修改核心配置文件".into(),
        });
        assert!(resolved.iter().any(|event| matches!(
            event,
            ServerMessage::ApprovalResolved {
                approved: false,
                ..
            }
        )));
        assert!(resolved.iter().any(|event| matches!(
            event,
            ServerMessage::Audit {
                outcome,
                ..
            } if outcome == "rejected"
        )));
    }

    #[test]
    fn list_modes_and_set_mode_and_thought_level_reflected_in_run_options() {
        let engine = Engine::new();
        let list_events = engine.handle(ClientMessage::ListModes {
            client_msg_id: "modes-1".into(),
        });
        assert_eq!(list_events.len(), 2);
        assert!(matches!(&list_events[0], ServerMessage::Ack { .. }));
        if let ServerMessage::Modes {
            modes,
            current_mode,
            thought_levels,
            current_thought_level,
        } = &list_events[1]
        {
            assert_eq!(modes.len(), 3);
            assert_eq!(current_mode, "build");
            assert_eq!(thought_levels.len(), 4);
            assert_eq!(current_thought_level, "medium");
        } else {
            panic!("expected ServerMessage::Modes");
        }

        // 修改模式为 plan
        let mode_events = engine.handle(ClientMessage::SetMode {
            client_msg_id: "set-mode-1".into(),
            mode: "plan".into(),
        });
        if let ServerMessage::Modes { current_mode, .. } = &mode_events[1] {
            assert_eq!(current_mode, "plan");
        } else {
            panic!("expected ServerMessage::Modes");
        }
        assert_eq!(engine.run_options().mode.as_deref(), Some("plan"));

        // 修改思考强度为 high
        let thought_events = engine.handle(ClientMessage::SetThoughtLevel {
            client_msg_id: "set-thought-1".into(),
            thought_level: "high".into(),
        });
        if let ServerMessage::Modes {
            current_thought_level,
            ..
        } = &thought_events[1]
        {
            assert_eq!(current_thought_level, "high");
        } else {
            panic!("expected ServerMessage::Modes");
        }
        assert_eq!(engine.run_options().thought_level.as_deref(), Some("high"));
    }

    /// 档位值域与 `ReasoningEffort::from_str` 同域：客户端切模型时默认补全
    /// `values` 的最后一档（宿主广告的是 "max"），旧实现只认 4 档、把 max 静默改写成
    /// medium。越界值现在必须被拒收并报错，而不是偷改。
    #[test]
    fn thought_level_accepts_every_canonical_tier_and_refuses_unknown_ones() {
        let engine = Engine::new();
        assert_eq!(engine.current_thought_level(), None);

        for tier in ["none", "minimal", "low", "medium", "high", "xhigh", "max"] {
            let events = engine.handle(ClientMessage::SetThoughtLevel {
                client_msg_id: format!("set-thought-{tier}"),
                thought_level: tier.to_string(),
            });
            assert!(
                matches!(events[0], ServerMessage::Ack { .. }),
                "{tier} 应当被接受"
            );
            assert_eq!(
                engine.current_thought_level().as_deref(),
                Some(tier),
                "档位 {tier} 必须原样落进 run_options 的来源"
            );
            assert_eq!(engine.run_options().thought_level.as_deref(), Some(tier));
        }

        // 未知档位：拒绝并保持原值，绝不静默改写成默认档。
        let rejected = engine.handle(ClientMessage::SetThoughtLevel {
            client_msg_id: "set-thought-bad".into(),
            thought_level: "ultra".into(),
        });
        assert!(
            matches!(
                &rejected[1],
                ServerMessage::Error { code, .. } if code == "invalid_thought_level"
            ),
            "越界档位要报错：{:?}",
            rejected[1]
        );
        assert_eq!(engine.current_thought_level().as_deref(), Some("max"));
    }

    fn background_subagent_event(
        event: &str,
        child_session_id: &str,
        attempt_id: Option<&str>,
        event_seq: Option<u64>,
    ) -> SubagentLifecycleEvent {
        SubagentLifecycleEvent {
            event: event.into(),
            subagent_id: child_session_id.into(),
            child_session_id: child_session_id.into(),
            subagent_type: "explore".into(),
            description: "Inspect the repository".into(),
            status: Some(if event == "finished" { "completed" } else { "running" }.into()),
            model: Some("grok-test".into()),
            run_in_background: Some(true),
            started_at_ms: Some(100),
            parent_session_id: None,
            workflow_run_id: None,
            agent_address: None,
            attempt_id: attempt_id.map(str::to_owned),
            event_seq,
            duration_ms: Some(200),
            parent_tool_call_id: Some("tool-parent".into()),
        }
    }

    #[test]
    fn background_subagent_lifecycle_updates_snapshot_and_rejects_duplicates() {
        let engine = Engine::new();
        let parent = Uuid::new_v4();
        let mut emitted = Vec::new();
        assert!(engine.update_subagent(
            parent,
            background_subagent_event("spawned", "child-1", Some("attempt-1"), Some(1)),
            None,
            &mut |event| emitted.push(event),
        ));
        assert_eq!(emitted.len(), 1);
        assert!(matches!(emitted[0], ServerMessage::SubagentsUpdated { revision: 1, .. }));
        assert!(!engine.update_subagent(
            parent,
            background_subagent_event("spawned", "child-1", Some("attempt-1"), Some(1)),
            None,
            &mut |event| emitted.push(event),
        ));
        let (revision, ids, running, ended) = engine.session_subagents(parent);
        assert_eq!(revision, 1);
        assert_eq!(ids, vec!["child-1"]);
        assert_eq!(ended, 0);
        assert_eq!(running[0].parent_tool_call_id.as_deref(), Some("tool-parent"));
        assert_eq!(running[0].status, "running");

        let mut progress = background_subagent_event("progress", "child-1", Some("attempt-1"), Some(2));
        progress.description = String::new();
        assert!(engine.update_subagent(parent, progress, None, &mut |event| emitted.push(event)));
        let mut finish = background_subagent_event("finished", "child-1", Some("attempt-1"), Some(3));
        finish.run_in_background = None;
        assert!(engine.update_subagent(parent, finish.clone(), None, &mut |event| emitted.push(event)));
        assert!(!engine.update_subagent(parent, finish, None, &mut |event| emitted.push(event)));
        let (revision, ids, running, ended) = engine.session_subagents(parent);
        assert_eq!(revision, 3);
        assert_eq!(ids, vec!["child-1"]);
        assert!(running.is_empty());
        assert_eq!(ended, 1);
    }

    #[test]
    fn foreground_and_workflow_children_do_not_enter_background_projection() {
        let engine = Engine::new();
        let parent = Uuid::new_v4();
        let mut foreground = background_subagent_event("spawned", "child-fg", None, Some(1));
        foreground.run_in_background = Some(false);
        assert!(!engine.update_subagent(parent, foreground, None, &mut |_| {}));
        let mut workflow = background_subagent_event("spawned", "child-wf", None, Some(2));
        workflow.workflow_run_id = Some("workflow-1".into());
        assert!(!engine.update_subagent(parent, workflow, None, &mut |_| {}));
        assert_eq!(engine.session_subagents(parent), (0, Vec::new(), Vec::new(), 0));
    }

    #[test]
    fn workflow_and_subagent_lifecycle_contract_query() {
        let directory = tempfile::tempdir().unwrap();
        let engine = Engine::with_workspace(directory.path()).unwrap();
        let session_id = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "create".into(),
            workspace_id: None,
        })[0]
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
            _ => panic!("failed to create session"),
        };
        let wf_events = engine.handle(ClientMessage::ListWorkflows {
            client_msg_id: "wf-1".into(),
            session_id,
        });
        assert_eq!(wf_events.len(), 2);
        assert!(matches!(&wf_events[0], ServerMessage::Ack { .. }));
        if let ServerMessage::WorkflowRuns { workflows, .. } = &wf_events[1] {
            assert!(workflows.is_empty(), "new session has empty workflows");
        } else {
            panic!("expected WorkflowRuns");
        }

        let sub_events = engine.handle(ClientMessage::ListSubagents {
            client_msg_id: "sub-1".into(),
            session_id,
        });
        assert_eq!(sub_events.len(), 2);
        assert!(matches!(&sub_events[0], ServerMessage::Ack { .. }));
        if let ServerMessage::Subagents { subagents, .. } = &sub_events[1] {
            assert!(subagents.is_empty(), "new session has empty subagents");
        } else {
            panic!("expected Subagents");
        }

        // 目标与预算查询与设置测试
        let get_target_events = engine.handle(ClientMessage::GetSessionTarget {
            client_msg_id: "gt-1".into(),
            session_id,
        });
        assert_eq!(get_target_events.len(), 2);
        if let ServerMessage::SessionTarget { target, .. } = &get_target_events[1] {
            assert!(target.is_none(), "initial session target is none");
        } else {
            panic!("expected SessionTarget");
        }

        let set_target_events = engine.handle(ClientMessage::SetSessionTarget {
            client_msg_id: "st-1".into(),
            session_id,
            objective: "实现全量未完成项验收".into(),
            token_budget: Some(50000),
        });
        assert_eq!(set_target_events.len(), 2);
        if let ServerMessage::SessionTarget { target, .. } = &set_target_events[1] {
            let t = target.as_ref().unwrap();
            assert_eq!(t.objective, "实现全量未完成项验收");
            assert_eq!(t.token_budget, Some(50000));
            assert_eq!(t.status, "active");
        } else {
            panic!("expected SessionTarget");
        }

        // 验证工作流异常终态状态机：部分失败、取消、超时
        let failed_run = WorkflowRunInfo {
            id: "wf-fail".into(),
            name: "阶段失败工作流".into(),
            status: "failed".into(),
            current_phase: "编译代码".into(),
            total_phases: 3,
            completed_phases: 1,
            artifacts: vec!["log.txt".into()],
        };
        let cancelled_run = WorkflowRunInfo {
            id: "wf-cancel".into(),
            name: "父任务取消工作流".into(),
            status: "cancelled".into(),
            current_phase: "等待输入".into(),
            total_phases: 2,
            completed_phases: 0,
            artifacts: vec![],
        };
        let timeout_subagent = SubagentInfo {
            id: "sub-timeout".into(),
            parent_session_id: session_id.to_string(),
            role: "分析代理".into(),
            agent_type: "analyzer".into(),
            status: "timeout".into(),
            depth: 2,
        };
        let msg_wf = ServerMessage::WorkflowRuns {
            session_id,
            workflows: vec![failed_run, cancelled_run],
        };
        let msg_sub = ServerMessage::Subagents {
            session_id,
            subagents: vec![timeout_subagent],
        };
        assert_eq!(msg_wf.session_id(), Some(session_id));
        assert_eq!(msg_sub.session_id(), Some(session_id));

        // 验证本会话授权记忆（remember approval）
        let events = engine.handle(ClientMessage::ProposeFileWrite {
            client_msg_id: "tool-req".into(),
            session_id,
            relative_path: "note.txt".into(),
            contents: "hello".into(),
        });
        let req_id = match events[1] {
            ServerMessage::ToolApprovalRequested { request_id, .. } => request_id,
            _ => panic!("expected ToolApprovalRequested"),
        };
        let resolved = engine.handle(ClientMessage::ApproveRemembered {
            client_msg_id: "app-remember".into(),
            request_id: req_id,
        });
        assert!(resolved.iter().any(|event| matches!(
            event,
            ServerMessage::ApprovalResolved { approved: true, .. }
        )));
        let has_perm = engine
            .state
            .lock()
            .unwrap()
            .sessions
            .get(&session_id)
            .unwrap()
            .remembered_permissions
            .contains("workspace.write_file");
        assert!(
            has_perm,
            "remembered_permissions should record workspace.write_file"
        );

        // 验证会话一等对象 RPC 与生命周期（M1.1）
        let list_res = engine.handle(ClientMessage::ListSessions {
            client_msg_id: "ls-1".into(),
            workspace_id: None,
        });
        assert_eq!(list_res.len(), 2);
        if let ServerMessage::Sessions { sessions, .. } = &list_res[1] {
            assert_eq!(sessions.len(), 1);
            assert_eq!(sessions[0].session_id, session_id);
            assert_eq!(sessions[0].title, "新会话");
            assert!(!sessions[0].pinned);
        } else {
            panic!("expected Sessions");
        }

        // 重命名会话
        let rename_res = engine.handle(ClientMessage::RenameSession {
            client_msg_id: "rename-1".into(),
            session_id,
            title: "核心审计会话".into(),
        });
        if let ServerMessage::SessionUpdated { session } = &rename_res[1] {
            assert_eq!(session.title, "核心审计会话");
        } else {
            panic!("expected SessionUpdated");
        }

        // 置顶会话
        let pin_res = engine.handle(ClientMessage::PinSession {
            client_msg_id: "pin-1".into(),
            session_id,
            pinned: true,
        });
        if let ServerMessage::SessionUpdated { session } = &pin_res[1] {
            assert!(session.pinned);
        } else {
            panic!("expected SessionUpdated");
        }

        // 分叉会话 (Fork)
        let fork_res = engine.handle(ClientMessage::ForkSession {
            client_msg_id: "fork-1".into(),
            session_id,
        });
        let forked_session_id = match &fork_res[1] {
            ServerMessage::SessionCreated { session_id, .. } => *session_id,
            _ => panic!("expected SessionCreated"),
        };
        assert_ne!(forked_session_id, session_id);
        if let ServerMessage::SessionUpdated { session } = &fork_res[3] {
            assert_eq!(session.title, "核心审计会话 (Fork)");
        } else {
            panic!("expected SessionUpdated");
        }

        // 删除会话
        let del_res = engine.handle(ClientMessage::DeleteSession {
            client_msg_id: "del-1".into(),
            session_id: forked_session_id,
        });
        assert!(
            matches!(&del_res[1], ServerMessage::SessionDeleted { session_id } if *session_id == forked_session_id)
        );
    }

    /// 侧栏那格「待处理交互」数的是引擎会话里真停着的东西：`/ask` 把一个问题
    /// 挂进会话等人回答，`/approve-tool` 挂一个待审批操作。查询给出的是这个
    /// 事实，而不是写死的零——写死的零会把一个等审批的会话画成无事发生。
    #[test]
    fn pending_interaction_counts_reflect_what_waits_in_the_session() {
        let engine = Engine::new();
        let session_id = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "pending-count-create".into(),
            workspace_id: None,
        })[0]
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
            _ => panic!("expected SessionCreated"),
        };

        // 没有任何 pending：两个计数都是 0——这是事实，不是省略。
        assert_eq!(engine.pending_interaction_counts(session_id), (0, 0));

        // /ask 把一个问题挂进会话：userInput 计数变 1，permission 仍为 0。
        engine.handle(ClientMessage::Submit {
            client_msg_id: "pending-count-ask".into(),
            session_id,
            prompt: "/ask 这台机器的备份策略是什么".into(),
        });
        assert_eq!(engine.pending_interaction_counts(session_id), (0, 1));

        // 一个会话同时只挂一种交互（问题未答时新提交被 question_pending 拒掉），
        // 审批这条用另一个会话验：permission 计数变 1，userInput 仍为 0。
        let approval_session = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "pending-count-approval-create".into(),
            workspace_id: None,
        })[0]
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
            _ => panic!("expected SessionCreated"),
        };
        engine.handle(ClientMessage::Submit {
            client_msg_id: "pending-count-approve".into(),
            session_id: approval_session,
            prompt: "/approve-tool 删除过期日志".into(),
        });
        assert_eq!(engine.pending_interaction_counts(approval_session), (1, 0));

        // 不存在的会话不给假计数。
        assert_eq!(engine.pending_interaction_counts(Uuid::new_v4()), (0, 0));
    }

    /// Provider 在 `usage` 帧里报了多少，账本上就得是多少——「使用统计」快照
    /// 有没有数，全靠这条链：adapter 的 `TurnReport` → `record_turn` →
    /// `app_usage_snapshot`。
    /// 聚合函数自己有单测，但「一次真实轮次」到「快照里的数字」之间没人验过。
    #[test]
    fn reported_provider_usage_reaches_the_app_usage_snapshot() {
        struct Reporting;

        impl PromptAdapter for Reporting {
            fn run_prompt(
                &self,
                _prompt: &str,
                _options: &RunOptions,
            ) -> Result<Vec<String>, String> {
                Ok(vec!["答案".to_string()])
            }

            fn run_prompt_turn(
                &self,
                prompt: &str,
                options: &RunOptions,
                emit: &mut dyn FnMut(&str) -> bool,
            ) -> Result<TurnReport, String> {
                for chunk in self.run_prompt(prompt, options)? {
                    if !emit(&chunk) {
                        break;
                    }
                }
                Ok(TurnReport {
                    usage: Some(ProviderUsage {
                        input_tokens: 120,
                        output_tokens: 34,
                        ..Default::default()
                    }),
                })
            }

            fn default_model(&self) -> Option<&str> {
                Some("fixture-model")
            }
        }

        let engine = Engine::with_adapter(Reporting);
        let session_id = match engine.handle(ClientMessage::CreateSession {
            client_msg_id: "usage-create".into(),
            workspace_id: None,
        })[0]
        {
            ServerMessage::SessionCreated { session_id, .. } => session_id,
            _ => panic!(),
        };
        engine.handle(ClientMessage::Submit {
            client_msg_id: "usage-submit".into(),
            session_id,
            prompt: "这一轮的账记在哪".into(),
        });

        // 轮次跑在单独的线程上，等它落账，不要赌它已经跑完。
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let snapshot = loop {
            let snapshot = engine.app_usage_snapshot("all", None);
            if snapshot["summary"]["totalTurns"].as_u64() == Some(1) {
                break snapshot;
            }
            if std::time::Instant::now() >= deadline {
                panic!("这一轮的账没有进快照：{snapshot}");
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        let summary = &snapshot["summary"];
        assert_eq!(summary["inputTokens"], 120);
        assert_eq!(summary["outputTokens"], 34);
        assert_eq!(summary["totalTokens"], 154);
        assert_eq!(summary["totalSessions"], 1);
        // 记到实际选中的那个模型头上，不是笼统一笔。
        assert_eq!(snapshot["models"][0]["modelId"], "fixture-model");
        assert_eq!(snapshot["models"][0]["totalTokens"], 154);
    }
}
