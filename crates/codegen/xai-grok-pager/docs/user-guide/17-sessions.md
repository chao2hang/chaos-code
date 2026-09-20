# 会话管理

Chaos 会把每一段对话自动存到磁盘上。无论你是在 TUI 里工作、走无头模式，还是通过 agent stdio，Chaos 都会把这段来往记录成一个会话。你可以恢复、回退或压缩它。本文说明如何管理会话。

---

## 会话是什么

会话是一段带完整历史的持久对话，包含：

- 所有用户提示词与代理回复
- 工具调用及其结果
- TODO/任务清单状态
- 用于撤销后续回合的回退点
- token 用量与回合计数
- 子代理会话（启用时）

会话由一个唯一的会话 ID 标识（Chaos 自己生成时是 UUIDv7；客户端可以用 `-s` 提供自己的 ID），存放在磁盘上的 `~/.chaos/sessions/` 下。基准目录可以用 `CHAOS_HOME` 覆盖（兼容读取 `GROK_HOME`，但 `CHAOS_HOME` 优先）；两个都没设置时，已经存在的 `~/.chaos` 优先于已经存在的 `~/.grok`，都不存在就默认用 `~/.chaos`。

---

## 存储布局

Chaos 把每个会话放在它自己的目录里，按工作目录分组。分组的名字就是 URL 编码后的工作目录。编码后的名字超过 255 字节时，它改用「短名 + 哈希」，并把原始路径记在组内的一个 `.cwd` 文件里。

```
~/.chaos/sessions/<encoded-cwd>/<session-id>/
  summary.json            # metadata: summary/title, timestamps, model ID, message counts
  updates.jsonl           # ACP session update stream (conversation + tool calls)
  chat_history.jsonl      # raw chat messages sent to the model
  system_prompt.txt       # the rendered system prompt, as sent to the model
  prompt_context.json     # the inputs the system prompt was rendered from
  tool_definitions.json   # function tools sent on the latest model call (no MCP server__tool entries)
  plan.json               # TODO/task list state
  rewind_points.jsonl     # rewind points for /rewind undo
  signals.json            # session signals (token usage, tool/turn counters)
  feedback.jsonl          # user feedback and ratings
  compaction_checkpoints/ # saved state from compaction (manual or auto)
  subagents/              # per-subagent metadata (meta.json); the child sessions live in the normal sessions tree
```

`summary.json` 是索引条目。它记录会话摘要与生成的标题、模型 ID、创建与更新时间戳、消息计数，以及分叉或恢复出来的会话的父会话引用。它还记录最近一次的回合摘要与会话回顾，好让各种列表界面能把它们显示出来。`updates.jsonl` 是权威的对话日志，`/resume` 与会话恢复都靠它。`tool_definitions.json` 不含 MCP 的 `server__tool` 条目，因为模型是经 `search_tool` 与 `use_tool` 去访问那些工具的，而这两个本身在列。逐回合的 token 与费用累计可以用 `chaos usage` 看到。

### 会话标题

看板和 `/resume` 里显示的会话标题由对话自动生成。提示框边框只在手动 `/rename` 之后才显示标题，草稿被暂存时旁边还会带一个 `Stashed` 说明。标题生成在你第一条提示词之后就立即开始，所以会话总有标题；随后在最初几个回合里，标题会基于整段对话重新生成，然后冻结下来。这样标题既能越过一句含糊的开场白、反映这个会话真正在做什么，之后又保持稳定，不至于让你跟丢自己的会话。手动 `/rename` 永远赢：一旦你重命名过，自动生成就再也不会覆盖它。想把标题交还给自动生成，用 `/rename --auto`。

---

## 开始与结束会话

### 新建会话

每次启动时 TUI 都会新建一个会话。要在会话中途显式开一个全新的：

```
/new
```

它会清空当前上下文并开始一段新对话。别名：`/clear`。

### 退出

结束会话并退出 Chaos：

```
/quit
```

别名：`/exit`。想离开当前会话但仍留在 Chaos 里，用 `/home` 回到欢迎界面。

### 删除当前会话

```
/delete
```

