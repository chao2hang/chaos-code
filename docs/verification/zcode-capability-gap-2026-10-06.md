# ZCode 能力取证与 Chaos Web 端差距（2026-10-06）

用户实测本机跑起来的 Chaos Web 端之后给出的判断是「web 端和 zcode 项目的差异还是较大」。这份文件把这句话变成可核对的清单：ZCode 侧每条能力都指出它来自哪个文件的哪段内容，Chaos 侧每条现状都指出源码位置，两边对不上的地方逐条写清归属里程碑。`TODO.md` 第 2.2 节与各处新增开放项引用本文件。

## 1. 取证范围与取证不到的部分

本机 ZCode 安装目录只有一个前缀：

    /home/chaos/.zcode/
    ├── cli/      29 MB   CLI 运行痕迹：exec 快照、jsonl 日志、plugin 缓存、db/db.sqlite
    ├── server/  358 MB   server/zcode-server.cjs（9.6 MB 打包产物）、server/node、asset-cache/、tools/
    └── v2/       1.2 MB   tasks-index.sqlite、v2/certs/

取证不到的是 **ZCode 的客户端本体**。本机没有 ZCode 的可执行程序，没有桌面或 Web 客户端的 UI bundle，没有正在监听的 ZCode 进程；`server/zcode-server.cjs` 里 `BrowserWindow`、`webContents`、`ipcMain`、`index.html` 的命中数都是 0。因此本文件不描述 ZCode 界面长什么样，也不声称任何像素级或交互级等价；所有 ZCode 能力都是从它自己的**数据合同**（两张 SQLite 库的真实 DDL）与**协议词表**（打包产物里的方法名与事件名）推出来的 —— 一个产品愿意为某张表建索引、愿意为某个事件命名，说明那条能力是它真实交付的功能，而不是待办。

此前 `TODO.md` 第 2 章矩阵里「Whiteboard、Treemapping、轨迹 | Deferred | M5 后」这一行需要更正依据。`zcode-server.cjs` 里 `whiteboard` 命中 28 次，看着像 ZCode 有白板功能，逐条看上下文后是否定的：那 28 次全部来自随包打入的飞书开放接口 SDK，注释里写的是 `https://open.feishu.cn/api-explorer?project=board&resource=whiteboard.node&apiName=create_plantuml`，是「在飞书画板里创建节点」的第三方连接器代码，不是 ZCode 自己的界面能力。`treemap`、`treeMap` 命中 0 次。结论：白板与 Treemapping 在 ZCode 侧**没有证据**，矩阵那一行保留 Deferred 判定但依据要换掉。同一份谨慎也用在别处：`electron` 命中 14 次而同族窗口 API 全部为 0，因此不能据此说 ZCode 桌面端是 Electron。

## 2. ZCode 侧证据

### 2.1 会话索引 `v2/tasks-index.sqlite`

ZCode 把「会话」当作一等对象 `tasks`，并按工作区建索引。真实 DDL（截掉与本节无关的列注释）：

    CREATE TABLE tasks (
      workspace_key TEXT NOT NULL, workspace_path TEXT NOT NULL, workspace_identity TEXT,
      task_id TEXT NOT NULL, title TEXT NOT NULL DEFAULT '', task_status TEXT,
      provider TEXT, mode TEXT NOT NULL DEFAULT 'build', model TEXT,
      migration_source TEXT, forked_from_task_id TEXT,
      created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
      unread_at INTEGER, last_unread_at INTEGER NOT NULL DEFAULT 0,
      pinned INTEGER NOT NULL DEFAULT 0, archived INTEGER NOT NULL DEFAULT 0,
      deleted INTEGER NOT NULL DEFAULT 0, title_overridden INTEGER NOT NULL DEFAULT 0,
      meta_json TEXT NOT NULL DEFAULT '{}', searchable_text TEXT NOT NULL DEFAULT '',
      cron_automation_id TEXT, off_peak_task_id TEXT,
      PRIMARY KEY (workspace_key, task_id));

