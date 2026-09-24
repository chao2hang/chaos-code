# Chaos 执行计划：Desktop / Web 交付 + 现有分叉维护

> 本文件是项目的**唯一执行清单**，覆盖两条并行的线：
>
> - **GUI 线（M-1～M5，第 4 章）**：新建的桌面端与 Web 端。
> - **维护线（MT-1～MT-7，第 7 章）**：现有 Chaos TUI 分叉在途的发布阻断项、安全债、测试债与中文化收尾。这条线**不依赖 GUI 线**，且其中 MT-1 是下一次发版的硬前提。
>
> 功能规格、架构说明和风险用于解释任务，不重复设置复选框。
>
> **目标**：在保留现有 Chaos TUI 和上游同步能力的前提下，交付共享 Rust 后端的 Tauri 桌面端与 Axum Web 端。
>
> **准确表述**：后端、桌面宿主和系统能力使用 Rust，不依赖 Node.js 或 Electron 运行时；React 前端构建为静态 JavaScript/WASM 资产，在浏览器或系统 WebView 中运行。Node/pnpm 仅用于构建和测试前端。

---

## 1. 执行规则

### 1.1 状态与责任

- `[ ]` 未开始；`[~]` 进行中；`[x]` 已完成；`[-]` 明确取消。
- 每个里程碑开始前必须填写 Owner、目标日期和关联 ADR/Issue；当前统一标记为 `TBD`，**未填写不得开工**。
- M-1～M5（GUI 线）和 MT-1～MT-7（维护线）是唯一排期和勾选入口。能力矩阵和第 5 章的设计约束不是第二套任务表。
- 两条线抢同一批人时，**维护线的 P0/P1 优先**：分叉现有用户已经在用 TUI，GUI 还没有用户。
- 完成任务时，在复选项下补充 PR/commit、测试命令和结果；不能只勾选。
- 遇到范围变化，先更新兼容矩阵和 ADR，再修改实现。

### 1.2 完成定义（Definition of Done）

一项功能只有同时满足以下条件才算完成：

1. 代码、测试、用户文档和迁移说明一并提交；
2. Rust 通过 `fmt`、`check`、`clippy` 和相关测试；前端通过格式化、lint、typecheck、单测与 build；
3. 协议或配置变化有兼容测试，生成文件无漂移；
4. 安全敏感路径覆盖拒绝、超时、取消、重放和越权测试；
5. UI 变更必须在真实桌面/Web 环境操作验证，不能只看截图；涉及共享状态时验证所有读取该状态的页面；布局变更验证桌面和窄视口；
6. 性能敏感功能提供可重复 benchmark，不以主观观感验收；
7. 未解决缺陷记录到 issue，并按本文件定义的 release blocker 规则处理。

### 1.3 发布阻断级别

| 等级 | 定义 | 处理 |
|---|---|---|
| P0 | 数据丢失、任意代码执行、鉴权绕过、密钥泄露、协议导致重复危险操作 | 阻断所有里程碑发布 |
| P1 | 核心闭环不可用、会话无法恢复、写入/Diff 状态不一致、安装或升级失败 | 阻断当前里程碑发布 |
| P2 | 有可靠绕过方案的非核心问题 | 可延期，但必须登记 owner 和目标版本 |
| P3 | 文案、低影响视觉问题 | 可进入后续维护列表 |

### 1.4 变更隔离

GUI 新代码默认只进入：

- `apps/chaos-ui`
- `crates/codegen/chaos-engine`
- `crates/codegen/xai-grok-desktop`
- `crates/codegen/xai-grok-web`
- 新增且经 ADR 批准的 transport/protocol crate

修改 `xai-grok-agent`、`xai-grok-tools`、`xai-grok-config`、`xai-grok-pager` 等上游核心 crate 前，必须说明为何无法通过适配层完成，并更新 `sync/fork-layer-inventory.md`。目标是**可审计的低冲突**，不承诺不现实的“零冲突”。

---

## 2. 范围与兼容矩阵

“参考 ZCode”不等于无条件复制全部源码和资产。每项功能必须在 M-1 完成许可证和产品归属判断后标记为下列状态之一：

- **Parity**：目标交互和结果等价；
- **Replacement**：用 Chaos 原生能力替代；
- **Degraded**：因平台限制提供明确降级；
- **Deferred**：不进入首个稳定版；
- **Unsupported**：明确不实现。

| 能力 | 首版目标 | 计划里程碑 | 说明 |
|---|---|---:|---|
| 会话列表、对话时间线、流式输出 | Parity | M1 | 保留虚拟滚动、锚点和错误态 |
| 富文本输入、`/` 命令、`@` 文件 | Parity | M1/M2 | M1 先交付文本输入，富文本增强在 M2 |
| 工具卡片、提问、权限审批 | Parity | M1 | 必须覆盖拒绝、取消、超时和重连 |
| Diff 审查、文件回滚 | Replacement | M1/M2 | 数据来自现有 Rust Git/hunk 能力 |
| 文件树、快搜、终端、Git | Replacement | M2 | 复用 workspace RPC，避免重复建设 |
| 设置、Provider、模型 | Replacement | M0/M3 | M0 提供最小配置，M3 提供完整 UI |
| MCP、插件、技能 | Replacement | M3 | 对接现有 Rust crates |
| 工作流、子代理 | Replacement | M3 | 对接 `xai-workflow` 和现有子代理能力 |
| 桌面端 | Parity | M0～M5 | Tauri v2 |
| Web 端 | Degraded | M0～M5 | 无本地系统能力时显示明确降级 |
| 远程文件/Git/工具执行 | Replacement | M4 | 复用 workspace RPC 类型与 handler |
| 远程交互式 PTY | Deferred | M4 后评估 | 先做 spike，再承诺正式范围 |
| 本地断线后远程 Agent 继续推理 | Deferred | M4 后评估 | 取决于远程执行拓扑 ADR |
| 内嵌浏览器/CDP | Deferred | M5 后 | iframe 不能视为等价实现 |
| 分享、配额、云同步、登录墙 | TBD | M-1 | 必须选择自建、替代或下线 |
| Whiteboard、Treemapping、轨迹 | Deferred | M5 后 | 不阻断首个稳定版 |
| CUA、语音、自动化/闲时任务 | Deferred | M5 后 | 单独产品评审 |

### 2.1 品牌替换边界

必须替换：用户可见产品名、窗口标题、默认文案、新增图标和新资源。

默认保留：`xai-grok-*` crate 名、wire/tool ID、`GROK_*` 环境变量、`$GROK_HOME`、`~/.grok` 兼容读取、历史数据字段、上游同步引用和必要测试夹具。任何协议标识改名都需要版本化迁移，禁止机械全局替换。

---

## 3. 当前仓库能力基线

实施前以 M-1 盘点结果为准；当前已确认的起点如下：

| 领域 | 当前能力 | 计划动作 |
|---|---|---|
| Headless | `xai-grok-pager::headless` 已支持单轮流式运行 | 提取稳定服务接口，不从零重写 Agent |
| Workspace server | `xai-grok-workspace` 提供 `xai-workspace-server` binary | 评估作为远端 server 的基础 |
| Daemon lifecycle | `xai-grok-workspace-daemon` 只有 daemonize/pidfile/preview supervision | 不把它误当完整 RPC daemon |
| Workspace client | `xai-grok-workspace-client` 基于 `ToolHarness` 的 hub-proxied RPC | 复用类型和语义；SSH transport 需另行设计 |
| Workspace RPC | 已覆盖文件、Range 读取、传输、搜索、Git、hunk、worktree、配置等 | 建立复用矩阵，只补缺口 |
| SQLite | `xai-sqlite-journal` 提供 WAL/TRUNCATE 选择及 busy retry | 不把它当 schema migration 框架 |
| 配置目录 | `xai-dirs` 支持 `$CHAOS_HOME`/`$GROK_HOME` 和 `~/.chaos`/`~/.grok` 双读 | 未经迁移 ADR 不切换到纯 XDG |
| Secrets | `xai-grok-secrets` 当前主要是日志/JSON/URL 脱敏 | API Key 加密存储需要单独实现或选型 |
| 系统电源 | 已有 `xai-system-power` | 在 engine 生命周期层接入 |
| CI/Release | 现有 Rust CI 和 CLI 多平台发布 | 新增前端/Tauri/Web 流水线，不假设现成支持 |

### 3.1 为什么 TUI 与 GUI 同仓（依赖图证据）

同仓的依据不是"monorepo 是好实践"，而是本仓库的分层**已经**支持它。核对于
2026-09-22：

| 事实 | 实测值 |
|---|---|
| 依赖 `ratatui` / `crossterm` 的 crate | **7 个**：`xai-grok-pager`、`-pager-render`、`-pager-minimal`、`xai-grok-markdown`、`xai-grok-gboom`、`xai-ratatui-inline`、`xai-ratatui-textarea` |
| `xai-grok-shell`（Agent 引擎，约 40 万行） | **零** TUI 依赖 |
| 依赖方向 | `xai-grok-pager → xai-grok-shell`，单向，无反向依赖 |
| workspace 成员总数 | 97（`codegen` 79 + `common` 12 + 其余 6） |

结论：TUI 已经是引擎之上的一层皮，GUI 是在同一引擎上加第二层皮，而不是把两个
产品硬塞进一个仓库。拆仓需要跨仓发版、对齐两份 `Cargo.lock`、维护两份分叉层
清单，且上游同步只认一个仓库——成本高于收益。

**但同仓有四项必须前置处理的代价**，分别落在 M-1.6（构建与 CI 隔离）、
M-1.4 的 `ADR-002`（headless 下沉）和 M-1.3（`Cargo.toml` 分叉登记）。未完成这
四项前不得把 GUI crate 加入 workspace `members`。

---

# 4. GUI 线路线图

## M-1：可行性、合规与架构冻结

**Owner**：Chaos 主线维护者
**目标日期**：2026-10
**前置依赖**：无  
**退出条件**：以下所有任务完成，关键 ADR 被批准，且 M-1.6 的构建与 CI 隔离指标达标；否则 M0 不得开始。

### M-1.1 来源与许可证门禁

- [x] 记录 GUI 来源与资产基线：本阶段采用 clean-room，自有实现不复制参考产品源码或资产；系统字体/CSS 无新增第三方资产。（2026-09-24；`docs/legal/ui-source-baseline.md`）
- [x] 核验当前 GUI 新增范围的许可证与 NOTICE：无复制源码/字体/图标/图片，仅使用仓库既有 Apache-2.0 依赖和系统字体；证据见 `docs/legal/ui-source-baseline.md`。（2026-09-24）
- [x] 逐类审计字体、图标、插画、图片、Office 预览资产和商标；本阶段无新增字体、图标、插画、图片或 Office 资产，品牌资产标为 Replacement/Deferred，清单见 `docs/legal/ui-source-baseline.md`。（2026-09-24）
- [x] 确定文件头、NOTICE、第三方声明和修改记录规则：新 GUI 代码沿用仓库 Apache-2.0 文件许可；仅使用既有 workspace 依赖，未引入复制资产；证据与复核命令见 `docs/legal/ui-source-baseline.md`。（2026-09-24）
- [x] 无法证明可复制的参考 UI 不进入实现；本阶段采用 clean-room 重写，用户可见品牌与资源不复制，后续资产必须重新过许可证门禁。（2026-09-24）

**验收证据**：`docs/legal/ui-source-baseline.md`、资产清单、许可证副本或链接、审批人和日期。

### M-1.2 产品范围冻结

- [x] 为第 2 节每个 TBD 项选择 Parity、Replacement、Degraded、Deferred 或 Unsupported；见 `docs/architecture/m1-scope-matrix.md`。（2026-09-24）
- [x] 明确首个稳定版是单用户本地开发工作台，Web 默认 loopback；公网、多用户、云同步和登录墙不在首版范围。（2026-09-24）
- [x] 固定当前最低平台策略：M0 Linux 本地验收；macOS/Windows 需独立 CI 构建通过后才列为支持平台；WebView/浏览器版本由 M5 CI 固定。（2026-09-24）
- [x] 明确移动 Web 是窄视口适配，不是独立正式移动平台。（2026-09-24）

**验收证据**：更新后的兼容矩阵；无未解释的 TBD。

### M-1.3 现有能力盘点

- [x] 建立 `docs/architecture/gui-capability-matrix.md`，记录 Agent、会话、文件、Git、hunk、PTY、MCP、插件、工作流、远程 RPC 的已有实现、reuse/adapter/new/unsupported 分类和下一步负责人。（2026-09-24）
- [x] 对 `xai-workspace-server`、`xai-grok-workspace-client`、`xai-grok-workspace-daemon` 做调用链边界盘点：M0 不接入远程；M4 复用 workspace RPC，不把 daemon 当 RPC server。（2026-09-24；`docs/architecture/gui-capability-matrix.md`）
- [x] 盘点现有会话、事件、索引、配置和缓存的真实存储边界：M0 engine snapshot 仅保存 timeline/audit/dedup；provider secrets 留在配置边界；canonical SQLite migration 延至 M2。（2026-09-24；ADR-003）
- [x] 识别当前主线无需修改上游 Agent/pager 核心文件；adapter 以独立 GUI crate 接入，headless 迁移保留为 M0 后续项。（2026-09-24；ADR-002）
- [x] 把根 `Cargo.toml` 和 GUI 独立目录登记进 `sync/fork-layer-inventory.md`。（2026-09-24）
- [x] 约定 GUI crate 在 `members` 中集中追加并用注释标出分叉区段，使同步冲突可机械识别。（2026-09-24）

**验收证据**：能力矩阵能把后续每个实现任务标为“复用、适配、新增/不支持”之一；`sync/fork-layer-inventory.md` 含根 `Cargo.toml` 与 GUI fork 区段；构建隔离见 `docs/architecture/gui-build-isolation.md`。

### M-1.4 ADR 冻结

