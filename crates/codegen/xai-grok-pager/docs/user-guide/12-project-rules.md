# 项目规则 (AGENTS.md)

项目规则让你按项目或按目录来配置 Chaos。通过在仓库中放置一个 AGENTS.md 文件，你可以设定编码约定、构建说明、风格指南，以及 Chaos 在该代码库中工作时应遵循的任何其他指令。启动时加载需要文件夹信任（`--trust` 或交互式授权）。

---

## 什么是项目规则？

项目规则是 Chaos 读取并添加到其上下文中的 Markdown 文件。Chaos 在该目录树中的每一次交互都会遵循其内容。

这是让 Chaos 了解你的项目约定（conventions）的主要机制，因此你无需在每个会话中重复说明这些约定。

---

## 支持的文件名

Chaos 会在每个目录中检查这些文件名（按此顺序）：

- `Agents.md`
- `Claude.md`
- `CLAUDE.md`
- `CLAUDE.local.md`
- `AGENT.md`
- `AGENTS.md`

Chaos 会加载目录中每个匹配的文件，因此同时包含 `AGENTS.md` 和 `CLAUDE.md` 的文件夹会让两者同时生效。在不区分大小写的文件系统上，解析为同一文件的名称（例如 `Agents.md` 和 `AGENTS.md`）会被去重并只计算一次。支持 `Claude.md`、`CLAUDE.md` 和 `CLAUDE.local.md` 是为了兼容 Claude Code 工作流。当启用 Claude 兼容性（默认设置）时，Chaos 还会扫描你的主目录级 `~/.claude/` 目录以查找这些文件名，并在每个目录层级检查 `.claude/CLAUDE.md` 和 `.claude/CLAUDE.local.md` -- 即 Claude Code 用于项目记忆的位置。启用 Cursor 兼容性后，主目录级的 `~/.cursor/` 目录也会以相同的方式被扫描。

### 规则目录

除了 AGENTS.md 文件之外，Chaos 还会在从仓库根目录到当前工作目录的每一级（`<dir>`）的规则目录中扫描 `*.md` 文件：

| 位置 | 说明 |
|----------|-------|
| `<dir>/.chaos/rules/` | 始终扫描 |
| `<dir>/.claude/rules/` | Claude 兼容（可配置） |
| `<dir>/.cursor/rules/` | Cursor 兼容（可配置） |

无论从何处启动，Chaos 还会扫描主目录级规则。这些根目录本身已按厂商区分，因此规则直接位于 `rules/` 之下：

| 位置 | 说明 |
|----------|-------|
| `$CHAOS_HOME/rules/`（兼容旧名 $GROK_HOME；默认 `~/.chaos/rules/`） | 始终扫描；适用于所有项目 |
| `~/.claude/rules/` | 由 `compat.claude.rules` 控制 |
| `~/.cursor/rules/` | 由 `compat.cursor.rules` 控制 |
| `[paths] extra_rule_dirs` 的每一项 | 你在 `config.toml` 中列出的任意绝对目录；`~` 会展开 |

主目录规则最先加载，顺序即表中的顺序，随后是从仓库根目录到当前目录的项目文件。每个规则目录内的文件按字母顺序排列。要从非内置位置加载规则，请在 `[paths]` 下列出该目录：

```toml
[paths]
extra_rule_dirs = ["~/team-rules", "/opt/company/grok-rules"]
```

