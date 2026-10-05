# ZCode 能力取证与 Chaos Web 端差距（2026-10-06）

用户实测本机跑起来的 Chaos Web 端之后给出的判断是「web 端和 zcode 项目的差异还是较大」。这份文件把这句话变成可核对的清单：ZCode 侧每条能力都指出它来自哪个文件的哪段内容，Chaos 侧每条现状都指出源码位置，两边对不上的地方逐条写清归属里程碑。`TODO.md` 第 2.2 节与各处新增开放项引用本文件。

本文件在同一天做过一次逐条复量：每一条关于 ZCode 的断言都在这台机器上重新读过一遍才留下。第一版写错的四处（表数、一个命中数、把打包产物里的迁移 SQL 说成活表、对 Electron 的结论过弱）连同它们错在哪记在第 7 节。第 5 节给的命令是复量时真正跑过的那些。

## 1. 取证范围与取证不到的部分

本机 ZCode 安装目录只有一个前缀，`du -sh` 实测 387 MB：

    /home/chaos/.zcode/            387 MB
    ├── cli/       29 MB   plugins 13 MB、rollout 11 MB、db 5.2 MB、log 552 KB、exec 120 KB
    ├── server/   358 MB   asset-cache 196 MB、node 116 MB、agents 26 MB、tools 11 MB、
    │                      zcode-server.cjs 9.6 MB、build 84 KB
    └── v2/        1.2 MB   tasks-index.sqlite 与其 WAL、certs/

取证面只有这四份东西：

- `v2/tasks-index.sqlite`：主文件 4 096 字节、`-wal` 1 104 192 字节、`-shm` 32 768 字节。主文件只装得下 schema，行都在 WAL 里，因此拷走取证必须连 `-wal` 一起拷；只拷主文件仍读得到全部建表语句与索引，读不到行。
- `cli/db/db.sqlite`：主文件 1 200 128 字节、`-wal` 4 152 992 字节。
- `server/zcode-server.cjs`：10 049 783 字节，sha256 `c8b0dd682f5fe7d508b10ec78847b33af9b3c5c10709a66bcc7dccd91d547d9c`。`server/asset-cache/components/linux-x64/server-bundle/` 下另有两份按内容摘要命名的同名副本（10 049 783 与 10 037 895 字节），说明服务端是按摘要热更新的两版并存结构。
- `server/agents/glm/zcode.cjs`：13 125 321 字符的第二个打包产物，由 157 字节的 `agents/glm/zcode-agent` 起进程 —— 那个包装是 `exec "$runtime_root/node" "$HOME/.zcode/server/agents/glm/zcode.cjs" "$@"`，即「每个 agent 一个独立 node 进程，用绝对路径把 bundle 交出去」。

取证不到的是 **ZCode 的客户端本体**。本机没有 ZCode 的桌面或 Web 客户端可执行文件，`find /home/chaos/.zcode -iname '*.asar' -o -iname '*.html' -o -iname 'renderer*.js'` 命中 0；没有正在运行的 ZCode 进程（`ps` 与 `ss -ltnp` 都没有），也没有任何 UI 资产。`server/zcode-server.cjs` 里 `BrowserWindow`、`webContents`、`ipcMain`、`index.html` 四个串的大小写敏感命中数也都是 0。因此本文件不描述 ZCode 界面长什么样，也不声称任何像素级或交互级等价；所有 ZCode 能力都是从它自己的**数据合同**（两个 SQLite 库的真实 DDL 与真实行数）、**迁移 SQL**（打包产物里的建表语句）与**协议词表**（打包产物里的方法名与事件名）推出来的 —— 一个产品愿意为某张表建索引、愿意为某个事件命名，说明那条能力是它真实交付的功能，而不是待办。

客户端本体不在，但它**运行的形态**在打包产物里留了名字，这一点第一版判断得过于保守。`electron` 在 `zcode-server.cjs` 里大小写敏感命中 14 次（不敏感 34 次），逐条看上下文：1 次是 MIME 表里的 `application/vnd.ibm.electronic-media`（无关），其余 13 次是 ZCode 自己的代码 —— 更新通道 `electronReleaseChannelSchema = enum(["stable", "preview"])` 与设置项 `skippedElectronUpdateVersions`（同一段里还有 `autoDownloadAndInstallUpdates`）、出站请求头 `"X-Title": "Z Code@electron"`、`resolveElectronRuntimeZCodeAgentCommand()` 里那句 `if (!process.versions.electron) { return null; }`、遥测字段 `electron_version`，以及区分宿主进程是 node 还是 electron 的正则。结论要这样写：**ZCode 的桌面端跑在 Electron 上是有名字级证据的**，本机拿不到的是那个客户端本体，所以比较仍然落在数据合同与协议词表上，而不是界面上。