配套索引 `idx_tasks_workspace_pinned_updated`、`idx_tasks_workspace_archived_updated`（都带 `WHERE deleted = 0`）说明置顶、归档、软删除是列表页每次都要走的查询路径，不是备字段。

分组是独立的三张表：`task_groups(group_id, title, color, ...)`、`task_group_members(group_id, workspace_key, task_id, sort_order, ...)`、`task_group_view_node_orders(node_type, node_key, sort_order)`，另有 `task_group_workspace_bootstraps` 记录某个工作区首次进组。也就是：**带颜色、可跨工作区、可自定义排序的会话分组**。

打包产物里对 `cronAutomationId` 与 `offPeakTaskId` 的注释是中文原文，写明了单一来源加索引投影的持久化策略：

    // cron automation 身份：随 meta_json 一起持久化（单一来源），同时在写入时投影到 tasks 表
    // cron_automation_id 索引列，供按 automation 反查 session。runId 属于 automation_runs /
    // 投递 metadata，不属于 task 表。

### 2.2 定时自动化与闲时任务

`automations` 表是一张完整的调度器表，不只是「以后想做」：`automation_id`、`cron_expr`、`prompt`、`model`、`provider`、`mode`、`thought_level`、`workspace_*`、`target_task_id`、`bot_delivery_target`、`location_kind`（默认 `local`）、`recurring`、`max_runs`、`end_at`、`schedule_rule`、`schedule_edited_by_user`、`run_count`、`enabled`、`lifecycle_status`、`next_run_at`、`last_run_at`、`running`、`claimed_at`、`dispatch_status`、`dispatch_attempts`、`retry_at`、`last_error`，配 `idx_automations_due(enabled, next_run_at)`、`idx_automations_retry(enabled, retry_at)`、`idx_automations_workspace(workspace_key)` 三条索引。执行历史另有一张 `automation_runs(run_id, automation_id, workspace_key, scheduled_at, trigger DEFAULT 'schedule', dispatch_status DEFAULT 'claimed', outcome, session_id, error, attempts, ...)`。

闲时任务另开一条链：`off_peak_tasks` 表带 `server_ticket_id`、`prompt`、`permission_mode`、`thought_level`、`status`、`queued_at`、`queue_position`、`next_poll_at`、`claim_running`、`schedulable`、`attempt_count`、`files_changed`、`settled_at`，服务端另有 `/api/v1/off-peak/ticket`、`/api/v1/off-peak/ticket/availability`、`/api/v1/off-peak/ticket/status`、`/api/v1/off-peak/anthropic/v1/messages` 四个端点。

### 2.3 CLI 库 `cli/db/db.sqlite`

21 张表，其中与「客户端要展示什么」直接相关的形状如下。

`session` 表除了标题还带**标题来源**与**改动统计**：`title_source` 用 CHECK 约束限定为 `default`/`first_input`/`generated`/`custom`，另有 `title_message_id`、`time_title_updated`、`slug`、`version`、`share_url`、`summary_additions`、`summary_deletions`、`summary_files`、`summary_diffs`、`revert`、`permission`、`time_compacting`、`time_archived`、`trace_id`。

`session_input` 是**中途插话**的合同：`delivery` 用 CHECK 限定为 `startNow`/`guide`/`queue`，配 `admitted_sequence`、`promoted_sequence`、`promoted_message_id`、`status` ∈ `admitted`/`promoted`/`cancelled`/`discarded`/`failed`。

`session_target` 是**带预算的长目标任务**：`objective`、`status` ∈ `active`/`paused`/`budget_limited`/`complete`、`token_budget`、`tokens_used`、`time_used_seconds`、`active_input_id`、`active_run_started_at`、`active_run_last_seen_at`。

`workflow_run`、`workflow_definition`、`workflow_event`、`workflow_activity` 四张表加 `session_task_link` 构成**子代理树**：`session_task_link` 有 `role`、`depth`、`path`、`phase`、`label`、`agent_type`、`model`、`status` 以及 `root_workflow_run_id`/`parent_link_id` 两条自引用；`workflow_activity.status` ∈ `queued`/`running`/`completed`/`failed`/`skipped`/`cancelled`/`cached`/`lost`，`workflow_run` 带 `budget_total`、`budget_spent`、`stats_json`、`failure_json`。

