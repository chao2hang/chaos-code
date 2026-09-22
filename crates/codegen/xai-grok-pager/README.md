# xai-grok-pager

Chaos 的终端 UI（TUI）。提供交互式全屏界面，包括滚动回看视图、提示词输入、
会话管理和全部模态对话框。

## 架构

```
src/
├── app/                 # Application state and event handling
│   ├── app_view.rs      # Top-level state (welcome screen, agents, config)
│   ├── agent_view/      # Per-session agent view (struct in mod.rs + per-domain impl modules)
│   ├── dispatch/        # Action → Effect dispatcher (router + per-domain modules)
│   ├── effects.rs       # Async side effects (ACP calls, file I/O)
│   └── event_loop.rs    # Main event loop (input, ticks, ACP messages)
├── views/               # UI components
│   ├── prompt_widget.rs # Text editor with file search, slash, history
│   ├── welcome/         # Welcome screen (logo, menu, prompt)
│   ├── extensions_modal.rs   # Extensions modal (hooks, plugins, marketplace, skills, MCP servers)
│   ├── file_search/     # @-completion dropdown and line viewer
│   ├── slash_dropdown.rs# /command completion dropdown
│   └── ...              # Scrollback, status bar, panes, etc.
├── scrollback/          # Message history rendering
├── slash/               # Slash command registry and built-in commands
├── appearance/          # Theme and pager.toml config
├── acp/                 # Agent Communication Protocol client state
└── render/              # Low-level rendering helpers (color, wrapping, etc.)
```

## 核心概念

- **AppView** —— 拥有欢迎界面、Agent 会话和全局配置
- **AgentView** —— 每个会话一个；拥有提示词、滚动回看、工具面板和模态框
- **PromptWidget** —— 文本编辑器组件，支持文件搜索（`@`）、斜杠命令（`/`）、历史搜索和粘贴元素
- **Action/Effect** —— Elm 风格架构：输入 → Action → dispatch → Effect → 状态更新

## 快捷键

| Key | Context | Action |
|-----|---------|--------|
| `Ctrl+P` 或 `?` | Agent 界面 | 打开命令面板 |
| `Ctrl+L` | 任意界面（非 VS Code 系） | 打开插件/钩子模态框；在 VS Code / Cursor / Windsurf / Zed 上请改用 `/plugins` 或 `/hooks`（`Ctrl+L` 是回合中途插话） |
| `Tab` | 提示词输入框 | 切换到滚动回看 |
| `Esc` | 回合运行中 | 取消——在极简模式下，或 vim 滚动回看模式关闭时（默认）。全屏 vim 模式：无操作（请用 `Ctrl+C`） |
| `Esc` `Esc` | 空闲、提示词非空 | 清空提示词（800ms 内；第一次按下会显示提示） |
| `Esc` `Esc` | 空闲、提示词为空且有消息 | 打开回退选择器（第一次按下静默） |
| `Ctrl+M` | 提示词输入框 | 切换多行模式 |
| `Shift+Enter` | 提示词输入框 | 插入换行 |
| `/` | 提示词输入框 | 开始输入斜杠命令 |
| `@` | 提示词输入框 | 开始文件搜索 |
| `!` | 提示词输入框（空） | 进入 bash 模式 |
| `Ctrl+C` | 提示词输入框（有文本） | 清空提示词（即使回合正在运行） |
| `Ctrl+C` | 提示词输入框（空）+ 回合运行中 | 取消正在运行的回合 |
| `Ctrl+B` | Agent 界面 + 前台命令运行中 | 把该命令送到后台 |
| `Ctrl+G` | Agent 界面（完整 TUI） | 切换任务面板 |
| `Ctrl+G` | 普通输入框（极简模式） | 在外部编辑草稿；如果该组合键已被占用，请用命令面板入口 |

## 文档

- [终端支持与故障排查](docs/user-guide/21-terminal-support.md) —— tmux/SSH 真彩色、剪贴板、鼠标、诊断、`/doctor`
- [钩子与插件指南](docs/hooks-and-plugins.md) —— 管理钩子、插件和插件市场源
- [自定义钩子指南](docs/custom-hooks.md) —— 创建、配置并编写你自己的钩子
- [钩子示例](../xai-grok-hooks/examples/README.md) —— 常见工作流的示例钩子
- [钩子 crate（`xai-grok-hooks`）](../xai-grok-hooks/) —— 钩子运行时、事件类型和执行引擎
- [插件市场 crate（`xai-grok-plugin-marketplace`）](../xai-grok-plugin-marketplace/) —— 插件市场源的加载、扫描与安装
