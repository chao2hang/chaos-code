# 用户指南待核验断言的核对结果（2026-09-18）

T1-7c / T1-7d 原本要把上游增量与本轮行为注记写进 `04`、`10`、`23`、`25`
四章。那些断言此前只来自上游 diff，没有和代码对过。本次逐条核对，
结论直接决定译文该写什么：**其中 5 条是错的，包括两条原本已经准备落笔的。**

核对方式是读代码取 `file:line`，不看文档、不看上游提交信息。
下列行号均可复查。

## 一、错误的断言 —— 译文不能照写

| # | 原以为 | 实际 | 证据 |
| --- | --- | --- | --- |
| 1 | `/memory` 在 `[memory] enabled = false` 时不可用 | **恰好相反**：门禁是 `BuiltinGate::MemoryConfigured`，判据为 `memory.backend_params.is_some()`。字段注释写明它表示「已配置但未必当前启用」，gating 的目的正是**让用户关掉之后还能重新打开** | `xai-grok-shell/src/session/slash_commands.rs:440-445`、`:118`、`:40`、`:47` |
| 2 | `/flush` 有状态串 `"Flushing memory…"` | 不存在。实际为 headless 的 `Memory flush started.` / `Memory flush {result}: {path}`，以及回滚区事件「记忆已保存（{trigger}） → {short_path} · 用 /memory 查看」 | `xai-grok-pager/src/headless/reducer/mod.rs:142-146`、`src/scrollback/blocks/session_event.rs:270-273` |
| 3 | `/flush` 有状态串 `"Memory flushed through turn 12"` | 不存在，全库无该格式串 | 同上 |
| 4 | `/dream` 有状态串 `"Consolidating memory…"` | 不存在。`/dream` 成功时**有意不产生用户可见输出**（与 `/flush` 一致），仅在跳过时经 `MemoryDreamCompleted` 上报 `skipped: {reason}`，reason 取值如「another consolidation is already running」「could not acquire the consolidation lock」「nothing new to consolidate」 | `xai-grok-shell/src/session/acp_session_impl/slash_exec.rs:78-88`、`memory_dream.rs:272-289`、`:318-335` |
| 5 | 记忆浏览模态绑定 `s` 键 | 未绑定。实际绑定 `j`/`k`、`↑`/`↓`、`PgUp`/`PgDn`、`x`、`y`、`t`、`/`、`i`、`Ctrl+F`、`Backspace`。其中 `t` 切换开/关（发出 `/memory on` 或 `/memory off` 并乐观更新），`x` 删除（仅会话日志，需二次确认） | `xai-grok-pager/src/views/memory_modal.rs:994-1004`、`:973-983`、`:944-1011` |

## 二、另有两条断言不成立

| # | 原以为 | 实际 | 证据 |
| --- | --- | --- | --- |
| 6 | `10-hooks.md` 应加注「`GROK_SESSION_ID` 已不再传给钩子」 | **仍然注入**：命令型 `.env("GROK_SESSION_ID", ctx.session_id)`，HTTP 型同样设置；且它在保留变量清单中，用户 env 无法伪造 | `xai-grok-hooks/src/runner/command.rs:175`、`runner/http.rs:159`、`config.rs:244` |
| 7 | —— | 上游 `#problem-ctrlenter-...` 这类锚点漂移与本节无关，见 `03`/`21` | 已随 5572bb6b 修复 |

第 6 条尤其值得记一笔：它与第 1 条一样，本来已经准备写进译文。核对之后，
`10-hooks.md` 不需要任何行为注记 —— 上游文本在这一项上就是对的。

## 三、核对为真的断言

| # | 断言 | 证据 |
| --- | --- | --- |
| 8 | `/effort` 接受 `max`；内置回退等级为 `max` / `xhigh` / `high` / `medium` / `low` | `xai-grok-pager/src/slash/commands/effort_levels.rs:8-14` |
| 9 | 不存在「把 high/medium/low 映射到别的值」的别名表；`none`/`minimal` 可被解析，但只有当模型菜单提供时才接受，否则报 unknown effort level | `effort_levels.rs:29-44`、`src/acp/model_state.rs:186-212` |
| 10 | provider 的 `reasoningEfforts` 优先于内置菜单，缺失/非数组/不可用时才回退 | `src/acp/model_state.rs:160-172` |
| 11 | `supportsReasoningEffort: false` 使菜单为空并报「current model does not support reasoning effort」；**缺失声明则放行** | `src/acp/model_state.rs:243-249`、`:14-17` |
| 12 | `/model <name> [effort]` 接受第二个参数（`split_trailing_token` 在最后一段空白处切分，尾部 token 经 `resolve_effort_for_model` 校验） | `src/slash/commands/model.rs:44-66`、`:79-88` |
| 13 | `/btw` 可在流式回合中使用 —— 绕过提示队列 | `src/app/dispatch/notes.rs:655-657` |
| 14 | `/usage` 打开三标签的会话内模态：`Context usage` / `Usage limit` / `Session info`，默认落在 `Usage limit`。看板的 `/usage` 打开**同一个**模态（不是另一个视图），只是会话标签显示 No active session | `src/views/usage_modal.rs:33-52`、`src/app/dispatch/status.rs:330-334` |
| 15 | 看板 `s:` 过滤接受 `paused`，与 `inactive`/`dormant`/`blocked` 同归 `RowState::Inactive` | `src/views/dashboard/state.rs:4284-4290` |
| 16 | `turn-timer` 格式取自 `format_elapsed`：时>0 为 `{h}h{m:02}m`，分>0 为 `{m}m{s:02}s`，否则 `{s}s` | `src/views/goal_detail.rs:58-69`、`src/views/status_line/segments.rs:105-111` |

