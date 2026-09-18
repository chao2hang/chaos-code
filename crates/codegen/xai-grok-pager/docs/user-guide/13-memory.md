# 记忆

记忆让 Chaos 能回想起此前会话中的事实、决策与模式。Chaos 会为你保存的信息建立索引并自动检索，因此新会话可以复用相关的上下文。

---

## 什么是记忆？

没有记忆时，每个 Chaos 会话都从零开始：模型对之前的会话一无所知。启用记忆后，Chaos 可以：

- 回想起你此前说明过的项目约定。
- 复用验证有效的调试步骤。
- 让架构决策跨会话延续。
- 不再重复询问已经知道答案的问题。

记忆是实验性功能，默认关闭。

### 记忆的组织方式

记忆分两个作用域。全局记忆存放适用于你所有项目的事实；工作区记忆存放某个仓库的事实。同一仓库的克隆与工作树共用一个工作区作用域。

每个作用域的知识都保存在一份普通的 Markdown 文件 `MEMORY.md` 里：全局那份在 `~/.chaos/memory/MEMORY.md`，工作区那份在 `~/.chaos/memory/<project-slug>-<hash8>/MEMORY.md`。会话结束时写下的日志落在同一工作区目录的 `sessions/` 下，之后由整理流程（`/dream`）把会话日志折叠进工作区的那份 `MEMORY.md`。

记忆功能的埋点只包含固定的枚举名、布尔值、计数、时长和分数，绝不包含提示词、陈述、主题名、关键词、路径、模型输出或自由格式的错误信息。

---

## 启用记忆

### 配置文件（持久）

在 `~/.chaos/config.toml`（或兼容的 `~/.grok/config.toml`）里启用：

```toml
# ~/.chaos/config.toml
[memory]
enabled = true
```

### 环境变量

```bash
export GROK_MEMORY=1
chaos
```

### 强制关闭

即使 TOML 或远端设置已经启用，也可以只为当前进程关掉记忆：

```bash
export GROK_MEMORY=0
```

### 会话中切换

不必重启就能在会话里开关记忆：打开 `/memory`，按 `t`。

这个开关只作用于当前会话 —— 它不会写回 `config.toml`，而且两个方向都有效：以 `[memory] enabled = true` 开始的会话可以关掉记忆，以 `[memory] enabled = false` 开始的会话也可以打开。新会话重新遵循 `config.toml`。关闭会撤掉记忆工具的访问入口和系统提示里的记忆说明，但磁盘上已有的文件保留。打开会重新初始化记忆存储、注册记忆工具、恢复系统提示里的记忆说明；打开时会等待进行中的回合结束。

这个开关无法覆盖进程级的强制关闭（`--no-memory` 或 `GROK_MEMORY=0`）；那两者会让 `/memory` 在整个会话里都不可见。

### 优先级顺序

1. 进程级强制关闭（`--no-memory` 兼容标志或
   `GROK_MEMORY=0`）会关掉记忆。
2. 生效 TOML 里显式的 `[memory] enabled = false` 会关掉记忆，
   包括由远端托管设置启用的部分。`/memory` 的 `t`
   开关仍可为当前会话把它打开。
3. 否则，记忆由 `GROK_MEMORY=1`、`[memory] enabled = true`
   或远端托管设置启用。

---

## 记忆的存储方式

记忆以 Markdown 文件的形式存放在 `~/.chaos/memory/` 下：

| 位置 | 作用域 | 说明 |
|----------|-------|-------------|
| `~/.chaos/memory/MEMORY.md` | 全局 | 适用于你所有项目的事实 |
| `~/.chaos/memory/<project-slug>-<hash8>/MEMORY.md` | 工作区 | 项目专属的约定与上下文 |
| `~/.chaos/memory/<project-slug>-<hash8>/sessions/` | 会话 | 每个会话的摘要与日志 |

Chaos 会给每个工作区目录附上仓库身份的一小段哈希。当目录是带 `origin` 远端的 Git 仓库时，身份取 `origin` 远端归一化后的 `org/repo` 形式，否则取目录路径。由于同一仓库的克隆与工作树共用同一个 `origin` 远端，它们也就共用一个记忆目录。

一个 SQLite 索引支撑跨所有记忆文件的检索：
- **FTS5** 提供默认的全文检索，用于关键词匹配。
- 配置了嵌入模型后，**vec0** 补上用于语义相似度的向量检索。

---

## 自动保存

会话结束时，Chaos 会把一份结构化的元数据摘要写进该会话的每日日志。摘要包含：

- 消息计数（用户、助手与工具结果）。
- 主题：该会话里前几条实打实的用户提示词，最多五条。
- 会话的日期与时间（UTC）。

Chaos 只依据对话元数据生成摘要，不调用 LLM，也不增加延迟。过于简短的会话会跳过保存 —— 实打实的提示词少于三条，或用户文本不足 50 字节的都会跳过。