- [x] `ADR-001`：前后端逻辑协议；确定 Rust `chaos-engine` 作为 envelope 单一来源，Web/Desktop 共享逻辑协议，协议版本从 1 起步。（2026-09-24；`docs/architecture/adr-001-gui-walking-skeleton.md`）
- [x] `ADR-002`：engine 边界；walking skeleton 先以 `chaos-engine` 协议 mock 验证 Web/Desktop seam，保留现有 headless 实现，后续通过 adapter 接入而不复制生命周期。（2026-09-24；`docs/architecture/adr-002-gui-engine-adapter.md`）已明确 `headless.rs` 当前物理位于依赖 ratatui 的 pager crate，先保留原位并以 adapter 接入，避免把终端渲染栈拖进 GUI。（2026-09-24）
- [x] `ADR-003`：确定 M0 使用 engine 原子 JSON 快照验证恢复，后续 canonical SQLite store、迁移、备份和 NFS 策略按文档推进。（2026-09-24；`docs/architecture/adr-003-gui-persistence.md`）
- [x] `ADR-004`：M0/M1 先支持本地 Agent + 本地 workspace；远程 Agent/工具、SSH、端口转发和 detached Agent 明确留至 M4，不把 loopback 原型伪装成远程控制面。（2026-09-24；`docs/architecture/adr-004-remote-topology.md`）
- [~] `ADR-005`：已冻结本地 Web 安全基线，见 `docs/architecture/adr-005-web-security.md`；桌面 IPC、远程连接和完整凭据边界待补。
- [x] `ADR-006`：前端采用 clean-room React/TypeScript 重写；不复制参考源码/资产，Chaos UI 仅通过版本化 Rust protocol 消费 engine，未来更新以本仓库审查为准。（2026-09-24；`docs/legal/ui-source-baseline.md`、`docs/architecture/adr-006-gui-source.md`）

**ADR 必须回答**：备选方案、选择理由、兼容影响、失败模式、迁移和回滚。

### M-1.5 依赖与平台 spike

- [~] Tauri v2 spike：Desktop host boundary 已隔离且不引入 Tauri 依赖；真实 Tauri 三平台构建待环境依赖与独立 runner，不能以 host crate 通过替代。
- [~] Axum + 静态资源嵌入：Axum loopback/WebSocket 已真实运行；静态资源嵌入、压缩、SPA fallback 待 M5。
- [~] Rust→TS：M0 使用 serde JSON envelope 与协议文档；自动生成 TypeScript 类型/运行时校验待 M0.2 完成。
- [ ] 验证候选 SSH 库的 SSH Agent、私钥口令、keyboard-interactive、ProxyJump/ProxyCommand 和端口转发能力。
- [~] SQLite migration：M0 JSON transitional；`SqliteSessionStore` 已正式接入 Engine/Web，复用当前 `rusqlite`/`xai-sqlite-journal`，schema/损坏库/新版本/重启恢复测试通过；已核对 TUI 真实 session persistence 仍是 `xai-grok-shell` 的目录/summary actor，不与 GUI SQLite schema 直接兼容，需独立迁移 fixture；多进程/NFS/迁移回滚和 TUI fixture 仍待 M2 gate。（2026-09-24；`sqlite_entry_flow.rs`、`final-adversarial.log`）
- [x] 新增依赖许可证/维护状态完成初步审查：GUI 仅复用 workspace 已声明 axum/tower/tower-http/serde/uuid/tokio 依赖，未新增第三方资产。（2026-09-24）

**验收命令**：每个 spike 有独立 README、最小测试和 CI job；失败的候选不得写入正式架构。

### M-1.6 构建资源与 CI 隔离（同仓的前置条件）

**这一节是 GUI crate 获准进入 workspace `members` 的门禁。** 同仓本身可行（见
第 3.1 节），但本仓库的构建足迹已经接近硬件上限，叠加 Tauri 与前端工具链前必须
先设好隔离。

实测现状（2026-09-22）：

| 项 | 实测值 | 风险 |
|---|---|---|
| `target/` 体积 | **173 GB** | `docs/known-issues/wsl-p9io-crash-20260728.md` 记录的 WSL2 强制重启，诱因正是 `target/` 达 150 GB 时的并发编译。这是本机**已发生过**的事故，不是理论风险 |
| CI `rust` job | 单 `ubuntu-latest`，`timeout-minutes: 60` | 执行 `cargo check/clippy/test --workspace --all-targets`；GUI 入 workspace 后，改一行 TUI 代码也会连带编译 Tauri 依赖树 |
| `Cargo.lock` | 15 480 行 | 前端与 Tauri 依赖会显著拉长，冲突解决成本上升 |

- [x] 定义 GUI crate 的构建隔离方式：GUI Rust crates 保持独立 package，Tauri 依赖不进入 workspace；前端使用独立 `apps/chaos-ui/node_modules`/`dist`，并由 `.gitignore` 排除。（2026-09-24）
- [x] 确认默认 TUI binary 不触发 Tauri/WebView：当前 Desktop crate 无 Tauri 依赖，GUI package 独立检查通过；结构检查见 CI `gui` job。（2026-09-24）
- [~] 主 CI `rust` job 不包含 GUI；GUI 走独立 `gui` job，当前 workflow 已验证分离，主 job 前后耗时对比与平台 runner 结果待 CI 完成。
- [x] 文档化 target/node 构建容量治理入口：GUI 输出目录已隔离并忽略；WSL 低内存 `-j 4` 约束沿用贡献基线。（2026-09-24）
- [x] 评估 `node_modules` 与前端构建产物落盘并补 `.gitignore`，避免 GUI 本地生成物进入 `git status`。（2026-09-24）
- [x] 若隔离失败的退出方案：撤回 workspace GUI members，保留独立 engine crate 并拆分发布；当前隔离未失败。（2026-09-24）

**验收证据**：加入 GUI crate 前后的主 CI job 耗时对比、一次完整 `cargo build` 的
峰值内存与磁盘增量、隔离生效的验证命令输出。**任一指标不达标则 M0 不得开始。**

---

## M0：安全的双端 Walking Skeleton

**Owner**：Chaos 主线维护者
**目标日期**：2026-10
**依赖**：M-1 已冻结范围、核心 ADR 与本地安全边界；未完成的 Tauri、浏览器与跨平台 gate 继续阻断 M0 完成。
**交付目标**：桌面和本地 Web 均可创建一个会话、提交纯文本 Prompt、接收流式文本、取消请求并恢复历史；Web 从第一天具备基础安全边界。

### M0.1 工程骨架

- [x] 创建 `apps/chaos-ui`：Vite、React、TypeScript、Vitest 脚本和 npm lockfile；实现最小时间线/composer/演示响应。（2026-09-24；`npm run typecheck`、`npm run build` 通过）
- [x] 创建 `crates/codegen/chaos-engine`，提供版本化 client/server envelope、session create、submit、ack、text delta、completed、cancel。（2026-09-24；crate tests 通过）
- [~] 按 `ADR-002` 的结论处理 headless：已新增 `chaos-engine::PromptAdapter` 和受显式路径控制的 `HeadlessProcessAdapter`，Web 可通过 `CHAOS_AGENT_BINARY` 接入真实 `chaos --headless --output-format json`；物理迁移 `headless.rs` 及其 ACP 胶水仍待独立回归批次，现有 CLI 行为未改。（2026-09-24；engine/Web adapter tests 通过）
- [x] 创建 `xai-grok-desktop` 和 `xai-grok-web`，加入 workspace 末尾的分叉区段；Web 提供 loopback Axum health/handshake，Desktop 提供独立 host boundary。（2026-09-24；Rust check/test 与独立 `ci.yml` GUI job 已加入）
- [x] 保证现有 `chaos` TUI/CLI 默认构建和行为不变；GUI crate 独立于 TUI binary，默认 workspace check 不引入 Tauri。（2026-09-24；GUI crate 独立 check 通过；完整 workspace 回归待 M0.6）
- [ ] 回归验证 M-1.6 的隔离指标在真实 GUI crate 下依然成立（此前是最小 spike，此处是真实依赖树）：主 CI job 耗时无变化、默认构建不拉 Tauri 依赖。

### M0.2 最小协议

- [x] 实现并版本化 handshake、create/resume session、submission、ack、分块 text delta、completed、error、cancel、snapshot；engine protocol v1 与 WebSocket integration test 已提交。（2026-09-24）
- [x] 每条命令携带 `client_msg_id`；engine 以进程/持久化状态作用域去重，重复命令只返回 ack；单测覆盖重复提交和取消。（2026-09-24）
- [x] sequence 定义为 session 级；snapshot 返回原子序列切点，delta 带 sequence；engine 单测覆盖恢复顺序。（2026-09-24）
- [~] WebSocket 已映射共享 envelope；Desktop host 已导出同一 engine 类型并有 dispatch 测试，Tauri IPC adapter 尚待 M0 Desktop 实现。
- [x] 建立 Rust→TypeScript 协议生成与漂移 CI：`chaos-engine` 是唯一 source，`apps/chaos-ui/src/generated/protocol.ts` 由 `chaos-protocol-schema` 生成，CI `gui` job 用 `check-gui-protocol.sh` 比对；React reducer 使用生成的 `ServerMessage`/`TimelineMessage` 类型。（2026-09-24；生成命令、drift check、typecheck、Vitest 和 build 通过）

### M0.3 最小模型配置

- [x] 提供 deterministic engine responder 作为开发测试 provider/mock，CI 不依赖真实云端凭据；真实 headless Agent adapter 仍待接入。（2026-09-24）
- [~] 提供最小 Provider、Base URL、model slug 和 API Key 配置路径；M0 adapter 复用现有 headless 配置边界，GUI 配置表单和凭据存储留至 M3。
- [x] 当前 GUI protocol 不接受 API Key，Web token 仅使用 Authorization header，engine 不记录或返回凭据；真实 provider credential storage 待 M3。（2026-09-24）
- [~] 空模型目录、无凭据、401/429/5xx、网络断开均有可操作错误提示；Web adapter 会把 headless 启动/JSON/非零错误映射为 `agent_failed`，provider 专项错误仍待真实 provider adapter。

### M0.4 前端最小闭环

- [~] React 已接入真实 WebSocket，支持会话创建、纯文本 composer、时间线、流式 delta、审批/question 卡片和停止按钮；会话列表与多会话工作区待补。（2026-09-24；`src/session.ts` reducer 与 Vitest 覆盖事件投影）
- [x] React 已有连接中/已连接、空态、生成中、连接错误和取消入口；WebSocket 断线自动重连并通过 resume 恢复历史。（2026-09-24；typecheck/build/Vitest 通过）
- [~] 流式更新当前按 WebSocket delta 逐事件更新；UTF-8 安全分块与真实 WebSocket 测试已完成，按帧批处理和 benchmark 尚待 M5 性能门禁。
- [~] Web 使用显式 WebSocket transport；Desktop 已有 engine host boundary，Tauri transport injection 待补。

### M0.5 Web 基础安全

- [x] 默认仅绑定 `127.0.0.1`；当前 Web host 无非回环绑定入口，后续公网部署必须另立安全门禁。（2026-09-24）
- [~] 实现 Token 和常量时间比较；当前支持通过 `CHAOS_WEB_TOKEN` 配置 bearer token，Token 不接受 query 参数；`CHAOS_SAFE_WEB_MODE` 已由 WebSocket 后端强制阻断 mutation；高熵生成/轮换和完整部署模式仍待完成。（2026-09-24）
- [x] 校验 Origin/Host，设置 CSP、frame policy、`nosniff` 和 64 KiB HTTP/WS 消息上限；公网部署模式门禁仍需在公网能力启用前补齐。（2026-09-24；Web tests）
- [x] WebSocket 握手和 HTTP API 使用同一 bearer/Origin 策略；单测覆盖未授权、错误 Origin 和安全响应头。（2026-09-24）
- [x] M0 Web 仅提供会话 handshake/create/WS 路由，不暴露命令执行和任意文件写入。（2026-09-24）

### M0.6 验收门禁

- [~] 自动测试覆盖提交成功、取消、重复 submission、断线、重连、snapshot fallback、无凭据和 provider 错误；当前已覆盖真实 WebSocket submit/completed/cancel、重复/dedup、UTF-8 delta、adapter error boundary、跨 engine 重启 resume、认证/Origin/Host 和安全头，浏览器与真实 provider 仍待补齐。（2026-09-24）
- [~] Desktop host 已通过共享 engine dispatch 单测；Tauri 真实入口、平台构建、增量/取消/重启恢复操作仍是 M0 gate。
- [~] 浏览器自动化环境不可用（本机无 Playwright/Chromium）；已用真实 WebSocket 集成测试、React reducer unit tests 和真实服务运行验证协议/认证，浏览器可视交互仍为 M0 gate。
- [~] Linux engine/Web/前端实测已记录；macOS/Windows GUI runner 与 Tauri 冒烟尚待提供。
- [x] 新增 `apps/chaos-ui/README.md`，记录 GUI 启动、`CHAOS_WEB_STATE`、测试和当前 provider/Tauri/远程限制。（2026-09-24）

---

## M1：核心对话、工具与安全审批闭环

**Owner**：TBD  
**目标日期**：TBD  
**依赖**：M0  
**交付目标**：完成核心时间线、工具展示、用户提问、权限审批和 Diff 审查；桌面和 Web 的危险操作都不能绕过审批。

### M1.1 会话投影与交互协议