## 四、对译文的直接要求

- `04` 的 `/memory` 小节：**不要**写「关闭记忆后 `/memory` 不可用」。
  应写：`/memory` 只要后端已配置就可用（`backend_params` 存在即可），
  这样用户关掉记忆后还能用同一命令打开。这正是它不跟 `enabled` 走的原因。
- `04` 的 `/flush`、`/dream` 小节：不要引用上面那三个不存在的状态串。
  `/flush` 按实际事件文案描述；`/dream` 要说明**成功时没有用户可见输出**，
  只有跳过时才给出原因。
- `04` 的 `/memory` 模态键位：按第二节第 5 条的真实键位写，不要写 `s`。
- `10-hooks.md`：不需要任何行为注记。
- `23`、`25`：第 14–16 条为真，可照写。

---

## 五、`/login` `/logout` 的定性（推翻早先「删除」的处理）

早先按「分叉不存在登录」把 `/login`、`/logout` 归入要删除的上游功能。
逐文件核对后**不成立**，两者都是本分叉注册的兼容桩：

| 事实 | 证据 |
|---|---|
| `slash/commands/mod.rs` 的第 151–152 行确实注册了 `LoginCommand`、`LogoutCommand` | `slash/commands/mod.rs:151-152` |
| 有测试断言 `/login` 是 builtin | `slash/registry.rs:970`、`:1006` |
| `/login` 的行为是 fail-closed：不启动浏览器 OIDC，直接打开 provider 模态 | `slash/commands/login.rs:19-21`，描述文案「Chaos 不支持账号登录；请使用 /provider 配置 API Key」 |
| `/logout` 只返回一条提示消息，让你去改 `config.toml` / 用 `/provider` | `slash/commands/logout.rs:17-22` |
| CLI 侧 `chaos login` 是真实交互入口（不是死枚举） | `xai-grok-pager-bin/src/main.rs:55` → `(Entrypoint::Cli, Interactivity::Interactive)`；`:2240` 另有匹配臂 |

**两个源码模块注释是过期的**：`login.rs:3` 与 `logout.rs:3` 都写着
「Not registered in `builtin_commands()`」，但 `mod.rs` 注册了它们。
照抄注释会写出与代码相反的话。

**因此修正**：

- `02-authentication.md` 原文写「`/login`、`/logout` … **未注册**」——已改为
  「仍在命令表中，但都是兼容桩」，差异表那行同步改（本次提交）。
- `04-slash-commands.md` 的 `/login`「Log in or re-authenticate without
  leaving the session」、`/logout`「Log out and return to the login screen」
  都是**上游行为**，本轮译成兼容桩的实际行为，不是删除。
- `14-headless-mode.md` 的「Authentication for Headless Environments」整节
  要按 BYOK 重写：删掉 `grok login --device-auth` / `grok login` 两条，
  指向 `02-authentication.md`。它也是目前 `--links` 报的
  `02-authentication.md#device-code-flow` 死锚点的来源。

## 六、命令名与路径的本地化（`--fork-names`）

新增的机器检查显示整本指南有 **443 处**仍是上游拼法（截至第 3 章提交后
剩 424 处）。逐条核对代码后的对照表已写进
`sync/doc-l10n-conventions.md` §4.1，要点：

- **只有 `GROK_HOME` 有 Chaos 孪生**（`CHAOS_HOME`，优先级更高；
  `xai-dirs/src/lib.rs:86-113` 的 `resolve_grok_home_from`）。其余 `GROK_*` 环境变量没有对应项，
  一律原样保留 —— 不要顺手改成 `CHAOS_*`。
- `/etc/grok/` **没有**改名：`system_config_dir()` 仍返回它
  （`xai-grok-config/src/paths.rs:98-104`）。
- 项目级目录双读，合并顺序是 `.grok` 在前、`.chaos` 在后（后者胜）：
  `project_config_dirnames()` (`paths.rs:54-56`)。
- 用户级解析顺序与 §4.2 的措辞完全一致，已核对 `resolve_grok_home_from()`
  与 `dual_default_home_in()`。

## 七、代码侧遗留（本轮不改，仅记录）

- `chaos --help` 的若干文案仍是上游拼法：`Command::Wrap` 的 `long_about`
  里写着 `grok wrap docker exec …` 与 `~/.grok/README.md`；
  `Command::DiskUsage` 的注释是「what the grok home (~/.grok) uses on disk」；
  `Command::Dashboard` 的注释提到 `~/.grok/config.toml`
  （`xai-grok-pager/src/app/cli.rs:87`、`:90` 为 `Wrap`；`:142` 为 `DiskUsage`；
  `:152-153` 为 `Dashboard`）。
- `slash/commands/logout.rs:19` 的提示文案里写的是 `~/.grok/config.toml`，
  而该章正文按 `~/.chaos` 写。
- 这两处属于源码文案而非用户指南，按「不碰架构」的范围留待后续统一。

---

## 八、第 13 章《记忆》的核对（2026-09-18）

翻 `13-memory.md` 之前按同样的方式把它逐节和代码对了一遍。这一章的问题比
前面四章都重：它有一整节描述了一套**本仓库根本没有的存储架构**。

### 8.1 不成立的断言

