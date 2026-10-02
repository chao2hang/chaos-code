# Chaos 执行计划：Desktop / Web 交付 + 现有分叉维护

> **长期全量完成路线**（阶段顺序、验收门槛、外部资源和负责人输入）：[`docs/architecture/todo-completion-roadmap.md`](docs/architecture/todo-completion-roadmap.md)。本文件仍是唯一逐项状态与证据清单；路线图不代表相关功能已经实现或外部门禁已通过。

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
| 设置、Provider、模型 | 已接受的路线图目标，尚未完全支持 | 按 M3 验收 | Provider/keyring 合同未批准前，不启用凭据/网络调用；见 ADR-007 |
| MCP、插件、技能 | 已接受的路线图目标，尚未支持 | 按 M3 验收 | 信任、签名和权限策略获批并经真实执行验收后再开放 |
| 工作流、子代理 | 已接受的路线图目标，尚未支持 | 按 M3 验收 | 真实 lifecycle contract、权限过滤和恢复验收后再开放 |
| 桌面端 | 已接受的 Tauri 路线图目标，尚未支持 | Tauri 与目标 OS 验收后 | 不以共享 Engine host seam 声称桌面已交付 |
| Web 端 | Degraded | M0～M5 | 首发范围仅单用户 loopback；公网/多用户 Unsupported |
| 远程文件/Git/工具执行 | Deferred（首个稳定版） | 独立安全/产品评审后 | ADR-007 门禁满足前不开放远程 transport |
| 远程交互式 PTY | Unsupported（首个稳定版） | 单独产品/安全评审 | 不作为首版功能 |
| 本地断线后远程 Agent 继续推理 | Unsupported（首个稳定版） | 单独产品/安全评审 | 不作为首版功能 |
| 内嵌浏览器/CDP | Deferred | M5 后 | iframe 不能视为等价实现 |
| 分享、配额、云同步、登录墙 | Unsupported（首个稳定版） | 后续产品/隐私/运维评审 | ADR-007；不作为首版功能
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
M-1.4 的 `ADR-002`（headless 下沉）和 M-1.3（`Cargo.toml` 分叉登记）。各子项当前仅部分完成；M-1.6 Actions runner telemetry/历史资源 baseline 仍需 CI maintainer 核验；headless 物理迁移和 ACP glue 需单独兼容/安全回归。GUI packages 已在独立隔离边界接入，不得把后续里程碑误标为全部通过。

---

# 4. GUI 线路线图

## M-1：可行性、合规与架构冻结

**Owner**：Chaos 主线维护者
**目标日期**：2026-10
**前置依赖**：无  
**退出条件**：以下所有任务完成，关键 ADR 被批准，且 M-1.6 的构建与 CI 隔离指标达标；TODO 中仍保留的资源测量/决策项是 M-1 exit gate，不因历史开始实施 M0 而静默视为已通过。

### M-1.1 来源与许可证门禁

- [x] 记录 GUI 来源与资产基线：本阶段采用 clean-room，自有实现不复制参考产品源码或资产；系统字体/CSS 无新增第三方资产。（2026-09-24；`docs/legal/ui-source-baseline.md`）
- [x] 核验当前 GUI 新增范围的许可证与 NOTICE：无复制源码/字体/图标/图片，仅使用仓库既有 Apache-2.0 依赖和系统字体；证据见 `docs/legal/ui-source-baseline.md`。（2026-09-24）
- [x] 逐类审计字体、图标、插画、图片、Office 预览资产和商标；本阶段无新增字体、图标、插画、图片或 Office 资产，品牌资产标为 Replacement/Deferred，清单见 `docs/legal/ui-source-baseline.md`。（2026-09-24）
- [x] 确定文件头、NOTICE、第三方声明和修改记录规则：新 GUI 代码沿用仓库 Apache-2.0 文件许可；仅使用既有 workspace 依赖，未引入复制资产；证据与复核命令见 `docs/legal/ui-source-baseline.md`。（2026-09-24）
- [x] 无法证明可复制的参考 UI 不进入实现；本阶段采用 clean-room 重写，用户可见品牌与资源不复制，后续资产必须重新过许可证门禁。（2026-09-24）

**验收证据**：`docs/legal/ui-source-baseline.md`、资产清单、许可证副本或链接、审批人和日期。

### M-1.2 产品范围冻结

- [x] 为第 2 节每个 TBD 项选择 Parity、Replacement、Degraded、Deferred 或 Unsupported；见 `docs/architecture/m1-scope-matrix.md`、`docs/architecture/adr-007-first-stable-product-scope.md`。本次受托执行把分享/云、远程和公开多用户部署明确排除在首个稳定版；依赖签字/凭据的 Provider、MCP/插件执行及 Tauri 平台支持仍保持未决门禁，未伪称获外部批准。（2026-10-01）
- [x] 明确首个稳定版是单用户本地开发工作台，Web 默认 loopback；公网、多用户、云同步和登录墙不在首版范围。（2026-09-24）
- [x] 固定当前最低平台策略：M0 Linux 本地验收；macOS/Windows 需独立 CI 构建通过后才列为支持平台；WebView/浏览器版本由 M5 CI 固定。（2026-09-24；本轮 Linux Chromium 153 临时 Playwright browser flow 已用于 Web locale/workspace smoke，非正式平台支持声明）
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
- [~] `ADR-005` local Web security baseline frozen, see `docs/architecture/adr-005-web-security.md`; development `/health`/`/api`/`/ws` proxy only targets loopback backend, which enforces Host/Origin/token/Safe Web Mode. Production TLS termination/reverse proxy/token rotation/audit, Tauri IPC, remote links and secret storage remain separate host/security reviews.
- [x] `ADR-006`：前端采用 clean-room React/TypeScript 重写；不复制参考源码/资产，Chaos UI 仅通过版本化 Rust protocol 消费 engine，未来更新以本仓库审查为准。（2026-09-24；`docs/legal/ui-source-baseline.md`、`docs/architecture/adr-006-gui-source.md`）

**ADR 必须回答**：备选方案、选择理由、兼容影响、失败模式、迁移和回滚。

### M-1.5 依赖与平台 spike

- [~] Tauri v2 spike：Desktop host boundary 已隔离且不引入 Tauri 依赖；真实 Tauri 三平台构建待环境依赖与独立 runner，不能以 host crate 通过替代。
- [~] Axum + 静态资源部署：loopback HTTP/WebSocket 已运行，`CHAOS_WEB_ASSETS_DIR` 可选静态目录托管及 SPA fallback 已实现/测试；真实 release binary+Vite dist 的桌面/窄屏 browser flow、资源字节/MIME、missing asset 404、health/API 与 WebSocket session create 已通过。静态目录可通过 `.gz`/`.br` 变体协商压缩；静态文件 SHA-256 ETag/If-None-Match 304 及 SPA HTML no-cache 行为已有真实 handler/browser 回归。前端资产仍未嵌入 binary；CDN cache invalidation 和 CI/release 默认产物接线仍待 M5。（2026-09-30；`xai-grok-web/src/lib.rs`、`apps/chaos-ui/e2e/static-host.pw.ts`）
- [x] Rust→TS：M0 使用 serde JSON envelope；`chaos-protocol-schema` 生成 TypeScript 类型并由 `scripts/ci/check-gui-protocol.sh` 执行漂移门禁，GUI CI 已运行；运行时 schema validation 仍待后续边界。（2026-09-24）
- [ ] M-1/M4 SSH spike remains blocked: no maintainer-selected client/auth scope or approved ProxyCommand trust rules, and this Linux session has no approved SSH host, disposable key/agent or forwarding lab. Local `RemoteEndpoint` tests are not transport evidence; keep M4 implementation/acceptance open pending decision and controlled runner. (2026-09-29; `docs/architecture/adr-004-remote-topology.md`, `docs/architecture/todo-completion-roadmap.md`)
- [~] SQLite migration：M0 JSON transitional；`SqliteSessionStore` 已正式接入 Engine/Web，支持 schema/损坏库/新版本/重启恢复和旧 schema 备份升级；TUI 真实 `summary.json`/`updates.jsonl` 有只读 allowlisted import seam，但完整 ACP 更新转换、多进程/NFS/迁移中断回滚仍待 M2 gate。（2026-09-24；`sqlite_entry_flow.rs`、`tui_import.rs`）
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
- [~] 主 CI `rust` job 不包含 GUI；GUI 和 Chromium 分走独立 jobs，workflow 静态隔离与真实 CI 全绿已验证。2026-09-25 CI run `36094218127` Rust job 53m14s，低于 `timeout-minutes: 60`；但主 job 加 GUI 前后的历史耗时对比及 peak RSS/disk delta 仍需有 Actions telemetry 权限的 maintainer 核对。
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
- [~] 按 `ADR-002` 的结论处理 headless：`chaos-engine::PromptAdapter` / 显式路径 `HeadlessProcessAdapter` 已经 Web adapter tests 验证可接真实 `chaos --headless --output-format json`；物理迁移 `headless.rs` 及 ACP glue 仍待独立安全审计和兼容回归批次，不在本次 Clippy/CI 收尾顺带变更。
- [x] 创建 `xai-grok-desktop` 和 `xai-grok-web`，加入 workspace 末尾的分叉区段；Web 提供 loopback Axum health/handshake，Desktop 提供独立 host boundary。（2026-09-24；Rust check/test 与独立 `ci.yml` GUI job 已加入）Chromium 发现 workspace create 前端在 backend client_msg_id 去重协议下必须先收到 ACK；已补 ACK，再发 workspace/session/registry events，并用真实浏览器流程验证。
- [x] 保证现有 `chaos` TUI/CLI 默认构建和行为不变；GUI crate 独立于 TUI binary，默认 workspace check 不引入 Tauri。（2026-09-24；GUI crate 独立 check 通过；完整 workspace 回归待 M0.6）
- [~] 真实 GUI crate 已通过独立 GUI CI、协议漂移、前端 typecheck/unit/build 和无 Tauri 依赖结构门禁；主 rust job 历史耗时对比与跨 runner 资源指标仍待 CI 维护者提供。（2026-09-24；`.github/workflows/ci.yml`、`check-gui-protocol.sh`）

### M0.2 最小协议

- [x] 实现并版本化 handshake、create/resume session、submission、ack、分块 text delta、completed、error、cancel、snapshot；engine protocol v1 与 WebSocket integration test 已提交。（2026-09-24）
- [x] 每条命令携带 `client_msg_id`；engine 以进程/持久化状态作用域去重，重复命令只返回 ack；单测覆盖重复提交和取消。（2026-09-24）
- [x] sequence 定义为 session 级；snapshot 返回原子序列切点，delta 带 sequence；engine 单测覆盖恢复顺序。（2026-09-24）
- [~] WebSocket 已映射共享 envelope；Desktop host 已导出同一 engine 类型并有 dispatch 测试，Tauri IPC adapter 尚待 M0 Desktop 实现。真实 Chromium workspace flow 曾抓到 workspace-create wire protocol 缺少 Ack：Web Engine 以 client_msg_id 全局去重，少 Ack 会把 switch/session/registry 的第一事件冒充 Ack、令浏览器等待/状态不一致；已按真实操作 flow 修复服务端 ACK/event 次序并复测。
- [x] 建立 Rust→TypeScript 协议生成与漂移 CI：`chaos-engine` 是唯一 source，`apps/chaos-ui/src/generated/protocol.ts` 由 `chaos-protocol-schema` 生成，CI `gui` job 用 `check-gui-protocol.sh` 比对；React reducer 使用生成的 `ServerMessage`/`TimelineMessage` 类型。（2026-09-24；本轮 final `check-gui-protocol.sh`、Vitest、typecheck、build 通过）

### M0.3 最小模型配置

- [x] 提供 deterministic engine responder 作为开发测试 provider/mock，CI 不依赖真实云端凭据；真实 headless Agent adapter 仍待接入。（2026-09-24）
- [~] 提供最小 Provider、Base URL、model slug 和 API Key 配置路径；M0 adapter 复用现有 headless 配置边界，GUI 配置表单和凭据存储留至 M3。
- [x] 当前 GUI protocol 不接受 API Key，Web token 仅使用 Authorization header，engine 不记录或返回凭据；真实 provider credential storage 待 M3。（2026-09-24）
- [~] 空模型目录、无凭据、401/429/5xx、网络断开均有可操作错误提示；Web adapter 会把 headless 启动/JSON/非零错误映射为 `agent_failed`，provider 专项错误仍待真实 provider adapter。

### M0.4 前端最小闭环

- [~] React 已接入真实 WebSocket，支持会话、timeline、streaming、审批/问题、停止和工具活动卡片；Chromium desktop/mobile 验证 workspace isolation、文件流和 composer。E2E runner 自动选择空闲 UI/backend 端口并通过受控 Origin 配置连接；Vite 代理 `/ws`、`/api`、`/health` 到 loopback Web routes，由后端检查 Host/Origin/token/Safe Web Mode。production TLS termination、reverse proxy 和正式部署安全仍待独立 gate。（2026-09-29；`apps/chaos-ui/e2e-runner.mjs`、`e2e/workspace-flow.pw.ts`、`e2e/tool-activity.pw.ts`）
- [x] React 已有连接中/已连接、空态、生成中、连接错误和取消入口；WebSocket 断线自动重连并通过 resume 恢复历史。（2026-09-24；typecheck/build/Vitest 通过）
- [~] 流式文本使用 WebSocket delta 逐事件更新；UTF-8 安全分块和实际 Browser E2E 已完成，React timeline 现以安全 Markdown/GFM parser 显示真实 response。Delta batching semantics (flush/latency) remain undefined; require M5 reproducible p50/p95 benchmarks and target thresholds before implementing any batching.
- [~] Web 已用显式 WebSocket transport 走真实 Engine；Desktop 目前仅 shared-engine host boundary，无 Tauri IPC transport injection。Tauri integration 要在 M0 desktop ADR/三平台启动验证后实现，不把 lib unit test 称真实桌面 flow。（2026-09-25）

