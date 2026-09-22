# 技能

技能是可复用的提示词包，用任务专属的指令扩展 Chaos。你可以把可重复的流程沉淀一次，而不必每个会话重新解释一遍。启动时的发现过程会跳过不可信目录下的项目技能与命令。

---

## 什么是技能？

技能是一个包含 `SKILL.md` 文件的目录。它的 markdown 正文告诉 Chaos 如何处理某类任务：分步指令、约定，以及工具使用模式。

当某个可重复流程太具体、不适合写进 AGENTS.md，又太长、不值得每次重打一遍时，就为它建一个技能。Chaos 只在技能与当前任务相关时才会激活它。

---

## 技能位置

Chaos 按以下优先级顺序从这些目录发现技能（本分叉兼容双读 `.chaos/` 与 `.grok/`，同名时 Chaos 优先；下表以 `.chaos/` 写法列出）：

| 位置 | 作用域 | 优先级 | 说明 |
|----------|-------|----------|-------|
| `./.chaos/skills/`, `./.chaos/commands/` | 本地（CWD） | 最高 | 当前目录技能 / 旧式命令 markdown |
| `<repo_root>/.chaos/skills/`, `…/commands/` | 仓库 | 中 | 在整个仓库内共享 |
| `~/.chaos/skills/`, `~/.chaos/commands/` | 用户 | 最低 | 所有项目共用的个人技能 |
| `~/.claude/skills/`, `~/.claude/commands/` | 用户 | 最低 | Claude Code 兼容（可配置） |
| `./.claude/skills/`, `./.claude/commands/` | 本地 / 仓库 | 高 | 项目级 Claude 技能与旧式自定义斜杠命令 |
| `~/.cursor/skills/` | 用户 | 最低 | Cursor 兼容（可配置） |
| `./.cursor/skills/` | 本地 / 仓库 | 高 | 项目级 Cursor 技能（启用 Cursor 兼容技能时生效） |

Chaos 按名称对技能去重——优先级更高的位置会覆盖更低的。Chaos 还会在每一层（与 `.chaos/` 并列）扫描 `.agents/skills/`（以及 `commands/`），并遍历工作目录到仓库根之间的每个目录。

`commands/` 目录下的扁平 `*.md` 文件会变成用户可调用的斜杠命令（文件名主干即命令名），与 Claude Code 旧式自定义命令的布局一致。

技能与命令的发现过程**不**使用 `.gitignore`。已知技能根目录（`.chaos/`、`.agents/`、`.claude/`、`.cursor/`）下的路径只要在磁盘上存在就会加载——团队常把 `.claude/**` 当作仅限本地的配置忽略掉，却仍期望 `/frontend` 这类项目命令可用。要隐藏某个技能，请使用配置中的 `[skills] ignore`（而不是仓库的忽略规则）。

