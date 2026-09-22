# 配置

Chaos 从配置文件、环境变量和 CLI flags 读取设置。本页覆盖常用选项。

> **配置根（user home）：** `$CHAOS_HOME` → `$GROK_HOME` → 已有 `~/.chaos` →
> 已有 `~/.grok` → 默认新建 `~/.chaos`。下文写 `~/.chaos/...` 时，若你仍在用
> 兼容目录，请替换为 `~/.grok/...`。项目级同样双读 `.chaos/` 与 `.grok/`。
> 模型与 Provider 完整示例见 [CHAOS.md](../../../../../CHAOS.md)。

---

## 优先级

设置按以下优先级从高到低解析：

1. **CLI flags**（如 `--yolo`、`--model`、`--sandbox`）
2. **环境变量**（如 Provider 的 `env_key`、`GROK_MEMORY`）
3. **config.toml**（`~/.chaos/config.toml` 或兼容路径）
4. **Managed / requirements config**（组织部署的 `managed_config.toml` / `requirements.toml`）
5. **内置默认值**

---

## config.toml（主配置）

位置：`~/.chaos/config.toml`（兼容旧名 `~/.grok/config.toml`，双读）。文件不存在时使用内置默认值，只需覆盖你需要的项。

### 通用设置

```toml
[cli]
auto_update = true                     # check for updates on launch

[models]
default = "gpt-5"                      # catalog key from [model.*] (required for BYOK)
# web_search / image_description / session_summary 未设时回落到 default

# Defaults applied to every model; a per-model [model.<id>] value always wins.
# See "Custom Models" for the per-model overrides and full details.
extra_headers = { "X-Request-Tags" = "team=example,env=prod" }
temperature = 0.7
top_p = 0.95
max_completion_tokens = 8192
max_retries = 8
inference_idle_timeout_secs = 600
subagent_rate_limit_max_attempts = 8
stream_tool_calls = true

[ui]
simple_mode = true                     # readline-style prompt editing (default); false = vim editing in the prompt
vim_mode = false                       # vim-style scrollback navigation keys (default: false)
max_thoughts_width = 120               # max column width for reasoning display
default_selected_permission = "always_allow_all_sessions" # preselected row on the FIRST approval prompt
remember_tool_approvals = true         # show per-command "Always allow" options on permission prompts;
                                       # grants are remembered per project (default: true); see 22-permissions-and-safety.md
show_thinking_blocks = true            # show agent thinking blocks in the TUI (default: true)
group_tool_verbs = true                # fold runs of read/search/list tool calls and subagent rows
                                       # — and finished thoughts among them — into one row (default: true)
collapsed_edit_blocks = false          # show edits as one-line +N/-M diffstat summaries and merge
                                       # back-to-back same-file edits into one row, expand for the
                                       # diffs (default: false; pager.toml [scrollback.blocks.edit]
                                       # expanded_by_default/line_summary override its fold shape)
page_flip_on_send = true               # pin a just-sent prompt at the top of the viewport so the
                                       # response starts on a fresh page (default: true); set false
                                       # so sending never moves the scroll position
follow_up_behavior = "queue"           # mid-turn follow-ups: "queue" (wait for turn end; default) or
                                       # "steer" (plain Enter still queues visibly, then injects at the
                                       # next tool/model safe gap). See Keyboard Shortcuts → Mid-turn.
screen_mode = "fullscreen"             # default render mode: "fullscreen" | "minimal"
                                       # (unset → fullscreen); set via /settings → Default screen mode

[features]
telemetry = false                      # anonymous usage telemetry
feedback = true                        # feedback system (default: true)
lsp_tools = false                      # expose the lsp tool
codebase_indexing = true               # code graph indexing (default: true)
two_pass_compaction = false            # prefire two-pass compaction (default: false, opt-in)
remote_fetch = false                   # online model-catalog / settings fetches (default: false for
                                       # Chaos BYOK — empty catalog until you add [model.*]; set true
                                       # only if you want optional remote catalog fetches; background
                                       # managed-config sync has its own switch: managed_config)

[session]
auto_compact_threshold_percent = 85    # auto-compact at this % of context window (default: 85)
load_envrc = true                      # load .envrc environment variables

[tools]
respect_gitignore = false              # default: false; set true to make every tool skip gitignored files

# Optional caps on parallel media generation in a single model step.
# Per tool name. First 2×-or-more burst: discard that step and retry once.
# Any other over-cap (including a second 2× burst) keeps the first K.
# Defaults: image 8, video 4.
# Env vars GROK_MAX_PARALLEL_IMAGE_GEN_CALLS / GROK_MAX_PARALLEL_VIDEO_GEN_CALLS
# override these values (see environment-variables doc).
# [tools.media_gen]
# max_parallel_image_gen_calls = 8
# max_parallel_video_gen_calls = 4
```

#### 输入模式