### M0.5 Web 基础安全

- [x] 默认仅绑定 `127.0.0.1`；当前 Web host 无非回环绑定入口，后续公网部署必须另立安全门禁。（2026-09-24）
- [~] 实现 Token 和常量时间比较；当前支持通过 `CHAOS_WEB_TOKEN` 配置 bearer token，Token 不接受 query 参数；`CHAOS_SAFE_WEB_MODE` 在 WebSocket dispatch 前拒绝 Git、终端和设置更新等 mutation，真实 WebSocket regression 确认设置更新返回 `safe_web_mode_blocked` 且随后读取值不变；高熵生成/轮换和完整部署模式仍待完成。（2026-09-28；`safe_web_mode_blocks_direct_mutation_protocol_calls`）
- [x] 校验 Origin/Host，protected HTTP routes 要求语法有效且存在的 Host authority，拒绝 userinfo、无效 port 和非 loopback host；HTTP JSON endpoints（含公开 `/health`）设置 CSP、frame policy、`nosniff` 和 64 KiB HTTP/WS 消息上限；真实 handler tests 覆盖缺失/错误 Host 及 `/health` 公开与安全响应头。随机 E2E Origin 仅在 debug build 且显式 `CHAOS_WEB_ALLOW_DYNAMIC_DEV_ORIGIN=1` 时启用，且只接受无路径 loopback HTTP Origin；release/常规配置不扩大 allowlist。真实 release/proxy 公网部署门禁仍未启用。（2026-09-29；Web tests、`approved_dynamic_dev_origin` 单测）
- [x] HTTP `POST /api/sessions` 接受不超过 128 字节的 `client_msg_id` header，并复用 Engine dedup 语义；重复请求只建立一个 session，空/过长/无效 header 回退到新 ID。真实 Axum handler 测试断言首次响应含 SessionCreated、重试返回 Ack 且 workspace/session 状态只创建一次（`session_creation_is_idempotent_for_a_valid_request_id`、`invalid_session_request_id_falls_back_to_a_fresh_id`）。Web host strict Clippy、7 个 lib tests、真实 workspace WebSocket flow、fmt 和 classifier tests 通过；scratch evidence `web-session-create-idempotency-*.log`。（2026-09-28）
- [x] WebSocket 握手和 HTTP API 使用同一 bearer/Origin 策略；单测覆盖未授权、错误 Origin 和安全响应头。（2026-09-24）
- [x] M0 Web 仅提供会话 handshake/create/WS 路由，不暴露命令执行和任意文件写入。（2026-09-24）

### M0.6 验收门禁

本机浏览器路径现有可重复的 Playwright Chromium E2E 与 GUI CI job；本地 desktop/mobile 两个项目各通过（全命令 4 tests passed）。远端已确认 clean-runner 缺 dotslash，且 Git test 未设置隔离 user identity；第三次运行 browser 首次编译超过 180 秒，现加入预编译和较长 timeout，远端 browser 与 GUI CI 已通过；当前 Clippy 修复的完整 Rust gate 已本地验证，commit `c324306f` 的远端 check/strict Clippy 已通过；full test job 暴露 sandbox 自动套接字 deny 在不可读容器运行时路径上的误报，以及 LSP mock push 在受压时快于 pending 标记的时序竞争。现已跳过不可读 endpoint（child network filter 仍负责网络隔离）、使 fixture 等待报告并为 mock analysis 加调度间隔；两个 targeted tests 均通过，完整 workspace 本地重跑已发现 Git invalidation fixture 未隔离全局 ODB permit并过早释放第二个 walk；fixture 改为两个 permit 且在 release 前等待第二 walk 开始，回归通过。workspace fmt/check/strict Clippy/test 全量已全部通过；输出见私有 goal scratch `rust-workspace-final-suite.log`。真实 provider 和 Tauri 桌面入口仍为独立 gate。

- [~] 自动测试覆盖提交成功、取消、重复 submission、断线、重连、snapshot fallback、无凭据和 provider 错误；当前已覆盖真实 WebSocket submit/completed/cancel、重复/dedup、UTF-8 delta、adapter error boundary、跨 engine 重启 resume、认证/Origin/Host 和安全头，浏览器与真实 provider 仍待补齐。（2026-09-24）
- [~] Desktop host 仅通过 shared Engine dispatch unit test；无 Tauri executable/desktop WebView entry point，不能从此 host boundary drive visible path。Tauri build/desktop restart/cancel/streaming 与 macOS/Windows/Linux checks remain gated on dependency/runner availability.
- [~] Repository Playwright Chromium E2E 与 Linux CI 覆盖真实 browser→Vite→WebSocket→Engine flows。Desktop 与 narrow/mobile tests 验证 workspace create/submit/switch/reload/archive transcript isolation、layout restore、health/handshake、empty/cancel、multiline composer、GFM rendering、raw script not executed、external-link rel/target. Frontend checks/build pass. CI install fixes: dotslash/protoc, local Git identity test setup, prebuilt Web host and 35-minute cold browser build allowance. Engine Clippy warnings-as-errors 已修复（跨平台 dunce canonicalization、collapsed if、simplified boolean）；本地 strict workspace Clippy/check/test pass。首次 full workspace rerun 揭示 sandbox runtime socket inaccessible path 被 permission error 阻断及 LSP test pending/report event race、Git gate fixture permit race；已修正路径行为并添加回归/同步 fixtures，最终 full local run pass，CI run `36094218127` 全部 jobs pass。Local proofs `{SCRATCH}/playwright-markdown-final.log`、`frontend-final.log`、`ignored-tests-unit-final.log`、`ignored-baseline-final.log`、`rust-workspace-final-suite.log`、`final-l10n-guard.log`。真实 provider/keyring、Tauri host 和跨平台安装仍依赖平台/credentials。
- [~] Linux engine/Web/前端实测已记录。CI 新增 `platform-tests` job（`macos-14` + `windows-latest`），对 `xai-tty-utils`、`xai-grok-sandbox`、`xai-grok-shell-terminal`、`xai-grok-update`、`xai-grok-tools`、`xai-grok-pager-bin` 跑真实 target-OS 测试；同一 crate 集合同时提供本机入口 `scripts/test-platform.sh` 与 `scripts/test-platform.ps1`（报告 OS/`rustc -V`/CPU/commit 后执行 `cargo test --locked --no-fail-fast`）。本机已用该脚本对 `xai-tty-utils` 实跑：62 lib tests + 2 doctests 全通过；PowerShell 入口通过 PowerShell 解析器语法校验，但其 Windows 实跑结果仍需在 Windows 机器上取得。YAML 与 shell guard 本地校验通过，但 macOS/Windows 远端首轮结果尚未观察到，不得据此声称平台已通过；GUI 与 Tauri 冒烟仍待补。（2026-10-02；`.github/workflows/ci.yml`、`scripts/test-platform.sh`、`scripts/test-platform.ps1`、`CONTRIBUTING.md`）
- [x] 新增 `apps/chaos-ui/README.md`，记录 GUI 启动、`CHAOS_WEB_STATE`、测试和当前 provider/Tauri/远程限制。（2026-09-24）

---

## M1：核心对话、工具与安全审批闭环

**Owner**：TBD  
**目标日期**：TBD  
**依赖**：M0  
**交付目标**：完成核心时间线、工具展示、用户提问、权限审批和 Diff 审查；桌面和 Web 的危险操作都不能绕过审批。

### M1.1 会话投影与交互协议

- [~] Rust protocol/Engine/WebSocket covers actual ToolAdapter start/progress/result/usage, question, file/Diff changes and fail-closed errors with real integration fixtures. Real provider/MCP command execution/tool sourcing is not part of this protocol/UI mock seam and needs approved adapters/credentials.
- [~] Engine 维护 session sequence、dedup 和有界 snapshot；UI reducer 按当前会话投影服务端事件，并忽略来自先前会话的过期消息。sequence 仅附在部分消息上，尚未定义为可连续检测的全事件游标，因此不据此丢弃重复或判断缺口；需先冻结包含每类事件的恢复合同，再实现 delta replay、TTL/容量预算、snapshot/delta 竞争和真实 WS 丢帧恢复。（2026-09-30；`apps/chaos-ui/src/session.ts`、`session.test.ts`、Playwright desktop/mobile 24/24；完整 Rust workspace 31,169 passed/0 failed/490 ignored（使用 CI 要求的 `RUST_MIN_STACK=16777216`；默认测试线程栈会触发既有 `xai-grok-shell` actor test stack overflow）。日志位于私有 goal scratch）
- [x] 定义命令/交互在重复投递和 engine 重启时的状态机：client message 去重、question/approval 单次 resolve、snapshot resume 已有测试；超时/断线中的真实 Agent 状态仍待补齐。（2026-09-24）
- [~] 覆盖重复与恢复；真实 WebSocket question、工具审批、Diff 和 workspace 流程及 React reducer 事件投影均有测试，乱序、丢帧、snapshot/delta 竞争及多标签订阅测试待 Web client/真实浏览器阶段补齐。（2026-09-24；`m1-events-verify.log`）

### M1.2 对话 UI

当前状态：`apps/chaos-ui/src/session.ts` 提供真实 WebSocket timeline/streaming/approval/question/completion/cancel/snapshot 投影；`main.tsx` 以 safe `react-markdown`/GFM 渲染真实 timeline，新增 layout-effect scroll anchoring 在贴底时跟随增量消息、阅读历史时保持滚动位置，并在新 session/workspace 或返回 Chat 时复位到新内容；Playwright desktop/mobile 使用真实 Engine 对话验证贴底、读历史、继续接收和切换页面/会话的锚点行为。虚拟列表、轮次分组、动态行高缓存、reasoning 折叠、分页和大对话性能门槛仍待完成。

- [~] 完整时间线、轮次分组、虚拟滚动、滚动锚点和行高缓存。已交付增量消息视口锚定：贴近底部时跟随新内容，阅读旧消息时追加内容不移动 `scrollTop`，切换 workspace/session 时重置锚点；desktop/390×844 browser 覆盖真实 Engine stream 和工作区会话路径。虚拟列表、轮次模型、动态高度/折叠项 anchor、历史分页、性能基线和行高缓存仍未实现。
- [~] Shipped Web timeline 用 `react-markdown`+remark-gfm 渲染真实 Engine 消息；raw HTML 不产生 DOM，Markdown 图片也渲染为文字而不建立远程 image request，只有 in-page `#`/HTTP(S)/mailto href 可激活，相对文件路径/javascript/protocol-relative/data schemes 均保持 inert text，外站 link `noopener noreferrer` 新 tab。Playwright desktop/mobile 验证 markup/list/script DOM mutation/image 请求阻止以及各类 link policies；审批和问题变化通过独立 `role=status`/`aria-live=polite` 区域播报。当前 scroll anchor slice 验证用户贴底时新响应继续贴底，滚到历史时增量响应保持阅读位置；完整虚拟滚动/动态 row cache/turn grouping/reasoning collapse仍未完成。（2026-09-30；`apps/chaos-ui/e2e/workspace-flow.pw.ts`）
- [~] composer 支持多行输入、Enter 发送、Shift+Enter 换行、上下键历史导航和未发送草稿恢复；IME composition/keyCode 229 安全处理。`/` 命令候选与 `@` 当前目录/已加载搜索结果文件候选现支持上下键循环、Enter 插入、Escape 关闭，并使用 listbox/option 与 active descendant 无障碍语义；桌面/窄屏浏览器覆盖嵌套搜索结果键盘插入。候选仍只是插入建议，不代表生产命令执行或安全的文件上下文附加合同。（2026-09-29；`apps/chaos-ui/src/composer.ts`、`e2e/workspace-flow.pw.ts`）
- [~] Web app shell/workspace/session/composer 有稳定 `data-testid` 和 Playwright desktop/390×844 E2E；同源 Vite `/api`/`health`/`ws` 走真实 Web Engine。浏览器覆盖真实 workspace/session、文件读取/搜索、批准及拒绝文件写入、批准一次性终端/Git stage、工具活动事件、demo approval/question 与 fail-closed 情况；demo prompt trigger 仍非生产工具。safe Markdown、错误态、对比度和键盘候选均有真实浏览器验证；生产工具/Diff 审查表单及 Desktop/Tauri visible flow remain open。（2026-09-29；`apps/chaos-ui/e2e/workspace-flow.pw.ts`、`e2e/approved-workspace-ops.pw.ts`、`e2e/tool-activity.pw.ts`）

### M1.3 权限、提问与审计