此前 `TODO.md` 第 2 章矩阵里「Whiteboard、Treemapping、轨迹 | Deferred | M5 后」这一行的依据要更正。`zcode-server.cjs` 里 `whiteboard` 命中 28 次，看着像 ZCode 有白板功能，逐条看上下文后是否定的：28 次全部落在字符偏移 1 780 390 到 1 788 181 这一段里，那段带 22 个 `open.feishu.cn` 链接，形如 `this.board = { v1: { whiteboard: { /** {@link https://open.feishu.cn/api-explorer?project=board&resource=whiteboard&apiName=download_as_image…` —— 是随包打入的飞书开放接口 SDK，「在飞书画板里创建节点/下载为图片」的第三方连接器代码，不是 ZCode 自己的界面能力。第二个打包产物 `agents/glm/zcode.cjs`（13 MB）里 `whiteboard` 命中 0。`treemap`、`treeMap` 两处都是 0。结论：白板与 Treemapping 在 ZCode 侧**没有证据**，矩阵那一行保留 Deferred 判定但依据要换掉。

同样要写清取证不到的另一半：**本机没有任何 automation 存储**。全盘 `find` 只找到上面两个 SQLite 库，`automations` 与 `automation_runs` 在其中都不存在（见第 2.2 节）—— 它们的建表语句只以字符串形式躺在服务端打包产物里，这台机器没有跑出一个调度器库。

## 2. ZCode 侧证据

### 2.0 证据强度分级（复量新增）

「表在 DDL 里」与「这台机器真的用过它」是两件事，第一版把它们混在一起写。2026-10-06 以只读方式打开两个活库逐表 `count(*)`，得到下面这份分级，第 2.1–2.3 节引用表时按它标注：

| 表 | 本机行数 | 级别 |
|---|---|---|
| `session`(2)、`message`(90)、`part`(321)、`session_input`(16)、`turn_usage`(16)、`tool_usage`(56)、`model_usage`(67)、`input_history`(15)、`local_setting`(3)、`session_entry`(3) | 有行 | A：schema 与真实数据都在，功能确实在本机跑过 |
| `tasks`(2)、`task_group_view_node_orders`(2) | 有行 | A：会话索引在本机确实被写过 |
| `session_target`(0)、`todo`(0)、`permission`(0)、`session_task_link`(0)、`workflow_run`/`workflow_definition`/`workflow_event`/`workflow_activity`(0) | 0 行 | B：表、CHECK 约束、索引、协议名都在，但本机没跑出来过 |
| `task_groups`(0)、`task_group_members`(0)、`task_group_workspace_bootstraps`(0)、`off_peak_tasks`(0) | 0 行 | B：同上 |
| `automations`、`automation_runs` | 库里没有这张表 | C：只在服务端打包产物里以 `CREATE TABLE IF NOT EXISTS` 字符串存在 |

因此本文件的比较口径是：**A 级差距是「它有数据合同且真在用，我们没有」；B 级是「它把合同钉死了，我们连合同都没有」；C 级是「它把 SQL 都发到了客户端，只是这台机器没用过」**。三级的紧迫度不同，第 4 节的判定按此排。

### 2.1 会话索引 `v2/tasks-index.sqlite`

6 张表、8 条显式索引：`tasks`、`task_groups`、`task_group_members`、`task_group_view_node_orders`、`task_group_workspace_bootstraps`、`off_peak_tasks`。

ZCode 把「会话」当作一等对象 `tasks`。下面这段是从库里 `select sql from sqlite_master` 逐字取回的 DDL，只把源码里挤在同一行的几个列换行排版，未删一字：

    CREATE TABLE tasks (
        workspace_key TEXT NOT NULL,
        workspace_path TEXT NOT NULL,
        workspace_identity TEXT,
        task_id TEXT NOT NULL,
        title TEXT NOT NULL DEFAULT '',
        task_status TEXT,
        provider TEXT,
        mode TEXT NOT NULL DEFAULT 'build',
        model TEXT,
        migration_source TEXT,
        forked_from_task_id TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        unread_at INTEGER,
        last_unread_at INTEGER NOT NULL DEFAULT 0,
        pinned INTEGER NOT NULL DEFAULT 0,
        archived INTEGER NOT NULL DEFAULT 0,
        deleted INTEGER NOT NULL DEFAULT 0,
        title_overridden INTEGER NOT NULL DEFAULT 0,
        meta_json TEXT NOT NULL DEFAULT '{}', searchable_text TEXT NOT NULL DEFAULT '', cron_automation_id TEXT, off_peak_task_id TEXT,
        PRIMARY KEY (workspace_key, task_id)
      )

配套的是**四条**部分索引（partial index），不是两条，且每一条都带 `WHERE … deleted = 0`：

    CREATE INDEX idx_tasks_workspace_archived_updated
      ON tasks (workspace_key, archived, updated_at DESC) WHERE deleted = 0
    CREATE INDEX idx_tasks_workspace_pinned_updated
      ON tasks (workspace_key, pinned, updated_at DESC) WHERE deleted = 0
    CREATE INDEX idx_tasks_cron_automation
      ON tasks (cron_automation_id, updated_at DESC)
      WHERE cron_automation_id IS NOT NULL AND deleted = 0
    CREATE INDEX idx_tasks_off_peak_task
      ON tasks (off_peak_task_id, updated_at DESC)
      WHERE off_peak_task_id IS NOT NULL AND deleted = 0

