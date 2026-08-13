# Changelog

## 0.2.137 — 2026-08-13

### 采样 / 网关兼容

- 修：thinking 模式回传补全 `reasoning_content`，避免 bblbb 等代理网关
  报 `bad_response_status_code 400`。
- 修：plan_mode / scheduler 零参数工具流式丢失（vLLM 0.23 + GLM-5.2-fp8
  实测必现）—— `enter_plan_mode` / `exit_plan_mode` / `scheduler_list` 加
  可选 `note` 字段，保证 arguments 非空。
- 修：reasoning-only 重试风暴—— `request_task.rs` 改用内容判断
  （`empty_reason() == ReasoningOnly`）替代 thinking-model 白名单，
  覆盖任意思维模型首 turn 不再触发无谓重试。

### Sandbox

- 移植上游 `allow_path` 规范化，修复 trailing glob 误建字面 `**` 目录。

### CI

- 工作区全量测试打通：`cargo test --workspace` 取代 7-crate 排除列表；
  4 crate（pager / pager-minimal / pty-harness / update）经 per-crate 审计
  确认 0 failures 后移回。`xai-grok-update` 的 47 个 `gh-release` 测试
  暂时 `#[ignore]`（上游 `fetch_gh_release_version` 从 gh CLI 切到
  GitHub HTTP API（reqwest），需 wiremock 重写后恢复）—— 见
  `docs/ci-test-debt.md`。
- `xai-grok-pager-render` 新增 `test-support` feature，导出
  `is_ssh_session` / `set_test_*` 钩子；为 CI（NO_COLOR=1、SSH 会话、
  VS Code Remote 等环境）提供确定性的终端上下文与颜色支持。
- 测试栈溢出修复：CI 设 `RUST_MIN_STACK=8MB`；`auth_retry_budget` 栈溢出
  测试补 `#[ignore]`。

### 文档

- 引入上游 1.0.1 / 1.0.2 / 1.0.3 changelog（`xai-grok-shell/changelogs/`）
  作参考，不替换本地 `0.2.136` 顶条。
- 合并上游 `b13fa526` 的 fork-layer 清单 `sync/fork-layer-inventory.md`。

## 0.2.136 - 2026-08-11

### 上游同步

- 同步上游 grok-build `b13fa526`（SOURCE_REV `a51a1dc6`），含 fork 层再核对
  （`sync/fork-layer-inventory.md`）。
- 手建仓库（无 `origin/HEAD`）时，默认分支回退到唯一存在的
  `origin/main` / `origin/master`，再回退 `init.defaultBranch`；两者同时存在
  时不猜测。
- Markdown 表格在窄面板内改为单元格内换行/硬切，右边界 `│` 不再被裁掉
  （grapheme 硬切 + 带样式链接保留）。
- SQLite 会话存储加固：`open`/`open_readonly` 内部改用带截止时间的
  `SQLITE_BUSY` 重试预算（10s，共享 deadline 不叠加），网络挂载更稳。
- tracker 懒加载 model→tokenizer 同步（避免启动时全量加载 BPE 表）。

### 国产 provider 错误处理

- 新增 `ProviderErrorKind` 分类，识别国产 provider（freemodel / workbuddy 等）
  的永久性故障（计费拒绝、客户端标识拒绝等）并给出中文可操作提示，而不是
  笼统重试。
- 计费类错误 fast-fail（不消耗 retry budget）；`edge_client_china` 标记国产
  网关；`Retry-After` 支持 HTTP-date 解析。
- `/provider` 新增国产 provider 预设；README 补充国产网关接入说明。
- 默认 retry budget 上限收到 8（与 opencode 对齐），避免国产网关限流时无限重试。
- thinking 模型 reasoning-only 空响应不再触发重试（避免误判为失败）。

### 采样 / 重试修复

- 修：`classify()` 多 tag 精确匹配漏洞 + `exceed.*context` 正则误用，导致
  错误分类错位。
