# MCP 服务器

MCP（Model Context Protocol）服务器为 Chaos 接入外部工具。可与任何实现 MCP 标准的服务交互。

> Chaos **没有** grok.com 远程连接器门户。请在用户或项目 `config.toml` 的
> `[mcp_servers]` 中配置本地/远程 MCP。扩展面板中的「托管 MCP」指本地托管分类，
> 不是 xAI 云托管。

---

## 什么是 MCP 服务器？

MCP 服务器通过标准协议向 Chaos 暴露工具。配置后，其工具与内置工具一并提供给模型，可在会话中发现并调用。

例如 GitHub MCP 可提供 `create_issue`、`list_pull_requests`、`search_code`；数据库服务器可提供 `query`、`list_tables`、`describe_schema`。

协议细节见 [MCP specification](https://modelcontextprotocol.io)。

---

## 配置

在用户配置根的 `config.toml`（`~/.chaos` 或兼容 `~/.grok`）中配置
`[mcp_servers.<name>]`。项目级见 `.chaos/config.toml` / `.grok/config.toml`。

要把 MCP 服务器分发给团队，或限制用户能运行哪些服务器（在 `requirements.toml` /
`managed_config.toml` 里用 `allowedMcpServers` / `deniedMcpServers`；对由外部来源
定义的服务器，另有 Claude `managed-settings.json` 的劝告性设置），见插件指南中的
[跨组织分发](09-plugins.md#跨组织分发)。

### stdio 传输（本地进程）

Chaos 拉起本地进程，经 stdin/stdout 通信：

```toml
[mcp_servers.my-server]
command = "/path/to/server"           # Server executable
args = ["--flag", "value"]            # Command arguments
env = { API_KEY = "sk-..." }          # Environment variables
enabled = true                        # Enable or disable the server (default: true)
startup_timeout_sec = 30              # Server startup timeout, seconds (default: 30)
tool_timeout_sec = 6000               # Per-tool-call timeout fallback, seconds (default: 6000)
tool_timeouts = { slow_op = 120 }     # Per-tool timeout overrides, seconds
```

> **全局启动超时覆盖：** 不必逐个服务器设置 `startup_timeout_sec`，可以用 `MCP_TIMEOUT`
> 环境变量（毫秒，兼容 Claude Code）或 `GROK_MCP_STARTUP_TIMEOUT_SECS`（秒）改动所有
> 服务器的默认值。单个服务器的 `startup_timeout_sec` 仍然优先于这两者。首次启动就要
> 下载包的 `npx`/`uvx` 冷启动服务器常常需要调大；默认 30 秒。
>
> **MCP 工具结果大小上限：** 过大的 MCP / `use_tool` 结果会被就地截断（完整内容落到
> 会话的 `mcp/` 目录下）。默认 **20_000 字节**。覆盖方式：
>
> - 环境变量 `GROK_MAX_MCP_OUTPUT_BYTES` 或 `MAX_MCP_OUTPUT_BYTES`（单位字节；两个都
>   设时以 Grok 原生名为准；后者是 Claude 风格的命名，但我们的上限按**字节**计，
>   不是 token）
> - `config.toml` —— 用户级（`~/.chaos/config.toml`）**或仓库级**
>   （cwd → git 根链路上任意位置的 `.chaos/config.toml`；最深的那份生效，而仓库级
>   的取值只在该目录被信任后才起作用）：
>
> ```toml
> [mcp]
> max_output_bytes = 40000
> ```
>
> 优先级：requirements.toml > 环境变量 > 仓库 `.chaos/config.toml` >
> 用户/托管配置 > 默认值。改动仓库里的配置会通过配置热重载作用于该目录下正在运行的
> 会话。

### HTTP/SSE 传输（远程服务器）

能通过 HTTP 访问的远程 MCP 服务器：

```toml
[mcp_servers.remote-api]
url = "https://mcp.example.com/api"
headers = { "Authorization" = "Bearer token" }
```

MCP 数据面请求（JSON-RPC 与 SSE）以及匿名访问探测默认带一个
`User-Agent: grok-cli/<version>` 头，`<version>` 是 Grok 二进制的版本号。OAuth 发现、
客户端注册和取 token 的请求由 rmcp 的 OAuth 客户端发出，保持它自己的行为（不带默认
`User-Agent`）。服务器 `headers` 里配了合法的 `User-Agent` 会覆盖默认值；配了非法的
`User-Agent` 值会在解析请求头时被丢弃（并给出警告），因此该服务器仍会收到默认值。例外：Figma MCP
服务器（服务器名 `figma`、旧版托管名 `grok_com_figma`，或主机为 `figma.com`——都不区分
大小写）会收到不带版本号的裸 token `grok-cli`，除非配置里自带 `User-Agent`。

### 带会话 ID 的 Streamable HTTP

```toml
[mcp_servers.my-streamable-server]
url = "https://mcp.example.com/api/mcp"
headers = { "x-mcp-session-id" = "{{session_id}}" }
```

---

## 命令行管理

不用改配置文件，直接从命令行管理 MCP 服务器：

```bash
# List configured MCP servers
chaos mcp list
chaos mcp list --json          # Machine-readable output

# Add a stdio server. Everything after -- is the server command, so flags
# like -y reach the server instead of being parsed by chaos.
chaos mcp add filesystem -- npx -y @modelcontextprotocol/server-filesystem /path/to/dir

# Add a stdio server with environment variables (-e is repeatable)
chaos mcp add postgres -e DATABASE_URL=postgres://localhost/mydb -- npx -y @modelcontextprotocol/server-postgres

# Add a remote HTTP server
chaos mcp add --transport http sentry https://mcp.sentry.dev/mcp

# Add a remote server with an authentication header (--header is repeatable)
chaos mcp add --transport http api https://mcp.example.com/mcp --header "Authorization: Bearer YOUR_TOKEN"

# Add a remote SSE server
chaos mcp add --transport sse linear https://mcp.linear.app/sse

# Remove a server
chaos mcp remove github

# Enable or disable a local/TOML (or compat-sourced) server
chaos mcp enable github
chaos mcp disable github

# Diagnose a server's configuration and connectivity
chaos mcp doctor               # Check every configured server
chaos mcp doctor github        # Check one server
chaos mcp doctor --json        # Machine-readable output
```

传输方式默认 `stdio`；远程服务器请加 `--transport http` 或 `--transport sse`。

默认情况下 `chaos mcp add` 写入 `~/.chaos/config.toml`（`--scope user`）。加
`--scope project` 改为写入当前目录的 `.chaos/config.toml`，这份文件可以提交并与团队
共享（见[项目级 MCP 服务器](#项目级-mcp-服务器)）。请求头和环境变量的值按原样
保存，所以密钥请写成 `${VAR}` 引用，不要直接粘进会被提交的项目配置里（见
[配置示例](#配置示例)）。`chaos mcp list` 会列出两个作用域里的服务器，
项目级的标注 `(project)`，已停用的标注 `(disabled)`。

`chaos mcp remove` 在两个作用域里查找，删除成功后退出码为 0。名字找不到，或者用户在
用户级和项目级都定义了同名服务器时，退出码为 1——这时用 `--scope` 指明删哪一个。

`chaos mcp enable` / `disable` 把个人的启停状态持久化到用户级
`~/.chaos/config.toml`（`disabled_mcp_servers`，以及该条目存在时的
`[mcp_servers.<name>].enabled`）。作用范围：

- **已知名字：** 用户/项目级的原生 TOML 配置、已在停用列表里的名字、兼容来源
  （`.mcp.json`、Claude、Cursor），以及**插件**提供的 MCP 服务器（发现范围与
  doctor/`/mcps` 一致）。
- **只有 `enable` 会写项目配置：** 如果 cwd 最近的那份项目定义里写死了
  `enabled = false`，只清掉这一个键（保留注释）；`disable` 从不改写项目配置。
- **与 `/mcps` 并非完全等价：** 网关连接器（`managed_gateway:…`，存放在
  `disabled_mcp_tools.__managed_gateway_connectors` 下）在 TUI 里仍然只能在面板上用
  空格键操作。该命令是幂等的；名字未知时退出码为 1。

与更早版本相比的破坏性变更：`--env` 现在每个标志只接受一个 `KEY=value`（用
`-e A=1 -e B=2`，而不是 `--env A=1 B=2`）；服务器名只能包含字母、数字、连字符和下划线。

---

## 项目级 MCP 服务器

在仓库里放一份 `.chaos/config.toml`，就能按项目配置 MCP 服务器：

```
my-project/
  .chaos/
    config.toml
  src/
  ...
```

```toml
# .chaos/config.toml
[mcp_servers.linear]
url = "https://mcp.linear.app/mcp"
enabled = true
```

当服务器本身提供 HTTP/SSE 端点时，优先用 `url` 形式，而不是用 `npx mcp-remote <url>`
这类 stdio 代理包一层。Grok 直接处理 HTTP/SSE 和 OAuth，用原生形式可以省掉每个会话
多起一个子进程，同时还会向服务商注册 Grok 自己的 OAuth 客户端。

Grok 从当前目录逐级向上走到 git 仓库根，每层都加载 `.chaos/config.toml`：

| 位置 | 作用域 | 优先级 |
|----------|-------|----------|
| `~/.chaos/config.toml` | 所有项目 | 最低 |
| `<repo-root>/.chaos/config.toml` | 本仓库 | 中 |
| `<cwd>/.chaos/config.toml` | 当前目录 | 最高 |

如果项目里定义的服务器与全局同名，项目里的定义会整体替换全局定义（字段不合并）。

项目级的文件只贡献 `[mcp_servers]`、`[plugins]` 和 `[permission]` 三类条目；其他大多数
配置段 Grok 只从 `~/.chaos/config.toml` 读取。

---

## 工具命名

MCP 工具以服务器名作命名空间，避免重名：

- 服务器 `filesystem` 的 `read_file` 工具成为 `filesystem__read_file`
- 服务器 `github` 的 `create_issue` 工具成为 `github__create_issue`

---

## 运行时开关服务器

不用重启 Grok 就能启停 MCP 服务器（TUI 里用 `/mcps`，或用命令行——见
[命令行管理](#命令行管理)）。

### /mcps 面板

在 TUI 里打开 MCP 服务器面板：

- 作为斜杠命令运行 `/mcps`
- 或者按 `Ctrl+L`（VS Code 系列除外）再切到 MCP 服务器标签页；VS Code 系列请用
  `/plugins` 或 `/mcp`，然后打开 MCP 服务器标签页

在面板里可以：

- 查看每个服务器的来源、启用状态和工具数量
- 用 `Space` 启用或停用某个服务器
- 展开某个服务器，查看它提供哪些工具
- 改完 `config.toml` 后按 `r` 刷新列表
- 按 `i` 为需要 OAuth 的服务器完成认证
- 按 `a` 添加服务器，或用 `x` 删除本地服务器（面板会要求确认；按小写 `y` 确认删除，
  按其他任意键取消）

### 工具发现

模型有两个内置工具用于操作 MCP 服务器：

- `search_tool` —— 在所有已启用的 MCP 服务器中发现可用的集成工具。按名字或描述查找。
- `use_tool` —— 调用通过 `search_tool` 发现的集成工具。需要给出完整限定的工具名
  （例如 `github__create_issue`）。

---

## 兼容性

为了兼容，Grok 会从多个来源加载 MCP 服务器配置：

| 来源 | 格式 | 位置 | 可配置 |
|--------|--------|----------|-------------|
| `config.toml` | Grok 原生配置 | `~/.chaos/config.toml`、`.chaos/config.toml` | 始终开启 |
| `.claude.json` | Claude Code 格式 | `~/.claude.json` | `[compat.claude] mcps` |
| `.cursor/mcp.json` | Cursor 格式 | `~/.cursor/mcp.json`, `<project>/.cursor/mcp.json` | `[compat.cursor] mcps` |
| `.mcp.json` | MCP 标准格式 | 项目根（cwd 到 git 根） | 除非你已经导入或忽略过 Claude 导入提示（导入标记已写入），否则会加载 |

各来源按优先级合并：config.toml > Claude > Cursor > `.mcp.json`。名字冲突时，优先级高的
来源胜出。

Claude 和 Cursor 两个 MCP 来源默认会被扫描。要关掉某个来源，在
`~/.chaos/config.toml` 里设置 `[compat.<vendor>] mcps = false`，或改用对应的环境变量
（`GROK_CURSOR_MCPS_ENABLED`、`GROK_CLAUDE_MCPS_ENABLED`）。详见
[配置](05-configuration.md#厂商兼容性开关)。用 `chaos inspect` 可以看到加载了哪些
MCP 服务器以及它们的来源（`[cursor]`、`[claude]`）。

---

## MCP OAuth

对于需要 OAuth 认证的 MCP 服务器，Grok 会自动处理凭据流程。当 MCP 服务器要求 OAuth
凭据时，Grok 会打开浏览器授权流程，并把拿到的 token 存下来备用。

---

## 配置示例

托管的 MCP 服务器用 `url` 形式，本地 stdio 工具用 `command` / `args` 形式。

### 原生 HTTP（托管服务）

基于 OAuth 的 MCP 服务器必须先完成认证才能使用。Grok 把拿到的 token 存放在
`~/.chaos/mcp_credentials.json`，本地明文保存，文件权限仅限属主（Unix 下为 `0600`）。
建议宿主开启全盘加密。改完 `config.toml` 后，在 `/mcps` 面板里按 `r` 刷新服务器列表。

```toml
[mcp_servers.linear]
url = "https://mcp.linear.app/mcp"
enabled = true

[mcp_servers.sentry]
url = "https://mcp.sentry.dev/mcp"
enabled = true

[mcp_servers.mixpanel]
url = "https://mcp.mixpanel.com/mcp"
enabled = true
```

用静态 bearer token 而不是 OAuth 认证的内部或自建服务器，请显式设置 `Authorization` 头：

```toml
[mcp_servers.internal-tools]
url = "https://mcp.internal.example.com/mcp"
enabled = true

[mcp_servers.internal-tools.headers]
Authorization = "Bearer <token>"
```

为了不把密钥写进配置文件，可以用 `${VAR}`（或 `${VAR:-default}`）引用环境变量。加载时
Grok 会展开 `[mcp_servers.*]` 里的字符串字段——`url`、`command`、`args`，以及 `env` 和
`headers` 的值：

```toml
[mcp_servers.internal-tools]
url = "https://mcp.internal.example.com/mcp"
enabled = true
headers = { "Authorization" = "Bearer ${INTERNAL_MCP_TOKEN}" }
```

### 本地 stdio

必须在本地运行的工具（文件系统访问、本地数据库、自建服务）用 stdio。

```toml
# Filesystem access scoped to a directory
[mcp_servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/allowed/directory"]

# Local Postgres
[mcp_servers.postgres]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-postgres", "postgresql://user:pass@localhost/db"]

# Custom server with a longer startup timeout and tuned per-tool timeouts
[mcp_servers.my-tools]
command = "/usr/local/bin/my-mcp-server"
args = ["--config", "/etc/my-mcp.json"]
startup_timeout_sec = 30
tool_timeout_sec = 120
tool_timeouts = { slow_analysis = 300, quick_lookup = 10 }
```

在 Windows 上，npm 把 `npx`、`npm`、`pnpm`、`yarn` 这类启动器装成 `.cmd` 批处理垫片
（并不存在 `npx.exe`）。拉起进程之前，Grok 会把 `npx` 这样光秃秃的 `command` 解析成
`PATH` 上的真实启动器路径（遵循 `PATHEXT`），所以不必手动用 `cmd /c` 包一层。写成绝对
路径、或含路径分隔符的 `command` 会按原样使用。

---

## 可用的 MCP 服务器

下面是一份不完整的清单，都可以用上文介绍的 `url` 或 `command` 形式配置。使用前请向各
服务商确认当前的端点或包名：

| 服务器 | 传输 | 端点 / 包名 |
|--------|-----------|--------------------|
| Linear | HTTP (OAuth) | `https://mcp.linear.app/mcp` |
| Sentry | HTTP (OAuth) | `https://mcp.sentry.dev/mcp` |
| Mixpanel | HTTP (OAuth) | `https://mcp.mixpanel.com/mcp` |
| 文件系统 | stdio | `@modelcontextprotocol/server-filesystem` |
| Git | stdio | `@modelcontextprotocol/server-git` |
| GitHub | stdio | `@modelcontextprotocol/server-github` |
| GitLab | stdio | `@modelcontextprotocol/server-gitlab` |
| PostgreSQL | stdio | `@modelcontextprotocol/server-postgres` |
| SQLite | stdio | `@modelcontextprotocol/server-sqlite` |
| Puppeteer | stdio | `@modelcontextprotocol/server-puppeteer` |

完整社区服务器清单见 [MCP Server Registry](https://github.com/modelcontextprotocol/servers)，协议细节见 [MCP specification](https://modelcontextprotocol.io)。

---

## 子代理与 MCP

子代理默认继承父会话已连接的 MCP 服务器，插件提供的代理也一样。可以用代理 frontmatter
里的 `mcpInheritance` 收窄这个集合（`all`、`none`、`named`、`except`）。详见
[子代理 —— MCP 继承](16-subagents.md#mcp-继承)。

如果子代理明明列出了 `search_tool` / `use_tool`，返回的工具目录却是空的，请检查：

1. 父会话是否真的连上了该服务器（见扩展面板 / `chaos inspect`）
2. 该代理的 `mcpInheritance` 是否为 `none`，或用了排除该服务器的过滤器
3. 插件代理不能在 frontmatter 里自行声明 `mcpServers`——它们只能看到父会话已连接的服务器

---

## 故障排查

### 服务器起不来

```bash
# Test the server command manually
npx -y @modelcontextprotocol/server-filesystem /path

# Increase startup timeout
# In config.toml:
[mcp_servers.filesystem]
startup_timeout_sec = 30
```

对 stdio 服务器，Grok 会把进程的标准错误输出抓到
`~/.chaos/logs/mcp/<server>.stderr.log`，每次启动都截断重写。服务器能起来但握手失败
时，看这个文件：

```bash
tail -f ~/.chaos/logs/mcp/filesystem.stderr.log
```

### 被组织策略拦截

如果原生 TOML 策略或 Claude `managed-settings.json` 设置了 `deniedMcpServers`、非空的
`allowedMcpServers`，或 `allowManagedMcpServersOnly`，Grok 会在合并阶段丢弃不匹配的
服务器，并记录 `MCP server blocked by managed settings policy`。原生 TOML 层会约束每一个
服务器；Claude 文件只约束由外部定义的服务器。`chaos inspect` 会显示这些名单、锁定范围
以及每个仍然保留的服务器。细节与示例见
[限制哪些 MCP 服务器可运行](09-plugins.md#限制可以运行哪些-mcp-服务器)。

### 查看服务器状态

用 `chaos inspect` 查看所有已加载的 MCP 服务器及其来源：

```bash
chaos inspect          # Human-readable
chaos inspect --json   # Machine-readable
```

### 调试日志

```bash
RUST_LOG=debug GROK_LOG_FILE=/tmp/grok.log chaos
tail -f /tmp/grok.log
```

看日志里含 `mcp` 的条目，可以追踪服务器启动、工具发现和工具调用执行。