也就是说：置顶、归档、软删除是列表页每次都要走的查询路径，而「按 automation 反查会话」「按闲时任务反查会话」是同样被索引化的常规查询，不是备字段。

分组是独立的三张表：`task_groups(group_id, title, color DEFAULT 'gray', created_at, updated_at)`、`task_group_members(group_id, workspace_key, workspace_path, workspace_identity, task_id, sort_order, added_at, …)` 主键 `(workspace_key, task_id)` 加 `FOREIGN KEY (group_id) REFERENCES task_groups(group_id) ON DELETE CASCADE` 与索引 `idx_task_group_members_group_order(group_id, sort_order, added_at)`、`task_group_view_node_orders(node_type, node_key, sort_order)` 加 `idx_task_group_view_node_orders_order`，另有 `task_group_workspace_bootstraps` 记录某个工作区首次进组。也就是：**带颜色、可跨工作区、级联删除、可自定义排序的会话分组**。本机分组表 0 行（B 级），但 `task_group_view_node_orders` 有 2 行，说明视图排序代码真跑过。

打包产物里对 `cronAutomationId` 与 `offPeakTaskId` 的注释是中文原文（`zcode-server.cjs` 字符偏移 6 663 184 附近，逐字）：

    // cron automation 身份：随 meta_json 一起持久化（单一来源），同时在写入时投影到 tasks 表
    // cron_automation_id 索引列，供按 automation 反查 session。runId 属于 automation_runs /
    // 投递 metadata，不属于 task 表。
    cronAutomationId: nonEmptyStringSchema2.optional(),
    // off-peak 身份（D48）：与 cron 同款持久化策略——meta_json 单一来源 + tasks 表
    // off_peak_task_id 索引投影列（兜底/反查）。
    offPeakTaskId: nonEmptyStringSchema2.optional(),

### 2.2 定时自动化与闲时任务

**`automations` 与 `automation_runs` 不是本机的表**，第一版把它们写成了「表」。它们的真实存在形式是 `zcode-server.cjs` 里的建表 SQL 字符串（字符偏移 7 025 397 与 7 026 955），随服务端一起分发、在某个尚未在这台机器上创建过的库里执行。逐字取回：

    CREATE TABLE IF NOT EXISTS automations (
        automation_id TEXT PRIMARY KEY,
        title TEXT NOT NULL DEFAULT '',
        cron_expr TEXT NOT NULL,
        prompt TEXT NOT NULL,
        model TEXT,
        provider TEXT,
        mode TEXT,
        thought_level TEXT,
        workspace_key TEXT NOT NULL,
        workspace_path TEXT NOT NULL,
        workspace_identity TEXT,
        target_task_id TEXT,
        bot_delivery_target TEXT,
        location_kind TEXT NOT NULL DEFAULT 'local',
        recurring INTEGER NOT NULL DEFAULT 1,
        max_runs INTEGER,
        end_at INTEGER,
        schedule_rule TEXT,
        schedule_edited_by_user INTEGER NOT NULL DEFAULT 0,
        run_count INTEGER NOT NULL DEFAULT 0,
        scheduled_run_count INTEGER NOT NULL DEFAULT 0,
        enabled INTEGER NOT NULL DEFAULT 1,
        lifecycle_status TEXT NOT NULL DEFAULT 'active',
        next_run_at INTEGER,
        last_run_at INTEGER,
        running INTEGER NOT NULL DEFAULT 0,
        claimed_at INTEGER,
        dispatch_status TEXT NOT NULL DEFAULT 'idle',
        dispatch_attempts INTEGER NOT NULL DEFAULT 0,
        retry_at INTEGER,
        last_error TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
      );

    CREATE TABLE IF NOT EXISTS automation_runs (
        run_id TEXT PRIMARY KEY,
        automation_id TEXT NOT NULL,
        workspace_key TEXT NOT NULL,
        scheduled_at INTEGER,
        trigger TEXT NOT NULL DEFAULT 'schedule',
        dispatch_status TEXT NOT NULL DEFAULT 'claimed',
        outcome TEXT,
        session_id TEXT,
        error TEXT,
        attempts INTEGER NOT NULL DEFAULT 0,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
      );

配套的索引是**五条**，不是三条，第一版漏了两条：`idx_automations_due(enabled, next_run_at)`、`idx_automations_retry(enabled, retry_at)`、`idx_automations_workspace(workspace_key)`、`idx_automations_target_task(target_task_id) WHERE target_task_id IS NOT NULL`（部分索引）、`idx_automation_runs_by_automation(automation_id, created_at DESC)`。`run_count` 与 `scheduled_run_count` 分列、「计划次数」与「实际次数」分开记，是调度器的形状；`claimed_at`/`dispatch_status`/`dispatch_attempts`/`retry_at` 是「多实例抢占 + 失败重试」的形状。