| # | 原文断言 | 实际 | 证据 |
| --- | --- | --- | --- |
| 17 | 「`topics/` 存放整理过的笔记，一个主题一个文件；完成的回合先落成小的 observations，之后由 `/dream` 折叠进 topics；两个作用域的生成式索引每会话注入一次」 | **没有 `topics/` 目录，也没有 observation 层。** 每个作用域只有**一份** `MEMORY.md`：全局在 `grok_home()/memory/MEMORY.md`，工作区在 `grok_home()/memory/<slug>-<hash8>/MEMORY.md`；会话日志在 `.../<slug>-<hash8>/sessions/`；索引是 `.../<slug>-<hash8>/index.sqlite`。`/dream` 是把会话日志喂给模型、拿回**一份** markdown 文档写回工作区 `MEMORY.md`，然后删掉已处理的会话日志 | `xai-grok-memory/src/storage.rs:51-56`、`:103`、`:113`、`:118`、`:140`；`dream.rs:350-372`（`write_long_term(Workspace, …)`）、`:302`（`clean_processed_sessions`，`:298` 有 5 分钟新近度保护） |
| 18 | 「早期版本的笔记在更新后首次打开工作区时自动迁移，同名小节追加到 `"From earlier sessions"` 标题下」 | **没有这段迁移代码。** 全仓库（不含 `target/`）搜 `From earlier sessions` 只命中本文档自身；`xai-grok-memory` 里唯一的 `legacy` 是 dream 锁的旧文件名回退与 SQLite WAL 注释，与笔记迁移无关 | `dream_lock.rs:451-470`、`index.rs:784`（两处都不是迁移） |
| 19 | 「（打开记忆）会在下一个回合注入记忆索引」 | **不会重新注入。** `MemoryToggle` 只做三件事：`ensure_initialized()`、重新注册 `memory_search`/`memory_get`、写回 storage。`context_injected` 没有被重置，而首回合注入只在它为 `false` 时发生 | `slash_exec.rs:750-795`；置位处 `turn.rs:2001-2005`，初始化 `spawn.rs:1814` |
| 20 | 首回合注入的是「两个作用域的有界生成式索引」 | 注入的是**检索结果**：取最后一条真实用户提问（若是空/短于 20 字符/寒暄，则退回 `"project conventions preferences architecture"`）调 `backend.search(&query, 6, min_score)`，把命中格式化成提醒 | `turn.rs:2013-2116`；`session/helpers/memory_context.rs:29-60` |

第 17、18、19 条按 §4.3 处理：改写为真实布局 / 删除描述。第 20 条在同一节里
与本文档自己的「首回合注入」一节自相矛盾，随手删掉。`topics/` 的
行内代码 span 丢失已在 `scripts/doc-span-removals.tsv` 里声明。

### 8.2 保留未改的一条（散文数字是硬门禁）

| # | 原文断言 | 实际 | 证据 |
| --- | --- | --- | --- |
| 21 | 「当记忆模态的内容区不足 **64** 列时，只显示文件列表并隐藏大小列」 | 阈值是 **80**：`SPLIT_MIN_WIDTH: u16 = 80`，`show_preview = content_area.width >= SPLIT_MIN_WIDTH`。另外「隐藏大小列」也不准 —— 窄宽度下只是不画分栏预览和分隔线、列表占满整宽，元信息列（修改时间）在 `render_file_list` 里照画 | `xai-grok-pager/src/views/memory_modal.rs:36`、`:424-429`、`:596-618` |

**这条故意没改。** 散文数字由 `numbers` 不变量逐字比对，没有像
`doc-span-removals.tsv` 那样的声明通道，而 `sync/doc-l10n-conventions.md` §五
明令「不改数字」。要修得先给散文数字开一条声明机制（或等上游自己修），
不在本轮范围内。修的时候一并把「隐藏大小列」改掉。

### 8.3 核对为真、照写

| # | 断言 | 证据 |
| --- | --- | --- |
| 22 | 存储表格三行、`<project-slug>-<hash8>` 后缀、身份取 `origin` 远端的 `org/repo` 形式（无远端则取目录路径）、克隆与工作树共用一份 | `storage.rs:584-618`、`:620-624` |
| 23 | 自动保存写结构化元数据摘要、不调 LLM、不加延迟；主题取前五条真实用户提示词；含消息计数与 UTC 时间；真实提示词少于 3 条或用户文本不足 50 字节则跳过；`session.save_on_end`；会话 ID 进文件名；不记录工具调用、路径与 shell 命令 | `xai-grok-shell/src/session/memory/hooks.rs:1-35`、`:75-118`、`:123-166` |
| 24 | 索引由 FTS5 全文检索 + 可选 vec0 向量检索组成 | `xai-grok-memory/src/index.rs:1-6`、`:166` |
| 25 | 时效提示只给会话来源，全局与工作区不附加 | `session/helpers/memory_context.rs:48`；测试 `:269` |
| 26 | 记忆埋点只有枚举、布尔、计数、时长与分数（`session_id` 之外没有自由文本） | `observation.rs:26-47`；`xai-grok-telemetry/src/memory_telemetry.rs:83-158` |
| 27 | 配置参考各表的默认值与键名 | `xai-grok-config-types/src/memory.rs:134-140`、`:155-162`、`:192-210`、`:228-236`、`:245-270`、`:341-352`、`:376-393`、`:408-423`、`:466-508` |
| 28 | `/remember` 先开审核面板，`Tab` 切换改写版本，`Enter`/`y` 才写入 | `xai-grok-pager/src/app/modals.rs:199-240` |
| 29 | 浏览模态键位：`j`/`k`、`↑`/`↓`、`PgUp`/`PgDn` 每次 10 条、`x` 仅会话日志且需二次确认、`y` 复制路径、`t` 切换、`/`（`i` 亦可）进筛选、`Ctrl+F` 全屏、`Backspace` 清筛选 | `xai-grok-pager/src/views/memory_modal.rs:907-1012` |
| 30 | 通知串 `Memory saved to …` 用的是配置根路径，本分叉渲染成 `~/.chaos/memory/MEMORY.md` | `xai-grok-pager/src/app/dispatch/notes.rs:815` |
| 31 | `/dream` 要求记忆已启用 | `slash_exec.rs:85` |