- [~] 增加 tool start/progress/result、question request/response、file change、usage 和 structured error；M1 engine/WebSocket 已真实覆盖 question、tool started/progress/result/usage、Diff、workspace file changed/write 和 fail-closed errors；完整真实 provider/MCP tool adapter 仍待接入。（2026-09-24；`tests/m1_flow.rs`、`m1-events-verify.log`）
- [~] 已有有界 engine snapshot、session sequence、dedup 和 resume；客户端 cursor、追赶、TTL/容量预算与 snapshot/delta 竞争测试仍待补齐。
- [x] 定义命令/交互在重复投递和 engine 重启时的状态机：client message 去重、question/approval 单次 resolve、snapshot resume 已有测试；超时/断线中的真实 Agent 状态仍待补齐。（2026-09-24）
- [~] 覆盖重复与恢复；真实 WebSocket question、工具审批、Diff 和 workspace 流程及 React reducer 事件投影均有测试，乱序、丢帧、snapshot/delta 竞争及多标签订阅测试待 Web client/真实浏览器阶段补齐。（2026-09-24；`m1-events-verify.log`）

### M1.2 对话 UI

当前状态：`apps/chaos-ui/src/session.ts` 已提供真实 WebSocket 事件投影，覆盖 timeline、streaming、approval、question、completion/cancel 和 snapshot；虚拟滚动、Markdown/代码块、reasoning 折叠、完整错误恢复和稳定 test-id 仍待后续实现。

- [ ] 移植或重写时间线、轮次分组、虚拟滚动、滚动锚点和行高缓存。
- [ ] 实现 Markdown、代码块、reasoning 折叠、工具卡片和错误恢复。
- [ ] 实现 composer 基础增强：多行、历史草稿、发送快捷键、`/` 命令；`@` 文件可延至 M2。
- [ ] 为关键元素建立稳定 test-id，不盲目复制来源项目的全部测试 ID。

### M1.3 权限、提问与审计

- [~] engine 已支持 ToolAdapter 边界：审批通过才执行、无 adapter 安全失败、执行结果写入 timeline/audit；真实命令、写文件、网络、MCP tool adapter 和通用提问对话框仍待补。（2026-09-24；engine tests）
- [x] engine 审批记录绑定 session、tool、参数摘要、UUID request ID、结果和序列；ToolAdapter 只在批准后调用，测试覆盖执行、拒绝和无 adapter fail-closed。（2026-09-24）
- [~] WebSocket 工具审批真实集成测试已覆盖允许、拒绝、重复 question resolve、无 Diff adapter 结构化失败和 ordered ack/resolution/audit；记住规则、超时、断线中的审批恢复和多客户端竞争仍待补。（2026-09-24；`tests/m1_flow.rs`）
- [~] Web 写操作已有 workspace root confinement、message dedup、Origin/Host/token 校验和 Safe Web Mode 后端 gate；CSRF/重放跨 HTTP 写操作防护、审计日志脱敏轮转仍待完整部署模式。
- [x] Safe Web Mode 已由 WebSocket 后端强制执行：命令、文件写入、Git/Diff mutation、审批执行等 mutation message 在 `CHAOS_SAFE_WEB_MODE` 下直接返回 `safe_web_mode_blocked`；真实 WebSocket 测试覆盖 terminal direct call。（2026-09-24；`safe_mode_flow.rs`）

### M1.4 Diff 闭环

- [~] 建立 `chaos-engine::DiffAdapter` 边界，提供 session-scoped accept/rollback/error 事件；下一步接入 `xai-grok-pager-diff`、`xai-hunk-tracker` 和 workspace RPC 的真实文件实现，当前未伪造磁盘状态。（2026-09-24；engine tests）
- [x] 明确“工具已写盘”与“接受/拒绝 Diff”的真实语义：engine 只在 DiffAdapter 成功后发 `diff_resolved`，无 adapter 或失败发 `diff_failed`，不更新磁盘假象。（2026-09-24）
- [~] 已覆盖 adapter 成功、缺 adapter 和 session 绑定；外部文件修改、部分 hunk、回滚失败和二进制文件仍待真实 workspace adapter。

### M1.5 验收门禁

- [ ] E2E：读取文件 → 请求修改 → 拒绝一次 → 再次批准 → 查看 Diff → 接受/回滚 → 确认磁盘状态。
- [ ] E2E：审批时断网并重连；操作不能重复执行，审批状态必须一致。
- [ ] 同时打开两个 Web 标签，验证同一审批只接受一个终态。
- [ ] 验证 Safe Web Mode 无法通过直接 API/WS 调用绕过。
- [~] Web 真实 WebSocket 工具审批/Diff/question 流程和 React reducer 已测试；Desktop/Tauri 真实操作与浏览器人工验证仍待平台 gate。

---

## M2：本地工作台与持久化

**Owner**：TBD  
**目标日期**：TBD  
**依赖**：M1、ADR-003  
**交付目标**：交付多工作区、文件树、快搜、`@` 文件、终端、Git 和可靠的数据迁移。

### M2.1 工作区与布局

- [ ] 实现工作区新增、切换、重命名、置顶、归档和最近使用列表。
- [ ] 实现标签页、分屏、尺寸和主题持久化；定义损坏布局的安全回退。
- [ ] 多工作区并发使用独立 session/cancellation/resource key，文件锁和 Git 锁行为可测试。

### M2.2 文件、搜索和附件

- [~] 新增受 root confinement 保护的 `WorkspaceAdapter`，WebSocket 已支持 list/read/search 和“提议写入→审批→落盘”协议，越界/未审批写入被拒绝；后续接入现有 workspace RPC 的 range/get/put 能力，禁止复制业务协议。（2026-09-24；workspace engine/WebSocket tests）
- [~] 已实现受限目录列表、文本读取和内容搜索（最多 100 个匹配、单文件 1 MiB）；增量文件树、模糊搜索和 `@` 文件候选待前端/M2 后续。
- [~] 当前普通文本写入限制 1 MiB 并执行 root confinement；新增 attachment filename/content-type/10 MiB allowlist、`.chaos-staging` 分块写入/失败清理 stager，以及 Web/engine BeginAttachment/Chunk/Progress/Cancel/quota 协议测试；真实 staging-to-workspace 审批移动、断线续传和配额跨会话策略仍待补。（2026-09-24；`attachment_protocol.rs`、`attachment-stager-test.log`）
- [~] `WorkspaceAdapter` 对 root 和目标执行 canonicalization 并拒绝越界路径，WebSocket 已有真实越界测试；symlink escape、写入/删除和网络文件系统 fixture 待 M2 完整 adapter。
- [~] Web 外部编辑器打开入口暂未暴露；workspace path/remote path 已区分并默认不生成本机深链接，handler/远程降级待 UI adapter。（2026-09-24）

### M2.3 终端

当前状态：固定 cwd 的 `ProcessTerminalAdapter` 已支持审批后执行、输出上限、退出码和结构化错误；现有 `ptyctl` 已盘点并新增 `PtyCapability` 声明 approval/resize/reconnect/cancel 要求，真实 PTY stdin/stdout/resize/重连/取消 transport 仍待 ptyctl/Tauri/Web adapter。（2026-09-24）

- [~] 新增固定 cwd 的 `ProcessTerminalAdapter` 边界：必须先审批、输出上限、退出码、非零错误和 terminal result 已有 engine tests；Xterm.js/ptyctl 交互 stdin/resize/重连/进程取消仍待真实 PTY adapter。（2026-09-24；`terminal-adapter-test.log`）
- [~] Web terminal 当前不提供任意命令入口；未来 terminal route 必须复用审批和 Safe Web Mode，不能绕过后端策略。（2026-09-24）

### M2.4 Git 与变更审查

当前状态：Git status 与 approval-gated stage/commit/checkout_branch adapter 边界已有 engine tests；真实 workspace mutation、stage/unstage/rollback 状态仍待接入。

- [~] engine 已提供固定 `git -C <canonical-root>` status 和真实 `ProcessGitAdapter`：stage/unstage/commit/checkout_branch/discard 必须先审批，WebSocket 临时 Git 仓库测试验证 stage 结果；仍未开放 push/pull、rollback 和冲突恢复。（2026-09-24；`git_real_flow.rs`、`git-real-flow.log`）
- [ ] push、覆盖性 checkout、discard 等共享/破坏性动作必须二次确认。
- [ ] AI commit message 只是建议，提交前可编辑；失败不丢 staged state。
- [ ] 验证非 Git 工作区、detached HEAD、冲突、无 remote 和认证失败。

### M2.5 数据持久化与迁移

- [~] `CHAOS_WEB_STATE` 保留为 transitional JSON；`CHAOS_WEB_SQLITE` 现正式选择 `SqliteSessionStore` Engine 入口，支持 schema reject/round-trip 和真实 Web sqlite entry 恢复测试；TUI fixture、多进程/NFS/迁移回滚仍须按 ADR-003 完成。（2026-09-24；`sqlite_entry_flow.rs`）
- [ ] 保留 `$CHAOS_HOME`/`$GROK_HOME` 与旧目录兼容；路径变化必须提供一次性导入和回滚。
- [~] 不宣称“无锁”：`SqliteSessionStore` 复用 `xai-sqlite-journal` 的 WAL/TRUNCATE 与 busy retry policy；NFS/多进程并发策略已有底层 journal 文档，但 GUI 真实并发 fixture 尚待补。
- [ ] 用现有真实会话 fixture 验证 TUI→GUI 读取，以及 GUI 数据不破坏 TUI。
- [~] SQLite store 已覆盖损坏 DB、新 schema、缺父目录和重启恢复；迁移中断/磁盘满/旧版本回退仍需真实 filesystem fault fixture，当前环境不能把普通 tempfile 测试冒充完成。（2026-09-24）

### M2.6 验收门禁

- [ ] E2E：两个工作区间切换，布局、标签页、草稿和会话不串位，重启后恢复。（待多工作区 UI/desktop transport）
- [ ] E2E：终端创建文件，文件树实时更新；搜索、打开、编辑和 Git Diff 状态一致。
- [~] engine `AttachmentStager` 已覆盖分块写入、10 MiB/类型/路径策略和失败清理；真实 WebSocket 上传、取消/进度和最终 staging-to-workspace 审批移动仍待补。（2026-09-24；`attachment-stager-test.log`）
- [~] `SqliteSessionStore` 本地 round-trip/schema reject/既有 journal policy 测试已通过；多进程、网络文件系统 fixture、busy retry 和迁移中断测试仍待真实 filesystem fixture。
- [ ] 桌面宽屏和 Web 窄视口均验证布局；所有共享状态页面执行回归导航。

---

## M3：设置与扩展生态

**Owner**：TBD  
**目标日期**：TBD  
**依赖**：M2  
**交付目标**：提供完整的 Chaos 原生设置，以及 MCP、插件、技能、工作流和子代理的可管理闭环。

### M3.1 设置与凭据

- [~] engine 已提供最小 settings envelope（Base URL/model），严格拒绝非 HTTPS 或含凭据 URL，并永不返回 API Key；完整 Chaos 配置 schema 表单仍待接入现有 config boundary。
- [ ] 覆盖通用、外观、模型、Provider、权限、安全、快捷键、远程和更新。
- [ ] API Key 使用经 M-1 选型的 OS keyring/加密方案；本切片不接触或存储 API Key，完整凭据方案仍待维护线/安全 ADR。
- [~] 已有 Base URL/model 更新校验和错误事件；新增 provider shape validation（HTTPS/no credentials/model length）且明确 `network_not_attempted`，不触碰 API Key；真实 provider 能力/连通性测试、超时/取消和脱敏网络错误待真实 provider adapter。（2026-09-24；engine tests）
- [ ] 配置变更定义即时生效或重启生效，并在 UI 明确提示；错误写入不得破坏旧配置。

### M3.2 MCP、插件与技能

- [~] MCP/插件/技能仍沿用现有 Rust crates，GUI 当前只保留 adapter/approval 边界，尚未宣称完整生态闭环；下一步需把列表、连接状态、来源校验和错误诊断映射到同一 engine audit/protocol。
- [ ] 安装或执行第三方扩展前显示来源和权限；覆盖恶意 manifest、路径穿越和签名/校验失败。

### M3.3 工作流与子代理

- [ ] 展示工作流阶段、状态、通知、产出物和取消操作。
- [ ] 展示子代理列表、状态和允许暴露的上下文；敏感内容按权限过滤。
- [~] 工作流/子代理暂未暴露 GUI 操作入口；engine 已提供取消、事件序号、恢复和审计基础，真实 workflow/subagent adapter 待后续阶段。
- [ ] 覆盖部分失败、父任务取消、子代理超时和应用重启后的状态。

### M3.4 品牌与本地化

- [ ] 按第 2.1 节替换用户可见品牌，同时运行兼容标识保留清单检查。
- [ ] 完成 zh-CN/en-US 文案、缺键检测、长文本布局和基本无障碍检查。
- [ ] 添加品牌/协议守卫，防止误改 crate 名、wire ID、环境变量和旧路径兼容。

### M3.5 验收门禁

- [ ] E2E：添加 OpenAI-compatible Provider → 测试连接 → 选择模型 → 新会话生效 → 重启后保留。
- [ ] E2E：配置 MCP → 发现工具 → 触发审批 → 执行 → 禁用后不可调用。
- [ ] E2E：安装测试插件/技能、执行测试工作流并观察子代理；取消和失败状态正确。
- [ ] 配置文件损坏、只读、并发修改和凭据不可用均有可恢复行为。

---

## M4：远程工作区 MVP

**Owner**：TBD  
**目标日期**：TBD  
**依赖**：M3、ADR-004、SSH spike 通过  
**交付目标**：根据 ADR-004 交付一种明确、受支持的远程拓扑。首版至少支持远程文件、搜索、Git 和工具执行；不自动承诺远程 Agent 脱机推理或完整交互式 PTY。

### M4.1 远程 transport 与部署

- [~] 新增 `chaos-engine::remote` capability/endpoint boundary：声明 workspace/tool/port-forward 能力，要求 Strict 或有 fingerprint 的 TOFU host-key policy，拒绝 detached Agent；真实 SSH/workspace transport 仍需远端主机与 SSH spike。（2026-09-24；remote unit test）
- [ ] 明确复用 `xai-workspace-server` 的方式；`xai-grok-workspace-daemon` 只承担其已有的生命周期职责。
- [ ] 实现架构/OS 探测、校验和原子部署；二进制放在版本化目录，权限最小化。
- [~] `chaos-engine::remote::RemoteEndpoint` 已提供 capability/host-key policy 校验边界；真实 client/server version negotiation、降级提示和传输仍需 M4 远端实现。
- [ ] 实现心跳、有限重试、取消和明确的连接状态机。