- [~] engine 已支持 ToolAdapter 边界：审批通过才执行、无 adapter 安全失败、执行结果写入 timeline/audit；真实命令、写文件、网络、MCP tool adapter 和通用提问对话框仍待补。（2026-09-24；engine tests）
- [x] engine 审批记录绑定 session、tool、参数摘要、UUID request ID、结果和序列；ToolAdapter 只在批准后调用，测试覆盖执行、拒绝和无 adapter fail-closed。（2026-09-24）
- [~] WebSocket 工具审批集成覆盖允许、拒绝、重复 resolve、缺 adapter fail-closed、恢复及双客户端竞争；Playwright desktop/390×844 覆盖 demo protocol 拒绝、无 adapter 失败和提问响应。`tool_started`/`tool_progress`/`tool_result` 现在投影为有界活动卡片（最多 20 条，截断 progress/result 文本），浏览器测试经运行中的应用 WebSocket 入口驱动一整组工具活动事件。生产工具授权、审批超时/记住规则仍待补。真实浏览器回归在同一个 browser context 打开两个标签：首标签经 UI 发起待审批操作，第二标签的原生 WebSocket 通过同源 Vite `/ws` 代理连接同一 Web host/Engine；首标签先通过 UI 允许请求并观察到无 adapter 的 fail-closed 结果后，第二标签再对原 request ID 提交晚到的竞争批准，得到 `approval_not_found`。测试断言 UI 仍保留失败结果，证实共享 Engine 拒绝重复/过期审批；它没有模拟同时到达时的胜出顺序。（2026-09-30；`apps/chaos-ui/e2e/approval-competition.pw.ts`；goal scratch `verification/approval-competition-playwright.log`）（2026-09-30；`apps/chaos-ui/e2e/approval-competition.pw.ts`）（2026-09-29；`approval_resume_flow.rs`、`question_resume_flow.rs`、`approval_competition_flow.rs`、`apps/chaos-ui/e2e/approval-competition.pw.ts`、`apps/chaos-ui/e2e/tool-activity.pw.ts`）
- [~] Web 写操作已有 workspace root confinement、message dedup、Origin/Host/token 校验和 Safe Web Mode 后端 gate；CSRF/重放跨 HTTP 写操作防护、审计日志脱敏轮转仍待完整部署模式。
- [x] Safe Web Mode 已由 WebSocket 后端强制执行：命令、文件写入、Git/Diff mutation、审批执行等 mutation message 在 `CHAOS_SAFE_WEB_MODE` 下直接返回 `safe_web_mode_blocked`；真实 WebSocket 测试覆盖 terminal 和 destructive Git direct call。（2026-09-24；`safe_mode_flow.rs`）

### M1.4 Diff 闭环

- [~] 建立 `chaos-engine::DiffAdapter` 边界，提供 session-scoped preview/accept/rollback/error 事件；WebSocket 已通过真实 workspace fixture 覆盖文件写入拒绝/批准、Diff 预览、接受和回滚，仍待接入 `xai-grok-pager-diff`、`xai-hunk-tracker` 与 workspace RPC 的生产级部分 hunk/二进制实现。（2026-09-24；`workspace_diff_flow.rs`）
- [x] 明确“工具已写盘”与“接受/拒绝 Diff”的真实语义：engine 只在 DiffAdapter 成功后发 `diff_resolved`，无 adapter 或失败发 `diff_failed`，不更新磁盘假象。（2026-09-24）
- [~] 已覆盖 adapter 成功、缺 adapter 和 session 绑定；外部文件修改、部分 hunk、回滚失败和二进制文件仍待真实 workspace adapter。

### M1.5 验收门禁

- [~] E2E：engine/WebSocket 已覆盖读取 workspace 文件 → 请求写入 → 拒绝一次 → 再次批准 → 预览 Diff → 接受 → 回滚并确认磁盘状态；浏览器/桌面人工验收和真实 hunk/二进制场景仍待平台 gate。（2026-09-24；`workspace_diff_flow.rs`）
- [~] WebSocket integration 已在 socket 断开后通过持久化 Engine reopen，恢复待审批 approval/question prompt，并验证批准/回答只完成一次；Playwright desktop/mobile 覆盖真实 Web 页面上的单客户端拒绝与允许后无 adapter 失败、问题回答；真实浏览器同一 Engine 的多标签晚到审批 resolve 已由 `apps/chaos-ui/e2e/approval-competition.pw.ts` 验证，仍未覆盖同时到达时的胜出顺序及完整通知时序。（2026-09-30；`approval_resume_flow.rs`、`question_resume_flow.rs`、`apps/chaos-ui/e2e/workspace-flow.pw.ts`、`apps/chaos-ui/e2e/approval-competition.pw.ts`；goal scratch `verification/approval-competition-playwright.log`）
- [~] WebSocket 双客户端 fixture 已验证同一审批只接受一个终态，第二次 resolve 返回 `approval_not_found`；真实浏览器同一 Engine 多标签晚到竞争已有 `apps/chaos-ui/e2e/approval-competition.pw.ts` 覆盖：首标签允许后，第二标签对同一 request ID 的晚到 resolve 被真实 WebSocket handler 以 `approval_not_found` 拒绝，首标签 UI 保留 fail-closed 结果。该测试不主张模拟并发先后顺序；单页面 allow/reject path 亦由 desktop/mobile Playwright 覆盖。（2026-09-30；goal scratch `verification/approval-competition-playwright.log`）（2026-09-25；`approval_competition_flow.rs`、`apps/chaos-ui/e2e/workspace-flow.pw.ts`）
- [x] Safe Web Mode 已通过真实 WebSocket 直接发送 terminal 和 destructive Git mutation 验证无法绕过；当前 Playwright E2E 未新增 Safe Web Mode UI 场景。（2026-09-24；`safe_mode_flow.rs`）
- [~] Web 真实 WebSocket 工具审批/Diff/question 流程和 React reducer 已测试；Desktop/Tauri 真实操作与浏览器人工验证仍待平台 gate。

---

## M2：本地工作台与持久化

**Owner**：TBD  
**目标日期**：TBD  
**依赖**：M1、ADR-003  
**交付目标**：交付多工作区、文件树、快搜、`@` 文件、终端、Git 和可靠的数据迁移。当前已在 single host-root 下实测浏览器批准文件写入/拒绝、一次性终端与 Git stage；各注册 workspace 独立根和互动 PTY 仍未实现。

### M2.1 工作区与布局

当前状态：engine 已提供 workspace registry 的创建/切换/归档/最近使用和 session 绑定协议，React 已按 workspace 隔离 transcript/session 并持久化有限 layout preferences；但 Engine 文件/Git/terminal adapter 仍绑定单一 host-level root，registry workspace 暂不能配置多根。完整多 workspace 物理隔离、tabs/split 和 Desktop 状态隔离仍待 root mapping/desktop 实现。

- [~] engine 已实现 workspace 创建/切换/归档/最近使用协议，记录每个 workspace 最近 session 并在切换时发该会话 snapshot；创建和切换到空 workspace 自动创建 session，归档活动 workspace 后切回/创建 fallback session，归档 session 的 Resume/Submit/Approve/RespondQuestion/写文件/Git/终端 mutation fail-closed；create/archive/switch 同步 workspace 列表；session 持久化 workspace_id，Resume/Snapshot 拒绝错误 workspace；React 按 workspace 缓存 session、切换时清理旧 transcript 再恢复对应快照；当前 Engine 仍只有一个 host-level `WorkspaceAdapter`/Git root，不能宣称 registry workspace 有各自独立文件根，需设计 host-owned root mapping 并对双 root files/Git/attachments 做真实安全验证后再宣布 multi-project。（2026-09-24；`archive_workspace_flow.rs`、`archive_last_workspace_flow.rs`、`archived_approval_flow.rs`、`workspace_session.rs`、`workspace-ui.test.ts`、`active_workspace_access` probe）
- [~] React UI 已新增版本化 layout persistence helper 和面板/主题/尺寸控件，持久化 sidebar/composer 尺寸、theme 和 panel visibility；损坏 JSON、未知 schema、非法值及 storage denied 安全回退，无法保存时向用户显示提示。Chromium 已操作 theme/panel/size inputs 并 reload，验证 browser storage state 可恢复；键盘可循环选择 slash/@ 建议并通过 Enter 插入、Escape 关闭，desktop/390×844 浏览器覆盖；完整 tabs/split 和 Tauri 桌面人工验收仍待平台。（2026-09-29；`apps/chaos-ui/src/layout.test.ts`、`src/composer.test.ts`、`e2e/workspace-flow.pw.ts`）
- [~] UI registry isolates session/transcript selection only; current Engine binds one host workspace/Git root. Do not claim each listed workspace is a project/filesystem. Multi-root needs a host-owned root-ID map, root provisioning/trust policy and dual temp-repository file/Git/attachment threat fixtures. Browser-provided physical paths stay forbidden。（2026-09-25；source adapter structure、archive/session fixtures）

### M2.2 文件、搜索和附件

- [~] 新增受 root confinement 保护的 `WorkspaceAdapter`，WebSocket 支持 list/read/search 和“提议写入→审批→落盘”。文件编辑 UI 现从真实浏览器发起提议，后端审批通过后写磁盘、由 Engine `file_changed` 事件自动刷新当前展示目录并可重新读回；拒绝审批后重读仍为原内容，越界/未批准写入仍由 Engine 拒绝。真实 Playwright 断言批准写入后客户端从 WebSocket 发出当前 nested 目录的 `list_files` 请求，并重读确认内容已落盘。adapter 仍是单 root host 配置，registry workspace 主要隔离 session；workspace_id→host-owned canonical root mapping、多根安全审核和浏览器物理路径禁止仍未完成。（2026-09-30；`apps/chaos-ui/src/main.tsx`、`src/session.ts`、`e2e/workspace-flow.pw.ts`、workspace Engine/WebSocket tests）
- [~] engine/WebSocket 提供 root-confined 文件列表、文本读取、搜索（最多 100 个命中、单文件 1 MiB），并以有界实际读取限制搜索内存；真实 Engine 与 WebSocket 测试验证目录元数据、空目录、staging 隔离、越界拒绝、超限文件不匹配和常规结果。Web UI 用协议目录元数据呈现路径导航、读取、搜索及加载/空/错状态；active-session 的 `file_changed` write/attachment_write 事件现在仅当 path 的 parent 与当前所列目录匹配时触发 list refresh，拒绝 stale-session/删除/其他目录事件。Reducer 正负例覆盖；桌面和 390×844 移动真实浏览器批准 nested file 写入时观察实际 WebSocket `list_files(relative_path: nested)`，再读盘验证内容。仍是单一 host-configured root；每 workspace 独立 root、多-root 安全审核、模糊搜索未交付。（2026-09-30；`crates/codegen/chaos-engine/src/lib.rs`、`crates/codegen/xai-grok-web/tests/workspace_flow.rs`、`apps/chaos-ui/src/session.test.ts`、`apps/chaos-ui/e2e/workspace-flow.pw.ts`）
- [~] 当前普通文本写入限制 1 MiB 并执行 root confinement；浏览器提供提议写入→展示审批→允许/拒绝→由 Engine `file_changed` 触发当前目录刷新→重新读取的闭环，真实 desktop/390×844 测试确认批准后磁盘内容更新、拒绝后内容保持原样。attachment filename/content-type/10 MiB allowlist 由 shared `AttachmentStager::validate_name_type_size` 同时校验 `ValidateAttachment` 与 `BeginAttachment`，拒绝 `/` 与 `\\` 分隔符并覆盖批准 finalize。真实 Engine protocol tests 验证无效 Base64 chunk 返回 `attachment_chunk_invalid` 后同一 upload 可接收有效 chunk 并报告进度，以及超额 chunk 返回 `attachment_quota_exceeded` 后 upload 被移除、后续 chunk 返回 `attachment_not_found`（`attachment_protocol_keeps_upload_available_after_invalid_base64_chunk`、`attachment_protocol_rejects_over_quota_and_unknown_upload`）。`WorkspaceAdapter` 和 `AttachmentStager` 启动时以 `symlink_metadata` 要求 `.chaos-staging` 是根目录内的真实目录；真实 Unix symlink 指向外部目录时两种 adapter 构造均 fail-closed，且外部目标保持空白（`workspace_and_attachment_stagers_reject_staging_symlink_escape`）；正常 staging 隐藏与附件 WebSocket finalize flow 仍通过。附件 `.chaos-staging` 分块上传、失败清理、Web/engine Begin/Chunk/Progress/Cancel/quota 协议测试，以及 `FinalizeAttachment` 审批门控和 staging-to-workspace 原子落盘均已交付；WebSocket 覆盖拒绝/批准与磁盘状态。Engine 在实际 Begin→Chunk→Finalize→Approve 流程中拒绝将附件目标设为 root 或 nested `.chaos-staging` 内路径，拒绝后同一上传仍可重试到普通 workspace 文件；取消 upload 后 finalize 返回 not-found。chunk 到达期间，真实 WebSocket flow 还直接尝试读取、枚举和搜索 staging 路径，确认暂存字节不能被 workspace API 看到；批准的正常 finalize 仍成功。断线续传和配额跨会话策略仍待补。（2026-09-28；`crates/codegen/chaos-engine/src/lib.rs`、`crates/codegen/chaos-engine/tests/attachment_protocol.rs`、`crates/codegen/xai-grok-web/tests/attachment_flow.rs`、`workspace_flow.rs`；配额拒绝后 upload 不可复用的真实 Engine regression 输出见私有 goal scratch `attachment-quota-retry.log`）
- [~] `WorkspaceAdapter` 对当前 host-configured root 和目标执行 canonicalization 并拒绝越界路径，WebSocket 有真实越界测试；普通 symlink escape 与经真实 Engine session→approval→write 调用链拒绝指向 root 外不存在目标的 dangling symlink 均有回归测试，后者断言返回 `path_escape` 且外部目标未创建（`approved_write_rejects_dangling_symlink_escape`）。registry 还未绑定每 workspace 独立 root；完成 mapping 后需对每个 root 再补 symlink/write/delete 和网络文件系统 fixture。
- [~] Web 外部编辑器打开入口暂未暴露；workspace path/remote path 已区分并默认不生成本机深链接，handler/远程降级待 UI adapter。（2026-09-24）