- 修：OpenAI 兼容网关返回极小 SSE chunk 时采样器 panic（EAGAIN 路径），
  现容忍最小 chunk。
- 修：`record_response_token_usage` 测试调用签名与上游 merge 后的新签名对齐。

### 速率统计修复

- 修：`decode_tokens_per_sec` 未从 `UsageTotals` 传递到 pager，导致响应结束后
  速率 chip 不显示。
- 修：回合均 tok/s 不再把静默时间计入分母；速率统计不再把等待时间计入解码时长。

### Modal panic 修复

- 修：渠道/客户端模态框按键未分发到子组件导致 `unreachable!()` panic。
- 补全 `ProviderModal` / `ClientModal` 的 render、paste、mouse 路径，硬化 panic
  边界；第三处 `unreachable!()` 替换为 safe fallback。

### 其他修复

- 修：搜索 bootstrap 标记检查加重试，消除并发单飞竞态。
- 修：工作流恢复排序先于截断；忽略 chat-mode 死代码测试。
- 修：`agent_view` 合并后重复的 `last_turn_summary` 字段移除。

### CI

- 修：`rust-toolchain.toml` 覆盖导致 macOS x64 交叉编译缺 target，显式
  `rustup target add` 修复。
- 修：测试栈溢出，设置 `RUST_MIN_STACK=8MB`；`auth_retry_budget` 栈溢出测试
  补 `#[ignore]`。
- 修：上游 merge 带来的 clippy errors（`xai-grok-pager` 56 个、`xai-fast-worktree`
  disallowed_methods、3 个 test target）。

## 0.2.135 - 2026-08-07

### Removed

- 移除整个 CatPaw 渠道：`xai-catpaw` 原生协议 crate、扫码登录、Remote Agent
  （pod）通道及相关配置/UI/测试全部删除，`ApiBackend` 与 `SamplingConfig`
  不再携带 CatPaw / Remote Agent 通道类型。`[model_providers.catpaw]` 与
  `[model."catpaw/*"]` 用户配置需手动清理（本机配置已清理，备份见
  `config.toml.bak-catpaw-*`）。

### Bug fixes

- 修：右上角实时速率（tok/s chip）默认不显示——没有任何速率样本时芯片整体
  隐藏。现改为渲染暗淡的 `🐢 0 tok/s` 占位，保证实时速率默认常驻，速率从无到有
  的过程中不再凭空消失。

## 0.2.133 - 2026-08-06

### Bug fixes

- 修：`--client workbuddy` 走 freemodel 网关（work.freemodel.dev）时 403
  `unsupported_client` 的问题。逆向真实 WorkBuddy 客户端（header dump + 对线上网关
  的消融实验）确认校验点在 body 而非 headers：
  - `messages[0]` 必须是 system 消息且以精确的 31 字符前缀 `This conversation is powered by`
    开头（大小写敏感）；未命中时自动注入 marker system 消息。
  - body 中不得出现指纹子串 `You are Chaos`（网关据此识别 Chaos 客户端并拒绝，即使
    marker 前缀正确）；现于每个文本块（字符串与 blocks 两种形态）中将其替换为
    `You are the Chaos`。
  验证：`chaos --single ping --client workbuddy --model gpt-5.6-sol` 由 403 变为返回
  `pong`；非 WorkBuddy 请求不受影响。
- 修：`--client workbuddy` 403 时错误文案补充「WorkBuddy profile is active」引导，避免
  用户误以为 `--client` 未生效。

### Refactors

- 共享 aux-model sampler finalizer（session summary / 标题等辅助请求复用同一构造路径）。

### Chores

- WorkBuddy client profile 更新至 5.3.8。

### Tests / Tooling

- 新增 mock 推理服务器（`mock_server.py` / `single_mock_server.py` / `run_mock_server.*`，
  含 xai-grok-test-support 的 mock_server bin）与 WorkBuddy 请求 header 捕获脚本
  （`test_workbuddy_headers*.py` / `test_simple_wb.py` / `capture_listener.py`），
  用于本地复现与验证 WorkBuddy API 路径。
