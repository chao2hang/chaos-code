//! CLI argument parsing for the pager.
pub use crate::headless::OutputFormat;
use clap::{ArgAction, Parser, Subcommand, ValueHint};
use clap_complete::Shell;
use std::net::SocketAddr;
use std::path::PathBuf;
/// Chaos 主命令。
#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// 在无交互界面的模式下运行 Chaos Agent
    Agent(Box<AgentArgs>),
    /// 显示当前目录生效的配置
    Inspect {
        /// 输出机器可读的 JSON。
        #[arg(long)]
        json: bool,
    },
    /// 检查终端支持与配置，但不启动 Chaos
    Doctor(crate::doctor_cmd::DoctorArgs),
    /// 管理正在运行的 Leader 进程
    Leader(LeaderMgmtArgs),
    /// 管理 MCP 服务器配置
    Mcp(crate::mcp_cmd::McpArgs),
    /// 管理插件与插件市场源
    Plugin(crate::plugin_cmd::PluginArgs),
    /// 管理跨会话记忆
    Memory(crate::memory_cmd::MemoryArgs),
    /// 列出可用模型并退出
    Models,
    /// 列出、搜索或恢复会话
    Sessions(crate::sessions_cmd::SessionsArgs),
    /// 获取并安装托管配置
    Setup {
        /// 以 JSON 输出获取到的配置，不执行安装或写入。
        #[arg(long)]
        json: bool,
    },
    /// 分享会话并输出分享链接
    #[command(hide = true)]
    Share(crate::share_cmd::ShareArgs),
    /// 运行任意命令，并将 OSC 52 剪贴板内容转发到本机
    #[cfg_attr(not(any(unix, windows)), command(hide = true))]
    #[command(long_about = "\
在本地伪终端中运行任意命令，并将远端剪贴板内容转发到本机。

适用于 `docker exec`、`kubectl exec`、SSH 等无法直接访问本机剪贴板的环境。
Chaos 会拦截命令输出中的 OSC 52 转义序列并写入系统剪贴板，同时同步窗口大小。

示例：
  chaos wrap docker exec -it my-container bash
  chaos wrap kubectl exec -it my-pod -- bash

