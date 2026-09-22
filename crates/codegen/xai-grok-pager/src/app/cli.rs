pub use crate::headless::OutputFormat;
use clap::{ArgAction, Parser, Subcommand, ValueHint};
use clap_complete::Shell;
use std::net::SocketAddr;
use std::path::PathBuf;
/// Top-level commands for the pager binary.
#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// 无需交互式界面运行 Chaos
    Agent(Box<AgentArgs>),
    /// 显示 Chaos 为此目录发现的配置
    Inspect {
        /// 输出机器可读的 JSON。
        #[arg(long)]
        json: bool,
    },
    /// 在不启动 Chaos 的情况下检查终端、剪贴板、颜色和输入支持
    Doctor(crate::doctor_cmd::DoctorArgs),
    /// 管理正在运行的 leader 进程
    Leader(LeaderMgmtArgs),
    /// 说明无需登出：Chaos 使用 config.toml 中的自带凭证
    Logout,
    /// 配置模型提供方（请在 config.toml 中设置；此分支不登录 xAI）
    Login {
        /// 已忽略（为向后兼容保留）。现在请在 config.toml 中配置模型提供方。
        #[arg(long, hide = true)]
        legacy: bool,
        /// 已忽略（为向后兼容保留）。本分支不进行 xAI 登录。
        #[arg(long = "oauth", alias = "oidc", conflicts_with_all = ["device_auth"])]
        oauth: bool,
        /// 已忽略（为向后兼容保留）。本分支不进行设备码登录。
        #[arg(
            long = "device-auth",
            visible_alias = "device-code",
            conflicts_with_all = ["oauth"]
        )]
        device_auth: bool,
        /// 为远程开发环境进行认证（隐藏）。
        ///
        /// 该字段始终存在，以便各 match 分支在 Bazel/cargo 依赖图中保持
        /// feature 合并安全；仅当启用 `devbox-login` 时 clap 才注册 `--devbox`
        /// （否则为 `arg(skip)` → 恒为 false）。
        #[arg(skip)]
        devbox: bool,
    },
    /// 管理 MCP 服务器配置
    Mcp(crate::mcp_cmd::McpArgs),
    /// 管理插件与市场源
    Plugin(crate::plugin_cmd::PluginArgs),
    /// 管理跨会话记忆
    Memory(crate::memory_cmd::MemoryArgs),
    /// 列出可用模型并退出
    Models,
    /// 列出可用的请求客户端配置并退出
    Clients {
        /// 输出机器可读的 JSON。
        #[arg(long)]
        json: bool,
    },
    /// 列出、搜索或恢复会话
    Sessions(crate::sessions_cmd::SessionsArgs),
    /// 打印某会话持久化的 token 与费用用量
    Usage(crate::usage_cmd::UsageArgs),
    /// 获取并安装托管配置
    Setup {
        /// 以 JSON 形式打印获取到的配置而不安装；不会写入 ~/.chaos。
        #[arg(long)]
        json: bool,
    },
    /// 分享会话并打印分享 URL
    #[command(hide = true)]
    Share(crate::share_cmd::ShareArgs),
    /// 在本地剪贴板支持下运行任意命令（将 OSC 52 转发到系统剪贴板）。
    #[cfg_attr(not(any(unix, windows)), command(hide = true))]
    #[command(long_about = "\
在本地 PTY 中运行任意命令，并将其剪贴板转发到你的剪贴板。

将任意命令（例如 `docker exec`、`kubectl exec` 或远程 shell）包装在本地伪终端中，
拦截其输出中的 OSC 52 剪贴板转义序列，并写入你本地的系统剪贴板。这样，当程序
运行在无法访问你剪贴板的位置（容器、SSH）且你的终端本身不处理 OSC 52（例如
Apple Terminal）时，复制仍能正常工作。被包装命令的终端也会与你的窗口大小保持
同步。

示例：
  chaos wrap docker exec -it my-container bash
  chaos wrap kubectl exec -it my-pod -- bash

