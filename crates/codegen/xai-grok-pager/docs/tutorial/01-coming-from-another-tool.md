# 从 Claude、Cursor 或 Codex 迁移过来？

别担心 —— 你的设置、规则和技能都会跟着过来。Chaos 读取其他代理所用的同一套项目约定，并导入其余内容。

## 自动读取

- **规则与指令** —— `AGENTS.md`（Codex/OpenCode 约定）、`CLAUDE.md`（含嵌套的），以及 `.claude/rules/` 和 `.cursor/rules/` 下的 `*.md` 规则。
- **技能与自定义命令** —— `~/.claude/skills/`、`~/.claude/commands/`、`~/.cursor/skills/`，以及它们对应的项目级副本。扁平的 `.md` 命令文件在这里也会变成斜杠命令。
- **MCP 服务器** —— 来自 `~/.claude.json`、`.cursor/mcp.json` 和项目里的 `.mcp.json`。
- **钩子** —— 来自 `.claude/settings.json`，包括 `Bash` 这类匹配别名，因此大多数钩子无需改动即可运行。

## 一步导入

**`/import-claude`** 扫描你的 `~/.claude` 设置 —— 权限、环境变量、MCP 服务器、钩子 —— 并显示一份带复选框的预览；确认后会把所选条目写入你的 `.chaos` 配置。随时可以重跑。

## 接着上次继续

**`/resume-claude`**、**`/resume-codex`** 和 **`/resume-cursor`** 技能可以就在这里继续这些工具最近的会话。

## 查看发现了什么

在仓库里运行 **`chaos inspect`**，可以看到 Chaos 读取到的每个规则文件、技能和 MCP 服务器，并标注它们来自哪里。每个兼容来源都可以在 `[compat.claude]` / `[compat.cursor]` 配置节里开关。

还有一些你可能在别处错过的东西：`/btw` 可以在不打断当前任务的情况下问一个旁支问题，`/rewind` 恢复的是真实文件快照，而不只是聊天记录。

*深入了解：`/docs Project Rules (AGENTS.md)`、`/docs Skills` 或 `/docs MCP Servers`*