### M4.2 SSH 安全

- [ ] 支持 ADR/spike 已验证的认证方式；未验证的 ProxyCommand 等能力不得宣传。
- [~] remote endpoint 类型已强制 Strict/带 fingerprint 的 TOFU policy，缺 fingerprint 或 detached Agent 会被拒绝；真实 SSH host-key 交换、变更阻断和凭据测试仍待远端 runner。（2026-09-24；`gui-remote-status.md`）
- [ ] 密钥、口令和 Token 不写日志、不同步到远端；Agent forwarding 默认关闭。
- [ ] 远端 server 仅监听 Unix socket、stdio 或回环端口，并使用一次性握手凭据。
- [ ] 对上传目录、daemon 权限、日志、pidfile 和升级回滚做威胁测试。

### M4.3 远程能力

- [ ] 文件树、Range 读取、搜索、写入、Git、Diff 和 Agent 工具经同一远程 workspace 会话执行。
- [ ] 明确本地/远程路径类型，禁止把远程路径传给本机深链接或本地文件 API。
- [ ] 附件上传支持断点/重试或明确从头重传，并有远端清理策略。
- [ ] 若交付交互式 PTY，必须另有 capability、resize、断线和进程所有权测试；否则 UI 明确标记不支持。
- [ ] 若 ADR 选择本地 Agent + 远程工具，明确本地休眠会暂停 Agent，不承诺 detached Agent。
- [ ] 若 ADR 选择远程 Agent，补齐远端 engine、凭据、会话存储、审批和恢复测试后才能开启该功能。

### M4.4 开发端口转发

- [ ] 将“远端服务映射到本地端口”正确建模为 local forwarding；另行定义真正的 remote forwarding。
- [ ] 支持端口冲突检测、随机本地端口、隧道生命周期和 WebSocket 转发。
- [ ] 区分桌面本机浏览器和远程部署的 Web 浏览器；Web 场景不得错误使用浏览器机器的 `localhost`。
- [ ] 预览代理处理 Host、Origin、Cookie、WebSocket 和鉴权；默认不公开暴露。

### M4.5 WSL 与容器

- [ ] 先以 capability/transport adapter 形式完成 WSL spike；通过后再加入受支持矩阵。
- [ ] Docker/Podman 保持 Deferred，除非有独立 owner、威胁模型和验收环境。

### M4.6 验收门禁

- [ ] 在干净 Linux 远端完成部署、版本协商、文件读取、搜索、修改、Git Diff 和工具执行。
- [ ] 验证错误 host key、错误凭据、网络中断、服务器升级失败和磁盘满。
- [ ] 验证远端路径不会被本机 API 误解释，两个远端工作区状态不串位。
- [ ] 启动远端 HTTP/WebSocket 测试服务，通过本地转发访问；关闭会话后端口释放。
- [ ] 对未交付的 PTY/detached Agent 能力，UI、文档和 capability 均明确显示不支持。

---

## M5：生产化、打包与稳定版发布

**Owner**：TBD  
**目标日期**：TBD  
**依赖**：M0～M4；可按产品决定在不启用远程功能时发布本地版  
**交付目标**：三平台桌面安装包、Web/CLI 发行包、升级/回滚、安全和性能门禁达到稳定版标准。

### M5.1 CI

- [~] 独立 `gui` CI 已运行前端 typecheck、Vitest unit 和 production build；format/lint 与生成类型漂移检查待接入 schema/typegen 后补齐。（2026-09-24）
- [~] 独立 `gui` CI 已运行 Rust GUI/Web crates test（workspace pinned toolchain）和前端 typecheck/unit/build；新增 installer signature policy fixture 到 npm-scripts CI，GUI crate fmt/clippy 独立门禁与更细的 job 仍待补齐。（2026-09-24）
- [ ] 复测主 `rust` job 耗时仍在 `timeout-minutes: 60` 内且未因 GUI 增长；超出则先修隔离，不得直接调高 timeout。
- [ ] 增加 Playwright Web E2E；当前环境无可用 Playwright/Chromium，React reducer 与真实 WebSocket 集成测试已作为降级证据；桌面端仍需 Tauri/WebDriver 平台自动化。
- [~] GUI CI 已使用 npm cache 与 `package-lock.json`；Rust cache 由现有 CI 提供，pnpm/sccache 尚未引入，避免没有测量就叠加缓存系统。（2026-09-24）
- [~] 现有 `THIRD-PARTY-NOTICES`、lockfile 和 secret scan 提供基础审查；SBOM、漏洞扫描和自动 license gate 尚待接入工具/CI runner。
- [~] ignored inventory 已修复并纳入 Q4 CSV；稳定版相关 ignored test 的逐项 owner/豁免审查仍未完成，不能作为 release 通过依据。

### M5.2 桌面打包与签名

- [ ] macOS：arm64/x64 或 Universal 构建、entitlements、签名、公证、Gatekeeper 和升级测试。
- [ ] Windows：MSI/NSIS、Authenticode、WebView2 检测、安装/卸载/升级测试。
- [ ] Linux：明确支持 AppImage/deb/rpm 中的实际集合，验证 Wayland/X11 和 WebKitGTK 依赖。
- [ ] 签名证书、secret 名称、轮换、权限和失效处理形成 runbook。

### M5.3 Web/CLI 与更新

- [ ] Web 静态资源嵌入、gzip/br、ETag、SPA fallback 和缓存失效测试通过。
- [ ] 提供 sha256、签名、版本索引和可验证的更新 feed。
- [ ] 自动更新覆盖正常升级、签名失败、下载中断、回滚和旧数据迁移。
- [ ] 保持现有 CLI 发行路径兼容；明确桌面/Web 是否独立版本或 lockstep。

### M5.4 可重复性能门禁

- [ ] 建立固定 benchmark 环境文档：硬件、OS、WebView/浏览器、release profile、采样次数和数据集。
- [ ] 测量冷启动、热启动、空闲 RSS、长会话滚动、150 tps 流、10 万文件搜索和大 Diff。
- [ ] 指标使用 p50/p95 与允许回归比例；首次基线测量后再冻结数值，不预设未经验证的包体或内存承诺。
- [ ] 性能脚本输出机器可读结果并保存 CI artifact；稳定版阻断阈值写入 CI。

### M5.5 最终人工验收

- [ ] 桌面和 Web：新建会话 → Prompt → 流式输出 → 工具 → 审批 → Diff → 终端/Git → 重启恢复。
- [ ] 检查空态、错误态、断线、超长会话、大文件、无权限目录、磁盘满和损坏配置。
- [ ] 检查多个工作区、多个标签页、主题、语言、宽屏和窄视口状态一致性。
- [ ] 检查 macOS、Windows、Linux 的安装、首次启动、升级、回滚和卸载。
- [ ] 检查 Web 回环与非回环模式、Token 轮换、Safe Web Mode、TLS（若支持）和审计日志。
- [ ] 检查远程支持矩阵中的每种认证和故障路径；未支持能力无误导入口。

### M5.6 发布资料

- [ ] 更新 README、用户指南、架构文档、配置参考、故障排查和安全说明。
- [ ] 生成准确的 CHANGELOG、THIRD-PARTY-NOTICES、SBOM 和校验文件。
- [ ] 发布说明列出支持平台、已知限制、数据迁移、回滚方式和 Deferred 项。
- [ ] 发布前召开 go/no-go：P0/P1 为零，所有豁免有 owner 与截止日期。

---

## 5. 协议和数据设计最低要求

这些是实现约束，不另设重复 checkbox。

### 5.1 协议

- Rust 或独立 schema package 是唯一类型来源；TS 类型由其生成，并保留运行时校验。
- 所有 envelope 至少包含 protocol version、message kind、session/subscription ID、message ID 和 payload。
- sequence domain、snapshot boundary、ack、cancel、retry、dedup TTL、retention 和最大消息尺寸必须写入协议文档。
- Tauri IPC、WebSocket 和未来远程 transport 共享逻辑语义，但允许使用各自合适的物理编码。
- 大文件、附件和媒体不进入普通事件队列；使用 Range/stream/upload 通道。
- 未识别 capability 必须安全降级，不能静默执行近似动作。

### 5.2 数据

- 不创建第二套相互竞争的会话事实源。
- 所有 schema migration 单向、可测试，并在变更前创建可恢复备份。
- 新版本数据库被旧版本打开时必须只读或明确拒绝，不得降级写入。
- WAL 只用于支持它的本地文件系统；遵循 `xai-sqlite-journal` 的 NFS 防护。
- GUI/TUI 并发访问需要真实多进程测试，不使用“无锁互通”作为设计假设。

### 5.3 安全

- 浏览器提交的路径、命令、tool ID、审批结论和 capability 均视为不可信。
- 前端隐藏按钮不是权限控制；所有限制由 Rust 后端强制执行。
- 凭据只在需要它的进程/请求中可见，日志、事件、诊断包和前端状态必须脱敏。
- 外部 URL、iframe、深链接和预览代理默认不可信；必须防止 SSRF、本地网探测和路径泄露。
- 每次危险操作审批只适用于明确的动作和范围，不跨会话自动扩张。

---

## 6. 后续候选，不阻断首个稳定版

以下项目只有在新增 owner、ADR、威胁模型和独立验收标准后才能进入执行路线图：

- 完整远程交互式 PTY；
- 远程 Agent 脱机运行与 reattach；
- Docker/Podman 执行目标；
- 内嵌浏览器、CDP、元素拾取；
- 会话云分享和配置云同步；
- 用量/费用中心；
- Whiteboard、Treemapping、Model Trajectory；
- Computer Use；
- 语音输入；
- 自动化和闲时任务；
- 正式移动 Web 支持。

---

# 7. 维护线：现有 TUI 分叉的在途工作

GUI 线从零起步，维护线管的是**已经有用户在跑的 `chaos` 二进制**。下列任务来自
`docs/` 与 `sync/` 已有的记录，之前散落在各份报告里、没有统一的勾选入口，这里
合并为可排期的条目。每条都标注了**当前实测状态**（核对于 2026-09-22，版本
`0.4.0`，分支 `sync/curated-port-20260918`）。

| 编号 | 主题 | 阻断级别 | 依据文件 |
|---|---|---|---|
| MT-1 | 发布链路正确性 | P1（阻断下次发版） | `sync/fork-layer-inventory.md` §6 |
| MT-2 | 自更新签名收尾 | P1 | `docs/audit-followup-report.md` §4 |
| MT-3 | 工作树在途改动落地 | P2 | `git status` |
| MT-4 | 界面文案中文化（指南之外） | P2 | `sync/doc-l10n-progress.md` §7 |
| MT-5 | 测试债与 ignored 台账 | P2 | `docs/ci-test-debt.md`、`docs/ignored-audit-2026q3.md` |
| MT-6 | unsafe / unwrap 收敛 | P2 | `docs/audit-followup-report.md` §1、§2、§5 |
| MT-7 | 上游同步节奏与仓库卫生 | P3 | `.agents/skills/chaos-upstream-sync/`、`SOURCE_REV` |

---

## MT-1：发布链路正确性（下次发版前必须清零）

**Owner**：TBD  
**目标日期**：下一次 tag 之前  
**阻断级别**：P1

npm 元包 `crates/codegen/xai-grok-pager/npm/chaos/package.json` 的版本已经是
`0.4.0`，但 `optionalDependencies` 里六个平台包仍钉在 `0.2.121`；而
`scripts/assemble-platform-packages.js` 打包时会把平台包版本盖成元包版本。两者
对不上，`npm install chaos-code@0.4.0` 会去请求一个从未发布过的平台包版本，安装
直接失败。

- [x] 让 `optionalDependencies` 的六个平台包版本随元包版本走：`b7947cda`（2026-09-22）按
  `stamp-npm-version.mjs` 的口径把元包 + 六个平台包 + 六条钉版一起盖到 `0.4.0`。
  **仍未做到的**：值仍是手写常量，脚本只在发布时盖一遍。
- [x] 复核 `scripts/ci/publish-npm.sh` 与 `stamp-npm-version.mjs` 是否还有第二处版本来源：
  没有。`stamp-npm-version.mjs` 只吃一个 semver 参数，元包 `version`、六个
  `optionalDependencies` 钉版、六个平台包 `version` 全由它推出；`publish-npm.sh` 里没有
  任何版本字面量；`release.yml` 两处（Stamp npm versions / Assemble）都取
  `needs.resolve-version.outputs.version`。**结论：发布链路的唯一来源是 tag，仓库里的
  package.json 只是它的快照——快照会漂，0.2.121 那次就是。**
- [x] 加一条 CI 检查：仓库里的 npm 快照（元包版本 = 六个平台包版本 = 六个钉版）与工作区
  Cargo 版本一致时才算过，不一致就构建失败。**2026-09-23 完成（`db30c56e`）**：新增
  `scripts/ci/check-versions.sh`，接进 `ci.yml` 的 `npm-scripts` job。脚本从
  `crates/codegen/xai-grok-pager-bin/Cargo.toml` 的 `[package]` 段取版本（正则限定在
  该段内，避免抓到别的 `version =`），要求元包、六个平台包、以及六条
  `optionalDependencies` 钉版全部相等；平台包由**磁盘目录发现**，再拿各自
  `package.json` 的 `name` 字段（不是目录名）与声明的键比对。顺带提：`ci.yml` 原来只做
  `stamp-npm-version.mjs 0.0.0-ci` 的 dry run + `git checkout -- npm` 还原，等于只验脚本
  能跑，不验快照对不对——现在两者都验。
  **负向验证**：把 Cargo.toml 临时改成 `9.9.9`，脚本输出 13 行 MISMATCH、退出 1，改回后
  输出 `check-versions: OK — Cargo and all npm packages agree on 0.4.0`。
