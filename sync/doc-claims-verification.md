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