Chaos 默认会扫描 Claude 与 Cursor 的技能目录。要停止扫描某个厂商，请在 `~/.chaos/config.toml` 中把 `[compat.cursor]` 或 `[compat.claude]` 下的 `skills` 项设为 `false`，或将 `GROK_CURSOR_SKILLS_ENABLED` 或 `GROK_CLAUDE_SKILLS_ENABLED` 环境变量设为 `false`。详见[配置](05-configuration.md#厂商兼容性开关)。无论这些设置如何，Chaos 始终会过滤掉已知的厂商自带默认技能（例如 Cursor 的 `shell`、`canvas`、`statusline`）。

### 额外技能目录

通过 `~/.chaos/config.toml` 中的 `[skills]` 添加目录、排除路径或禁用单个技能：

```toml
[skills]
paths = ["~/my-team-skills"]          # Additional directories to scan
ignore = ["~/my-team-skills/wip"]     # Paths to exclude (hidden entirely)
disabled = ["wip-skill"]              # Skill names to keep listed but inactive
```

`paths` 中的每个条目是一个 `SKILL.md` 文件或一个会被 Chaos 递归遍历的目录。`ignore` 会把技能完全隐藏；`disabled` 保留它在列表中，但不纳入系统提示词、也不可调用。`paths` 与 `ignore` 接受文件系统路径并支持 `~` 展开；`disabled` 接受技能名。

`[paths] extra_skill_dirs` 由 `/import-claude` 写入。它不会注入技能。额外目录请放进 `[skills] paths`。

---

## 创建技能

### 目录结构

每个技能各自位于一个目录中，并带有一个 `SKILL.md` 文件：

```
~/.chaos/skills/
  commit/
    SKILL.md
  review-pr/
    SKILL.md
  deploy/
    SKILL.md
```

### SKILL.md 格式

技能文件由 YAML frontmatter 加 markdown 指令组成：

```markdown
---
name: commit
description: Create well-formatted git commits following conventional commit standards. Use when the user wants to commit changes or asks for /commit.
---

# Git Commit Skill

Review staged changes and create a commit with a clear, conventional message.

## Steps

1. Run `git diff --staged` to see changes
2. Summarize what changed and why
3. Create commit message following conventional commits format
4. Run `git commit -m "..."` with the message
```

### 核心 frontmatter 字段

| 字段 | 说明 |
|-------|-------------|
| `name` | 技能标识符。使用小写字母、数字和连字符，最长 64 个字符。Chaos 会把空格和下划线归一化为连字符。若省略 `name`，Chaos 使用技能的目录名。 |
| `description` | 技能做什么、何时使用。Chaos 依据它判断是否调用该技能。若省略，Chaos 使用正文的第一段。 |

写一条具体的 `description`。它决定了 Chaos 何时自动调用该技能。把触发短语和使用场景写进去。

### 可选 frontmatter 字段

多词 frontmatter 键使用 kebab-case（`model` 这类单词键按原样书写）。

| 字段 | 说明 |
|-------|-------------|
| `when-to-use` | 自动调用的触发短语，与 `description` 分开维护。 |
| `allowed-tools` | 技能使用的工具，写成 YAML 列表，或以逗号/空格分隔的字符串。 |
| `argument-hint` | 斜杠命令自动补全中显示的提示文本（例如 `commit message`）。 |
| `user-invocable` | 能否把该技能当斜杠命令运行。默认 `true`；设为 `false` 可把它从斜杠命令中隐藏。（若想阻止模型调用某技能，应改设 `disable-model-invocation`。） |
| `disable-model-invocation` | 为 `true` 时，只有你的斜杠命令能运行该技能——模型不能自动调用它。默认 `false`。 |
| `model` | 运行该技能时的模型覆盖。 |
| `effort` | 推理强度覆盖。 |
| `license` | 许可证标识符（例如 `Apache-2.0`）。 |
| `compatibility` | 环境要求（例如 `Requires git, docker, jq`）。 |
| `metadata` | 任意字符串键值对。Chaos 会提取 `metadata.author` 与 `metadata.short-description` 用于展示。 |

---

## 用 /create-skill 创建技能

`/create-skill` 命令以交互方式带你构建一个新技能。Chaos 会询问你的需求、起草文件，并写入磁盘。

### 工作方式

运行 `/create-skill` 时，Chaos 会：

1. **收集需求。** Chaos 询问技能名称、保存的作用域，以及你想沉淀的工作流描述。名称用小写字母、数字和连字符（2–64 个字符，以字母或数字开头结尾）。

2. **起草描述。** Chaos 写出一条 `description`，说明技能做什么、哪些短语触发它、斜杠命令名是什么。你可以批准或修改草稿后再继续。

3. **创建技能目录。** Chaos 创建 `<scope>/.chaos/skills/<name>/` 目录，并在技能需要时一并创建 `scripts/` 或 `references/` 子目录。

4. **写入 SKILL.md。** Chaos 写入 frontmatter（`name` 与 `description`）和 markdown 指令正文，以及任何配套文件。

5. **校验并确认。** Chaos 把文件读回来，确认写入无误，并告诉你如何运行该技能。

### 选择作用域

Chaos 会询问把技能保存到哪里：

- **项目**（`<repo_root>/.chaos/skills/<name>/`）——仅在本仓库可用，可通过版本控制分享给队友。在 git 仓库中，Chaos 推荐这个作用域。
- **用户**（`~/.chaos/skills/<name>/`）——在你所有项目中可用。

要把技能分发给整个团队或组织，可以把它打包进插件并通过插件市场发布。见[创建你自己的插件市场](09-plugins.md#创建你自己的插件市场)与[跨组织分发](09-plugins.md#跨组织分发)。

新技能几秒内就会出现在斜杠菜单中，因为磁盘上的文件变化时 Chaos 会重新加载技能。

---

## 使用技能

### 按名称运行技能

每个技能都是一个以技能名命名的斜杠命令。输入名称即可运行：

```
/commit              # Runs the "commit" skill
/review-pr           # Runs the "review-pr" skill
```

运行技能会把它的指令加载进对话，并引导模型照着执行。要传参数，在名称后面输入：

```
/commit fix the build
```

要浏览你的技能，输入 `/` 打开斜杠命令菜单。Chaos 会列出所有内置命令和技能，并随你的输入过滤。如果想从命令行列出技能，运行 `chaos inspect`（见[查看技能详情](#查看技能详情)）。

### 限定名

当技能名与另一个技能或内置命令冲突时，Chaos 会保持**两者**都可调用。内置命令保留裸名（`/login`、`/compact`、…）。技能则以带作用域前缀的限定名提供——`local:`、`repo:`、`user:` 或插件名：

```
/local:commit        # The "commit" skill from ./.chaos/skills/
/user:commit         # The "commit" skill from ~/.chaos/skills/
/acme:login          # A plugin skill named "login" (built-in /login is unchanged)
```

在斜杠菜单中输入 `/login` 会同时显示两行，并带有右对齐的 **built-in** 或 **skill · plugin-name** 徽标，便于区分。如果你想让技能用裸 `/name`，就重命名该技能（或其目录）。

`chaos inspect` 会给冲突的技能打上 `[collides with /login → /acme:login]` 标记。

### 自动调用

当 Chaos 识别出相关任务时，可以自行调用技能。Chaos 会把你的提示词与技能的 `description` 和 `when-to-use` 字段做匹配，所以要把这两个字段都写成能描述触发场景的样子。

例如，某技能的描述写的是「Use when the user wants to commit changes」，那么你说 "commit my changes" 就可能自动触发它。若要求必须显式使用斜杠命令、禁止自动调用，在 frontmatter 中设置 `disable-model-invocation: true`。

---

## 查看技能详情

运行 `chaos inspect` 可以查看 Chaos 发现的每个技能，以及其余配置：

```bash
chaos inspect          # Human-readable summary
chaos inspect --json   # Machine-readable report
```

在人类可读的输出中，Skills 一节列出每个技能的名称及其来源——`project`、`user`、`bundled`、`config`（某个 `[skills].paths` 条目）、`server`（托管工作区中从技能商店同步的技能），或 `plugin: <name>`。凡是通过 `[skills].disabled` 或来自被禁用厂商入口而禁用的技能，Chaos 都会打上 `[disabled]` 标记。

这份报告遵循你的 `[skills]` 配置，与实际会话一致：来自 `paths` 的技能会被列出，位于 `ignore` 前缀下的技能被隐藏，`disabled` 中点名的技能保留在列表中但标记为 `[disabled]`。

`--json` 报告包含每个技能的完整细节：`name`、`description`、`source`（含 SKILL.md 文件路径）和 `userInvocable` 标志。裸斜杠名有争议的技能——被内置命令或其他技能占用——还会包含 `collidesWith`（有争议的名称）和 `invocableAs`（应输入的限定命令）。

---

## 内置与插件技能

Chaos 把平台技能与你的个人技能分开分发。内置技能缓存于 `~/.chaos/bundled/skills/`；Chaos 从不把它们写进 `~/.chaos/skills/`。同名的本地、仓库或用户技能会覆盖内置副本。`chaos inspect` 按实际来源标注每个定义。（同名插件技能不会覆盖原生技能；它仍以带限定的 `plugin:name` 形式可用。）

技能也可以来自插件。安装包含技能的插件后，它们会与你的用户技能和项目技能并列出现。`chaos inspect` 会把每个插件提供的技能来源标注为 `plugin: <name>`。

详见[插件指南](09-plugins.md)，了解如何安装提供技能的插件。

---

## 最佳实践

1. **写具体的描述。** 描述驱动自动调用。"Create git commits" 太含糊；"Create well-formatted git commits following conventional commit standards. Use when the user wants to commit changes or asks for /commit." 的效果更好。

2. **写明具体步骤。** 技能给 Chaos 一套清晰、有序的执行流程时效果最好。

3. **按名称引用工具。** 当技能依赖特定工具（如 `run_terminal_cmd` 或 `search_replace`）时，点名它们，让模型知道该用什么。

4. **保持技能聚焦。** 每个工作流写一个技能。一个 "deploy" 技能加一个 "rollback" 技能，好过一个 "deploy-and-rollback" 技能。

5. **把项目技能纳入版本控制。** 把 `.chaos/skills/` 提交到仓库，让整个团队受益。`~/.chaos/skills/` 里的用户技能保持个人私有、不共享。

6. **跑一遍验证。** 在依赖自动调用之前，先调用 `/name` 确认技能可用。

7. **正文控制在文件读取上限之内。** Chaos 至多内联技能正文的前 25,000 个 token（与 `read_file` 的上限相同）。把较长的参考资料放进同级文件，并让 Chaos 用行偏移和条数上限去读。
