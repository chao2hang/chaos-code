# Chaos 用户指南

> **Chaos** 是本仓库 fork：无需 Grok / xAI 登录，用户自带模型凭证（BYOK）。
> 认证与模型配置以仓库根 [CHAOS.md](../../../../CHAOS.md) 与
> [Authentication](02-authentication.md) 为准。
> 部分进阶章节仍沿用上游 Grok Build 结构；路径请按双读规则把 `~/.grok` 理解为
> 「解析后的配置根」（也可能是 `~/.chaos`），`grok` 命令一律用 `chaos`。

了解如何安装、配置与扩展终端 AI 编码助手。

---

## Tier 1: 入门

从这里开始，覆盖第一天所需内容。

| # | 文档 | 说明 |
|---|------|------|
| 1 | [Getting Started](01-getting-started.md) | 构建、首次启动、BYOK、基本交互 |
| 2 | [Authentication](02-authentication.md) | BYOK：`model_providers`、API Key、`/provider`（无浏览器登录） |
| 3 | [Keyboard Shortcuts](03-keyboard-shortcuts.md) | TUI 快捷键与鼠标操作 |
| 4 | [Slash Commands](04-slash-commands.md) | 全部 `/` 命令 |
| 5 | [Configuration](05-configuration.md) | `config.toml`、`pager.toml`、环境变量与文件位置 |

---

## Tier 2: 核心功能

自定义与扩展 Chaos。

| # | 文档 | 说明 |
|---|------|------|
| 6 | [Theming and Appearance](06-theming.md) | 主题、`/theme`、`pager.toml` |
| 7 | [MCP Servers](07-mcp-servers.md) | 通过 MCP 接入外部工具 |
| 8 | [Skills](08-skills.md) | SKILL.md 可复用提示包 |
| 9 | [Plugins](09-plugins.md) | 打包 skills/commands/agents/hooks/MCP |
| 10 | [Hooks](10-hooks.md) | 工具前后生命周期脚本 |
| 11 | [Custom Models](11-custom-models.md) | BYOK、Ollama、OpenAI 兼容端点 |
| 12 | [Project Rules (AGENTS.md)](12-project-rules.md) | 目录级指令与优先级 |
| 13 | [Memory](13-memory.md) | 跨会话记忆 |

---

## Tier 3: 进阶

自动化、脚本与系统集成。

| # | 文档 | 说明 |
|---|------|------|
| 14 | [Headless Mode and Scripting](14-headless-mode.md) | `chaos -p`、输出格式、CI |
| 15 | [Agent Mode and IDE Integration](15-agent-mode.md) | ACP、WebSocket、SDK |
| 16 | [Subagents and Personas](16-subagents.md) | 子代理与能力模式 |
| 17 | [Session Management](17-sessions.md) | 会话保存/恢复/压缩 |
| 18 | [Sandbox Mode](18-sandbox.md) | 沙箱配置 |
| 19 | [Plan Mode](19-plan-mode.md) | 计划模式 |
| 20 | [Background Tasks and Monitoring](20-background-tasks.md) | 后台任务与 `monitor` |
| 21 | [Terminal Support and Troubleshooting](21-terminal-support.md) | tmux、SSH、剪贴板 |
| 22 | [Permissions and Safety Controls](22-permissions-and-safety.md) | 权限与安全 |
| 23 | [Agent Dashboard](23-dashboard.md) | 本地会话总览 |
| 24 | [Monitoring Usage (External OpenTelemetry)](24-monitoring-usage.md) | 外部 OTEL 导出 |