摘要不记录工具调用、文件路径或 shell 命令。会话 ID 是日志文件名的一部分。要关掉自动保存，设置 `session.save_on_end = false`。想更完整地留存决策、模式与推理过程，请用 `/flush`。

---

## 用 /flush 保存更丰富的知识

想留下更完整的内容 —— 决策、模式、调试流程、API 发现 —— 在 TUI 里用 `/flush`：

```
/flush
```

它会触发一次由 LLM 生成的摘要，把这轮会话里最重要的内容写进一份带日期的会话日志。摘要会被索引，未来的会话里可以检索到。

以下场合适合用 `/flush` 保住重要上下文：
- 压缩之前（压缩会丢弃旧的对话回合）
- 一次有产出的调试会话结束时
- 发现重要的模式或约定之后

---

## 使用记忆

### 记住

让 Chaos 记住某件事，它会把这条笔记追加到某个 `MEMORY.md` 里 —— 项目专属的内容进工作区那份，跨项目的偏好进全局的 `~/.chaos/memory/MEMORY.md`：

```
> remember to always open PR links after pushing
```

Chaos 会把条目作为持久陈述记录在整理好的标题下，例如 `## Preferences`、`## Project Context` 或 `## Debugging`。文件监视器会在下次检索记忆时重建索引，所以新条目在当前会话里就能被搜到。

你也可以用 `/remember` 命令直接存一条笔记：

```
/remember always open PR links after pushing
```

不带文本运行 `/remember` 会进入记忆模式，你接下来输入的一行就成为笔记。两种方式下，Chaos 都会打开一个审核面板展示这条笔记（另有一份可选的改写版本，用 `Tab` 切换）；只有你确认之后笔记才会写入。保存时 Chaos 会显示 `Memory saved to ~/.chaos/memory/MEMORY.md`。

### 忘记

让 Chaos 忘掉某件事，它会找出并删除匹配的条目：

```
> forget the snake_case convention
```

忘记是尽力而为的：模型检索记忆，删掉匹配的条目。想确保删除，请直接编辑 `~/.chaos/memory/` 下的文件，自己动手删掉那一条。要定位文件，打开 `/memory` 浏览器，按 `y` 复制它的路径。

### 回忆

问问 Chaos 记住了什么：

```
> what do you remember?
```

Chaos 会检索所有记忆文件，并按来源分组总结它知道的内容：全局偏好、项目专属知识，以及会话历史。想浏览原始文件，请用 `/memory`。

### 直接编辑

你可以直接编辑 `~/.chaos/memory/` 下的记忆文件。文件监视器会在下次检索记忆时重建索引。想立刻保存当前会话，用 `/flush`；想把会话日志整理成有条理的主题，用 `/dream`。

---

## 用 /memory 浏览记忆

`/memory` 命令会打开一个模态，列出所有记忆文件：

```
/memory
```

文件按作用域分组：
- **全局** —— 跨项目的记忆（`MEMORY.md`）。
- **工作区** —— 项目专属的记忆（`MEMORY.md`）。
- **会话** —— 每个会话的摘要，按时间倒序排列。

这个模态是分栏布局：左边是文件列表，右边是只读的内容预览。随着你在列表里移动，预览会跟着更新。

### 键盘快捷键

| 键 | 操作 |
|-----|--------|
| `↑`/`↓` 或 `j`/`k` | 在文件列表里移动 |
| `PgUp`/`PgDn` | 跳 10 条 |
| `/` | 筛选文件列表 |
| `Enter` | 阅读选中的笔记：预览取得键盘焦点（方向键、`PgUp`/`PgDn`、`Home`/`End` 可滚动它） |
| `y` | 把选中文件的路径复制到剪贴板 |
| `x` | 删除选中的笔记（再按一次 `x` 确认） |
| `t` | 打开或关闭记忆 |
| `Ctrl+F` | 切换全屏 |
| `Esc` | 关闭模态，或离开筛选、预览焦点 |

筛选同时匹配笔记名与笔记内容；多个词必须全部命中。筛选时，预览会滚动到第一个命中处。没有命中时列表会说明；`Backspace` 清空筛选。

预览窗格是只读的。可以用鼠标滚轮、拖动它的滚动条，或按 `Enter` 之后用键盘滚动。在预览文本上拖动即可把那段文字复制到剪贴板；每次复制，文件列表下方都会短暂确认一下。生成的 `MEMORY.md` 索引无法删除。

当记忆模态的内容区不足 64 列时，模态只显示文件列表并隐藏大小列；按 `Enter` 全宽阅读选中的笔记，按 `Esc` 回到列表。

你也可以从命令面板打开 `/memory`。

---

## 记忆通知