- [x] **同批顺手清掉工具链漂移**（原列在 MT-4 的「未改 CI 配置」里，属同一类「两处真相」）：
  `ci.yml` 的 `env.RUST_TOOLCHAIN: 1.92.0` 与 `release.yml` 的 `1.94.0`、以及
  `rust-toolchain.toml` 的 `channel = "1.94.0"` 三份互不一致。改为**单一来源**：两个
  workflow 各加一步 `Read pinned toolchain`，从 `rust-toolchain.toml` 用 `sed` 抽出
  `channel` 写进 `$GITHUB_OUTPUT`，`dtolnay/rust-toolchain` 的 `toolchain:` 改吃该输出；
  两个 `env.RUST_TOOLCHAIN` 删除。**验证**：两个 workflow 都能被 YAML 解析；`sed` 抽取
  结果为 `1.94.0`；除一处解释性注释外仓库内再无 `RUST_TOOLCHAIN` 引用。此后升级工具链
  只需改 `rust-toolchain.toml` 一个文件。
- [x] **修复 npm 发布假成功与不完整元包风险**（本轮）：真实根因已从 `v0.3.1` Actions 日志确认——npm 对首个平台包返回 `E404`，脚本明确退出 1，但 workflow 的 `continue-on-error: true` 把步骤改成 `success`，导致 release package job 仍绿。已移除 `continue-on-error`，缺少 `NPM_TOKEN` 时明确失败；发布脚本对每个包执行 registry read-after-write 核验；只在六个平台归档都有效时发布元包，partial opt-in 只发布平台包。新增 `scripts/ci/test-publish-npm.sh` 覆盖空目录、`.gitkeep`、不完整集合、partial mode、完整集合及 registry 假阳性；CI 已接入。**新发现的硬阻塞**：npm 上 `chaos-code-win32-arm64` 与 `chaos-code-win32-x64` 是 `0.0.1-security` 占位包；要继续 npm 全平台发布，必须先由维护者通过 npm 支持取回包名，或选择新包名并迁移 pins。当前需要用户/包所有者介入，不能由我安全地擅自改品牌包名。已将 tag 发布默认改为 GitHub Release only；仓库变量 `CHAOS_NPM_PUBLISH_ENABLED=true` 显式启用 npm 后才会做占位探测与发布。npm 问题已隔离，不再阻塞二进制发版。
- [ ] 在干净环境里实测 `npm install`（至少 linux-x64 一条路径）并跑通 `chaos --version`。—— 当前 Windows npm 包名仍为 `0.0.1-security` 占位，七个包不能安全完成真实安装发布；保留为 npm 所有者介入后的 release gate。

**2026-09-24 复核与版本决策**：Cargo/npm 仍为 `0.4.2`，仓库已有 `v0.4.2` tag。release workflow 已强制签名/`require-sig`，安装器也默认 fail-closed；但 signing preflight 尚未在真实 release dispatch 成功验证 GitHub secrets 公私钥匹配，且 Windows installer runner 未通过。MT-1 的七个 npm 包仍受 Windows `0.0.1-security` 占位包阻塞。故不能创建 `0.4.3` tag 或 Release。

**验收证据**：CI 检查的 PR、一次真实的 `npm install` 输出。

---

## MT-2：自更新签名收尾

**Owner**：发布负责人（需在开工前指派具体维护者）  
**阻断级别**：P1（涉及供应链完整性）

`docs/audit-followup-report.md` §4 已于 2026-09-23 按实际代码与仓库配置状态更新。本项仍未清零：当前 release 可在无签名时发布，updater 默认不强制验签，Windows 安装脚本在依赖或签名缺失时会跳过验证。

- [x] 更新 `docs/audit-followup-report.md` §4，记录签名接线、降级路径和未配置密钥时的真实状态。（2026-09-23；证据：本次复核）
- [~] GitHub 配置项名称 `CHAOS_SIGNING_PRIVATE_KEY` / `CHAOS_SIGNING_PUBLIC_KEY` 已存在；本次只检查名称，未读取值。`signing-preflight` 会在真实 release workflow 中核验长度和公私钥匹配；在该 preflight 运行成功前不能创建补丁 tag。（2026-09-24）
- [x] release workflow 强制要求签名密钥与 `require-sig` feature；新增 `signing-preflight` 校验 secret/variable 非空、Ed25519 公私钥匹配，配置缺失或不匹配时在构建前阻断。（2026-09-24；workflow shell/YAML 检查通过）
- [x] Unix/PowerShell/batch installers 默认 fail-closed；新增 `scripts/ci/test-installer-signature-policy.py` 并接入 CI，缺少 signature/public key/cryptography 会失败，仅 `CHAOS_SKIP_SIGNATURE=1` 显式 opt-out。（2026-09-24；fixture 和 bash -n 通过；Windows runner 仍待运行）
- [~] Unix、PowerShell 与 batch 安装路径已统一为默认 fail-closed：缺 sidecar、公钥或 cryptography 均失败，只有显式 `CHAOS_SKIP_SIGNATURE=1` 绕过；结构 fixture 与 Unix 语法测试通过，Windows runner 实测仍待平台 gate。（2026-09-24）
- [~] updater 现有单测覆盖有效/篡改/错误公钥/缺 sidecar；新增 `scripts/ci/test-installer-signature-policy.py` 锁住三安装器 fail-closed 结构，真实签名安装成功/错误/缺签名与 Windows PowerShell runner 仍待具备真实 release asset 的外部 gate。
- [ ] 完成以上项后运行真实 release dry-run，确认各平台签名 sidecar 与编译内公钥匹配，再允许创建补丁版本 tag。

**本轮不代建或代存私钥**：该操作需要维护者控制的密钥生成环境和 GitHub secret 权限。

---

## MT-3：工作树在途改动落地

**Owner**：TBD  
**阻断级别**：P2

当前工作树有三个文件未提交，都是快捷键/弹窗底栏文案的中文化
（`actions/defaults.rs`、`app/modals.rs`、`views/agents_modal.rs`，合计 75 行改动）。
未提交的改动既不受 CI 保护，也会在下次同步时被冲突淹没。

**2026-09-22 复核**：那三个文件**已经提交**（`96224012`「快捷键详情与弹窗底栏文案改中文」，
`app/modals.rs` 后续又被 `1f1b0b09` 碰过一次）；工作树现在只剩未跟踪的 `.chaos/`（会话上传的
截图，1.2 MB）与 `TODO.md`。本条只剩两个决定项没拍板。

- [x] 跑 `cargo test -p xai-grok-pager --lib -j 2`，确认没有断言还在比对被改掉的英文串。
  （本轮全量基线清账已覆盖 `xai-grok-pager`：`settings_e2e` 279 条、pager lib 相关用例全绿。）
- [x] 跑 `bash scripts/l10n-guard.sh --before main --after WORKTREE`，确认 regressed / shrunk /
  fortress-breach 全为空。（2026-09-22 实测 `--before main --after HEAD` → 0 / 0 / 0。）
- [x] 提交并说明改了哪些面。
- [x] 决定 `sync/2026-09-18-curated-port.md` 是否纳入版本管理：**已纳入**（`sync/` 下 5 个文件
  都在版本控制里）。`TODO.md` 仍未跟踪——若它要继续当"唯一执行清单"，建议跟踪；若要留在本机，
  就把这一条从清单里去掉，别每次复核都重问一遍。
- [x] 将 `.chaos/uploads/` 加入 `.gitignore`：规则 `/.chaos/uploads/` 已存在，且刻意不忽略整个 `.chaos/`，以保留项目配置可入库。（2026-09-23 复核；无需代码修改）
- [x] 根目录调试脚本保持原位：复核确认这些脚本由上游提交 `f380bbca` 引入，移动会造成上游同步冲突；本项取消搬移/删除要求。（2026-09-23 复核；无代码变更）
- [x] 让 `TODO.md` 纳入版本管理，作为本仓库唯一执行清单。（本次提交）

---

## MT-4：界面文案中文化（`docs/user-guide/` 之外）

**Owner**：TBD  
**阻断级别**：P2

26 篇用户指南**已经译完**：`check-doc-l10n.py --english` 为 0 行、`--links` 为 0 条
死锚点、`--cells` 只剩 4 条有意保留的 note（`03` 两条引用界面原文的提醒、`05` 两条
引用按钮原文）。`sync/doc-l10n-progress.md` §2 的"剩余工作量"表是旧数据，已被追平。

真正剩下的是指南目录**之外**的面（口径见该文件 §7）：