### M2.3 终端

当前可保证：固定 cwd 的 `ProcessTerminalAdapter` 仅经 approval-gated path 执行并限制输出/报告 exit status；Web host 在明确配置 `CHAOS_WORKSPACE_ROOT` 时，将同一 canonical root 用于文件、Git 和受限终端 adapter（单命令输出最多 256 KiB）。真实浏览器流程经 WebSocket→Engine→子进程批准运行 `pwd` 与 Git stage，并检查固定 cwd、退出状态和 staged status。PTY capability 类型描述 resize/reconnect/cancel 要求。尚无 Web/Tauri stdin+resize+process-lifecycle bridge，故这只是审批后的单命令执行，不是互动 Terminal GUI，不称 `ptyctl` 类型等于实际 transport。

- [~] 新增固定 cwd 的 `ProcessTerminalAdapter` 边界：必须先审批、输出上限、退出码、非零错误和 terminal result 已有 engine tests；真实 adapter regression 独立断言 cwd 等于配置目录，并精确验证输出截断；adapter 并发排空 stdout/stderr、每路最多保留配置字节数，失败且 stdout 为空时返回截断后的 stderr，并通过 `ProcessScope` 登记子进程以支持进程组回收。真实测试覆盖双路各输出 64 KiB 时不死锁、错误码与 stderr fallback、零上限、UTF-8 多字节截断边界。现有真实 PTY close/scope teardown 测试验证杀死后台 job；Linux 检查按 `/proc` 状态排除已被终止但等待 init 回收的 zombie，避免误判为仍在运行。历史 GitHub run `36368578937` 在 workspace tests 并行负载下曾有一次 close-path 异常；其后在 GitHub Actions run `36381870574`（commit `71e3a09f`）中，远端全量 workspace `cargo test`（含 `close_pty_kills_a_background_grandchild` 与 `scope_teardown_kills_a_background_grandchild`，Step 耗时 28 分 31 秒）已完整跑通并通过（`conclusion: success`）。本地单独运行、terminal crate 全量测试（76 passed、0 failed）、连续 25 次独立重跑及完整 workspace 测试均通过。Xterm.js/ptyctl 交互 stdin/resize/重连/进程取消仍待真实 PTY adapter。（2026-09-28；`terminal_process_adapter_runs_in_fixed_cwd_and_truncates_output`、`terminal_process_adapter_drains_both_streams_and_bounds_fallback_output`、`terminal_process_adapter_respects_zero_limit_and_utf8_byte_boundary`、`xai-grok-shell-terminal/src/pty_session.rs`）
- [~] Web terminal 现提供一次性命令入口：只有配置 host workspace root 时装配固定 cwd 的 `ProcessTerminalAdapter`；命令先经审批，Safe Web Mode 拒绝 mutation，单命令输出限制 256 KiB。真实浏览器在 desktop/390×844 验证批准 `pwd` 输出与 workspace root 一致，审批后的 `terminal_result` 不被后续 `approval_resolved` 状态覆盖；无 root 时 adapter 不装配。尚无 stdin/resize/互动 PTY、断线恢复；非 loopback 公共部署不在此能力范围内。（2026-09-29；`crates/codegen/xai-grok-web/src/main.rs`、`src/session.ts`、`apps/chaos-ui/e2e/approved-workspace-ops.pw.ts`）

### M2.4 Git 与变更审查

当前可保证：host-root `git status/stage/unstage/commit/checkout_branch/discard` 是 Engine 审批后的固定 Git adapter；Web Git 面板可查看状态并提出受审批操作。真实 desktop/390×844 browser→Vite→WebSocket→Engine flow 已在隔离临时仓库验证 stage 被批准后状态改变；破坏性操作需要第二次后端确认并在 UI 显示提示。Git adapter 仍是 single host root，完整 Diff/冲突恢复、push/pull 和多根映射未交付。

- [~] Engine 提供固定 `git -C <canonical-root>` status 和真实 `ProcessGitAdapter`：stage/unstage/commit/checkout_branch/discard 必须审批，破坏性操作还需二次确认。Web host 配置 `CHAOS_WORKSPACE_ROOT` 时，Git/终端 adapter 与文件 `WorkspaceAdapter` 共用同一 host root；桌面/移动真实浏览器批准 `git.stage`，并通过 Git 状态确认暂存生效、显示 stage 结果。Reducer 覆盖 operation result 与 approval resolved 消息的到达顺序。仍是单 host root，multi-root mapping、push/pull、rollback 和冲突恢复未开放。（2026-09-29；`e2e/approved-workspace-ops.pw.ts`、`src/session.test.ts`、`git_real_flow.rs`）
- [~] commit/branch checkout/discard destructive operations require two independent backend confirmations; temp repo WebSocket verifies HEAD stays put before second approval and changes only after. React approval card explains the second-confirmation event; UI state now preserves the concrete terminal/Git operation result when the subsequent `approval_resolved` event arrives, with reducer and browser regressions. Backend remains authoritative. No typed confirmation or Diff detail form; push/pull/conflict resolution also unopened. (2026-09-29; `apps/chaos-ui/src/session.ts`, `src/session.test.ts`, `e2e/approved-workspace-ops.pw.ts`)（2026-09-24；`git_confirmation_flow.rs`）
- [~] `ProcessGitAdapter` 不丢 staged state 的失败路径已有临时仓库测试（stage 后无效 commit message）；AI commit-message 建议与 UI 编辑表单尚未实现，当前没有 LLM 自动改写提交信息。（2026-09-24；`tests/git_edges.rs`）
- [~] 已补真实 fixture 覆盖非 Git 工作区、detached HEAD、缺失分支和无效 commit；失败不会丢 staged state。无 remote/认证失败需要远端或凭据环境，冲突恢复仍待 Git fixture。（2026-09-24；`crates/codegen/chaos-engine/tests/git_edges.rs`）

### M2.5 数据持久化与迁移

- [~] `CHAOS_WEB_STATE` 保留为 transitional JSON；`CHAOS_WEB_SQLITE` 现正式选择 `SqliteSessionStore` Engine 入口，支持 schema reject/round-trip、旧 schema 升级备份/恢复、只读 TUI 导入 fixture 和真实 Web sqlite entry 恢复测试；完整 ACP→GUI 转换、多进程/NFS 仍须按 ADR-003 完成。（2026-09-24；engine migration fixture、`tui_import.rs`、`sqlite_entry_flow.rs`）
- [~] TUI 现有 `$CHAOS_HOME`/`$GROK_HOME` 双读仍由 `xai-dirs` 维持，GUI `CHAOS_WEB_SQLITE` 不改写 TUI 根；尚无 GUI SQLite 数据迁移或 home-path 重定位/import/rollback 命令，等产品配置根决策与 migration format 明确后再实现。
- [~] 不宣称“无锁”：`SqliteSessionStore` 复用 `xai-sqlite-journal` 的 WAL/TRUNCATE 与 busy retry policy；NFS/多进程并发策略已有底层 journal 文档，但 GUI 真实并发 fixture 尚待补。
- [~] 已新增 host allowlist 下的只读 `ImportTuiSession` seam：读取真实格式 `summary.json`/`updates.jsonl` 的 cwd、标题和消息数，损坏/缺字段/未配置根目录 fail-closed，并断言源文件字节不变；尚未把 ACP 更新完整转换为 GUI SQLite 会话，也未覆盖 GUI 写回 TUI。（2026-09-24；`crates/codegen/chaos-engine/tests/tui_import.rs`）
- [~] SQLite store 已覆盖损坏 DB、新 schema、缺父目录、旧 schema 升级备份/恢复、非法版本和重启恢复；迁移中断/磁盘满、多进程/NFS 和 TUI 旧目录导入仍需真实 filesystem fault/TUI fixture，当前环境不能把普通 tempfile 测试冒充完成。（2026-09-24）

### M2.6 验收门禁

- [~] engine/Web/React 已验证 session workspace ID 绑定、错误 workspace Resume/Snapshot 拒绝、snapshot 返回 workspace_id、重启后绑定恢复以及 registry session 切换时 transcript/审批/question/busy 状态隔离；**不表示不同 registry workspace 有独立物理 root，当前仍 single-root**。layout 尺寸/theme/panel visibility 有版本化 JSON 持久化与损坏回退；跨 workspace 多-root files/Git、tabs/split/draft、宽窄屏浏览器和 Desktop transport 仍待验收。（2026-09-24；`workspace_session.rs`、`workspace-ui.test.ts`、`layout.test.ts`）
- [~] 单 host-root Engine 上有已批准 file write→FileChanged/WebSocket 测试与临时 Git fixture；完整终端创建文件→增量 file tree 搜索/编辑/Diff 同状态 E2E 因无 PTY interactive adapter、无 workspace ID→多 canonical root mapping 而未覆盖，不能只凭 single-root 测试标完成。
- [~] engine `AttachmentStager` 已覆盖分块写入、10 MiB/类型/路径策略和失败清理；真实 WebSocket 上传、取消/进度与最终 staging-to-workspace 审批移动均有测试，上传后未批准不会写入 workspace。（2026-09-24；`attachment_flow.rs`）
- [~] `SqliteSessionStore` 本地 round-trip/schema reject、old-schema backup/forward-upgrade、损坏/非法版本拒绝和重启恢复测试已通过；多进程、网络文件系统/busy contention、磁盘满及进程中断 recovery 仍需要可控 filesystem fault/独立进程测试环境，未以普通 tempfile 伪报完成。该条仍需独立 storage fault scope，不被 UI layout/localStorage persistence 取代。
- [~] Web desktop-width and 390×844 browser state/routes for workspace/sidebar/theme/layout/composer are in Playwright, and all changed Web state surfaces pass. Remaining desktop window/WebView and shared settings/provider/approval routes do not exist; workflow/real desktop cross-navigation awaits corresponding feature contracts and Tauri entry.

---

## M3：设置与扩展生态

**Owner**：TBD  
**目标日期**：TBD  
**依赖**：M2  
**交付目标**：提供完整的 Chaos 原生设置，以及 MCP、插件、技能、工作流和子代理的可管理闭环。

### M3.1 设置与凭据

- [~] engine 已提供最小 settings envelope（Base URL/model），严格拒绝非 HTTPS、含凭据或 fragment URL，并永不返回 API Key；真实 `UpdateSettings` handler regression 证明 fragment URL 被拒绝且不改变当前设置。非敏感 settings 现随 JSON snapshot 持久化并在重启后恢复，完整 Chaos 配置 schema 表单仍待接入现有 config boundary。（2026-09-28；`settings_never_return_api_key_and_reject_unsafe_base_urls`）
- [ ] 覆盖通用、外观、模型、Provider、权限、安全、快捷键、远程和更新。
- [ ] API Key 使用经 M-1 选型的 OS keyring/加密方案；本切片不接触或存储 API Key，完整凭据方案仍待维护线/安全 ADR。
- [~] 已有 Base URL/model 更新校验和错误事件；新增 provider shape validation（HTTPS/no credentials/fragment/model length）且明确 `network_not_attempted`，不触碰 API Key；`UpdateSettings` 与 `ValidateProvider` 对 fragment URL 一致返回 `invalid_base_url`。真实 provider 能力/连通性测试、超时/取消和脱敏网络错误待真实 provider adapter。（2026-09-28；engine tests）
- [~] Base URL/model 更新在当前 engine 内即时生效并写入 transitional JSON snapshot，非法更新先返回错误且不改变旧值；UI 明确生效提示和完整配置存储策略仍待 M3 表单/凭据方案。（2026-09-24；engine settings persistence test）

### M3.2 MCP、插件与技能

- [~] plugin marketplace 现已通过 `chaos-engine` 暴露只读 `ScanMarketplace` adapter，复用现有 catalog/scanner/path validation；扫描根目录必须由 host 显式配置，WebSocket 已覆盖允许/拒绝路径；危险安装/执行、来源权限、签名失败和 MCP 连接状态仍需 approval-gated adapter 与真实 registry/credential 环境。（2026-09-24；`crates/codegen/chaos-engine/tests/marketplace_scan.rs`、`crates/codegen/xai-grok-web/tests/marketplace_scan.rs`）
- [~] 已有只读 marketplace scanner 对 indexed relative path traversal/symlink escape 的 crate tests；GUI 尚无第三方安装/执行入口，来源/权限审批 UX、恶意 manifest/supply-chain signature fail gate 需先由维护者批准 extension trust/signature policy，之后接 approval-gated adapter。（2026-09-24；`crates/codegen/chaos-engine/tests/marketplace_scan.rs`、`crates/codegen/xai-grok-web/tests/marketplace_scan.rs`）

本轮全量测试与安全审计证据、按阶段路线和前置条件映射、当前未完成行的精确原因和对应 Owner/决策/外部资源清单见 [`docs/architecture/todo-open-item-classification.md`](docs/architecture/todo-open-item-classification.md)；本段不再复制过期统计及历史运行日志摘要。所有产品矩阵、安全策略、平台/签名/凭据/远端环境门槛仍须各自 Owner 批准并提供资源，不因本轮本地实现而视作通过。

## M3.3 工作流与子代理

- [ ] 展示工作流阶段、状态、通知、产出物和取消操作。
- [ ] 展示子代理列表、状态和允许暴露的上下文；敏感内容按权限过滤。