用 `/remember` 保存笔记时，Chaos 会在回滚区确认：

```
Memory saved to ~/.chaos/memory/MEMORY.md
```

后台保存 —— 自动 flush、自动 Dream，以及会话结束 —— 都静默运行，不会往回滚区发消息。你自己运行 `/flush` 和 `/dream` 时，它们会在回滚区报告结果。随时可以用 `/memory` 浏览 Chaos 存了什么。

---

## 用 /dream 整理记忆

`/dream` 命令把散落的记忆片段整理成有条理的主题：

```
/dream
```

Dream 会把单个会话日志与记忆条目重组为一份连贯、去重后的知识库，从而随着时间推移降低噪声、提升检索质量。`/dream` 需要记忆处于启用状态。

### 自动 Dream

Dream 也会自动运行。默认情况下，Chaos 在启动时以及会话进行中定期检查整理闸门，等到经过足够时间、攒够足够会话之后运行一次 Dream：

```toml
[memory.dream]
enabled = true     # Run automatic consolidation (default: true)
min_hours = 24     # Minimum hours between consolidations
min_sessions = 5   # Minimum sessions since the last consolidation
check_interval_secs = 3600 # Also check the gates hourly
```

---

## 记忆如何影响提示词

### 首回合注入

在每个会话的第一个回合，Chaos 会自动检索与当前项目相关的记忆内容，并把它作为上下文注入。也就是说，Chaos 一开始就带着此前会话的知识，不需要你再提醒。

首回合注入可以配置：

```toml
[memory.initial_injection]
enabled = true     # Enable or disable first-turn injection
min_score = 0.9    # Score threshold for first-turn injection
```

### 压缩之后

自动压缩之后也会检索一次记忆，以找回可能被丢弃的相关上下文。

---

## 记忆检索

Chaos 会自动检索记忆，你也可以在对话里手动触发检索：

```
Search memory for "auth middleware patterns"
Read my workspace MEMORY.md
```

模型可以访问两个记忆工具：
- `memory_search` —— 检索全部记忆
- `memory_get` —— 按路径读取指定的记忆文件

### 检索打分

默认没有配置嵌入模型，所以记忆起步时是纯全文检索模式。若你配置了嵌入模型，检索会把向量相似度（权重 `0.7`）与 BM25 文本相似度（权重 `0.3`）结合起来。结果会按最低分阈值过滤（默认 `0.7`）。

### 来源权重

每个记忆来源都有一个施加在得分上的权重乘数。所有来源默认 `1.0`，可以在 `[memory.search.source_weights]` 下逐个调整：

| 来源 | 权重 | 说明 |
|--------|--------|-------------|
| `workspace` | 1.0 | 项目专属记忆 |
| `session` | 1.0 | 会话日志 |
| `global` | 1.0 | 跨项目记忆 |

### 时间衰减

会话记忆会随时间衰减，让更近的会话优先：

```toml
[memory.search.temporal_decay]
enabled = true           # Enable time-based decay
half_life_days = 30.0    # Score halves after this many days
```

只有会话分块会衰减。全局与工作区记忆不受影响，因为它们存放的是整理过的长期知识。

### MMR（最大边际相关）

MMR 重排会惩罚冗余结果，以提升多样性：

```toml
[memory.search.mmr]
enabled = true           # Enable diversity re-ranking
lambda = 0.7             # 0.0 = max diversity, 1.0 = pure relevance
```

---

## 命令行

`chaos memory` 命令在 shell 里管理记忆。它只有一个子命令 `clear`：

```bash
# Clear workspace memory (MEMORY.md, sessions/, and index.sqlite). This is the default scope.
chaos memory clear

# The same scope, stated explicitly
chaos memory clear --workspace

# Clear the global MEMORY.md
chaos memory clear --global

# Clear both workspace and global memory
chaos memory clear --all

# Skip the confirmation prompt (-y is the short form)
chaos memory clear --yes
```

想从 shell 编辑记忆，直接用编辑器打开那些文件即可 —— 例如 `$EDITOR ~/.chaos/memory/MEMORY.md`。

---

## 配置参考

### 核心设置（`[memory]`）

| 键 | 默认 | 说明 |
|-----|---------|-------------|
| `enabled` | `false` | 启用记忆 |
| `session.save_on_end` | `true` | 会话结束时写入元数据摘要 |
| `watcher.enabled` | `true` | 监视 `~/.chaos/memory/` 的外部改动并重建索引 |

### 索引设置（`[memory.index]`）

| 键 | 默认 | 说明 |
|-----|---------|-------------|
| `max_chunk_chars` | `1600` | 分块的最大字符数 |
| `chunk_overlap_chars` | `320` | 分块之间的字符重叠量 |

### 嵌入设置（`[memory.embedding]`）