### 8.4 又一处源码文案遗留（本轮不改）

`ensure_initialized()` 给全局 `MEMORY.md` 的模板已经是中文
（`# 全局记忆` / `## 偏好设置`），但工作区那份还是英文
（`# Project Memory — {cwd}` / `> Auto-populated by dream consolidation. Edit freely.`）：
`xai-grok-memory/src/storage.rs:352-362` 与 `:374-383`。属源码文案，
按「不碰架构」留待后续统一。

---

## 九、第 4 章《斜杠命令》的核对（2026-09-20）

第四节已经把该改的三处写清楚了，本轮按它执行。另补四条：

| # | 结论 | 证据 |
| --- | --- | --- |
| 32 | 译文里照引的两条拒绝文案与代码**逐字一致**，所以它们该保持英文：`/theme isn't available in minimal mode (minimal renders with your terminal's own palette). Run /fullscreen to switch this session.` 与 `/expand isn't available in fullscreen mode: press Tab to focus the scrollback, then → on the block.` | `slash/commands/theme.rs:27-29`（`Remedy::SwitchMode.why`）、`expand.rs:21-23`（`Remedy::UseInstead`）；拼装处 `slash/mode_support.rs:43-51` |
| 33 | `/login` `/logout` 的**源码描述文案已经是中文**，与 §五 的定性一致，可照译：前者「Chaos 不支持账号登录；请使用 /provider 配置 API Key」且 `run()` 只发 `Action::OpenProviderModal`；后者「Chaos 无需退出登录；请修改 config.toml 中的 Provider 配置」，`run()` 只回一条消息 | `slash/commands/login.rs:14`、`:19-21`；`logout.rs:14`、`:17-22` |
| 34 | `/usage` 的可用性还有一道闸：`UsageCommand::visible()` 走 `ctx.usage_command_visible`，配置了外部认证提供方（`auth_provider_command`）的安装里这条命令**被隐藏并拒绝**。译文按此加了行为注记 | `slash/commands/usage.rs:8-9`、`:31`、`:46-51` |
| 35 | `/privacy` 在本分叉**保留**，且译文提到的 ZDR 与 `· Admin Managed` 两行真实存在（不是上游专有） | `app/dispatch/status.rs:212`、`:235-237` |

第 32 条值得记一笔：它是「照 §一 保留字面量」和「译文要写成中文」两条规则的
交界处，判据是**这句引文是不是运行时会原样打印的字符串**。是，就保留英文。

顺带清掉的上游旧名 6 处：`~/.grok/last-copy.txt`、`.grok/workflows/`、
`~/.grok/workflows/`（两处）、`~/.grok/skills/commit/SKILL.md`
（均按 §4.1 改为 `.chaos`），以及 `grok usage <session-id>` → `chaos usage`。

本轮的副作用：`### /minimal and /fullscreen` 改中文标题后，章首那条
`](#minimal-and-fullscreen)` 变成死锚点。这是 §二 预期的，留给收尾的
`--fix-anchors` 机械重写，不手工改。

---

## 十、第 17 章《会话管理》的核对（2026-09-20）

| # | 结论 | 证据 |
| --- | --- | --- |
| 36 | `/session-info` 的认证行是**源码里活着的上游遗留**：API key 会话会原样打印 ``  Run `grok login` to use your SuperGrok subscription instead.``，所以译文把 `grok login` 当引文保留，并在同句注明那是上游屏幕文本、凭据改用 `/provider` | `xai-grok-pager/src/app/effects/mod.rs:4984-4996`（`format_auth_lines`）；两个分支都有测试钉住：`app/effects/tests.rs:2573-2620` |
| 37 | `/session-info` 的字段与顺序（Title、Shell version、Session ID、Conversation ID（有则）、Working directory、Model、Model Hash（`show_model_fingerprint` 时）、API Backend、Sandbox、Turn、Context）与译文一致；认证行是在 Shell version 之后另外拼进去的散文，不是字段 | `effects/mod.rs:4900-4956`、`:4966-4983` |
| 38 | 按标题恢复的规则全部为真：忽略大小写（`trim().to_lowercase()`）、UUID 形状的值直接走 ID、重复标题里唯一被手动改过名的那个胜出、其余重复项报错并列出各自的 ID | `app/session_title_resolve.rs:18-20`、`:38-78` |
| 39 | `-s`/`--session-id` 只用于**新建**，且与 `-r`/`-c` 同用必须带 `--fork-session`；`--fork-session` 确实存在 | `app/cli.rs:600-605` |
| 40 | `chaos sessions list/search`、`chaos du`（可见别名 `disk-usage`）、`chaos worktree gc/rm/show/db rebuild` 全部存在；`sessions list` 按 worktree 标签分组，每行是 ID、创建、更新、来源与摘要（截断 50 字） | `sessions_cmd.rs:13-26`、`:194-238`；`app/cli.rs:143-144`；`worktree_cmd/mod.rs:18-70` |
| 41 | `costUsdTicks` 是 1e10 ticks = 1 美元 | `xai-grok-sampling-types/src/types.rs:545`；`xai-chat-state/src/usage.rs:44` |

### 10.1 保留未改的两处