更多信息请参阅 ~/.grok/README.md。
")]
    Wrap(WrapArgs),
    /// 将会话记录导出为 Markdown
    Export(crate::export_cmd::ExportArgs),
    /// 导出或上传会话追踪数据
    Trace(crate::trace_cmd::TraceArgs),
    /// 检查更新或安装指定版本
    Update {
        /// 仅检查更新，不安装。
        #[arg(long)]
        check: bool,
        /// 输出机器可读的 JSON（配合 --check）。
        #[arg(long)]
        json: bool,
        /// 即使已是最新版本，也强制重新下载并安装。
        #[arg(long)]
        force_reinstall: bool,
        /// 安装指定版本（如 0.1.150）。
        #[arg(long)]
        version: Option<String>,
        /// 切换到 alpha 更新通道。
        #[arg(long, conflicts_with_all = ["stable", "enterprise"])]
        alpha: bool,
        /// 切换到 stable 更新通道（默认）。
        #[arg(long, conflicts_with_all = ["alpha", "enterprise"])]
        stable: bool,
        /// 切换到 enterprise 更新通道。
        #[arg(long, conflicts_with_all = ["alpha", "stable"], hide = true)]
        enterprise: bool,
    },
    /// 输出版本信息
    #[command(visible_alias = "v")]
    Version {
        /// 输出机器可读的 JSON。
        #[arg(long)]
        json: bool,
    },
    /// 生成 Shell 补全脚本（bash、zsh、fish、PowerShell 等）
    Completions {
        /// 目标 Shell
        #[arg(value_enum)]
        shell: Shell,
    },
    /// 管理 Git worktree
    Worktree(crate::worktree_cmd::WorktreeArgs),
    /// 将此工作区暴露给 Computer Hub（通过 leader）。
    ///
    /// 默认禁用，由服务端按账号启用；设置
    /// `GROK_WORKSPACE_COMMAND=1` 可在本地启用以进行测试。
    #[command(hide = true)]
    Workspace(WorkspaceMgmtArgs),
    /// 启动时打开 Agent 仪表盘
    ///
    /// 所有会话（顶层和子代理）的集中式、代理原生概览。
    /// 当 `~/.grok/config.toml` 中 `[dashboard].enabled = false` 或
    /// 设置环境变量 `GROK_AGENT_DASHBOARD=0` 时禁用。
    Dashboard,
}
/// `wrap` 子命令参数：要运行的命令及其参数。
#[derive(Debug, clap::Args, Clone)]
pub struct WrapArgs {
    /// 要运行的命令及参数（如 `docker exec -it my-container bash`）。
    #[arg(
        required = true,
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "CMD"
    )]
    pub command: Vec<String>,
}
/// 通过 PID 指定运行中的 leader 进程（用于 `chaos leader` / `chaos workspace`）。
#[derive(Debug, clap::Args, Clone, Default)]
pub struct LeaderTargetArgs {
    /// Leader 进程 ID（来自 `chaos leader list`）。
    #[arg(long)]
    pub pid: Option<u32>,
}
#[derive(Debug, clap::Args, Clone)]
pub struct LeaderMgmtArgs {
    #[command(subcommand)]
    pub command: LeaderMgmtCommand,
}
#[derive(Debug, Subcommand, Clone)]
pub enum LeaderMgmtCommand {
    /// 列出运行中的 leader 进程
    List {
        /// 输出机器可读的 JSON。
        #[arg(long)]
        json: bool,
    },
    /// 显示 leader 进程详情
    Info {
        #[command(flatten)]
        target: LeaderTargetArgs,
        /// 输出机器可读的 JSON。
        #[arg(long)]
        json: bool,
    },
    /// 停止所有运行中的 leader 进程
    Kill,
}
#[derive(Debug, clap::Args, Clone)]
pub struct WorkspaceMgmtArgs {
    #[command(subcommand)]
    pub command: WorkspaceMgmtCommand,
}
#[derive(Debug, Subcommand, Clone)]
pub enum WorkspaceMgmtCommand {
    /// 启动（或更新）工作区→hub 暴露。
    Start(WorkspaceStartArgs),
    /// 排空并断开与 hub 的连接，保持暴露热状态。
    Pause {
        #[command(flatten)]
        target: LeaderTargetArgs,
        /// 输出机器可读的 JSON。
        #[arg(long)]
        json: bool,
    },
    /// 将暂停的暴露重新连接到 hub。
    Resume {
        #[command(flatten)]
        target: LeaderTargetArgs,
        /// 输出机器可读的 JSON。
        #[arg(long)]
        json: bool,
    },
    /// 停止暴露工作区（leader 继续运行）。
    Stop {
        #[command(flatten)]
        target: LeaderTargetArgs,
        /// 输出机器可读的 JSON。
        #[arg(long)]
        json: bool,
    },
    /// 重启暴露（停止后以给定选项重新启动）。
    Restart(WorkspaceStartArgs),
    /// 显示当前工作区暴露状态。
    #[command(visible_alias = "list")]
    Status {
        #[command(flatten)]
        target: LeaderTargetArgs,
        /// 输出机器可读的 JSON。
        #[arg(long)]
        json: bool,
    },
}
#[derive(Debug, clap::Args, Clone)]
pub struct WorkspaceStartArgs {
    /// Computer Hub WebSocket URL（默认：`[hub].url`，然后是生产 hub）。
    #[arg(long, value_name = "URL")]
    pub hub_url: Option<String>,
    /// 要暴露的工作区根目录。默认为当前目录。
    #[arg(long, value_name = "DIR", value_hint = ValueHint::DirPath)]
    pub cwd: Option<PathBuf>,
    /// 强制 leader 模式，覆盖配置。
    #[arg(long, conflicts_with = "no_leader")]
    pub leader: bool,
    /// 拒绝启动，即使配置启用了 leader 模式。
    #[arg(long, conflicts_with = "leader")]
    pub no_leader: bool,
    /// 输出机器可读的 JSON。
    #[arg(long)]
    pub json: bool,
}
/// `agent` 子命令参数。
#[derive(Debug, clap::Args, Clone)]
pub struct AgentArgs {
    /// 启动前先进行认证
    #[arg(
        long = "reauth",
        visible_alias = "--reauthenticate",
        default_value = "false"
    )]
    pub reauthenticate: bool,
    /// 使用的模型 ID
    #[arg(short = 'm', long = "model", value_name = "MODEL")]
    pub model: Option<String>,
    /// 推理模型的推理强度
    #[clap(
        long = "reasoning-effort",
        visible_alias = "effort",
        value_name = "EFFORT",
        overrides_with = "reasoning_effort"
    )]
    pub reasoning_effort: Option<String>,
    /// 自动批准所有工具执行
    #[arg(long = "always-approve", alias = "yolo")]
    pub yolo: bool,
    /// Agent 配置文件路径。
    #[arg(long = "agent-profile", value_name = "PATH")]
    pub agent_profile: Option<PathBuf>,
    /// 仅为本次进程从此目录加载插件（可重复）。
    /// 最高优先级插件作用域；始终受信 — hooks 和 MCP 服务器
    /// 无需提示即激活。由 Agent SDK 用于注入每连接插件。
    #[arg(long = "plugin-dir", value_name = "DIR", value_hint = ValueHint::DirPath)]
    pub plugin_dirs: Vec<PathBuf>,
    /// 连接到共享 leader 进程而非启动新 agent。
    /// 允许多个客户端共享一个后端。
    /// 默认为 config.toml 中的 [cli] use_leader。
    #[arg(long, conflicts_with = "no_leader")]
    pub leader: bool,
    /// 即使配置启用 leader 模式也启动新 agent。
    #[arg(long, conflicts_with = "leader")]
    pub no_leader: bool,
    #[command(flatten)]
    pub headless: HeadlessArgs,
    /// 覆盖 CLI 聊天代理 base URL。
    #[arg(long = "cli-chat-proxy-base-url")]
    pub cli_chat_proxy_base_url: Option<String>,
    /// 覆盖公共 API base URL。
    #[arg(long = "xai-api-base-url")]
    pub xai_api_base_url: Option<String>,
    /// Agent 运行时模式
    #[command(subcommand)]
    pub mode: Option<AgentCmd>,
}
impl AgentArgs {
    /// Canonicalized `--plugin-dir` paths, warning to stderr and skipping
    /// anything that isn't an existing directory (stderr is safe: JSON-RPC
    /// rides stdout).
    pub fn canonical_plugin_dirs(&self) -> Vec<PathBuf> {
        self.plugin_dirs
            .iter()
            .filter_map(|p| match dunce::canonicalize(p) {
                Ok(canonical) if canonical.is_dir() => Some(canonical),
                Ok(_) => {
                    eprintln!("chaos：--plugin-dir {} 不是目录，已跳过", p.display());
                    None
                }
                Err(e) => {
                    eprintln!("chaos：无法读取 --plugin-dir {}：{e}，已跳过", p.display());
                    None
                }
            })
            .collect()
    }
}
/// Agent 子命令。
#[derive(Debug, Subcommand, Clone)]
pub enum AgentCmd {
    /// 通过 stdio 运行 Agent
    Stdio,
    /// 通过 WebSocket 中继以无界面模式运行 Agent
    Headless(HeadlessArgs),
    /// 将 Agent 作为 WebSocket 服务运行
    Serve(ServeArgs),
    /// 作为其他客户端共享的 Leader 进程运行
    Leader(LeaderArgs),
}
/// WebSocket URL 覆盖参数，用于 headless / leader / serve 模式。
#[derive(Debug, clap::Args, Clone, Default)]
pub struct HeadlessArgs {
    #[arg(long = "grok-ws-origin")]
    pub grok_ws_origin: Option<String>,
    #[arg(long = "grok-ws-url")]
    pub grok_ws_url: Option<String>,
}
/// `agent serve` 子命令参数。
#[derive(Debug, clap::Args, Clone)]
pub struct ServeArgs {
    /// 服务器监听地址
    #[arg(long, default_value = "127.0.0.1:2419")]
    pub bind: SocketAddr,
    /// 客户端认证密钥（未提供则自动生成）
    #[arg(long, env = "GROK_AGENT_SECRET")]
    pub secret: Option<String>,
    /// 代理模式的远程 agent URL
    #[arg(long)]
    pub remote: Option<String>,
    /// 认证和 WebSocket URL 覆盖参数
    #[command(flatten)]
    pub headless: HeadlessArgs,
}
impl ServeArgs {
    /// Get the secret, generating a random one if not provided.
    pub fn get_secret(&self) -> String {
        self.secret
            .clone()
            .unwrap_or_else(|| generate_random_key(12))
    }
}
/// Generate a random alphanumeric key of the given length.
fn generate_random_key(len: usize) -> String {
    let raw = uuid::Uuid::new_v4().to_string().replace('-', "");
    raw.chars().cycle().take(len).collect()
}
/// `agent leader` 子命令参数。
#[derive(Debug, clap::Args, Clone)]
pub struct LeaderArgs {
    /// 最后一个客户端断开后保持 leader 运行。
    #[arg(long)]
    pub no_exit_on_disconnect: bool,
    /// 延迟 grok.com 中继 WebSocket 连接，直到首个 headless IPC 客户端
    /// 注册。不带此标志时 leader 在启动时即连接中继 —
    /// 适用于通过中继接收远程提示的裸 leader（headless 远程环境 / systemd）。
    /// 由交互式客户端（TUI/IDE）自动生成的 leader 传递，仅在 headless 客户端出现时才需要中继。
    #[arg(long)]
    pub relay_on_demand: bool,
    /// 禁用 leader 的定期自动更新检查。
    #[arg(long)]
    pub no_auto_update: bool,
    /// 所有环境 URL 覆盖（从 follower 进程传入）
    #[command(flatten)]
    pub headless: HeadlessArgs,
}
#[derive(Debug, Clone, Parser)]
#[command(
    name = "chaos",
    version = env!("VERSION_WITH_COMMIT"),
    about = "Chaos AI 编码助手",
    disable_version_flag = true,
    next_display_order = None,
    help_template = "\
{before-help}{about-with-newline}
用法： {usage}