- [x] 更新 `sync/doc-l10n-progress.md`，把 §2 的过期统计表替换为当前结论，避免下一个人重做已完成的章节。**2026-09-22 完成**：§二 改为「已清零」+ 四项验收口径（`--english` / `--cells` / `--fork-names` / `--links` 全 0），原表降级为 `d6d4508c` 的历史快照并注明「仅供追溯」；新增 §一之补 列出 `6bf588a7` 之后补齐 11 篇的 13 个提交；§三 标注 1–12 步全部做完。文件头的「统计数字截至 `6bf588a7`」也已改掉——那句话本身就会误导。
- [x] `docs/tutorial/01…09-*.md`：9 篇、约 302 行，**全英文**，一行中文都没有。**2026-09-22 完成（`7813a355`）**：9 篇教程整篇汉化，与 `docs/hooks-and-plugins.md`、`docs/custom-hooks.md` 同批，共 11 篇 376 插入 / 452 删除。注意这 11 篇**不在** `docs/` 根下，而在 `crates/codegen/xai-grok-pager/docs/`（`docs.rs` 的 `REFERENCE_DOCS` 从这里 include），所以 `sync/doc-l10n-progress.md` 与 `check-doc-l10n.py` 的默认 glob 都不会自动看到它们——这也是它们长期"零门禁"的原因。
- [x] `docs/hooks-and-plugins.md`（164 行）与 `docs/custom-hooks.md`（290 行）：`REFERENCE_DOCS` 引用，标题与正文全英文；要么整篇译，要么整篇保持英文，不做"中文标题 + 英文正文"。**2026-09-22 完成（`7813a355`）**：走「整篇译」，标题一并译。同批改了 `docs.rs` 的 10 行（`REFERENCE_DOCS` 的标题字符串）——这一步必须与正文同一次提交，否则 `d` 键打开的弹窗标题与内容语言不一致。
- [x] `src/actions/defaults.rs` 的 `long_help`，以及 `src/views/shortcuts_help.rs` 的 `PASTE_LONG_HELP`。**2026-09-23 复核：这条待办早已作废，属过期条目。**实测 `defaults.rs` 的 `long_help` 是 **41 条 `Some(` / 31 条 `None`**（不是 40 条），其中 **41/41 全部为中文**（逐块扫描含 Han 字符，0 条英文）；`shortcuts_help.rs:88-101` 的三份 `PASTE_LONG_HELP`（Windows / macOS / 其它）也已是中文。此条当初记的是「上游英文原文未译」，而本仓库早在更早的提交里译完了——**留在这里会让人重做已完成的事**，故标为完成。教训同 `doc-l10n-progress.md`：**待办清单里的"未做"必须每次核实，不能继承**。
- [~] 弹窗底栏、toast、错误提示、CLI 用法串等代码侧文案（`sync/2026-09-18-curated-port.md` §6 明确未纳入上一轮，规模大于指南正文，需单独排期）。
  - **CLI 用法串：已清零（`3fb82749` + `74f95a8c`）**。两层都做了：`app/cli.rs` 顶层（161 条 doc comment / 203 行 `///`，含 `--help` 的 `about` / `long_about` / 各 `--flag` 说明）与 11 个子命令模块（119 条 doc comment + 2 处属性字符串：`disk_usage_cmd` 的 `after_help`、`mcp_cmd` 的 `ADD_AFTER_HELP`）。**`help_template` 逐字节未动**（`diff` 验证 `TEMPLATE_IDENTICAL`），`Arguments:` / `Options:` / `Commands:` 三个结构标签仍为英文——这是**有意决定**，理由见下条。
  - **翻译顺带暴露了三处 HEAD 就存在的过期文案（已修，同一提交 `3fb82749`）**：`Logout` 原写「退出登录并清除缓存的凭据」，但 `xai-grok-pager-bin/src/main.rs:2234` 只打印「Chaos 使用自带模型凭证（config.toml），没有需要登出的会话。」就返回，**不清任何东西**；`Login` 的 `--oauth` / `--device-auth` 仍在宣传已被本分支移除的登录流程，而 `Command::Login { .. }`（`main.rs:2240`）忽略全部 flag。英文原文本来就是错的（"Sign out and clear cached credentials" / "Sign in to Grok" / "Use Grok OAuth via auth.x.ai"），**忠实翻译只是让错误在中文里第一次可见**——这正是"翻译即审查"的价值。三处已按实际行为改写（`Logout` → 「说明无需登出」、`Login` → 「请在 config.toml 中设置」、两个 flag 标注「已忽略（为向后兼容保留）」）。**做法上仍遵守既定口径：指向已删除功能的认证文案应当删/改写，绝不改名。**
  - **本轮同批完成的其它用户可见面（不在原清单里）**：`xai-grok-pager/npm/**` 七份 README（元包 + 六个平台包，`e4184c95`）、`xai-grok-pager/README.md`（`0d00fe4f`）、随二进制落地的 `xai-grok-shell/README.md` 全文（61% 重写，`5a6f9785`）。这三份都是**会被用户读到的文件**（npm 页面 / crate 页面 / `~/.chaos/README.md`），与指南正文同等重要。
  - **仍未做**：弹窗底栏、toast、错误提示（非 CLI 帮助类的短文案）。这些散落在大量文件里、且多为断言目标，应作为独立批次，先建"用户可见发射点"清单再动。
  - **已修一处（`c5f583e0`）**：`xai-grok-pager-render/src/util.rs` 的 `display_grok_home_prefix_for` 在「传入的就是默认目录」分支返回写死的 `"~/.grok"`，而本仓库默认目录早已是 `~/.chaos`（`~/.grok` 只作既有旧目录继续双读）。判据不是"哪个名字好看"，而是用户手册第 17 章**已经**把这行输出记作 `Disk usage for ~/.chaos`、样例 worktree 行写作 `~/.chaos/worktrees/...`——即代码与文档不一致，既定行为在文档那侧。**同一实现里原本有两份标签且已经分叉**：`xai-grok-config::paths::default_home_display_prefix()` 早就是对的，`xai-grok-pager-bin/src/main.rs` 的三处用户可见文案（`chaos setup` 引导、`chaos workspace` 与 Dashboard 报错）在用，于是同一进程里一边打印 `~/.chaos`、一边打印 `~/.grok`；已改为直接复用该函数，两份实现不可能再分叉。改后实机验证：`chaos du` 输出 `Disk usage for ~/.chaos`，`chaos du --json` 的 `grok_home` 为 `/home/chaos/.chaos`。判断"是不是默认目录"仍用入参 `home`，但**标签只能取自默认目录本身**，否则默认目录的规范形式（软链）会把目标目录名泄漏成 `~/grok-on-disk`（`disk_usage_cmd::tests::symlinked_default_home_keeps_home_label` 守这一点）。同一函数还喂给 `doctor`/`mcp` 的配置路径、`abbreviate_path`（内存路径、复制提示）、扩展与文档弹窗。
  - **`grok <cmd>` 命令名残留：已清零（`1c3e3367` + `bebc8f64` + `3698e5e3`）**。规模已实测（不是 1 行）：真正**用户可见**的发射点 **16 行**——`mcp_cmd.rs` 5（含 `after_help` 示例块另 6 行）、`plugin_cmd.rs`/`trace_cmd.rs`/`wrap_cmd.rs` 各 1、`mcp_doctor.rs` 2、`inspect/mod.rs` 1、`xai-grok-update` 3、`version_policy.rs` 3——再加 clap 的 `name = "grok"`（`app/cli.rs`）与 `app/mod.rs` 里钉住用法串的两处断言。**方向无争议**：分叉层既定策略就是 `chaos`，`MANAGED_NAMESPACE = "chaos doctor"`（写进用户 shell rc 的托管块标记，属磁盘格式）已经改过，`check-doc-l10n.py --fork-names` 也把「`grok` 用作命令」判为残留。**但不要全库扫**：`grok <cmd>` 在 `crates/**/*.rs` 有 2700+ 命中，绝大多数是开发向文档注释（`` `grok update` ``、`` `grok inspect` ``、`` `grok memory doctor` ``）与测试夹具，改它们是纯噪声、且属于"碰架构"。

    **本轮结论：已清零，共 3 个提交、37 个文件。** 最刺眼的一处不在上面那张清单里，而是**程序自称**：`chaos --help` 打印的用法串是 `Usage: grok [OPTIONS] [PROMPT] [COMMAND]`——用户看不到自己的程序名。两层成因：① `app/cli.rs` 的 `#[command(name = "grok", …)]`（clap derive 元数据）；② `parse_cli()` 里 `std::env::args().next().unwrap_or("grok")` 的 argv[0] 兜底 —— **真正每次实机调用走的都是第②层**，第①层只影响 `PagerArgs::command()` 这类单元测试渲染。上一轮 `6bf588a7` 改了 `grok: ` 前缀与 70 处测试 argv[0] 夹具，恰好两个源头都没碰到。已抽出 `program_display_name(argv0)`：只回显本程序**确实安装过的名字**（`chaos` / `agent`），其余（`grok`、`cargo run` 的 crate 路径、构建树路径）一律归一到 `chaos`；`None` 也是 `chaos`。加了 `program_display_name_normalizes_to_an_installed_name` 单测——原来唯一的守卫是 `cli_command_name_is_grok`，它渲染的是 derive、看不见 argv[0] 那条路径。**实机验证**：release 构建后 `chaos --help` 与 `chaos --nonexistent-flag` 都打印 `Usage: chaos …`；把二进制复制到 `/tmp/notchaos-bin`（专门走兜底分支）仍打印 `chaos`；`--version` 为 `chaos 0.4.0 (6f8fb6b6e6ed)`。

    第二遍用全库正则 `(^|[^_a-zA-Z./-])grok (mcp|plugin|wrap|trace|update|setup|doctor|leader|workspace|worktree|export|completions|agent|sessions|login|logout|du|import|model|voice|sandbox|hooks|memory|inspect)([^a-zA-Z-]|$)` 逐条定性，又揪出 4 处**同一类遗漏**（第一遍的 grep 只覆盖了已挑定的文件，所以漏了）：`app/cli.rs` 的 `wrap` 示例、`disk_usage_cmd/mod.rs` 的 `after_help`、`disk_usage_cmd/display.rs` 的补救提示、`worktree_cmd/mod.rs` 的 `--force` 帮助；另有 `startup_failure/render.rs:166`（`grok leader kill`）与 `diagnostics/fix.rs:513`（同文件四个兄弟早就是 `chaos doctor fix …`，只有它一个漏网，属**同屏自相矛盾**，最该修）。教训：**sweep 必须先用正则枚举全集再分类，不能只扫已选定的文件**——第一遍就是这么漏的。

    **判定口径（本轮确立，后续同类改动静按此办）**：`grok` 作为**标识符**（用户要敲的命令、正在跑的进程）→ 改；`Grok` 作为**英文帮助语料里的产品词** → 留；`tracing::*` 日志 → 留（用户不读）；`//` 与内部 `///` 开发注释 → 留；指向**已删除功能**的认证文案 → 删或改写，**绝不改名**。语言上同理：这一批触及的都是英文语料文件，所以**只改名、不翻译**（翻译是 MT-4 的事）。

    顺带暴露两个**守护缺口**，都不是本轮该修的：`plugin_cmd::tests::trust_prompt_marketplace_has_no_error_framing` 断言的是 `  grok plugin install …`，而我第一遍只改了同一个测试里的另一条断言——**全量回归把它抓出来了**，说明「同文件多条断言」也得逐条扫；以及 `cli_command_name_is_grok` 这种「渲染 derive 而非真实 argv[0]」的断言天然看不见第②层成因。
  - **`~/.grok` 路径文案：用户可见面已清零（`3698e5e3`，20 文件）**。原判「另有三处」是**低估**——把「用户可见」当判据扫一遍，实际是 **8 个 crate、11 个发射点**，横跨 pager / shell / sandbox 三层：
    ① `mcp_cmd.rs` 的 `ADD_AFTER_HELP`：**同一行里两个路径、结论相反**。`~/.grok/config.toml` 已改（用户级根目录确定是 `~/.chaos`）；`./.grok/config.toml` **故意保留**——项目作用域 `config.toml` 的实现确实只认 `.grok`（`xai-grok-workspace/src/project_config.rs::find_project_configs_in` 与 `xai-grok-shell/src/util/config/mcp.rs::project_config_path` 都只 `join(".grok")`），所以那句文案**今天是准确的**。要改得先定 §12.2。
    ② `xai-grok-shell/src/session/acp_session_impl/slash_exec.rs:149` 的 `/hooks add` 用法串 → 改用 `default_home_display_prefix()`。
    ③ `xai-grok-shell/src/builtin.rs` 抽出到 home 的内建文档正文 —— **正文已全文汉化（`5a6f9785`）/ 注释已改（`ca69978c`）**，见下。
    ④ `xai-grok-shell/src/claude_import.rs` 的 `Global (…)` 行；⑤ `util/config/mcp.rs` 两处 `See ~/.grok/docs/user-guide/07-mcp-servers.md`；⑥ `mcp_doctor.rs` 的 `ConfigSourceStatus.path`（**Found / NotFound 两处写死**，而同一个函数三行之上刚解析出真实 `grok_home`、同屏的项目行打印的是真路径）；⑦ `config/mod.rs:1914` hook 路径越界提示（同函数已算出 `canonical_home`，却还在文案里写死另一个常量）＋ `config/tests.rs` 两条断言；⑧ `xai-grok-sandbox/src/deny/glob.rs:481`（`see the sandbox guide (…)`）；⑨ `xai-grok-sandbox/src/profiles.rs:513`（自定义 sandbox profile 的报错，提示用户去写 `~/.grok/sandbox.toml`）。pager 侧三处（`import_claude_modal.rs` 的 `Global` 行、`logout.rs`、`toggle_mouse_reporting.rs`）走的是既有且**已感知 `$GROK_HOME`** 的 `display_user_grok_path`。

    **新增的公共设施**：`xai-grok-config::display_home_path(relative)`（`paths.rs`，5 行）。有了它，shell / sandbox 那 8 处不必各自手搓 `format!`。**它是 env-blind 的**——和 `default_home_display_prefix()` 一样忽略 `$CHAOS_HOME`/`$GROK_HOME`，所以头部 doc comment 明确写了这个 caveat，并指向 `xai-grok-pager-render::util::display_user_grok_path`（那个是 env-aware 的，能看见 render crate 的调用方应优先用它）。这是**有意选择而非疏漏**：让 `xai-grok-config` 也感知 `$GROK_HOME` 需要引入 `dunce` 做规范化，会把 `display_grok_home_prefix_for` 已记录的「曾经分叉」隐患复制一份；宁可留一条注释。**待办**：若要彻底消除，应作为独立项统一两条路径（届时 `default_home_display_prefix()` 一并重构）。

    **判据不是「哪个名字好看」**：实机 `~/.chaos/README.md`（112 KB）与 `~/.chaos/docs/user-guide/` 都存在——`docs.rs::extract_user_guide_docs` 与 `builtin.rs::extract_builtin_files` 都是往**解析出的 home** 写，所以提示用户去读 `~/.grok/...` 的话，那个文件在默认配置下**根本不在那儿**。

    **③ 已动，但哈希那条前提查清后不成立（`ca69978c`）**。原担心是：`builtin.rs` 抽出的文档正文里写着 `~/.grok/`，而 `:149` 会**反向重写**这个字符串再去比对 `FORMER_PLATFORM_SKILL_HASHES` 里的 sha256。查清后有两件事与直觉相反：**其一**，`:149` 的 `content.replace(&home_prefix, "~/.grok/")` 与 `:306-307` 的测试夹具处理的是 **platform skill 的 `SKILL.md` 正文**（`help` / `docx` / `pptx` / `xlsx` 等），**与 README 无关**——`BUILTIN_FILES` 只含 `("README.md", include_str!("../README.md"))`，而 README 的哈希从未进过任何常量表（`builtin.rs` 的测试只断言 `read_to_string(README) != "old"`）。所以 ③ 分成了两半：**README 正文**（被 `check-doc-l10n` 之外无门禁，可自由译）与**哈希耦合的注释/常量**（一个字不能动）。**其二**，`ca69978c` 改的是 builtin.rs 的**模块/函数 doc comment**（说抽出到 `~/.grok/`），不是正文——这两处纯注释成本为零，改了不会碰哈希。**结论：README 全文汉化（`5a6f9785`）安全，已落地；`:149` 与 `:306-307` 至今逐字节未动，这是有意为之。** 同理本轮一并改掉的"需先查清解析路径"尾巴：`implementations/grok_build/lsp/mod.rs:99`（用户级 → `display_home_path("lsp.json")`，项目级 `<cwd>/.grok/lsp.json` 保留）、`implementations/grok_build/workflow/mod.rs:29` 与 `implementations/opencode/skill/mod.rs:37`（schemars / 模板常量，编译期字面量不能调函数，直接写 `~/.chaos/`）、`managed_config/response.rs`（`#[error(...)]`，thiserror 不支持调函数）、`agent/auth_method.rs`（改用 `format!` + `display_home_path`）、`voice_probe.rs:155`（该 crate 不依赖 `xai-grok-config`，加依赖属动架构，故用散文写明解析顺序）。
  - **明确不要连带改**：指向**已删除功能**的 `grok login` / `--device-auth` 文案（`auth/device_code.rs`、`workspace/hub_auth/mod.rs`、`voice/auth.rs`、pager 的 ACP 会话提示）。改成 `chaos login` 会指向本仓库不存在的命令，那是另一类缺陷（让用户去跑一个不存在的命令），要么删、要么改写，不要顺手改名。
  - **CLI 子命令的 `about`：已汉化（`74f95a8c` + `3fb82749`）**。原记「约 25 条，需单独拍板：会动到大量 `--help` 快照类断言」。实测**没有一条快照断言**因翻译失败——`--help` 类测试断言的是结构（section 顺序、flag 是否存在、`Usage:` 首行），不断言文案正文；全量测试跑过即证。**只留下一个有意为之的例外**：clap 的**结构标签** `Arguments:` / `Options:` / `Commands:`。这几条不是本仓库的文案，而是 clap 在渲染时写死的英文——顶层 `help_template` 里是字面量，子命令的 `{all-args}` 更是**无法覆盖**（自定义模板**不能省略空段落**，而 `{all-args}` 会把英文标题一并吐出来）。要中文化只有两条路：自己做一套帮助渲染（等于重写 clap 的输出层，属动架构），或逐个 `--help` 手写 —— 都不划算。**决定：顶层的 `Arguments:` / `Options:` / `Commands:` 统一保持英文，与子命令一致；只翻译"我们写的"内容，不翻译"clap 打印的"骨架。** 好处是同屏语言切换点清晰（标签英文、说明中文），且升级 clap 不会让这份决定失效。`app/cli.rs` 的 `help_template` 因此**逐字节未动**（`grep -A 13 'help_template = '` 两侧 diff 为空，另核验无任何 hunk 触及 `help_template|Arguments:|Options:|Commands:`）。
  - **`CHANGELOG.md` 里的历史 `~/.grok` 条目：有意不动**。那是**历史记录**（每个版本当时实际写的是什么），改它等于篡改发布历史。同理 `sync/*.md`、`scripts/check-doc-l10n-selftest.py`、`scripts/doc-span-removals.tsv` 里的上游形式是**夹具/规格**，也不动。
  - **门禁缺口（本轮才暴露）**：`check-doc-l10n.py --fork-names` 的 `DEFAULT_GLOB` 只覆盖用户指南 `.md`，**对 `crates/**/*.rs` 从未扫过**——本轮把它指到 `--glob 'crates/**/*.rs'` 才发现以上全部。若要长期守住代码侧文案，应新增一个**限定在用户可见发射点**（输出宏 / clap 属性 / 托管块标记）的检查器；直接复用现有启发式会淹没在 `.grok` 路径注释里（2727 命中，其中绝大多数无害）。