- 散文数字 `30`（reflog 名字只保留 30 天）按 §五 原样保留，没有可声明漂移的通道。
- `refs/grok/reclaimed/<worktree>/<commit>` 与 `git log refs/grok/reclaimed/`
  **保持上游命名**：代码里这个 ref 名字空间就叫这个
  （`xai-fast-worktree/src/nfs/remove.rs:294`），`FORK_NAME` 的前后视也把它
  排除在外，不算残留。

### 10.2 本轮有意新增的行内字面量

`/provider`（第 36 条那句要指向本分叉真正的凭据入口）、`CHAOS_HOME` 两处与
`~/.chaos` 两处（配置根那段的双读兼容说明，原来的英文段没有点名环境变量）。
因此收尾清单第 6 条**不能**用 `--strict-spans`：那个开关会把**有意补的**
字面量也算成漂移，而补字面量正是本轮允许的动作。已按此改写第 6 条。

### 10.3 顺带发现的代码侧文案缺口（本轮不改，仅记录）

`title_miss_hint()` 是用户会看到的英文提示，而且点名了
`grok sessions search`（本分叉的二进制叫 `chaos`）：
`app/session_title_resolve.rs:25-30`。同一批代码里
`正在恢复会话 {}（按标题匹配）` 已经中文化，说明这是漏网的一条。
钉住它的测试有 `app/session_startup.rs:1882` 与
`app/session_title_resolve_tests.rs:113`。

### 10.4 死锚点计数

本章译完后全库 `--links` 是 16 条死锚点，其中两条与本章有关：入站
`04-slash-commands.md#the-grok-usage-subcommand`（本章 `grok usage` 一节改了
中文标题）、出站 `17-sessions.md` → `15-agent-mode.md#session-config-options`。
都属 §二 预期，留给收尾的 `--fix-anchors` 机械重写，不手工改。

---

## 十一、第 9 章《插件》的核对（2026-09-20）

| # | 结论 | 证据 |
| --- | --- | --- |
| 42 | 插件市场索引的协议目录确实叫 `.grok-plugin/`（再退到 `.grok-plugin/plugin.json`，然后是 `.claude-plugin/` 的等价形式），**不是** `.chaos-plugin/`；译文三处照旧 | `xai-grok-plugin-marketplace/src/index.rs:196-204`、`catalog.rs:63` |
| 43 | `chaos plugin` 的子命令与译文列出的选项逐一对应：List（`--json`，`--available` 要求 json）、Install（`--trust`）、Uninstall（别名 `rm`/`remove`，`--confirm`/`--keep-data`）、Update、Enable、Disable、Details、Validate、Tag（`--push`、`-f`、`--dry-run`）、Marketplace | `xai-grok-pager/src/plugin_cmd.rs:84-155` |
| 44 | 扩展模态的标签栏顺序与文字是 Hooks、插件、市场、Skills、工作流、MCP 服务器；其中 `Hooks`/`Skills` 是界面上的英文品牌词，所以译文里只有另外四个写中文 | `xai-grok-pager/src/views/extensions_modal.rs:731-747`、`:750-762` |
| 45 | 插件组件的位置与文件名全部属实：`skills/`、`commands/`、`agents/`、`hooks/hooks.json`、`.mcp.json`、`.lsp.json`，以及可选的 `plugin.json` 清单；惯例式插件就是按这些路径认出来的 | `xai-grok-agent/src/plugins/manifest.rs:171-212`、`discovery.rs:613-628` |
| 46 | 插件发现的作用域与优先级（`--plugin-dir` 0 → 项目 1 → 用户 2 → `[plugins].paths` 3）、项目与用户目录、以及「`.claude/plugins/` 等价形式也可用」都属实。译文按本分叉把 `~/.grok/plugins/`、`.grok/plugins/` 写成 `.chaos/...`：`grok_home()` 返回的就是 chaos home | `xai-grok-agent/src/plugins/discovery.rs:27-36`、`:208-226`；`xai-dirs/src/lib.rs:120-138` |
| 47 | `_meta.pluginDirs` 是 `session/new` / `session/load` 上真实的 meta 键；`--plugin-dir` 可重复，且在 leader 模式下被忽略并打一条警告——两处行为都对得上译文 | `xai-grok-shell/src/agent/mvp_agent/mod.rs:369`；`xai-grok-pager/src/app/cli.rs:302`；`xai-grok-pager-bin/src/main.rs:1303-1305`、`:1391-1393` |
| 48 | 钩子拿到的两个插件变量（`GROK_PLUGIN_ROOT`、`GROK_PLUGIN_DATA`）与两个 `CLAUDE_*` 别名属实，技能侧用的是同一对变量 | `xai-grok-tools/src/implementations/skills/skill.rs:250-257` |
| 49 | 插件代理用 `plugin:agent` 限定名；未受信任的插件只解析 frontmatter，正文不进模型——与译文「未受信任的插件代理只保留 frontmatter」一致 | `xai-grok-agent/src/discovery.rs:438`、`:519-527` |
| 50 | 「`require_sha` 两条开关都只能收紧、关不掉」属实：`[marketplace] require_sha` 与 `GROK_MARKETPLACE_REQUIRE_SHA` 是或的关系 | `xai-grok-plugin-marketplace/src/config.rs:32-44` |
| 51 | `plugin_auto_update` / `pluginAutoUpdate` 是只收紧的 `PolicyPin`；Claude 每个市场级的 `autoUpdate: false` 会把**全局**会话启动自动更新钉死，代码注释说明了为什么不做细粒度等价物——与译文那句括注一致 | `xai-grok-workspace/src/permission/managed_policy/mod.rs:58-59`、`:233-236`、`:250-258` |
| 52 | MCP 策略条目只认 `serverUrl`/`server_url`、`command`、`serverCommand`/`server_command`、`serverName`/`server_name` 四种（解析处的告警文案就是这么写的）；名字比较是「空格转 `_` 后小写、剥掉 `grok_com_` 前缀」，前缀常量也在 | `managed_policy/parse.rs:202-243`；`managed_policy/mcp.rs:245-291` |
| 53 | 译文照引的两条运行时日志与源码逐字一致：`MCP server blocked by managed settings policy`、`Marketplace source blocked by allowlist` | `xai-grok-shell/src/session/managed_mcp.rs:211`；`xai-grok-shell/src/plugin.rs:746` |
| 54 | `**Enforced by policy**` 是 `chaos inspect` 真正打印的小标题，所以保留英文并只用括号补一句中文注解 | `xai-grok-shell/src/inspect/mod.rs:1472` |

