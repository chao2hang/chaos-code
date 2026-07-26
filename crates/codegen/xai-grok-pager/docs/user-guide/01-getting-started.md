# Getting Started

> **Chaos 分支：** 从源码构建的二进制名为 `chaos`（包 `xai-grok-pager-bin`）。
> 模型凭证由用户自带，无需 Grok 登录。详见仓库根 [CHAOS.md](../../../../CHAOS.md)。

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

1. 按 [Authentication](02-authentication.md) 或 [CHAOS.md](../../../../CHAOS.md)
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

Press `Tab` to move focus between the prompt and the scrollback. While a turn is running, `Ctrl+C` cancels it (or clears a non-empty draft first); `Esc` is a no-op mid-turn. Idle, press `Esc` twice within 800ms to clear a non-empty prompt, or (with an empty prompt and conversation messages) to open rewind — see [Keyboard Shortcuts](03-keyboard-shortcuts.md#escape). With the scrollback focused, use the arrow keys to select entries and to collapse or expand them. To navigate with `j`/`k` and fold with `h`/`l` instead, enable Vim mode.

### File References

Use `@` in your prompt to attach files:

```
@src/main.rs              # Attach a file
@src/main.rs:10-50        # Attach lines 10-50
@src/                     # Browse a directory
```

The `@` operator opens a fuzzy file picker. By default it respects `.gitignore` and hides dotfiles. Prefix with `!` to search hidden files:

```
@!.github                 # Search hidden files
@!.env                    # Attach a .env file
```

### Permissions

默认情况下，Chaos 在执行 shell 或编辑文件前会请求确认。可单次批准，或开启始终批准：

- 按 `Ctrl+O` 切换始终批准模式
- 启动时加 `--yolo`：`chaos --yolo`
- 在提示框输入 `/always-approve` 切换

---

## Key Concepts

### Sessions

Every conversation is a **session**. Sessions are automatically saved to `~/.grok/sessions/` and can be resumed later. Each session tracks the full conversation history, tool calls, file edits, and task state.

- Start a new session: `Ctrl+N` or `/new`
- Resume a previous session: `/resume` in the TUI, or `--resume <ID>` from the CLI
- Continue the most recent session: `grok -c`

### Scrollback

The scrollback is the main display area. It shows:

- **User prompts** -- your messages, rendered as sticky headers
- **Agent messages** -- Grok's responses with full markdown rendering and syntax highlighting
- **Thinking blocks** -- Grok's reasoning process (collapsible)
- **Tool calls** -- file edits (with inline diffs), command executions, search results, and more
- **Task lists** -- TODO items tracking progress

Collapse or expand the selected entry with the `Left`/`Right` arrow keys (or `h`/`l` and `e` in Vim mode). In Vim mode, press `y` to copy its content and `Y` to copy its metadata (for example, the command that ran). Press `Enter` to open it in the fullscreen viewer (in any mode).

### Tools

Grok has built-in tools for:

| Tool | Description |
|------|-------------|
| `read_file` / `search_replace` | Read and edit files with line-precise changes |
| `grep` | Regex search across your codebase (powered by ripgrep) |
| `list_dir` | List directory contents |
| `run_terminal_command` | Execute shell commands |
| `web_search` / `web_fetch` | Search the web and fetch URLs |
| `todo_write` | Create and manage task lists |
| `spawn_subagent` | Spawn parallel subagent sessions |
| `memory_search` | Search cross-session memory |

Tools can be extended with [MCP servers](05-configuration.md#mcp-servers) for integrations like GitHub, databases, and more.

### Slash Commands

Type `/` in the prompt to access commands. These provide quick actions without writing a full prompt:

```
/model grok-build                 # Switch model
/compact                          # Compress conversation history
/always-approve                   # Toggle always-approve mode
/new                              # Start a new session
```

See [Slash Commands](04-slash-commands.md) for the complete reference.

---

## Common Launch Options

```bash
# Launch the interactive TUI and submit an initial prompt as the first turn
grok "fix the failing auth test and run it"

# Initial prompt in a new git worktree. Use --worktree=<name> (with `=`) so the
# prompt isn't swallowed as the worktree name — `grok -w "refactor module X"`
# would treat "refactor module X" as the worktree label, not the prompt.
grok --worktree=feat "refactor module X"

# Base the worktree on a specific branch (e.g. main) instead of the current HEAD:
grok -w --ref main "implement feature from main"


# Start in a specific project directory
grok --cwd ~/projects/my-app

# Add project-specific rules
grok --rules "Always use TypeScript. Prefer functional components."

# Auto-approve all tool executions
grok --yolo

# Use a specific model
grok -m grok-build

# Resume a previous session
grok --resume <session-id>

# Continue the most recent session
grok -c

# Experimental scrollback-native render mode. Sticky: plain `grok` reopens in
# the mode last chosen via --minimal/--fullscreen (or /minimal//fullscreen).
grok --minimal

# Back to the standard fullscreen TUI (and make it sticky again)
grok --fullscreen

# Headless mode (for scripts)
grok -p "Explain this codebase"
```

---

## Headless Mode

Run Grok non-interactively for scripting, CI/CD, and automation:

```bash
grok -p "Your prompt here"
```

Output formats:

| Format | Flag | Description |
|--------|------|-------------|
| `plain` | (default) | Human-readable text |
| `json` | `--output-format json` | Single JSON object with `text`, `stopReason`, `sessionId`, and `requestId` |
| `streaming-json` | `--output-format streaming-json` | NDJSON event stream for real-time processing |

Example CI/CD usage:

```bash
grok -p "Review changes for bugs" --output-format json --yolo | jq -r '.text'
```

---

## Project Rules (AGENTS.md)

Add per-project instructions by creating an `AGENTS.md` file in your repository. Grok reads these files and injects their contents as a project-instructions message at the start of the conversation:

```
~/.grok/AGENTS.md           # Global rules (apply to all projects)
<repo-root>/AGENTS.md       # Repository-level rules
<cwd>/AGENTS.md             # Directory-level rules (highest priority)
```

Deeper files take precedence. Grok also reads `CLAUDE.md` files for compatibility.

---

## Where to Go Next

| Document | What You Will Learn |
|----------|-------------------|
| [Authentication](02-authentication.md) | Browser login, API keys, OIDC, external auth, device code flow |
| [Keyboard Shortcuts](03-keyboard-shortcuts.md) | Complete reference for all key bindings |
| [Slash Commands](04-slash-commands.md) | All available `/` commands |
| [Configuration](05-configuration.md) | config.toml, pager.toml, environment variables |