- [~] Workflow/subagent GUI operations are not exposed. Existing Engine cancel/sequence/resume/audit operations are session-level only; there are no workflow/subagent lifecycle events/data contract. Design and approval-gated adapters are required before a status/artefact panel can represent actual work; do not present prompt text/mock as a production executor.
- [ ] 覆盖部分失败、父任务取消、子代理超时和应用重启后的状态。

### M3.4 品牌与本地化

- [~] CLI/npm/manual-rendered app shell already display Chaos. `check-brand-protocol.py` CI scans ship CLI commands and embedded reference docs (`grok <user command>`) plus direct rendered UI source title/wordmark for `Grok Build` product label; fixtures prove it rejects CLI/doc/UI mutation but passes historical/compatibility text. It intentionally preserves crate/wire/env/path compatibility strings. No icon or native Tauri window branding is implemented; OS/manual accessibility review remains gated on Tauri platform acceptance.
- [~] Chaos Web 已为交互控件加入深/浅主题键盘焦点轮廓，工作区切换/归档是独立语义按钮；真实 desktop/mobile Playwright 覆盖 Tab/Enter 与工作区流程。Axe-core 全页扫描在空页面和真实工作区/消息状态下检查 WCAG 2.0/2.1 A/AA、WCAG 2.2 AA 与 best-practice 标签，并断言扫描结果没有任何 violations（包含 serious/critical）；曾发现并已修复 document title、`lang` 和语义主标题缺失。该自动扫描是有界浏览器检查，不替代完整本地化、屏幕阅读器与平台验收。中英文案覆盖、长文本、屏幕阅读器实机及 macOS/Windows/Tauri 无障碍验收仍开放。（2026-09-26；`apps/chaos-ui/e2e/workspace-flow.pw.ts`、`apps/chaos-ui/index.html`、`src/style.css`）
- [x] Added bounded `check-brand-protocol.py` CI guard: rejects obsolete shipped CLI commands (`grok <subcommand>`) and rendered UI source name `Grok Build`, while excluding internal comments/tests, history, crate/wire/env and `~/.grok` compatibility. Clean/negative CLI, doc, UI and compatibility fixtures pass; Rust-generated wire type IDs are covered by separate schema-drift check.

### M3.5 验收门禁

- [ ] M3 Provider E2E remains gated on an approved OS keyring/config decision and owner-provided revocable credential/contract endpoint. Engine shape validation intentionally reports `network_not_attempted`; the browser invalid-shape regression proves rejection/UI behavior only, not Provider connectivity or secret persistence.
- [~] MCP 执行 E2E 已用**自建端点**真实打通（`43a90aca`）：`crates/codegen/xai-grok-mcp/tests/real_mcp_server_e2e.rs` 共 7 条测试，驱动生产入口 `start_mcp_server → ensure_initialized → get_tool_registrations → call_tool`，对端是真实 OS 进程 `crates/codegen/xai-grok-mcp/src/bin/mock-mcp-server.rs`（newline-delimited JSON-RPC over stdio，作为 Cargo bin 构建，只依赖 `std`+`serde_json`，不参与 rmcp/reqwest 0.13 feature 隔离）与真实 axum streamable-HTTP 端点（bearer token 门 + `mcp-session-id`）。实测断言：握手身份、`server__tool` 限定名与 schema/description 规范化、`isError` 工具结果与 JSON-RPC 协议错误之分、配置的 `Authorization` 头在每个请求上送达且会话 id 被后续请求携带、凭据被拒或缺失时**永不**产出工具输出、客户端 drop 后 stdio 子进程确实被杀（PID 由工具调用路径本身取回）、config diff 移除服务器时清掉它的限定工具但保留未变服务器的工具与持久化 `disabled_tools`。**`get_tool_registrations` 与 `call_tool` 此前在全仓零覆盖**。进程存活断言仅 unix 生效（Windows 需要本 crate 未依赖的 OpenProcess 探针，测试里明写而非假断言）。仍不关闭的部分：marketplace 安装/审批链路与 Web GUI 的启用开关链路，以及对真实第三方服务器做来源/签名信任验收——`/approve-tool` demo 依旧不证明已发布服务器连接。（2026-10-02；`cargo test -p xai-grok-mcp --test real_mcp_server_e2e` 7 passed，连跑三次稳定；`cargo test -p xai-grok-mcp` 全量通过）
- [ ] Plugin/skill/workflow/subagent E2E waits for approved extension trust/signature/permission policy, approval-gated install/execute adapters, test registry artifacts and a real workflow/subagent lifecycle event contract; cancellation/failure/restart tests must use that service, not demo prompts.
- [~] GUI settings/provider shape validation、SQLite schema rejection/restore/upgrade 有 Engine/store tests。设置 UI 已通过真实桌面/移动浏览器 flow 测试无效带 fragment Provider URL 的错误显示与 `has_api_key=false` 状态，未提交或显示 API Key；主题深色错误徽标对比度也经过状态扫描修复。仍无 settings persistence/import UI 或 secret keyring integration；真实 Provider 连接和 credentials unavailable/轮换页面恢复要等 keyring 决策和获批测试端点。（2026-09-29；`apps/chaos-ui/e2e/workspace-flow.pw.ts`、`src/style.css`）

---

## M4：远程工作区 MVP

**Owner**：TBD  
**目标日期**：TBD  
**依赖**：M3、ADR-004、SSH spike 通过  
**交付目标**：根据 ADR-004 交付一种明确、受支持的远程拓扑。首版至少支持远程文件、搜索、Git 和工具执行；不自动承诺远程 Agent 脱机推理或完整交互式 PTY。

### M4.1 远程 transport 与部署

- [~] 新增 `chaos-engine::remote` capability/endpoint boundary：声明 workspace/tool/port-forward 能力，要求 Strict 或有 fingerprint 的 TOFU host-key policy，拒绝 detached Agent；真实 SSH/workspace transport 仍需远端主机与 SSH spike。（2026-09-24；remote unit test）
- [ ] M4 topology decision must explicitly name RPC contract/server ownership: capability/client seam lists `xai-workspace-server` as candidate and daemon as lifecycle-only, but no maintainer-approved deployment/lifecycle/security design currently authorizes a remote adapter.
- [ ] Requires an approved M4 supported-host/OS matrix, signed/versioned server artifact source and a remote deployment runner; then probe OS/architecture, verify hashes/signature, atomically install versioned server with least-privilege permissions and rollback.
- [~] `chaos-engine::remote::RemoteEndpoint` 已提供 capability/host-key policy 校验边界；真实 client/server version negotiation、降级提示和传输仍需 M4 远端实现。
- [ ] Implement heartbeat, bounded retry/backoff, cancellation and connection states only after the SSH transport and version-negotiation contract is selected; acceptance needs a controlled endpoint capable of deterministic disconnect/upgrade failures.

### M4.2 SSH 安全

- [ ] 支持 ADR/spike 已验证的认证方式；未验证的 ProxyCommand 等能力不得宣传。
- [~] remote endpoint 类型已强制 Strict/带 fingerprint 的 TOFU policy，缺 fingerprint 或 detached Agent 会被拒绝；真实 SSH host-key 交换、变更阻断和凭据测试仍待 maintainer 选定候选库/auth policy 并提供获批远端 runner/test credentials。（2026-09-24；`gui-remote-status.md`）
- [ ] M4 credential forwarding gate: after auth modes/key handling are chosen, add recording test endpoints proving password/token/private-key material never enters logs or remote payloads; Agent forwarding default-off needs an actual SSH client option/security test.
- [ ] Real remote-server bind/one-time-handshake gate requires M4 server implementation and Linux controlled runner: assert Unix socket/stdio/loopback only, external bind refusal, handshake replay rejection and credential expiry.
- [ ] Remote threat test requires deployed test daemon/artifact and host-specific filesystem/process controls; exercise upload dir traversal, least privilege, secret-free logs/pidfiles, interrupted upgrade and rollback before enabling deployment.

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

- [x] CI `36186580928` passed separate Linux GUI browser, GUI Rust/frontend/protocol jobs and main Rust `fmt`, all-target `check`, strict workspace `clippy`, and full workspace `test`; Rust job elapsed 42m02s (<60m), with `RUST_MIN_STACK=16 MiB`. Run `36178108811` first verified the stack-size correction; final run also passed after extending a pager history-daemon test's polling deadline for overloaded runners. Gate executes ignored inventory and bounded brand drift checks. Local full workspace evidence: scratch `rust-workspace-16m-after-ime.log`; final remote JSON: `github-ci-history60.json`。（2026-09-25）
- [~] M-1.6 still has telemetry acceptance open: one current Rust job under budget is observed; historical GUI-on/off timings, peak RSS and disk delta need Actions telemetry owner. A green CI run does not prove zero GUI cost.
- [x] Repository Playwright flows drive actual Engine/WebSocket/Vite desktop and narrow projects: workspace create/submit/switch/reload/archive, layout/theme, health/handshake, empty/cancel, demo approval reject/allow-to-adapter-failure and question answer, GFM/inline+block formatting, injected HTML inertness, denied relative/javascript hrefs, inert image text with no remote request, and hardened external hrefs. Linux browser CI passed runs `36094218127` and `36133400667`。（2026-09-25）
- [~] Tauri/WebView browser automation, true tool adapter progress/results/approved mutation/Diff forms, and provider/keyring paths need app adapters, design/credentials and OS runner; current Web automation doesn't imply those modes.
- [~] GUI CI 已使用 npm cache 与 `package-lock.json`；Rust cache 由现有 CI 提供，pnpm/sccache 尚未引入，避免没有测量就叠加缓存系统。（2026-09-24）
- [~] 现有 `THIRD-PARTY-NOTICES`、lockfile 和 secret scan 提供基础审查；SBOM、漏洞扫描和自动 license gate 尚待接入工具/CI runner。
- [~] ignored inventory 已修复并纳入 Q4 CSV；稳定版相关 ignored test 的逐项 owner/豁免审查仍未完成，不能作为 release 通过依据。

### M5.2 桌面打包与签名

- [ ] macOS：arm64/x64 或 Universal 构建、entitlements、签名、公证、Gatekeeper 和升级测试。
- [ ] Windows：MSI/NSIS、Authenticode、WebView2 检测、安装/卸载/升级测试。
- [ ] Linux：明确支持 AppImage/deb/rpm 中的实际集合，验证 Wayland/X11 和 WebKitGTK 依赖。
- [ ] 签名证书、secret 名称、轮换、权限和失效处理形成 runbook。

### M5.3 Web/CLI 与更新

- [~] `xai-grok-web` 现可由显式 `CHAOS_WEB_ASSETS_DIR` 启用静态目录托管：`/assets/*` 提供构建资源，其他未知 GET 路由使用 `index.html` 支持 React SPA fallback，已声明的 `/health`、`/api/handshake`、`/api/sessions` 与 `/ws` 优先保留原 handler；MIME、缺失资源 404、SPA 路由回退及 health/API non-shadowing 有 Axum router tests；静态资源还支持同名 `.gz`/`.br` 变体协商与 `Vary: Accept-Encoding`、缓存重新验证；hashed bundle 和 `index.html` 的策略已由 router/browser tests 验证。独立 Playwright 测试启动真实 Web binary 加本次 Vite dist，验证页面渲染、JS asset bytes/type、SPA path、missing asset 404、健康和受保护握手，以及浏览器 WS 握手→session create。静态资源现在可通过同名 `.gz`/`.br` 预压缩产物协商返回，带 `Vary: Accept-Encoding`；hashed `/assets/*` 的 Cache-Control 要求每次重新验证，SPA `index.html` 为 `no-cache`，避免部署后旧入口引用过期 bundle。router unit 与 desktop/mobile Playwright 均验证压缩响应/内容协商和缓存头。identity 静态文件现按实际资源字节计算 SHA-256 ETag，支持 `If-None-Match`/304；真实 Axum handler regression 覆盖匹配 ETag 返回空 304、文件内容变化后旧 ETag 返回新 200/body/new ETag。SPA index 仍为 no-cache（无 ETag），并加 `Vary: Accept-Encoding`。middleware按协商选定的预压缩文件（若有）或identity文件字节生成 SHA-256 ETag，并以 `.gz`/`.br` 后缀隔离编码表示；真实静态部署的 gzip/Brotli 响应均已在桌面/移动浏览器验证正文可由浏览器透明解码、带对应磁盘变体 SHA-256 ETag，且匹配 `If-None-Match` 返回 304；质量值、大小写与标准 `x-gzip` 兼容别名由真实 router unit test 覆盖；`x-gzip` 在真实 built-host 浏览器路径验证与 gzip 使用同一压缩文件及 ETag。ETag 条件逻辑限制于 GET/HEAD；POST 即使带匹配 validator 也由 ServeDir 返回方法拒绝（router test 覆盖 HEAD 200/ETag 与 POST 405）。CDN cache invalidation 和正式 release pipeline 接线仍待 M5；这些本地 HTTP 语义不替代 CDN/发行策略验收。（2026-09-30；`crates/codegen/xai-grok-web/src/lib.rs`、`apps/chaos-ui/e2e/static-host.pw.ts`；goal scratch `verification/static-host-cache-test.log`、`static-host-cache-clippy.log`、`static-host-cache-build.log`、`static-host-cache-ui-build.log`、`static-host-cache-vitest.log`、`static-host-cache-typecheck.log`、`static-host-cache-web-suite.log`、`static-host-cache-playwright.log`、`verification/static-host-etag-router.log`、`static-host-etag-clippy.log`、`verification/etag-and-spa-assets-test.log`、`etag-and-spa-assets-clippy.log`、`verification/etag-final-check.log`、`etag-final-web-tests.log`、`etag-final-clippy.log`、`etag-final-typecheck.log`、`etag-final-vitest.log`、`etag-final-ui-build.log`、`etag-final-playwright.log`、`final-audit/etag-final-fmt.log`、`final-audit/etag-final-diff.log`、`verification/compression-web-check.log`、`compression-web-suite.log`、`compression-web-clippy.log`、`compression-ui-typecheck.log`、`compression-ui-vitest.log`、`compression-ui-build.log`、`compression-browser.log`、`final-audit/compression-fmt.log`、`final-audit/compression-diff.log`、`final-audit/compression-protocol.log`、`compression-brand.log`、`compression-secrets.log`、`compression-ignored.log`、`compression-l10n.log`、`verification/final-compression-check.log`、`final-compression-web-suite.log`、`final-compression-clippy.log`、`final-compression-typecheck.log`、`final-compression-vitest.log`、`final-compression-ui-build.log`、`final-compression-playwright.log`、`final-audit/final-compression-fmt.log`、`final-compression-diff.log`、`final-compression-protocol.log`、`final-compression-brand.log`、`final-compression-secrets.log`、`final-compression-ignored.log`、`final-compression-l10n.log`、`verification/x-gzip-final-check.log`、`x-gzip-final-tests.log`、`x-gzip-final-clippy.log`、`x-gzip-final-typecheck.log`、`x-gzip-final-vitest.log`、`x-gzip-final-ui-build.log`、`x-gzip-final-browser.log`、`final-audit/x-gzip-final-fmt.log`、`x-gzip-final-diff.log`、`x-gzip-final-protocol.log`、`x-gzip-final-brand.log`、`x-gzip-final-secrets.log`、`x-gzip-final-ignored.log`、`x-gzip-final-l10n.log`）
- [ ] 提供 sha256、签名、版本索引和可验证的更新 feed。
- [ ] 自动更新覆盖正常升级、签名失败、下载中断、回滚和旧数据迁移。
- [ ] 保持现有 CLI 发行路径兼容；明确桌面/Web 是否独立版本或 lockstep。