### 11.1 ⚠️ 文档超前于代码的三处说法（来自上游 a28ee2b2 的增量，本轮只译不改）

这三处都是 `ca7e2f1f`（「补入纯上游章节增量」）有意搬进来的上游 tip 文案，而本
分支的代码是 `SOURCE_REV 72a61251` + 精选移植，所以它们描述的是**上游更新**的
行为。本轮按「文档照译、差异记录在案」处理，没有改写成「本分支实际行为」——
要改的话应先决定是移植代码还是改文档，那是另一张工单。

1. **「配置错误是锁死而不是放行」那一段不成立。** 它的理由是
   `locked down by policy (<file>)`，而这个串在本修订版根本不存在：
   `McpBlockReason` 只有三种 Display——`matches deniedMcpServers (<file>)`、
   `not in allowedMcpServers (<file>)`、
   `project MCP disabled (enableAllProjectMcpServers = false, <file>)`
   （`managed_policy/verdict.rs:44-60`）。而且类型写错的策略键是被**忽略**而不是
   锁死：`policy_array` 打一条 `policy key must be an array of entries; the whole
   list is ignored` 的告警后返回空（`parse.rs:177-189`），allowlist 只在条目非空
   时才登记（`mod.rs:203-221`）。
2. **「`strict_known_marketplaces` 键存在即限制、空数组等于全面锁死」也不成立。**
   `MarketplaceAllowlist::is_restricted()` 就是 `!allowed_urls.is_empty()`
   （`marketplace.rs:19-21`），空数组不产生任何限制；这一条还有测试正面钉住：
   `tests.rs:1956-1975` 的用例名是「wrong-typed policy lists do not drop sibling
   keys」，断言 `Expect::MarketRestricted(false)`。也就是说本修订版是 fail-open，
   与文档写反了。
3. **更广的一层：本章第 16–19 节描述的「原生 TOML 策略层」当前没有被运行时消费。**
   所有 MCP / 插件市场策略的运行时入口都走
   `resolution::managed_settings()`，而它是 `managed_policy/compat.rs` 的兼容
   视图：只读 Claude 的 `managed-settings.json`，而且用的是旧解析器（只认
   `serverUrl`/`command`/`serverName`，不认 `serverCommand`，见 `compat.rs:196-224`）。
   engine 从 `requirements.toml` / `managed_config.toml` 解析出来的策略层没有
   任何调用方，`plugin_auto_update` 这个 pin 更是全库无人读取。`compat.rs:134-137`
   的注释自己写明了这一点：「The engine's TOML policy layers are invisible here —
   multi-source enforcement lands with the migrated callers in the stacked PR」。

三处的处置建议相同：要么把 engine 的调用方按上游那张 stacked PR 移植过来
（推荐，属上游已完成的迁移），要么在文档里补一句行为注记。**不要**在翻译轮里
顺手改写，否则文档与上游差分表就再也对不上了。

### 11.2 顺带发现的两处代码侧英文残留（已记入 t1-7x）

- `xai-grok-pager/src/app/cli.rs:334`、`:340` 打印 `grok: --plugin-dir …`，而
  二进制叫 `chaos`。
- `title_miss_hint()`（§10.3 已记）是同一类。

### 11.3 死锚点

本章译完后全库 `--links` 从 16 条升到 28 条，新增的 12 条全部指向
`09-plugins.md#…`（来自第 7、8 章与本章自身），属 §二 预期，留给收尾的
`--fix-anchors` 机械重写。

### 11.4 一个工具坑：`--after HEAD` 读的是提交后的版本

`--after HEAD` 比较的是**已提交**的 HEAD，不含工作树。逐章自查要用
`--before HEAD --after WORKTREE`（或 `--after .`），否则会把这一章刚做的改动
整个漏掉，却把这一章在更早提交里已有的差异当成「本轮漂移」报出来——

本章就踩过这个坑：`--before main --after HEAD` 报出的 5 条结构差异（围栏
16→17、丢 `*://…` 与 `/*` 两个行内跨度、多一张 2×4 表格、多一条
`#restrict-which-mcp-servers-can-run` 链接、多一个 H3）全部来自 `ca7e2f1f`
搬进来的上游增量，与本次翻译无关；换成 `--before HEAD --after WORKTREE` 后是
0 漂移。这也说明收尾时第 6 条不能用 `--after HEAD` 当「本章无漂移」的判据，
而要按「继承差异逐条登记」处理。

---

## 十二、第 16 章《子代理与人设》的核对（2026-09-20）