`[ui] simple_mode` 控制你在**提示框**（输入编辑器）里如何编辑文本。它不影响你在回滚区里怎么移动，那是 [`vim_mode`](#vim-mode) 的事。

| 取值 | 行为 |
|-------|----------|
| `true`（默认） | **Readline 编辑。** 普通 readline 风格的文本输入。 |
| `false` | **Vim 编辑（实验性）。** Vim 风格的模态编辑（normal 与 insert 模式）。提示框为空时以 normal 模式启动并聚焦回滚区。 |

把提示框切换为 vim 风格编辑：

```toml
[ui]
simple_mode = false
```

也可以在设置面板里切换（`/settings` → **Disable vim input mode**）；Chaos 会把你的选择写入 `[ui] simple_mode`。`simple_mode` 与 `vim_mode` 相互独立——前者管提示框编辑器，后者管回滚区导航。完整按键参考见 [键盘快捷键](03-keyboard-shortcuts.md)。

#### 默认预选权限

当智能体请求运行命令（或执行其他工具操作）时，审批菜单默认会高亮某一行。`[ui] default_selected_permission` 决定会话中**第一个**提示出现时高亮的是哪一行。

| 取值 | 预选中行 |
|-------|-----------------|
| `always_allow_all_sessions`（默认） | "Always allow on all sessions"（在所有会话中始终允许）那一行。 |
| `allow_command_always` | "Always allow this command"（始终允许该命令）那一行。 |
| `allow_once` | "Yes" / 仅本次允许那一行。 |
| `reject` | 拒绝那一行。 |

```toml
[ui]
default_selected_permission = "allow_once"
```

回答完第一个提示后，光标会变成**粘性**的：之后的每个提示都会预选你上一次确认的选项（比如选过一次 "No"，后续提示就从拒绝行开始），并跨编辑 / bash / MCP 提示一直延续，直到重启。因此这个设置只决定起始位置。

取值不区分大小写；未设置或无法识别的值会回退到 `always_allow_all_sessions`。`allow_command_always` 行始终只作用于当前正在审批的具体操作（命令 / 工具 / 域名 / 编辑会话），绝不会是全局放行一切——那是 `always_allow_all_sessions` 的职责。注意，按命令区分的 "Always allow" 行只在 `[ui] remember_tool_approvals` 启用时出现（默认启用；设为 `false` 可隐藏它们）。参见 [22-permissions-and-safety.md](22-permissions-and-safety.md)。

你也可以用 `GROK_DEFAULT_SELECTED_PERMISSION` 覆盖它，这在无头运行或智能体测试（不应改动 `config.toml`）时非常方便。优先级：环境变量 → `config.toml` → `always_allow_all_sessions`。

#### Vim 模式

`[ui] vim_mode` 控制 vim 风格按键绑定是否在**回滚区**窗格中生效。它不影响输入框。

| 取值 | 行为 |
|-------|----------|
| `false`（默认） | 裸字母键与 `Shift+字母` 键（`j`/`k`、`h`/`l`、`g`/`G`、`y`/`Y`、`o`/`O`、`r`、`x`、`e`/`E`、`H`/`L`，以及 `i`）在回滚区里被屏蔽：按下去会聚焦提示框并把该字符输进去。方向键、`Tab`、`Space`、`PageUp`/`PageDown`，以及所有 `Ctrl+字母` 快捷键仍然可以导航。`Esc` **不是**回滚区按键——它从不取消正在运行的回合（那是 `Ctrl+C` 的事），空闲时则遵循清屏 / 回退策略（见[键盘快捷键](03-keyboard-shortcuts.md#escape)）。 |
| `true` | 所有 vim 风格的回滚区绑定全部生效，与[键盘快捷键](03-keyboard-shortcuts.md)里列出的完全一致。两种设置下 Esc 的行为相同。 |

运行时可用 `/vim-mode` 切换，或从 `/settings` → **Vim scrollback navigation** 切换。Chaos 会立即把改动写入 `[ui] vim_mode`，并应用到之后的每个分页器会话，包括同一进程里新建的智能体与子代理。没有会话级覆盖 —— 下次启动时以 `config.toml` 为准。`vim_mode` 与 `simple_mode` 相互独立。

#### 屏幕模式

`[ui] screen_mode` 是裸跑 `chaos` 时的**默认渲染模式**。可从 `/settings` → **Default screen mode**（需重启）设置，或手工编辑 `config.toml` —— 两者都会写文件。命令行开关（`--minimal` / `--fullscreen`）和斜杠命令（`/minimal` / `/fullscreen`）都是会话级的，**不会**写这个键；斜杠切换之后，反向命令只在该会话内把你切回去。

| 取值 | 行为 |
|-------|----------|
| unset | 设置面板显示 **Fullscreen**。启动时没有粘性偏好：旧版 `pager.toml` 的 `[terminal] minimal` 仍可强制最小模式，而会泄漏鼠标上报的终端（JediTerm/Windows）在你显式设值之前可能自动开成最小模式。除此之外，全屏还是内联由备用屏幕策略决定。 |
| `"fullscreen"` | 粘性非最小模式。全屏还是内联仍由备用屏幕策略决定（`--no-alt-screen`、`[terminal] alt_screen`、终端自动检测）。 |
| `"minimal"` | 粘性最小模式（以回滚区为主）。 |

对于当次调用，命令行开关永远优先于配置值。

#### 发送时把提示置顶

默认情况下，发送提示会把它滚动到视口顶端，让回答从新的一页开始。设置 `[ui] page_flip_on_send = false`（或在 `/settings` → Appearance 里切换 **Snap prompt to top on send**）可在发送时保持滚动位置不变。下次发送即生效 —— 无需重启。

#### 滚动

四个 `[ui]` 设置调节鼠标滚轮与触控板滚动。全部立即生效，并可在设置面板（`/settings` → **Scroll speed** / **Scroll input** / **Scroll lines** / **Invert scroll**）中编辑。

| 键 | 取值（默认） | 行为 |
|-----|------------------|----------|
| `scroll_speed` | `1`–`100` (`50`) | 滚轮与触控板的滚动速度倍数。`50` = 1.0x，`1` = 0.1x，`100` = 6.0x。 |
| `scroll_mode` | `auto` \| `wheel` \| `trackpad` (`auto`) | 滚轮与触控板的判定是启发式的（终端的滚动事件不带幅度信息）；当自动检测误判你的设备时，用这个键强制指定一类——例如滚轮一格跳得太远，或触控板手感一顿一顿。 |
| `scroll_lines` | `1`–`10`（未设置） | 每档滚动的行数，对滚轮与触控板**都**生效。未设置时，各终端自己的配置生效（例如 tmux 下保守的 1 行/事件）。一旦提交任何值——哪怕是设置面板显示的那个 `3`——就会永久切换到该显式覆盖值。 |
| `invert_scroll` | `false` \| `true` (`false`) | 反转垂直滚动方向（「自然」滚动）。 |

```toml
[ui]
scroll_speed = 50
scroll_mode = "auto"     # auto | wheel | trackpad
invert_scroll = false
# scroll_lines is unset by default: the per-terminal profile stays in charge.
# scroll_lines = 3
```

每个设置还支持环境变量覆盖，只在首次加载时应用（同样便于无头 / 测试运行）：`GROK_SCROLL_SPEED`、`GROK_SCROLL_MODE`、`GROK_INVERT_SCROLL`（`1`/`true`/`0`/`false`）和 `GROK_SCROLL_LINES`。优先级：环境变量 → `config.toml` → 默认值。无法识别的值回退到默认值，超范围的数字会被钳制。

### 工具配置

```toml
[toolset.bash]
timeout_secs = 120.0                   # foreground command timeout in seconds (default: 120)
output_byte_limit = 20000              # max captured output in bytes (default: 20000)

[toolset.ask_user_question]
timeout_enabled = true                 # false = wait forever for answers (default: true)
timeout_secs = 1800                    # seconds to wait when enabled (default: 1800 / 30 min)

[toolset.web_fetch]
proxy_endpoint = "https://proxy.example.com"   # egress proxy URL
allowed_domains = ["docs.rs", "x.ai"]          # override the built-in allowlist
allow_local = false                            # true = allow localhost / 127.0.0.0/8 / ::1 only

[toolset.web_search]
# Restrict web_search to these domains (max 5). Mutually exclusive with excluded_domains.
allowed_domains = ["docs.x.ai", "arxiv.org"]
# ...or block these domains instead (leave allowed_domains unset):
# excluded_domains = ["reddit.com", "pinterest.com"]
```

`allow_local` 默认关闭（对 SSRF 失败即关）。开启它（或设置 `GROK_WEB_FETCH_ALLOW_LOCAL=1`）后，`web_fetch` 也只能访问**显式**回环主机 —— 私有、链路本地和云元数据网段仍然被封禁。解析顺序：TOML > 环境变量 > 默认关闭。

`[toolset.web_search]` 约束 `web_search` 工具的域名 —— 即搜索本身运行时所依据的允许清单/屏蔽清单（不是事后过滤）。`allowed_domains` 与 `excluded_domains` **互斥**；两者都设时允许清单获胜，屏蔽清单被丢弃并给出警告。清单为空或缺省即不受限。这同时适用于后端托管的搜索（带服务端搜索的模型）和客户端回退。已配置的策略是**权威的**：模型无法绕过 —— 只要你在这里设置了 `allowed_domains` 或 `excluded_domains`，模型自己按调用传入的 `allowed_domains` 就会被忽略（所以屏蔽清单是真屏蔽）。模型的按调用允许清单只在你什么都没配置时才生效。解析顺序：requirements → 用户 `config.toml` → managed → 默认（未设置）。配置在会话启动时读取，所以要改就趁会话开始前改 —— 中途修改不生效。

`[toolset.ask_user_question]` 在 **requirements.toml**、**managed 配置**和你的用户 **`config.toml`** 中均被采纳。优先级：requirements → 环境变量（`GROK_ASK_USER_QUESTION_TIMEOUT_ENABLED` / `GROK_ASK_USER_QUESTION_TIMEOUT_SECS`）→ 用户配置 → managed → 默认值。在用户配置里设 `timeout_enabled = false` 可为自己关闭问卷的自动超时；`timeout_secs` 必须是正整数。也可以从 `/settings` → **Ask-Question timeout**（位于 Agent & Approval 下）切换 `timeout_enabled`；改动对新启动的会话生效。

### 认证

完整说明参见 [认证](02-authentication.md)。

```toml
[auth]
auth_provider_command = "/usr/local/bin/my-auth-provider"
auth_provider_label = "Acme Corp"
auth_token_ttl = 3600

[grok_com_config.oidc]
issuer = "https://acme.okta.com"
client_id = "0oa1b2c3d4e5f6g7h8i9"
# scopes = ["openid", "profile", "email", "offline_access", "api:access"]
# audience = "https://api.acme.com"
```

### 自定义模型

添加自定义模型端点，以使用替代提供商或自托管模型。

```toml
[model.my-model]
model = "model-id"                    # model identifier sent to API
base_url = "https://api.example.com/v1"  # OpenAI-compatible endpoint
name = "Display Name"                 # shown in model picker
description = "Model description"      # optional
api_key = "sk-..."                    # API key for this provider
env_key = "XAI_API_KEY"               # env var(s) holding the API key; string or array (first set, non-empty wins)
temperature = 0.7                     # sampling temperature (0.0-2.0)
top_p = 0.95                          # nucleus sampling parameter
max_completion_tokens = 8192          # max tokens per response
context_window = 128000               # context window size (for auto-compact)
```

凭据解析顺序：`api_key` > `env_key` > 已登录会话 token > `XAI_API_KEY`。

要覆盖内置模型，用它的名字作为节键，只设置你需要的字段：

```toml
[model.grok-4.6]
api_key = "my-api-key"
```

### MCP 服务器

通过 Model Context Protocol 配置外部工具集成。

```toml
[mcp_servers.github]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
env = { GITHUB_PERSONAL_ACCESS_TOKEN = "ghp_xxx" }
enabled = true                        # enable/disable (default: true)
startup_timeout_sec = 30              # init timeout in seconds (default: 30)
tool_timeout_sec = 6000              # tool call timeout in seconds (default: 6000)
tool_timeouts = { create_issue = 120 }  # per-tool timeout overrides

[mcp_servers.postgres]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-postgres", "postgresql://user:pass@localhost/db"]

[mcp_servers.my-streamable-server]
url = "https://mcp.example.com/api/mcp"  # HTTP/SSE transport
headers = { "x-mcp-session-id" = "{{session_id}}" }
```

远程（HTTP/SSE）服务器会收到默认的 `User-Agent: grok-cli/<version>` 头；
`headers` 里有效的 `User-Agent` 项会覆盖它（Figma 服务器收到的是裸
`grok-cli`）。细节见 [MCP 服务器](07-mcp-servers.md)。

MCP 服务器也可以按项目设置在项目根下的 `.chaos/config.toml`（兼容旧名 `.grok/config.toml`）里。项目作用域配置只贡献 `[mcp_servers]`、`[plugins]` 和 `[permission]` 规则；其他所有 section 都只从 `~/.chaos/config.toml`（兼容 `~/.grok`）加载。

`[mcp_servers]` 与 `[plugins]` 的优先级：`.chaos/config.toml`（当前目录）> `<仓库根>/.chaos/config.toml` > `~/.chaos/config.toml`（各层均兼容 `.grok` 旧名）。`[permission]` 规则不受优先级覆盖——它们跨所有文件合并，按 `deny` > `ask` > `allow`（见 [22-permissions-and-safety.md](22-permissions-and-safety.md)）。

### 记忆

跨会话持久保存知识。用 `[memory] enabled = true` 或
`GROK_MEMORY=1` 启用；显式的 `[memory] enabled = false` 会把它关掉，
即使受管的远程设置把它打开了也一样。早期版本记录下的笔记
会自动沿用。见 [13-memory.md](13-memory.md)。

```toml
[memory]
enabled = true

[memory.session]
save_on_end = true                    # write metadata summary on session end

[memory.watcher]
enabled = true                        # watch memory files for external edits

[memory.search]
max_results = 6                       # default number of results
min_score = 0.7                       # minimum relevance score

[memory.initial_injection]
enabled = true                        # auto-inject memory on first turn
min_score = 0.9                       # score threshold for first-turn injection

[memory.embedding]
# model is unset by default, so retrieval uses full-text search only
dimensions = 1024                     # vector dimensions
```

### 子智能体

```toml
[subagents]
enabled = true
sampling_limit = 12                   # concurrent in-flight subagent sampling calls per process; defaults to max_concurrent (32) when unset (GROK_SUBAGENT_SAMPLING_LIMIT)

[subagents.toggle]
explore = true                        # enable/disable specific types
plan = false

[subagents.models]
explore = "grok-4.6"               # route to different models
```

要钉住某个子智能体所用的模型，在 `[subagents.models]` 下设置它的条目。

### 目标模式与后台工作流

`/goal` 有两种驱动方式，由「后台工作流」这一设置选择。工作流启用时，宿主自有的工作流引擎评估各轮并驱动完成度校验；禁用时，`/goal` 回退到旧版面向模型的 `update_goal` 工具。`/goal` 本身是否可用是另一个开关（goal 功能设置）。

后台工作流——`workflow` 工具、具名的 `.chaos/workflows/*.rhai` 脚本（兼容 `.grok/workflows/`）、`/deep-research` 和 `/workflow` 启动——**默认关闭**。

```toml
[workflows]
enabled = true                        # enable background workflows (or GROK_WORKFLOWS=1)
```

项目工作流从 `<仓库根>/.chaos/workflows/` 发现；用户工作流从 `~/.chaos/workflows/` 发现（两处均兼容 `.grok` 旧名）。发现与调用都以脚本的 `meta.name` 为准，所以让每个文件名与它的 `meta.name` 保持一致。内置名胜过项目名，项目名胜过用户名，因此请让各作用域的名字互不重复。

每次启动都会得到一个会话内唯一的展示句柄，例如 `deep-research-2`。这个句柄就是你在 `/workflow runs` 面板里看到的、也是传给 `/workflow pause`、`resume`、`stop` 的那个；内部运行 ID 从不出现在命令里。带编号的句柄不是可复用的定义名，所以面板会禁用 **save**，直到你另选一个唯一的 `meta.name` 并自己保存改过的脚本。例子见[斜杠命令](04-slash-commands.md)。

### 技能

```toml
[skills]
paths = ["~/my-team-skills"]          # additional directories to scan
ignore = ["~/my-team-skills/wip"]     # paths to exclude
disabled = ["wip-skill"]              # skill names to keep listed but inactive
```

### 厂商兼容性开关

控制对 Cursor、Claude 与 Codex 的厂商兼容。每个单元格默认 `true`。session 类单元格会一直停留在预备状态、不产生作用，直到有外部会话扫描器消费它们；而且每个工具同时需要它的 `sessions` 单元格和对应的 `resume-claude`、`resume-codex` 或 `resume-cursor` 技能——技能缺失就意味着完全不对外部会话的文件系统做 I/O。

```toml
[compat.cursor]
skills = true     # scan ~/.cursor/skills/ and <cwd>/.cursor/skills/
rules = true      # scan ~/.cursor/rules/ and <dir>/.cursor/rules/
agents = true     # scan ~/.cursor/ for named instruction files
mcps = true       # scan ~/.cursor/mcp.json and <cwd>/.cursor/mcp.json
hooks = true      # scan ~/.cursor/hooks.json and <cwd>/.cursor/hooks.json
sessions = true   # staged; no scanner consumer yet

[compat.claude]
skills = true     # scan ~/.claude/skills/ and <cwd>/.claude/skills/
rules = true      # scan ~/.claude/rules/ and <dir>/.claude/rules/
agents = true     # scan ~/.claude/ and <dir>/.claude/CLAUDE*.md
mcps = true       # scan ~/.claude.json for MCP servers
hooks = true      # scan ~/.claude/settings.json for hooks
sessions = true   # staged; no scanner consumer yet

[compat.codex]
sessions = true   # staged; no scanner consumer yet
```

Codex 的 `skills`、`rules`、`agents`、`mcps` 和 `hooks` 单元格是保留项，目前不产生作用——它们不会启用 `.codex` 发现。

对 Claude 与 Cursor 而言，`rules` 和 `agents` 相互独立：关掉具名说明文件不会停用 home 或项目规则目录，关掉 rules 也不会停用具名文件。Claude 的 `agents` 单元格管辖 home 级的 `~/.claude/` 具名文件和项目里的 `<dir>/.claude/CLAUDE*.md`；顶层的通用 `Claude.md`、`CLAUDE.md` 和 `CLAUDE.local.md` 仍会被识别。项目规则路径会从仓库根一路扫到当前目录的每一层。

每个单元格都可以用环境变量或 `config.toml` 设置；变量名见环境变量参考。解析顺序：环境变量 > config.toml > 默认值（开启）。

`chaos inspect` 会把仍需在会话启动时才能解析的单元格显示为 `?`，直到有值可用为止；带显式环境变量或 TOML 值的单元格直接用那个值。受影响的发现条目在 JSON 里上报 `compatibilityStatus: "unresolved"`，在人类可读输出里显示 `[compat unresolved]`。

### 插件

```toml
[plugins]
paths = ["~/my-plugins/custom-tools"]
disabled = ["user/a1b2c3d4/noisy-plugin"]
```

### 提示项

`[hints]` 保存一些小的持久化 UI 偏好——多半是「别再问我」类的免打扰项。你在 TUI 里选「不再询问」时 Chaos 会替你写入，但你也可以手工编辑或删除；删掉一个键就恢复默认。

`[hints]` 从**生效后的配置合并结果**读取，遵循通常的优先级：系统 managed → 用户 `managed_config.toml` → 用户 `config.toml` → 用户 `requirements.toml` → 系统 `requirements.toml`，层越高越优先。TUI **只会**把免打扰项**写入**你用户的 `~/.chaos/config.toml`（兼容 `~/.grok`）。

```toml
[hints]
project_picker_disabled = false        # skip the project-directory picker
memory_modal_fullscreen = false        # remember the memory modal fullscreen state
new_session_worktree_mode = "never"    # /new worktree prompt: "ask" | "always" | "never"
fork_worktree_mode = "ask"             # /fork worktree prompt: "ask" | "always" | "never"
```

| 键 | 类型 | 默认 | 说明 |
|-----|------|---------|-------------|
| `project_picker_disabled` | bool | `false` | 为 `true` 时，Chaos 从非项目目录（主目录、桌面、下载、`/tmp`）启动的首个提示不再弹出挑选项目目录的选择器。在该选择器里选过 **"Don't ask me again"（别再问我）** 后会自动置位。团队可在 `managed_config.toml` 或 `requirements.toml` 中钉死。 |
| `memory_modal_fullscreen` | bool | `false` | 记录记忆弹窗上次是否以全屏方式打开。 |
| `new_session_worktree_mode` | string | `"never"` | `/new` 的工作树提示：`ask` 弹窗询问，`always` 创建 worktree，`never` 跳过。 |
| `fork_worktree_mode` | string | `"ask"` | `/fork` 的工作树提示：`ask`、`always` 或 `never`。 |

### 通知

当智能体完成一个回合或需要批准时发出终端通知。它们使用终端原生协议（OSC 9、OSC 99、OSC 777 或 BEL），并且默认受焦点门控，因此只在你没看着终端的时候触发。

```toml
[ui.notifications]
method = "auto"           # auto|osc9|osc99|osc777|bel|none
condition = "unfocused"   # unfocused|always|never
idle_threshold_secs = 3   # seconds unfocused before a notification fires
events = ["turn_complete", "approval_required"]
sleep_prevention = true   # prevent display sleep during agent turns
progress_bar = true       # show tab progress bar (OSC 9;4)

[ui.notifications.title]
enabled = true
items = ["action-required", "spinner", "activity", "session-name", "grok"]
```

| 选项 | 类型 | 默认 | 说明 |
|--------|------|---------|-------------|
| `method` | string | `"auto"` | 通知协议。`auto` 会为你的终端挑最合适的那个。 |
| `condition` | string | `"unfocused"` | 何时通知：`unfocused`（仅在终端失去焦点时）、`always` 或 `never`。 |
| `idle_threshold_secs` | integer | `3` | 失去焦点至少多少秒后才发出通知。 |
| `events` | array | `["turn_complete", "approval_required"]` | 触发通知的事件。可选值：`turn_complete`、`approval_required`、`session_ready`、`task_complete`、`agent_error`。 |
| `sleep_prevention` | bool | `true` | 智能体工作期间保持屏幕不休眠（macOS/Linux）。 |
| `progress_bar` | bool | `true` | 在终端标签里显示进度指示（OSC 9;4）。 |
| `title.enabled` | bool | `true` | 把终端标题设为反映智能体状态。 |
| `title.items` | array | （见上） | 标题栏显示的条目。可选值：`action-required`、`spinner`、`activity`、`session-name`、`cwd`、`model`、`turn-timer`、`grok`（沿用上游段名，渲染出来是本分叉品牌 `Chaos Code`）。 |

#### 终端支持矩阵

| 终端 | 自动协议 | 焦点跟踪 | 进度条 |
|----------|---------------|----------------|--------------|
| iTerm2 | OSC 9 | 是 | 是 |
| Kitty | OSC 99 | 是 | 否 |
| Ghostty | OSC 777 | 是 | 是 |
| WezTerm | OSC 9 | 是 | 是 |
| Warp | OSC 9 | 是 | 否 |
| Alacritty | BEL | 是 | 否 |
| VS Code | BEL | 是 | 否 |
| Apple Terminal | BEL | 否 | 否 |
| VTE（GNOME 终端） | OSC 777 | 是 | 否 |
| 上游 Grok Desktop | 无（原生） | N/A | N/A |
| 未知 | BEL | 否 | 否 |

`method = "auto"` 时 Chaos 会识别终端品牌并选出最合适的协议；显式设置 `method` 可覆盖它。

#### 通知钩子

事件触发时运行你自己的命令。钩子会在环境变量中拿到 `$GROK_EVENT`、`$GROK_MESSAGE` 和 `$GROK_SESSION_ID`。

```toml
# macOS native notification
[[ui.notifications.hooks]]
command = "terminal-notifier -title 'Grok' -message '$GROK_MESSAGE'"
events = ["turn_complete", "approval_required"]
only_unfocused = true
timeout_secs = 10

# Push to ntfy server
[[ui.notifications.hooks]]
command = "curl -s -d '$GROK_MESSAGE' ntfy.sh/my-grok-alerts"
events = ["turn_complete"]
only_unfocused = true
timeout_secs = 10

# Play a sound
[[ui.notifications.hooks]]
command = "afplay /System/Library/Sounds/Glass.aiff"
events = ["turn_complete"]
only_unfocused = true
timeout_secs = 5
```

| 钩子选项 | 类型 | 默认 | 说明 |
|-------------|------|---------|-------------|
| `command` | string | （必填） | 要运行的 shell 命令。 |
| `events` | array | `[]` | 触发该钩子的事件（留空 = 所有事件）。 |
| `only_unfocused` | bool | `true` | 仅在终端失去焦点时触发。 |
| `timeout_secs` | integer | `10` | 超过这么多秒后杀掉钩子进程。 |

#### 常见问题

**tmux 里通知不工作：** tmux 默认拦截转义序列，需要打开 passthrough：

```bash
# In ~/.tmux.conf
set -g allow-passthrough on
```

之后重启 tmux。若所用 tmux 不支持 passthrough（tmux < 3.3），改用 `method = "bel"`，它无需 passthrough 也能工作。

**焦点跟踪不工作：** 有些终端不上报焦点事件。若 `condition = "unfocused"` 从不触发，试试 `condition = "always"`。除 Apple Terminal 与无法识别的终端外，Chaos 在所有可识别终端上都支持焦点跟踪。

**防休眠不生效：** macOS 上防休眠通过 CoreFoundation 的 `IOPMAssertionCreateWithName` 实现；Linux 上则用 `systemd-inhibit`（必须在 `$PATH` 中）。确认相应工具可用。防休眠只在智能体回合期间生效，回合结束后自动释放。

### 状态行

全屏 pager 底部可选的一行，默认关闭。用 `[ui.status_line]` 打开：

```toml
[ui.status_line]
type = "builtin"                # builtin | command | disabled
items = ["cwd", "model", "context"]
```

其余键为 `items`（按顺序显示哪些内置段）、`command`、`padding` 与 `refresh_interval`（单位秒；按定时器重跑 `command` 行，好让事故页面或 CI 状态也能送进空闲会话）。[状态行指南](25-status-line.md) 记录了全部键，以及 `command` 脚本从 stdin 读取的 JSON 约定和一个示例脚本。

极简模式没有状态行，改用终端标签标题（见[通知](#notifications) 的 `title.items`）。

### 键盘快捷键

键盘快捷键**不可配置**——所有绑定都是内置的。完整参考见[键盘快捷键](03-keyboard-shortcuts.md)。

### 遥测

这些是彼此独立的开关（见[用量监控](24-monitoring-usage.md#related-settings)）：

- **`[features] telemetry`** / `GROK_TELEMETRY_ENABLED` —— 产品分析的总开关。`/privacy` 不改动它。
- **`/privacy`** / 设置 —— 代码数据分享，与遥测相互独立。
- **`[telemetry] trace_upload`** / `GROK_TELEMETRY_TRACE_UPLOAD` —— 会话轨迹；未设置时跟随遥测开关。
- **`[telemetry] otel_*`** / `GROK_EXTERNAL_OTEL` —— 发往你自己 collector 的外部 OTEL（见下文）。

遥测打开时，自建 collector 的企业可以把它重定向，或按 `[telemetry]` 下的项关掉其中一部分：

```toml
[telemetry]
events_url = "https://telemetry.your-company.com/events"  # send events to your own collector
events_api_key = "your-collector-token"                   # auth for your collector, if required
mixpanel_enabled = false                                  # disable Mixpanel product analytics
trace_upload = false                                      # disable session/trace uploads (inherits the telemetry toggle when unset)
```

只在要把遥测指向自己的基础设施或关掉部分内容时才设置这些项。内置端点与凭据由 Chaos 托管——保持未设置即用默认值。

同一个 `[telemetry]` 表还配置**外部 OpenTelemetry 流**：一个独立的开关（不依赖上面的遥测总开关），把一套经过筛选、不含内容的用量 schema 送进你*自己的* OTLP collector。Collector 认证来自 `OTEL_EXPORTER_OTLP_HEADERS`，从不落盘。完整 schema、环境变量与隐私模型见[监控与用量](24-monitoring-usage.md)。

```toml
[telemetry]
otel_enabled = true                                       # external OTEL master switch (= GROK_EXTERNAL_OTEL)
otel_metrics_exporter = "otlp"                            # otlp | console | none
otel_logs_exporter = "otlp"                               # otlp | console | none
otel_endpoint = "https://collector.corp.example:4318"     # OTLP base endpoint
otel_protocol = "http/protobuf"                           # http/protobuf | grpc
otel_certificate = "/etc/ssl/corp-ca.pem"                 # optional: trust private CA (path only)
otel_client_certificate = "/etc/ssl/client.crt"           # optional: mTLS client cert (path only)
otel_client_key = "/etc/ssl/client.key"                   # optional: mTLS client key (path only)
otel_log_user_prompts = false                             # content gate (admins pin via requirements)
otel_log_assistant_responses = false                      # unset follows prompts; pin false for prompts-only
otel_log_tool_details = true                              # metadata/preview; enterprise default on for SIEM join
otel_log_tool_content = false                             # full-body gate; independent of details — does not imply names/paths
```

### 企业部署

一份完整的企业配置：

```toml
[cli]
auto_update = false

[auth]
auth_provider_command = "/usr/local/bin/my-company-auth-provider"
auth_provider_label = "Acme Corp"
auth_token_ttl = 3600

[models]
default = "company-grok"

[model.company-grok]
model = "grok-4.6"
base_url = "https://grok-proxy.acme.com/"
name = "Grok 4.6 (Proxy)"
context_window = 128000

[features]
telemetry = false
```

---

## pager.toml（外观配置）

位置：`~/.chaos/pager.toml`（兼容旧名 `~/.grok/pager.toml`，双读）。该文件控制 TUI 的外观，重启后生效。

### 终端

```toml
[terminal]
alt_screen = "auto"                   # fullscreen mode: "auto", "always", "never"
```

- `auto`（默认）：终端支持时使用备用屏幕。
- `always`：总是使用备用屏幕。
- `never`：直接在终端主滚动缓冲区里内联运行。

### 动画

```toml
[animation]
fps = 30                              # animation frame rate (ticks per second)
wave_rows = 32                        # rows per wave cycle for accent animation
```

### 提示符

```toml
[prompt]
collapse_unfocused = true             # collapse prompt when scrollback is focused
mouse_hover = true                    # show hover highlight on the prompt widget
show_prefix = true                    # show the prompt prefix character
```

紧凑模式不在这里持久化——运行时用 `[ui] compact_mode` 或 `/compact-mode` 命令控制。

### 滚动缓冲

```toml
[scrollback.layout]
outer_vpad = 1                        # vertical padding
outer_hpad_left = 2                   # left horizontal padding
outer_hpad_right = 2                  # right horizontal padding
block_pad_left = 2                    # padding inside block, left of content
block_pad_right = 2                   # padding inside block, right of content

[scrollback.scrollbar]
enabled = true                        # show scrollbar
gap_left = 0                          # gap between content and scrollbar
gap_right = 0                         # gap between scrollbar and screen edge

[scrollback.scroll]
margin = 0                            # minimum context lines above/below selection
min_page_fraction = 0                 # minimum scroll as % of viewport (0-100)
follow_indicator = "center"           # follow indicator: "center" or "none"
follow_auto_select = true             # auto-select latest entry in follow mode
follow_by_overscroll = true           # scrolling past bottom engages follow mode
anchor_on_fold = true                 # keep block position when folding
respect_manual_folds = true           # opt-in (default: false): keep manually folded blocks as-is during streaming/finish; expanding while following stops auto-scroll

[scrollback.display]
sticky_headers = true                 # pin user prompts as sticky headers
tab_width = 4                         # spaces per tab character
expandable_indicator = true           # show expand indicator on foldable entries
expandable_indicator_running = true   # show indicator on running entries
expandable_indicator_char = "›"       # character for the expand indicator (default: "›")
selection_buttons = false             # show copy/view buttons on selection
line_under_last_entry = false         # horizontal line below last entry
group_selection_split = true          # split selection box for expanded blocks
highlight_overlays_border = false     # highlight extends over selection box border
dim_accent = 0.5                      # dimming factor for collapsed accents (0.0-1.0)
```

`respect_manual_folds` 默认关闭。打开后，你手动折叠的块会被钉住：流式更新与完成事件（比如 thinking 块结束）不会改动它的折叠状态；在 follow 模式追踪新内容时展开某个块，会停下自动滚动，让视图留在原处。按 `Shift+G`、在最后一条上按 `j`、向下滚过底部，或发送新的提示，都会恢复 follow。`Shift+E` 清除所有钉子；`Ctrl+E` 只清除 thinking 块上的钉子。

### 块配置

```toml
[scrollback.blocks.edit]
indent = true                         # indent diff content
vpad = false                          # vertical padding
# expanded_by_default = true          # unset: follows [ui] collapsed_edit_blocks in config.toml
                                      # (flag on = collapsed one-liner); uncomment to pin either shape
dual_line_numbers = false             # two-column line numbers (old + new)
# line_summary = false                # show +N/-M in the collapsed header; unset follows the same flag
hunk_separator = "…"                  # separator between diff hunks (default: "…")

[scrollback.blocks.prompt]
vpad = true                           # vertical padding
show_prefix = true                    # show prompt prefix character
min_lines = 2                         # minimum content lines in sticky mode

[scrollback.blocks.thinking]
animate = true                        # animated accent while thinking
truncated_lines = 3                   # lines in truncated mode
```

### 插件

```toml
disable_plugins = false               # hide hooks/plugins UI entirely
```

---

## 环境变量

以下是关键项，完整列表见 README。

### 认证

| 变量 | 说明 |
|----------|-------------|
| `XAI_API_KEY` | 来自 console.x.ai 的 API 密钥 |
| `GROK_AUTH_PROVIDER_COMMAND` | 外部认证程序路径 |
| `GROK_AUTH_PROVIDER_LABEL` | TUI 登录界面上显示的名字 |
| `GROK_AUTH_TOKEN_TTL` | token 有效期（秒） |
| `GROK_AUTH_EARLY_INVALIDATION_SECS` | 到期前多少秒刷新（默认 300） |
| `GROK_OIDC_ISSUER` | OIDC issuer URL（上游遗留项） |
| `GROK_OIDC_CLIENT_ID` | OIDC client ID（上游遗留项） |

### 端点

| 变量 | 说明 |
|----------|-------------|
| `GROK_CLI_CHAT_PROXY_BASE_URL` | 覆盖 API 代理的 base URL |

### 功能开关

| 变量 | 说明 |
|----------|-------------|
| `GROK_MEMORY` | 启用（`1`）或禁用（`0`）跨会话记忆 |
| `GROK_SUBAGENTS` | 启用（`1`）或禁用（`0`）子智能体 |
| `GROK_WORKFLOWS` | 启用（`1`）或禁用（`0`）后台工作流，并选择 `/goal` 的驱动方式（默认关：沿用旧的 `update_goal`；打开：由宿主的工作流驱动） |
| `GROK_WEB_FETCH` | 启用（`1`）或禁用（`0`）web_fetch 工具 |
| `GROK_WEB_FETCH_ALLOW_LOCAL` | 只允许 `web_fetch` 访问显式写出的回环地址（`localhost` / `127.0.0.0/8` / `::1`）。等同于 `[toolset.web_fetch] allow_local`。默认关闭；私有网段与 metadata 地址仍被拦截。 |
| `GROK_AGENT` | 自定义智能体定义的路径或名字 |
| `GROK_SANDBOX` | 沙箱 profile（off、workspace、devbox、read-only、strict，或自定义 profile 名） |
| `GROK_EXIT_TIMEOUT_SECS` | 请求退出后，若收尾卡住，多少秒后强制退出进程（默认 20，`0` 表示禁用；5 秒后硬退出） |

### 日志

| 变量 | 说明 |
|----------|-------------|
| `GROK_LOG_FILE` | 把日志写到这个文件路径（该值原样用作路径） |
| `RUST_LOG` | 日志级别过滤（如 `debug`）；同时控制 `GROK_LOG_FILE` 日志与无头模式的 stderr 输出 |

### 路径

| 变量 | 说明 |
|----------|-------------|
| `CHAOS_HOME` | 覆盖配置目录（Chaos 侧首选，优先级最高） |
| `GROK_HOME` | 覆盖配置目录（旧名，兼容保留；`CHAOS_HOME` 未设置时生效）。默认双读顺序：存在的 `~/.chaos`，否则存在的 `~/.grok`，否则 `~/.chaos` |
| `GROK_RESPECT_GITIGNORE` | 强制打开（`1`）或关闭（`0`）gitignore 过滤；覆盖 `[tools] respect_gitignore` |

### 遥测

| 变量 | 说明 |
|----------|-------------|
| `GROK_TELEMETRY_ENABLED` | 启用/禁用遥测 |
| `GROK_TELEMETRY_TRACE_UPLOAD` | 启用/禁用会话轨迹上传 |
| `GROK_TELEMETRY_MIXPANEL_ENABLED` | 单独启用/禁用 Mixpanel |
| `GROK_EXTERNAL_OTEL` | 发往你自己 collector 的外部 OTEL（见 [24-monitoring-usage.md](24-monitoring-usage.md)） |
| `GROK_FEEDBACK_ENABLED` | 启用/禁用反馈系统 |
| `GROK_DEPLOYMENT_KEY` | 企业用的管理 API 密钥 |

---

## 文件位置

下文所说的「用户主目录」指解析后的配置根（`$CHAOS_HOME` / `$GROK_HOME`；为兼容旧安装，双读 `~/.chaos` 或 `~/.grok`）。下表路径仍按旧形式 `~/.grok/...` 书写；若生效的是 `~/.chaos`，请自行替换。

| 路径 | 说明 |
|------|-------------|
| `~/.chaos/config.toml` 或 `~/.grok/config.toml` | 主配置文件（`.grok` 为兼容旧名，双读时 `.chaos` 优先） |
| `~/.chaos/pager.toml` 或 `~/.grok/pager.toml` | TUI 外观配置 |
| `~/.chaos/auth.json` 或 `~/.grok/auth.json` | 认证凭据（自动管理） |
| `~/.chaos/sessions/` 或 `~/.grok/sessions/` | 已持久化的会话（按工作目录组织） |
| `~/.chaos/memory/` 或 `~/.grok/memory/` | 跨会话记忆文件与索引 |
| `~/.chaos/skills/` 或 `~/.grok/skills/` | 用户级技能定义 |
| `~/.chaos/plugins/` 或 `~/.grok/plugins/` | 用户级插件 |
| `~/.chaos/agents/` 或 `~/.grok/agents/` | 用户级智能体定义 |
| `~/.chaos/lsp.json` 或 `~/.grok/lsp.json` | LSP 服务器配置（用户级） |
| `~/.chaos/logs/` 或 `~/.grok/logs/` | 内部日志文件（如 `unified.jsonl`、MCP 服务器日志） |
| `.chaos/config.toml` 或 `.grok/config.toml` | 项目级 MCP 服务器、插件与权限规则（两者兼容双读；同名冲突时 Chaos 侧优先） |
| `.chaos/skills/` 或 `.grok/skills/` | 项目级技能定义 |
| `.chaos/plugins/` 或 `.grok/plugins/` | 项目级插件 |
| `.chaos/agents/` 或 `.grok/agents/` | 项目级智能体定义 |
| `.chaos/hooks/` 或 `.grok/hooks/` | 项目级钩子 |
| `.chaos/lsp.json` 或 `.grok/lsp.json` | LSP 服务器配置 |

---

## 项目级配置

项目级配置放在仓库内的 `.chaos/`（为兼容旧安装，也读 `.grok/`；双读，同名冲突时 Chaos 侧优先）：

| 文件 | 配置了什么 |
|------|--------------------|
| `.chaos/config.toml` 或 `.grok/config.toml` | MCP、插件、权限与 `[mcp] max_output_bytes`（其余 section 只从用户 `config.toml` 加载；`.grok` 为兼容旧名） |
| `.chaos/skills/` 或 `.grok/skills/` | 项目级技能 |
| `.chaos/hooks/` 或 `.grok/hooks/` | 项目级钩子 |
| `.chaos/agents/` 或 `.grok/agents/` | 项目级智能体定义 |
| `.chaos/lsp.json` 或 `.grok/lsp.json` | LSP 服务器配置 |
| `.chaos/sandbox.toml` 或 `.grok/sandbox.toml` | 沙箱 profile |
| `AGENTS.md` | 项目指令（系统提示） |

同名项目级 MCP 会覆盖全局配置（整段替换，非字段合并）。

---

## LSP 服务器

语言服务器为被动诊断和可选的 `lsp` 工具提供支持（见 [`lsp_tools`](#general-settings) 功能开关）。定义来自三个来源，按服务器名合并：

| 来源 | 位置 | 作用域 |
|--------|----------|-------|
| 用户 | `~/.chaos/lsp.json`（兼容 `~/.grok/lsp.json`） | 所有项目 |
| 项目 | `.chaos/lsp.json`（兼容 `.grok/lsp.json`） | 当前仓库 |
| 插件 | 受信任插件的 `.lsp.json` 文件，或它 `plugin.json` 里的内联 `lspServers` 块 | 插件启用的任何位置 |

当同一个服务器名来自多个来源时，按优先级从高到低解析：

1. **项目** — `.chaos/lsp.json`（兼容 `.grok/lsp.json`）
2. **用户** — `~/.chaos/lsp.json`（兼容 `~/.grok/lsp.json`）
3. **插件** —— 基于文件的 `.lsp.json`，然后是内联 `lspServers`，按插件加载顺序

项目与用户条目会替换同名的低优先级条目。插件条目只补充本地文件尚未定义的名字，所以本地的 `lsp.json` 总是胜过插件。插件的 LSP 服务器只在插件被信任后才加载（见[插件](09-plugins.md)）。