所列目录中直接包含的每个 `*.md` 都会作为规则加载（不扫描子目录），适用于每个项目，且不受文件夹信任、仓库的 `.gitignore` 或兼容性配置项的影响；模型将其接收为用户规则，`chaos inspect` 会把它们列为 `global`。条目必须是绝对路径或以 `~/` 开头；相对路径或缺失的条目不会加载任何内容。`/import-claude` 会把你现有的 `~/.claude/rules/` 写到这里，使其在关闭 Claude 兼容性扫描后仍能继续加载。各厂商的 `rules` 配置项同时控制主目录规则与项目规则，并与相应的 `agents` 配置项相互独立。Claude 的 `agents` 配置项控制 `~/.claude/` 下的命名文件以及项目中的 `<dir>/.claude/CLAUDE*.md`；`Claude.md`、`CLAUDE.md` 和 `CLAUDE.local.md` 这类通用顶级文件名仍会被识别。参见 [配置](05-configuration.md#厂商兼容性开关)。

---

## 发现是如何工作的

Chaos 按以下顺序扫描项目规则：

1. **主目录规则**：`$CHAOS_HOME`，然后是已启用的 `~/.claude/` 与 `~/.cursor/` 来源，然后是 `[paths] extra_rule_dirs`
2. **仓库规则**：若在 git 仓库内，则是从仓库根目录到当前工作目录（含两端）的每个目录
3. **仅当前目录**：若不在 git 仓库内，则只扫描当前工作目录

### 示例

给定如下项目结构：

```
~/projects/my-app/
  AGENTS.md              # "Use TypeScript. Follow ESLint rules."
  src/
    AGENTS.md            # "Prefer functional components."
    components/
      AGENTS.md          # "Use CSS modules for styling."
```

当 Chaos 运行于 `~/projects/my-app/src/components/` 时，它会加载全部三个文件。指令会累积，因此 Chaos 能看到所有这些指令。

### 更深的文件优先

Chaos 把文件按从仓库根目录到当前工作目录的顺序排列，因此更深目录中的文件在其上下文中出现得更晚，并在指令冲突时优先。在上面的例子中，如果根目录写的是 "Use styled-components"，而 `components/AGENTS.md` 写的是 "Use CSS modules"，那么 CSS modules 指令胜出，因为它出现得更晚。

### 自动加载行为

- Chaos 在会话开始时自动加载从仓库根目录到当前工作目录的文件。
- 当 Chaos 读取、列出或编辑初始集合之外目录中的文件时，它会检测那里是否存在项目指令文件，记录其路径，并在它们与任务相关时读取它们。

---

## 项目规则里应写什么

### 编码约定

```markdown
# Coding Standards

- Use TypeScript for all new code
- Prefer functional components with hooks over class components
- Use `const` by default; only use `let` when reassignment is needed
- Maximum line length: 100 characters
```

### 构建与测试指令

```markdown
# Build & Test

- Run `npm test` before committing
- Use `npm run lint` to check code style
- Build with `npm run build` -- ensure no TypeScript errors
- Integration tests: `npm run test:e2e` (requires Docker)
```

### 风格指南

```markdown
# Style Guide

- Follow the Airbnb JavaScript Style Guide
- Use 2-space indentation
- Always use trailing commas in multi-line arrays/objects
- Prefer template literals over string concatenation
```

### PR 与提交要求

```markdown
# Version Control

- Write commit messages in conventional commits format
- Prefix branch names with `feature/`, `fix/`, or `chore/`
- All PRs require at least one approval before merge
- Squash-merge feature branches
```

### 架构说明

```markdown
# Architecture

- API routes go in `src/routes/` with one file per resource
- Business logic goes in `src/services/`
- Database queries go in `src/repositories/`
- Never import from `src/routes/` in `src/services/`
```

---

## 将规则限定到子目录

AGENTS.md 文件的作用域是以其所在文件夹为根的整棵目录树。利用这一点，可以为代码库的不同部分提供不同的指令：

```
my-monorepo/
  AGENTS.md                    # Monorepo-wide rules
  packages/
    frontend/
      AGENTS.md                # "Use React. Prefer CSS modules."
    backend/
      AGENTS.md                # "Use Express. Follow REST conventions."
    shared/
      AGENTS.md                # "No framework-specific code in this package."
```

---

## 会话规则标志

要为单个会话添加规则而不编辑文件，可传入 `--rules`（别名 `--append-system-prompt`）：

```bash
chaos --rules "Always use TypeScript. Prefer functional components."
```

Chaos 会把这段文本追加到该会话的系统提示。可用它做会话级的定制。

要完全替换系统提示，可传入 `--system-prompt-override`（别名 `--system-prompt`）。Chaos 会逐字使用该文本，并跳过默认系统提示和 `--rules`。（相比之下，通过 `--rules` 传入的文本会被包裹在 `<human_rules>` 块中并追加到默认提示。）

---

## 文件大小

Chaos 会完整加载每个项目指令文件；没有字符上限，也没有截断。即便如此，指令仍应保持简洁聚焦。较短而具体的规则比冗长的规则更容易被 Chaos 遵循，而且你加载的每个文件都会消耗上下文。

---

## Gitignore 过滤

被 `.gitignore` 忽略的文件在发现阶段会被跳过。为了让个人覆盖不进入共享仓库，可以把一个会被识别的文件名（如 `CLAUDE.local.md`）加入 gitignore：

```gitignore
# .gitignore
CLAUDE.local.md
```

作为顶级指令文件，Chaos 只会发现 [支持的文件名](#支持的文件名) 中列出的被识别文件名——而不是自定义名称（例如 `AGENTS.local.md` 或 `notes.md`）。（而在 `.chaos/rules/` 这类规则目录内，每个 `*.md` 文件无论叫什么名字都会被加载。）

---

## .chaos/ 项目目录

除了 AGENTS.md 文件之外，项目根目录中的 `.chaos/` 目录还可以包含额外的项目级配置（本分叉兼容读取 .chaos/ 与 .grok/ 两个项目目录，同名时 Chaos 优先）：

| 路径 | 用途 |
|------|---------|
| `.chaos/config.toml` | 项目级 MCP 服务器、插件和权限规则（其余设置仅从 `~/.chaos/config.toml` 加载） |
| `.chaos/skills/` | 项目级技能定义 |
| `.chaos/plugins/` | 项目级插件 |
| `.chaos/agents/` | 项目级代理定义 |
| `.chaos/hooks/` | 项目级生命周期钩子 |
| `.chaos/lsp.json` | LSP 服务器配置 |

这些都是可选的。各项的详细信息参见相应指南。

---

## 查看已加载的规则

使用 `chaos inspect` 查看所有已加载的项目指令：

```bash
chaos inspect
```

它会显示找到的每个项目指令文件，包括其路径和近似的 token 数。可用它确认 Chaos 已拾取你的规则。

---

## 最佳实践

1. **从根目录开始。** 把最重要的、全项目范围的规则放进仓库根目录的 AGENTS.md。

2. **要具体。** "Use TypeScript" 优于 "Use modern JavaScript"。「提交前运行 `cargo fmt`」优于「格式化你的代码」。

3. **保持简短。** 简洁的指令比冗长的指令更容易被遵循。

4. **大型仓库使用子目录限定。** Monorepo 的不同部分可能有不同的约定。用按目录划分的 AGENTS.md 来恰当地限定规则。

5. **把规则纳入版本控制。** 把 AGENTS.md 提交到仓库，让整个团队受益。用户个人的覆盖应放在 `~/.chaos/`（全局规则）。

6. **不要重复文档。** AGENTS.md 应包含可执行的指令，而不是项目 README 的副本。如有需要，可链接到外部文档。

7. **定期回顾。** 随着项目演进，更新你的规则以匹配当前的约定。