### M5.4 可重复性能门禁

- [~] 已记录一份可复用的 Linux Chromium benchmark 环境规格和采样/报告规则，供开发期对比使用；这不是稳定硬件 runner，也没有性能基线测量。固定 CI runner、Tauri/WebView 各平台规格仍待 M5/平台 Owner 确认。（2026-10-01；`docs/performance/benchmark-environment.md`）
- [~] 已新增可执行 smoke collector：独立启动本地 Vite UI，记录页面就绪时间、浏览器内合成 150 次/秒 DOM 更新调度和 Node runner RSS，并输出含原始样本/p50/p95/机器信息的 JSON。它不启动或验证 Web host；真实 WebSocket/React delta、冷启动、浏览器/服务进程树 RSS、长会话滚动、10 万文件搜索、大 Diff 尚未接入；真实 UI smoke regression 已通过：collector 启动本机 Vite，浏览器访问 app shell，报告含 Chromium 精确版本并显式声明未启动/验证 Web host；这只验证报告生成路径，不是 Engine streaming 指标或性能基线。（2026-10-01；`apps/chaos-ui/perf-benchmark.mjs`、`perf-benchmark.test.mjs`）
- [~] JSON collector 已为每个原始测量样本计算 p50/p95，不设未经基线验证的阈值。稳定 runner 重复测量、允许回归比例和 Owner 批准的阻断标准仍待完成。
- [~] `npm run perf:collect` 已输出机器可读 JSON 并经真实启动 UI/Web 服务的 smoke regression 验证；目前没有 CI artifact 保存、稳定 runner baseline 或获批稳定版阈值，门禁仍未接线。（2026-10-01；`apps/chaos-ui/perf-benchmark.mjs`、`perf-benchmark.test.mjs`）

### M5.5 最终人工验收

- [~] Web browser E2E 现在覆盖真实 Engine workspace/session create/submit/stream/switch/archive/reload，Markdown safe rendering，demo-protocol approval rejection 和 question answer。Backend approval competition/resume 与 Diff 另有真实 WebSocket fixtures；Playwright 未把这些 faked prompt triggers 当 production tools。Terminal/Git shipped routes、approved mutation/Diff UI、Tauri flow 和真实 provider/tools 后完整 loop remain open.
- [~] 当前 browser E2E 覆盖初始空 transcript/空 composer、prompt cancel、服务真实连接、reload snapshot、safe Markdown 与 narrow viewport；新增的 approved file write/deny/re-read、terminal/Git operation 和 invalid Provider shape 错误路径均有真实 Web browser regression。Large-conversation performance、跨平台 display/OS error、disk-full 和 corrupt runtime config recovery 仍待各自数据/I/O/平台 gate。（2026-09-29；`apps/chaos-ui/e2e/workspace-flow.pw.ts`、`e2e/approved-workspace-ops.pw.ts`）
- [~] workspace create/switch/reload/archive、transcript isolation、layout/theme persistence 与 composer 已在 desktop/mobile Playwright CI 验证；真实多 tab 审批通知及 Tauri 页面仍待 gate。主题和宽窄视口已有当前覆盖，未声称共享状态所有界面完成一致性验收。（2026-09-25；Playwright run `36094218127`）
- [ ] macOS、Windows 和 release package 安装/upgrade/rollback/uninstall 需各平台 runner、签名资源和产品 support matrix；现存 Linux CLI install 不等于 M5 GUI install，通过各对应 platform gate 验收。
- [~] 前端 transport 已在 HTTPS 页面选择 `wss:`、HTTP 页面选择 `ws:` 并保留 host port/base path；生产 TLS 终止、proxy headers、Token 轮换和审计日志仍待真实部署拓扑验收。（2026-09-24；`src/transport.test.ts`）
- [ ] 检查远程支持矩阵中的每种认证和故障路径；未支持能力无误导入口。

### M5.6 发布资料

- [ ] 稳定版发版时再更新 README、用户指南、架构文档、配置参考、故障排查和安全说明；当前 GUI seams/限制和 WSL build guidance 已记录于对应架构、CONTRIBUTING 与 audit docs，不能在产品/平台能力未定前写成功能交付。
- [ ] 在 release owner 批准的实际产品版本/支持矩阵确定后生成匹配的 CHANGELOG、THIRD-PARTY-NOTICES、SBOM 和 artifact checksums；本轮未发新 version 或 platform artifact，不能为测试 commits 伪造 shipping inventory。
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
- [ ] 在干净环境里实测官方 `npm install` 并跑通 `chaos --version`。已实测 Windows 两个子包名由 `0.0.1-security` placeholder 占用，元包的 all-six platform set 无法安全安装/发布；保留为 npm package owner 通过 support reclaim 或批准 rename/pin migration 的 release gate，不能替换为本地 tarball 模拟通过。

**2026-09-24 复核与版本决策**：Cargo/npm 仍为 `0.4.2`，仓库已有 `v0.4.2` tag。release workflow 已强制签名/`require-sig`，installer 也默认 fail-closed；本轮确认真实 release dispatch 未执行，所以 GitHub signing preflight 的真实 secret public/private match 与 Windows installer runner 仍未验证。MT-1 七个 npm packages 仍受 Windows `0.0.1-security` placeholder 阻塞；未获包所有者处理、签名 secret/runner 及 release 负责人审批前，不创建 `0.4.3` tag/Release。

**验收证据**：CI 检查的 PR、一次真实的 `npm install` 输出。本地能运行 GUI npm/Engine/Web 和临时 Chromium flow；官方 Windows npm subpackage 当前由 `0.0.1-security` placeholder 占有导致 package installation fails，必须由包所有者通过 npm support reclaim 或批准 new names/version-pin migration，不能本地镜像测试视作发版通过。

---

## MT-2：自更新签名收尾

**Owner**：发布负责人（需在开工前指派具体维护者）  
**阻断级别**：P1（涉及供应链完整性）

`docs/audit-followup-report.md` §4 已于 2026-09-23 按实际代码与仓库配置状态更新；本轮已把 release workflow、installer 和 policy fixture 的可本地部分接通并验证。该设计门禁与机器验证已在 workflow/fixture 完成；真实密钥匹配和正式 release dry-run 仍需 release owner 的受控密钥/签名资产，Windows installer runner 也仍是平台门禁。

- [x] 更新 `docs/audit-followup-report.md` §4，记录签名接线、降级路径和未配置密钥时的真实状态。（2026-09-23；证据：本次复核）
- [~] signing-preflight implementation and local match/failure cryptographic fixtures pass; this environment did not read GitHub secrets or execute a real signed-release preflight. Actual key configuration/matching must remain release-owner verified; no secret values were accessed.
- [x] release workflow 强制要求签名密钥与 `require-sig` feature；新增 `signing-preflight` 校验 secret/variable 非空、Ed25519 公私钥匹配，配置缺失或不匹配时在构建前阻断。（2026-09-24；workflow guard/本地 fixture 通过；真实签名资产流程未运行）
- [x] Unix/PowerShell/batch installers 默认 fail-closed；新增 `scripts/ci/test-installer-signature-policy.py` 并接入 CI，缺少 signature/public key/cryptography 会失败，仅 `CHAOS_SKIP_SIGNATURE=1` 显式 opt-out。（2026-09-24；fixture 和 bash -n 通过；Windows runner 仍待运行）
- [~] updater tests cover valid/tampered/wrong-key/missing sidecar and installers have fail-closed structural CI fixtures; actual signed asset acceptance/rejection/missing-sidecar install plus Windows PowerShell run need release assets and Windows runner.
- [ ] Release owner 在签名 secrets 和 Windows runner 可用后运行正式 release dry-run，验证实际 sidecar/embedded key match；在该真实受控 gate 完成前不创建新 tag。

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
  fortress-breach 全为空。（2026-09-22 实测 `--before main --after HEAD` → 0 / 0 / 0。）本轮 Chromium browser screenshots/logs 存于 `/tmp/grok-goal-3fd74e087187/implementer/` 私有 goal scratch，不属于工作树用户输入。
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
- [x] 2026 Q4 review（到期：2026-10-01，完成于 2026-10-02）：428 个 `#[ignore]` 属性**全部**具备 in-source reason，218 条裸属性清零，其中 1 条（`xai-grok-shell` `session_thread_detects_panic`）实测通过后**删除 ignore 恢复运行**，其余按 8 个族记录 Owner=项目负责人 与下次复核日 2027-01。分族处置：362 需已构建二进制/PTY 会话（保留 ignore，reason 内含可直接执行的命令）；33 断言 fork 已删除的功能（billing/subscription、上游 xAI 登录与 endpoint 默认值、connectors URL、Grove pin backend）；8 属人工/soak/性能测量；7 在并行下 flaky 或需单进程隔离；7 根本不是测试（子进程入口/父测试辅助）；5 依赖特定 OS 能力（cgroupv2 delegation、X11）；2 依赖外部语言服务器；1 读取真实 `$HOME`。**边界**：本轮只真实执行了 3 个属性（1 通过并恢复、`smoke_push_pull_round_trip` 实测失败并记录原因、候选筛选），其余按 harness 与模块文档分类，**未声称跑通**。（2026-10-02；`docs/ignored-audit-2026q4-summary.md`、`docs/ignored-audit-2026q4.csv`）
- [x] 补齐 reason 的做法是**推导**而非猜测：测试文件的 reason 由 `tests/<target>.rs` 出发递归跟踪 `#[path]` 与 `mod x;` 声明找到真正承载它的 Cargo target 后写入，218 条里有 147 条所在文件被多个 target 共享；11 个源码内站点逐个阅读后单独定理由（子进程入口写明其 env 变量名、visual preview 写明只 `println!` 不断言、LSP 写明需要 `typescript-language-server`/`ROSLYN_DLL`）。新增 `scripts/ci/ignored-tests.py --require-reasons` 门禁，**无豁免表**，`#[ignore = ""]` 这类空 reason 同样判失败；`scripts/ci/test-ignored-tests-reasons.py` 6 个 fixture 通过注入裸属性与空 reason 断言退出 1，并断言行尾注释保留、文档注释里的 `#[ignore]` 不计、真实仓库通过。`ignored-tests-baseline.tsv` 清空为仅注释头。顺带纠正 14 条**失实** reason：13 条写 “CI/Bazel provides PAGER_BINARY”、1 条写 “CI runs the ignored pty_e2e suite”，而本仓库 `git ls-files` 无任何 `BUILD`/`WORKSPACE`/`MODULE.bazel`/`*.bzl`，`ci.yml` 亦无 `--ignored`/`--include-ignored`，即 CI 从不执行任何 ignored 测试；`xai-grok-pager/Cargo.toml` 里那句 Bazel 接线说明一并改正。**验证**：`--require-reasons` 与 `--check-baseline` 均退出 0（428 total / 0 bare / 0 new）；`cargo check --workspace --all-targets` 干净；strict `cargo clippy --all-targets -- -D warnings` 干净；`cargo fmt --all -- --check` 干净；`test-ignored-tests.py` 8 passed、`test-ignored-tests-baseline-fixture.py` 3 passed、`test-ignored-tests-baseline.py` OK、`test-ignored-tests-reasons.py` 6 passed；恢复的测试在**不带** `--ignored` 的默认集合里 `1 passed; 0 failed; 0 ignored`。（2026-10-02；`scripts/ci/ignored-tests.py`、`scripts/ci/test-ignored-tests-reasons.py`、`scripts/ci/ignored-tests-baseline.tsv`、`.github/workflows/ci.yml`、`docs/ci-test-debt.md`、`crates/codegen/xai-grok-pager/Cargo.toml`）
- [~] 已修复忽略测试扫描器并保留 8 个解析器 fixtures；2026-10-01 真实清点 429 项，其中 218 个裸 `#[ignore]`。CI baseline gate 按 package/path/function key 双向检查新增和 stale-removal；baseline fixtures 对两种负向变异均断言退出 1，且 live repo 429/218 对照通过。现存 218 个裸属性仍需逐 crate 补 reason/review date，不把 baseline 误称审批。（2026-10-01；`scripts/ci/ignored-tests-baseline.tsv`、`test-ignored-tests-baseline-fixture.py`）
- [~] 2026-10-01 Q4 inventory reports one `xai-grok-update` ignored attribute: the opt-in 100k stress test. Five stale tests asserting upstream channel-specific install URLs have been replaced with running tests of the shipped Chaos `reinstall_hint` behavior (stable/alpha/malformed channel equivalence, platform installer, and safe enterprise fallback); the targeted real-entry-point test run passed 8/8. The remaining ignored 100k stress test now has a checked-in `scripts/test-blitz-stress.sh` entry point; its actual test was run with 120 iterations through that script and passed 1/1. The full default 100k run completed on 2026-10-01: 1 passed, 0 failed in 4306.69 seconds through `scripts/test-blitz-stress.sh` (private goal scratch `blitz-stress-100k.log`). The 100k stress execution is complete; the independent Q4 owner/reviewer decision is still open.
- [x] 维持 `docs/ci-test-debt.md` 的“append-never, remove-only”规则；当前 `--exclude` 列表为空，CI 使用 `cargo test --workspace` 不含添加 exclusions；baseline gate 只限制 ignored inventory，不替代此规则。（2026-09-25；`.github/workflows/ci.yml`、`docs/ci-test-debt.md`）
- [x] 确认 `registered_features_are_documented` 的 `internal-docs` feature 门控是长期方案还是临时绕过。**结论：是长期方案，且已写清理由。** 该 target `include_str!` 的是 `docs/internal/25-enterprise.md` 与 `docs/internal/22-environment-variables.md`，而 `docs/internal/` **在本仓库全部历史与 `origin/main` 里都不存在**（`git log --all -- crates/codegen/xai-grok-pager/docs/internal/**` 无输出），所以它在 cargo 下**根本无法编译**。`crates/codegen/xai-grok-pager/Cargo.toml` 的 `[[test]]` 条目旁已写明这一点，并给了持有内部文档者的跑法（`--features internal-docs`）。删掉它反而是信息损失：它是唯一把 `FEATURES` 与操作员表格对起来的检查。