`todo(session_id, content, status, priority, position, ...)` 是计划清单的持久化；`input_history(id, project_id, session_id, text, kind, attachments)` 是**跨会话、按项目分组的输入历史且带附件**；`local_setting(scope, scope_id, namespace, key, value, schema_version)` 是**按作用域命名空间分的设置**；`permission(project_id, data)` 是**按项目持久化的授权**。用量核算独立三张表：`model_usage`、`turn_usage`、`tool_usage`，索引到 `session_turn`、`started_model`、`trace` 三个维度。

### 2.4 协议词表

从 `zcode-server.cjs` 里按 `"a.b"` 形状提出并按出现次数排序，能读出的方法/事件名（次数为打包产物内命中数，只用于说明它是主干还是边角）：

    session.created/resumed/closed/updated/upserted/titleUpdated/removed   session.event
    turn.started / turn.completed / turn.failed / turn.terminal
    turn.steerQueued / turn.steerDrained                                    中途插话的两端
    part.delta / part.started / part.upserted / part.removed
    message.upserted / message.removed / reply.list / reply.set
    model.list / model.set / model.streaming
    mode.list / mode.set / thoughtLevel.list / thoughtLevel.set
    workspace.list / workspace.set / task.list / task.set
    permission.request / permission.requested / permission.respond / permission.resolved
    userInput.request / userInput.requested / userInput.response / userInput.resolved
    elicitation.respond / elicitation.submit
    streamRecovery.updated                                                  流断裂后的恢复
    subagent.lifecycle / subagent_message / subagentProjectionStateSchema
    checkpoint.created / checkpointId / checkpointRootDir / rewind.triggered
    usage.delta / state.updated / tool.updated / tool.lifecycle
    mcp.servers / marketplace / worktreeRoot

`thoughtLevel` 命中 283 次、`pty` 985 次、`automation` 407 次、`offPeak` 174 次、`marketplace` 127 次、`terminal` 122 次、`subagent` 66 次、`worktree` 22 次、`checkpoint` 22 次。

## 3. Chaos Web 端实测现状

同一个仓库、同一台机器上的实测（`apps/chaos-ui` 全部非测试源码合计 3,196 行，其中 `src/main.tsx` 1,753 行，没有组件目录）。