参数：
{positionals}

选项：
{options}

命令：
{subcommands}{after-help}\
"
)]
pub struct PagerArgs {
    /// 输出版本
    #[arg(short = 'v', short_alias = 'V', long = "version", action = ArgAction::SetTrue)]
    pub version: bool,
    /// 工作目录。
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    /// 使用自定义 Leader socket 路径。
    #[arg(
        long = "leader-socket",
        value_name = "PATH",
        global = true,
        value_hint = ValueHint::FilePath
    )]
    pub leader_socket: Option<PathBuf>,
    /// 启用调试日志。
    #[arg(long = "debug", global = true)]
    pub debug: bool,
    /// 将调试日志写入 FILE。
    #[arg(
        long = "debug-file",
        value_name = "FILE",
        global = true,
        value_hint = ValueHint::FilePath
    )]
    pub debug_file: Option<PathBuf>,
    /// 自动批准所有工具操作。
    #[clap(
        long = "always-approve",
        alias = "yolo",
        alias = "dangerously-skip-permissions"
    )]
    pub yolo: bool,
    /// 信任此文件夹并将决策持久化到信任存储。
    #[arg(long = "trust", alias = "trust-folder", hide = true)]
    pub trust: bool,
    /// 权限允许规则（兼容别名：--allowedTools）。
    #[arg(
        long = "allow",
        alias = "allowedTools",
        value_name = "RULE",
        value_delimiter = ','
    )]
    pub allow_rules: Vec<String>,
    /// 权限拒绝规则（兼容别名：--disallowedTools）。
    #[arg(
        long = "deny",
        alias = "disallowedTools",
        value_name = "RULE",
        value_delimiter = ','
    )]
    pub deny_rules: Vec<String>,
    /// 单轮提示词；将响应输出到 stdout 后退出。
    #[clap(
        short = 'p',
        long = "single",
        alias = "print",
        value_name = "PROMPT",
        conflicts_with_all = &["prompt_json",
        "prompt_file"]
    )]
    pub single: Option<String>,
    /// 以 JSON 内容块提供单轮提示词。
    #[clap(
        long = "prompt-json",
        value_name = "JSON",
        conflicts_with_all = &["single",
        "prompt_file"]
    )]
    pub prompt_json: Option<String>,
    /// 从文件读取单轮提示词。
    #[clap(
        long = "prompt-file",
        value_name = "PATH",
        conflicts_with_all = &["single",
        "prompt_json"],
        value_hint = ValueHint::FilePath
    )]
    pub prompt_file: Option<PathBuf>,
    /// 原样发送提示词。
    #[clap(long)]
    pub verbatim: bool,
    /// 无界面模式的输出格式。
    #[clap(long = "output-format", value_enum, default_value = "plain")]
    pub output_format: OutputFormat,
    /// 结构化输出的 JSON Schema。设置后，模型将被约束为
    /// 生成匹配此 schema 的 JSON。隐含 --output-format json。
    /// 示例：--json-schema '{"type":"object","properties":{"name":{"type":"string"}}}'
    #[clap(long = "json-schema", value_name = "SCHEMA")]
    pub json_schema: Option<String>,
    /// 要使用的模型 ID。
    #[clap(short = 'm', long = "model", value_name = "MODEL")]
    pub model: Option<String>,
    /// 推理模型的推理强度
    #[clap(
        long = "reasoning-effort",
        visible_alias = "effort",
        value_name = "EFFORT",
        overrides_with = "reasoning_effort"
    )]
    pub reasoning_effort: Option<String>,
    /// 追加到系统提示词的规则。
    #[clap(long = "rules", alias = "append-system-prompt")]
    pub rules: Option<String>,
    /// 压缩模式 [summary|transcript|segments]：`summary`（默认）不添加
    /// 指针；`transcript` 指向原始转录；`segments` 按段持久化 markdown 以便 grep。设置 `GROK_COMPACTION_MODE`。
    #[clap(long = "compaction-mode", value_name = "MODE", hide = true)]
    pub compaction_mode: Option<String>,
    /// 段落逐字详情 [none|minimal|balanced|verbose]（默认
    /// `verbose`）。仅影响 `--compaction-mode segments`。设置
    /// `GROK_COMPACTION_DETAIL`。
    #[clap(long = "compaction-detail", value_name = "DETAIL", hide = true)]
    pub compaction_detail: Option<String>,
    /// 覆盖 agent 的系统提示词（兼容别名：--system-prompt）。
    #[clap(
        long = "system-prompt-override",
        alias = "system-prompt",
        value_name = "PROMPT"
    )]
    pub system_prompt_override: Option<String>,
    /// 按 ID 恢复会话；省略 ID 时恢复最近会话。
    #[arg(
        long = "resume",
        short = 'r',
        value_name = "SESSION_ID",
        num_args = 0..= 1,
        default_missing_value = "",
        conflicts_with_all = ["continue_last_session"]
    )]
    pub resume_session: Option<String>,
    /// 按会话 ID 恢复之前的会话（--resume 的别名）。
    #[arg(
        long = "load",
        value_name = "SESSION_ID",
        hide = true,
        conflicts_with_all = ["continue_last_session"]
    )]
    pub load_session: Option<String>,
    /// 继续当前工作目录中最近的会话。
    #[arg(
        short = 'c',
        long = "continue",
        conflicts_with_all = ["resume_session",
        "load_session"]
    )]
    pub continue_last_session: bool,
    /// 为**新**对话使用指定会话 UUID（必须是有效 UUID 且
    /// 不能已存在于目标会话目录下）。配合 `--resume`/`--continue` 时，
    /// 仅与 `--fork-session` 一起使用（命名分叉会话）。不恢复现有会话 —
    /// 请使用 `--resume` / `--continue`。
    #[arg(short = 's', long = "session-id", value_name = "SESSION_ID")]
    pub session_id: Option<String>,
    /// 恢复时（`--resume` / `--continue`），创建新会话 ID
    /// 而非复用原始 ID（可通过 `--session-id` 指定）。
    #[arg(long = "fork-session")]
    pub fork_session: bool,
    /// 在新的 Git worktree 中启动会话，可指定名称。
    #[arg(short = 'w', long = "worktree", num_args = 0..= 1, default_missing_value = "")]
    pub worktree: Option<String>,
    /// worktree 基于的分支、标签或提交（配合 `--worktree`）。
    /// 省略时默认为源检出的当前 HEAD。
    #[arg(long = "worktree-ref", visible_alias = "ref", requires = "worktree")]
    pub worktree_ref: Option<String>,
    /// 恢复时检出原始会话的提交。
    #[arg(long = "restore-code", requires = "resume_session")]
    pub restore_code: bool,
    /// 禁用计划模式。
    #[arg(long = "no-plan")]
    pub no_plan: bool,
    /// 禁止创建子 Agent。
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,
    /// 禁用结构化用户提问提示。
    #[arg(long = "no-ask-user", hide = true)]
    pub no_ask_user: bool,
    /// 启用跨会话记忆。
    #[arg(long = "experimental-memory", conflicts_with = "no_memory")]
    pub experimental_memory: bool,
    /// 在本次会话中禁用跨会话记忆。
    #[arg(long = "no-memory", conflicts_with = "experimental_memory")]
    pub no_memory: bool,
    /// Agent 名称或定义文件路径。
    #[arg(long = "agent", value_name = "NAME")]
    pub agent: Option<String>,
    /// 以 JSON 内联定义子 Agent。
    #[arg(long = "agents", value_name = "JSON")]
    pub agents_json: Option<String>,
    /// 允许使用的内置工具，以逗号分隔。
    #[arg(long = "tools", value_name = "TOOLS")]
    pub cli_tools: Option<String>,
    /// 禁止使用的内置工具，以逗号分隔。
    #[arg(long = "disallowed-tools", value_name = "TOOLS")]
    pub cli_disallowed_tools: Option<String>,
    /// Agent 最大轮数。
    #[arg(
        long = "max-turns",
        value_name = "N",
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    pub max_turns: Option<u32>,
    /// 权限模式。
    #[arg(
        long = "permission-mode",
        value_name = "MODE",
        value_parser = clap::builder::PossibleValuesParser::new(
            xai_grok_shell::agent::config::PermissionMode::VALID_VALUES
        )
    )]
    pub permission_mode_flag: Option<String>,
    /// 禁用网页搜索与抓取工具。
    #[arg(long = "disable-web-search")]
    pub disable_web_search: bool,
    /// 在首个 Agent 轮次结束后立即退出，不等待后台 bash/monitor 任务或后台子
    /// Agent 完成（仅限无头模式）。默认所有 `chaos -p` 运行都会等待（上限为
    /// `--background-wait-timeout`），以便评估框架能看到完整任务完成。使用此
    /// 选项可加速只需首轮文本的脚本。不会等待服务端自动唤醒输出或持久监视器
    /// （它们会触发超时）。
    #[arg(long = "no-wait-for-background", hide = true)]
    pub no_wait_for_background: bool,
    /// 首轮结束后等待后台任务的最大秒数（仅限无头模式）。适用于
    /// bash/monitor `task_completed`、后台子 Agent（`SubagentFinished`）及
    /// 任何仍在运行的非持久任务。持久 `monitor(persistent:true)` 永不完成，
    /// 总是等待完整超时——可使用 `--no-wait-for-background` 或降低超时值
    /// 来提高吞吐。与 `--no-wait-for-background` 互斥。
    #[arg(
        long = "background-wait-timeout",
        value_name = "SECS",
        default_value = "600",
        conflicts_with = "no_wait_for_background",
        hide = true,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub background_wait_timeout_secs: u64,
    /// 文件系统和网络访问的沙箱配置。
    #[arg(long, env = "GROK_SANDBOX", value_name = "PROFILE")]
    pub sandbox: Option<String>,
    /// 会话存储模式：local 或 writeback。
    #[arg(long = "storage-mode", value_name = "MODE", hide = true)]
    pub storage_mode: Option<String>,
    /// 覆盖发送给 Agent 的客户端标识符。
    #[arg(long = "client-identifier", value_name = "ID", hide = true)]
    pub client_identifier: Option<String>,
    /// Hunk 跟踪模式：agent_only、all_dirty 或 off（"disabled" 是 off 的
    /// 别名，完全关闭 hunk 跟踪）。
    #[arg(long = "hunk-tracker-mode", value_name = "MODE", hide = true)]
    pub hunk_tracker_mode: Option<String>,
    /// 为 Agent 启用终端支持。
    #[arg(long = "terminal", hide = true)]
    pub terminal: bool,
    /// 启用客户端文件读取。
    #[arg(long = "fs-read", hide = true)]
    pub fs_read: bool,
    /// 启用客户端文件写入。
    #[arg(long = "fs-write", hide = true)]
    pub fs_write: bool,
    /// 本次会话禁用自动更新。
    #[arg(long = "no-auto-update", hide = true)]
    pub no_auto_update: bool,
    /// 启用运行时轮次结束 TodoGate。
    ///
    /// 仅对当前会话生效（不持久化）。优先级最高——
    /// 覆盖远程 `todo_gate_enabled` 和内置默认值（`false`）。
    #[arg(long = "todo-gate", hide = true)]
    pub todo_gate: bool,
    /// 设置 config.toml 中的 installer 字段。
    #[arg(long = "installer", value_name = "VALUE", hide = true)]
    pub installer: Option<String>,
    /// 内联运行，不使用终端备用屏幕。
    #[arg(long = "no-alt-screen")]
    pub no_alt_screen: bool,
    /// 实验性：滚动原生渲染。已完成的块会输出到终端原生滚动缓冲区
    /// （使用终端自身的滚动/选择）；底部固定区域显示提示符和运行中的轮次。
    /// 仅对当前会话生效——不写入配置。要将默认 `chaos` 设为 minimal 模式，
    /// 在 ~/.grok/config.toml 中设置 `[ui] screen_mode = "minimal"`。
    #[arg(long = "minimal")]
    pub minimal: bool,
    /// 以标准全屏 TUI 打开本次会话，覆盖配置中的
    /// `[ui] screen_mode = "minimal"` 偏好。仅对当前会话生效——不写入配置。
    /// 全屏与内联仍遵循备用屏幕策略（--no-alt-screen、[terminal] alt_screen、
    /// 终端自动检测）。
    #[arg(long = "fullscreen", conflicts_with = "minimal")]
    pub fullscreen: bool,
    /// 将采样事件写入 ~/.grok/logs/sampling.jsonl。
    #[arg(long = "log-sampling", env = "GROK_LOG_SAMPLING", hide = true)]
    pub log_sampling: bool,
    /// 连接到共享的 leader 进程。
    #[arg(long, conflicts_with = "no_leader", hide = true)]
    pub leader: bool,
    /// 即使配置了 leader 模式也独立运行。
    #[arg(long, conflicts_with = "leader", hide = true)]
    pub no_leader: bool,
    /// 交互式会话的初始提示，例如 `chaos "fix the bug"` 或 `chaos --worktree=feat "create this feature"`。
    #[arg(
        value_name = "PROMPT",
        conflicts_with_all = &["single",
        "prompt_json",
        "prompt_file"]
    )]
    pub prompt: Option<String>,
    /// 子命令（例如 `agent`）。
    #[command(subcommand, next_display_order = 0)]
    pub command: Option<Command>,
}
/// Outcome of resolving the startup sandbox profile for a (possibly resumed)
/// session. See [`PagerArgs::startup_sandbox_profile`].
#[derive(Debug, PartialEq, Eq)]
pub enum SandboxStartup {
    /// Apply this profile. `None` means fall through to config/`off`.
    Apply(Option<String>),
    /// Resume requested a profile that differs from the one the session was
    /// created with. Refused so resuming can't silently change the sandbox.
    Conflict { requested: String, saved: String },
}
/// How resume-selection flags resolve for sandbox profile lookup.
/// Derived from [`PagerArgs::session_startup_intent`]; new-with-id is not a resume.
#[derive(Debug, PartialEq, Eq)]
pub enum ResumeTarget {
    /// Resume (or fork-from) a specific session id.
    SessionId(String),
    /// Resume (or fork-from) the most recent session for the current directory.
    MostRecentForCwd,
    /// Not resuming an existing session (new auto or new-with-id).
    None,
}
fn anchor_to_launch_dir(path: PathBuf, launch_dir: Option<&std::path::Path>) -> PathBuf {
    if path.is_absolute() {
        strip_cur_dir(path)
    } else if let Some(launch_dir) = launch_dir {
        strip_cur_dir(launch_dir.join(path))
    } else {
        strip_cur_dir(path)
    }
}
fn strip_cur_dir(path: PathBuf) -> PathBuf {
    path.components()
        .filter(|component| !matches!(component, std::path::Component::CurDir))
        .collect()
}
impl PagerArgs {
    /// Parse CLI arguments without applying side effects.
    pub fn parse_cli() -> Self {
        let bin_name = std::env::args()
            .next()
            .as_deref()
            .map(std::path::Path::new)
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .filter(|n| *n == "chaos" || *n == "agent")
            .unwrap_or("chaos")
            .to_owned();
        Self::parse_from(std::iter::once(bin_name).chain(std::env::args().skip(1)))
    }
    /// Apply launch-directory path anchoring and `--cwd` after early commands
    /// have been dispatched without filesystem or process initialization.
    pub fn apply_cwd(self) -> anyhow::Result<Self> {
        let launch_dir = std::env::current_dir().ok();
        self.apply_cwd_from(launch_dir.as_deref())
    }
    fn apply_cwd_from(mut self, launch_dir: Option<&std::path::Path>) -> anyhow::Result<Self> {
        if let Some(socket) = self.leader_socket.take() {
            self.leader_socket = Some(anchor_to_launch_dir(socket, launch_dir));
        }
        if let Some(file) = self.debug_file.take() {
            self.debug_file = Some(anchor_to_launch_dir(file, launch_dir));
        }
        if let Some(ref cwd) = self.cwd {
            std::env::set_current_dir(cwd).map_err(|e| {
                anyhow::anyhow!("Failed to set working directory to {:?}: {}", cwd, e)
            })?;
        }
        Ok(self)
    }
    /// Optional-flag accessor; always `false` in builds without the optional
    /// feature, so call sites need no `cfg` of their own.
    pub fn chat(&self) -> bool {
        false
    }
    /// Get the session ID to resume, from either --resume or --load (hidden alias).
    ///
    /// Returns `None` when `--resume` was used without a value (the empty-string
    /// sentinel). Use [`resume_most_recent`] to detect that case.
    pub fn session_to_resume(&self) -> Option<&str> {
        self.resume_session
            .as_deref()
            .or(self.load_session.as_deref())
            .filter(|s| !s.is_empty())
    }
    /// Whether `--resume` was used without a session ID (meaning "resume most recent").
    pub fn resume_most_recent(&self) -> bool {
        self.resume_session.as_deref() == Some("")
    }
    /// Classify flags for sandbox profile lookup on an existing session.
    ///
    /// Uses [`Self::session_startup_intent`]; invalid combos fall through to
    /// `None` (caller should have rejected intent errors earlier at startup).
    pub fn resume_target(&self) -> ResumeTarget {
        use crate::app::session_startup::SessionStartupIntent;
        match self.session_startup_intent() {
            Ok(SessionStartupIntent::Resume {
                session_id: Some(id),
                ..
            })
            | Ok(SessionStartupIntent::ForkFrom {
                source_session_id: Some(id),
                ..
            }) => ResumeTarget::SessionId(id),
            Ok(SessionStartupIntent::Resume {
                most_recent_for_cwd: true,
                ..
            })
            | Ok(SessionStartupIntent::ForkFrom {
                most_recent_for_cwd: true,
                ..
            }) => ResumeTarget::MostRecentForCwd,
            _ => ResumeTarget::None,
        }
    }
    /// Resolve the sandbox profile to apply at startup, accounting for the
    /// profile the resumed session was created with. `saved` is the resumed
    /// session's persisted profile (read once via [`Self::saved_resume_profile`]).
    ///
    /// A session's profile is fixed at creation. Resuming restores it; passing an
    /// explicit `--sandbox`/`GROK_SANDBOX` that differs from the saved profile is
    /// refused (changing a session's sandbox on resume is a safety footgun). A
    /// matching flag, or no flag, resumes with the saved profile.
    pub fn startup_sandbox_profile(&self, saved: Option<&str>) -> SandboxStartup {
        let explicit = self.sandbox.as_deref().filter(|s| !s.is_empty());
        Self::resolve_startup_sandbox(explicit, saved.map(String::from))
    }
    /// The sandbox profile persisted with the session being resumed, if any.
    /// Local, best-effort; `None` when not resuming or nothing is found. Read once
    /// for the profile resume resolution.
    pub fn saved_resume_profile(&self) -> Option<String> {
        let cwd_buf = std::env::current_dir().ok();
        let cwd_str = cwd_buf.as_deref().map(|p| p.to_string_lossy());
        let cwd = cwd_str.as_deref();
        match self.resume_target() {
            ResumeTarget::SessionId(id) => {
                xai_grok_shell::session::persistence::resumed_session_sandbox_profile(
                    Some(&id),
                    cwd,
                )
            }
            ResumeTarget::MostRecentForCwd => {
                xai_grok_shell::session::persistence::resumed_session_sandbox_profile(None, cwd)
            }
            ResumeTarget::None => None,
        }
    }
    /// Pure resolution of the explicit flag against the resumed session's saved
    /// profile. Separated from disk access so it can be unit-tested.
    fn resolve_startup_sandbox(explicit: Option<&str>, saved: Option<String>) -> SandboxStartup {
        match (explicit, saved) {
            (Some(x), Some(s))
                if x.parse::<xai_grok_sandbox::ProfileName>().ok()
                    != s.parse::<xai_grok_sandbox::ProfileName>().ok() =>
            {
                SandboxStartup::Conflict {
                    requested: x.to_owned(),
                    saved: s,
                }
            }
            (Some(x), _) => SandboxStartup::Apply(Some(x.to_owned())),
            (None, saved) => SandboxStartup::Apply(saved),
        }
    }
    /// The initial interactive prompt from the positional argument, trimmed.
    ///
    /// Returns `None` when no positional prompt was given or it is only
    /// whitespace. This is the `grok "<prompt>"` launch form; the headless
    /// `-p`/`--single` path is handled separately.
    pub fn initial_prompt(&self) -> Option<&str> {
        self.prompt
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn root_help_uses_chaos_brand_and_chinese_copy() {
        let mut command = PagerArgs::command();
        let mut help = Vec::new();
        command.write_long_help(&mut help).unwrap();
        let help = String::from_utf8(help).unwrap();
        assert!(help.contains("Chaos AI 编码助手"), "{help}");
        assert!(help.contains("参数："), "{help}");
        assert!(help.contains("用法："), "{help}");
        assert!(help.contains("选项："), "{help}");
        assert!(help.contains("列出可用模型并退出"), "{help}");
        assert!(!help.contains("Grok Build TUI"), "{help}");
    }

    #[test]
    fn version_flags_parse_as_early_intent_without_exiting() {
        for flag in ["--version", "-v", "-V"] {
            let args = PagerArgs::try_parse_from(["grok", flag]).expect("version flag parses");
            assert!(args.version, "{flag} must set the early version intent");
            assert!(args.command.is_none());
        }
    }
    #[test]
    fn ordinary_and_doctor_parsing_do_not_set_version_intent() {
        assert!(!PagerArgs::try_parse_from(["grok"]).unwrap().version);
        assert!(
            !PagerArgs::try_parse_from(["grok", "doctor"])
                .unwrap()
                .version
        );
        assert!(matches!(
            PagerArgs::try_parse_from(["grok", "version"])
                .unwrap()
                .command,
            Some(Command::Version { json: false })
        ));
    }
    #[test]
    fn doctor_accepts_report_and_explicit_fix_forms() {
        let bare = PagerArgs::try_parse_from(["grok", "doctor"]).expect("bare doctor parses");
        assert!(matches!(
            bare.command,
            Some(Command::Doctor(crate::doctor_cmd::DoctorArgs {
                json: false,
                command: None,
            }))
        ));
        let json =
            PagerArgs::try_parse_from(["grok", "doctor", "--json"]).expect("doctor --json parses");
        assert!(matches!(
            json.command,
            Some(Command::Doctor(crate::doctor_cmd::DoctorArgs {
                json: true,
                command: None,
            }))
        ));
        let fix =
            PagerArgs::try_parse_from(["grok", "doctor", "fix", "terminal.ssh-wrap", "--yes"])
                .expect("doctor fix parses");
        assert!(
            matches!(fix.command, Some(Command::Doctor(crate ::doctor_cmd::DoctorArgs {
            json : false, command : Some(crate ::doctor_cmd::DoctorCommand::Fix(crate
            ::doctor_cmd::FixArgs { ref id, yes : true })), })) if id ==
            "terminal.ssh-wrap")
        );
        for unsupported in [
            vec!["grok", "doctor", "fix"],
            vec!["grok", "doctor", "all"],
            vec!["grok", "doctor", "--json", "fix", "terminal.ssh-wrap"],
        ] {
            let error = PagerArgs::try_parse_from(unsupported)
                .expect_err("unsupported doctor form must fail");
            assert_eq!(error.exit_code(), 2);
        }
    }
    #[test]
    fn resume_target_classifies_flags() {
        assert_eq!(
            PagerArgs::try_parse_from(["grok"]).unwrap().resume_target(),
            ResumeTarget::None
        );
        assert_eq!(
            PagerArgs::try_parse_from(["grok", "-c"])
                .unwrap()
                .resume_target(),
            ResumeTarget::MostRecentForCwd
        );
        assert_eq!(
            PagerArgs::try_parse_from(["grok", "--resume"])
                .unwrap()
                .resume_target(),
            ResumeTarget::MostRecentForCwd
        );
        assert_eq!(
            PagerArgs::try_parse_from(["grok", "--resume", "sess-1"])
                .unwrap()
                .resume_target(),
            ResumeTarget::SessionId("sess-1".to_string())
        );
        assert_eq!(
            PagerArgs::try_parse_from(["grok", "-s", "sess-2"])
                .unwrap()
                .resume_target(),
            ResumeTarget::None
        );
        assert_eq!(
            PagerArgs::try_parse_from(["grok", "-r", "old", "--fork-session"])
                .unwrap()
                .resume_target(),
            ResumeTarget::SessionId("old".to_string())
        );
    }
    /// The screen-mode flags are mutually exclusive: the pair exists so one
    /// can override the other's sticky config value, so accepting both in one
    /// invocation would be ambiguous.
    #[test]
    fn minimal_and_fullscreen_flags_conflict() {
        let args = PagerArgs::try_parse_from(["grok", "--minimal"]).unwrap();
        assert!(args.minimal && !args.fullscreen);
        let args = PagerArgs::try_parse_from(["grok", "--fullscreen"]).unwrap();
        assert!(args.fullscreen && !args.minimal);
        let err = PagerArgs::try_parse_from(["grok", "--minimal", "--fullscreen"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }
    #[test]
    fn agent_plugin_dir_repeatable_and_canonicalized() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("plugin");
        std::fs::create_dir(&dir).unwrap();
        let file = tmp.path().join("file.txt");
        std::fs::write(&file, "x").unwrap();
        let missing = tmp.path().join("missing");
        let args = PagerArgs::try_parse_from([
            "grok".as_ref(),
            "agent".as_ref(),
            "--no-leader".as_ref(),
            "--plugin-dir".as_ref(),
            dir.as_os_str(),
            "--plugin-dir".as_ref(),
            file.as_os_str(),
            "--plugin-dir".as_ref(),
            missing.as_os_str(),
            "stdio".as_ref(),
        ])
        .unwrap();
        let Some(Command::Agent(agent)) = args.command else {
            panic!("expected agent subcommand");
        };
        assert_eq!(agent.plugin_dirs, vec![dir.clone(), file, missing]);
        assert!(matches!(agent.mode, Some(AgentCmd::Stdio)));
        assert!(agent.no_leader);
        assert_eq!(
            agent.canonical_plugin_dirs(),
            vec![dunce::canonicalize(&dir).unwrap()]
        );
    }
    #[test]
    fn resolve_startup_sandbox_cases() {
        use SandboxStartup::{Apply, Conflict};
        assert_eq!(
            PagerArgs::resolve_startup_sandbox(Some("strict"), None),
            Apply(Some("strict".to_string()))
        );
        assert_eq!(
            PagerArgs::resolve_startup_sandbox(Some("workspace"), Some("workspace".to_string())),
            Apply(Some("workspace".to_string()))
        );
        assert_eq!(
            PagerArgs::resolve_startup_sandbox(Some("read-only"), Some("workspace".to_string())),
            Conflict {
                requested: "read-only".to_string(),
                saved: "workspace".to_string(),
            }
        );
        assert_eq!(
            PagerArgs::resolve_startup_sandbox(None, Some("workspace".to_string())),
            Apply(Some("workspace".to_string()))
        );
        assert_eq!(PagerArgs::resolve_startup_sandbox(None, None), Apply(None));
        assert_eq!(
            PagerArgs::resolve_startup_sandbox(Some("readonly"), Some("read-only".to_string())),
            Apply(Some("readonly".to_string()))
        );
        assert_eq!(
            PagerArgs::resolve_startup_sandbox(Some("none"), Some("off".to_string())),
            Apply(Some("none".to_string()))
        );
    }
    #[test]
    fn startup_sandbox_profile_no_resume() {
        assert_eq!(
            PagerArgs::try_parse_from(["grok", "--sandbox", "strict"])
                .unwrap()
                .startup_sandbox_profile(None),
            SandboxStartup::Apply(Some("strict".to_string()))
        );
        assert_eq!(
            PagerArgs::try_parse_from(["grok", "--sandbox", ""])
                .unwrap()
                .startup_sandbox_profile(None),
            SandboxStartup::Apply(None)
        );
        assert_eq!(
            PagerArgs::try_parse_from(["grok"])
                .unwrap()
                .startup_sandbox_profile(None),
            SandboxStartup::Apply(None)
        );
    }
    #[test]
    fn launch_directory_anchoring_precedes_cwd_change() {
        let args = PagerArgs::try_parse_from([
            "grok",
            "--leader-socket",
            "relative.sock",
            "--debug-file",
            "relative.log",
        ])
        .unwrap()
        .apply_cwd_from(Some(std::path::Path::new("/launch")))
        .unwrap();
        assert_eq!(
            args.leader_socket.as_deref(),
            Some(std::path::Path::new("/launch/relative.sock"))
        );
        assert_eq!(
            args.debug_file.as_deref(),
            Some(std::path::Path::new("/launch/relative.log"))
        );
    }
    #[test]
    fn launch_directory_anchoring_normalizes_dot_components() {
        for (input, expected) in [
            ("./leader.sock", "/launch/leader.sock"),
            ("logs/../debug.log", "/launch/logs/../debug.log"),
            ("../leader.sock", "/launch/../leader.sock"),
        ] {
            assert_eq!(
                anchor_to_launch_dir(PathBuf::from(input), Some(std::path::Path::new("/launch"))),
                PathBuf::from(expected),
                "input: {input}"
            );
        }
    }
    #[test]
    fn leader_socket_flag_parses_at_root() {
        let args = PagerArgs::try_parse_from(["grok", "--leader-socket", "/tmp/leader-x.sock"])
            .expect("--leader-socket parses at the root");
        assert_eq!(
            args.leader_socket.as_deref(),
            Some(std::path::Path::new("/tmp/leader-x.sock"))
        );
    }
    #[test]
    fn leader_socket_flag_is_global_for_subcommands() {
        let args = PagerArgs::try_parse_from([
            "grok",
            "agent",
            "leader",
            "--leader-socket",
            "/tmp/leader-y.sock",
        ])
        .expect("--leader-socket parses after a subcommand (global)");
        assert_eq!(
            args.leader_socket.as_deref(),
            Some(std::path::Path::new("/tmp/leader-y.sock"))
        );
    }
    #[test]
    fn leader_socket_flag_defaults_to_none() {
        let args = PagerArgs::try_parse_from(["grok"]).expect("bare grok parses");
        assert!(args.leader_socket.is_none());
    }
    #[test]
    fn leader_mgmt_list_info_kill_parse() {
        let list = PagerArgs::try_parse_from(["grok", "leader", "list", "--json"])
            .expect("grok leader list --json");
        assert!(matches!(
            list.command,
            Some(Command::Leader(LeaderMgmtArgs {
                command: LeaderMgmtCommand::List { json: true },
            }))
        ));
        let info = PagerArgs::try_parse_from(["grok", "leader", "info", "--pid", "42"])
            .expect("grok leader info --pid");
        assert!(matches!(
            info.command,
            Some(Command::Leader(LeaderMgmtArgs {
                command: LeaderMgmtCommand::Info {
                    target: LeaderTargetArgs { pid: Some(42) },
                    json: false,
                },
            }))
        ));
        let kill = PagerArgs::try_parse_from(["grok", "leader", "kill"]).expect("grok leader kill");
        assert!(matches!(
            kill.command,
            Some(Command::Leader(LeaderMgmtArgs {
                command: LeaderMgmtCommand::Kill,
            }))
        ));
        assert!(PagerArgs::try_parse_from(["grok", "leader", "profile"]).is_err());
    }
    #[test]
    fn debug_file_flag_parses_and_is_global() {
        let root = PagerArgs::try_parse_from(["grok", "--debug-file", "/tmp/fire.txt"])
            .expect("--debug-file parses at the root");
        assert_eq!(
            root.debug_file.as_deref(),
            Some(std::path::Path::new("/tmp/fire.txt"))
        );
        let sub =
            PagerArgs::try_parse_from(["grok", "agent", "stdio", "--debug-file", "/tmp/f.txt"])
                .expect("--debug-file parses after a subcommand (global)");
        assert_eq!(
            sub.debug_file.as_deref(),
            Some(std::path::Path::new("/tmp/f.txt"))
        );
    }
    #[test]
    fn debug_file_flag_defaults_to_none() {
        let args = PagerArgs::try_parse_from(["grok"]).expect("bare grok parses");
        assert!(args.debug_file.is_none());
    }
    #[test]
    fn positional_prompt_seeds_interactive_session() {
        let args =
            PagerArgs::try_parse_from(["grok", "fix the bug"]).expect("positional prompt parses");
        assert_eq!(args.initial_prompt(), Some("fix the bug"));
        assert!(args.command.is_none());
        assert!(args.single.is_none());
    }
    #[test]
    fn bare_grok_has_no_initial_prompt() {
        let args = PagerArgs::try_parse_from(["grok"]).expect("bare grok parses");
        assert_eq!(args.initial_prompt(), None);
    }
    #[test]
    fn initial_prompt_trims_and_ignores_whitespace_only() {
        let args = PagerArgs::try_parse_from(["grok", "  spaced  "]).expect("padded prompt parses");
        assert_eq!(args.initial_prompt(), Some("spaced"));
        let blank = PagerArgs::try_parse_from(["grok", "   "]).expect("blank prompt parses");
        assert_eq!(blank.initial_prompt(), None);
    }
    #[test]
    fn positional_prompt_conflicts_with_headless_single() {
        let err = PagerArgs::try_parse_from(["grok", "-p", "headless", "interactive"])
            .expect_err("positional prompt + --single must conflict");
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }
    #[test]
    fn worktree_flag_and_initial_prompt_combine() {
        let a = PagerArgs::try_parse_from(["grok", "do the thing", "-w"])
            .expect("prompt then bare -w parses");
        assert_eq!(a.initial_prompt(), Some("do the thing"));
        assert_eq!(a.worktree.as_deref(), Some(""));
        let b = PagerArgs::try_parse_from(["grok", "--worktree=feat", "do the thing"])
            .expect("--worktree=name + positional parses");
        assert_eq!(b.initial_prompt(), Some("do the thing"));
        assert_eq!(b.worktree.as_deref(), Some("feat"));
        let c = PagerArgs::try_parse_from(["grok", "-w", "x"]).expect("-w x parses");
        assert_eq!(c.worktree.as_deref(), Some("x"));
        assert_eq!(c.initial_prompt(), None);
    }
    #[test]
    fn trust_flag_parses_on_pager_and_alias() {
        let bare = PagerArgs::try_parse_from(["grok"]).expect("bare grok parses");
        assert!(!bare.trust);
        let long = PagerArgs::try_parse_from(["grok", "--trust"]).expect("--trust parses");
        assert!(long.trust);
        let alias =
            PagerArgs::try_parse_from(["grok", "--trust-folder"]).expect("--trust-folder parses");
        assert!(alias.trust);
    }
    #[test]
    fn reasoning_effort_and_effort_alias_parse_same_field() {
        let long = PagerArgs::try_parse_from(["grok", "--reasoning-effort", "high"])
            .expect("--reasoning-effort parses");
        assert_eq!(long.reasoning_effort.as_deref(), Some("high"));
        let alias =
            PagerArgs::try_parse_from(["grok", "--effort", "high"]).expect("--effort alias parses");
        assert_eq!(alias.reasoning_effort.as_deref(), Some("high"));
    }
    #[test]
    fn reasoning_effort_accepts_max_and_remapped_ids() {
        let max = PagerArgs::try_parse_from(["grok", "--effort", "max"]).expect("max parses");
        assert_eq!(max.reasoning_effort.as_deref(), Some("max"));
        let deep =
            PagerArgs::try_parse_from(["grok", "--reasoning-effort", "deep"]).expect("deep parses");
        assert_eq!(deep.reasoning_effort.as_deref(), Some("deep"));
    }
    #[test]
    fn reasoning_effort_last_flag_wins_when_both_names_set() {
        let args =
            PagerArgs::try_parse_from(["grok", "--reasoning-effort", "low", "--effort", "high"])
                .expect("both effort flag names parse");
        assert_eq!(args.reasoning_effort.as_deref(), Some("high"));
        let reverse =
            PagerArgs::try_parse_from(["grok", "--effort", "high", "--reasoning-effort", "low"])
                .expect("both effort flag names parse (reverse order)");
        assert_eq!(reverse.reasoning_effort.as_deref(), Some("low"));
    }
    #[test]
    fn agent_args_effort_alias_parses() {
        let args = PagerArgs::try_parse_from(["grok", "agent", "--effort", "max", "stdio"])
            .expect("agent --effort parses");
        let Command::Agent(agent) = args.command.expect("agent subcommand") else {
            panic!("expected agent subcommand");
        };
        assert_eq!(agent.reasoning_effort.as_deref(), Some("max"));
    }
}