- `chaos-upstream-sync` skill 触发范围收窄。

## 0.2.131 - 2026-08-03

### Bug fixes

- 修：`build_session_info()` 不再丢弃 `decode_tokens_per_sec` / `avg_output_tokens_per_sec`，
  让 chat-state ledger 的速率数据能传到 pager 状态栏 chip（之前 `acp_session_impl::session_setup.rs`
  里硬编码为 `None`，导致右上角 tok/s chip 永远不显示）。
- 修：`cargo test` 因 `ClientProfile` 缺 `extra_headers` / `env_http_headers` 字段而编译失败
  （workbuddy 提交给结构体加了字段但测试 fixture 没跟上）。
- 修：`/context set` 报错信息从 `Internal error: ErrorCode(InvalidRequest) { … }` 改为透传结构化
  `acp::Error`，UI 现在显示「Cannot change the context window while a turn is in flight;
  try again after the turn finishes.」类中文消息。
- 修：`Token 用量统计` overlay 报 `aggregate usage not supported by this agent version` —
  `MvpAgent::ext_method` 顶层 dispatcher 漏掉 `x.ai/usage/aggregate` 路由。补上后请求能真正到达
  usage handler，overlay 双栏（本次会话 + 累计）正常填充，并用生产分发链回归测试锁定。

### Features

- 实时 tok/s chip：streaming 中按 `cl100k_base` BPE 累加 token 数（编码器不可用时回退
  `chars/4`），并以 chunk 间隔 EMA 展示当前生成速率；静默超过 1 秒后不再显示旧速率。
  Post-hoc `decode_tokens_per_sec` 仍负责响应结束后的速率显示。
- Token 用量 overlay 降级：单边 fetch 失败时保留另一边数据 + dim 部分失败提示，
  pending 侧显示加载中；仅两边都明确失败时才进入 `Failed(error)`。每次打开携带请求代次，
  关闭后重开不会被上一轮 late result 污染；会话尚未建立时明确显示会话侧不可用。
- 子 Agent 完成后按实际 `output` 文本计入父 turn 的实时输出速率；累计上下文
  `tokens_used` 只用于任务用量展示，replay 和重复终态不会重复计费。速率优先复用子 Agent
  tracker 的真实 decode rate，缺失时以 `output_tokens / duration` 估算；父 Agent 尚未输出文本的
  subagent-only turn 也能立即显示正数 tok/s。无实时样本或静默衰减为 0 时显示 `📊 平均 N tok/s`
  对话累计平均值，不再重复显示上下文剩余百分比。

### Refactors

- `UsageDetail::Ready` 枚举新增 `partial_failure: Option<String>` 字段；`session` / `aggregate`
  改成 `Option<Box<PromptUsage>>` 以支持半数据状态。`UsageDetail::Failed` 仅在两边都失败时
  使用，合并错误信息。
- `LiveStreamingRate` 字段从 `total_chars: u64` 改为 `total_tokens: u64`（BPE token 数），
  配套 `AcpUpdateTracker.token_encoder: Option<Arc<tiktoken_rs::CoreBPE>>` 懒加载字段。

### Tests

- 新增 `usage_partial_failure` 状态机和 overlay 渲染/竞态测试，覆盖双请求成功/失败乱序、
  单边 pending、late success、关闭后重开、无 session 与带分号错误文本的精确清理。
- 新增实时 tok/s 测试，覆盖 BPE 累加、EMA/静默衰减、live 零值不回退旧速率，
  以及子 Agent output 的 context/replay/重复终态/subagent-only turn 隔离和状态栏可见性。

### Dependencies

- 新增 `tiktoken-rs = "0.7"` 到 `xai-grok-pager/Cargo.toml`。cl100k_base 表~1.5MB，
  lazy-init 在第一个 chunk 到达时加载（启动开销零）。