| 能力 | Chaos Web 端现状（源码位置） |
|---|---|
| 会话列表 | 不存在。`ClientMessage` 只有 `create_session`/`resume`/`snapshot`/`list_workspaces`/`switch_workspace`/`archive_workspace`，没有 `list_sessions`、重命名、删除、fork（`apps/chaos-ui/src/generated/protocol.ts:6-44`）。侧栏列的是工作区不是会话（`apps/chaos-ui/src/main.tsx:743-782`）。`create_session` 直接覆写 `last_session_id`（`crates/codegen/chaos-engine/src/lib.rs:2371`），因此浏览器重载后旧转录本被孤立。 |
| 会话标题/置顶/归档/未读/分组/搜索 | 全部不存在。协议与 UI 里没有标题生成、`pinned`、`unread`、分组、可搜索文本这些概念。归档只有工作区级（`main.tsx:373-379`）。 |
| 真流式与服务端推送 | `Engine::handle` 是同步函数，返回**整段**回复后 socket 才写第一个字节（`chaos-engine/src/lib.rs:2254`、`xai-grok-web/src/lib.rs:742-762`）；`Engine::subscribe` 已定义但 Web 宿主从未调用（`chaos-engine/src/lib.rs:2216`）。前端的逐字追加是在切分一个已经算完的字符串，因此没有 `part.delta` 语义，也没有断流恢复。 |
| 中途插话 | 不存在。协议只有 `submit`，运行中再发一条被 busy 状态挡住（`main.tsx:1129`）。`turn.steerQueued`/`guide`/`queue` 这些语义在 engine 里没有对应物。 |
| 取消 | 有按钮，但没有可中断的东西：`cancel` 返回 `cancelled` 而不打断已完成的 prompt（`chaos-engine/src/lib.rs:2263` 起的分支），UI 随后把已完成的一轮标成「本轮已取消」。 |
| 模型/模式/思考强度 | 三个写死的胶囊：`通用模式`（`main.tsx:1118`）、`session.settings?.model \|\| 'gpt-4o'`（`main.tsx:1122`）、`0 tokens`（`main.tsx:1124`）。协议里没有 `model.list`/`mode.list`/`thoughtLevel.list`；设置面板保存的 model 与 Base URL 不参与决定哪个模型应答（`PromptAdapter` 在启动时按环境变量装配，`xai-grok-web/src/main.rs:14-53`）。 |
| 用量与费用 | `usage` 只在未装配的通用工具 adapter 分支上产生，且数字是编的：`input_tokens: pending.summary.len()`（`chaos-engine/src/lib.rs:3580`）。没有 `model_usage`/`turn_usage`/`tool_usage` 这类按轮、按工具的核算。 |
| 工具卡片 | 只有一张通用卡：工具名 + 状态词 + 截断的 progress/result（`main.tsx:998-1005`）。发布出的 `chaos-web` 没有装配任何工具 adapter，真实审批一律回 `tool_unavailable`／「没有配置获准的工具 adapter」（`chaos-engine/src/lib.rs:3598-3599`）。浏览器里唯一那条工具卡测试喂的是伪造 socket 帧。 |
| 推理/思考展示 | 不存在，`apps/chaos-ui/src` 里 `reasoning`/`thinking` 命中 0；协议里也没有推理事件（原先界面上那块「🧠 思考过程」是写死的装饰，2026-10-03 已连同样式一起删除）。 |
| 消息级操作 | 复制、重试、改后再发、重新生成全都不存在，`main.tsx` 里没有对应处理器。 |
| 授权记忆 | 没有。`approve` 只带 `request_id`（`protocol.ts:16`），每次写盘、每条命令、每个 Git 操作都重新问；没有「本会话总是允许」，没有按项目持久化的授权。拒绝理由是写死的 `用户拒绝`（`main.tsx:404`），虽然 `reject` 消息本身支持任意 `reason`。 |
| Diff 与回滚 | 两栏全文 `变更前`/`变更后`（`main.tsx:1734-1743`），提案 id 要人手敲（`main.tsx:1700-1711`）；无 hunk、无按块接受、无高亮。undo point 只在进程内存里，最多 32 个。没有 checkpoint/rewind 概念。 |
| 输入历史 | 只活在内存里且受光标位置限制（`apps/chaos-ui/src/composer.ts:9-28`），重载即丢；`localStorage` 只存 `{version, sidebarWidth, composerHeight, theme, panelOpen}`（`apps/chaos-ui/src/layout.ts:1-24`），其中 `composerHeight` 存了也无处生效（`main.tsx:199` 写入的 CSS 变量没有任何规则读取）。 |
| 文件树/搜索/监听 | `list_files` 每次只读一层目录，界面是面包屑下钻（`chaos-engine/src/lib.rs:1338-1360`）；`search` 是 `walkdir` 全树同步遍历 + `String::contains`，100 条上限、忽略 `.gitignore`、只回文件名（`chaos-engine/src/lib.rs:1547`）；没有文件系统监听，编辑器外部的改动看不到。 |
| 交互式终端 | 只有审批后的一次性 `sh -c`，一个输入框一个输出块（`main.tsx:1473-1508`）；Xterm.js + PTY 的 stdin/resize/重连仍是开放项（`TODO.md` M2.3 已有行）。 |
| 工作流/子代理 | 完全没有事件与数据合同，界面无从展示（`TODO.md` M3.3 的四条开放项）。 |
| 计划清单 | 没有 todo/plan 的协议与界面（engine 的 plan 模式属于 TUI 侧，不在这条路径上）。 |
| 目标与预算 | 没有 objective/token_budget/tokens_used 这一层，界面上也无从显示「目标进行中」。 |
| 凭据/Provider | 设置面板只有主题、model、Base URL 三个可编辑项；`验证 Provider 连接` 不发任何网络请求，恒回 `network_not_attempted`（`chaos-engine/src/lib.rs:2855`）。 |

