# 把它变成你的

## 最省事的办法：直接开口

Chaos 了解自己的能力，也能自己配置自己。试试：

- *"给我们的 staging 数据库加上 Postgres MCP 服务器"*
- *"换成浅色主题"*
- *"给这个仓库写一份 AGENTS.md"*

如果你更想自己动手，下面每一样也都有对应命令。

## 教会 Chaos 你的项目：AGENTS.md

在仓库根目录放一个 `AGENTS.md`，写上构建命令、约定和各种坑。Chaos 每次会话都会自动读取它——这是投入产出比最高的一项定制：

```markdown
# My Project
- Run tests with `pnpm test`
- Never edit files under generated/
```

## 教会 Chaos 你的事实：记忆

用 `#` 开头（或使用 `/remember`）可以把一条笔记存给以后的会话：`# the staging deploy uses eu-west`。

## 外观、按键与扩展

- **`/theme`** —— 配色主题（或用 `auto` 跟随操作系统）；其余设置交给 **`/settings`**（或 `F2`）；如果你好这一口，还有 **`/vim-mode`**。
- **技能**（`/skills`）—— 可复用的提示包；用户可调用的技能会自动变成斜杠命令。
- **MCP 服务器**（`/mcps`）以及**插件与钩子**（`/plugins`、`/hooks`）。

先从 `AGENTS.md` 和一个主题起步；需要时再加其余部分。

*深入了解：`/docs Project Rules (AGENTS.md)`、`/docs Skills` 或 `/docs MCP Servers`*