- [x] 把 `docs/tutorial/` 与 `docs/*.md` 纳入 `check-doc-l10n.py` 的 `--glob` 范围，否则这批文件没有任何门禁。**2026-09-23 完成（`999ebaff`）**：新增 CI 作业 `docs-l10n`，用显式 `--glob 'crates/codegen/xai-grok-pager/docs/**/*.md'`（36 个文件）跑 `--english` / `--fork-names --strict` / `--links` / `--cells --strict`；`workflows-present` 加一行 `grep -q 'docs-l10n'` 防止该作业被静默删除。**为什么是显式 glob 而不是改 `DEFAULT_GLOB`**：实测 `--before main --after HEAD`（连**默认** glob 都是）会报 21 个文件"结构漂移"——那套结构不变式是为「比较相邻两次翻译提交」设计的，`main` 远在本次同步之前，两端不可比。默认 glob 保持 `docs/user-guide/*.md`，避免把翻译时的工具误当合并门禁。**这个门禁第一次运行就抓到了真缺陷**（`962d4bf8` 修的 2 条死锚点 + 4 个漏译标题）——说明它补的正是"参考文档没人读"这个洞。
- [x] 拍板两个悬置项：`persona` 在用户指南第 04 章（角色）与第 16 章（人设）的译名统一；项目作用域 `.chaos` 是否参与 agents/roles/personas/skills 解析。
  - **已拍板并修复（`persona`）**：不是「两种译法选一个」，而是第 04 章译错了。上游把 `persona` 与 `role` 分得很清（第 16 章据此分层：代理 < 人设 < 角色），而第 04 章的 `## Agents and Personas` 被译成「代理与角色」、`Create, edit, and delete personas` 被译成「创建、编辑和删除角色」。代码侧本来就是「人设」（`views/subagent_catalog_pane.rs` 的表头是 `("人设", "persona", …)`）。已按第 16 章口径改掉第 04 章的三处（章节标题、`/personas` 条文、第 25 行对 `/config-agents` 的描述）。
  - **仍待拍板（项目作用域 `.chaos`）**：**建议先不参与**解析。分叉层把用户级 `xai_dirs::grok_home()` 当唯一配置根；引入项目级根目录会带进「谁有权写 `.chaos/`」的信任问题（等价于 `AGENTS.md` 的仓库可控面），而本轮范围是「不加架构」。真要做应作为独立设计项，附「项目根会被 clone 内容污染」的风险评估。

**注意**：`scripts/l10n-guard.sh` 守的是指南目录的结构不变式，**不覆盖**上述这些面；
改 `docs.rs` 目录标题必须与六处 `go_deeper` 同一次提交改完，否则 `d` 键静默失效。

**2026-09-22 本轮收尾门禁（`3698e5e3` 上实跑，全部通过）**：

| 门禁 | 命令 | 结果 |
| --- | --- | --- |
| 中文化硬闸 | `bash scripts/l10n-guard.sh --before main --after HEAD` | `regressed 0 / shrunk 0 / fortress-breach 0` → **PASS**（Han 文件 357 → 372） |
| 格式 | `cargo fmt --all -- --check` | 干净 |
| 静态检查 | `cargo clippy -j 4 --workspace --all-targets --locked -- -D warnings` | 退出 0（`Finished dev profile`） |
| 全量测试 | `RUST_MIN_STACK=8388608 cargo test -j 4 --workspace --locked --no-fail-fast` | **31076 passed / 0 failed / 490 ignored**，`CARGO_TEST_EXIT=0` |
| 密钥扫描 | `bash scripts/ci/secret-scan.sh` | `clean (3729 files)` |

**踩过的坑（写下来免得下一个人重踩）**：第一次把全量测试的结果用
`grep -E '^(test result|…) $'` 过滤，这个正则把 `$` 加在了分组之外，于是
`test result: ok. …` **一行都匹配不到**；脚本又开了 `set -o pipefail`，于是
`$?` 拿到的是 **grep 的退出码 1**，看上去像「测试失败」，实际完全没有测试信息。
全量门禁**必须**把完整输出落盘（`> /tmp/ws-test-full.log 2>&1`）再分析，不要
边跑边用正则截断——否则要么丢信息，要么报假警报。

**2026-09-23 授权变更（用户明确指示）**：「CI 配置、push、tag 也按最佳实践进行 尽量全部
汉化」。此前本节写的「未改 CI 配置 / 未 push / 未打 tag」是**当时的**有意克制，现已由用户
逐项解除。具体落地：

- **CI 配置**：① 工具链单一来源（`db30c56e`，两个 workflow 各加 `Read pinned toolchain`
  从 `rust-toolchain.toml` 抽 `channel`，删掉两处 `env.RUST_TOOLCHAIN`，此前 CI 说 1.92.0、
  toml 说 1.94.0）；② 版本一致性门禁（`db30c56e`，`scripts/ci/check-versions.sh`，
  负向验证过 13 行 MISMATCH + 退出 1）；③ 新增 `docs-l10n` 作业（`999ebaff`，
  见上）。三项都遵循同一条判据：**门禁必须能失败，且失败原因看得见**——所以每项都做了负向验证。
- **push / tag**：见文末「本轮收尾门禁」`v0.4.0` 一节。

**仍未做、且仍必须有意识不做**：不动 `stash@{0}: On main: web-ui-integration-WIP`
（不是本轮的工作，动它可能丢别人的在途改动）；不 `git add -A`、不暂存未跟踪的
`.chaos/` 与 `TODO.md`；不推 `upstream`、不 `--force`；不改 `CHANGELOG.md` 的历史条目
（那是发布历史，改了等于篡改）；不碰 `scripts/check-doc-l10n-selftest.py` 与
`scripts/doc-span-removals.tsv` 里的上游形式（它们是夹具与规格）。

---

## MT-5：测试债与 ignored 台账

**Owner**：TBD  
**阻断级别**：P2  
**下次季度审计**：2026-10（已到期，本条即是执行入口）

**2026-09-24 复核更正**：统计器已修复并重跑 Q4 CSV；现有 434 属性行/218 条无 reason/37 条含 review date 仅为 scanner inventory，逐条 review 仍待维护者排期。Q3 基线与扫描口径不同，不作直接比较。历史错误初扫数字已撤回，说明见 `docs/ignored-audit-2026q4-summary.md`。

同一轮还清掉了一件优先级更高的事：**本轮开始时本地全量 `cargo test --workspace`
有 46 条失败**（分属 shell、五个 prefetch 二进制、`xai-fast-worktree --features
metadata`、`xai-grok-config`、`xai-tool-types`、pager `settings_e2e` 等），逐条定性
后全部处理完毕，现为 0 失败。台账写在 `docs/ci-test-debt.md`（提交 `f10d3d58`），
不是改测试去迁就实现：其中 3 条是**真的产品缺陷**（`ab61a53e`：访问门禁复用了别的
身份的 `allow_access` 判定；远程抓取关闭时延迟工作从不执行；目录重试读了环境
而非传入参数），其余是过期期望与分叉语义。

还发现并修了一类**完全不执行**的守护测试（`#[cfg(all(test, feature = …))]` 而全仓
没有依赖边打开该特性），详见 `docs/ci-test-debt.md` 新增的「一组『从不执行的守护
测试』」一节与 `sync/doc-l10n-progress.md` §六之补。已接回 `config-docs`；同类
残留还有 pager 两条 `local-workspace` 用例未编译（判定见该节表格）。

再往下是三类**只在全量并行下才冒头**的失败，也已修完（`docs/ci-test-debt.md` 的
「全量 test 暴露的四类遗留失败」一节）：

- 两条确定性失败来自拿「资源里没有这条状态」冒充「没报告过」——提醒流水线上任何
  一次工具调用都会经 `get_or_default` 把空状态建出来。上游 `75810042` 早已改成只读
  `is_reported`，本分叉还停在旧写法，且那个 `already_reported` 帮手「问谁就标记谁」，
  等待与断言都自证。已按上游形状改回只读。（`ceaf6abb`）
- steer 缓存与 `recovery::CACHE_EPOCH` 都是**进程全局**的：前者补
  `#[serial_test::serial]`（`cf5bcfc0`）；后者让两条断言容忍外来 heal（`reindex_all` 在
  epoch 变化时本来就**按契约**扣下完成标记并返回 `RunAgain`）（`b6ae7899`）。加压复现
  4/120 → 修后 150/150 全绿。
- `auth::manager::lock` 那条 6819 分之一的 `WouldBlock`：先排除「`join()` 没关掉心跳
  线程那份 dup」（单跑 3000 次 0 失败），再用 `pre_exec` 拉长 fork→exec 窗口稳定复现
  ——别的用例 `Command::spawn` 的 fork 会把当刻开着的锁文件 dup 带进子进程，那份 dup
  压着 flock 到 exec 才随 CLOEXEC 释放。改成有界重试，真泄漏仍会超时失败。（`c9a6e542`）
- `xai-grok-telemetry` 的 `span_profile::tests::nested_timer_folds_under_parent_without_enter`
  是**全量跑才冒头**的那类里最凶的一条：它不定时把整条测试二进制打崩（SIGABRT，
  275 条结果一起丢），单跑 8 次挂 2 次。证据是在 `timer_parents::open` 加的一行诊断
  ——`DIAG open name=parent.work top=false global=true current=false`：父跨度回退取到了
  **另一个用例**线程局部 subscriber 里的 phase span，克隆进自己的 registry 即
  `tried to clone Id(N)`，析构再踩污染锁就成了非展开 panic。修法是让 profile 夹具与
  startup 用例共用那把 `SERIAL` 锁（`b6fdb417`）；紧邻的覆盖断言另给 1ms 采样偏斜界，
  那条改前改后同比例出现，与本轮无关（`13502dc7`）。修前 70 次挂 3 次 → 修后 150 次 0 次。

以上五类与「从不执行的守护测试」一并写进 `docs/ci-test-debt.md`（`4119abe9`、
`c21541cc`），`sync/doc-l10n-progress.md` 的收尾更新见 `ecb3d1a5`。

**仍未修的一条已知偶发（本轮发现，记录不改）**：`xai-grok-pager` 的
`scrollback::text_selection::tests::rtl_override_row_copies_whole_stored_text`
在一次 9193 通过的 pager lib 全量跑里挂了 1 次（`left: Some("/p")` 对
`right: Some("/path/خوب/file")`），**单跑必过**（130/130）。这是**顺序依赖**而非本轮
改动引起：该用例的 `with_rtl_bidi` 只持有文件局部的 `BIDI_TEST_LOCK`，改的却是
**进程全局**的 `RTL_BIDI_ENABLED`（`xai-grok-pager-render/src/render/bidi.rs` 的
`static … AtomicBool`），而生产写入方 `scrollback/state/mod.rs:388`（`set_appearance`）
与 `app/app_view.rs:2252` 可从**别的用例**触达且不取那把锁，于是并发写会在断言中途把
闩锁翻掉。修法要么把闩锁改成线程局部（但渲染可能跨线程，风险大）、要么给所有
`set_appearance` 路径补同一把锁（涉及数百个用例），**收益与风险不成比例，故判定为
既存缺陷、只记录不动**。下次有人碰 `bidi.rs` 时应顺手把它改成显式传入的
布尔参数（而非全局静态），那才是根治。

- [x] 修复 `scripts/ci/ignored-tests.sh`：入口改用 Python 标准库 CSV 输出与 Rust 字符串转义解析；`#[ignore] // 注释` 仍识别为裸属性。四个 fixture 覆盖行尾注释、多行 reason/转义引号、CSV 逗号/引号/换行、空清单。（2026-09-23；`python3 scripts/ci/test-ignored-tests.py`：4 passed）
- [x] 修复后重新生成 `docs/ignored-audit-2026q4.csv`，用标准 CSV reader 回读验证 434 行、5 列；盘点 218 个无理由属性与 37 条含日期 reason。已更新 `docs/ignored-audit-2026q4-summary.md`；Q3 使用不同扫描口径，不作直接差异比较。逐项 review 仍待维护者审查。
- [ ] 复核所有 fork 债务的 `review 2026-10` 到期条目：逐条判定“修复 / 删除 / 续期并写明理由”，不允许无声续期。
- [ ] 裸 `#[ignore]` 治理：先用可靠解析结果确定存量；再建显式存量基线/豁免的 CI 门禁拒绝新增；最后按 crate 分批补准确理由与 review date。不得把行尾注释自动当作属性 reason。
- [ ] `xai-grok-update` 的 wiremock 重写队列：`test_concurrent_*` 系列 8 条是优先项（该 crate 现存 7 个 `#[ignore]`，与文档记的 48 条不符，需要先核对口径）。
- [ ] 维持 `docs/ci-test-debt.md` 的"append-never, remove-only"规则；当前 `--exclude` 列表为空，任何新增都要在 PR 里被显式质询。
- [x] 确认 `registered_features_are_documented` 的 `internal-docs` feature 门控是长期方案还是临时绕过。**结论：是长期方案，且已写清理由。** 该 target `include_str!` 的是 `docs/internal/25-enterprise.md` 与 `docs/internal/22-environment-variables.md`，而 `docs/internal/` **在本仓库全部历史与 `origin/main` 里都不存在**（`git log --all -- crates/codegen/xai-grok-pager/docs/internal/**` 无输出），所以它在 cargo 下**根本无法编译**。`crates/codegen/xai-grok-pager/Cargo.toml` 的 `[[test]]` 条目旁已写明这一点，并给了持有内部文档者的跑法（`--features internal-docs`）。删掉它反而是信息损失：它是唯一把 `FEATURES` 与操作员表格对起来的检查。