## 4. 差距结论与归属

判定沿用 `TODO.md` 第 2 章的五个标签，逐项登记位置见右列。

| 差距 | 判定 | 登记 |
|---|---|---|
| 会话是一等对象：枚举、标题、置顶、归档、未读、软删除、分组、可搜索 | Degraded（当前只有单会话 + 工作区列表） | M1.1、M2.1 各一行 |
| 服务端推送与真流式（`part.delta` 语义、断流恢复） | Degraded（同步整段回复） | M1.1 |
| 中途插话（startNow/guide/queue） | Unsupported（协议无对应） | M1.1 |
| 模型/模式/思考强度可选且真生效 | Degraded（三个写死胶囊） | M3.1 |
| 用量核算（按模型/轮/工具） | Unsupported（数字是编的） | M2.5 |
| 输入历史跨会话持久化（含附件） | Degraded（内存态） | M2.5 |
| 按作用域命名空间的设置 | Unsupported | M3.1 |
| 按项目持久化授权与「总是允许」 | Unsupported | M1.3 |
| 自由文本拒绝理由、审批内嵌 diff 预览 | Degraded | M1.3 |
| hunk 级 Diff 审查、checkpoint/rewind | Degraded（全文两栏） | M1.4 |
| 递归文件树、正则搜索、文件系统监听 | Degraded | M2.2 |
| 工具卡按类型分卡且真执行 | Degraded（通用卡 + 无 adapter） | M1.2 |
| 消息级复制/重试/改后再发/重新生成 | Unsupported | M1.2 |
| 推理过程展示 | Unsupported（协议无推理事件） | M1.2 |
| 带预算的长目标任务 | Unsupported | M2.5 |
| 工作流/子代理树 | Unsupported | M3.3 |
| 定时自动化与闲时任务 | Deferred（ZCode 已交付；本产品需先定产品与安全评审） | 第 6 章与矩阵行说明 |
| 交互式本地终端 | Degraded | M2.3（既有行） |
| 白板/Treemapping | 依据作废，ZCode 侧无证据 | 矩阵行说明 |

## 5. 复现

ZCode 侧的表结构可直接从两个 SQLite 库回读（只读打开，不写）：

```bash
python3 - <<'PY'
import sqlite3
for p in ['/home/chaos/.zcode/v2/tasks-index.sqlite', '/home/chaos/.zcode/cli/db/db.sqlite']:
    con = sqlite3.connect('file:%s?mode=ro' % p, uri=True)
    for t, sql in con.execute("select name, sql from sqlite_master where type='table' order by name"):
        print('=' * 12, t); print(sql)
PY
```

协议词表可按同样的形状重取：

```bash
grep -aoE '"[a-z][a-zA-Z]{2,20}\.[a-zA-Z][a-zA-Z]{2,24}"' \
  /home/chaos/.zcode/server/zcode-server.cjs | sort | uniq -c | sort -rn | head -100
```

Chaos 侧的现状用同一批位置回读即可，例如 `sed -n '6,44p' apps/chaos-ui/src/generated/protocol.ts` 看 `ClientMessage` 的全部种类，`grep -n 'composer-mode-pill\|composer-model-pill' apps/chaos-ui/src/main.tsx` 看三个写死胶囊。

## 6. 这份清单不支持的说法

- 不支持「ZCode 有白板/Treemapping」，也不支持「ZCode 桌面端是 Electron」—— 依据见第 1 节。
- 不支持任何界面布局、配色、间距层面的比较：客户端本体不在本机，比较只建立在数据合同与协议词表上。
- 不支持「ZCode 的表结构就是 Chaos 该照抄的 schema」。第 4 节给的是能力差距，不是迁移脚本；`TODO.md` 第 2 章「参考 ZCode 不等于无条件复制全部源码和资产」这条约束继续有效，M-1 的许可与产品归属门禁未通过之前，任何一项都不因本文件而视为已批准。