闲时任务这一条比 automation 更硬：`off_peak_tasks` 是 `v2/tasks-index.sqlite` 里**真实存在**的表（本机 0 行，B 级），带 `server_ticket_id`、`prompt`、`permission_mode`、`thought_level`、`status`、`queued_at`、`started_at`、`ended_at`、`failure_reason`、`files_changed`、`settled_at`、`history_deleted_at`、`registered_at`、`schedulable`、`queue_position`、`next_poll_at`、`claim_running`、`claimed_at`、`attempt_count`、`last_error`，并有 `idx_off_peak_pick(status, queued_at)`、`idx_off_peak_ws(workspace_key, status)` 两条索引。服务端另有四个端点，逐字命中：`/api/v1/off-peak/ticket`、`/api/v1/off-peak/ticket/availability`、`/api/v1/off-peak/ticket/status`、`/api/v1/off-peak/anthropic/v1/messages`（最后那个意味着闲时任务把 Anthropic 的 messages 接口也代理了一份）。

### 2.3 CLI 库 `cli/db/db.sqlite`

**19 张表**（第一版写 21，错）：`input_history`、`local_setting`、`message`、`model_usage`、`part`、`permission`、`schema_migration`、`session`、`session_entry`、`session_input`、`session_target`、`session_task_link`、`todo`、`tool_usage`、`turn_usage`、`workflow_activity`、`workflow_definition`、`workflow_event`、`workflow_run`。下面每条列名与 CHECK 约束都是从活库的 `sqlite_master` 读回来的，不是从打包产物里的 SQL 推的。

`session` 表（本机 2 行）除了标题还带**标题来源**与**改动统计**：`title_source` 用 CHECK 限定为 `default`/`first_input`/`generated`/`custom` 且 `not null default 'first_input'`，另有 `title_message_id`、`time_title_updated`、`slug`、`version`、`share_url`、`summary_additions`、`summary_deletions`、`summary_files`、`summary_diffs`、`revert`、`permission`、`time_compacting`、`time_archived`、`trace_id`，以及 `task_type not null default 'interactive'`。

`session_input` 是**中途插话**的合同，本机 16 行（A 级，这台机器真插话过）：`delivery` 用 CHECK 限定为 `startNow`/`guide`/`queue`，配 `admitted_sequence`、`promoted_sequence`、`promoted_message_id`、`status` ∈ `admitted`/`promoted`/`cancelled`/`discarded`/`failed`、`status_reason`。协议侧有对应的用户设置 `zcodeInteractionBehaviorSchema = enum(["queue", "guide"])`。

`session_target` 是**带预算的长目标任务**（本机 0 行，B 级）：`objective`、`status` ∈ `active`/`paused`/`budget_limited`/`complete`、`token_budget`、`tokens_used`、`time_used_seconds`、`summary_title`、`active_input_id`、`active_run_started_at`、`active_run_last_seen_at`。

`workflow_run`、`workflow_definition`、`workflow_event`、`workflow_activity` 四张表加 `session_task_link` 构成**子代理树**（本机全 0 行，B 级）：`session_task_link` 有 `role`、`depth`、`path`、`phase`、`label`、`agent_type`、`model`、`status` 以及 `root_workflow_run_id`/`parent_link_id` 两条自引用；`workflow_activity` 有 `call_path`、`attempt`、`input_hash`、`child_session_id`、`result_json`、`error_json`，`status` ∈ `queued`/`running`/`completed`/`failed`/`skipped`/`cancelled`/`cached`/`lost`，且 `unique(run_id, call_path, attempt)`（同一路径可重试且留每次尝试）；`workflow_run.status` ∈ `pending`/`running`/`paused`/`completed`/`failed`/`cancelled`，并带 `budget_total`、`budget_spent`、`stats_json`、`failure_json`。

`todo(session_id, content, status, priority, position, …)` 是计划清单的持久化（本机 0 行）；`input_history(id, project_id, session_id, text, kind, attachments)` 是**跨会话、按项目分组的输入历史且带附件**，本机 15 行（A 级），索引 `input_history_project_time_idx(project_id, time_created desc, id desc)` 与 `input_history_time_idx`；`local_setting(scope, scope_id, namespace, key, value, schema_version)` 是**按作用域命名空间分的设置**，本机 3 行（A 级），索引 `local_setting_namespace_key_idx`、`local_setting_scope_idx`；`permission(project_id, data)` 是**按项目持久化的授权**，本机 0 行 —— 表在，但这台机器一次都没往里写过，这条差距的说服力要按 B 级算。