| # | 结论 | 证据 |
| --- | --- | --- |
| 55 | 内置代理类型就是 `general-purpose`、`explore`、`plan` 三种；并且**本分叉没有改掉内置代理名 `grok-build`**（它仍是真实的内置定义名），所以译文表格里的 `grok-build` 照旧保留 | `xai-grok-agent/src/discovery.rs:294`、`:802-804` |
| 56 | 界面文字：代理模态框的标签页是 **代理** 与 **人设**（不是 Agents/Personas），命令面板条目是 **管理 Agent**，任务面板分组名是 **子代理**。这两个界面串只对得上「人设 = persona、角色 = role」这一种读法（`/personas` 命令的说明偏偏写的是「管理角色」），再加上 `[subagents.personas]` 与 `[subagents.roles]` 是两层配置，本章译文因此定 **persona = 人设、role = 角色**。**代价**：第 4 章已提交的三处（`04-slash-commands.md:25`、`:388` 标题「代理与角色」、`:398`）用「角色」翻 persona，与本章不一致 | `xai-grok-pager/src/views/agents_modal.rs:37-38`；`views/modal.rs:531`；`views/tasks_pane.rs:188`；`slash/commands/personas.rs:14`；`xai-grok-shell/src/config/mod.rs:53-86`（`[subagents.toggle]`/`[subagents.models]`/`[subagents.roles.*]`/`[subagents.personas.*]` 四层） |
| 57 | 滚动回溯里的子代理生命周期块逐字为 `子代理运行中：“…”`（阻塞）/`子代理已启动：“…”`（后台）/`子代理已完成（用时 43s）：“…”`/`子代理失败（用时 43s）：“…”`/`子代理已取消（用时 43s）：“…”`；描述用中文弯引号，`(persona · role · model)` 是半角括号加前导空格 | `scrollback/blocks/subagent.rs:183-255`、`:159-166`；`app/subagent.rs:875-887` |
| 58 | 活动后缀是中文：`思考中`、`回复中`、`运行: cargo test`、`运行工具`、`压缩中`、`重试中 (2/3)`（半角括号加空格）。译文里的例子按这些写 | `app/subagent.rs:891-931` |
| 59 | 仍是英文、译文要保留英文的界面串：`resumed`、`forked` 徽章、`[Dashboard]`、`[‹]`、`[›]`、`Subagent ID: `、`Message:` | `app/subagent.rs:816-823`；`views/dashboard/render.rs:3892`；`scrollback/blocks/tool/sent_message.rs:222`、`:243` |
| 60 | 嵌套深度上限确实是 1（`MAX_SUBAGENT_DEPTH = 1`），超限报 `Subagent depth limit exceeded (current depth: …, max: …)` | `xai-grok-tools/src/implementations/grok_build/task/mod.rs:39-46`、`:430-435` |
| 61 | `send_subagent_message` 默认关闭，由 `[features] active_agent_messages`（环境变量 `GROK_ACTIVE_AGENT_MESSAGES`）打开，默认值是 `false` | `xai-grok-config-types/src/registry.rs:221-225`；`xai-grok-agent/src/builder.rs:96`、`:230` |
| 62 | 能力模式的四个取值 `read-only` / `read-write` / `execute` / `all` 属实（另有 `readonly`、`read_only`、`ReadOnly` 等别名） | `common/xai-tool-types/src/task.rs:166-185` |
| 63 | agent frontmatter 的 `mcpInheritance` 属实：省略即 `All`，映射只允许一个键，四态 `all` / `none` / `named` / `except` 各有解析测试 | `xai-grok-agent/src/config.rs:872`、`:1040`、`:1570`、`:2727-2746` |
| 64 | 项目级路径写 `.chaos/…` 是本分叉的正确写法：项目配置根按「已存在的 `.chaos` → 已存在的 `.grok` → 默认 `.chaos`」解析，合并时两者都读 | `xai-grok-config/src/paths.rs:60-82` |

### 12.1 ⚠️ 文档超前于代码的两处说法（本轮只译不改）

1. **「一行 `Message` 行」那一段描述的渲染方式在本修订版不存在。** 译文照译的那段说
   每次发送在会话记录里显示为一行 `Message` 行（动词 + 标签 + 弯引号里的描述，
   截到第一行且最多 40 字符），示例是 `Message sent to Explore “find callers”` 等五条。
   实际实现里 `send_subagent_message` 的折叠行就是块标题，四个取值仍是**英文**：
   `Sending message to subagent`、`Sent message to subagent`、
   `Failed to send message to subagent`、`Message delivery unconfirmed`；全仓库搜不到
   `Message sent to …` / `Message queued for …` / `Message interjected to …` 这些串，
   也没有「40 字符」截断（描述是按可用宽度截的）。
   所以译文保留那五条英文示例——它们本来就是界面串，属「该英文的地方就英文」；
   差异记在这里：这是「文档对不上本分支实现」，不是翻译问题。
2. **「子发送方有配额：每个发送方-目标对 4 条在途、每次发送尝试 32 条外发」的数字对不上。**
   代码里是每子代理 `MAX_ACTIVE_MESSAGE_ADMISSIONS_PER_CHILD = 8`、
   每协调器 `MAX_ACTIVE_MESSAGE_ADMISSIONS = 64`，另有
   `MAX_ACTIVE_AGENT_MESSAGE_BYTES = 32 * 1024`（32 KiB 的**消息体积**上限，
   很可能是「32」这个数字的来源）。没有 per-(sender, target) 的 4 条这种常量。
   因为 `--numbers` 不变式禁止翻译轮改动散文里的数字（`32` 会被 `--numbers` 抓到），
   本轮原文照译，差异记在这里，留给收尾报告决定改代码还是改文档。