---

## MT-6：unsafe / unwrap 收敛

**Owner**：TBD  
**阻断级别**：P2（单条 P0 除外，见下）

`docs/audit-followup-report.md` §5 给了按投入产出比排的顺序，但没有任何勾选入口，
结果是报告写完就停在那里。按原顺序落为任务：

- [~] B 批 unwrap 治理：本轮移除 `xai-grok-update::fetch_gcs_channel_pointer` 对重试错误状态的 `unwrap()`，增加无错误状态的结构化 fallback；现有 `gcs_pointer_connection_refused_is_retried_and_returns_error` 验证真实网络失败仍返回错误；对应单个集成测试和 update crate lib tests 均通过。更新 crate 原有 5 处生产进度条模板解析 unwrap 已统一替换为 `progress_style_or_default`：非法模板记录 warning 并使用相应默认样式；真实 helper regression 以无效 indicatif 模板验证 fallback，不依赖终端颜色/网络。更新 crate lib 166 tests、fmt、strict Clippy 通过（`{SCRATCH}/updater-progress-style-test.log`、`updater-progress-style-lib.log`、`updater-progress-style-fmt.log`、`updater-progress-style-clippy.log`）；`docs/audit-followup-report.md` 的历史生产 unwrap 计数仍需全仓实测刷新；`xai-grok-sandbox` 旧计数 28 尚待逐项生产/测试分类和审查，未把整批标为完成。（2026-09-25；`crates/codegen/xai-grok-update/src/version.rs`、`crates/codegen/xai-grok-update/tests/test_network.rs`）
- [~] unsafe P0 审计：逐项复核 `xai-grok-sandbox` 与 `xai-tty-utils` 当前源码 unsafe block/fn/impl/extern，逐条核对平台 cfg、SAFETY 前置条件和真实调用路径。修复发现的安全缺口：sandbox `child_net` 实现模块改为私有；所有 workspace/MCP/pager/terminal/hooks/LSP child spawn 调用已迁移到公开的 `restrict_child_network` / `_std` 安全封装。底层 BPF builder/install API 为 crate-private，不能作为下游可调用的任意 policy 安装接口；namespace TSYNC API 的不可逆、全线程及后续 mount/namespace syscall 拒绝影响已写入 unsafe contract；`statvfs/fstatvfs` 输出缓冲区补齐局部 SAFETY rationale。`xai-tty-utils` 的真实终端 stderr 保存/dup descriptors 改为 `F_DUPFD_CLOEXEC`，防止后续 exec 子进程继承可绕过 stderr 屏蔽的终端句柄；Windows `dup_tui_stderr` 不再将可能无效的 GetStdHandle 直接包装为 File，而是校验并通过 DuplicateHandle 创建真正 owned handle；Windows Job Object Send/Sync unsafe impl 补充线程/内核句柄安全论据。移除 Linux pdeathsig pre-exec hook 中 debug-only `std::thread::current().id()` 检查，避免 fork 后 TLS handle 惰性初始化/分配；同线程 arm/spawn 明确为调用方契约。新增回归通过跨线程 armed command 的真实 pre-exec 路径，确认不会因 TLS 检查 panic/阻止执行。现有真实 Unix-socket child-network integration 证明允许控制连接、restricted child 获 EPERM；实际 stderr 重定向 subprocess 检查保存与 caller dup 均设置 FD_CLOEXEC。Windows/macOS 的代码路径目前仍只有静态审查；CI 已加入 `platform-tests`（`macos-14`/`windows-latest`）以在目标 OS 编译并运行这两个 crate 的测试，但首轮远端结果尚未观察，不能提前当作平台实测证据。两 crate 全量测试、all-target check、strict Clippy 和 fmt 通过；最终 Rust workspace suite 亦通过（31,172 passed / 0 failed / 490 ignored，按 CI 要求设置 `RUST_MIN_STACK=16777216`；scratch `verification/final-rust-workspace-tests-after-api.log`）；目标 crates `xai-grok-sandbox`、`xai-grok-hooks`、`xai-grok-mcp`、`xai-grok-pager`、`xai-grok-shell-terminal`、`xai-grok-tools`、`xai-grok-workspace` 的 all-target check 和 E2E child-spawn gate 均通过；本轮静态逐项审计没有发现 read-deny fd ownership/statx 或 tty kill-on-drop 其他已证实漏洞。还发现 per-spawn filter 的网络保证假设子进程没有继承已连接网络 socket，必须纳入威胁审查和 spawn descriptor audit。仍待独立安全 Reviewer 对 P0 审计签字及 macOS/Windows 平台编译运行，因此 P0 不关闭；restricted child 会继承的既有 network socket fd 风险已经记录到 `docs/audit-followup-report.md` §1.7，必须先梳理受限 spawn 的 fd 来源/close policy，再给网络保证作最终 signoff。`xai-tty-utils` test-only env mutation 位于单测内部即 set→hook selection→remove，前置 lock 只锁本测试 sibling；这是 test harness 无法全局序列化其他线程 env access 的审计备注，未改全进程 env 架构。（2026-09-30；`crates/codegen/xai-grok-sandbox/src/child_net.rs`、`src/hook_write_deny.rs`、`src/read_deny_verify.rs`、`tests/child_net_e2e.rs`、`crates/codegen/xai-tty-utils/src/lib.rs`、私有 goal scratch `verification/mt6-sandbox-tty-check.log`、`mt6-sandbox-tests-final3.log`、`mt6-child-net-e2e-final-clean.log`、`mt6-child-net-e2e-final-ack.log`、`mt6-sandbox-ignored-tests.log`、`mt6-tty-tests-final-portfolio.log`、`mt6-tty-stderr-cloexec-regression.log`、`mt6-pdeath-signal-safety-regression.log`、`mt6-p0-check-portfolio.log`、`mt6-p0-clippy-portfolio.log`、`mt6-p0-fmt-portfolio.log`、`mt6-unsafe-clippy-final-clean.log`、`mt6-child-network-private-api-check.log`、`mt6-network-api-final-check.log`、`mt6-network-api-final-e2e.log`、`mt6-network-api-final-ignored.log`、`rust-workspace-mt6-final.log`、`mt6-p0-clippy-final-api.log`、`mt6-child-network-private-api-check.log`、`mt6-child-network-private-api-e2e.log`、`mt6-sandbox-private-api-tests.log`、`mt6-child-net-e2e-final-ack.log`、`mt6-sandbox-ignored-tests.log`、`mt6-p0-clippy-final-api4.log`）
- [~] `xai-tty-utils` 的一条资源计量测试原本在已通过 `expect` 的 `self_time` 上重复调用 `unwrap()`；现复用同一局部值，与同一次采样的 user/system 字段作精确比较。真实资源读取测试、fmt、strict Clippy 通过（私有 goal scratch `tty-resources-unwrap-test.log`、`tty-resources-unwrap-fmt.log`、`tty-resources-unwrap-clippy.log`）；只清理了这一个无必要 unwrap，不替代整 crate unwrap / unsafe 审计。
- [x] `xai-grok-shell` 的环境变量 `unsafe` 消除：该 crate `unsafe` 关键字行从 327 降到 49（-85%，超过原估的 ~60%），crate 内只剩 `external_otel_pin::apply_process_env_strip` 一处保留 `unsafe`（它本身是带 caller contract 的 `pub unsafe fn`）。做法：把环境写操作收敛到 `xai-grok-test-support::env` 的单一入口——进程级写锁 + 线程本地重入计数（`with_write_lock` 可嵌套而不自锁死），`set_var`/`remove_var`/`var_os` 各自只包一个带 SAFETY 注释的块，`EnvGuard` key 放宽为 `Cow<'static, str>` 以支持运行时构造的 key，`isolate_grok_env` 也改走同一入口。274 处 `unsafe { std::env::set_var/remove_var }` 与 4 处混合 `unsafe` 块（PATH/GIT_EXEC_PATH、credential 早失效 guard、app.rs `set_or_clear`、`with_api_key_env`）全部改为 helper，多步读改写放进一次 `with_write_lock` 以保证成对写入原子。同时修掉真实缺陷：生产代码不再在运行时改进程环境——`initialize()` 从 auth.json 读到 key 和 `x.ai/setApiKey` 原本调用 `std::env::set_var("XAI_API_KEY", ..)`，这是与其他线程 `std::env::var` 并发的进程全局写，且把密钥泄进之后每个子进程（shell 工具/hook/MCP server）的 env block；改为 `auth_method` 内 `RwLock` 保护的 runtime key cell（Present/Cleared/Unset 三态，Cleared 只屏蔽 `XAI_API_KEY`、保留 legacy 变量，与原 `remove_var` 作用域一致）。新增测试：test-support 6 项（真实进程 env 生效、guard 恢复既有值/新建键、panic 时仍恢复、嵌套写锁不死锁、8 线程并发写各自读回、成对写对读者原子）+ shell 4 项（未设置时环境权威、runtime key 覆盖环境且不回写 env、clear 的屏蔽范围、并发读写不撕裂），全部通过；`xai-grok-shell --all-targets`、workspace `--all-targets` check 与 `cargo fmt --check` 通过。（2026-10-02；`crates/codegen/xai-grok-test-support/src/env.rs`、`crates/codegen/xai-grok-shell/src/agent/auth_method.rs`、`src/agent/mvp_agent/acp_agent.rs`、`src/extensions/auth.rs`；私有 goal scratch `env-helper-tests.log`、`runtime-api-key-tests.log`）
- [x] 环境变量 `unsafe` 收敛推广到其余四个高频 crate（`xai-grok-workspace`、`xai-grok-update`、`xai-fast-worktree`、`xai-grok-pager`），复用上一行同一个 `xai-grok-test-support::env` 加锁入口。计量口径统一为 `crates/**/*.rs` 中 `unsafe {` 的出现次数：上游 `SOURCE_REV 72a61251` 为 1028，上一行收敛后为 768，本步为 **586**。逐 crate（`unsafe {` 块数 / 裸 `std::env::set_var·remove_var` 调用数）：workspace 112→4 / 110→1，update 27→1 / 30→0，fast-worktree 70→55 / 32→4，pager 58→25 / 39→3；fast-worktree 残留的 55 个 `unsafe` 全部是 libc/FFI（AF_UNIX socket、procfs/vnode 查询、sigaction、kill/waitpid），与环境变量无关。三个 crate 新增 `xai-grok-test-support` 作为 dev-dependency（该 crate 不依赖三者，无环）。多步读改写包进一次 `with_write_lock` 保证成对写入原子：fast-worktree `GrokHomeFixture::drop`（一次恢复 `GROK_HOME`+`XDG_DATA_HOME`+`GROVE_DATA_DIR`+`HOME` 四个键，避免读者看到半恢复状态）、update `InstallerEnvGuard::isolate`/`drop`（四个安装器变量的批量清空/恢复）、pager `EnvVarGuard::drop`。**保留裸 `unsafe` 的 5 处生产写入**并各自补上 SAFETY 依据，原因是它们位于 library/bin 的非 `cfg(test)` 代码里，而加锁 helper 所在的 `xai-grok-test-support` 只是 dev-dependency，正常构建不可达：pager `app/mod.rs` 的 `GROK_LOG_SAMPLING`（CLI 启动、spawn 线程前）、`app/event_loop.rs` 的 `GROK_OPEN_DASHBOARD_AT_STARTUP`（event loop 之前的 init）、`app/screen_mode_relaunch.rs` 的 `GROK_SCREEN_MODE_ENV`（子进程不得继承屏幕模式覆盖，原有 SAFETY 注释已扩写）、workspace `bin/workspace_server.rs` 的 `RESET_CHILD_OOM_ENV`（tokio runtime 构建前的原始终启动线程）、fast-worktree `auto_gc.rs` 的 `clear_auto_gc_env_for_test`（`#[doc(hidden)] pub unsafe fn`，跨 crate 测试复用，caller contract 已写入 `# Safety` 段并说明为何不走 helper）。**验证**：`cargo check --workspace --all-targets` 干净；`xai-grok-workspace --lib` 1934 passed / 0 failed，`--bins` 26 passed；`xai-fast-worktree --lib` 504 passed / 4 ignored，`xai-grok-update --lib` 164 passed；`xai-grok-pager --lib` 9196 passed / 0 failed / 25 ignored；四 crate `--all-targets` strict Clippy `-D warnings` 与 `cargo fmt` 干净。（2026-10-02；`crates/codegen/xai-grok-workspace/src/{lib.rs,status_config.rs,handle_tests.rs,session/tool_config.rs,bin/workspace_server.rs}`、`crates/codegen/xai-grok-update/src/auto_update_tests.rs`、`crates/codegen/xai-fast-worktree/src/{db/mod.rs,auto_gc.rs,nfs/remove.rs}`、`crates/codegen/xai-grok-pager/src/test_util.rs` 等 27 个文件；私有 goal scratch `workspace-lib-tests.log`、`workspace-bin-tests.log`、`update-fw-lib-tests.log`、`pager-lib-tests.log`、`check-ws-env3.log`、`clippy-env4.log`）
- [~] A 批 unwrap 治理：`xai-grok-shell` 的大批量清理仍需逐批实施和审查。已移除 `claude_import::apply_hooks_to_dir` 在新 hook JSON 对象构造上的两个结构性 `unwrap()`，改为直接构造 `serde_json::Map`，序列化结构不变；helper 的实际临时文件写入/去重/timeout tests 覆盖此路径。另移除 `SessionSignalsActor::GetSnapshot` 对非空 ITL buffer 的 `max().unwrap()`，用 Option `max()` 直接守住非空分支，新增 actor 测试提交 inference metrics 后经真实 snapshot 验证 buffered mean/max。再移除过期内存凭据路径上 `run_auth_flow` 对已校验磁盘凭据的 `unwrap()`，由真实临时 auth-store regression 验证复用兼容且未过期的磁盘凭据。另将 stop-gate 转换测试读取 description 的测试专用 unwrap 改为带语义提示的 expect，并复跑映射器测试；这不计入生产 unwrap 减量。附加的 session-updates 本地正确性切片将 tail offset 的 `i64::MIN` 安全化，并对 turn limit 使用饱和加法，避免极端合法参数导致 signed overflow/panic；实际 `x.ai/session/updates` handler 的持久化 JSONL 回归和整个扩展模块 13 项测试通过。该增强与 unwrap 数量无关，不作为 A 批计数或关闭证据；随后亦以真实 handler 回归补测 `limit: usize::MAX` 与 `i64::MIN` 组合及非 rewind turnIndex，13 项扩展模块测试、fmt 和 strict Clippy 均通过。（2026-09-30；`crates/codegen/xai-grok-shell/src/extensions/session_updates.rs`、goal scratch `verification/session-updates-bounds-regression.log`、`session-updates-limit-bound.log`、`session-updates-tests-verified.log`、`session-updates-fmt-verified.log`、`session-updates-clippy-verified.log`）三处生产局部 unwrap 批次有 shell crate strict Clippy 和 fmt 证据，不代表全 crate 完成。（2026-09-30；`crates/codegen/xai-grok-shell/src/claude_import.rs`、`src/session/signals.rs`、`src/session/signals_tests.rs`、`src/auth/flow.rs`；私有 goal scratch `verification/claude-import-hooks-test.log`、`signals-buffered-itl-test.log`、`auth-disk-token-test.log`）
- [~] 已将单个 `xai-grok-update` 错误分支收敛写入审计跟进报告并实测 update crate lib 与集成路径；其余批次完成后仍须用当前源代码重算生产 unwrap/unsafe 分布，不能沿用 2026-08 旧计数。另，GitHub run `36165469964` 暴露 `xai-grok-shell` current-thread actor tests 在 test harness 默认小栈下溢出；统一提高 CI `RUST_MIN_STACK` 至 16 MiB 后 package lib tests（6,804 passed）、full workspace tests 和 run `36178108811` 均通过。（2026-09-25；`docs/audit-followup-report.md`、scratch `mt6-update-unwrap-test.log`、`xai-grok-shell-lib-16m-after-cleanup.log`、`rust-workspace-16m-after-ime.log`）