用量核算是独立三张表，且本机全在跑（A 级）：`model_usage` 67 行、`turn_usage` 16 行、`tool_usage` 56 行。三张表都按 `session_id`/`turn_id`/`trace_id` 关联，都记 `input_tokens`/`output_tokens`/`reasoning_tokens`/`cache_creation_input_tokens`/`cache_read_input_tokens`/`computed_total_tokens` 与 `first_token_at`/`duration_ms`/`time_to_first_token_ms`；`model_usage` 另有 `query_source`、`provider_id`、`model_id`、`variant`、`agent`、`mode`、`task_type`、`retry_count`、`finish_reason`、`raw_usage_json`；`tool_usage` 另有 `side_effect_scope`、`read_only`、`destructive`、`approval_status`、`exit_code`、`stdout_bytes`/`stderr_bytes`/`truncated`。索引形状也各不相同，不是笼统三个维度：`model_usage` 上有 `model_usage_session_turn_idx(session_id, turn_id)`、`model_usage_started_model_idx(started_at, provider_id, model_id)`、`model_usage_trace_idx(trace_id)`、`model_usage_query_source_idx(query_source)` 四条，`tool_usage` 上是 `tool_usage_session_turn_idx`、`tool_usage_started_tool_idx(started_at, tool_name)` 与唯一索引 `tool_usage_session_tool_call_idx(session_id, tool_call_id)`，`turn_usage` 只有 `turn_usage_started_idx(started_at)`（它的主键已是 `(session_id, turn_id)`）。

### 2.4 协议词表

按 `"a.b"` 形状从 `zcode-server.cjs` 提取，再逐个回数，能读出的方法/事件名与**实测命中次数**（普通子串计数，含引号内外）：

    session.created 5 / session.resumed 2 / session.closed 2 / session.updated 5 / session.upserted 2
    session.titleUpdated 2 / session.removed 1 / session.event 7
    turn.started 7 / turn.completed 8 / turn.failed 7 / turn.terminal 1
    turn.steerQueued 3 / turn.steerDrained 3                       中途插话的两端
    part.delta 2 / part.started 2 / part.upserted 3 / part.removed 2
    message.upserted 2 / message.removed 2 / reply.list 2 / reply.set 5
    model.list 2 / model.set 12 / model.streaming 5
    mode.list 4 / mode.set 6 / thoughtLevel.list 2 / thoughtLevel.set 4
    workspace.list 2 / workspace.set 6 / task.list 2 / task.set 6
    permission.request 8 / permission.requested 3 / permission.respond 8 / permission.resolved 3
    userInput.request 4 / userInput.requested 2 / userInput.response 1 / userInput.resolved 2
    elicitation.respond 12 / elicitation.submit 2
    streamRecovery.updated 4                                       流断裂后的恢复
    subagent.lifecycle 1 / subagent_message 3 / subagentProjectionStateSchema 3
    checkpoint.created 2 / checkpointId 28 / checkpointRootDir 3 / rewind.triggered 2
    usage.delta 1 / state.updated 7 / tool.updated 4 / tool.lifecycle 1
    mcp.servers 7 / worktreeRoot 6 / selection.cancel 8

按主题词的计数（大小写敏感的普通子串计数；第二列是第二个打包产物 `agents/glm/zcode.cjs` 里的同一计数，用来区分「只在服务端字符串里」与「哪一侧才是它的主场」）：

| 词 | server bundle | agent bundle |
|---|---|---|
| `pty` | 985 | 474 |
| `thoughtLevel` | 283 | 108 |
| `automation` | 407 | 190 |
| `offPeak` | 174 | 75 |
| `off_peak` | 76 | 0 |
| `marketplace` | 127 | 245 |
| `terminal` | 122 | 80 |
| `checkpoint` | **86**（第一版写 22，错） | 197 |
| `rewind` | 13 | 238 |
| `subagent` | 66 | 342 |
| `worktree` | 22 | 15 |
| `session_input` | 3 | 48 |
| `session_target` | 0 | 28 |
| `input_history` | 0 | 13 |
| `todo` | 45 | 94 |
| `reasoning`（词族：`reasoning_effort`/`reasoning_content`/`reasoning_tokens` 等） | 153 | 6,238 |
| `whiteboard` | 28（全在飞书 SDK 段） | 0 |
| `treemap` / `treeMap` | 0 / 0 | 0 / 0 |
| `BrowserWindow`/`webContents`/`ipcMain`/`index.html` | 0 | 0 |

右列值得单独读一句：`session_target`、`input_history` 在服务端产物里一次都没有，`session_input` 只有 3 次，而它们在 agent 侧分别是 28、13、48 次 —— 插话、目标预算、输入历史这些语义的主场是 agent 进程，服务端只负责搬运。第 3 节把这几条记为 Chaos 的缺口时，要对着 agent 侧那份词表看，而不是只看协议表面。

第 5 节那条按形状提取的命令还会带回一堆文件名噪声（`settings.json` 35 次、`plugin.json` 15 次、`config.json` 14 次、`package.json` 4 次、`git.exe` 4 次、`app.asar` 2 次），上面这份表是其中左侧为 ZCode 领域名词的子集 —— 引用它的时候别声称「这就是全部命中」。