| 键 | 默认 | 说明 |
|-----|---------|-------------|
| `provider` | `"api"` | 嵌入 provider（目前只有 `"api"`） |
| `model` | 未设置 | 嵌入模型名。未设置或为 `""` 时只用全文检索。 |
| `dimensions` | `1024` | 嵌入向量维度 |

### 检索设置（`[memory.search]`）

| 键 | 默认 | 说明 |
|-----|---------|-------------|
| `max_results` | `6` | 最大检索结果数 |
| `min_score` | `0.7` | 最低相关度得分 |
| `vector_weight` | `0.7` | 向量相似度的权重 |
| `text_weight` | `0.3` | BM25 文本相似度的权重 |

### 首回合注入设置（`[memory.initial_injection]`）

| 键 | 默认 | 说明 |
|-----|---------|-------------|
| `enabled` | `true` | 启用首回合的记忆注入 |
| `min_score` | `0.9` | 首回合结果的分阈值 |

### Dream 设置（`[memory.dream]`）

| 键 | 默认 | 说明 |
|-----|---------|-------------|
| `enabled` | `true` | 启用自动 Dream 整理 |
| `min_hours` | `24` | 两次整理之间的最少小时数 |
| `min_sessions` | `5` | 距上次整理至少经过的会话数 |
| `stale_lock_secs` | `3600` | 多久之后可以回收过期的整理锁（秒） |
| `check_interval_secs` | `3600` | 定期检查 Dream 闸门的间隔（秒）。设为 `0` 关闭定期检查。 |

### Flush 设置（`[compaction.memory_flush]`）

flush 配在 `[compaction]` 下而不是 `[memory]` 下，因为它是压缩行为。

| 键 | 默认 | 说明 |
|-----|---------|-------------|
| `enabled` | `true` | 启用压缩前的记忆 flush |
| `soft_threshold_tokens` | `4000` | 触发 flush 的、距压缩阈值的 token 余量 |
| `max_flush_write_chars` | `8000` | 一次 flush 最多可写入记忆的字符数 |
| `flush_model` | 未设置 | flush 回合使用的模型。未设置或为 `""` 时用会话的主模型。 |
| `idle_timeout_secs` | `300` | 后台 flush 之前的空闲秒数。设为 `0` 关闭空闲 flush。 |
| `semantic_dedup_threshold` | 未设置 | 给 flush 内容去重用的余弦相似度阈值。未设置时默认 `0.92`。 |

### 剪枝设置（`[compaction.pruning]`）

剪枝配在 `[compaction]` 下而不是 `[memory]` 下，因为它是压缩行为。

| 键 | 默认 | 说明 |
|-----|---------|-------------|
| `enabled` | `true` | 启用工具结果剪枝 |
| `keep_last_n_turns` | `3` | 最近多少个回合的工具结果永不剪枝 |
| `soft_trim_threshold` | `4000` | 超过多少字符的旧工具结果会被软裁剪 |
| `soft_trim_head` | `1500` | 软裁剪结果从头保留的字符数 |
| `soft_trim_tail` | `1500` | 软裁剪结果从尾部保留的字符数 |
| `hard_clear_age_turns` | `10` | 超过多少个回合后，工具结果被替换为占位符 |

---

## 记忆的时效性

会话记忆变旧之后，Chaos 会在检索结果里给它附上一条时效提示。结果越旧，提示你「先核实当前状态再依赖它」的语气越强。这些提示能帮你发现可能已经不再准确的记忆。全局与工作区记忆永远不会收到时效提示，因为它们存放的是整理过的长期知识。

---

## 文件监视器

默认情况下，Chaos 会监视 `~/.chaos/memory/` 的外部文件改动。如果你直接编辑记忆文件（例如在编辑器里），改动会在下次检索记忆时自动被接收：

- 新建或修改过的文件会重建索引。
- 已删除文件在索引里的过期分块会被清掉。

```toml
[memory.watcher]
enabled = true    # default
```

---

## 故障排查

### 记忆不工作

1. 确认记忆已启用：查看 `chaos inspect` 的输出。
2. 检查 `GROK_MEMORY` 或生效 TOML 里的 `[memory] enabled`。
3. 检查是否有 `GROK_MEMORY=0` 或已废弃的兼容标志覆盖了配置。

### 会话里看不到记忆

记忆是在第一个回合注入的。如果你在启用记忆之前就已经开始了会话，用 `/new` 开一个新会话。

### 查看记忆文件

在 TUI 里用 `/memory` 可以带预览浏览所有记忆文件。也可以直接访问它们：

```bash
ls ~/.chaos/memory/
cat ~/.chaos/memory/MEMORY.md
$EDITOR ~/.chaos/memory/MEMORY.md
```

### 调试日志

```bash
RUST_LOG=debug GROK_LOG_FILE=/tmp/grok.log chaos
grep "memory" /tmp/grok.log
```