### 12.2 ⚠️ 项目作用域的 `.chaos/…` 当前不被读取（本分叉的漏网点）

译文按本分叉的写法把所有路径写成 `.chaos/…`（`RENAMES` 会把两边的 `.grok` 归一成
`.chaos`，所以门禁看不见这个差异）。但本修订版里，**项目作用域**的四个发现点仍是写死的
`.grok`，项目里建 `.chaos/agents/`、`.chaos/roles/`、`.chaos/personas/` 不会被读：

| 发现点 | 项目作用域实际路径 | 用户作用域 |
| --- | --- | --- |
| 代理定义 | `PROJECT_AGENT_SUBDIRS = [".grok/agents", ".claude/agents"]` | 走 `user_grok_home()`，即 `~/.chaos/agents` ✅ |
| 子代理角色 | `cwd.join(".grok").join("roles")`（写死） | `user_grok_root.join("roles")`，即 `~/.chaos/roles` ✅ |
| 子代理人设 | `cwd.join(".grok").join("personas")`（写死） | `user_grok_root.join("personas")`，即 `~/.chaos/personas` ✅ |
| 技能（agent 侧） | 优先级注释仍写 `.grok/skills`/`.agents/skills`/`.claude/skills` | 同左，走 grok_home |

证据：`xai-grok-agent/src/discovery.rs:16`、`xai-grok-shell/src/config/mod.rs:197`、
`:238`（对 `:403-405`）。

对照本分叉**已经**支持双名的其它项目级发现点——它们都是「两个都列，`.grok` 在前」：
`xai-grok-sandbox/src/profiles.rs:131`、`:145` 的 `[".grok", ".chaos"]`，
`xai-grok-tools/src/reminders/skill_discovery.rs:15` 的
`[".grok", ".chaos", ".agents", ".claude", ".cursor"]`，
`xai-grok-tools/src/implementations/skills/discovery.rs:855` 的
`[".grok", ".chaos", ".agents"]`。按这个已成型的写法，上表前三行属于**漏改的点**，
而不是有意的设计。

顺带记一个更干净的证据：`xai-grok-config/src/paths.rs` 里本分叉新加的
`project_config_toml_candidates`、`resolve_project_config_dir`、
`existing_project_config_dirs` 三个函数**全仓库没有任何调用点**（只有 `lib.rs:75-77`
的 re-export），即「按 `.chaos` 优先解析项目配置根」这条规则写好了但没接上去。

本轮不改代码：改这些发现点是**行为变更**（新目录开始被读），超出「中文化 + 精选上游
修复」的范围，且需要各自的测试。留给收尾报告让用户决定：补上 `.chaos`，还是把文档
改回 `.grok`。

### 12.3 一个工具坑：标题数变过的文件会被 `--fix-anchors` 静默跳过

收尾第 1 步打算用 `--fix-anchors --before <译前基线>` 机械重写锚点，它的做法是**按标题
序号**把旧 slug 映射到新 slug，因此要求「译前基线」与当前文件的标题**数量相同**，数量
不同就打印 `heading count N -> M, skipped` 并跳过整个文件。

对 `main`（译前基线）比一遍，有 5 个文件数量不同，全部是 `ca7e2f1f` 搬进来的上游增量
加了标题：

| 文件 | `main` 标题数 | 工作树标题数 |
| --- | ---: | ---: |
| `07-mcp-servers.md` | 23 | 24 |
| `09-plugins.md` | 29 | 30 |
| `13-memory.md` | 46 | 47 |
| `16-subagents.md` | 26 | 27 |
| `21-terminal-support.md` | 21 | 22 |

（比法：把 `check-doc-l10n.py` 当模块导入，对每个文件跑
`heading_list(git show main:<path>)` 与 `heading_list(<工作树>)` 比长度；第 16 章多出来
的那个标题就是 `ca7e2f1f` 加的 `### Docked bar (when enabled)`。）

所以收尾不能对全库跑一次 `--fix-anchors --before main` 就以为完事：这 5 个文件会被跳过，
指向它们的入站锚点仍是英文（`03`、`07`、`09` 各有一条指向第 16 章的
`#fullscreen-framed-view-the-child-transcript` 与 `#mcp-inheritance`）。这 5 个要**逐文件**
用「该文件译前的那次提交」当基线，或者手工改掉——数量不大，`--links` 全库本轮从 28 条
（`d6d4508c`）升到 32 条，新增的 4 条都是第 16 章改标题造成的：本章自己一条
`#sending-messages-to-subagents`，加上 `03`、`07`、`09` 指向本章的三条入站锚点。

再记一条相关的坑：**不要在章节提交里顺手改锚点**。实测把第 16 章自己那条
`#sending-messages-to-subagents` 改成 `#给子代理发消息` 之后，`--before HEAD --after
WORKTREE` 立刻从 0 漂移变成 1 条：

```text
- link targets changed: lost ['#sending-messages-to-subagents'], added ['#给子代理发消息']
```

因为 `links` 不变式比的是 `](target)` 的**集合**，改锚点同时算「丢失」和「新增」，
既不是 `note:` 也没有声明通道（`scripts/doc-span-removals.tsv` 只管行内字面量）。
所以锚点一律留到收尾的 `--fix-anchors` 那一次改，逐章提交保持 0 漂移——前 15 章都是
这么留下来的（第 16 章自己那条也就成了这 32 条中的一条，试改后已回退）。全库重写那一步
会**整体**改动 `links` 集合，那批差异要在收尾报告里按「有意改锚点」逐条说明，不能当成
漂移。