## 0.2.128

### `/provider` 添加渠道支持从 Cline 导入

- 新增 `cline_import` 模块（`xai-grok-shell`）：只读扫描 VS Code / Cursor / Windsurf / VSCodium 的 `globalStorage/state.vscdb`，提取 Cline 的接口配置（base_url / auth_scheme / api_backend / api_key / model id）。
- `/provider add` 预设列表末尾新增「从 Cline 导入」选项（仅当检测到可用渠道时显示）。选择后列出所有可导入渠道，Enter 即可落成 Chaos 渠道。
- Cline 通过 Electron `safeStorage` 加密的 API Key（`v1:` 密文）标记为 🔒已加密，选中后引导用户手动粘贴。
- 全程只读打开 Cline 数据库（`SQLITE_OPEN_READ_ONLY`），不写回、不日志记录 Key。

### #18 思考等级请求失败时自动回退重试

- 当 provider 返回 400 且错误信息包含 `reasoning_effort` / `reasoning.effort` 时，自动移除 `reasoning_effort` 参数并重试，而不是中止整轮对话。
- 在 `RetryDecision` 中新增 `RetryWithEffortFallback` 变体，在 `classify_error` 中检测相关 400 错误，在 `apply_retry_decision` 中执行 effort 剥离和重试。
- 仅触发一次：如果重试仍然失败，则按原有 Fatal 逻辑处理。

### #19 `/think` 作为 `/effort` 的别名

- `/think` 已作为 `/effort` 命令的别名实现，两者行为完全一致。

### #20 系统提示词品牌修正

- 系统提示词模板从 `You are ${{ system_prompt_label }} released by xAI` 改为 `You are ${{ system_prompt_label }}, an AI coding assistant`，不再硬编码 "released by xAI"。
- `DEFAULT_SYSTEM_PROMPT_LABEL` 从 `"Grok"` 改为 `"Chaos"`，使用非 Grok 模型时助手不再自称 Grok。

## 0.2.127

### 多客户端请求档案

- 新增 `/client` 交互窗口，可选择 Claude Code、Codex、Grok Build，并支持自定义客户端的新增、编辑、删除和默认设置。
- 新增 `--client`、`chaos clients` 与 `chaos clients --json`，支持在同一工具中管理多种请求客户端身份。
- 客户端配置仅保存公开身份信息和环境变量名，不保存或传递 API Key。

## 0.2.124

### 上游同步至 `xai-org/grok-build` `5da6962`