先确认，然后永久删除该会话的历史。删完回到欢迎界面；如果你是从看板打开的这个会话，就回看板。在 `/resume` 或欢迎界面的会话列表里，按 `d` 再按 `y`。在[代理看板](23-dashboard.md)上，连按两次 `Ctrl+X`（或把鼠标悬到 `[✗]`）即永久删除。

---

## 恢复会话

### 在 TUI 里

用 `/resume` 命令浏览并恢复以前的会话：

```
/resume
```

它会打开会话选择器，列出当前工作区的近期会话。选中一条即可恢复。该命令不接受参数。

在选择器里输入会按标题过滤列表，同时实时搜索对话内容；内容命中的结果显示在 "Extended search results" 标题下。按 `Ctrl+/` 可跳过那短暂停顿、立即搜索。

要在本分页器里管理活的顶层会话（父会话与分叉）——切换、改名、窥视、派发、关闭——请用[代理看板](23-dashboard.md)：`/dashboard`（别名 `/sessions`、`/agents-dashboard`）或 `Ctrl+\`。

### 在命令行里

按 ID 或标题恢复指定会话：

```bash
chaos --resume <session-id-or-title>
```

不是会话 ID 的值会与当前目录的会话标题比对，忽略大小写（就是简单的转小写比较）——在 `/rename` 之后很好用。如果多个会话共用同一标题，单个被手动改过名的会话优先于自动生成的重复项；否则命令报错并列出匹配的 ID。UUID 形状的值一律按会话 ID 处理，绝不会当作标题。脚本里应优先用 ID。

不带值运行 `chaos --resume` 会恢复当前目录最近的会话。

### 从欢迎界面

启动 `chaos` 时，欢迎界面会列出当前目录的近期会话。选中一条即可恢复。

---

## 分叉与重命名会话

### 分叉

把当前会话分叉成一个对等代理，从对话的一份拷贝开始：

```
/fork [--worktree|--no-worktree] [directive]
```

可选的 `directive` 会设置新会话的第一条提示词。用 `--worktree` 或 `--no-worktree` 选择分叉是否在新的 git worktree 里运行；两个都不写就每次都问你。本版本不支持 `--at <turn>` 参数。

### 重命名

重命名当前会话的标题：

```
/rename <title>
/rename --auto
```

别名：`/title`。`/rename --auto` 会清除手动标题并重新启用自动起标题。

---

## rewind 命令

`/rewind`（别名 `/undo`）把对话回退到更早的一轮，并丢掉其后的轮次。那一轮之后对文件做的改动原样留在磁盘上。

```
/rewind
/undo
```

当你运行 `/rewind` 或 `/undo`（或在空闲、提示词为空、对话里已有消息时 800ms 内按 **Esc Esc**）时，Chaos 会：

1. 列出可回退的点（每个用户提示词一个）
2. 让你选择回到哪个点
3. 把对话历史截断到那个点

当 **Confirm before rewind** 开着时（`/settings` 里的默认值），每次选择都会请求确认（界面上的按钮是 Yes / Yes, and don't ask again / No）。选 **Yes, and don't ask again** 会把这个设置关掉。设置关掉后，选择立即执行。

**重要：** `/rewind` 不会还原磁盘上的文件。被截断的只有对话历史。

---

## compact 命令

`/compact` 会压缩对话历史，以节省上下文窗口空间。适合用在早期消息已经不再相关的长会话里。

```
/compact
/compact [context]
```

可选的 `context` 参数让你补充说明压缩时要保留什么。

### 自动压缩

上下文窗口接近上限时，Chaos 会自动压缩对话。自动压缩触发时你会看到一条通知。模型配置上的 `context_window` 设置决定何时到达这个阈值。

---

## session-info 命令

查看当前会话的详情：

```
/session-info
```

它会显示：

- 会话标题（设置过时）
- Shell 版本
- 认证方式（OAuth 还是 API key；API key 会话还会附一句建议你跑 `grok login` 用 SuperGrok 订阅——那是上游遗留的屏幕文本，本分叉没有账号登录，凭据请改用 `/provider` 配置）
- 会话 ID
- 工作目录
- 模型（编码模型还会带上模型哈希）
- API 后端与沙箱配置（设置过时）
- 上下文窗口用量（已用与总 token 数，以及使用百分比）

在 Session info 标签页上，点某个值即可复制它，或拖动选中一段（高亮与工具查看器一致）。`c` 复制会话 ID，`y` 复制整块。复制走的是与 Chaos 其他部分相同的剪贴板通道，包括 `chaos wrap`。

---

## 无头模式的会话管理

在无头模式下，你通过命令行参数管理会话：

```bash
# New session each time (default)
chaos -p "Hello"