### 2.5 随包交付的系统能力

这条与协议词表互补，因为它是**二进制级**的证据，第一版完全没有：

- `server/build/Release/pty.node`，75 976 字节，同时以内容摘要形式存在于 `asset-cache/components/linux-x64/node-pty/7270a362affb9968/pty.node`。交互式终端不是词表里的一个词，而是随包分发并已编译到本机架构的 node 原生 PTY 插件。
- `server/tools/` 下三个真实搜索二进制：`bfs`（1.0 MB）、`ripgrep/rg`（5.6 MB）、`ugrep/ugrep`（4.1 MB），各自带 `.version`。文件搜索在 ZCode 侧是外部进程 + 成熟实现，不是库内的目录遍历。
- `asset-cache/components/linux-x64/` 下按组件名的内容寻址目录：`node-pty`、`node-runtime`、`server-bundle`、`bfs`、`ripgrep`、`ugrep`、`glm`，外加一个空的 `staging/`。这是「能力=可替换组件」的落盘形状。
- `server/agents/glm/`：13 MB 的 `zcode.cjs` 加 157 字节的 shell 包装，`packages/` 13 MB。多 agent 是独立进程，不是同进程里的一个函数。

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
| 授权记忆 | 没有。`approve` 只带 `request_id`（`protocol.ts:16`），每次写盘、每条命令、每个 Git 操作都重新问；没有「本会话总是允许」，没有按项目持久化的授权。拒绝理由是写死的 `用户拒绝`（`main.tsx:404`），虽然 `reject` 消息本身支持任意 `reason`。ZCode 侧对应表 `permission(project_id, data)` 在本机 0 行，差距按 B 级读。 |
| Diff 与回滚 | 两栏全文 `变更前`/`变更后`（`main.tsx:1734-1743`），提案 id 要人手敲（`main.tsx:1700-1711`）；无 hunk、无按块接受、无高亮。undo point 只在进程内存里，最多 32 个。没有 checkpoint/rewind 概念。 |
| 输入历史 | 只活在内存里且受光标位置限制（`apps/chaos-ui/src/composer.ts:9-28`），重载即丢；`localStorage` 只存 `{version, sidebarWidth, composerHeight, theme, panelOpen}`（`apps/chaos-ui/src/layout.ts:1-24`），其中 `composerHeight` 存了也无处生效（`main.tsx:199` 写入的 CSS 变量没有任何规则读取）。ZCode 侧 `input_history` 本机 15 行，是 A 级差距。 |
| 文件树/搜索/监听 | `list_files` 每次只读一层目录，界面是面包屑下钻（`chaos-engine/src/lib.rs:1338-1360`）；`search` 是 `walkdir` 全树同步遍历 + `String::contains`，100 条上限、忽略 `.gitignore`、只回文件名（`chaos-engine/src/lib.rs:1547`）；没有文件系统监听，编辑器外部的改动看不到。ZCode 侧随包分发 `bfs`/`rg`/`ugrep` 三个二进制。 |
| 交互式终端 | 只有审批后的一次性 `sh -c`，一个输入框一个输出块（`main.tsx:1473-1508`）；Xterm.js + PTY 的 stdin/resize/重连仍是开放项（`TODO.md` M2.3 已有行）。ZCode 侧随包分发已编译的 `pty.node`。 |
| 工作流/子代理 | 完全没有事件与数据合同，界面无从展示（`TODO.md` M3.3 的四条开放项）。 |
| 计划清单 | 没有 todo/plan 的协议与界面（engine 的 plan 模式属于 TUI 侧，不在这条路径上）。ZCode 侧 `todo(session_id, content, status, priority, position)` 本机 0 行，但 `todo` 词在 agent 产物里 94 次 —— 合同与实现都在，只是这台机器没用过。 |
| 目标与预算 | 没有 objective/token_budget/tokens_used 这一层，界面上也无从显示「目标进行中」。ZCode 侧 `session_target` 表与 `session_target` 词（agent 侧 28 次）构成同一件事的两半。 |
| 凭据/Provider | 设置面板只有主题、model、Base URL 三个可编辑项；`验证 Provider 连接` 不发任何网络请求，恒回 `network_not_attempted`（`chaos-engine/src/lib.rs:2855`）。 |

## 4. 差距结论与归属

判定沿用 `TODO.md` 第 2 章的五个标签，逐项登记位置见右列；「级别」列取第 2.0 节的分级。