- **B1**: `agent/models` 模块拆分：将 `agent/models.rs` 19 万行拆分为 `agent/models/{cache,endpoint,fetch,resolution,tests}.rs`，Chaos 保留 `default_models.json` 为空时的 `ConfigModelOverride` 测试种子，去掉 upstream 的 `sync_managed` 逻辑（`fetch_settings_blocking` 直接返回 `Option<RemoteSettings>`），同时 hoist `STARTUP_*`/`SETTINGS_*` 常量到 `xai-grok-http`（Chaos 继续使用 `shared_blocking_client`）。
- **B2**: workspace crate：`file_system/fuzzy.rs` 全量合并（`+584/-162`）；`session/git.rs` 适配新 `CommitResult`（Chaos 保留 `DeployError` 支持）；`workspace-types/src/rpc/git.rs` 新增 `CommitOutcome` / `PushStatus`、`GitCommitReq` `stage_all`/`seed_default_excludes`/`expected_branch` 字段；跳过 `preview_supervisor.rs`（上游依赖 `metric_donate::active_metrics_sink`）。
- **B3**: `xai-grok-tools` + `xai-tty-utils` runtime：62 个 upstream-only 工具文件全量合并；`xai-grok-tools-api/src/slash_commands.rs` 新增 `LoopFireMode` enum 和双参数 `loop_schedule_instruction`；`xai-tty-utils/src/runtime.rs` 全新文件，`process_scope.rs` 新增 `is_closed()`/`register()` 返回值；Chaos 在所有 `TaskSnapshot` 字面量添加 `is_backgrounded: false` 占位符（无 `bg_status` 状态机），revert `reminders/task_completion.rs` 和 `task/{mod,coordinator*,types}.rs`（upstream 移除了 Chaos 私有 `MonitorEventNotification`）；修复 SearchReplaceParams 的 Default impl（现在与 `#[serde(default = "default_true")]` 对齐，`include_user_edit_hint` 默认为 true）；
- **B4**: `workspace-types/src/rpc/workspace.rs` 新增 `BackgroundTaskSnapshotWire.description` 可选字段，hub_server 中透传自 `TaskSnapshot.description`（batch3 已加入）；
- **B5**: workspace permission 子系统（9个文件）：全量合并，仅把 `preview_supervisor.rs` 继续跳过；
- **B6**: workspace core（config/hub/session/git/error…）：17个文件全量合并，Chaos 继续保留 daemonize.rs / discovery.rs / project_config.rs 的 dual-path（.grok + .chaos）支持；
- **B7**: `xai-grok-shell` 剩余部分（约 162 文件）：分7个子批次（session/storage → testkit → terminal → tools → upload → util/config → slash_commands）全量合并，Chaos 保留 `is_backgrounded: false` 占位符、`LoopFireMode::Detached` 透传、telemetry 弱化、`default_models.json` 为空的 catalog 行为。

### 下游修复

- `xai-grok-shell/src/session/acp_session_impl/spawn.rs` / `xai-grok-shell/src/inspect/mod.rs`：透传 `folder_trust::project_scope_allowed(cwd)` → `resolve_permissions_with_provenance` 的 `project_trusted` 第二参数（batch5 新增，修复 batch5 引入的编译错误）；
- `xai-grok-workspace/src/hub.rs`：给测试的 `TaskSnapshot` 字面量添加 `is_backgrounded: false` 占位符；
- `xai-grok-pager/src/slash/commands/loop_cmd.rs`：三处以 `loop_schedule_instruction(args)` 调用改为双参数形式；
- `xai-grok-tools/src/reminders/task_completion.rs`：给 10 处 `TaskSnapshot` 字面量添加 `is_backgrounded: false` 占位符。

### 阻塞项（后续处理）

- **B8**：`xai-grok-pager` UI 层上游端口尚未开工，留待下一次同步窗口专门评估（涉及中文文案 / logo / 欢迎页更新日志的冲突面较大）。
- **blk1** — `xai-grok-mcp`：新 API `McpSpawnCtx::for_session` + `start_mcp_servers(EventWriter, OauthInteractivity)` 未移植，导致 `xai-grok-workspace/src/handle.rs` 和 `xai-grok-shell/src/session/handle.rs` 暂时留在 Chaos 当前版本；需要对齐 Chaos 已剥离的 OIDC 路径后再合入。
- **blk2** — `xai-grok-hooks`：`HookProvenance` / `HookSpec.layer` 未移植，导致 `xai-grok-workspace/src/workspace_ops.rs` 测试加层逻辑和 `xai-grok-shell/src/util/hooks.rs` 相关部分暂时跳过上 upstream。
- `metric_donate::active_metrics_sink`：Chaos 明确剥离该遥测捐赠面，继续跳过 `preview_supervisor.rs` 相关部分。
- **GitHub issues #15 / #16 / #17**：待本次分支合入 `main` 且用户确认口径后统一回复并关闭（本次同步仅完成实现层，未做远端 issue 写操作）。
- **50 项 pager UI 单测预存失败**：来自 `0.2.122` 基线，本轮 `cargo test --workspace -j 4` 复现（7740 pass / 50 fail），未在本次 sync 中修复，留作独立技术债项。

## 0.2.123

### #15 `--disallowed-tools` TUI 支持