# Resume an existing session by ID or title (errors if it does not exist)
chaos -p "Continue where we left off" -r <session-id-or-title>

# Continue the most recent session in the current directory
chaos -p "What were we doing?" -c
```

在无头模式下，用 `-r`/`--resume` 恢复已有会话，会话不存在就报错；或用 `-c`/`--continue` 继续当前目录最近的会话。非 ID 的值会与当前目录的会话标题比对，忽略大小写（重复项里唯一被手动改过名的那个胜出；其余重复项会带着各自的 ID 报错；UUID 形状的值一律走 ID 那条路）——脚本应该把 JSON 输出（见下）里的会话 ID 传给 `-r`。

`-s`/`--session-id` 只用来**新建**一个带 **UUID** 的会话（值不是 UUID、或目标会话目录下该 ID 已有会话，都会报错）。它**不会**恢复已有会话——那是旧的隐藏 upsert 行为；请改用 `-r`/`-c`。只有在同时传 `--fork-session` 时，才把 `-s` 和 `-r`/`-c` 一起用（把历史分叉到一个新 ID；可选的 `-s` 指定子会话 UUID）。这与 Claude Code 的防覆盖模型一致（在写入用的 cwd 下做客户端预检；顺序使用可靠，同 ID 并发只是尽力而为）。

要把会话 ID 读回来，就请求 JSON 输出：

```bash
chaos -p "Hello" --output-format json | jq -r '.sessionId'
```

---

## Agent stdio 会话管理

用 ACP 构建时，会话通过协议方法来管理：

```typescript
// Create new session
const { sessionId } = await connection.request("session/new", {
  cwd: "/path/to/project",
  mcpServers: [],
});

// Load existing session
await connection.request("session/load", {
  sessionId: "existing-session-id",
  cwd: "/path/to/project",
  mcpServers: [],
});

// Change a live option (model or reasoning_effort).
// session/new and session/load already return the typed configOptions list.
await connection.request("session/set_config_option", {
  sessionId,
  configId: "model",
  value: { value: "grok-4.6" },
});
```

代理会自动持久化所有会话更新。客户端可以按 ID 重连并加载以前的会话。选项 ID、值的形状，以及 leader 模式下的旁听行为，见[代理模式](15-agent-mode.md#session-config-options)。

---

## chaos sessions 子命令

在命令行里列出或搜索会话。`chaos sessions` 需要一个子命令：

```bash
# List recent sessions for the current directory
chaos sessions list

# Limit the number of results (default 20)
chaos sessions list --limit 50

# Search sessions by keyword (matches titles and prompts)
chaos sessions search "rate limit"
```

`chaos sessions list` 显示当前工作目录的会话，按 worktree 标签分组。每一行列出来会话 ID、创建与更新日期、来源状态和摘要。`chaos sessions search` 把本地 SQLite 索引与远程结果合并起来。

---

## chaos usage 子命令

打印某个会话持久化下来的 token 与费用用量。要读这些数据就用它，而不是直接读会话文件：

```bash
# Session totals plus every recorded turn
chaos usage <session-id>