| 差距 | 级别 | 判定 | 登记 |
|---|---|---|---|
| 会话是一等对象：枚举、标题、置顶、归档、未读、软删除、分组、可搜索 | A（`tasks` 2 行） | Degraded（当前只有单会话 + 工作区列表） | M1.1、M2.1 各一行 |
| 服务端推送与真流式（`part.delta` 语义、断流恢复） | A（`part` 321 行） | Degraded（同步整段回复） | M1.1 |
| 中途插话（startNow/guide/queue） | A（`session_input` 16 行） | Unsupported（协议无对应） | M1.1 |
| 模型/模式/思考强度可选且真生效 | A（`model_usage` 67 行） | Degraded（三个写死胶囊） | M3.1 |
| 用量核算（按模型/轮/工具） | A（三表皆有行） | Unsupported（数字是编的） | M2.5 |
| 输入历史跨会话持久化（含附件） | A（15 行） | Degraded（内存态） | M2.5 |
| 按作用域命名空间的设置 | A（3 行） | Unsupported | M3.1 |
| 按项目持久化授权与「总是允许」 | B（表在、0 行） | Unsupported | M1.3 |
| 自由文本拒绝理由、审批内嵌 diff 预览 | — | Degraded | M1.3 |
| hunk 级 Diff 审查、checkpoint/rewind | B（`checkpoint` 词表 86 次，无表） | Degraded（全文两栏） | M1.4 |
| 递归文件树、正则搜索、文件系统监听 | A（随包三个搜索二进制） | Degraded | M2.2 |
| 工具卡按类型分卡且真执行 | A（`tool_usage` 56 行） | Degraded（通用卡 + 无 adapter） | M1.2 |
| 消息级复制/重试/改后再发/重新生成 | — | Unsupported | M1.2 |
| 推理过程展示 | A 的账 + B 的流（`model_usage.reasoning_tokens` 有行，`reasoning` 词族 agent 侧 6,238 次，但协议里无推理事件） | Unsupported | M1.2 |
| 带预算的长目标任务 | B（0 行） | Unsupported | M2.5 |
| 工作流/子代理树 | B（四表 0 行） | Unsupported | M3.3 |
| 定时自动化与闲时任务 | C / B（automation 只有迁移 SQL；`off_peak_tasks` 是真表 + 四个端点） | Deferred（本产品需先定产品与安全评审） | 第 2 章矩阵行与本文件第 2.2 节 |
| 交互式本地终端 | A（`pty.node` 75 976 字节随包） | Degraded | M2.3（既有行） |
| 白板/Treemapping | 无证据（28 次命中全在飞书 SDK 段） | 依据作废 | 第 2 章矩阵行 |

## 5. 复现

两个 SQLite 库以只读方式直接回读（这台机器上实跑，返回 6 与 19 张表）：

```bash
python3 - <<'PY'
import sqlite3
for p in ['/home/chaos/.zcode/v2/tasks-index.sqlite', '/home/chaos/.zcode/cli/db/db.sqlite']:
    con = sqlite3.connect('file:%s?mode=ro' % p, uri=True)
    n = con.execute("select count(*) from sqlite_master where type='table'").fetchone()[0]
    print('=' * 10, p, n, 'tables')
    for t, sql in con.execute("select name, sql from sqlite_master where type='table' order by name"):
        print('----', t); print(sql)
    for name, sql in con.execute("select name, sql from sqlite_master where type='index' and sql is not null order by name"):
        print('idx', ' '.join(sql.split()))
    for t in ('tasks', 'session', 'session_input', 'session_target', 'todo', 'permission',
              'input_history', 'turn_usage', 'tool_usage', 'model_usage', 'workflow_run',
              'off_peak_tasks', 'task_groups'):
        try:
            print('rows', t, con.execute('select count(*) from "%s"' % t).fetchone()[0])
        except Exception as e:
            print('rows', t, type(e).__name__)
PY
```

表结构在 `sqlite_master` 里，`tasks-index.sqlite` 的行在 `-wal` 里：只拷主文件仍读得到建表语句与索引，读不到行，所以拷贝取证要连 `-wal`、`-shm` 一起拷。

`automations`/`automation_runs` 只存在于打包产物的字符串里，取出来看一眼：

```bash
python3 - <<'PY'
import pathlib
s = pathlib.Path('/home/chaos/.zcode/server/zcode-server.cjs').read_text('utf-8', 'ignore')
for name in ('automations', 'automation_runs'):
    i = s.index('CREATE TABLE IF NOT EXISTS %s' % name)
    seg = s[i:i + 2600]
    print('### char offset', i); print(seg[:seg.find(');') + 2])
PY
```

协议词表与主题词计数（两份打包产物各量一次，第 2.4 节右表就是这段的输出）：

```bash
python3 - <<'PY'
import pathlib, re, collections
S = pathlib.Path('/home/chaos/.zcode/server/zcode-server.cjs').read_text('utf-8', 'ignore')
A = pathlib.Path('/home/chaos/.zcode/server/agents/glm/zcode.cjs').read_text('utf-8', 'ignore')
c = collections.Counter(m.strip('"') for m in re.findall(r'"[a-z][a-zA-Z]{2,20}\.[a-zA-Z][a-zA-Z]{2,24}"', S))
for k, v in c.most_common(60): print(v, k)          # 含 settings.json 这类文件名噪声
print('%-16s %8s %8s' % ('word', 'server', 'agent'))
for w in ('pty', 'thoughtLevel', 'automation', 'offPeak', 'off_peak', 'marketplace', 'terminal',
          'checkpoint', 'rewind', 'subagent', 'worktree', 'session_input', 'session_target',
          'input_history', 'todo', 'reasoning', 'whiteboard', 'treemap', 'treeMap', 'electron'):
    print('%-16s %8d %8d' % (w, S.count(w), A.count(w)))
for w in ('BrowserWindow', 'webContents', 'ipcMain', 'index.html'):
    print(w, S.count(w), A.count(w))
PY
```

