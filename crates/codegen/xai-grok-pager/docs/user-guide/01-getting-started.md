# 快速上手

> **Chaos 分支：** 从源码构建的二进制名为 `chaos`（包 `xai-grok-pager-bin`）。
> 模型凭证由用户自带，无需 Grok 登录。详见仓库根 [CHAOS.md](../../../../../CHAOS.md)。

Chaos 是终端 AI 编码助手。它以全屏 TUI 理解代码库、执行 shell、编辑文件、搜索网页并管理任务；也可无头运行（脚本/CI）或通过 ACP 嵌入编辑器。

---

## 从源码构建

环境要求：Rust（见 `rust-toolchain.toml`）、DotSlash、`protoc`。在仓库根目录：

```bash
cargo build -p xai-grok-pager-bin --release
./target/release/chaos --version
```

开发模式：

```bash
cargo run -p xai-grok-pager-bin
```

（上游官方安装脚本安装的是 `grok`，与本 fork 无关。）

---

## 首次启动

1. 按 [认证](02-authentication.md) 或 [CHAOS.md](../../../../../CHAOS.md)
   在配置根（`~/.chaos` 或兼容的 `~/.grok`）的 `config.toml` 中配置
   `model_providers` 与 `model`。
2. 导出密钥环境变量（例如 `OPENAI_API_KEY` 或 `ANTHROPIC_API_KEY`）。
3. 启动：

```bash
./target/release/chaos
```

Chaos **不会**打开浏览器登录 grok.com。缺少凭证时，欢迎页会提示配置 Provider（`/provider` 或按 `p`）。

---

## 基本交互

启动后 TUI 主要区域：

- **回滚区（Scrollback）** — 对话历史：提示、回复、工具调用、文件编辑等。
- **提示框（Prompt）** — 底部输入区。

输入消息后按 `Enter` 发送。助手会按需读文件、跑命令、改代码；工具输出实时进入回滚区。