更多信息请参见 ~/.chaos/README.md。
")]
    Wrap(WrapArgs),
    /// 将会话记录导出为 Markdown
    Export(crate::export_cmd::ExportArgs),
    /// 导出或上传会话追踪数据
    Trace(crate::trace_cmd::TraceArgs),
    /// 检查更新或安装指定版本
    Update {
        /// 只检查更新，不安装。
        #[arg(long)]
        check: bool,
        /// 输出机器可读的 JSON（用于 --check）。
        #[arg(long)]
        json: bool,
        /// 即使已是最新版本也强制重新下载并安装。
        #[arg(long)]
        force_reinstall: bool,
        /// 安装指定版本（例如 0.1.150 或 0.1.151-alpha.2）。
        #[arg(long)]
        version: Option<String>,
        /// 切换到 alpha 发布通道（更新更快，可能有 bug）。
        #[arg(long, conflicts_with_all = ["stable", "enterprise"])]
        alpha: bool,
        /// 切换到 stable 发布通道（默认，每周发布）。
        #[arg(long, conflicts_with_all = ["alpha", "enterprise"])]
        stable: bool,
        /// 切换到 enterprise 发布通道。
        #[arg(long, conflicts_with_all = ["alpha", "stable"], hide = true)]
        enterprise: bool,
        /// 内部使用：是什么触发了此 `chaos update`（`user_command`、`auto_background`、`leader_converge`）。隐藏。
        #[arg(long, hide = true)]
        trigger: Option<String>,
        /// `--trigger=auto_background` 的内部兼容别名（旧版父进程仍会用它启动子进程）。
        #[arg(long, hide = true)]
        auto: bool,
    },
    /// 打印版本信息
    #[command(visible_alias = "v")]
    Version {
        /// 输出机器可读的 JSON。
        #[arg(long)]
        json: bool,
    },
    /// 生成 shell 补全脚本（bash、zsh、fish、powershell 等）
    Completions {
        /// 目标 shell
        #[arg(value_enum)]
        shell: Shell,
    },
    /// 管理 git worktree
    Worktree(crate::worktree_cmd::WorktreeArgs),
    /// 显示 Chaos 主目录（~/.chaos）在磁盘上的占用
    #[command(name = "du", visible_alias = "disk-usage")]
    DiskUsage(crate::disk_usage_cmd::DiskUsageArgs),
    /// 将本工作区暴露给 Computer Hub（通过 leader）。
    ///
    /// 默认禁用，由服务器端按账号启用；设置 `GROK_WORKSPACE_COMMAND=1` 可在本地启用以便测试。
    #[command(hide = true)]
    Workspace(WorkspaceMgmtArgs),
    /// 启动时打开 Agent Dashboard 视图。
    ///
    /// Dashboard 显示每个会话，包括顶层会话和子 agent。
    /// 当 `~/.chaos/config.toml` 中设置 `[dashboard].enabled = false`，或设置了 `GROK_AGENT_DASHBOARD=0` 环境变量时禁用。
    Dashboard,
}
/// Arguments for the `wrap` subcommand: the command to run, then its args.
#[derive(Debug, clap::Args, Clone)]
pub struct WrapArgs {
    /// 要运行的命令，后跟其参数（例如 `docker exec -it my-container bash`）。
    /// 在 Unix 上，单个带引号的字符串或别名命令会通过 `$SHELL -i -c` 运行。
    #[arg(
        required = true,
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "CMD"
    )]
    pub command: Vec<String>,
}
/// Targets a running leader process by PID (used by `chaos leader` / `chaos workspace`).
#[derive(Debug, clap::Args, Clone, Default)]
pub struct LeaderTargetArgs {
    /// 来自 `chaos leader list` 的 leader 进程 ID。
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
    /// 列出正在运行的 leader 进程
    List {
        /// 输出机器可读的 JSON。
        #[arg(long)]
        json: bool,
    },
    /// 显示某个 leader 进程的详情
    Info {
        #[command(flatten)]
        target: LeaderTargetArgs,
        /// 输出机器可读的 JSON。
        #[arg(long)]
        json: bool,
    },
    /// 停止所有正在运行的 leader 进程
    Kill,
}
#[derive(Debug, clap::Args, Clone)]
pub struct WorkspaceMgmtArgs {
    #[command(subcommand)]
    pub command: WorkspaceMgmtCommand,
}
#[derive(Debug, Subcommand, Clone)]
pub enum WorkspaceMgmtCommand {
    /// 启动（或更新）工作区到 hub 的暴露。
    Start(WorkspaceStartArgs),
    /// 排空并断开与 hub 的连接，同时保持暴露处于待命状态。
    Pause {
        #[command(flatten)]
        target: LeaderTargetArgs,
        /// 输出机器可读的 JSON。
        #[arg(long)]
        json: bool,
    },
    /// 将已暂停的暴露重新连接到 hub。
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
    /// 重启暴露（先停止，再用给定选项启动）。
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
    /// Computer Hub WebSocket URL（默认：`[hub].url`，否则使用生产 hub）。
    #[arg(long, value_name = "URL")]
    pub hub_url: Option<String>,
    /// 要暴露的工作区根目录。默认为当前目录。
    #[arg(long, value_name = "DIR", value_hint = ValueHint::DirPath)]
    pub cwd: Option<PathBuf>,
    /// 为此命令强制使用 leader 模式，覆盖配置。
    #[arg(long, conflicts_with = "no_leader")]
    pub leader: bool,
    /// 即使配置启用了 leader 模式也拒绝启动。
    #[arg(long, conflicts_with = "leader")]
    pub no_leader: bool,
    /// 输出机器可读的 JSON。
    #[arg(long)]
    pub json: bool,
}
/// Arguments for the `agent` subcommand.
#[derive(Debug, clap::Args, Clone)]
pub struct AgentArgs {
    /// 在启动 agent 前先运行认证
    #[arg(
        long = "reauth",
        visible_alias = "--reauthenticate",
        default_value = "false"
    )]
    pub reauthenticate: bool,
    /// 要使用的模型 ID
    #[arg(short = 'm', long = "model", value_name = "MODEL")]
    pub model: Option<String>,
    /// 选择请求客户端配置（claude-code、codex、grok-build 或 workbuddy）。
    #[arg(long = "client", value_name = "PROFILE")]
    pub client: Option<String>,
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
    /// agent 配置文件路径。
    #[arg(long = "agent-profile", value_name = "PATH")]
    pub agent_profile: Option<PathBuf>,
    /// 按名称选择一个 agent 预设（standard、minimal、explore、plan 等）。
    /// 预设打包了一套工具集、人设和提示词段落。当两者同时给出时，它会遮蔽
    /// `--agent-profile`。可在会话中途通过 `/preset` 切换（仅限空白会话）。
    #[arg(long = "preset", value_name = "PRESET")]
    pub preset: Option<String>,
    /// 仅为此进程从该目录加载插件（可重复）。
    /// 最高优先级的插件作用域；始终受信任：hooks 和 MCP 服务器会无需提示即激活。
    /// 由 Agent SDK 用于注入按连接区分的插件。
    #[arg(long = "plugin-dir", value_name = "DIR", value_hint = ValueHint::DirPath)]
    pub plugin_dirs: Vec<PathBuf>,
    /// 连接到共享的 leader 进程，而不是启动新的 agent。
    /// 允许多个客户端共享同一个后端。
    /// 默认为 config.toml 中的 [cli] use_leader。
    #[arg(long, conflicts_with = "no_leader")]
    pub leader: bool,
    /// 即使配置启用了 leader 模式也启动新的 agent。
    #[arg(long, conflicts_with = "leader")]
    pub no_leader: bool,
    #[command(flatten)]
    pub headless: HeadlessArgs,
    /// 覆盖 CLI 聊天代理的基础 URL。
    #[arg(long = "cli-chat-proxy-base-url")]
    pub cli_chat_proxy_base_url: Option<String>,
    /// 覆盖公共 xAI API 的基础 URL。
    #[arg(long = "xai-api-base-url")]
    pub xai_api_base_url: Option<String>,
    /// agent 运行时模式
    #[command(subcommand)]
    pub mode: Option<AgentCmd>,
}
impl AgentArgs {
    /// Canonicalized `--plugin-dir` paths, warning to stderr and skipping anything that isn't an existing directory.
    /// stderr is safe: JSON-RPC uses stdout.
    pub fn canonical_plugin_dirs(&self) -> Vec<PathBuf> {
        self.plugin_dirs
            .iter()
            .filter_map(|p| match dunce::canonicalize(p) {
                Ok(canonical) if canonical.is_dir() => Some(canonical),
                Ok(_) => {
                    eprintln!(
                        "chaos: --plugin-dir {}: not a directory; skipping",
                        p.display()
                    );
                    None
                }
                Err(e) => {
                    eprintln!("chaos: --plugin-dir {}: {e}; skipping", p.display());
                    None
                }
            })
            .collect()
    }
}
/// Agent sub-subcommands.
#[derive(Debug, Subcommand, Clone)]
pub enum AgentCmd {
    /// 通过 stdio 运行 agent
    Stdio,
    /// 通过 Grok WebSocket 中继以无头方式运行 agent
    Headless(HeadlessArgs),
    /// 将 agent 作为 WebSocket 服务器运行
    Serve(ServeArgs),
    /// 作为供其他客户端共享的 leader 进程运行
    Leader(LeaderArgs),
}
/// WebSocket URL override arguments, used by headless / leader / serve modes.
#[derive(Debug, clap::Args, Clone, Default)]
pub struct HeadlessArgs {
    #[arg(long = "grok-ws-origin")]
    pub grok_ws_origin: Option<String>,
    #[arg(long = "grok-ws-url")]
    pub grok_ws_url: Option<String>,
}
/// Arguments for the `agent serve` subcommand.
#[derive(Debug, clap::Args, Clone)]
pub struct ServeArgs {
    /// 服务器监听的地址
    #[arg(long, default_value = "127.0.0.1:2419")]
    pub bind: SocketAddr,
    /// 用于客户端认证的密钥令牌（未提供时自动生成）
    #[arg(long, env = "GROK_AGENT_SECRET")]
    pub secret: Option<String>,
    /// 代理模式下的远程 agent URL
    #[arg(long)]
    pub remote: Option<String>,
    /// 认证与 WebSocket URL 覆盖项
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
/// Arguments for the `agent leader` subcommand.
#[derive(Debug, clap::Args, Clone)]
pub struct LeaderArgs {
    /// 在最后一个客户端断开后保持 leader 继续运行。
    #[arg(long)]
    pub no_exit_on_disconnect: bool,
    /// 将 grok.com 中继 WebSocket 推迟到第一个无头 IPC 客户端注册时。
    /// 不带此标志时，leader 会在启动时立即连接中继。
    /// 裸 leader（无头远程环境 / systemd）需要立即连接：它们通过中继*接收*远程提示词。
    /// 由交互式客户端（TUI/IDE）自动启动的 leader 会传入此项，它们仅在出现无头客户端时才需要中继。
    #[arg(long)]
    pub relay_on_demand: bool,
    /// 禁用 leader 的周期性自动更新检查。
    #[arg(long)]
    pub no_auto_update: bool,
    /// 所有环境 URL 覆盖项（由 follower 进程传入）
    #[command(flatten)]
    pub headless: HeadlessArgs,
}
#[derive(Debug, Clone, Parser)]
#[command(
    name = "chaos",
    version = xai_grok_version::full_version(),
    about = "Chaos AI 编码助手",
    disable_version_flag = true,
    next_display_order = None,
    help_template = "\
{before-help}{about-with-newline}
{usage-heading} {usage}

Arguments:
{positionals}

Options:
{options}

Commands:
{subcommands}{after-help}\
"
)]
pub struct PagerArgs {
    /// 打印版本
    #[arg(short = 'v', short_alias = 'V', long = "version", action = ArgAction::SetTrue)]
    pub version: bool,
    /// 工作目录。
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    /// 使用自定义 leader socket 路径，替代默认的 `~/.chaos/leader.sock`。
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
    /// 自动批准所有工具执行。
    #[clap(
        long = "always-approve",
        alias = "yolo",
        alias = "dangerously-skip-permissions"
    )]
    pub yolo: bool,
    /// 信任此文件夹并将该决定持久化到信任存储。
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
    /// 单轮提示词。将响应打印到 stdout 后退出。
    #[clap(
        short = 'p',
        long = "single",
        alias = "print",
        value_name = "PROMPT",
        conflicts_with_all = &["prompt_json",
        "prompt_file"]
    )]
    pub single: Option<String>,
    /// 以 JSON 内容块表示的单轮提示词。
    #[clap(
        long = "prompt-json",
        value_name = "JSON",
        conflicts_with_all = &["single",
        "prompt_file"]
    )]
    pub prompt_json: Option<String>,
    /// 从文件读取的单轮提示词。
    #[clap(
        long = "prompt-file",
        value_name = "PATH",
        conflicts_with_all = &["single",
        "prompt_json"],
        value_hint = ValueHint::FilePath
    )]
    pub prompt_file: Option<PathBuf>,
    /// 按原样发送提示词。
    #[clap(long)]
    pub verbatim: bool,
    /// 无头模式的输出格式。
    #[clap(long = "output-format", value_enum, default_value = "plain")]
    pub output_format: OutputFormat,
    /// 在完整消息之外额外输出增量 `stream_event` 行（文本/思考的增量）。
    /// 仅影响 `--output-format streaming-messages-json`。
    #[clap(long = "include-partial-messages")]
    pub include_partial_messages: bool,
    /// 用于结构化输出的 JSON Schema。设置后，模型将被约束为生成匹配此 schema 的 JSON。
    /// 隐含 --output-format json。
    /// 示例：--json-schema '{"type":"object","properties":{"name":{"type":"string"}}}'
    #[clap(long = "json-schema", value_name = "SCHEMA")]
    pub json_schema: Option<String>,
    /// 要使用的模型 ID。
    #[clap(short = 'm', long = "model", value_name = "MODEL")]
    pub model: Option<String>,
    /// 选择请求客户端配置（claude-code、codex、grok-build 或 workbuddy）。
    #[clap(long = "client", value_name = "PROFILE")]
    pub client: Option<String>,
    /// 推理模型的推理强度
    #[clap(
        long = "reasoning-effort",
        visible_alias = "effort",
        value_name = "EFFORT",
        overrides_with = "reasoning_effort"
    )]
    pub reasoning_effort: Option<String>,
    /// 追加到系统提示词的额外规则。
    #[clap(long = "rules", alias = "append-system-prompt")]
    pub rules: Option<String>,
    /// 压缩模式 [summary|transcript|segments]。
    /// `summary` 不添加指针；`transcript` 指向原始记录；`segments`（默认）以逐段 markdown 持久化以便 grep。
    /// 设置 `GROK_COMPACTION_MODE`。
    #[clap(long = "compaction-mode", value_name = "MODE", hide = true)]
    pub compaction_mode: Option<String>,
    /// 段的逐字详细程度 [none|minimal|balanced|verbose]（默认 `verbose`）。
    /// 仅影响 `--compaction-mode segments`。设置 `GROK_COMPACTION_DETAIL`。
    #[clap(long = "compaction-detail", value_name = "DETAIL", hide = true)]
    pub compaction_detail: Option<String>,
    /// 覆盖 agent 的系统提示词（兼容别名：--system-prompt）。
    #[clap(
        long = "system-prompt-override",
        alias = "system-prompt",
        value_name = "PROMPT"
    )]
    pub system_prompt_override: Option<String>,
    /// 按 ID 或标题恢复会话；省略时恢复最近一个。
    /// 非 ID 值会匹配当前目录下的会话标题，忽略字母大小写；UUID 形式的值始终表示 ID。
    /// 标题重复时，若只有一个经过重命名的匹配项则其胜出；否则恢复会因歧义而失败。
    #[arg(
        long = "resume",
        short = 'r',
        value_name = "SESSION_ID_OR_TITLE",
        num_args = 0..= 1,
        default_missing_value = "",
        conflicts_with_all = ["continue_last_session"]
    )]
    pub resume_session: Option<String>,
    /// 按会话 ID 恢复先前的会话（--resume 的别名）。
    #[arg(
        long = "load",
        value_name = "SESSION_ID",
        hide = true,
        conflicts_with_all = ["continue_last_session"]
    )]
    pub load_session: Option<String>,
    /// Set by [`Self::pin_local_resume_target`]: the resume target was resolved (or definitively missed) before the OS sandbox.
    /// Materialization must therefore not re-run local title selection.
    #[clap(skip)]
    pub resume_target_pinned: bool,
    /// Sandbox profile of the title-pinned session, captured at pin time from the selected summary (outer `None` means no title pin happened).
    /// The id-based peek cannot re-derive it: a legacy id duplicated across cwd dirs makes that lookup ambiguous.
    #[clap(skip)]
    pub(crate) pinned_resume_profile: Option<Option<String>>,
    /// 继续当前工作目录下最近的会话。
    #[arg(
        short = 'c',
        long = "continue",
        conflicts_with_all = ["resume_session",
        "load_session"]
    )]
    pub continue_last_session: bool,
    /// 为**新**对话指定特定的会话 UUID（必须是有效的 UUID，且目标会话目录下不能已存在）。
    /// 与 `--resume`/`--continue` 一起使用时，仅在与 `--fork-session` 搭配时有效（用于命名分叉出的会话）。
    /// 不会恢复已有会话，请改用 `--resume` / `--continue`。
    #[arg(short = 's', long = "session-id", value_name = "SESSION_ID")]
    pub session_id: Option<String>,
    /// 恢复会话（`--resume` / `--continue`）时，创建新的会话 ID 而不是复用原有的（可用 `--session-id` 指定）。
    #[arg(long = "fork-session")]
    pub fork_session: bool,
    /// 在新的 git worktree 中启动会话，可选命名。
    /// 恢复远程会话（`--resume`）时，传入 `--restore-code` 以应用快照代码库（两种情况下对话都会被恢复）。
    /// 无头模式（`-p`）不会因该标志创建 worktree。
    #[arg(short = 'w', long = "worktree", num_args = 0..= 1, default_missing_value = "")]
    pub worktree: Option<String>,
    /// worktree 所基于的分支、标签或提交（配合 `--worktree`）。
    /// 省略时默认为源检出的当前 HEAD。
    #[arg(long = "worktree-ref", visible_alias = "ref", requires = "worktree")]
    pub worktree_ref: Option<String>,
    /// 恢复时还原本会话所属仓库的快照。
    /// 远程会话要求使用 `--worktree`（绝不会检出到当前目录）。
    /// 不带此标志时，恢复仅还原对话。
    #[arg(long = "restore-code", requires = "resume_session")]
    pub restore_code: bool,
    /// 禁用计划模式。
    #[arg(long = "no-plan")]
    pub no_plan: bool,
    /// 拥有一个本地 `workspace_server`（替代远程沙箱）。要求 `--chat`。
    ///
    /// 仅在启用 `--features local-workspace` 时编译（`chat` 不会隐含启用）。
    #[cfg(feature = "local-workspace")]
    #[arg(
        long = "local-workspace",
        num_args = 0..= 1,
        value_name = "CWD",
        conflicts_with = "local_workspace_attach",
        requires = "chat"
    )]
    pub local_workspace: Option<Option<PathBuf>>,
    /// 按 `server_id` 附加一个已有的本地 `workspace_server`，替代聊天沙箱（仅限 ExistingWorkspace）。要求 `--chat`。
    #[cfg(feature = "local-workspace")]
    #[arg(
        long = "local-workspace-attach",
        value_name = "SERVER_ID",
        conflicts_with = "local_workspace",
        requires = "chat"
    )]
    pub local_workspace_attach: Option<String>,
    /// local-workspace attach/own 的 Cwd 覆盖。要求 `--chat`。
    #[cfg(feature = "local-workspace")]
    #[arg(long = "local-workspace-cwd", value_name = "PATH", requires = "chat")]
    pub local_workspace_cwd: Option<PathBuf>,
    /// 禁止生成子 agent。
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,
    /// 禁止来自 agent 的结构化提问。
    #[arg(long = "no-ask-user", hide = true)]
    pub no_ask_user: bool,
    /// 用于启用跨会话记忆的旧版兼容标志。
    #[arg(
        long = "experimental-memory",
        conflicts_with = "no_memory",
        hide = true
    )]
    pub experimental_memory: bool,
    /// 用于禁用跨会话记忆的旧版兼容标志。
    #[arg(
        long = "no-memory",
        conflicts_with = "experimental_memory",
        hide = true
    )]
    pub no_memory: bool,
    /// 在无头回合结束后执行记忆刷写（或在恢复会话时替代提示词）。
    /// 调用 `x.ai/memory/flush` 并等待刷写用的 LLM。
    /// 仅无头模式：将 `/flush` 作为 `-p` 文本并非可靠的刷写触发方式。
    #[arg(long = "memory-flush", hide = true)]
    pub memory_flush: bool,
    /// agent 名称或定义文件路径。
    #[arg(long = "agent", value_name = "NAME")]
    pub agent: Option<String>,
    /// 以内联 JSON 表示的子 agent 定义。
    #[arg(long = "agents", value_name = "JSON")]
    pub agents_json: Option<String>,
    /// 要允许的内置工具（逗号分隔）。
    #[arg(long = "tools", value_name = "TOOLS")]
    pub cli_tools: Option<String>,
    /// 要移除的内置工具（逗号分隔）。
    #[arg(long = "disallowed-tools", value_name = "TOOLS")]
    pub cli_disallowed_tools: Option<String>,
    /// agent 的最大轮数。
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
    /// 禁用网页搜索和网页抓取工具。
    #[arg(long = "disable-web-search")]
    pub disable_web_search: bool,
    /// 在第一个 agent 回合结束后立即退出，不等待挂起的后台 bash/monitor 任务或后台子 agent（仅无头模式）。
    /// 所有 `chaos -p` 运行默认都会等待（最长至 `--background-wait-timeout`），以便评测框架看到任务完整完成。
    /// 对只需要第一个回合文本的快速脚本使用此项。
    /// 不等待服务器端的自动唤醒输出或持久化 monitor（这些会触发超时）。
    #[arg(long = "no-wait-for-background", hide = true)]
    pub no_wait_for_background: bool,
    /// 第一个回合结束后等待后台工作的最长秒数（仅无头模式）。
    /// 适用于 bash/monitor 的 `task_completed`、后台子 agent（`SubagentFinished`）以及任何仍在运行的非持久化工作。
    /// 持久化的 `monitor(persistent:true)` 永不完成，因此总会等待满整个超时。
    /// 为提升吞吐量可使用 `--no-wait-for-background` 或更短的超时。与 `--no-wait-for-background` 冲突。
    #[arg(
        long = "background-wait-timeout",
        value_name = "SECS",
        default_value = "600",
        conflicts_with = "no_wait_for_background",
        hide = true,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub background_wait_timeout_secs: u64,
    /// 用于文件系统和网络访问的沙箱配置。
    #[arg(long, env = "GROK_SANDBOX", value_name = "PROFILE")]
    pub sandbox: Option<String>,
    /// 会话存储模式：local 或 writeback。
    #[arg(long = "storage-mode", value_name = "MODE", hide = true)]
    pub storage_mode: Option<String>,
    /// 覆盖发送给 agent 的客户端标识符。
    #[arg(long = "client-identifier", value_name = "ID", hide = true)]
    pub client_identifier: Option<String>,
    /// Hunk 追踪器模式：agent_only、all_dirty 或 off（"disabled" 是 off 的别名，会完全关闭 hunk 追踪器）。
    #[arg(long = "hunk-tracker-mode", value_name = "MODE", hide = true)]
    pub hunk_tracker_mode: Option<String>,
    /// 为 agent 启用终端支持。
    #[arg(long = "terminal", hide = true)]
    pub terminal: bool,
    /// 启用客户端文件读取。
    #[arg(long = "fs-read", hide = true)]
    pub fs_read: bool,
    /// 启用客户端文件写入。
    #[arg(long = "fs-write", hide = true)]
    pub fs_write: bool,
    /// 为此会话禁用自动更新。
    #[arg(long = "no-auto-update", hide = true)]
    pub no_auto_update: bool,
    /// 为此会话启用运行时的回合结束 TodoGate。
    ///
    /// 会话范围内（不持久化）。
    /// 最高优先级：覆盖远程 `todo_gate_enabled` 和内置默认值（其为 `false`）。
    #[arg(long = "todo-gate", hide = true)]
    pub todo_gate: bool,
    /// 设置 config.toml 中的 installer 字段。
    #[arg(long = "installer", value_name = "VALUE", hide = true)]
    pub installer: Option<String>,
    /// 以内联方式运行，而不使用终端备用屏幕。
    #[arg(long = "no-alt-screen")]
    pub no_alt_screen: bool,
    /// 实验性：原生回滚缓冲渲染。
    /// 已完成的块会打印到终端原生的回滚缓冲中（使用终端自身的滚动 / 选择功能）。
    /// 一个小的固定区域用于承载提示词和正在进行的回合。
    /// 仅在会话范围内生效，不写入配置。
    /// 要让普通的 `chaos` 默认使用 minimal，请在 ~/.chaos/config.toml 中设置 `[ui] screen_mode = "minimal"`。
    #[arg(long = "minimal")]
    pub minimal: bool,
    /// 为此会话在标准全屏 TUI 中打开，覆盖配置中的 `[ui] screen_mode = "minimal"` 偏好。
    /// 仅在会话范围内生效，不写入配置。
    /// 全屏与内联仍遵循备用屏幕策略（--no-alt-screen、[terminal] alt_screen、终端自动检测）。
    #[arg(long = "fullscreen", conflicts_with = "minimal")]
    pub fullscreen: bool,
    /// 将采样事件写入 ~/.chaos/logs/sampling.jsonl。
    #[arg(long = "log-sampling", env = "GROK_LOG_SAMPLING", hide = true)]
    pub log_sampling: bool,
    /// 即使凭据已可用也显示登录界面。
    #[arg(long = "force-login", hide = true)]
    pub force_login: bool,
    /// 当欢迎界面开始认证时使用 OAuth。
    #[arg(long = "oauth")]
    pub oauth: bool,
    /// 连接到共享的 leader 进程。
    #[arg(long, conflicts_with = "no_leader", hide = true)]
    pub leader: bool,
    /// 即使配置了 leader 模式也独立运行。
    #[arg(long, conflicts_with = "leader", hide = true)]
    pub no_leader: bool,
    /// 交互式会话的初始提示词，例如 `chaos "fix the bug"` 或 `chaos --worktree=feat "create this feature"`。
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
/// Outcome of resolving the startup sandbox profile for a (possibly resumed) session. See [`PagerArgs::startup_sandbox_profile`].
#[derive(Debug, PartialEq, Eq)]
pub enum SandboxStartup {
    /// Apply this profile. `None` means fall through to config/`off`.
    Apply(Option<String>),
    /// Resume requested a profile that differs from the one the session was created with.
    /// Refused so resuming can't silently change the sandbox.
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
/// The program name shown in usage and error output.
///
/// Only a name this binary is installed under is echoed back; anything else
/// normalizes to the public `chaos` rather than leaking the invoker's file name
/// (a crate-named path from `cargo run`, a build-tree path). The fallback used
/// to be `grok`, which is not a name this branch ships, so every invocation
/// reported `Usage: grok …` while the completion scripts it generated said
/// `chaos`.
fn program_display_name(argv0: Option<&str>) -> &'static str {
    match argv0
        .map(std::path::Path::new)
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
    {
        Some("agent") => "agent",
        _ => "chaos",
    }
}
impl PagerArgs {
    pub fn memory_enabled_override(&self) -> Option<bool> {
        if self.experimental_memory {
            Some(true)
        } else if self.no_memory {
            Some(false)
        } else {
            None
        }
    }
    pub(crate) fn memory_override_flag(&self) -> Option<&'static str> {
        if self.experimental_memory {
            Some("--experimental-memory")
        } else if self.no_memory {
            Some("--no-memory")
        } else {
            None
        }
    }
    /// Parse CLI arguments without applying side effects.
    pub fn parse_cli() -> Self {
        let bin_name = program_display_name(std::env::args().next().as_deref());
        Self::parse_from(std::iter::once(bin_name.to_string()).chain(std::env::args().skip(1)))
    }
    /// Apply launch-directory path anchoring and `--cwd` after early commands have been dispatched without filesystem or process initialization.
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
    /// Optional-flag accessor; always `false` in builds without the optional feature, so call sites need no `cfg` of their own.
    pub fn chat(&self) -> bool {
        false
    }
    /// `--local-workspace[=cwd]` own-mode flag.
    #[cfg(feature = "local-workspace")]
    pub fn local_workspace(&self) -> Option<Option<&std::path::Path>> {
        self.local_workspace.as_ref().map(|inner| inner.as_deref())
    }
    /// `--local-workspace-attach=<server_id>`.
    #[cfg(feature = "local-workspace")]
    pub fn local_workspace_attach(&self) -> Option<&str> {
        self.local_workspace_attach.as_deref()
    }
    /// `--local-workspace-cwd=<path>`.
    #[cfg(feature = "local-workspace")]
    pub fn local_workspace_cwd(&self) -> Option<&std::path::Path> {
        self.local_workspace_cwd.as_deref()
    }
    /// Get the session ID to resume, from either --resume or --load (hidden alias).
    ///
    /// Returns `None` when `--resume` was used without a value (the empty-string sentinel).
    /// Use [`resume_most_recent`] to detect that case.
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
    pub(crate) fn local_resume_selection(
        &self,
    ) -> xai_grok_shell::session::persistence::RecentSessionSelection {
        use xai_grok_shell::session::unified_list::HeadlessPolicy;
        let policy = if self.single.is_some()
            || self.prompt_json.is_some()
            || self.prompt_file.is_some()
            || self.memory_flush
        {
            HeadlessPolicy::Include
        } else {
            HeadlessPolicy::Exclude
        };
        xai_grok_shell::session::persistence::RecentSessionSelection::from_headless_policy(policy)
    }
    /// Classify flags for sandbox profile lookup on an existing session.
    ///
    /// Uses [`Self::session_startup_intent`]; invalid combos fall through to `None` (caller should have rejected intent errors earlier at startup).
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
    /// Resolve the sandbox profile to apply at startup, accounting for the profile the resumed session was created with.
    /// `saved` is the resumed session's persisted profile (read once via [`Self::saved_resume_profile`]).
    ///
    /// A session's profile is fixed at creation. Resuming restores it.
    /// An explicit `--sandbox`/`GROK_SANDBOX` that differs from the saved profile is refused: changing a session's sandbox on resume would be unsafe.
    /// A matching flag, or no flag, resumes with the saved profile.
    pub fn startup_sandbox_profile(&self, saved: Option<&str>) -> SandboxStartup {
        let explicit = self.sandbox.as_deref().filter(|s| !s.is_empty());
        Self::resolve_startup_sandbox(explicit, saved.map(String::from))
    }
    /// Pin an explicit non-UUID, non-chat resume/load target to its canonical local session id, before the (irreversible) OS sandbox is applied.
    ///
    /// Resolving once makes the saved-profile peek and materialization consume the same immutable target.
    /// `resume_target_pinned` records the pin so materialization never re-runs local title selection.
    /// Re-selecting after the sandbox would race a concurrent rename/create.
    /// Listing failures and ambiguity are hard errors here, reported before the sandbox (fail closed).
    /// A definitive no-match keeps the raw arg for the legacy remote/worktree id path.
    pub fn pin_local_resume_target(&mut self) -> anyhow::Result<()> {
        let cwd_buf = std::env::current_dir().ok();
        let cwd_str = cwd_buf.as_deref().map(|p| p.to_string_lossy());
        self.pin_local_resume_target_for_cwd(cwd_str.as_deref())
    }
    /// Same as [`Self::pin_local_resume_target`] with an explicit cwd, so tests never mutate the process cwd.
    pub fn pin_local_resume_target_for_cwd(&mut self, cwd: Option<&str>) -> anyhow::Result<()> {
        if self.chat() {
            return Ok(());
        }
        let Some(target) = self.session_to_resume().map(str::to_owned) else {
            return Ok(());
        };
        use crate::app::session_title_resolve::{PinnedResumeTarget, presandbox_resume_target};
        let pinned = presandbox_resume_target(&target, cwd, self.local_resume_selection())?;
        self.resume_target_pinned = true;
        if let PinnedResumeTarget::Title {
            ref id,
            ref sandbox_profile,
        } = pinned
        {
            eprintln!("Resuming session {} (matched by title)", id);
            self.pinned_resume_profile = Some(sandbox_profile.clone());
        }
        let Some(id) = pinned.id() else {
            return Ok(());
        };
        if self
            .resume_session
            .as_deref()
            .is_some_and(|s| !s.is_empty())
        {
            self.resume_session = Some(id);
        } else if self.load_session.as_deref().is_some_and(|s| !s.is_empty()) {
            self.load_session = Some(id);
        }
        Ok(())
    }
    /// The sandbox profile persisted with the session being resumed, if any.
    /// Local, best-effort; `None` when not resuming or nothing is found.
    /// Read once for the profile resume resolution.
    pub fn saved_resume_profile(&self) -> Option<String> {
        let cwd_buf = std::env::current_dir().ok();
        let cwd_str = cwd_buf.as_deref().map(|p| p.to_string_lossy());
        self.saved_resume_profile_for_cwd(cwd_str.as_deref())
    }
    /// Same as [`Self::saved_resume_profile`] with an explicit cwd, so tests never mutate the process cwd.
    pub fn saved_resume_profile_for_cwd(&self, cwd: Option<&str>) -> Option<String> {
        if let Some(pinned) = &self.pinned_resume_profile {
            return pinned.clone();
        }
        match self.resume_target() {
            ResumeTarget::SessionId(id) => {
                xai_grok_shell::session::persistence::resumed_session_sandbox_profile(
                    Some(&id),
                    cwd,
                )
            }
            ResumeTarget::MostRecentForCwd => {
                xai_grok_shell::session::persistence::resolve_recent_session_sandbox_profile(
                    cwd,
                    self.local_resume_selection(),
                )
            }
            ResumeTarget::None => None,
        }
    }
    /// Pure resolution of the explicit flag against the resumed session's saved profile.
    /// Separated from disk access so it can be unit-tested.
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
    /// Returns `None` when no positional prompt was given or it is only whitespace.
    /// This is the `grok "<prompt>"` launch form; the headless `-p`/`--single` path is handled separately.
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
    #[test]
    fn version_flags_parse_as_early_intent_without_exiting() {
        for flag in ["--version", "-v", "-V"] {
            let args = PagerArgs::try_parse_from(["chaos", flag]).expect("version flag parses");
            assert!(args.version, "{flag} must set the early version intent");
            assert!(args.command.is_none());
        }
    }
    #[test]
    fn ordinary_and_doctor_parsing_do_not_set_version_intent() {
        assert!(!PagerArgs::try_parse_from(["chaos"]).unwrap().version);
        assert!(
            !PagerArgs::try_parse_from(["chaos", "doctor"])
                .unwrap()
                .version
        );
        assert!(matches!(
            PagerArgs::try_parse_from(["chaos", "version"])
                .unwrap()
                .command,
            Some(Command::Version { json: false })
        ));
    }
    #[test]
    fn doctor_accepts_report_and_explicit_fix_forms() {
        let bare = PagerArgs::try_parse_from(["chaos", "doctor"]).expect("bare doctor parses");
        assert!(matches!(
            bare.command,
            Some(Command::Doctor(crate::doctor_cmd::DoctorArgs {
                json: false,
                command: None,
            }))
        ));
        let json =
            PagerArgs::try_parse_from(["chaos", "doctor", "--json"]).expect("doctor --json parses");
        assert!(matches!(
            json.command,
            Some(Command::Doctor(crate::doctor_cmd::DoctorArgs {
                json: true,
                command: None,
            }))
        ));
        for id in [
            "terminal.ssh-wrap",
            "tmux-clipboard",
            "terminal.dcs-passthrough",
            "tmux-extended-keys",
        ] {
            let fix = PagerArgs::try_parse_from(["chaos", "doctor", "fix", id, "--yes"])
                .expect("doctor fix parses");
            assert!(matches!(
                fix.command,
                Some(Command::Doctor(crate::doctor_cmd::DoctorArgs {
                    json: false,
                    command: Some(crate::doctor_cmd::DoctorCommand::Fix(
                        crate::doctor_cmd::FixArgs { id: Some(ref parsed), yes: true }
                    )),
                })) if parsed == id
            ));
        }
        let list = PagerArgs::try_parse_from(["chaos", "doctor", "fix"])
            .expect("doctor fix without an ID lists applicable fixes");
        assert!(matches!(
            list.command,
            Some(Command::Doctor(crate::doctor_cmd::DoctorArgs {
                json: false,
                command: Some(crate::doctor_cmd::DoctorCommand::Fix(
                    crate::doctor_cmd::FixArgs {
                        id: None,
                        yes: false
                    }
                )),
            }))
        ));
        for unsupported in [
            vec!["grok", "doctor", "all"],
            vec!["grok", "doctor", "fix", "ssh-wrap", "extra"],
            vec!["grok", "doctor", "fix", "--yes"],
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
            PagerArgs::try_parse_from(["chaos"])
                .unwrap()
                .resume_target(),
            ResumeTarget::None
        );
        assert_eq!(
            PagerArgs::try_parse_from(["chaos", "-c"])
                .unwrap()
                .resume_target(),
            ResumeTarget::MostRecentForCwd
        );
        assert_eq!(
            PagerArgs::try_parse_from(["chaos", "--resume"])
                .unwrap()
                .resume_target(),
            ResumeTarget::MostRecentForCwd
        );
        assert_eq!(
            PagerArgs::try_parse_from(["chaos", "--resume", "sess-1"])
                .unwrap()
                .resume_target(),
            ResumeTarget::SessionId("sess-1".to_string())
        );
        assert_eq!(
            PagerArgs::try_parse_from(["chaos", "-s", "sess-2"])
                .unwrap()
                .resume_target(),
            ResumeTarget::None
        );
        assert_eq!(
            PagerArgs::try_parse_from(["chaos", "-r", "old", "--fork-session"])
                .unwrap()
                .resume_target(),
            ResumeTarget::SessionId("old".to_string())
        );
    }
    /// The screen-mode flags are mutually exclusive.
    /// The pair exists so one can override the other's sticky config value; accepting both in one invocation would be ambiguous.
    #[test]
    fn minimal_and_fullscreen_flags_conflict() {
        let args = PagerArgs::try_parse_from(["chaos", "--minimal"]).unwrap();
        assert!(args.minimal && !args.fullscreen);
        let args = PagerArgs::try_parse_from(["chaos", "--fullscreen"]).unwrap();
        assert!(args.fullscreen && !args.minimal);
        let err = PagerArgs::try_parse_from(["chaos", "--minimal", "--fullscreen"]).unwrap_err();
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
            PagerArgs::try_parse_from(["chaos", "--sandbox", "strict"])
                .unwrap()
                .startup_sandbox_profile(None),
            SandboxStartup::Apply(Some("strict".to_string()))
        );
        assert_eq!(
            PagerArgs::try_parse_from(["chaos", "--sandbox", ""])
                .unwrap()
                .startup_sandbox_profile(None),
            SandboxStartup::Apply(None)
        );
        assert_eq!(
            PagerArgs::try_parse_from(["chaos"])
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
        let args = PagerArgs::try_parse_from(["chaos", "--leader-socket", "/tmp/leader-x.sock"])
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
    /// The name in usage and error output must be one this branch installs.
    ///
    /// [`cli_command_name_is_chaos`] cannot catch a wrong fallback here: it
    /// renders the derive, while every real invocation goes through `argv[0]`.
    /// The fallback was `grok` long after the binary became `chaos`, so `--help`
    /// named a program the user does not have.
    #[test]
    fn program_display_name_normalizes_to_an_installed_name() {
        assert_eq!(program_display_name(Some("chaos")), "chaos");
        assert_eq!(program_display_name(Some("/usr/local/bin/chaos")), "chaos");
        assert_eq!(program_display_name(Some("agent")), "agent");
        assert_eq!(
            program_display_name(Some("grok")),
            "chaos",
            "the upstream name must not survive as a fallback"
        );
        assert_eq!(
            program_display_name(Some("/t/debug/xai-grok-pager-bin")),
            "chaos",
            "a crate-named path must not leak into --help"
        );
        assert_eq!(program_display_name(None), "chaos");
    }
    #[test]
    fn leader_socket_flag_defaults_to_none() {
        let args = PagerArgs::try_parse_from(["chaos"]).expect("bare grok parses");
        assert!(args.leader_socket.is_none());
    }
    #[test]
    fn leader_mgmt_list_info_kill_parse() {
        let list = PagerArgs::try_parse_from(["chaos", "leader", "list", "--json"])
            .expect("grok leader list --json");
        assert!(matches!(
            list.command,
            Some(Command::Leader(LeaderMgmtArgs {
                command: LeaderMgmtCommand::List { json: true },
            }))
        ));
        let info = PagerArgs::try_parse_from(["chaos", "leader", "info", "--pid", "42"])
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
        let kill =
            PagerArgs::try_parse_from(["chaos", "leader", "kill"]).expect("grok leader kill");
        assert!(matches!(
            kill.command,
            Some(Command::Leader(LeaderMgmtArgs {
                command: LeaderMgmtCommand::Kill,
            }))
        ));
        assert!(PagerArgs::try_parse_from(["chaos", "leader", "profile"]).is_err());
    }
    #[test]
    fn debug_file_flag_parses_and_is_global() {
        let root = PagerArgs::try_parse_from(["chaos", "--debug-file", "/tmp/fire.txt"])
            .expect("--debug-file parses at the root");
        assert_eq!(
            root.debug_file.as_deref(),
            Some(std::path::Path::new("/tmp/fire.txt"))
        );
        let sub =
            PagerArgs::try_parse_from(["chaos", "agent", "stdio", "--debug-file", "/tmp/f.txt"])
                .expect("--debug-file parses after a subcommand (global)");
        assert_eq!(
            sub.debug_file.as_deref(),
            Some(std::path::Path::new("/tmp/f.txt"))
        );
    }
    #[test]
    fn debug_file_flag_defaults_to_none() {
        let args = PagerArgs::try_parse_from(["chaos"]).expect("bare grok parses");
        assert!(args.debug_file.is_none());
    }
    #[test]
    fn positional_prompt_seeds_interactive_session() {
        let args =
            PagerArgs::try_parse_from(["chaos", "fix the bug"]).expect("positional prompt parses");
        assert_eq!(args.initial_prompt(), Some("fix the bug"));
        assert!(args.command.is_none());
        assert!(args.single.is_none());
    }
    #[test]
    fn bare_grok_has_no_initial_prompt() {
        let args = PagerArgs::try_parse_from(["chaos"]).expect("bare grok parses");
        assert_eq!(args.initial_prompt(), None);
    }
    #[test]
    fn initial_prompt_trims_and_ignores_whitespace_only() {
        let args =
            PagerArgs::try_parse_from(["chaos", "  spaced  "]).expect("padded prompt parses");
        assert_eq!(args.initial_prompt(), Some("spaced"));
        let blank = PagerArgs::try_parse_from(["chaos", "   "]).expect("blank prompt parses");
        assert_eq!(blank.initial_prompt(), None);
    }
    #[test]
    fn subcommand_takes_precedence_over_positional_prompt() {
        let args = PagerArgs::try_parse_from(["chaos", "logout"]).expect("subcommand parses");
        assert!(matches!(args.command, Some(Command::Logout)));
        assert!(args.prompt.is_none());
    }
    #[test]
    fn usage_command_parses_session_and_optional_turn() {
        let session_only = PagerArgs::try_parse_from(["chaos", "usage", "sess-1"])
            .expect("grok usage <session-id>");
        assert!(matches!(
            session_only.command,
            Some(Command::Usage(crate::usage_cmd::UsageArgs {
                ref session_id,
                turn: None,
            })) if session_id == "sess-1"
        ));
        let with_turn = PagerArgs::try_parse_from(["chaos", "usage", "sess-1", "3"])
            .expect("grok usage <session-id> <turn>");
        assert!(matches!(
            with_turn.command,
            Some(Command::Usage(crate::usage_cmd::UsageArgs {
                ref session_id,
                turn: Some(3),
            })) if session_id == "sess-1"
        ));
    }
    #[test]
    fn positional_prompt_conflicts_with_headless_single() {
        let err = PagerArgs::try_parse_from(["chaos", "-p", "headless", "interactive"])
            .expect_err("positional prompt + --single must conflict");
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }
    #[test]
    fn worktree_flag_and_initial_prompt_combine() {
        let a = PagerArgs::try_parse_from(["chaos", "do the thing", "-w"])
            .expect("prompt then bare -w parses");
        assert_eq!(a.initial_prompt(), Some("do the thing"));
        assert_eq!(a.worktree.as_deref(), Some(""));
        let b = PagerArgs::try_parse_from(["chaos", "--worktree=feat", "do the thing"])
            .expect("--worktree=name + positional parses");
        assert_eq!(b.initial_prompt(), Some("do the thing"));
        assert_eq!(b.worktree.as_deref(), Some("feat"));
        let c = PagerArgs::try_parse_from(["chaos", "-w", "x"]).expect("-w x parses");
        assert_eq!(c.worktree.as_deref(), Some("x"));
        assert_eq!(c.initial_prompt(), None);
    }
    #[test]
    fn trust_flag_parses_on_pager_and_alias() {
        let bare = PagerArgs::try_parse_from(["chaos"]).expect("bare grok parses");
        assert!(!bare.trust);
        let long = PagerArgs::try_parse_from(["chaos", "--trust"]).expect("--trust parses");
        assert!(long.trust);
        let alias =
            PagerArgs::try_parse_from(["chaos", "--trust-folder"]).expect("--trust-folder parses");
        assert!(alias.trust);
    }
    #[test]
    fn reasoning_effort_and_effort_alias_parse_same_field() {
        let long = PagerArgs::try_parse_from(["chaos", "--reasoning-effort", "high"])
            .expect("--reasoning-effort parses");
        assert_eq!(long.reasoning_effort.as_deref(), Some("high"));
        let alias = PagerArgs::try_parse_from(["chaos", "--effort", "high"])
            .expect("--effort alias parses");
        assert_eq!(alias.reasoning_effort.as_deref(), Some("high"));
    }
    #[test]
    fn reasoning_effort_accepts_max_and_remapped_ids() {
        let max = PagerArgs::try_parse_from(["chaos", "--effort", "max"]).expect("max parses");
        assert_eq!(max.reasoning_effort.as_deref(), Some("max"));
        let deep = PagerArgs::try_parse_from(["chaos", "--reasoning-effort", "deep"])
            .expect("deep parses");
        assert_eq!(deep.reasoning_effort.as_deref(), Some("deep"));
    }
    #[test]
    fn reasoning_effort_last_flag_wins_when_both_names_set() {
        let args =
            PagerArgs::try_parse_from(["chaos", "--reasoning-effort", "low", "--effort", "high"])
                .expect("both effort flag names parse");
        assert_eq!(args.reasoning_effort.as_deref(), Some("high"));
        let reverse =
            PagerArgs::try_parse_from(["chaos", "--effort", "high", "--reasoning-effort", "low"])
                .expect("both effort flag names parse (reverse order)");
        assert_eq!(reverse.reasoning_effort.as_deref(), Some("low"));
    }
    #[test]
    fn agent_args_effort_alias_parses() {
        let args = PagerArgs::try_parse_from(["chaos", "agent", "--effort", "max", "stdio"])
            .expect("agent --effort parses");
        let Command::Agent(agent) = args.command.expect("agent subcommand") else {
            panic!("expected agent subcommand");
        };
        assert_eq!(agent.reasoning_effort.as_deref(), Some("max"));
    }
}