# One turn
chaos usage <session-id> 3
```

输出是 JSON，包含 `sessionId`、`updatedAt`、`session` 和 `turns`。指定某一轮时信封相同，只是 `turns` 里只有一个元素。会话合计覆盖整段对话，包括通过恢复或分叉继承下来的历史。`costUsdTicks` 是每美元 10¹⁰ 个 tick（除以 `1e10` 得到美元）。轮次号不存在就报错。交互式的额度与计费仍在 TUI 里的 `/usage`。

---

## Worktree 会话

配合子代理或会话分叉时，Chaos 可以给每个会话建一个隔离的 git worktree。每个 worktree 都有自己的工作目录副本，因此一个会话里的文件改动不会影响另一个。

worktree 会话在内部通过 `x.ai/git/worktree/*` 扩展方法管理。关键操作：

- **Create**：为隔离会话新建一个 worktree
- **Apply**：把 worktree 的改动合并回主工作目录
- **Remove**：会话结束后清理该 worktree

用 `chaos -w -r <session-id>` 在一个全新的 worktree 里恢复会话。

### 查看磁盘占用

`chaos du`（别名：`chaos disk-usage`）报告 Chaos 基准目录（`~/.chaos`）在磁盘上占用了多少空间。它先按大小从大到小列出每个顶层目录，然后逐个列出 worktree，给出大小、类型、年龄、标签和路径。登记表没有跟踪的 worktree 显示为 `untracked`。传 `--json` 可以得到同样内容的机器可读输出。

```text
Disk usage for ~/.chaos
    412.3 GB  worktrees
      1.2 GB  sessions
    412.0 MB  (top-level files)
    413.9 GB  total
  Worktree clones share storage with their source, so the total can exceed real disk use.

Worktrees
        SIZE  TYPE                AGE        LABEL  PATH
    380.0 GB  session             12d ago    my-fix ~/.chaos/worktrees/xai/worktree-abc
     32.3 GB  untracked (session) 40d ago           ~/.chaos/worktrees/xai/worktree-old

To reclaim space, run `chaos worktree gc --max-age 7d --dry-run`, then the same command without `--dry-run`. Without `--max-age`, gc expires nothing, and it keeps a worktree whose work it cannot find elsewhere, naming each one.
Untracked rows are not in the registry, so gc never visits them. Remove one with `chaos worktree rm --dry-run <path>`, then without `--dry-run`.
```

`AGE` 就是 `chaos worktree gc` 度量的那个值：该 worktree 最后一次被访问距今多久；如果它是在那之后才创建的，则从创建算起。会话与代理活动会更新它；在目录里开着的 shell 或编辑器不会。未被跟踪的 worktree 没有登记表条目，因此它的年龄来自其下最新的那个文件。

大小在 Unix 上是物理块数、在其他平台上是逻辑文件大小，与 `chaos worktree show` 报告的一致。worktree 克隆与它的来源共用存储，而每份拷贝都全额计入，因此合计可能同时超过 `du -sh` 和实际占用的空间。当合计超过该卷上已用的空间时，报告会说明这一点。`--json` 以 `volume_capacity_bytes` 与 `volume_available_bytes` 携带同样的数字。

报告只度量一个文件系统，即存放 Chaos 基准目录的那个。位于其他文件系统上的目录不计入合计，而是计入 `other_filesystem_dirs`，它的 worktree 行大小显示为 `-`（`--json` 里是 `null`）。指向目录的顶层符号链接——比如被挪到别处的 `worktrees`——计入 `unfollowed_dir_symlinks`；它的目标不计入合计，但它下面的行仍然计算大小。报告读不到的目录与条目分别计入 `unreadable_dirs` 和 `unstatable_entries`。运行 `RUST_LOG=debug chaos du` 就能逐个点名。

`--json` 里每个 worktree 行还带有以 unix 秒计的 `created_at`、`last_accessed_at`、`last_modified_at`，以及 `repo_name` 和 `git_ref`。未被跟踪的行，其登记表字段为 `null`。`git_ref` 是登记该 worktree 时记下的分支，不是当前检出的那个分支。

登记表不可用时，每一行都显示为 `untracked`，报告会说明原因。`--json` 的 `registry` 字段携带同样的值：`read`、`absent`、`busy`、`unopenable` 或 `corrupt`。`busy` 的登记表被另一个进程占着，重试即可。`unopenable` 的有权限或 I/O 问题，检查那个文件。只有 `corrupt` 才需要删除：删掉报告点名的文件，然后运行 `chaos worktree db rebuild`。

要回收空间，运行 `chaos worktree gc --max-age 7d`，它会移除比你给出的年龄更老的、已被跟踪的 worktree。不带 `--max-age` 时 gc 不会让任何 worktree 过期，而且它只访问登记表跟踪的 worktree。移除未被跟踪的 worktree 用 `chaos worktree rm <path>`。两条命令都接受 `--dry-run`，并报告自己将要做什么：gc 数出它会移除多少个 worktree，`rm` 点名路径。

每一轮都会在大约一分钟内尽量多判定一些 worktree，因为同一趟判定还按定时器在你的会话旁边跑，而读完一整个工作树并不便宜。没轮到的那些会被记为 `Not judged this pass`，等下一轮再看；所以在一台有很多东西可回收的机器上，要反复跑 gc 直到那个数字归零。

在移除一个过期 worktree 之前，gc 会检查这次移除是否会毁掉成果：未提交的文件，未被跟踪或被忽略的文件，没有任何存活 ref 持有的提交，或只存在该 worktree 的 git 目录里的状态。它无法检查的 worktree 也一样保留。报告会数出被保留的 worktree 并点名原因，与那些被活跃进程挡住的区分开。`--force` 不会跳过这项检查，而 `chaos worktree rm` 根本不应用它：你点名哪个路径它就删哪个。

被忽略的文件也算成果，只有一个例外：被仓库自己的忽略规则排除、并且要么带某个工具的缓存标记、要么名字像它的某个输出目录（`target`、`node_modules`、`.venv` 等等）的目录。光看名字永远不够，所以一个没人排除的手写 `build/` 仍然保得住 worktree。

只在某个 worktree 自己的 reflog 里留名的提交——`reset --hard` 或 amend 会留下这种东西——会在该 worktree 来源的那个仓库里，以 `refs/grok/reclaimed/<worktree>/<commit>` 获得一个长久的名字。Git 在修剪时把 reflog 算作可达性，所以没有这个名字，移除该 worktree 就会让那个提交变成不可达。用 `git log refs/grok/reclaimed/` 和 `git branch <name> <commit>` 可以把它找回来。

这些名字不会越积越多。每趟 gc 都会丢掉那些已经不再持有任何东西的：该提交现在能从真实 ref 到达了，或者它已经超过 30 天。报告把它们计为 `names_collected`。

---

## 会话存储细节

### 持久化格式

Chaos 把对话存成换行分隔的 JSON（JSONL）。`updates.jsonl` 里的每一行都是一个自包含的 ACP 会话更新事件。这种格式支持：

- 增量写入（会话期间只追加）
- 高效的流式读取（用于恢复会话）
- 便于调试（每一行都是合法 JSON）

那些较小的状态文件 —— `summary.json`、`plan.json` 和 `signals.json` —— 是普通 JSON 而不是 JSONL。JSONL 是会话内容的事实来源；`chaos sessions search` 另外还为会话标题与提示词维护一个本地 SQLite FTS5 索引，以便快速做关键词搜索。

### 会话元数据

`summary.json` 会记录（除其他字段外）：

- `info` —— 会话 ID 与工作目录
- `session_summary` 与 `generated_title` —— 会话摘要及其由模型生成的标题
- `title_is_manual` —— 为 true 时表示标题是手动 `/rename` 设置的（这样自动生成就不会动它）
- `created_at` 与 `updated_at` —— 创建与最后更新的时间戳
- `num_messages` 与 `num_chat_messages` —— 更新数与聊天消息数
- `current_model_id` —— 当前使用的模型
- `parent_session_id` —— 分叉或恢复时的来源会话
- `agent_name` —— 会话最后保存时生效的代理定义
- `last_turn_summary` —— 最近一轮的超短摘要
- `last_recap` —— 最新会话回顾的有界预览

### 磁盘占用

在长会话里，会话历史（`updates.jsonl`、`chat_history.jsonl`）是磁盘占用的主要部分。用 `/compact` 可以缩小历史。

---

## 小技巧

- 当前上下文已经不再相关时，用 `/new` 从零开始。
- 长会话里主动用 `/compact`，让上下文窗口保持有效。
- 用 `/rewind` 撤销失误；它把对话回退到更早的一轮（被丢掉的轮次所做的文件改动原样保留）。
- 无头模式下，从 JSON 输出里取出 `sessionId` 传给 `-r`，就能搭出保持上下文的多步自动化。
- 看 `/session-info` 可以知道上下文窗口用掉了多少。