---

## MT-6：unsafe / unwrap 收敛

**Owner**：TBD  
**阻断级别**：P2（单条 P0 除外，见下）

`docs/audit-followup-report.md` §5 给了按投入产出比排的顺序，但没有任何勾选入口，
结果是报告写完就停在那里。按原顺序落为任务：

- [ ] B 批 unwrap 治理：`xai-grok-update` 6 个 + `xai-grok-sandbox` 28 个，量小、位置重要。
- [ ] unsafe P0 审计：`xai-grok-sandbox` + `xai-tty-utils`，这两处是安全边界，优先于数量更大的 crate。
- [ ] `xai-grok-shell` 的环境变量 `unsafe` 消除：一次可砍掉该 crate 约 60% 的 unsafe 数量（364 处关键字里 332 个是 `unsafe {}` 块）。
- [ ] A 批 unwrap 治理：`xai-grok-shell` 约 1,270 个，必须分批，且每批要有独立的验收边界。
- [ ] 每批完成后更新审计报告的数字；报告与实测对不上时，以重新统计为准。

---

## MT-7：上游同步节奏与仓库卫生

**Owner**：TBD  
**阻断级别**：P3

- [ ] 确定上游侦察的固定节奏（如每两周一次只读侦察），产出写进 `sync/`；当前 `SOURCE_REV` 停在 `72a61251`，上游 tip 在 `a28ee2b2`，差距只在移植记录里、没有跟踪机制。
- [~] 当前 GUI/TUI 修改已在相关切片执行格式、GUI/engine tests 和文档检查；下一轮真实上游同步仍需按规则运行 `scripts/l10n-guard.sh` 前后对照。（2026-09-24）
- [~] 维持“分叉层内一律不搬”的判定：`sync/fork-layer-inventory.md` 已登记根 `Cargo.toml` 和 GUI fork 区段；每次继续上游同步仍需执行 l10n/fork-layer review。（2026-09-24）
- [x] 根目录六个调试脚本（`capture_listener.py`、`mock_server.py`、`run_mock_server.sh`、`single_mock_server.py`、`test_simple_wb.py`、`test_workbuddy_headers.py`、`test_workbuddy_headers_v2.py`）：**保持原位，不搬也不删**。它们是**上游自己放在仓库根**的（上游提交 `f380bbca`「test(tools): add mock inference server and WorkBuddy header capture scripts」，本地同一提交），不是散落的本地文件；挪进 `scripts/dev/` 会让每次上游同步都在这条路径上冲突，属于"碰架构"。
- [ ] 复核两项已延后的上游变更是否仍应延后：`oniguruma` 2→3、MCP admission 放宽。
- [ ] `docs/telemetry-status-design.md` 是 v0.1 设计草稿、**尚未实现**（实测无 `telemetry status` 子命令）：决定评审通过后开工，还是标记为不做并在文档头部写明。
- [ ] `docs/known-issues/wsl-p9io-crash-20260728.md` 仍是本地存档未外发：决定补齐 Windows 主机信息后上报，还是明确只留档。
- [ ] 在贡献文档里固化 WSL/低内存机器的 `-j 4` 约束（`CARGO_BUILD_JOBS=4`），避免新贡献者用默认并发把机器打爆。

---

## 8. 决策与进度记录模板

开始里程碑时复制并填写：

```md
### Milestone Mx kickoff

- Owner:
- Reviewers:
- Start date:
- Target date:
- Included capabilities:
- Explicitly excluded:
- Required ADRs:
- External prerequisites:
- Rollback/feature flag:
- Test environments:
```

完成任务时在复选项下记录：

```md
  - Evidence: <PR/commit>
  - Tests: `<command>` — <result>
  - Manual verification: <platform, flow, result>
  - Follow-ups: <issue or none>
```

### 8.1 维护线的额外要求

维护线（MT-*）的条目多数来自既有报告，特别容易出现"文档写了、代码早变了"的
漂移。因此每条 MT 任务完成时，除上面的记录外还要：

1. **回写依据文件**：`docs/` 或 `sync/` 里对应的报告必须同步更新，不允许只勾
   `TODO.md`。本次核对就发现三处漂移（签名进度、ignored 数量、中文化剩余量），
   都是报告停更造成的。
2. **数字以实测为准**：引用任何统计数字前先重跑统计命令，并在记录里写明统计
   时间和命令。
3. **触及 TUI 可见面时**跑一次 `scripts/l10n-guard.sh`，把三项结果写进记录。

### 8.2 本文件的核对节奏

- [~] 发版前重跑第 7 章当前实测状态：本轮已核对签名配置名称、npm Windows 占位、安装器策略和 GUI/CI 状态；真实 secret/runner/npm owner 仍需 release 负责人执行。（2026-09-24）
- [ ] 每季度（与 MT-5 的 ignored 审计同批）复核一次 MT 条目是否仍然成立。

---

## 9. 本轮收尾门禁（2026-09-23 实测）

这就是 MT-4「授权变更」里欠下的那张表。**发版前必须全绿**；下表的数字是 2026-09-23 实测
的，运行时代码在 `c5b76e2f`，其后只有版本号/文档/CI 配置的改动（`c14556c7`、`d3651986`），
所以 clippy 与全量测试没有重跑——重跑时按 8.1 的要求**覆盖**这些数字，不要追加。

| 门禁 | 命令 | 结果 |
|---|---|---|
| 中文化不倒退 | `bash scripts/l10n-guard.sh --before main --after HEAD --report /tmp/l10n-final3` | **PASS**：`regressed 0 / shrunk 0 / fortress-breach 0`；含 Han 文件 **357 → 381** |
| 代码格式 | `cargo fmt --all -- --check` | 无输出（`FMT_CLEAN`） |
| 静态检查 | `cargo clippy -j 4 --workspace --all-targets --locked -- -D warnings` | `CLIPPY_EXIT=0`，3m00s 完成 |
| 全量测试 | `RUST_MIN_STACK=8388608 cargo test -j 4 --workspace --locked --no-fail-fast` | `CARGO_TEST_EXIT=0`；339 个测试目标 + 91 个 Doc-tests，**31078 passed / 0 failed / 490 ignored**；完整日志 `/tmp/ws-test-full.log`（34033 行） |
| 版本一致 | `./scripts/ci/check-versions.sh` | `OK — Cargo and all npm packages agree on 0.4.0` |
| 密钥扫描 | `bash scripts/ci/secret-scan.sh` | `clean (3730 files)` |
| 文档中文化（本轮新增） | `python3 scripts/check-doc-l10n.py … --glob 'crates/codegen/xai-grok-pager/docs/**/*.md'`（38 个文件） | `--english` 0 行 / `--fork-names --strict` 0 处 / `--links` 0 断链 / `--cells --strict` 0 prose + 0 short（另有 4 条 note，是已裁定的保留字面量，非待办） |

**关于 `bidi` 的全局状态偶发**：本轮全量测试**没有**复现——`RTL_BIDI_ENABLED` 相关
22 个用例全部 `ok`，唯一的非 `ok` 行是 PTY e2e 的 `ignored`（预期）。但这是**顺序相关
的偶发，不是已修**：修法（把文件级 `BIDI_TEST_LOCK` 换成跨 crate 的全局锁）仍挂在
MT-5 上，别因为这次没翻车就删掉那条待办。

**关于「CI 步骤本地重放」**：新增的 `docs-l10n` 作业在本地用 `set -euo pipefail`
逐条重放过，四项全部退出 0；并做了**负向验证**——临时改坏 `custom-hooks.md` 的一条
锚点，`--links` 立刻报 `1 broken link(s)`、退出 1，还原后回到 0。**能失败的门禁才叫
门禁**，这条判据与 MT-1 的 `check-versions.sh` 负向验证是同一个。

### 9.1 打标与推送：`v0.4.0` 失败 → 改为发 `v0.4.1`

`release.yml` 的触发是 `push: tags: ["v*"]`，仓库既有 tag 到 `v0.3.1` 为止，所以本次
用带 `v` 前缀的附注 tag（不是轻量 tag：发布链路需要一个带作者与日期的物件）。

**已执行**：分支 `sync/curated-port-20260918` 推到 `origin`（只推 `origin`，不推
`upstream`，不 `--force`）；`v0.4.0` 打的是 `c5b76e2f` 并已推送。随后 `v0.4.1`（`d3651986`）尚未打 tag：本轮默认关闭 npm 发布以规避现有 Windows 安全占位包，更新发布门禁后可安全发布 GitHub Release。正式发布会用新补丁版号，不重用已推送标签。

**然后第一次发版就翻车了**，见 9.2。结论是：`v0.4.0` 的 tag 不重指，改为在修好的提交
上发 `v0.4.1`。判据有两条，都不是我现编的：

1. **仓库自己的做法**：四十几轮 release 里从来没有重指过 tag。唯一一次「失败后怎么办」
   的先例是 `v0.2.125`（run#20，失败）→ 紧接着发 `v0.2.126`（run#21，成功）——失败就
   发下一个补丁版。唯一那次 `attempt=2` 是 `v0.3.0` 的**同提交重跑**，不是重指。
2. **已推送的引用不重写**：`v0.4.0` 已经推出去了，改它等于重写远端引用；而这批内容
   本来就没发布出去（npm 上没有、GitHub Release 也没有），所以换个号发是干净的。

### 9.2 事故记录：第一次 `v0.4.0` 为什么没发出去

**症状**：`v0.4.0` 的 tag 推上去之后（run#44），六个构建里**两个 Windows 作业都在第 5 步
`Read pinned toolchain` 就失败**，Linux / macOS 全部正常。

**根因**（我引入的）：`db30c56e` 加的「从 `rust-toolchain.toml` 单点读工具链」步骤用了
`sed` / `test` / `set -euo pipefail`，却**没声明 `shell: bash`**。GitHub 的默认 shell 按
runner 的 OS 挑——Linux 与 macOS 给 bash，**Windows 给 pwsh**，而 pwsh 里没有 `sed`。
`release.yml` 里另外 9 个 `run:` 步骤都写着 `shell: bash`，就这一个漏了。

**为什么没被发现**：`ci.yml` 的 `on.push` 只跟 `main` 与 PR，而且它**只有 ubuntu runner**
——那份拷贝默认就是 bash。于是本地全绿、CI 全绿，缺陷只在发版时才现形，而那时 tag 已经
推出去、`package` 作业又带 `if: always()`，差一点就把一个**缺 Windows 产物的 0.4.0** 发到
npm 和 GitHub Release 上。**教训：只在发版矩阵里才走到的代码路径，等于没有门禁。**

**处置**：
- 立刻 `gh run cancel` 掉 run#44（cancel 让 `build.result == 'cancelled'`，`package` 的
  条件随之不成立；实测 `package` 虽然起来了，但在第 5 步 `Layout release-bins` 就因缺产物
  失败，`Publish to npmjs` 与 `Create GitHub Release` 都是 `skipped`）。**核实过**：npm 上
  没有 0.4.0、GitHub Release 里没有 0.4.0。
- `c14556c7` 补 `shell: bash`（`release.yml` + `ci.yml` 两处），并新增
  `scripts/ci/check-workflow-shells.py` 钉住这类问题，接进 `workflows-present`。
- **在真实 Windows runner 上验证过修复**：另开一次 `workflow_dispatch` 的
  **dry run**（`publish_npm=false`、`create_github_release=false`，不发任何东西），
  Windows x64 作业的第 5 步从 `failure` 变成 `success`。用 dry run 而不是直接推 tag，
  是因为推 tag 会强制 `publish_npm=true` / `create_github_release=true`，没有第二次试错
  的余地。
- `d3651986` 把版本号 0.4.0 → 0.4.1（四个 crate + `Cargo.lock` + npm 七个包 + changelog）。

**下次遇到同类问题的操作顺序**（写下来免得重踩）：先 `gh run cancel`，再核实
`gh release list` 与 `npm view <pkg> versions` 确认没发出去，然后修、**用 dry run 验证**、
最后才推 tag。

### 9.3 顺带发现的既有问题：npm 发布步骤会「假成功」

`v0.3.1` 的 release（run#43）里，代码 11「Publish to npmjs」的结论是 **success**、
代码 13「Create GitHub Release」也是 success，但 **`npm view chaos-code@0.3.1` 是 404**，
`npm view chaos-code versions` 只到 **`0.2.110`** 为止。也就是说发布步骤会报告成功而实际
没有发布（`v0.3.0` 同理）。

这与 MT-1 记的洞是同一个家族：`optionalDependencies` 指向的平台包在 npm 上根本不存在，
`npm install chaos-code@<任何 0.3.x>` 都装不上。**根因已确认并修复（本轮）**：run #43 的
Actions 步骤本身明确以 exit 1 报告 `E404`，但 `.github/workflows/release.yml` 的
`continue-on-error: true` 把失败吞掉，job 仍显示 success。此前“命令退出码为何是 0”的表述
不准确，实为 workflow 配置改写了结论。现在失败会阻断 package job，registry 发布后也会逐包核验；
GitHub Release 因 `always()` 仍可上传。另查到 `chaos-code-win32-arm64` 与 `chaos-code-win32-x64`
实际为 npm 的 `0.0.1-security` 保留占位包，不能按原名发布；这两个名字需要先走 npm 支持流程
处理，或设计新的平台包命名再更新元包 pins。新测试与 CI 门禁见 MT-1。