按 `Tab` 可在提示框与回滚区之间切换焦点。回合运行期间，输入框为空时 `Ctrl+C` 会取消该回合；若输入框里还有草稿，第一次按下只会清空草稿。`Esc` 从不取消回合——回合进行中按下它只会提示你改用 `Ctrl+C`。空闲时，800ms 内连按两次 `Esc` 可清空非空的提示框；若提示框为空且已有对话消息，则会打开回滚——见[键盘快捷键](03-keyboard-shortcuts.md#escape)。回滚区获得焦点后，可用方向键选中条目并折叠或展开它们。若想改用 `j`/`k` 导航、`h`/`l` 折叠，请启用 Vim 模式。

### 文件引用

在提示框中用 `@` 附加文件：

```
@src/main.rs              # Attach a file
@src/main.rs:10-50        # Attach lines 10-50
@src/                     # Browse a directory
```

`@` 会打开模糊文件选择器。默认遵循 `.gitignore` 并隐藏点文件；加上前缀 `!` 可搜索隐藏文件：

```
@!.github                 # Search hidden files
@!.env                    # Attach a .env file
```

### 权限

默认情况下，Chaos 在执行 shell 或编辑文件前会请求确认。可单次批准，或开启始终批准：

- 按 `Ctrl+O` 切换始终批准模式
- 启动时加 `--yolo`：`chaos --yolo`
- 在提示框输入 `/always-approve` 切换

---

## 核心概念

### 会话

每段对话都是一个**会话（session）**。会话会自动保存到 `~/.chaos/sessions/`，之后可以恢复。每个会话都会记录完整的对话历史、工具调用、文件编辑和任务状态。

- 新建会话：`Ctrl+N` 或 `/new`
- 恢复已有会话：TUI 里用 `/resume`，或从命令行用 `--resume <ID>`
- 继续最近的会话：`chaos -c`

### 回滚区

回滚区是主显示区域，其中显示：

- **用户提示** —— 你的消息，以吸顶标题的形式呈现
- **代理消息** —— Chaos 的回复，带完整的 Markdown 渲染与语法高亮
- **思考块** —— Chaos 的推理过程（可折叠）
- **工具调用** —— 文件编辑（含内联 diff）、命令执行、搜索结果等
- **任务列表** —— 跟踪进度的 TODO 项

用 `Left`/`Right` 方向键折叠或展开选中的条目（Vim 模式下则是 `h`/`l` 与 `e`）。Vim 模式下，按 `y` 复制其内容，按 `Y` 复制其元数据（例如所执行的命令）。按 `Enter` 可在全屏查看器中打开该条目（任何模式下都可用）。

### 工具

Chaos 内置以下工具：

| 工具 | 说明 |
|------|-------------|
| `read_file` / `search_replace` | 读取并编辑文件，改动精确到行 |
| `grep` | 在整个代码库中做正则搜索（由 ripgrep 驱动） |
| `list_dir` | 列出目录内容 |
| `run_terminal_command` | 执行 shell 命令 |
| `web_search` / `web_fetch` | 搜索网页并抓取 URL |
| `todo_write` | 创建和管理任务列表 |
| `spawn_subagent` | 派生并行的子代理会话 |
| `memory_search` | 搜索跨会话记忆 |

工具可以通过 [MCP 服务器](05-configuration.md#mcp-servers)扩展，以接入 GitHub、数据库等集成。

### 斜杠命令

在提示框中输入 `/` 即可访问命令。它们提供快捷操作，无需写完整提示：

```
/model grok-4.6                 # Switch model
/compact                          # Compress conversation history
/always-approve                   # Toggle always-approve mode
/new                              # Start a new session
```

完整清单见[斜杠命令](04-slash-commands.md)。

---

## 常用启动选项

```bash
# Launch the interactive TUI and submit an initial prompt as the first turn
chaos "fix the failing auth test and run it"

# Initial prompt in a new git worktree. Use --worktree=<name> (with `=`) so the
# prompt isn't swallowed as the worktree name — `chaos -w "refactor module X"`
# would treat "refactor module X" as the worktree label, not the prompt.
chaos --worktree=feat "refactor module X"

# Base the worktree on a specific branch (e.g. main) instead of the current HEAD:
chaos -w --ref main "implement feature from main"


# Start in a specific project directory
chaos --cwd ~/projects/my-app

# Add project-specific rules
chaos --rules "Always use TypeScript. Prefer functional components."

# Auto-approve all tool executions
chaos --yolo

# Use a specific model
chaos -m grok-4.6

# Resume a previous session
chaos --resume <session-id>

# Continue the most recent session
chaos -c

# Experimental scrollback-native render mode. Sticky: plain `chaos` reopens in
# the mode last chosen via --minimal/--fullscreen (or /minimal//fullscreen).
chaos --minimal

# Back to the standard fullscreen TUI (and make it sticky again)
chaos --fullscreen

# Headless mode (for scripts)
chaos -p "Explain this codebase"
```

---

## 无头模式

以非交互方式运行 Chaos，用于脚本、CI/CD 和自动化：

```bash
chaos -p "Your prompt here"
```

输出格式：

| 格式 | 标志 | 说明 |
|--------|------|-------------|
| `plain` | （默认） | 人类可读文本 |
| `json` | `--output-format json` | 单个 JSON 对象，包含 `text`、`stopReason`、`sessionId` 和 `requestId` |
| `streaming-json` | `--output-format streaming-json` | 用于实时处理的 NDJSON 事件流 |

CI/CD 用法示例：

```bash
chaos -p "Review changes for bugs" --output-format json --yolo | jq -r '.text'
```

---

## 项目规则（AGENTS.md）

在仓库中放置 `AGENTS.md` 文件即可添加按项目生效的指令。Chaos 会读取这些文件，并在对话开始时把其中的内容作为一条项目指令消息注入：

```
~/.chaos/AGENTS.md          # Global rules (apply to all projects)
<repo-root>/AGENTS.md       # Repository-level rules
<cwd>/AGENTS.md             # Directory-level rules (highest priority)
```

目录越深，文件优先级越高。为兼容起见，Chaos 也会读取 `CLAUDE.md` 文件。

---

## 接下来读什么

| 文档 | 你能学到什么 |
|----------|-------------------|
| [认证](02-authentication.md) | 模型凭证的配置方式、`/provider` 面板，以及本分叉与上游的差异 |
| [键盘快捷键](03-keyboard-shortcuts.md) | 所有键位的完整参考 |
| [斜杠命令](04-slash-commands.md) | 全部可用的 `/` 命令 |
| [配置](05-configuration.md) | config.toml、pager.toml、环境变量 |