---

## MT-7：上游同步节奏与仓库卫生

**Owner**：TBD  
**阻断级别**：P3

- [x] 上游侦察已固化为可重复入口：`scripts/upstream-recon.sh` 只读查询 `git ls-remote` 与 GitHub compare API，把 `SOURCE_REV` 与上游 tip 的差距写进 `sync/recon/<date>-<tip>.md`；无网络时退出 1 且**不写记录**（负向验证：伪造 API 返回 404 → exit=1、记录数不变）。2026-10-02 实测：`SOURCE_REV=72a61251f`、上游 tip=`2bdd1d6a6`、`status=ahead ahead=9 behind=0`，记录见 `sync/recon/2026-10-02-2bdd1d6a6.md`；节奏按 `CONTRIBUTING.md` “Upstream reconnaissance” 执行，侦察只登记差距，移植仍需 curated-port 评审。（2026-10-02；`scripts/upstream-recon.sh`、`CONTRIBUTING.md`）
- [~] 当前 GUI/TUI 修改已在相关切片执行格式、GUI/engine tests 和文档检查；下一轮真实上游同步仍需按规则运行 `scripts/l10n-guard.sh` 前后对照。（2026-09-24）
- [~] 维持“分叉层内一律不搬”的判定：`sync/fork-layer-inventory.md` 已登记根 `Cargo.toml` 和 GUI fork 区段；每次继续上游同步仍需执行 l10n/fork-layer review。（2026-09-24）
- [x] `scripts/` 下的 shell 脚本不再依赖 bash 4 / GNU userland，macOS 自带环境可直接跑仓库入口。实测发现三处只在 Linux 开发机上成立的写法：`scripts/l10n-guard.sh` 与 `scripts/ci/secret-scan.sh` 用 `mapfile`（bash 4.0+），`scripts/ci/local-publish-host.sh` 用 `${platform^^}` + `${key//-/_}`（bash 4.0+）；macOS 仍发行 bash 3.2，这三处会以 `mapfile: command not found` 或错误的变量名失败，而 CI 的 ubuntu runner 永远看不见。全部改成 3.2 可用写法（`while IFS= read -r` 读取列表、`tr '[:lower:]' '[:upper:]' | tr - _`），并为 `set -u` 下 bash 3.2 会把空数组 `"${files[@]}"` 判为 unbound 的已知行为加了提前返回。新增 `scripts/ci/check-script-portability.py` 门禁（25 条规则：`mapfile`/`readarray`/`declare -A`/nameref/`${var^^}`/`${var@Q}`/`EPOCHSECONDS`/`globstar`/`wait -n`/`nproc`/`readlink -f`/`realpath`/`date -d`/`stat -c`/GNU `sed -i`/`grep -P`/`grep --include`/`find -printf`/`xargs -r`/`install -D`/`cp --reflink`/`base64 -w`/`timeout`/`tac`/`wc -L`），**故意不设豁免表**——命中就改脚本；并在 `ci.yml` 的 `workflows-present` job 与 `check-workflow-shells.py` 一起执行。`scripts/ci/test-script-portability.py` 对每条规则各注入一次违规并断言退出 1，同时断言干净脚本、注释里提到关键字、`portability-check:ignore` 单行豁免与空目录 fail-closed 四种情形——写不出违规的门禁和仓库已修好长得一模一样，其中两条规则（`sed -i` 的否定预查、`base64 -w0` 的词边界）正是被这组夹具测出漏报后修正的。**验证**：`python3 scripts/ci/test-script-portability.py` 5 tests OK；`check-script-portability.py` 对 15 个脚本 OK；`bash -n` 覆盖 `scripts/*.sh`、`scripts/ci/*.sh`、`scripts/hooks/pre-commit` 全部通过；`secret-scan.sh` 三种模式（全量 3842 files、`--stdin` 单文件、`--stdin` 空输入）行为不变；`l10n-guard.sh --before HEAD --after WORKTREE` 实测 before/after 各 388 个含汉字文件、regressed 0、fortress-breach 0、退出 0；`local-publish-host.sh` 的键名推导对 6 个 platform 逐一与旧 `${var^^}` 结果比对相同；`check-workflow-shells.py` 与 `git diff --check` 干净。（2026-10-02；`scripts/l10n-guard.sh`、`scripts/ci/secret-scan.sh`、`scripts/ci/local-publish-host.sh`、`scripts/ci/check-script-portability.py`、`scripts/ci/test-script-portability.py`、`.github/workflows/ci.yml`、`CONTRIBUTING.md`；私有 goal scratch `l10n-guard-portable.log`、`test-platform-ps1-portable.log`）
- [x] 根目录六个调试脚本（`capture_listener.py`、`mock_server.py`、`run_mock_server.sh`、`single_mock_server.py`、`test_simple_wb.py`、`test_workbuddy_headers.py`、`test_workbuddy_headers_v2.py`）：**保持原位，不搬也不删**。它们是**上游自己放在仓库根**的（上游提交 `f380bbca`「test(tools): add mock inference server and WorkBuddy header capture scripts」，本地同一提交），不是散落的本地文件；挪进 `scripts/dev/` 会让每次上游同步都在这条路径上冲突，属于"碰架构"。
- [x] 两项延后的上游变更已按当前源码实测复核并给出结论。`oniguruma` 2→3：**本仓库已不存在该依赖**（`Cargo.lock` 14,215 行内无任何 `onig` 包，全仓 `Cargo.toml`/`.rs` 也无引用），因此上游这条升级在此分叉无对应改动，关闭而非“继续延后”。MCP admission 放宽：**维持不移植（决定，非待办）**——分叉已实现并测试受管准入闭环（`crates/codegen/xai-grok-shell/src/session/managed_mcp.rs` 在合并点丢弃命中 `deniedMcpServers`/非空 `allowedMcpServers`/`allowManagedMcpServersOnly` 的服务器并记录 `MCP server blocked by managed settings policy`；`crates/codegen/xai-grok-workspace/src/permission/managed_policy/mcp.rs` 实现正向 allowlist 才放行），放宽会削弱这道 fail-closed 安全闸，且用户指南 `07-mcp-servers.md#被组织策略拦截` 一节已移植；若将来要放宽须另立安全评审。（2026-10-02；`Cargo.lock`、`managed_mcp.rs`、`managed_policy/mcp.rs`）
- [~] Implemented read-only `chaos telemetry status [--json]`; loads effective on-disk config through the policy-aware loader and existing telemetry resolvers, reporting mode/source, trace-upload source, Mixpanel enabled state without token, and external OTEL activation/exporter names without collector URLs or headers. Durable integration tests invoke the built `chaos` binary with isolated config/env and assert JSON fields, config-root/source, human output, external OTEL env activation, omission of both configured and environment-provided endpoints/token/headers, and failure on malformed TOML; CLI parser tests cover human/JSON modes (2 integration + 1 parser test pass, 2026-10-01). `disable`/`enable` remain unimplemented until a reviewed contract for requirements pins, precedence and atomic config edits exists; scope clarification is recorded in ADR-007. (`crates/codegen/xai-grok-pager-bin/src/telemetry_status.rs`, `tests/telemetry_status.rs`; offline targeted Cargo tests)
- [ ] `docs/known-issues/wsl-p9io-crash-20260728.md` 明确为未外发本地存档；Windows 主机/wsl 版本、最小复现和完整 prior-boot logs 缺失。实际补充需 Windows host; 是否上报 external issue 由报告 owner 决策。
- [x] 在贡献文档里固化 WSL/低内存机器的 `CARGO_BUILD_JOBS=4` 建议，避免并发编译耗尽内存/磁盘；注明 `target/` 清理仅限可再生成的 debug/incremental 产物并链接已归档事故。（2026-09-25；`CONTRIBUTING.md` §Low-memory builds）

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
- [ ] Q4 2026 maintenance cycle: complete source-by-source MT-row review alongside ignored-test debt, record each decision/owner/next review date, and refresh release-gate evidence. The ignored-test half closed on 2026-10-02: all 428 attributes carry an in-source reason with owner and 2027-01 review date, 1 test restored, and a no-exemption `--require-reasons` CI gate prevents regressions (see MT-5 and `docs/ignored-audit-2026q4-summary.md`). The remaining MT-row review still needs the release-gate evidence refresh, and this row does not substitute for external platform/security gates or release go/no-go approval.

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