ZCode 侧的行数、DDL 之外，第 2.5 节的二进制用 `ls -l`/`du -sh` 直接看：`server/build/Release/pty.node`、`server/asset-cache/components/linux-x64/*`、`server/tools/{bfs,ripgrep,ugrep}`、`server/agents/glm/zcode-agent`。

Chaos 侧的现状用同一批位置回读即可，例如 `sed -n '6,44p' apps/chaos-ui/src/generated/protocol.ts` 看 `ClientMessage` 的全部种类，`grep -n 'composer-mode-pill\|composer-model-pill' apps/chaos-ui/src/main.tsx` 看三个写死胶囊。

## 6. 这份清单不支持的说法

- 不支持「ZCode 有白板/Treemapping」：28 次 `whiteboard` 命中全在飞书 SDK 那一段，`treemap`/`treeMap` 为 0，且第二个打包产物里 `whiteboard` 为 0 —— 依据见第 1 节。
- 支持「ZCode 桌面端跑在 Electron 上」（名字级证据，见第 1 节），但**不支持**任何界面布局、配色、间距层面的比较：客户端本体不在本机，比较只建立在数据合同、迁移 SQL 与协议词表上。
- 不支持把「表在 DDL 里」等同于「这台机器用过它」。第 2.0 节的 A/B/C 三级就是为这句话准备的；`automations`/`automation_runs` 是 C 级，`permission`/`todo`/`session_target`/`workflow_*` 是 B 级。
- 不支持「ZCode 的表结构就是 Chaos 该照抄的 schema」。第 4 节给的是能力差距，不是迁移脚本；`TODO.md` 第 2 章「参考 ZCode 不等于无条件复制全部源码和资产」这条约束继续有效，M-1 的许可与产品归属门禁未通过之前，任何一项都不因本文件而视为已批准。
- 不支持把第 2.4 节两张计数表当成「打包产物里的全部命中」：形状提取还带回 `settings.json` 这类文件名噪声，且普通子串计数会把词族算进来（`reasoning` 那一行就是这么量的）。

## 7. 更正记录（2026-10-06 二次取证）

第一版本文件提交后逐条复量，改掉的内容如下。没有一条改动影响第 4 节的判定归属，被改掉的都是证据本身。

| 第一版写的 | 实测 | 位置 |
|---|---|---|
| CLI 库「21 张表」 | 19 张表（`sqlite_master` 逐个数） | 第 2.3 节 |
| `checkpoint` 命中 22 次 | 86 次（22 是 `worktree` 的数） | 第 2.4 节 |
| `automations` 是「一张完整的调度器表」，`automation_runs` 是「另有一张」 | 两者在本机两个库里都不存在，只是 `zcode-server.cjs` 偏移 7 025 397／7 026 955 处的 `CREATE TABLE IF NOT EXISTS` 字符串 | 第 2.2 节 |
| automation 索引「三条」 | 五条（含 `idx_automations_target_task … WHERE target_task_id IS NOT NULL` 与 `idx_automation_runs_by_automation`） | 第 2.2 节 |
| `automations` 列清单 | 漏 `title`、`scheduled_run_count`、`created_at`、`updated_at`，且 `dispatch_status` 有 `DEFAULT 'idle'` | 第 2.2 节 |
| `tasks` 的配套索引两条 | 四条部分索引 | 第 2.1 节 |
| 「`electron` 命中 14 次而同族窗口 API 全部为 0，因此不能据此说 ZCode 桌面端是 Electron」 | 那 14 次里 13 次是 ZCode 自己的更新通道、设置项、请求头与运行时判断，名字级证据成立；本机拿不到的是客户端本体，所以仍无界面级证据 | 第 1、6 节 |
| 「真实 DDL（截掉与本节无关的列注释）」 | DDL 本身是逐字的，但那一行没有注释可截，是同一行装了四列；表述改为「逐字取回，仅重新排版」 | 第 2.1 节 |

流程上的教训值得记进 `docs/ci-test-debt.md`：本仓库的文档门禁 `scripts/ci/check-doc-path-refs.py` 只校验**仓库内**路径（规则 4：首段必须是本仓库有跟踪内容的顶层条目），所以指向 `/home/chaos/.zcode/…` 与 `server/…`、`v2/…` 这类**仓库之外**的取证路径没有任何闸门会去验它存不存在。这类断言只能靠写的人在提交前逐条实测，第一版正是把一份未经复述的二手摘录直接抄进了文件才出现上面的偏差。