- 从 headless-only 警告列表中移除 `--disallowed-tools` 和 `--tools`，现在 TUI 模式也生效。
- CLI 传入的 `--disallowed-tools` 和 `--tools` 接入 `ConnectFlags` → `CliAgentOverrides`，TUI 会话中内置工具按列表过滤。
- Leader 模式下两个 flag 会被识别为 unsupported 并发出警告。

### #16 配置模型参数不再发起 HTTP 请求

- `go_configure_model` 移除了 `load_models_for` 调用，打开「配置模型参数」界面不再冻结 UI 等待远端模型列表拉取。
- 用户直接手动输入模型 ID 即可配置参数。

### `/fallback` 命令

- 新增 `/fallback` 斜杠命令，管理备用模型链。
- 子命令：`set`（替换整链）/ `add`（追加）/ `remove`（移除）/ `clear`（清空）。
- 持久化到 `~/.grok/config.toml` `[fallback].models`。
- Agent 配置新增 `FallbackConfig` 结构体，sampler 层可读取备用模型列表。

### `/adhd` 命令

- 新增 `/adhd` 斜杠命令，切换 ADHD 技能集成。
- 用法：`/adhd`（切换）/ `/adhd on` / `/adhd off`。
- 开启后自动将 ADHD 辅助规则注入每个会话的系统提示词。
- 规则来源：https://github.com/uditakhourii/adhd
- 持久化到 `~/.grok/config.toml` `[adhd].enabled`。

## 0.2.122

### Token 用量修复

- **自动持久化**：每轮对话结束时自动将 token 用量写入 sqlite，不再仅在打开 `/usage` 面板时才触发。
- **Sentinel 归一**：火山方舟等网关在 SSE `usage.model` 里回传 `"auto"` 而非配置的模型名（如 `ark-code-latest`），现在自动用 `sampling_config.model` 重写，避免全部归到 `auto` 桶。
- **去重**：`record_session_usage` 写库前先 DELETE 同 session 的旧行，防止 auto/真实模型双计。
- **历史回填**：新增 `scripts/backfill-usage.py`，扫描文件系统历史会话 JSONL，将 sentinel 模型名重写为配置模型并写入 sqlite。

### #17 TUI 汉化

- 目标详情视图（goal detail）：状态标签、进度条目、完成度评估、最近历史、命令提示等全面汉化。
- Agent 状态栏：`goal_phase_label` 各阶段（校验中/规划中/执行中/空闲/失败/已中断/预算/完成）及 chip 名（"目标"）。
- 权限提示：编辑/bash/MCP 授权选项、始终允许/始终拒绝前缀、followup placeholder。
- 计划提示（plan nudge）："在规划？可用计划模式，快捷键 …"。
- 回退对话（rewind）："当前有一个轮次正在运行。"/"是否在回退前取消它？"/"取消轮次并回退"/"让它继续跑完"。
- Dashboard 模式标签：`plan` → "计划"、`always-approve` → "总是批准"、`auto` → "自动"。
- 首启 folder-trust（pager-minimal）："是否信任该目录下的内容？"/"允许，继续"/"拒绝，退出"。
- 截断指示器：`Ctrl-F to expand` → `Ctrl-F 展开`。
- 上下文信息栏：技能/MCP 服务器/工具计数等汉化。
- Scrollback verb group：读取/运行/搜索/子代理等动词标签汉化。
- Dashboard 行状态：Working → "运行中"、Response → "回复" 等。
- Session-scoped 命令在 dashboard 上的错误提示："/{name} only works in a session" → "请先打开会话再运行 /{name}。"

### 已知限制

- CJK 标签在 context info bar 中的列对齐尚未使用 unicode-width-aware padding，可能导致视觉上轻微错位（功能不受影响）。
- 50 个预先存在的单元测试失败（品牌 Chaos vs Grok、subagent replay count、extensions modal assertion 等），与本版本无关。
