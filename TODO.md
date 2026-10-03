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
- [~] `ADR-005` local Web security baseline frozen, see `docs/architecture/adr-005-web-security.md`; development `/health`/`/api`/`/ws` proxy only targets loopback backend, which enforces Host/Origin/token/Safe Web Mode. Proxy-fronted TLS is now a supported shape rather than an impossibility: `CHAOS_WEB_PUBLIC_ORIGIN` declares the public name (bare `https` origin, requires `CHAOS_WEB_TOKEN`), the `https` Origin is accepted only for that name and only with `X-Forwarded-Proto: https`, and every 401 says which rule fired; a Docker lab drives the built binary behind stock nginx over a lab-issued certificate chain and covers credential rotation and a real `wss:` session (`scripts/web-deployment-in-docker.sh`, transcript `docs/verification/web-deployment-tls-linux-2026-10-02.log`). Still separate reviews: certificate issuance/revocation, CDN behaviour, rate limiting, audit-log retention, Tauri IPC, remote links and secret storage.
- [x] `ADR-006`：前端采用 clean-room React/TypeScript 重写；不复制参考源码/资产，Chaos UI 仅通过版本化 Rust protocol 消费 engine，未来更新以本仓库审查为准。（2026-09-24；`docs/legal/ui-source-baseline.md`、`docs/architecture/adr-006-gui-source.md`）

**ADR 必须回答**：备选方案、选择理由、兼容影响、失败模式、迁移和回滚。

### M-1.5 依赖与平台 spike

- [~] Tauri v2 spike：Desktop host boundary 已隔离且不引入 Tauri 依赖；真实 Tauri 三平台构建待环境依赖与独立 runner，不能以 host crate 通过替代。
- [~] Axum + 静态资源部署：loopback HTTP/WebSocket 已运行，`CHAOS_WEB_ASSETS_DIR` 可选静态目录托管及 SPA fallback 已实现/测试；真实 release binary+Vite dist 的桌面/窄屏 browser flow、资源字节/MIME、missing asset 404、health/API 与 WebSocket session create 已通过。静态目录可通过 `.gz`/`.br` 变体协商压缩；静态文件 SHA-256 ETag/If-None-Match 304 及 SPA HTML no-cache 行为已有真实 handler/browser 回归。前端资产仍未嵌入 binary；CDN cache invalidation 和 CI/release 默认产物接线仍待 M5。（2026-09-30；`xai-grok-web/src/lib.rs`、`apps/chaos-ui/e2e/static-host.pw.ts`）
- [x] Rust→TS：M0 使用 serde JSON envelope；`chaos-protocol-schema` 生成 TypeScript 类型并由 `scripts/ci/check-gui-protocol.sh` 执行漂移门禁，GUI CI 已运行；运行时 schema validation 仍待后续边界。（2026-09-24）
- [ ] M-1/M4 SSH spike remains open: no maintainer-selected client/auth scope or approved ProxyCommand trust rules, and this Linux session has no approved SSH host, disposable key/agent or forwarding lab. This is now specifically about SSH: a non-SSH remote transport (`chaos-remote-server`/`chaos-remote`, loopback + tunnel per ADR-004) exists and has a Docker acceptance lab, but that is not SSH evidence and does not close host-key handling. Keep M4.2/M4.4 SSH-specific rows open pending decision and controlled runner. (2026-10-02; `docs/architecture/adr-004-remote-topology.md`, `scripts/remote-acceptance-in-docker.sh`)
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
- [~] 提供最小 Provider、Base URL、model slug 和 API Key 配置路径；`CHAOS_AGENT_BINARY` 之外新增 `chaos-engine::provider::HttpPromptAdapter`，用 `CHAOS_PROVIDER_BASE_URL`/`CHAOS_PROVIDER_MODEL`/`CHAOS_PROVIDER_API_KEY` 接任意 OpenAI 兼容端点，端点派生 `/chat/completions` 与 `/models`，明文 `http:` 仅限 loopback，`user:pass@`/query/fragment 直接拒绝，TLS 走仓库统一 rustls 策略（支持 `GROK_EXTRA_CA_BUNDLE`）。凭据只从 Web host 进程环境读取，GUI protocol 里不出现 Key；启动时对 `/models` 做健康检查并打印结果，配置无效则以非零码退出且不监听。`CHAOS_WEB_SQLITE` 此前会丢弃已配置的 prompt adapter，现由 `Engine::with_sqlite_store_and_adapter` 保留并有回归测试。仍开放：GUI 配置表单与操作系统钥匙串凭据存储（M3）。
- [x] 当前 GUI protocol 不接受 API Key，Web token 仅使用 Authorization header，engine 不记录或返回凭据；真实 provider credential storage 待 M3。（2026-09-24）
- [x] 空模型目录、无凭据、401/429/5xx、网络断开均有可操作错误提示；Web adapter 把 headless 启动/JSON/非零错误映射为 `agent_failed`，provider 专项错误已由自建推理端点上的真实 WebSocket 测试逐项覆盖：401 提示检查服务端 API Key、403/404（提示 Base URL 是否含 `/v1`）/429（限流稍后重试）/5xx 各有 status hint，连接失败与超时分别报「无法建立连接」/「连接或读取超时」，缺 Key 时确认未发送 `Authorization` 头，坏帧报「无法解析」，端内 `error` 帧不被当作正文，空补全报「未返回任何文本」，`/models` 空目录被判为「配置的 model 未被列出」；错误文本与 `Debug` 输出里的 Key 一律替换为 `[redacted]`，并有断言检查事件序列 `Debug` 文本不含凭据。（2026-10-02；`crates/codegen/chaos-engine/src/provider.rs`、`crates/codegen/xai-grok-web/tests/provider_http_flow.rs` 13 tests、`tests/host_startup.rs` 4 tests）

### M0.4 前端最小闭环

- [~] React 已接入真实 WebSocket，支持会话、timeline、streaming、审批/问题、停止和工具活动卡片；Chromium desktop/mobile 验证 workspace isolation、文件流和 composer。E2E runner 自动选择空闲 UI/backend 端口并通过受控 Origin 配置连接；Vite 代理 `/ws`、`/api`、`/health` 到 loopback Web routes，由后端检查 Host/Origin/token/Safe Web Mode。Proxy-fronted TLS 部署形态已实现并被 Docker lab 驱动验证（`CHAOS_WEB_PUBLIC_ORIGIN` + `X-Forwarded-Proto` 规则、401 带拒绝原因码、真实 `wss:` 会话、credential 轮换）；证书签发/吊销、CDN、限速与审计日志留存仍是独立 gate。（2026-10-02；`apps/chaos-ui/e2e-runner.mjs`、`e2e/workspace-flow.pw.ts`、`e2e/tool-activity.pw.ts`、`scripts/web-deployment-in-docker.sh`）
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

本机浏览器路径现有可重复的 Playwright Chromium E2E 与 GUI CI job；本地 desktop/mobile 两个项目各通过（全命令 4 tests passed）。远端已确认 clean-runner 缺 dotslash，且 Git test 未设置隔离 user identity；第三次运行 browser 首次编译超过 180 秒，现加入预编译和较长 timeout，远端 browser 与 GUI CI 已通过；当前 Clippy 修复的完整 Rust gate 已本地验证，commit `c324306f` 的远端 check/strict Clippy 已通过；full test job 暴露 sandbox 自动套接字 deny 在不可读容器运行时路径上的误报，以及 LSP mock push 在受压时快于 pending 标记的时序竞争。现已跳过不可读 endpoint（child network filter 仍负责网络隔离）、使 fixture 等待报告并为 mock analysis 加调度间隔；两个 targeted tests 均通过，完整 workspace 本地重跑已发现 Git invalidation fixture 未隔离全局 ODB permit并过早释放第二个 walk；fixture 改为两个 permit 且在 release 前等待第二 walk 开始，回归通过。workspace fmt/check/strict Clippy/test 全量已全部通过；输出见未留存日志（未随仓库提交）`rust-workspace-final-suite.log`。真实 provider 已用自建 loopback 推理端点闭环（`crates/codegen/xai-grok-web/tests/provider_http_flow.rs`、`tests/host_startup.rs`）；Tauri 桌面入口仍为独立 gate。

- [x] 自动测试覆盖提交成功、取消、重复 submission、断线、重连、snapshot fallback、无凭据和 provider 错误；当前已覆盖真实 WebSocket submit/completed/cancel、重复/dedup、UTF-8 delta、adapter error boundary、跨 engine 重启 resume、认证/Origin/Host 和安全头；provider 错误已用自建推理端点闭环（含 4 路并发会话各自独立流式、且产生 4 次上游调用），并新增直接驱动 `CARGO_BIN_EXE_chaos-web` 二进制的启动路径测试（provider 回复流式回传、端点不可达、非法配置以非零码退出且不监听、无 provider 时仍回落到 demo responder）。**最后一处开放边界（浏览器层的断线/重连/snapshot fallback 故障注入）已闭合，并且一补就上两个真缺陷**（2026-10-03；`apps/chaos-ui/e2e/reconnect-snapshot.pw.ts` 新增 3 例，`playwright.config.ts` 的三处 `testMatch` 收录；`docs/verification/web-host-reconnect-2026-10-03.log`）。新用例不重载页面、只用 `page.routeWebSocket()` 关掉页面那一侧的套接字，第三条直接 `spawn` 真实 `target/debug/chaos-web` 后 `SIGKILL` 再起一个——故障是真的，不是 mock。抓取方式是先记录而非先断言：两侧每一帧落盘，`addInitScript` 里用 MutationObserver 把状态徽标出现过的每个值记进 `window.__chaosStatusSeen`（恢复会在半秒内穿过 连接断开，正在重连 → 已连接 → 历史已恢复，轮询渲染文本会漏掉中间态；这个技巧保留在正式测试里）。缺陷一：`ServerMessage::Workspaces.active_workspace_id` 不是 optional，主机没有活动工作区时报 nil UUID 占位符，UI 把它当真 id 存下并在重连时回 `create_session workspace_id="00000000-0000-0000-0000-000000000000"`，主机只能在登记表里找它，于是回 `workspace_unavailable`；修法是把占位符显式命名为 `NIL_WORKSPACE_ID` 并经 `activeWorkspaceIdOrNull()` 过滤后再回传，`workspaceChanged` 的 `workspaceId` 同时改为可选、无工作区时把 `sessionId` 置空（原来保留上一个工作区的会话）。缺陷二：会话活在进程内存里，主机重启即全丢，重连时 `resume(旧 id)` 吃到 `session_not_found` 后**没有任何东西接管**，UI 只把徽标刷成「请求错误」就停住，而 `submit()` 在没有 session id 时直接 return——输入框不是报错而是静默失灵；修法是新增 `sessionLossRecoveryMessage()`，对 `session_not_found` 与 `workspace_session_mismatch` 两个 code 主动申请一个主机能答的新会话。**两侧都要动**：`chaos-engine/src/lib.rs` 新增 `selected_workspace()`，把入站 nil 读作「未选」（`CreateSession` 与 `Resume/Snapshot` 臂各用一处），因为那两个错误的产生点都在主机侧，只改 UI 的话别的客户端发同一字符串仍然会红。修复前的症状在真实 shipped 路径上重放过（把 UI 两处改回去、`dist` 重新 build）：`Expected: "会话已创建" / Received: "请求错误"`，桌面与移动两个视口同时红，然后写回原文 `cmp` 校验字节一致。测试量：`npm test` 38 通过/5 文件（`session.test.ts` 16→18 例，新增「占位符视为无工作区」与「主机不再认识当前会话时申请替换会话」两条纯 reducer 测试）；`cargo test -p chaos-engine --test workspace_session` 4 通过（`placeholder_active_workspace_id_round_trips_as_no_workspace_selected` 走 `ListWorkspaces`→断言占位符→`CreateSession`→`Resume` 完整回环，`unknown_workspace_id_is_still_refused` 是同一段代码的反向夹具，防止「过滤」退化成「不校验」）；Playwright 6 通过（桌面+移动 ×3 条，11.3s）。**非空证明 7 处变异，0 存活**：R1 不翻译入站 nil → 报 `workspace_unavailable`；R2 用原始 id 判空 → 报 `workspace_session_mismatch`；R3 过滤函数直接返回 `None` → 反向夹具红；U1 不翻译占位符、U4 无工作区时保留旧 sessionId → 占位符那条 reducer 测试红；U2 回传不过滤、U3 少认一个 code → 恢复那条红。U2 值得单独记：把 `activeWorkspaceIdOrNull` 的过滤去掉，占位符那条测试**仍然绿**（它测的是存储路径，U2 动的是回传路径），红的是恢复测试里的 `workspace_id: null` 断言（`+ "workspace_id": "00000000-0000-0000-0000-000000000000"`）——这正是两个函数必须分开、且两侧各要有一条夹具的原因。正式测试注释里那句「关服务器那一侧的腿传不到页面」原本只是「我记得」，现在有记录：同一页面里先 `server.close()` 等 4 秒（腿数仍 1、徽标仍 `会话已创建`、无新帧），再 `route.close()`（腿数立刻 2、出站 `list_workspaces, resume`）；关错一侧的话这 3 条会全部「通过」而什么都没注入。一处断言被变异纠正过：原本写 `expect(firstHost.exitCode).not.toBeNull()`，但被信号停掉的进程 `exitCode` 本就是 `null`，现断言 `signalCode === 'SIGKILL'`，`finally` 的清理也改成 `exitCode === null && signalCode === null` 才补刀。边界：Tauri 主机未被 e2e 覆盖（桌面壳的套接字随窗口生死，「主机重启」在 Tauri 侧目前只有协议层的 Rust 用例覆盖）；重连退避的次数没钉成常数（只要求最终接上）；预览代理的 HMR 套接字由 `xai-grok-web` 的 Rust 集成测试覆盖，浏览器侧 HMR 断线不在范围；恢复出来的是空会话而非旧 transcript（会话在内存里，UI 变绿不等于用户回到原对话）。
- [~] Desktop host 仅通过 shared Engine dispatch unit test；无 Tauri executable/desktop WebView entry point，不能从此 host boundary drive visible path。Tauri build/desktop restart/cancel/streaming 与 macOS/Windows/Linux checks remain gated on dependency/runner availability.
- [~] Repository Playwright Chromium E2E 与 Linux CI 覆盖真实 browser→Vite→WebSocket→Engine flows。Desktop 与 narrow/mobile tests 验证 workspace create/submit/switch/reload/archive transcript isolation、layout restore、health/handshake、empty/cancel、multiline composer、GFM rendering、raw script not executed、external-link rel/target. Frontend checks/build pass. CI install fixes: dotslash/protoc, local Git identity test setup, prebuilt Web host and 35-minute cold browser build allowance. Engine Clippy warnings-as-errors 已修复（跨平台 dunce canonicalization、collapsed if、simplified boolean）；本地 strict workspace Clippy/check/test pass。首次 full workspace rerun 揭示 sandbox runtime socket inaccessible path 被 permission error 阻断及 LSP test pending/report event race、Git gate fixture permit race；已修正路径行为并添加回归/同步 fixtures，最终 full local run pass，CI run `36094218127` 全部 jobs pass。Local proofs `{SCRATCH}/playwright-markdown-final.log`、`frontend-final.log`、`ignored-tests-unit-final.log`、`ignored-baseline-final.log`、`rust-workspace-final-suite.log`、`final-l10n-guard.log`。真实 provider 已用自建推理端点在真实 WebSocket 与真实 `chaos-web` 二进制两条路径上闭环；keyring 凭据存储、Tauri host 和跨平台安装仍依赖平台/凭据决策。
- [~] Linux engine/Web/前端实测已记录。CI 新增 `platform-tests` job（`macos-14` + `windows-latest`），对 `xai-tty-utils`、`xai-grok-sandbox`、`xai-grok-shell-terminal`、`xai-grok-update`、`xai-grok-tools`、`xai-grok-pager-bin` 跑真实 target-OS 测试；同一 crate 集合同时提供本机入口 `scripts/test-platform.sh` 与 `scripts/test-platform.ps1`（报告 OS/`rustc -V`/CPU/commit 后执行 `cargo test --locked --no-fail-fast`）。本机已用该脚本对 `xai-tty-utils` 实跑：62 lib tests + 2 doctests 全通过；PowerShell 入口通过 PowerShell 解析器语法校验，但其 Windows 实跑结果仍需在 Windows 机器上取得。2026-10-02 用 `gh run list/view` 复查远端结论，确认此前的「platform tests 失败」是误读：`concurrency.cancel-in-progress: true` 使连续 push 把在跑的 run 全部取消（`901ff933 3aa3c6c8 39c31f69 023363d5 6bb6043e 228ccf83 1c85ca08` 的 run conclusion 均为 `cancelled`），而 `platform-tests` 因 `needs: rust` 最后启动，被取消时 check-run 显示为不通过；唯一 `completed` 的 `8f26dcc8` 是 `rust` job 的 `cargo test` 失败导致 platform job 被 skip。也就是说 macOS/Windows 至今没有产出过任何一次真实结果，不是失败而是从未运行。已把 `cancel-in-progress` 收紧为仅 `pull_request` 生效，并给 platform job 加 `if: failure()` 的 `::error::` 环境上报步骤（Runner OS/arch、`rustc/cargo/rustup/protoc` 是否就位），因为无仓库管理员权限时 job log 是 403、而 check-run annotation 公开可读。GUI 与 Tauri 冒烟仍待补。**2026-10-02 首次拿到真实 macOS/Windows 结果**：run `36959811483`（commit `fef61670`）是 platform job 第一次真正跑完（此前每次要么被 `cancel-in-progress` 取消、要么被 `rust` job 失败连带 skip），platform 集合共 33 个测试 target，其中 30 个 0 失败，3 个失败，四类根因全部定位并修好：（1）Windows 测试构建直接编译失败——`terminal.rs` 两个 `#[test]` 调用了 `#[cfg(unix)]` 的 `parse_login_env_capture`（`E0425`×2、`E0282`×4），因此 Windows 侧此前从未执行过任何测试；测试补 `#[cfg(unix)]`，并清掉同 job 报出的全部 warning（仅 unix 读取的 `login_shell_capture` 字段、`collect_shell_state_dumps` 的未用 `task_ids`、`unified_log` 测试里多余的 `mut`、无 unix 调用者的 `default_shell_path`），Windows 构建现为无告警。（2）macOS `xai-grok-sandbox --lib` 68/8：7 条是 `UnixListener::bind` 的 `InvalidInput "path must be shorter than SUN_LEN"`——macOS 的 `TMPDIR` 位于 `/var/folders/…/T/`，而 `sun_path` 只有 104 字节，fixture 路径在到达被测代码之前就溢出；第 8 条是同一根因的另一面（`/var`→`/private/var` 使 fixture 路径看起来像无关 deny）。新增共享 fixture `test_util::short_socket_root`（canonical + 实际 bind 探测，装不下则退回 `/tmp`），`read_deny_verify_tests` 的 `temp_workspace`/`temp_parent` 与 `runtime_sockets_tests` 的 fixture 全部改用它；本机用与 macOS 同形（长且经 symlink）的 `TMPDIR` 复现，修复前 117 通过/2 失败，修复后两种 TMPDIR 均 119 通过/0 失败。（3）macOS `xai-grok-tools --lib` 3104/46/3：绝大多数是 grep/glob 在 spawn `rg` 时 `No such file or directory`——debug 构建（`cargo test`）不内嵌 rg，而 CI runner 的 PATH 里也没有，这是**产品缺口而不只是测试缺口**（桌面启动的进程不继承 shell PATH）；`rg_path()` 重写为可测的 `resolve_rg()`（`RG_BIN_PATH` → bazel runfile → `PATH` → `/opt/homebrew/bin`、`/usr/local/bin`、`~/.cargo/bin`，失败信息给出可执行处置），并新增 `--version` 自检的 CI/脚本供给步骤（`.github/workflows/ci.yml`、`scripts/test-platform.sh`、`scripts/test-platform.ps1`）。（4）macOS `xai-grok-pager-bin --test update_never_blocked_by_config` 0/1：测试打真实 GitHub API 并撞上 403；改为在 loopback 上自建 `/releases/latest` 与下载端点，并断言请求确实落在本地服务器上（防止「其实还是打了线上」这种假通过）。**该修复已在完全没有出网路径的网络命名空间里复跑验证**：`unshare -rn` 内对 `https://api.github.com/` 的探测返回 `000`（无路由），同一命名空间里该 target 1 通过/0 失败，因此结果不再取决于线上仓库是否有已发布 release。此外 LSP `a_server_that_announces_it_is_ready_is_waited_on_again` 原本靠固定 sleep 撞时序，在 macOS 上单独失败；改为等待两个 drain 之前就可观测的单调状态（服务器已被判定沉默、且刷新公告已待处理），并用变异检验确认它真的会失败：注释掉 `reopen_refreshed_questions` 里的 `note_server_spoke()` 后该测试以预期信息失败。本机 `xai-grok-tools --lib` 现为 3161 通过/0 失败/3 忽略。当前 `rust` job 唯一失败的 `xai-grok-pager-minimal::committed_edit_keeps_diff_line_backgrounds` 也已处理：该断言读取进程级的主题、色深与 terminal-native 锁三者状态，取到无色主题时主题与单元格同为 `Reset`，断言会失去意义；6 条渲染断言现统一先 `theme::cache::pin_theme()`，并显式断言所用主题的 `diff_insert_bg` 不是 `Reset`，失败信息同时打印主题、色深与锁定状态。把 `ChangeTag::Insert` 的背景改为 `None` 的变异检验确认该测试确实会失败（`wanted Rgb(6, 56, 6), the buffer only had ["Reset", "Rgb(66, 14, 20)"]`），还原后复跑通过。**这些修复的本机证据不能替代 macOS/Windows 实机结论**，需在新 HEAD 上重跑 platform job 才能关闭本行。**2026-10-02：platform job 首次真正启动**。`rust` 转绿后 run `36979345879`（commit `a206ec33`）的 `windows-latest` 与 `macos-14` 第一次进入运行，但 Windows 侧在 `Provide ripgrep for the search tools` 这一步就失败（exit 127：`/d/a/_temp/ripgrep-15.0.0/rg.exe: No such file or directory`），`cargo test` 被 skip——`ripgrep-15.0.0-x86_64-pc-windows-msvc.zip` 把 `rg.exe` 放在同名顶层目录里，而该步骤 `Expand-Archive -DestinationPath .` 之后直接引用上一层路径；tar 分支靠 `--strip-components=1` 摊平了同一层，所以只有 Windows 中招，而 `scripts/test-platform.ps1` 本来就用 `Move-Item` 处理了这一层。ci.yml 现与之一致，并在导出 `RG_BIN_PATH` 之前断言文件确实存在、否则打印目录内容后退出。验证方式：把该步骤脚本原文取出，在本机跑它的 Windows 分支（只把 `Expand-Archive` 换成写出同一目录树的 `python3 -m zipfile -e`）——`rg.exe` 出现在导出路径上，是 4,265,472 字节的 PE（`MZ`）二进制，脚本以 126「cannot execute binary file」结束，那是 Linux 拒绝**执行** Windows 可执行文件，恰好证明文件已就位；修复前同一段脚本在这里报的正是 runner 那句 127。job 结论与失败原文见 `docs/verification/platform-ci-2026-10-02.log`。**Windows 至今没有跑到任何一条测试，macOS 的结果也不在该文件证据之内**，两者都要由修复后的 workflow 重跑产生。**2026-10-02 重跑（run `36995349578`，commit `60e215fe`）**：`rust` job 完成且通过（11:12:14Z→11:55:09Z，含上一条修掉的 pager 时序 flake，说明该修复在 CI 上也站住了），`platform tests (macos-14)` 首次 `completed`/`success`——这是 macOS 第一次给出真实的全绿结论；`platform tests (windows-latest)` 首次 `completed`/`failure`，失败点已从「环境搭建」推进到 `cargo test` 本身：`error[E0425]: cannot find function process_not_running in the crate root`×4，位于 `crates/codegen/xai-tty-utils/src/kill_on_drop_tests.rs`，因为该函数原本只有 `#[cfg(unix)]` 版本（Linux 读 `/proc/<pid>/stat` 判断僵尸，其它 Unix 退化为 `ps -o stat=`）。修复不是给测试模块加 `#[cfg(unix)]`——那会得到一条什么都不检查的 Windows leg，因为原 fixture 直接 spawn `sleep`/`true`——而是补上真正的 Windows 实现（`OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` + `GetExitCodeProcess` 比对 `STILL_ACTIVE`；Windows 无僵尸，进程一退出即不再报告 `STILL_ACTIVE`），并把 fixture 改成跨平台（Windows 用 `ping -n 300 127.0.0.1`、退出码夹具用 `cmd /C exit 0`），同时用 `#[cfg(target_os = "linux")]` 收掉只有 Linux 测试用到的导入。本机 `cargo check --all-targets` 对 `x86_64-pc-windows-msvc`、`aarch64-pc-windows-msvc`、`x86_64-apple-darwin` 三个 target 复查均为 0 error/0 warning（修复前该 crate 的 lib test 报 4 条 warning）。**仍开放：Windows 上这三条测试的真实执行结果**——交叉 `check` 只证明编译得起来，不证明跑得起来，需下一次 push 后的 Windows leg。（2026-10-02；`.github/workflows/ci.yml`、`scripts/test-platform.sh`、`scripts/test-platform.ps1`、`CONTRIBUTING.md`、`crates/codegen/xai-grok-tools/src/implementations/grok_build/grep/ripgrep.rs`、`crates/codegen/xai-grok-sandbox/src/test_util.rs`、`crates/codegen/xai-grok-pager-bin/tests/update_never_blocked_by_config.rs`、`crates/codegen/xai-grok-pager-minimal/src/commit_tests.rs`、`docs/verification/platform-ci-2026-10-02.log`）。同一次复查还查清了一个一直在红的 job：`installer integrity labs` 死在前置步骤里的 `unshare -rn true`——GitHub 托管 ubuntu runner 关掉非特权用户命名空间（`write failed /proc/self/uid_map: Operation not permitted`），而这一句被放在装 crypto 的那步，于是把同 job 里本来不依赖用户命名空间的 `install-integrity-in-docker.sh`（`docker run --network none`）一起拖成 skip。现在前置步骤只打印本机能走哪条路、不再断言，命名空间由 lab 自己解析（`unshare -rn`，否则 `sudo -n unshare -n`；都没有才 exit 2 报原因），root 那条刻意不带 `-r`（进了子用户命名空间就对树外资源没有特权，实测连自己的脚本都读不到）。两条路都用 `CHAOS_PS1_LAB_NS_ROUTE` 在本机各跑 30/30 通过，`github.com cannot be resolved from this namespace` 这条前提两条路上都真实成立。另修掉一处会让任何 lab 在恰好一小时后假报产品缺陷的缺陷：四个 lab 都用 `--entrypoint sleep <image> 3600` 空跑容器，一小时到点容器 PID 1 退出、正在跑的 `docker exec` 被 SIGKILL，`install-sh-in-docker.sh` 就是这样把 150 MB 下载超时上报成「install.sh exited 137」；上限改为 `CHAOS_LAB_KEEPALIVE`（默认 6 小时并打印），保留有限上限是为了进程被硬杀、`trap` 没跑时不会永久留下容器。（2026-10-02；`scripts/install-integrity-powershell.sh`、`scripts/install-sh-in-docker.sh`、`scripts/install-integrity-in-docker.sh`、`scripts/npm-install-in-docker.sh`、`scripts/remote-acceptance-in-docker.sh`、`.github/workflows/ci.yml`、`docs/verification/platform-ci-2026-10-02.log`）**2026-10-03：Windows 腿此后唯一的失败定位到 CI 自身并已修好**（run 313/314 的 `platform tests (windows-latest)` 第 3 步 `Parse the PowerShell installers`）。`scripts/ci/test-installer-asset-names.py` 在 `subprocess.run(["bash", "-c", ...])` 上失败，原文 `FAILED running the shipped detect_platform failed:` 冒号后为空，即退出码非 0 而 stderr 空；CRLF（本机复现 rc=2 且语法错误可见）与打桩 MINGW `uname`（rc=1 且 `use PowerShell` 消息在 stderr，属按设计豁免）两条无辜解释逐一排除后，剩下解释器本身：Windows runner 的 `PATH` 把 `C:\Windows\System32` 排在 `C:\Program Files\Git\bin` 之前，`System32\bash.exe` 是 WSL 启动器，无发行版时退出非 0 且把抱怨写在 stdout。新增 `find_bash()`（`$BASH` → Git for Windows 三个安装根 → `PATH`，Windows 上丢弃含 `system32` 的候选，且每个候选必须真跑一条 `printf ok`；取不到时 POSIX 记 failure、Windows 记 note），并把 `install.sh` 的四条出资产分支与三条拒绝分支从「正则读赋值推出」改为打桩 `uname` 后**逐条真跑** `detect_platform`（断言 `ASSET` 与存盘用的 `PLATFORM` 双值、以及 `use PowerShell`/`unsupported OS`/`unsupported arch` 三条原文），`case` 里新增 `OS_KEY=` 而无对应探针行即判失败。新增 `scripts/ci/test-installer-bash-resolution.py`（13 用例，含「System32 下能用的 bash 在 Windows 必须落选、在 POSIX 必须照常可用」与「取不到 bash 时不得看起来像全绿」），5 个变异逐个注入各自变红（删 system32 过滤 → 2 红；候选不执行即接受 → 3 红；探针表写错 Darwin/arm64 → 检查红；`install.sh` 写错 `ARCH_KEY` → 检查 4 条红；改掉 `use PowerShell` 措辞 → 检查红），每次 `cmp` 校验还原逐字节一致；第一版 System32 用例删掉过滤也不红（排列里 Git 本来就排前面），是变异测出来才改对的。已接进 `platform tests` 步骤、ubuntu `rust` 脚本检查串与 `scripts/verify-in-docker.sh` 的 `installer asset names` 门禁，见 `docs/verification/installer-asset-probe-2026-10-03.log`。**Windows 侧 `cargo test` 的真实结果仍未取得**：这一步在 `cargo test` 之前，修好它只是让那条腿有机会往下跑，本行保持 `[~]`。（2026-10-03；`scripts/ci/test-installer-asset-names.py`、`scripts/ci/test-installer-bash-resolution.py`、`.github/workflows/ci.yml`、`scripts/verify-in-docker.sh`、`CHANGELOG.md`、`docs/verification/installer-asset-probe-2026-10-03.log`）**同一天再拆掉一个让那条腿永远拿不到结论的原因**：run `37074944913`（commit `f585f59d`）里 `platform tests (windows-latest)` 的第 1–8 步（含上一条刚修好的 `Parse the PowerShell installers`、`Install protoc`、`Read pinned toolchain`、`Install Rust`、`rust-cache`、`Provide ripgrep for the search tools`）全部 `completed/success`，第 9 步 `cargo test (target-OS crates)` 跑了 45m15s 后被取消，整个 run 的 conclusion 因此是 `cancelled`。上限来自 `timeout-minutes: 45`——那是写下这个 job 时（`da5e1ee0`）拍的数字，从未与任何一次真实运行对照过；同一次 run 的 `macos-14` 腿（同一批 crate、同一缓存键、而且是两者中较快的 arm64 runner）实测 31m17s 才跑完，Windows 冷构建更慢，45 分钟对它不够。这种取消在 `gh run list` 里与并发取消长得一模一样，必须逐个 run 查 job 数才能区分：当天另有 5 个 `cancelled` run 的 `repos/.../actions/runs/<id>/jobs` 返回 0 个 job，即排进并发组就被丢弃、一个 runner 都没起，与超时是两类原因，而且前者不是配置能修的：`cancel-in-progress: false` 只保护**已经在跑**的 run，新 push 仍会丢弃同一 ref 排队中的 run，能做的只有把做完的切片攒成一次 push（本次即按此办理）。现把该 job 抬到 75 分钟，并把上面两个实测数字写在设置正上方。**这仍然不是 Windows 测试通过的证据**：抬上限只是让 `cargo test` 有机会跑到出结论为止，本行保持 `[~]`，真实结论要等下一次批量 push 后的 Windows leg。（2026-10-03；`.github/workflows/ci.yml`、`CHANGELOG.md`、`docs/verification/platform-ci-2026-10-02.log`） **2026-10-03：Windows 腿第一次真的跑到了测试，而它 45m15s 里有 16m43s 卡在一条测试上，根因是一个测试夹具字符串。** 上一条把上限抬到 75 分钟之后，run `37074944913`（commit `f585f59d`）job `111075816970` 的步骤日志终于取到，时间线是：23:48:37 进步骤 9 `cargo test --locked --no-fail-fast`；编译 25m43s，00:14:20.97 才出现第一条 `test result:`；另外五个测试二进制在约 4 秒内全部收尾；00:14:24.04 `xai_grok_tools-c0c13642df1bb12b.exe` 起，`running 3087 tests`；**2960 ok / 124 FAILED / 2 ignored / 1 条永不返回**；00:15:45.283 打印 `test implementations::grok_build::task::tests::cwd_and_worktree_isolation_are_mutually_exclusive has been running for over 60 seconds`；00:15:59.274 是任何线程最后一条判决；此后 16m43s 全静默，00:32:42.023 `##[error]The operation was canceled.`。**这不是慢，是一条测试再也没回来**：libtest 逐条打印判决、却要等全部线程返回才打印 `test result:` 汇总，所以一个挂住的测试会连带抹掉整个二进制的汇总与 `failures:` 段——那 124 条失败**在全日志里没有任何断言原文**（该二进制输出里 `grep -c 'panicked at'` 为 0），此前一直「读不到失败原文」正是这个机制，而不是日志被截断（该机制由 libtest 的打印顺序推得，未复现一次挂住的 libtest 运行）。 **根因**：`TaskTool::run` 判断 `cwd` 与 `isolation="worktree"` 互斥只靠一次存在性检查（`crates/codegen/xai-grok-tools/src/implementations/grok_build/task/mod.rs:468` 的 `is_some_and(|p| std::path::Path::new(p).is_dir())`）。测试写 `cwd: Some("/tmp".into())` 想表达的是「某个已存在的目录」：Linux/macOS 上 `/tmp` 在，走「拒绝」分支，毫秒级返回；`windows-latest` 上没有 `C:\tmp`，于是走「把 cwd 清掉去 spawn 子代理」分支，而这类测试**从不读自己的响应接收端 `rx`**（它预期在校验阶段就被拒），`Tool::run` 从此等一个永远不会到来的响应——挂在窗口外看就像机器慢。同模块另有 3 条同族夹具（`cwd_strips_stray_leading_quote`、`cwd_threads_to_request`、`cwd_with_isolation_none_is_allowed`）在 Windows 上是 FAILED 而非挂起，因为它们断言的是「spawn 出的请求里 cwd 是什么」，而那个 cwd 已被静默清掉；同模块用 `/nonexistent/...` 与 `/tmp/some-dir` + `resume_from` 的几条则两端都绿——**危险在于夹具依赖存在性，而不在于选了 `/tmp` 这个名字**。 **非空证明（改生产分支、不改测试）**：把该条件换成 `is_some_and(|_p| false)`，即让生产代码走上Windows 实际走的那条路，本机该测试即红，且红的正是理论预测的样子——`cwd + worktree must be rejected by validation, not wait on a subagent: Elapsed(())`，`finished in 30.00s`；还原后 `1 passed ... finished in 0.00s`，还原文件与变异前副本 `cmp` 逐字节一致。那 30 秒上限是本轮新加的：两条校验测试的调用被 `tokio::time::timeout(VALIDATION_TIMEOUT, ..)` 包住，将来再误入 spawn 分支会**命名地失败**而不是吃掉整个作业预算。 **修法**：测试里新增 `existing_dir()`（`std::env::temp_dir().display().to_string()`）替换 5 处 `cwd:` 字面量；同一族夹具另在 8 处替换（`xai-grok-shell-terminal/src/adapter_tests.rs`、`background_task.rs` 两处、`xai-grok-tools/src/computer/types.rs` 两处、`implementations/task_output/tool.rs`、`implementations/grok_build/task_output/mod.rs`），`computer/local/terminal.rs` 那 14 处（即 54 条失败那一族）改走 `shell_path()` / `pwd_reports_dir()`，断言仍校验真实 `pwd` 输出而非路径字符串。另有两条**有原文可读**的 Windows 失败同轮修掉：`streaming_local_terminal::tests::test_kill_returns_signal`（`left: None, right: Some("signal 9")`，Windows 的 kill 不上报信号号，改为按平台断言）与 pager-bin `corrupt_config_never_changes_update_outcome`（`os error 10106` Winsock 不可用——测试把 updater 初始化所需的系统环境变量也剥掉了，现由 `platform_essentials()` 透传）。
  **结构改动**：那条步骤原本一次跑六个 crate，于是最大 crate 里一个挂住的测试既吃掉剩余预算、又抹掉另外五个 crate 的汇总；现拆成 `cargo test (target-OS crates: except xai-grok-tools)` 与 `cargo test (target-OS crates: xai-grok-tools)`，各自 `timeout-minutes: 35`、第二条带 `if: always()`，作业总预算仍为 75 分钟。配套两个门禁见本轮新增的 `scripts/ci/check-workflow-yaml.py` 与 `check-spawn-cwd-portability.py` 扩充条目。**本行仍保持 `[~]`：本轮没有任何 Windows 机器**，上述数字全部读自别的机器执行的运行日志，变异证明在本机对同一生产分支做的；124 条里除去 54 + 3 之后的约 67 条（lsp 38、skill_discovery 7、resolve_model_path 5、read_file 5、bash 3、skills 2 及其余各 1）**没有断言原文、也没有声称的根因**，列出只为下一次 Windows 日志有可比对的已知集合。（2026-10-03；`crates/codegen/xai-grok-tools/src/implementations/grok_build/task/mod.rs`、`crates/codegen/xai-grok-tools/src/computer/local/terminal.rs`、`crates/codegen/xai-grok-tools/src/computer/types.rs`、`crates/codegen/xai-grok-shell-terminal/src/streaming_local_terminal.rs`、`crates/codegen/xai-grok-pager-bin/tests/update_never_blocked_by_config.rs`、`.github/workflows/ci.yml`、`CHANGELOG.md`、`docs/verification/platform-ci-2026-10-02.log` 的 `== 5.` 节）
- [x] 新增 `apps/chaos-ui/README.md`，记录 GUI 启动、`CHAOS_WEB_STATE`、测试和当前 provider/Tauri/远程限制。（2026-09-24）

---

## M1：核心对话、工具与安全审批闭环

**Owner**：TBD  
**目标日期**：TBD  
**依赖**：M0  
**交付目标**：完成核心时间线、工具展示、用户提问、权限审批和 Diff 审查；桌面和 Web 的危险操作都不能绕过审批。

### M1.1 会话投影与交互协议

- [~] Rust protocol/Engine/WebSocket covers actual ToolAdapter start/progress/result/usage, question, file/Diff changes and fail-closed errors with real integration fixtures. Real provider/MCP command execution/tool sourcing is not part of this protocol/UI mock seam and needs approved adapters/credentials.
- [~] Engine 维护 session sequence、dedup 和有界 snapshot；UI reducer 按当前会话投影服务端事件，并忽略来自先前会话的过期消息。sequence 仅附在部分消息上，尚未定义为可连续检测的全事件游标，因此不据此丢弃重复或判断缺口；需先冻结包含每类事件的恢复合同，再实现 delta replay、TTL/容量预算、snapshot/delta 竞争和真实 WS 丢帧恢复。（2026-09-30；`apps/chaos-ui/src/session.ts`、`session.test.ts`、Playwright desktop/mobile 24/24；完整 Rust workspace 31,169 passed/0 failed/490 ignored（使用 CI 要求的 `RUST_MIN_STACK=16777216`；默认测试线程栈会触发既有 `xai-grok-shell` actor test stack overflow）。原始日志未随仓库留存）
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
- [~] WebSocket 工具审批集成覆盖允许、拒绝、重复 resolve、缺 adapter fail-closed、恢复及双客户端竞争；Playwright desktop/390×844 覆盖 demo protocol 拒绝、无 adapter 失败和提问响应。`tool_started`/`tool_progress`/`tool_result` 现在投影为有界活动卡片（最多 20 条，截断 progress/result 文本），浏览器测试经运行中的应用 WebSocket 入口驱动一整组工具活动事件。生产工具授权、审批超时/记住规则仍待补。真实浏览器回归在同一个 browser context 打开两个标签：首标签经 UI 发起待审批操作，第二标签的原生 WebSocket 通过同源 Vite `/ws` 代理连接同一 Web host/Engine；首标签先通过 UI 允许请求并观察到无 adapter 的 fail-closed 结果后，第二标签再对原 request ID 提交晚到的竞争批准，得到 `approval_not_found`。测试断言 UI 仍保留失败结果，证实共享 Engine 拒绝重复/过期审批；它没有模拟同时到达时的胜出顺序。（2026-09-30；`apps/chaos-ui/e2e/approval-competition.pw.ts`；not-retained log `verification/approval-competition-playwright.log`）（2026-09-30；`apps/chaos-ui/e2e/approval-competition.pw.ts`）（2026-09-29；`approval_resume_flow.rs`、`question_resume_flow.rs`、`approval_competition_flow.rs`、`apps/chaos-ui/e2e/approval-competition.pw.ts`、`apps/chaos-ui/e2e/tool-activity.pw.ts`）
- [~] Web 写操作已有 workspace root confinement、message dedup、Origin/Host/token 校验和 Safe Web Mode 后端 gate；CSRF/重放跨 HTTP 写操作防护、审计日志脱敏轮转仍待完整部署模式。
- [x] Safe Web Mode 已由 WebSocket 后端强制执行：命令、文件写入、Git/Diff mutation、审批执行等 mutation message 在 `CHAOS_SAFE_WEB_MODE` 下直接返回 `safe_web_mode_blocked`；真实 WebSocket 测试覆盖 terminal 和 destructive Git direct call。（2026-09-24；`safe_mode_flow.rs`）

### M1.4 Diff 闭环

- [~] 建立 `chaos-engine::DiffAdapter` 边界，提供 session-scoped preview/accept/rollback/error 事件；WebSocket 已通过真实 workspace fixture 覆盖文件写入拒绝/批准、Diff 预览、接受和回滚，仍待接入 `xai-grok-pager-diff`、`xai-hunk-tracker` 与 workspace RPC 的生产级部分 hunk/二进制实现。（2026-09-24；`workspace_diff_flow.rs`）
- [x] 明确“工具已写盘”与“接受/拒绝 Diff”的真实语义：engine 只在 DiffAdapter 成功后发 `diff_resolved`，无 adapter 或失败发 `diff_failed`，不更新磁盘假象。（2026-09-24）
- [~] 已覆盖 adapter 成功、缺 adapter 和 session 绑定；外部文件修改、部分 hunk、回滚失败和二进制文件仍待真实 workspace adapter。

### M1.5 验收门禁

- [~] E2E：engine/WebSocket 已覆盖读取 workspace 文件 → 请求写入 → 拒绝一次 → 再次批准 → 预览 Diff → 接受 → 回滚并确认磁盘状态；浏览器/桌面人工验收和真实 hunk/二进制场景仍待平台 gate。（2026-09-24；`workspace_diff_flow.rs`）
- [~] WebSocket integration 已在 socket 断开后通过持久化 Engine reopen，恢复待审批 approval/question prompt，并验证批准/回答只完成一次；Playwright desktop/mobile 覆盖真实 Web 页面上的单客户端拒绝与允许后无 adapter 失败、问题回答；真实浏览器同一 Engine 的多标签晚到审批 resolve 已由 `apps/chaos-ui/e2e/approval-competition.pw.ts` 验证，仍未覆盖同时到达时的胜出顺序及完整通知时序。（2026-09-30；`approval_resume_flow.rs`、`question_resume_flow.rs`、`apps/chaos-ui/e2e/workspace-flow.pw.ts`、`apps/chaos-ui/e2e/approval-competition.pw.ts`；not-retained log `verification/approval-competition-playwright.log`）
- [~] WebSocket 双客户端 fixture 已验证同一审批只接受一个终态，第二次 resolve 返回 `approval_not_found`；真实浏览器同一 Engine 多标签晚到竞争已有 `apps/chaos-ui/e2e/approval-competition.pw.ts` 覆盖：首标签允许后，第二标签对同一 request ID 的晚到 resolve 被真实 WebSocket handler 以 `approval_not_found` 拒绝，首标签 UI 保留 fail-closed 结果。该测试不主张模拟并发先后顺序；单页面 allow/reject path 亦由 desktop/mobile Playwright 覆盖。（2026-09-30；not-retained log `verification/approval-competition-playwright.log`）（2026-09-25；`approval_competition_flow.rs`、`apps/chaos-ui/e2e/workspace-flow.pw.ts`）
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
- [~] 当前普通文本写入限制 1 MiB 并执行 root confinement；浏览器提供提议写入→展示审批→允许/拒绝→由 Engine `file_changed` 触发当前目录刷新→重新读取的闭环，真实 desktop/390×844 测试确认批准后磁盘内容更新、拒绝后内容保持原样。attachment filename/content-type/10 MiB allowlist 由 shared `AttachmentStager::validate_name_type_size` 同时校验 `ValidateAttachment` 与 `BeginAttachment`，拒绝 `/` 与 `\\` 分隔符并覆盖批准 finalize。真实 Engine protocol tests 验证无效 Base64 chunk 返回 `attachment_chunk_invalid` 后同一 upload 可接收有效 chunk 并报告进度，以及超额 chunk 返回 `attachment_quota_exceeded` 后 upload 被移除、后续 chunk 返回 `attachment_not_found`（`attachment_protocol_keeps_upload_available_after_invalid_base64_chunk`、`attachment_protocol_rejects_over_quota_and_unknown_upload`）。`WorkspaceAdapter` 和 `AttachmentStager` 启动时以 `symlink_metadata` 要求 `.chaos-staging` 是根目录内的真实目录；真实 Unix symlink 指向外部目录时两种 adapter 构造均 fail-closed，且外部目标保持空白（`workspace_and_attachment_stagers_reject_staging_symlink_escape`）；正常 staging 隐藏与附件 WebSocket finalize flow 仍通过。附件 `.chaos-staging` 分块上传、失败清理、Web/engine Begin/Chunk/Progress/Cancel/quota 协议测试，以及 `FinalizeAttachment` 审批门控和 staging-to-workspace 原子落盘均已交付；WebSocket 覆盖拒绝/批准与磁盘状态。Engine 在实际 Begin→Chunk→Finalize→Approve 流程中拒绝将附件目标设为 root 或 nested `.chaos-staging` 内路径，拒绝后同一上传仍可重试到普通 workspace 文件；取消 upload 后 finalize 返回 not-found。chunk 到达期间，真实 WebSocket flow 还直接尝试读取、枚举和搜索 staging 路径，确认暂存字节不能被 workspace API 看到；批准的正常 finalize 仍成功。断线续传和配额跨会话策略仍待补。（2026-09-28；`crates/codegen/chaos-engine/src/lib.rs`、`crates/codegen/chaos-engine/tests/attachment_protocol.rs`、`crates/codegen/xai-grok-web/tests/attachment_flow.rs`、`workspace_flow.rs`。断线续传与配额跨会话策略仍未交付；浏览器侧客户端是同一天另开的切片，见下一行。（2026-09-28；`crates/codegen/chaos-engine/src/lib.rs`、`crates/codegen/chaos-engine/tests/attachment_protocol.rs`、`crates/codegen/xai-grok-web/tests/attachment_flow.rs`、`workspace_flow.rs`）
- [x] 浏览器第一次真的能把文件作为附件上传进 workspace：`apps/chaos-ui/src/attachments.ts` 是客户端（分片计划、base64、五条消息构造、终止性错误码集合、中文状态文案），文件页新增「附件上传」面板，`session.ts` 用纯函数把 `attachment_validated/_started/_progress/_cancelled/_completed` 投影成 `Upload`。**真正卡人的是帧上限而不是文件大小**：Web 主机每帧上限 `MAX_REQUEST_BYTES = 64 * 1024`，base64 又把载荷放大 4/3，于是 `UPLOAD_CHUNK_BYTES = 47 * 1024` 是算出来的（47 KiB → 64,172 个 base64 字符 + 144 字节 JSON 外框 = 64,316 < 65,536；48 KiB → 65,680 即超），并被测试从**两侧**夹住（写大失败、写小浪费带宽），常量本身由测试回读 `xai-grok-web/src/lib.rs` 与 `AttachmentStager::validate_name_type_size` 核对，避免客户端以为的上限与主机执行的上限各自漂移。主机侧新增 `websocket_upload_lands_file_bytes_in_the_workspace_after_approval`（先发一片**故意**超过 64 KiB 的帧断言 `message_too_large`，再按 47 KiB 切三片让累计 `received` 等于 100,000，审批后 `fs::read` 逐字节比对并断言 `.chaos-staging` 无残留——帧上限第一次被真实触发，且证明主机拒的是那一帧而非整段上传）与 `safe_web_mode_refuses_every_step_of_an_attachment_upload`（先证明安全模式下 `create_session` 仍成功，否则「被拒」只是因为压根没有会话，再逐一断言五个步骤各回 `safe_web_mode_blocked`）；同轮删掉 `safe_mode_allows` 里的 `ClientMessage::FinalizeAttachment`，它不是更宽的口子而是**永远走不到的死条目**（安全模式下 `begin_attachment` 已被拒，客户端拿不到 `upload_id`）。**真实浏览器验证**：desktop 1440×1000 与 mobile 390×844 各 3 例共 6 条全绿——`setInputFiles` 真的塞文件、状态行逐字等于 `附件已写入 nested/<name>.txt（122880 字节）`、审批点「允许」、磁盘逐字节比对，再换 workspace 内容搜索与编辑器这**第二条代码路径**复读一次；取消例必须等到「正在上传」才点取消（那三个字意味着主机已发回 `upload_id`），并在 300 ms 后复断文件仍不存在；`.exe` 例证明客户端不轻信自己的校验（本地不做扩展名白名单，主机的 `attachment_rejected` 必须被如实呈现）。e2e 顺带抓出一处**会吞掉错误的状态写法**：`ws.onmessage` 先推进 `sessionStateRef.current` 再 `setSession`，而上传用的是 React 函数式更新，那个 ref 要等下一次渲染后才跟上，于是主机最早的回复被应用在一个没有 `upload` 的旧状态上、又把排队的状态覆盖掉——用户看到的是徽标刷成一行通用错误、面板忘记这次上传。修法不是给上传打补丁而是消掉两份真相：新增 `updateSession()` 同时推进 ref 与 React 状态，20 处调用点全部改过去，并由 `session-wiring.test.ts` 钉住（读 `main.tsx` 源码断言不再有任何 `setSession((`；改回一处它当场红，还原后 `cmp` 字节一致）。**验证**：`npx vitest run` 56 passed、`npm run typecheck` 干净、真实 Playwright 全套 **39 passed / 53.2s**（含杀主机重启的 `reconnect-snapshot`、跨标签页审批的 `approval-competition`、两条 axe 无障碍用例）无一转红——plumbing 影响所有会话写入，故全套必须重跑；`cargo test -p xai-grok-web --test local_policy_flow --test safe_mode_flow` 3 + 2 全绿。非空洞性：把 `pumpUpload` 改成只发第一片，上传例当场红（字节数停在第一片的累计值）而另两例仍绿，还原后 `cmp` 一致。**边界**：断线续传与配额跨会话策略仍未交付（与上一行同一前置）；`.chaos-staging` 的分片只在审批通过那一刻才落盘（切片在内存里，approve 时 `File::create` + `sync_all` + `rename`），因此「取消/拒绝后 staging 留有半截文件」这一形状在当前实现里不存在，新测试断言的是审批通过后目录为空。（2026-10-03；`apps/chaos-ui/src/attachments.ts`、`src/attachments.test.ts`、`src/session.ts`、`src/session.test.ts`、`src/main.tsx`、`src/session-wiring.test.ts`、`e2e/attachment-upload.pw.ts`、`playwright.config.ts`、`crates/codegen/xai-grok-web/src/lib.rs`、`tests/local_policy_flow.rs`、`tests/safe_mode_flow.rs`、`docs/verification/web-attachment-upload-2026-10-03.log`） **提交前复跑抓到一条真偶发红，红在断言自身的形状**：取消例原本用 `toContainText('正在上传')` 等一个**瞬态**——4 KiB 正好一片，状态行在下一次轮询前已走到 `附件已传完，等待审批写入：…`，于是这条测试其实在测 loopback 有多快（实测 6 轮红 2 轮，两次都是 mobile 那一遍）。改为 `addInitScript` 挂 MutationObserver 记录状态行显示过的每个文本、断言用 `expect.poll` 轮询这份记录（与 `reconnect-snapshot` 同一招），取消点击仍排在等待之后，「取消的是真传输」的前提没有被削弱。把观察者监听的 testid 改错 → 两个视口都红在 `上传状态里从未出现过「正在上传」`；跑完写回 `cmp` 字节一致。记录形式 5/5 轮 30 条全绿，轮询形式同机 5 轮红 1 轮（样本只有几轮）。见 `docs/verification/web-attachment-upload-2026-10-03.log` 第 9 节。（2026-10-03；`apps/chaos-ui/e2e/attachment-upload.pw.ts`、`CHANGELOG.md`）
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
- [~] engine `AttachmentStager` 已覆盖分块写入、10 MiB/类型/路径策略和失败清理；真实 WebSocket 上传、取消/进度与最终 staging-to-workspace 审批移动均有测试，上传后未批准不会写入 workspace。（2026-09-24；`attachment_flow.rs` **2026-10-03 补**：真实浏览器上传（分片→审批→落盘、取消、主机拒绝不认识的类型）已在 desktop 1440×1000 与 mobile 390×844 两个视口各跑一遍，且批准写入后换了第二条代码路径（workspace 内容搜索 + 编辑器）复读一次；本行仍保持 `[~]`，因为断线续传与配额跨会话策略未交付。（`docs/verification/web-attachment-upload-2026-10-03.log`）
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
- [~] 设置面板现按通用、外观、模型、Provider、权限、安全、快捷键、远程、更新九个区域渲染，除主题（存本机）与快捷键表（代码里的绑定表）之外，每一行的值都来自 host 的应答（`host_info` 或 `settings`），不是前端常量：版本、协议、实际 bind 地址、state 后端、工作区根、token 是否设置、预览代理、更新方式与 Safe Web Mode 拦截清单均由应答浏览器的那个进程给出。`xai-grok-web/tests/host_info_flow.rs::host_info_reports_the_process_that_answered` 证明 host 谎报的 `state_backend`/`workspace_root` 与被清空的拦截清单会被引擎真实值覆写，`the_refusal_list_matches_what_the_socket_actually_refuses` 逐条对照清单与真实 safe-mode socket 的拒绝结果，`the_serving_process_reports_its_own_socket_and_version` 驱动 `chaos-web` 实际调用的 serve 函数核对端口与版本。可编辑项仍只有主题（写本机 localStorage，桌面/390×844 浏览器验证跨 reload 生效）与 Base URL/model（保存后行值随 host 应答更新，非法 URL 仍由 engine 拒绝）；`API Key` 行只显示 host 侧是否配置、快捷键区域只读展示实际生效的绑定，凭据方案、完整配置 schema 表单与 provider 能力探测仍开放。（2026-10-03；`apps/chaos-ui/src/settings.ts`、`apps/chaos-ui/src/shortcuts.ts`、`apps/chaos-ui/e2e/settings-panel.pw.ts`、`crates/codegen/xai-grok-web/tests/host_info_flow.rs`、`docs/verification/settings-panel-2026-10-03.log`）
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

- [~] M3 Provider E2E 的「端点」半边已用**自建**推理端点真实打通：`crates/codegen/xai-grok-web/tests/provider_http_flow.rs`（13 条）走生产 `router()` + 真实 WebSocket + `Engine` + `HttpPromptAdapter`，对端是真实 HTTP/SSE 服务；`tests/host_startup.rs`（4 条）直接驱动 `CARGO_BIN_EXE_chaos-web` 二进制验证环境变量、启动健康检查与非法配置退出。连通性、流式字节序、`Authorization` 头、401/429/5xx/断连/坏帧/空响应均已实测，不再是 `network_not_attempted`。仍不关闭的部分：API Key 目前只由 host 环境变量提供，OS keyring/加密存储与 settings 持久化仍待 M-1 选型决策，因此「secret persistence」与凭据轮换/撤销页面恢复没有实测；GUI 里也没有 provider 配置表单。（2026-10-02；`cargo test -p xai-grok-web --test provider_http_flow --test host_startup`）
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

- [~] `chaos-engine::remote` capability/endpoint boundary 已完成并接到真实传输上：声明 workspace/tool 能力、要求 Strict 或有 fingerprint 的 TOFU host-key policy、拒绝 detached Agent，并由 `chaos-remote-server`/`chaos-remote` 两个 CLI 在 Unix socket 与 loopback TCP 上实际跑通（ADR-004 的 tunnel-carries-trust 拓扑）。port-forward 也已从声明变成实现：`--allow-forward-to` 白名单 + forward ticket + `chaos-remote forward`。仍未交付：SSH transport（见 M-1/M4 SSH spike）与 remote forwarding（`ssh -R` 方向，保留名字但拒绝并指向 port-forward）。（2026-10-02；`crates/codegen/chaos-engine/src/remote/`、`docs/verification/remote-acceptance-linux-2026-10-02.log`）
- [x] M4 topology/RPC contract 与 server ownership 已命名而非停留在候选：newline-delimited JSON envelope + `PROTOCOL_VERSION`/`MIN_PROTOCOL_VERSION` 协商（`remote/protocol.rs`），server 是 `chaos-remote-server`（一 workspace 一授权会话，非常驻 daemon），client 是 `chaos-remote`；owner 决定由项目 owner 在本轮授权下作出，用法与信任模型写进 `CONTRIBUTING.md`「Remote workspace sessions」。（2026-10-02；`CONTRIBUTING.md`、`crates/codegen/chaos-engine/src/bin/`）
- [~] 版本化 artifact 部署已实现并验证：sha256 校验、版本目录 + `current` 指针、原子 rename、失败回滚、`VERSION` 标记，且拒绝「本机根本不能执行」的 artifact（`noexec` 文件系统上的 0755 文件也过不去校验）。签名/来源证明已交付：本仓库**本来就有**签名基础设施（release 的 ed25519 `.sig` sidecar、`CHAOS_SIGNING_PUBLIC_KEY`、`docs/release-signing.md`），缺的一直是 remote 这条路——`install` 只比过 sha256，而那个摘要正是发送字节的人自己算的，所以泄露的凭据或被篡改的构建可以把自己的字节变成下一台启动的 server。现在验签发生在任何文件系统改动之前（digest → 签名 → 平台头 → 才 move），host 默认 fail-closed，唯一退出口是 `--allow-unsigned-artifact` / `CHAOS_REMOTE_REQUIRE_SIGNATURE=0`，拒绝时给出 `signature_missing`/`signature_invalid`/`signature_malformed`/`no_trusted_key`/`artifact_too_large`/`wrong_platform` 之一。OS/arch 探测也已就位：读 ELF/Mach-O/PE 文件头声明的 target，与本机不符即在发布指针前拒绝（无法分类的文件——脚本包装、未知架构——不拒，方向是故意的）。尚未做的：Linux 之外的远端主机仍无证据（platform CI 那两条腿跑的是单机测试，不是 remote deploy）。**2026-10-03 把「删掉那一行会红几条」重新测了一遍**，因为 10-02 的记录只写了「会红」却没写删的是哪一行，不可复现的证据不算证据：锚点是 `install.rs` 里单独一行 `self.provenance.check_file(staged, signature)?;` 的删除（长度下限、摘要比对、平台头读取、`rename`、发布后复读一概不动），锚点/替换/真正落盘的 diff 先写进日志再跑。动手之前先量的基线是 install 20 绿、client 30 绿、provenance 15 绿；删掉之后 install 红 4、client 红 2、**`remote::provenance` 照绿 15**——那 15 条直接调 `ProvenancePolicy::check`/`check_file`，测的是策略不是接线，删调用点不可能让它们红，因此「provenance 的测试通过」对「这台主机到底执行不执行」零信息量，本行的结论必须引 install/client 那六个名字而不是策略自己的测试。第一次尝试暴露的缺陷在测量装置上：`shutil.copy2` 按设计连同备份的 mtime 一起还原，而它比 cargo 用被变异源码产出的构建更旧，cargo 按 mtime 做指纹便跳过重编，所谓「未变异基线」量的其实是被变异的二进制；加之基线被放在实验之后，等于把同一次变异测两遍再管第二遍叫对照组。现在基线先测、不绿即退出 4 拒绝继续（该分支被真实踩过），每次内容改动后刷新 mtime，还原后再跑一遍三组过滤器仍是 20/30/15 全绿、`cmp` 字节一致。同轮 `scripts/install-sh-in-docker.sh` 在容器存活上限改为 `CHAOS_LAB_KEEPALIVE` 后重跑 19/19，摘要与 `release-signature-v0.4.2-2026-10-02.log` 逐项对上，且 `cmp` 证明 `1487aded` 与 `ef33dfd9` 的 `scripts/install.sh` 无差异。全过程（含手工复现序列）见 `docs/verification/remote-provenance-linux-2026-10-02.log` 与 `docs/verification/install-sh-linux-2026-10-02.log` 各自的 2026-10-03 节。（2026-10-02；`crates/codegen/chaos-engine/src/remote/{install,provenance,artifact_format}.rs`、`crates/codegen/xai-grok-signature/`、`scripts/remote-acceptance-in-docker.sh`、`docs/verification/remote-provenance-linux-2026-10-02.log`）
- [x] client/server version negotiation 已实现并在真实会话上验证：`ping`/`info` 报告协商后的协议版本与 server build，版本区间不兼容会被拒绝而不是静默降级；`--capability` 取交集，未授予的能力在 `info` 里可见。（2026-10-02；`crates/codegen/chaos-engine/tests/remote_workspace.rs`、`docs/verification/remote-acceptance-linux-2026-10-02.log`）
- [~] Heartbeat, bounded retry/backoff and explicit connection states are implemented and observed on the deployed binaries. The heartbeat is `ping` with the deadline it previously lacked: `--reply-timeout` bounds the handshake and every request, and a reply that does not arrive ends the session (`SessionState::Abandoned`) rather than leaving open a stream whose next reply would answer the wrong question. Retry is bounded and dial-only: `--connect-wait` re-dials with the delay doubling to a 2s cap until its patience runs out, because the credential is one-time and the connect is the only step that is safe to repeat. A run that dies still retires the credential it sent, so it cannot leave the token file holding a spent one. What is still not here is automatic reconnect, and that is a decision rather than a patch: reconnecting means getting a new credential, which needs either an operator or a credential allowed to open more than one session. The lab produces all three failure shapes deterministically — a reply that never comes, a tunnel that appears late, a peer that accepts and never speaks.（2026-10-02；`crates/codegen/chaos-engine/src/remote/client.rs`、`crates/codegen/chaos-engine/tests/remote_workspace.rs`、`docs/verification/remote-acceptance-linux-2026-10-02.log`、`docs/architecture/adr-004-remote-topology.md`）

### M4.2 SSH 安全

- [ ] 支持 ADR/spike 已验证的认证方式；未验证的 ProxyCommand 等能力不得宣传。
- [~] remote endpoint 类型已强制 Strict/带 fingerprint 的 TOFU policy，缺 fingerprint 或 detached Agent 会被拒绝。真实 SSH host-key 交换与变更阻断仍然无证据——本 build 根本没有 SSH transport，Docker 验收 lab 也把这一点明确列为未覆盖（信任由 tunnel 承担），而不是假装由 loopback 测试替代。（2026-10-02；`crates/codegen/chaos-engine/src/remote/endpoint.rs`、`scripts/remote-acceptance-in-docker.sh`）
- [~] 凭据不落日志已被证明：lab 断言 server 日志不含 64 位 hex 凭据、不含 authorization/bearer 字样，且会话 transcript 本身也不含凭据形状的字串；session credential 是一次性且过期即失效。尚未做的：密码/私钥/Agent forwarding 类材料在本 build 中不存在，因此「永不进入日志」对它们仍是空话，需等 SSH auth 模式选定后另测。（2026-10-02；`scripts/remote-acceptance-in-docker.sh`）
- [x] Real remote-server bind/one-time-handshake gate 已在 Linux 受控环境执行：Unix socket 与 loopback TCP 可通，`0.0.0.0` 等可路由地址在 server 与 client 两侧解析处即被拒绝，握手重放被拒（并报为 one-time），未过期凭据可用、过期凭据报为 expired 而不是重放，`--token-ttl` 生效。（2026-10-02；`crates/codegen/chaos-engine/tests/remote_workspace.rs`、`docs/verification/remote-acceptance-linux-2026-10-02.log`）
- [~] Remote threat test 已在部署的 test server 上执行：目录穿越/绝对路径五种写法均被拒而非被解析，`--no-write` 与 `--allow` 白名单限制写入与可执行程序，日志无密钥，磁盘写满与 `noexec` 两种中断升级都被拒且指针回滚到位、不留半成品。仍未做：macOS/Windows 的文件系统与进程控制（lab 只有 Linux），以及 server 不写 pidfile（无 pidfile 可泄露，但也未验证过其它持久化文件）。（2026-10-02；`scripts/remote-acceptance-in-docker.sh`）

### M4.3 远程能力

- [x] 文件树、Range 读取、搜索、写入、Git Diff、工具执行与本地端口转发已在同一套远程能力里交付：`list`/`cat`/`read --offset --length`/`search`/`write --expect <sha256>`/`diff`/`exec --` 全部经由一条已协商会话，读取摘要与远端磁盘实际字节比对一致，`diff` 由远端主机自己的 git 产生，`exec` 传播远端退出码与超时（124）；`forward --to` 把远端服务映射到开发机的 loopback 端口（见 M4.4）。（2026-10-02；`docs/verification/remote-acceptance-linux-2026-10-02.log`）
- [~] 远程路径在本机上不可被误用：server 只接受 workspace 内相对路径，绝对路径与 `..` 穿越一律拒绝，`.git` 默认不出现在列举里；client 端不产生任何本机路径解释。UI 侧的深链接/本机文件 API 接线尚不存在，因此「不会被本机 API 误解释」目前只是传输层保证，还没有界面层回归测试。（2026-10-02；`crates/codegen/chaos-engine/src/remote/path.rs`）
- [~] 上传不做断点续传，且这是明确选择：artifact 先写到安装目录内的 `.staging` 隐藏文件，digest 不符或写入失败即丢弃暂存，已安装版本与指针保持原样；因此重传从头开始而不是续传。远端清理策略 = 失败的 staging 文件被丢弃 + 失败版本不发布指针（旧版本目录刻意保留以便回滚与查看）。（2026-10-02；`crates/codegen/chaos-engine/src/remote/install.rs`）
- [~] 交互式 PTY 与 detached Agent 不交付，并且是被点名拒绝而不是静默失败：`--capability interactive-pty|detached-agent` 在 client 侧即被拒并给出该能力为何不可用的具体理由，`info` 只报告真实授予的能力。`port-forward` 已从这份名单移出——它现在是真实能力，`--capability port-forward` 会被接受，lab 也照旧断言那两个未实现项被拒。文档已写明（`CONTRIBUTING.md`）；UI 尚未暴露任何远程入口，因此「UI 明确标记不支持」这一半还无从谈起。（2026-10-02；`crates/codegen/chaos-engine/src/remote/endpoint.rs`、`scripts/remote-acceptance-in-docker.sh`）
- [ ] 若 ADR 选择本地 Agent + 远程工具，明确本地休眠会暂停 Agent，不承诺 detached Agent。
- [ ] 若 ADR 选择远程 Agent，补齐远端 engine、凭据、会话存储、审批和恢复测试后才能开启该功能。

### M4.4 开发端口转发

- [x] “远端服务映射到本地端口”已按 local forwarding 建模（`ssh -L` 语义）：`chaos-remote forward --to HOST:PORT` 在执行端开 loopback 监听口，每来一条连接就重新拨 server、以 forward ticket 握手后双向搬运裸字节；目标由 server 侧 `--allow-forward-to host:port` 白名单决定（默认为空＝一律拒绝），ticket 与 session 凭据分属两个 vault、签发即绑死目标、只授予 `port-forward` 一项能力。真正的 remote forwarding（`ssh -R`，由 server 开放监听口回连客户机）另立名字 `remote-forward` 并被点名拒绝、指向 `port-forward`，不会因为配置写反方向而静默按另一方向生效。语义与理由见 ADR-004「Local port forwarding」。（2026-10-02；`crates/codegen/chaos-engine/src/remote/forward.rs`、`docs/architecture/adr-004-remote-topology.md`）
- [x] 端口冲突检测、随机本地端口、隧道生命周期和 WebSocket 转发四项均可用且被测：`--listen 0`（默认）向系统要空闲端口并在任何连接进来之前把实际端口打印到 stderr；端口被占用是点名拒绝（提示改传 0）而不是静默换端口；生命周期由授予额度与 TTL 决定——用完/到期监听口自行关闭，关闭会话即撤回 ticket，因此不会留下还在转发的端口；转发的是握手之后的裸字节，`tokio::io::copy_bidirectional` 双向到两端 EOF 为止，测试里真实 WebSocket 握手与 HTTP 请求都穿过隧道。（2026-10-02；`crates/codegen/chaos-engine/src/remote/forward.rs` 16 项测试、`scripts/remote-acceptance-in-docker.sh`）
- [~] Web 场景不得错误使用浏览器机器的 `localhost`：`webSocketUrl()` 只从页面自身的 `location` 取 protocol/hostname/port/base path（`hostname` 为空时才回落 `127.0.0.1`），Vitest 覆盖 https+非默认端口+子路径、HTTP 默认端口与 IPv6 三种形态，Docker TLS lab 再以真实 `wss://chaos.test:8443/ws` 会话验证同一 URL 形状可通。「区分桌面本机浏览器与远程部署浏览器」的另一半仍无对象可区分：本 build 里没有桌面 shell（M5 的 macOS/Windows 打包行仍未完成），因此保持部分完成。（2026-10-02；`apps/chaos-ui/src/transport.ts`、`src/transport.test.ts`、`docs/verification/web-deployment-tls-linux-2026-10-02.log`）
- [x] 预览代理已实现并处理 Host、Origin、Cookie、WebSocket 与鉴权，默认不公开暴露：`CHAOS_WEB_PREVIEW_PORTS=3000,5173` 把每个端口挂在 `/preview/<port>/` 下，上游恒为 `127.0.0.1:<port>`，端口必须在白名单里（白名单默认为空＝整条路径不可用）。`Host`/`Origin` 一律改写为 app 自己的 authority（测试里的 stand-in 像 Vite 一样拒绝不属于自己的 `Host`/`Origin`，重写退化为透传时测试看到的是 app 返回的 403 而不是代理的 200），`Set-Cookie` 的 `Path` 收窄到该前缀并丢掉 `Domain`（被预览的应用因此无法给 `/api/*` 种 cookie），`Location` 补回前缀。WebSocket 不是字节隧道而是两条握手：用浏览器自己的 key 完成面向浏览器的一跳，另开一条 `Host`/`Origin` 已重写的握手给 dev server，每跳的 `Sec-WebSocket-Accept` 各自正确，子协议只回显浏览器确实 offer 过的。鉴权是被明确决定并写进 ADR-005 的：这条路径前后都不带宿主 bearer token（本机上的 app 对「认证本服务器的凭据」没有索取权，而浏览器对页面子资源根本不附 `Authorization`，要求它只会拒掉页面），代价是 `/preview/<port>/` 与身后的 dev server 一样不设认证，因此默认只从 loopback 可达（拒绝原因码 `preview_loopback_only`，伪造 `Host: 127.0.0.1.evil.example` 不算 loopback），即使声明了 `CHAOS_WEB_PUBLIC_ORIGIN` 也还要 `CHAOS_WEB_PREVIEW_ALLOW_PUBLIC=1` 才允许走公开名字——公开这台 host 与公开某个项目的 dev server 是两个决定；从别的机器访问的正路仍是 `chaos-remote forward`。被写明而非隐瞒的限制：前缀代理无法重写应用 HTML 里写死的 `/main.js`，需要应用侧配 `base`/`publicPath`，`X-Forwarded-Prefix` 每个请求都附上供服务端渲染的 app 使用。（2026-10-02；`crates/codegen/xai-grok-web/src/preview.rs` 15 项单测、`crates/codegen/xai-grok-web/tests/preview_proxy.rs` 14 项集成测试（驱动真实 `chaos-web` 二进制 + axum stand-in + 逐字节读握手并用实收 key 推 accept 的 raw socket）、`docs/architecture/adr-005-web-security.md`「Preview proxy」、`docs/verification/preview-proxy-linux-2026-10-02.log`——含 curl 实跑 transcript 与 13 个变异的结果：5 处被抓、8 处为等价变异且原因已查明并钉成测试）

### M4.5 WSL 与容器

- [ ] 先以 capability/transport adapter 形式完成 WSL spike；通过后再加入受支持矩阵。
- [ ] Docker/Podman 保持 Deferred：本仓库确实开始使用容器，但用途是**开发/CI 验证**（`docker/verify.Dockerfile` + `scripts/verify-in-docker.sh`，在干净 Debian bookworm 里跑 CI `rust` job 的同一命令序列），没有任何产品代码运行在容器里，因此不构成把容器列入支持矩阵的理由。开启该功能仍需三件事：作为运行环境的独立 owner、容器逃逸/挂载逃逸威胁模型、以及验收环境；前两项无人认领，因此保持 Deferred。（2026-10-02；`CONTRIBUTING.md`「Clean-container verification」）

### M4.6 验收门禁

- [x] 在干净 Linux 远端完成部署、版本协商、文件读取、搜索、修改、Git Diff、工具执行与本地端口转发：`scripts/remote-acceptance-in-docker.sh` 把 server 放进一个从未见过本仓库的 stock Debian bookworm 容器（只拷入两个二进制），经 `socat` tunnel 由另一个容器充当开发机，102 项检查全绿。完整 transcript：`docs/verification/remote-acceptance-linux-2026-10-02.log`。（2026-10-02）
- [~] 错误凭据、网络中断、服务器升级失败与磁盘满已验证：重放/伪造凭据被拒，过期凭据报 expired（早期实现会误报为重放，已修并有回归测试）；`socat` 被杀即断网，进行中的 `exec sleep 90` 在 2s 内失败而非挂死，tunnel 恢复后新会话可用；磁盘写满与 `noexec` 文件系统两种升级失败都被拒且指针不回坏。未验证：错误 host key——本 build 无 SSH transport，该条件没有发生路径，lab 也如此声明。（2026-10-02；`docs/verification/remote-acceptance-linux-2026-10-02.log`）
- [~] 两个远端工作区状态不串位已验证：第二个 workspace 由另一台 loopback server（`--no-write`）提供，它的会话只列举得到自己的文件、读不到第一个 workspace，第一个会话也照旧读得到自己的；且一台 server 发布的凭据在另一台那里打不开任何会话。路径不会被本机 API 误解释目前只有传输层保证（见 M4.3），等 UI 接上远程入口后需要界面层回归。（2026-10-02；`scripts/remote-acceptance-in-docker.sh`）
- [x] 启动远端 HTTP/WebSocket 测试服务，通过本地转发访问；授权用完端口即释放：lab 在远端主机容器里起 `python3 -m http.server --bind 127.0.0.1` 与一个按 RFC 6455 手写的 WebSocket 回显服务，从另一侧容器 `chaos-remote forward --to ... --listen 127.0.0.1:<port>` 之后取回文件，其 sha256 与远端磁盘上的文件一致（该本地端口整轮都没有别的东西监听过，因此不存在第二种解释）；目标自己产生的 404 原样回传，证明请求确实抵达目标；同一授权上的第二条连接照常可用；`--connections 2` 的授权用完后进程自己退出、监听口消失，同一端口随即可再次绑定并再跑一次。WebSocket 那一半不是近似：回显服务每条回复带的是只存在于远端主机上的文件内容，lab 先断言开发机容器 `grep` 不到该字符串，再断言经隧道握手成功并读回它——因此看到的是穿过隧道的真实 WebSocket 会话，而不是对它的假设（库层另有 tokio-tungstenite 两端真实握手的测试）。（2026-10-02；`scripts/remote-acceptance-in-docker.sh`、`docs/verification/remote-acceptance-linux-2026-10-02.log`）
- [~] 未交付的 PTY 与 detached Agent 在 capability 协商、CLI 拒绝信息与 `CONTRIBUTING.md` 文档三处均明确不支持；UI 尚无远程功能面，故 UI 侧标记待该界面存在时补。（2026-10-02；`CONTRIBUTING.md`「Remote workspace sessions」）

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
- [x] 签名证书、secret 名称、轮换、权限和失效处理形成 runbook：`docs/release-signing.md` 逐项写清六个被签产物与 `.sig` 裸 base64 ed25519 格式、私钥所在 Actions secret 与公钥所在 repository variable 的确切名称和读取方、跨 cryptography 版本可用的密钥生成片段与 `gh secret/variable set` 命令、BOM 导致 release run 31796456487 失败的成因、谁能签（`actions: write` + workflow 本体才是真实信任边界）、以及轮换的真实后果——`signature::public_key()` 是 `option_env!` 编译期常量，一个二进制只认它构建时的那一把钥匙，无 key id、无并行窗口、无吊销，因此跨越轮换边界的 `chaos update` 必然拒签，用户只能经安装脚本或 npm（公钥来自仓库而非自身构建）迁移；失效处理一节明确私钥泄露时无法吊销已装二进制，只能停用旧钥发布并靠安装器搬运，该缺口本身仍留在下方 feed/revocation 行。另记录未被签名覆盖的两处：`SHA256SUMS` 与产物同源故只防损坏不防改写，npm 平台包内嵌 brotli 二进制、首装信任锚是 registry 而非 sidecar。runbook 里的验证命令均已实测（`scripts/verify-release-signature.sh --tag v0.4.2 --all` 16 项、`verify_release_artifact` 的 0/1/2 退出码）。真实轮换尚未发生过（仓库至今只有一把密钥），文档描述的是一次都没有演练过的操作序列。（2026-10-02；`docs/release-signing.md`、`docs/verification/release-signature-v0.4.2-2026-10-02.log`）

### M5.3 Web/CLI 与更新

- [~] `xai-grok-web` 现可由显式 `CHAOS_WEB_ASSETS_DIR` 启用静态目录托管：`/assets/*` 提供构建资源，其他未知 GET 路由使用 `index.html` 支持 React SPA fallback，已声明的 `/health`、`/api/handshake`、`/api/sessions` 与 `/ws` 优先保留原 handler；MIME、缺失资源 404、SPA 路由回退及 health/API non-shadowing 有 Axum router tests；静态资源还支持同名 `.gz`/`.br` 变体协商与 `Vary: Accept-Encoding`、缓存重新验证；hashed bundle 和 `index.html` 的策略已由 router/browser tests 验证。独立 Playwright 测试启动真实 Web binary 加本次 Vite dist，验证页面渲染、JS asset bytes/type、SPA path、missing asset 404、健康和受保护握手，以及浏览器 WS 握手→session create。静态资源现在可通过同名 `.gz`/`.br` 预压缩产物协商返回，带 `Vary: Accept-Encoding`；hashed `/assets/*` 的 Cache-Control 要求每次重新验证，SPA `index.html` 为 `no-cache`，避免部署后旧入口引用过期 bundle。router unit 与 desktop/mobile Playwright 均验证压缩响应/内容协商和缓存头。identity 静态文件现按实际资源字节计算 SHA-256 ETag，支持 `If-None-Match`/304；真实 Axum handler regression 覆盖匹配 ETag 返回空 304、文件内容变化后旧 ETag 返回新 200/body/new ETag。SPA index 仍为 no-cache（无 ETag），并加 `Vary: Accept-Encoding`。middleware按协商选定的预压缩文件（若有）或identity文件字节生成 SHA-256 ETag，并以 `.gz`/`.br` 后缀隔离编码表示；真实静态部署的 gzip/Brotli 响应均已在桌面/移动浏览器验证正文可由浏览器透明解码、带对应磁盘变体 SHA-256 ETag，且匹配 `If-None-Match` 返回 304；质量值、大小写与标准 `x-gzip` 兼容别名由真实 router unit test 覆盖；`x-gzip` 在真实 built-host 浏览器路径验证与 gzip 使用同一压缩文件及 ETag。ETag 条件逻辑限制于 GET/HEAD；POST 即使带匹配 validator 也由 ServeDir 返回方法拒绝（router test 覆盖 HEAD 200/ETag 与 POST 405）。CDN cache invalidation 和正式 release pipeline 接线仍待 M5；这些本地 HTTP 语义不替代 CDN/发行策略验收。（2026-09-30；`crates/codegen/xai-grok-web/src/lib.rs`、`apps/chaos-ui/e2e/static-host.pw.ts`；not-retained log `verification/static-host-cache-test.log`、`static-host-cache-clippy.log`、`static-host-cache-build.log`、`static-host-cache-ui-build.log`、`static-host-cache-vitest.log`、`static-host-cache-typecheck.log`、`static-host-cache-web-suite.log`、`static-host-cache-playwright.log`、`verification/static-host-etag-router.log`、`static-host-etag-clippy.log`、`verification/etag-and-spa-assets-test.log`、`etag-and-spa-assets-clippy.log`、`verification/etag-final-check.log`、`etag-final-web-tests.log`、`etag-final-clippy.log`、`etag-final-typecheck.log`、`etag-final-vitest.log`、`etag-final-ui-build.log`、`etag-final-playwright.log`、`final-audit/etag-final-fmt.log`、`final-audit/etag-final-diff.log`、`verification/compression-web-check.log`、`compression-web-suite.log`、`compression-web-clippy.log`、`compression-ui-typecheck.log`、`compression-ui-vitest.log`、`compression-ui-build.log`、`compression-browser.log`、`final-audit/compression-fmt.log`、`final-audit/compression-diff.log`、`final-audit/compression-protocol.log`、`compression-brand.log`、`compression-secrets.log`、`compression-ignored.log`、`compression-l10n.log`、`verification/final-compression-check.log`、`final-compression-web-suite.log`、`final-compression-clippy.log`、`final-compression-typecheck.log`、`final-compression-vitest.log`、`final-compression-ui-build.log`、`final-compression-playwright.log`、`final-audit/final-compression-fmt.log`、`final-compression-diff.log`、`final-compression-protocol.log`、`final-compression-brand.log`、`final-compression-secrets.log`、`final-compression-ignored.log`、`final-compression-l10n.log`、`verification/x-gzip-final-check.log`、`x-gzip-final-tests.log`、`x-gzip-final-clippy.log`、`x-gzip-final-typecheck.log`、`x-gzip-final-vitest.log`、`x-gzip-final-ui-build.log`、`x-gzip-final-browser.log`、`final-audit/x-gzip-final-fmt.log`、`x-gzip-final-diff.log`、`x-gzip-final-protocol.log`、`x-gzip-final-brand.log`、`x-gzip-final-secrets.log`、`x-gzip-final-ignored.log`、`x-gzip-final-l10n.log`）
- [~] sha256、签名、版本索引和可验证更新 feed 四项均已存在并各自有实测：sha256 由 `release.yml` 生成 `SHA256SUMS`，三个安装脚本在赋予执行权限之前比对，并已对真实 `v0.4.2` 产物在干净 Debian 容器里跑通（19 项全绿，`checksum OK 0ee7d6ee…`；但本轮才暴露出 `install.sh` 的 `verify_checksum()` **一直没有调用点**，即 `curl | bash` 此前根本不查摘要就安装，现已在 `verify_signature` 之前调用并由策略测试的结构性断言钉住，见 `docs/verification/install-sh-linux-2026-10-02.log`）；签名由 `signature.rs` 的 ed25519 `verify_file` 完成，release 每个产物配一个 `.sig`，`scripts/verify-release-signature.sh --tag v0.4.2 --all` 用仓库公钥把六个产物（含两个 Windows `.exe`）逐个接受、翻转一字节即拒收；版本索引即 `version.rs` 的 `/releases/latest`（stable）与 `/releases?per_page=10`（alpha 含预发布，取最高 semver），可用 `CHAOS_GH_API_BASE` 改指；feed 的强制性由 `CHAOS_REQUIRE_SIG` / `require-sig` feature 承担，release 构建固定启用该 feature，`crates/codegen/xai-grok-update/tests/test_update_feed_e2e.rs` 六个用例驱动真实 `run_update` 验证拒绝与回滚，`auto_update.rs` 另有 anti-downgrade 策略（`installer_allows_downgrade` / `needs_update`）阻止索引把指针指回旧版。残余且未变的限制：索引本身（tag 列表）只由 TLS 保护、未被签名，签名只绑定产物内容，因此被污染的索引能做的是「不提供更新」而不是装入任意字节；签名格式是裸 base64 ed25519 而非 minisign，无 key id 与 revocation 机制（该缺口连同「一个二进制只认构建时那把公钥」的轮换后果已写进 `docs/release-signing.md`）；`install.bat` 仍未被真实执行；`install.ps1` 自 `21f5a186` 起连解析都过不了（多余右花括号 → `The Try statement is missing its Catch or Finally block`），`4d3eb266` 修复并新增 `scripts/ci/check-powershell-syntax.py` 在 platform legs 上把关，同日 `scripts/install-integrity-powershell.sh` 更进一步——Linux 上用 pwsh 把它真正执行（见本节末）。同日补上负向半边：`scripts/install-integrity-in-docker.sh` 自建一份 release（产物 +
`SHA256SUMS` 行 + 对产物字节签名的 `.sig`），经 `install.sh` 已支持的 ghproxy 镜像路径端出，容器
`--network none`（开跑前断言 github.com 不可解析），35 项全绿：篡改一字节 → `checksum mismatch`；
把 `SHA256SUMS` 按篡改后的字节**重算** → 仍被 `signature verification FAILED` 拦下，这条正是「摘要
只防损坏、签名才防改写」的分工被实测而不是被声明；`.sig` 缺失、公钥合法但非我方、公钥空串
（零请求即拒）、manifest 无本资产行、空下载各成一项，且每项都同时断言 `bin/chaos` 未落地；两个
逃生开关的代价被量化（只跳摘要仍被签名拦住，两个都跳才真的装入）。该台第一次运行 33/34，
`html-sums` 拒了却没说原因，由此挖出并修掉 `download_github` 的真实缺陷：它只报最后一个候选源的
失败原因，镜像返回 200 却是一段 HTML 错误页时用户只会看到「HTTP 000 from 最后一个公共镜像」并
被 tip 引导去再换一个镜像；现在按序打印去重后的前四条原因。负向半边随后补上了 Windows 那
一侧：`scripts/install-integrity-powershell.sh` 用 pwsh 7.4.6 在 Linux 上真实执行
`install.ps1`，把自己 re-exec 进一个只有 loopback 的网络命名空间（开跑前同样断言 github.com
解析不出来；优先 `unshare -rn`，机器不放行非特权用户命名空间时改用 `sudo -n unshare -n`，
两条都没有就 exit 2 报明原因，`CHAOS_PS1_LAB_NS_ROUTE` 可钉住走哪条并在本机把 CI 那条路跑一遍），
面对的 release 与 shell 台**共用同一份夹具**（生成器/镜像/请求日志断言抽到
`scripts/ci/release-integrity-{fixture,serve,request-log}.py`，故意不给两个安装器各写一份近似
夹具），30 项全绿：一条正路（`checksum OK` + `signature OK` + 落到 `~/.chaos/bin/chaos.exe`
的字节与夹具摘要逐个对上 + 夹具是第一个候选且 origin/公共镜像一次都没被尝试 + 夹具只被取过
产物 / `SHA256SUMS` / `.sig`），七种拒绝各两问（为什么拒、拒后未落地），加公钥空串零请求即拒
与两个逃生开关的代价。写它的过程又挖出两处真实缺陷：`install.ps1` 一直有 `-MinBytes 1MB`
（产物不足 1 MiB 在哈希前即拒）而 `install.sh` 根本没有下限——被中途截断的响应体会被原样哈希
后把结果报给用户；`install.bat` 是第三种版本，它用 1 MiB 只决定要不要嗅探 HTML，短的非 HTML
响应体直接落到 `certutil` 上、以「摘要不匹配」示人。现在三者同为 1048576/1MB 并由 shell 台
第 35 项防漂移断言（同时读三个文件）钉住；`install.ps1` 的
`Download-GitHubFile` 与 `install.sh` 一样只报最后一个候选的原因，现已同样打印去重后的前四条
`why:`（把那段循环删掉重跑，恰好 3 项失败、其余 27 项照绿，即这些检查确实在测东西）。仍未覆盖
且已在脚本头部写明的三点：`[RuntimeInformation]::OSArchitecture` 选哪个资产名、Windows 是否真的
执行这些字节、注册表 `PATH` 写入（每次运行都带 `-NoPath`）；`install.bat` 依旧完全没有被执行过。两个实验台现已挂进 CI 的新任务 `installer integrity
labs`（ubuntu-latest，每次 push），运行期不需要网络/release/私钥，且两侧都没有跳过路径（缺
pwsh、缺 cryptography、内核不肯建网络命名空间都是 exit 2）。
（2026-10-02；`docs/verification/release-signature-v0.4.2-2026-10-02.log`、`docs/verification/updater-feed-e2e-2026-10-02.log`、`docs/verification/install-sh-linux-2026-10-02.log`、`docs/verification/install-integrity-linux-2026-10-02.log`、`docs/verification/install-integrity-powershell-linux-2026-10-02.log`、`crates/codegen/xai-grok-update/src/version.rs`）
- [~] 自动更新覆盖正常升级、签名失败、下载中断、回滚和旧数据迁移。`chaos update` 的真实入口 `auto_update::run_update` 现在由 `crates/codegen/xai-grok-update/tests/test_update_feed_e2e.rs` 针对自建 loopback release feed（wiremock 提供 API、`tests/common/artifact_server.rs` 提供资产字节）驱动：正常升级断言 `bin/chaos` 解析到 feed 字节、smoke 真的 exec 了新构建、`config.toml` 保留用户 `show_tips` 与 `[mcp_servers.*]` 且 `sessions/` 字节不变；`CHAOS_REQUIRE_SIG=1` 时不可验证 feed 在下载之后、exec 之前被拒绝且旧构建仍生效；截断 body、启动失败产物、feed 恢复后重试、以及安装后清理保留回滚版本并清除陈旧/遗留 `.tmp` 各有独立断言。该轮发现并修复了真实缺陷：crate 此前不校验下载字节数，`download_range` 也接受短读（`set_len` 预分配会留下静默零洞），现由 `check_complete_body` 与 range 字节数检查覆盖，两个检查均经删改源码的 mutation 验证为 load-bearing。同日补上签名 accept 半边（此前被误记为「无法覆盖」）：`scripts/verify-release-signature.sh --tag v0.4.2 --all` 用 shipped `signature::verify_file` 在仓库公钥下逐个接受六个真实产物并拒收翻字节产物，16 项全绿；同轮加了 `build.rs` 的 `rerun-if-env-changed` 修掉「改公钥不重新链接」隐患。残余：`run_update` + `CHAOS_REQUIRE_SIG=1` 指向一个真能验签的 feed 这一组合仍无法在测试内成立（测试二进制里的公钥只能拒签，见该文件 case 2 的限制）；Windows 资产名与 `.exe` 改名路径未覆盖（该文件为 `#![cfg(unix)]`）；`install.sh` 分支未由这六个用例驱动，改由 `scripts/install-sh-in-docker.sh` 在干净容器里对真实 release 单独驱动，最终 19 项全绿（见 M5.3 安装脚本文档与 `docs/verification/install-sh-linux-2026-10-02.log`）；那条驱动本身又挖出两个真实缺陷——「README 头条 `curl|bash` 因缺公钥必然失败」，以及 `install.sh` 的 `verify_checksum()` 没有调用点（摘要比对形同虚设），后者现在由结构性断言防止复发；本仓库已不再构建的旧版本磁盘格式未迁移测试。（2026-10-02；`crates/codegen/xai-grok-update/tests/test_update_feed_e2e.rs`、`src/auto_update.rs`、`docs/verification/updater-feed-e2e-2026-10-02.log`、`docs/verification/release-signature-v0.4.2-2026-10-02.log`）
- [x] 保持现有 CLI 发行路径兼容；桌面/Web 独立版本、CLI 发行链 lockstep 的边界已成文并被门禁强制。决定写在 `CONTRIBUTING.md`「Release versioning」：版本的唯一可信源是 npm meta 包 `crates/codegen/xai-grok-pager/npm/chaos/package.json`（`release.yml` 的 `resolve-version` 在该字段为空时读它），必须与它相等的 lockstep 集合是六个 `chaos-code-<platform>` 包版本、meta 里对应的六条 `optionalDependencies` pin、构建二进制的 `xai-grok-pager` + `xai-grok-pager-bin`，以及 `CHANGELOG.md` 的 `## <version>` 段落；`xai-grok-web`、`xai-grok-desktop`、`chaos-engine`、`xai-grok-update` 各自独立版本并在同节说明原因。`scripts/ci/check-version-lockstep.py`（已挂进 `ci.yml` 的 `workflows-present`）逐项把这些位置比回唯一可信源，并额外用 `--published`（需网络，非门禁）比对 npm 上真实存在的版本；漂移之所以危险，是因为更新器拿一个文件里的 `--version` 字符串去比另一个文件生成的 feed，标签公开后才会暴露。删改验证：把 meta 改成 0.4.3 触发 8 项失败（六个平台包、两个 crate、CHANGELOG），从 CONTRIBUTING 删掉 `xai-grok-web` 触发 1 项失败，还原后 7 项全绿；`--published` 正确报出两个 win32 包在 npm 上只有 `0.0.1-security` 占位（真实阻塞，见 MT 章），并显示已发布 latest 0.2.110 落后仓库 0.4.2。CLI 发行路径本身的兼容性（`chaos-code` 包名、`install.sh` 的资产名与 `~/.chaos/downloads/chaos-<ver>-linux-<arch>` 存储名、相对 symlink）由 `docs/verification/npm-install-linux-2026-10-02.log` 与 `docs/verification/install-sh-linux-2026-10-02.log` 在干净容器里对真实产物实测；macOS/Windows 两条安装路径仍未真实执行过。（2026-10-02；`scripts/ci/check-version-lockstep.py`、`CONTRIBUTING.md`、`.github/workflows/ci.yml`）

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
- [~] 前端 transport 已在 HTTPS 页面选择 `wss:`、HTTP 页面选择 `ws:`，并沿用页面自身的 hostname 与 port（不写回浏览器机器的 `localhost`）与 base path；`src/transport.test.ts` 覆盖 https+端口+子路径、默认端口与 IPv6 三种形态。后端侧的 TLS 终止/proxy headers/Token 轮换现已由 Docker 部署 lab 真实驱动验证（`scripts/web-deployment-in-docker.sh`）；审计日志留存与 CDN 行为仍待真实部署验收。（2026-10-02；`src/transport.test.ts`、`docs/verification/web-deployment-tls-linux-2026-10-02.log`）
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
- [~] 干净环境的官方 `npm install` 已实测跑通（Linux），Windows 那半边仍是外部阻塞。`scripts/npm-install-in-docker.sh` 在 stock `node:22-bookworm-slim` 容器（容器自身 registry 为 `registry.npmjs.org`，且断言 `/root/.npm/_cacache` 不存在以证明无预置缓存）里安装并发布物运行：`npm install -g chaos-code` 成功、落盘且仅落一个本平台包 `chaos-code-linux-x64`、`chaos --version` 输出 `chaos 0.2.110 (4055b47)`（带 build 串，JS 跳板自身产不出）、`chaos doctor --json` 可启动，共 13 项检查 12 绿。第 13 项**故意保持红**：元包把六个平台包都 pin 在自己的版本上，而 `chaos-code-win32-x64` / `-win32-arm64` 在 registry 上只有 npm 安全占位 `0.0.1-security`，即 `chaos-code-win32-x64@0.2.110` 这个字符串从未存在；npm 对无法解析的 optional 依赖是静默跳过，因此 `npm install -g --os=win32 --cpu=x64 chaos-code` 会「added 2 packages, exit 0」而一个平台包都不装，用户要到真正执行 `chaos` 时才看到失败。该失败路径由 `crates/codegen/xai-grok-pager/npm/chaos/scripts/test-postinstall.js` 用子 node 执行真实 `bin/chaos`（只覆写 `process.platform`/`arch`）固定：必须退出 1、点名平台、点名 pin 的版本、给出 `npm view <pkg> versions`、且不是堆栈；把 pin 从消息里删掉测试即红。本轮顺带修掉跳板的误导性文案（原本只说「可能 --no-optional 或不支持该平台」，两种都不是真因）。解锁需要仓库外的动作（向 npm 申请回收这两个名字，或决定改名并迁移 pin），`release.yml` 已有同名 `::error::` 发布闸门，与此处保持红一致。另记：npm 上只有 `0.2.110`，仓库已在 `0.4.2`。（2026-10-02；`scripts/npm-install-in-docker.sh`、`docs/verification/npm-install-linux-2026-10-02.log`）

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
- [~] 本行原本设前提「签名 secrets 与 Windows runner 可用后」，该前提已不成立且不再阻塞：`CHAOS_SIGNING_PRIVATE_KEY`（secret，created 2026-08-14）与 `CHAOS_SIGNING_PUBLIC_KEY`（公开 repo variable）都已配置，`v0.4.2` 已发布带签名产物。sidecar 与 embedded key 是否匹配已由 `scripts/verify-release-signature.sh` 正面回答——同一个 example 二进制在注入仓库公钥时接受 `v0.4.2` 的真实 sidecar，去掉该环境变量重新编译后必须以 `public_key=absent` 拒绝，因此「编译进去的公钥确实是签名所用那把」是被证明的而不是被假设的；`--all` 下六个产物全部通过，16 项检查全绿。同一轮还发现并修复了真实隐患：Cargo 默认不跟踪环境变量，改公钥后重新构建可能静默沿用旧公钥，现由 `crates/codegen/xai-grok-update/build.rs` 的 `cargo:rerun-if-env-changed` 与 `signature::tests::the_build_script_makes_the_embedded_key_rebuild` 固定。仍未完成且需要人来做的只剩：一次带 Windows runner 的 release dry-run（本机无 Windows 环境，`.exe` 只做到签名可验证、未执行），以及 tag 创建/推送这类发布 owner 动作。「在该 gate 完成前不创建新 tag」已被 `v0.4.2` 越过，保留原句仅为记录依据。（2026-10-02；`scripts/verify-release-signature.sh`、`docs/verification/release-signature-v0.4.2-2026-10-02.log`）

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
  fortress-breach 全为空。（2026-09-22 实测 `--before main --after HEAD` → 0 / 0 / 0。）本轮 Chromium browser screenshots/logs 当时只写在会话私有临时目录，未随仓库留存，不属于工作树用户输入。
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
- [~] 2026-10-01 Q4 inventory reports one `xai-grok-update` ignored attribute: the opt-in 100k stress test. Five stale tests asserting upstream channel-specific install URLs have been replaced with running tests of the shipped Chaos `reinstall_hint` behavior (stable/alpha/malformed channel equivalence, platform installer, and safe enterprise fallback); the targeted real-entry-point test run passed 8/8. The remaining ignored 100k stress test now has a checked-in `scripts/test-blitz-stress.sh` entry point; its actual test was run with 120 iterations through that script and passed 1/1. The full default 100k run completed on 2026-10-01: 1 passed, 0 failed in 4306.69 seconds through `scripts/test-blitz-stress.sh` (not-retained log `blitz-stress-100k.log`). The 100k stress execution is complete; the independent Q4 owner/reviewer decision is still open.
- [x] 维持 `docs/ci-test-debt.md` 的“append-never, remove-only”规则；当前 `--exclude` 列表为空，CI 使用 `cargo test --workspace` 不含添加 exclusions；baseline gate 只限制 ignored inventory，不替代此规则。（2026-09-25；`.github/workflows/ci.yml`、`docs/ci-test-debt.md`）
- [x] 确认 `registered_features_are_documented` 的 `internal-docs` feature 门控是长期方案还是临时绕过。**结论：是长期方案，且已写清理由。** 该 target `include_str!` 的是 `docs/internal/25-enterprise.md` 与 `docs/internal/22-environment-variables.md`，而 `docs/internal/` **在本仓库全部历史与 `origin/main` 里都不存在**（`git log --all -- crates/codegen/xai-grok-pager/docs/internal/**` 无输出），所以它在 cargo 下**根本无法编译**。`crates/codegen/xai-grok-pager/Cargo.toml` 的 `[[test]]` 条目旁已写明这一点，并给了持有内部文档者的跑法（`--features internal-docs`）。删掉它反而是信息损失：它是唯一把 `FEATURES` 与操作员表格对起来的检查。
- [x] 修掉 `xai-fast-worktree` 里一类**取决于开发机装了什么**的偶发红：`auto_gc` 的重建测试会 union `nfs::candidate_data_dirs()`，其中包含主机真实的 `$HOME/.local/share/grove`；夹具只种 1 个 worktree 而断言 `registered == 1`，于是主机目录里一条无关记录就把断言变红（`scripts/verify-in-docker.sh --full` 的 `cargo test` 门禁先撞上它）。修法沿用该 crate 已有的隔离：`auto_gc.rs` 的 10 处夹具构造改走 `isolated_fixture()`（内部调用 `GrokHomeFixture::isolate_xdg_grove_data()`，把 `XDG_DATA_HOME`/`GROVE_DATA_DIR`/`HOME` 全部收进夹具），并让该方法取 `nfs::GROVE_ENV_LOCK`——`GROVE_DATA_DIR` 是进程级变量，`nfs/remove.rs` 三条测试也写它，不共享一把锁会互相看到对方的中间状态；两处获取无嵌套，不构成死锁。**新增两条与机器无关的机制测试**，钉隔离本身而不是钉一个恰好依赖它的计数：`a_fixture_hides_any_grove_dir_outside_itself`（走 `isolated_fixture()` 本身，先断言候选列表里**有**夹具自己的目录——正向锚点，防止「列表恰好为空」让后半句空转——再断言没有任何一个落在夹具之外）、`a_fixture_clears_a_grove_data_dir_that_was_already_set`（在夹具创建**之前**把 `GROVE_DATA_DIR` 种到夹具之外，正是测试进程带上该变量的真实方式，断言隔离之后它不再出现在候选列表里）。**变异矩阵 5 行**（M1 去掉隔离调用／M2 不再重定向 `HOME`／M3 不再清空 `GROVE_DATA_DIR`／M4 `candidate_data_dirs()` 不再读 `XDG_DATA_HOME`／M5 = M4 再删掉正向锚点），每行改完立刻还原并 `cmp` + sha256 确认字节一致：M1–M4 各让对应新测试变红，**而原有那条计数断言在全部 5 行变异下保持绿色**——它从未证明过隔离，这就是新增测试的理由；M5 全绿则说明缺了正向锚点时 M4 这类改动会连新测试一起放过。**验证**：`cargo test -p xai-fast-worktree --offline --locked --features metadata --lib` **506 passed / 0 failed / 4 ignored**。证据 `docs/verification/fast-worktree-grove-isolation-2026-10-03.log`（docker 原始报错、诊断打印的环境变量快照、变异矩阵逐条报错、以及哪些轮次日志已不存的留存范围说明）。（2026-10-03；`crates/codegen/xai-fast-worktree/src/auto_gc.rs`、`crates/codegen/xai-fast-worktree/src/db/mod.rs`、`docs/verification/fast-worktree-grove-isolation-2026-10-03.log`）
- [x] `xai-fast-worktree` `nfs::client` 测试的 mock daemon 启动等待从「睡 20 ms」换成**就绪信号**：`spawn_server()` 之后原有 19 处 `thread::sleep(Duration::from_millis(20))`，等的是 daemon 线程被调度到 `accept()`——负载下就是随机输的竞态（早前一轮 34 轮里红 2 次）。现在 `spawn_server` 持一个 `mpsc` 端点，daemon 线程在 `bind` 之后、进入 `incoming()` 之前发信号，收不到就 10 s 超时 panic；`listen` 队列保证这一瞬间到达的请求不丢，故 19 处 sleep 全部删除。**保留 3 处 sleep**，因为它们是被测行为本身：`query_interval` 轮询、模拟 daemon 的 `ping_delay` 与 `create_hold`。**验证**：`--lib` 全量 506 passed / 0 failed；`nfs::client` 一类在随后 120 轮全量复测里的计数写在 `docs/verification/fast-worktree-grove-isolation-2026-10-03.log` §5 的对照表。（2026-10-03；`crates/codegen/xai-fast-worktree/src/nfs/client.rs`）
- [~] `xai-fast-worktree` 的 `git::safety::tests::gate::snapshot_under_a_foreign_clean_filter` **机制已查明、子进程这一侧已修**（整仓并发跑时每几十轮红一次：子进程断言 `Safety::Keep(CheckFailed)` 而非 `Delete`；单跑 30 次 0 红、满载 25 轮 0 红）。第一步是**让它下一次红自带原因**：其一，`decide_safety` 的四条失败路径各写一行 `tracing::warn!`，而测试二进制没有 subscriber，四行全丢——两个 `#[ignore]` 子测试现在先调 `log_child_diagnostics()`（`tracing_subscriber::fmt().with_test_writer()`，`WARN` 级别；子进程只跑一条测试故全局订阅者安全，`with_test_writer` 平时静默、只在失败时随 libtest 回显，再经 `run_child` 已有的「把子进程 stderr 拼进 panic」通道回来）。装上后 21 轮里 2 次红都带上了那一行：`path did not open as a git repository path=/tmp/.tmp… reason=CheckFailed`，四选一收窄成 `gix::open` 失败这一条，且 `git_entry_definitely_absent()` 表明 `.git` 当时存在（否则 reason 会是 `NoRepo`）。其二，`gix::open` 的 `NotARepository { source, path }` 的 Display 与原因无关（`MissingCommonDir`／`FindHeadRef`／`MissingObjectsDirectory` 印出来都是同一句话），而 warn 用 `%error` 只印 Display。**本轮第一版改成 `error = %format!("{error:#}")`，事后证明是空操作**：`gix-0.83.0/src/open/mod.rs` 的 `#[error("\"{path}\" does not appear to be a git repository")]` 模板里没有 `{source}`，thiserror 的 `Alternate` 走同一个模板，`gix-discover-0.51.0/src/lib.rs:47` 那 11 个 `is_git::Error` 变体（其中 5 个带 `std::io::Error`）全被压成同一句话——这一点不是推理：把函数换回 `format!("{error:#}")` 跑新加的测试，它红在「必须同时印出 cause 与 errno」那句断言上。现改为手走 `source()` 的 `open_error_chain()`（warn 字段名不变，只换值），并新增 `the_open_failure_log_carries_the_cause_and_not_only_the_path`：构造 `NotARepository → MissingCommonDir → io::Error(EMFILE)` 这条**三层**链，同时断言「哪个文件」与「哪个 errno」；另有一条「遍历但不打印」的变异（`push_str` 换成 `let _ = current;`）同样让它红，两条跑完都 `cmp` 字节还原。**根因（本轮后半段查明）**：装上错误链之后，同一条命令在第 26、94 轮又红，链的第二层是
`is_git::Error::CurrentDir`、第三层是 `ENOENT`；`gix-discover-0.51.0/src/is.rs:36` 在 `is_git()` 的
**开头**无条件执行 `gix_fs::current_dir(false)`，与传进来的路径是绝对还是相对无关，所以那一刻真正在
说的是「**调用方的当前目录已经被 unlink 了**」，`gix::open` 把它包装成 `NotARepository`，门于是答
`CheckFailed`。继承这个坏 cwd 的一侧有名字：`run_child` 用 `Command::new(current_exe())` 重新执行测试
二进制时不指定 `current_dir`。修法是把子进程钉到 `temp_dir()`（子进程原本 cwd 就是仓库之外的普通目录，
钉进夹具会改变那条 `GIT_DIR` 测试的 cwd 类别），并加永久测试
`the_child_does_not_depend_on_the_cwd_it_inherits`——「删掉自己的当前目录」这个动作放在一个只跑它自己
的子进程里：第一版写在父进程，它确实红了，但同时把同模块另外 32 条测试一起弄红（`17 passed; 33 failed`），
那是用制造缺陷的方式测试缺陷。再加一个诊断 `unlinked_cwd()`：目录被删后 `getcwd()` 失败，而
`/proc/self/cwd` 仍然给出它的名字（Linux；macOS 无 `/proc` 时该函数是空操作），子测试里断言这个名字
必须**等于**刚被删掉的那个目录。**三条变异**（跑完 `cmp` 字节还原，均 `RESTORED_BYTE_IDENTICAL`）：
去掉 `.current_dir(std::env::temp_dir());` → **逐字复现历史偶发**（同测试名、同 verdict 对、同三层链）；
让那个临时目录不被删除 → 红在「cwd 确实已被 unlink」这条前置断言上；把 `unlinked_cwd()` 改成恒返回
`None` → 红在「诊断必须说出是哪个目录」上。**计数**：`cargo test -p xai-fast-worktree --lib git::safety`
默认与 `--features metadata` 都是 **50 passed / 0 failed / 3 ignored**，整仓 `--features metadata --lib`
**509 passed / 0 failed / 5 ignored**。**同形状 120 轮对照**：含错误链、不含 cwd 钉住的二进制红 3 次
（第 11、26、94 轮，全部是这一对父子；`nfs::client` 0、`auto_gc` 0），其中两次正是被修掉的形状。此前
已排除并有实测记录：`HOME` 继承（30 次三变体全绿）、线程耗尽与 probe 超时（24 spinner +
`--test-threads=64` 满载 25 轮 0 红）、临时索引名撞车（pid+纳秒+计数）、进程内 `GIT_*` 写入（`grep` 全
crate 无，且同文件早有一条测试钉住「外来 `GIT_DIR` 不改变判决」）。**仍未回答两个问题**，因此本行保持
`[~]`：其一，**谁** unlink 了那个目录——本 crate 三处 `set_current_dir`（`auto_gc.rs:898`、
`process_scan.rs:257`、`CwdGuard::drop`）的 drop 顺序都是「先把 cwd 换回来、再删目录」，逐条读过是对的，
制造侧仍然匿名，而只要它再发生一次，同进程里任何别的 `gix::open` 调用者都会跟着误判；其二，第 11 轮那种
「工作树路径自己消失」的形状（链里只有裸 `ENOENT`，不是 cwd 问题）。两者都记在证据 §5.3 作为待查，
不写成结论。证据 `docs/verification/fast-worktree-safety-gate-flake-2026-10-03.log`。（2026-10-03；`crates/codegen/xai-fast-worktree/src/git/safety_tests/gate.rs`、`crates/codegen/xai-fast-worktree/src/git/safety.rs`、`docs/verification/fast-worktree-safety-gate-flake-2026-10-03.log`）
- [x] 修掉 `xai-fast-worktree` `nfs::client` 的**第二个**偶发机制（就绪信号那一版没修完它）：`timeout_dead_daemon_unmounted_dest_is_fallback_without_second_create` 在 120 轮整仓复测里红 2 次（第 43、50 轮），报错 `called `Result::unwrap()` on an `Err` value: InFlight { phase: "unknown" }`。根因在**夹具自己**：`lost_create_script()` 的 `create_hold = 300ms` 让 mock daemon 在睡完 300ms 之后才 drop flock、unlink sock，而客户端的判决时刻 ≈ `create_timeout`(80ms) + `query_phase` 的 socket 下限 `QUERY_PHASE_MIN_TIMEOUT`(250ms) ≈ 330ms，两者只差 30ms，`is_provably_dead()`（sock 不存在或 ping 不通 **且** flock 已释放）在判决那一刻看到的仍是「锁被持有」。改成 `create_hold: Duration::ZERO` 后，mock 先 drop flock、先 unlink sock、再关掉连接，而客户端那次阻塞读**正是因为**连接被关掉才返回——被探测的两个状态因此在探测之前就已成立，不再与墙钟赛跑；产品代码一行未动，`die_after_create` 仍是无回包的丢失响应，断言的仍是 `poll_after_lost_reply → deadline_decision` 这条真实判决路径。**顺带补上两条此前无人看管的产品行为**：`deadline_decision` 一直会在 daemon 确定已死且 dest 非挂载点时清掉**空**的残留目录（否则 `git worktree add` 拒绝已存在的目录，兜底直接失败），并且**拒绝**在**非空**残留上拷贝兜底（`InFlight { phase: "dest-exists" }`，那可能是死掉 daemon 正在写的半成品投影）；改造前兜底那条测试只看判决、压根不看 dest。现加 `assert!(!dest.exists(), …)` 与新测试 `timeout_dead_daemon_nonempty_leftover_dest_refuses_fallback`（种 `dest/partial="not mine"`，断言 phase **等于** `dest-exists` 而非泛化的 `unknown`、`creates == 1`、字节未被触碰）。**变异矩阵 3 行**（跑完 `cmp` + sha256 还原）：N1 `is_provably_dead()` 取反 → 两条都红，且兜底那条的报错与历史偶发**逐字相同**（同一 unwrap 站点、同一 `InFlight { phase: "unknown" }`），证明历史偶发就是这条路径；N2 去掉 `remove_dir` → 只有 dest 断言红；N3 `if empty` 改成 `if true` → 只有拒绝那条红（`got Ok(Fallback)`）。**测量诚实性**：单条测试单线程 300 次、整模块 24-spinner 200 轮（**带着缺陷重编的对照二进制**）都是 0 红，即这些形状**不能**用来证明修复；只有整仓 `--lib` + `--test-threads=$(nproc)` 复现得起来，改前/改后/因果配对方三组同形状的 120 轮计数记在证据文件 §6。（2026-10-03；`crates/codegen/xai-fast-worktree/src/nfs/client.rs`、`docs/verification/fast-worktree-nfs-dead-daemon-race-2026-10-03.log`）
- [x] 新增门禁 `scripts/ci/check-evidence-paths.py`：提交进仓库的文档不允许把证据指向只存在于一次会话里的 scratch 目录。审计发现 40 处此类指针（一类写「详见会话 scratch」，另一类直接给出会话 scratch 根目录下的绝对路径），那些目录随会话结束删除，读者照着找不到任何东西，文档于是自己宣布不可复核。门禁 `os.walk` 跳过 `.git`/`target`/`node_modules`/`dist`/`build`（首版用 `rglob("*")` 会走进 `target/`，一次扫描挂住不返回），扫全部 `*.md` 与 `docs/` 下的 `*.tsv`（**生成的** tsv 也扫；`scripts/ci/*.tsv` 与 `docs/verification/*.log` 不扫——日志本来就要抄命令），四条模式逐条写在同文件的 `PATTERNS` 里（中英文两种「scratch 是证据所在」的说法、scratch 根目录的绝对路径前缀、两种把会话目录称作 scratch 的英文写法），TODO 里**不复述字面量**——门禁对本条描述自我命中过三处并已改正，这个自我命中记在 CHANGELOG 同一条目里。唯一豁免是一条**具名**条目（`xai-grok-shell` 里随产品发布的 prompt 模板，它教模型怎么用 scratch），且豁免条目若从扫描集里消失，门禁自己红——防止删了规则却留着豁免。**验证**：`test-check-evidence-paths.py` 16 passed（夹具覆盖中英说法、连字符与大小写变体、绝对 scratch 根、两种英文称作 scratch 的写法、生成的 tsv 被扫、`scripts/ci/*.tsv` 与 `docs/verification/*.log` 不被扫、构建产物不被 walk、豁免生效、豁免失效即红、真实仓库干净）；`check-evidence-paths.py` 在真实树上 275 个文件 0 命中。**变异矩阵 6 行如实记录**：M1（TODO.md 埋一处说法）红、M2（只在生成的 tsv 里埋）红、M3（豁免键挪走）红、M4（豁免指向不存在的路径）两处红，而 **M5「把 `target/` 也走一遍」与 M6「删掉中文那条模式」在今天的树上都是绿的**，只有夹具能证明这两条被覆盖。40 处分两遍清：第一遍脚本替换留下的句子读不通、还漏一处绝对路径，第二遍逐条手写。门禁同时进 `.github/workflows/ci.yml` 与 `scripts/verify-in-docker.sh` 的 `gates=()`，由 `check-guard-wiring.py` 双向钉住。（2026-10-03；`scripts/ci/check-evidence-paths.py`、`scripts/ci/test-check-evidence-paths.py`、`.github/workflows/ci.yml`、`scripts/verify-in-docker.sh`、`TODO.md`、`CHANGELOG.md`、`docs/architecture/*.md`、`docs/audit-followup-report.md`、`docs/ci-test-debt.md`）


---

## MT-6：unsafe / unwrap 收敛

**Owner**：TBD  
**阻断级别**：P2（单条 P0 除外，见下）

`docs/audit-followup-report.md` §5 给了按投入产出比排的顺序，但没有任何勾选入口，
结果是报告写完就停在那里。按原顺序落为任务：

- [~] B 批 unwrap 治理：本轮移除 `xai-grok-update::fetch_gcs_channel_pointer` 对重试错误状态的 `unwrap()`，增加无错误状态的结构化 fallback；现有 `gcs_pointer_connection_refused_is_retried_and_returns_error` 验证真实网络失败仍返回错误；对应单个集成测试和 update crate lib tests 均通过。更新 crate 原有 5 处生产进度条模板解析 unwrap 已统一替换为 `progress_style_or_default`：非法模板记录 warning 并使用相应默认样式；真实 helper regression 以无效 indicatif 模板验证 fallback，不依赖终端颜色/网络。更新 crate lib 166 tests、fmt、strict Clippy 通过（`{SCRATCH}/updater-progress-style-test.log`、`updater-progress-style-lib.log`、`updater-progress-style-fmt.log`、`updater-progress-style-clippy.log`）；`docs/audit-followup-report.md` 的历史生产 unwrap 计数仍需全仓实测刷新；`xai-grok-sandbox` 旧计数 28 尚待逐项生产/测试分类和审查，未把整批标为完成。（2026-09-25；`crates/codegen/xai-grok-update/src/version.rs`、`crates/codegen/xai-grok-update/tests/test_network.rs`）
- [~] unsafe P0 审计：逐项复核 `xai-grok-sandbox` 与 `xai-tty-utils` 当前源码 unsafe block/fn/impl/extern，逐条核对平台 cfg、SAFETY 前置条件和真实调用路径。修复发现的安全缺口：sandbox `child_net` 实现模块改为私有；所有 workspace/MCP/pager/terminal/hooks/LSP child spawn 调用已迁移到公开的 `restrict_child_network` / `_std` 安全封装。底层 BPF builder/install API 为 crate-private，不能作为下游可调用的任意 policy 安装接口；namespace TSYNC API 的不可逆、全线程及后续 mount/namespace syscall 拒绝影响已写入 unsafe contract；`statvfs/fstatvfs` 输出缓冲区补齐局部 SAFETY rationale。`xai-tty-utils` 的真实终端 stderr 保存/dup descriptors 改为 `F_DUPFD_CLOEXEC`，防止后续 exec 子进程继承可绕过 stderr 屏蔽的终端句柄；Windows `dup_tui_stderr` 不再将可能无效的 GetStdHandle 直接包装为 File，而是校验并通过 DuplicateHandle 创建真正 owned handle；Windows Job Object Send/Sync unsafe impl 补充线程/内核句柄安全论据。移除 Linux pdeathsig pre-exec hook 中 debug-only `std::thread::current().id()` 检查，避免 fork 后 TLS handle 惰性初始化/分配；同线程 arm/spawn 明确为调用方契约。新增回归通过跨线程 armed command 的真实 pre-exec 路径，确认不会因 TLS 检查 panic/阻止执行。现有真实 Unix-socket child-network integration 证明允许控制连接、restricted child 获 EPERM；实际 stderr 重定向 subprocess 检查保存与 caller dup 均设置 FD_CLOEXEC。Windows/macOS 的代码路径目前仍只有静态审查；CI 已加入 `platform-tests`（`macos-14`/`windows-latest`）以在目标 OS 编译并运行这两个 crate 的测试，但首轮远端结果尚未观察，不能提前当作平台实测证据。两 crate 全量测试、all-target check、strict Clippy 和 fmt 通过；最终 Rust workspace suite 亦通过（31,172 passed / 0 failed / 490 ignored，按 CI 要求设置 `RUST_MIN_STACK=16777216`；scratch `verification/final-rust-workspace-tests-after-api.log`）；目标 crates `xai-grok-sandbox`、`xai-grok-hooks`、`xai-grok-mcp`、`xai-grok-pager`、`xai-grok-shell-terminal`、`xai-grok-tools`、`xai-grok-workspace` 的 all-target check 和 E2E child-spawn gate 均通过；本轮静态逐项审计没有发现 read-deny fd ownership/statx 或 tty kill-on-drop 其他已证实漏洞。还发现 per-spawn filter 的网络保证假设子进程没有继承已连接网络 socket，必须纳入威胁审查和 spawn descriptor audit。仍待独立安全 Reviewer 对 P0 审计签字及 macOS/Windows 平台编译运行，因此 P0 不关闭；restricted child 会继承的既有 network socket fd 风险已经记录到 `docs/audit-followup-report.md` §1.7，必须先梳理受限 spawn 的 fd 来源/close policy，再给网络保证作最终 signoff。`xai-tty-utils` test-only env mutation 位于单测内部即 set→hook selection→remove，前置 lock 只锁本测试 sibling；这是 test harness 无法全局序列化其他线程 env access 的审计备注，未改全进程 env 架构。（2026-09-30；`crates/codegen/xai-grok-sandbox/src/child_net.rs`、`src/hook_write_deny.rs`、`src/read_deny_verify.rs`、`tests/child_net_e2e.rs`、`crates/codegen/xai-tty-utils/src/lib.rs`、未留存日志 `verification/mt6-sandbox-tty-check.log`、`mt6-sandbox-tests-final3.log`、`mt6-child-net-e2e-final-clean.log`、`mt6-child-net-e2e-final-ack.log`、`mt6-sandbox-ignored-tests.log`、`mt6-tty-tests-final-portfolio.log`、`mt6-tty-stderr-cloexec-regression.log`、`mt6-pdeath-signal-safety-regression.log`、`mt6-p0-check-portfolio.log`、`mt6-p0-clippy-portfolio.log`、`mt6-p0-fmt-portfolio.log`、`mt6-unsafe-clippy-final-clean.log`、`mt6-child-network-private-api-check.log`、`mt6-network-api-final-check.log`、`mt6-network-api-final-e2e.log`、`mt6-network-api-final-ignored.log`、`rust-workspace-mt6-final.log`、`mt6-p0-clippy-final-api.log`、`mt6-child-network-private-api-check.log`、`mt6-child-network-private-api-e2e.log`、`mt6-sandbox-private-api-tests.log`、`mt6-child-net-e2e-final-ack.log`、`mt6-sandbox-ignored-tests.log`、`mt6-p0-clippy-final-api4.log`）
- [~] `xai-tty-utils` 的一条资源计量测试原本在已通过 `expect` 的 `self_time` 上重复调用 `unwrap()`；现复用同一局部值，与同一次采样的 user/system 字段作精确比较。真实资源读取测试、fmt、strict Clippy 通过（未留存日志 `tty-resources-unwrap-test.log`、`tty-resources-unwrap-fmt.log`、`tty-resources-unwrap-clippy.log`）；只清理了这一个无必要 unwrap，不替代整 crate unwrap / unsafe 审计。
- [x] `xai-grok-shell` 的环境变量 `unsafe` 消除：该 crate `unsafe` 关键字行从 327 降到 49（-85%，超过原估的 ~60%），crate 内只剩 `external_otel_pin::apply_process_env_strip` 一处保留 `unsafe`（它本身是带 caller contract 的 `pub unsafe fn`）。做法：把环境写操作收敛到 `xai-grok-test-support::env` 的单一入口——进程级写锁 + 线程本地重入计数（`with_write_lock` 可嵌套而不自锁死），`set_var`/`remove_var`/`var_os` 各自只包一个带 SAFETY 注释的块，`EnvGuard` key 放宽为 `Cow<'static, str>` 以支持运行时构造的 key，`isolate_grok_env` 也改走同一入口。274 处 `unsafe { std::env::set_var/remove_var }` 与 4 处混合 `unsafe` 块（PATH/GIT_EXEC_PATH、credential 早失效 guard、app.rs `set_or_clear`、`with_api_key_env`）全部改为 helper，多步读改写放进一次 `with_write_lock` 以保证成对写入原子。同时修掉真实缺陷：生产代码不再在运行时改进程环境——`initialize()` 从 auth.json 读到 key 和 `x.ai/setApiKey` 原本调用 `std::env::set_var("XAI_API_KEY", ..)`，这是与其他线程 `std::env::var` 并发的进程全局写，且把密钥泄进之后每个子进程（shell 工具/hook/MCP server）的 env block；改为 `auth_method` 内 `RwLock` 保护的 runtime key cell（Present/Cleared/Unset 三态，Cleared 只屏蔽 `XAI_API_KEY`、保留 legacy 变量，与原 `remove_var` 作用域一致）。新增测试：test-support 6 项（真实进程 env 生效、guard 恢复既有值/新建键、panic 时仍恢复、嵌套写锁不死锁、8 线程并发写各自读回、成对写对读者原子）+ shell 4 项（未设置时环境权威、runtime key 覆盖环境且不回写 env、clear 的屏蔽范围、并发读写不撕裂），全部通过；`xai-grok-shell --all-targets`、workspace `--all-targets` check 与 `cargo fmt --check` 通过。（2026-10-02；`crates/codegen/xai-grok-test-support/src/env.rs`、`crates/codegen/xai-grok-shell/src/agent/auth_method.rs`、`src/agent/mvp_agent/acp_agent.rs`、`src/extensions/auth.rs`；未留存日志 `env-helper-tests.log`、`runtime-api-key-tests.log`）
- [x] 环境变量 `unsafe` 收敛推广到其余四个高频 crate（`xai-grok-workspace`、`xai-grok-update`、`xai-fast-worktree`、`xai-grok-pager`），复用上一行同一个 `xai-grok-test-support::env` 加锁入口。计量口径统一为 `crates/**/*.rs` 中 `unsafe {` 的出现次数：上游 `SOURCE_REV 72a61251` 为 1028，上一行收敛后为 768，本步为 **586**。逐 crate（`unsafe {` 块数 / 裸 `std::env::set_var·remove_var` 调用数）：workspace 112→4 / 110→1，update 27→1 / 30→0，fast-worktree 70→55 / 32→4，pager 58→25 / 39→3；fast-worktree 残留的 55 个 `unsafe` 全部是 libc/FFI（AF_UNIX socket、procfs/vnode 查询、sigaction、kill/waitpid），与环境变量无关。三个 crate 新增 `xai-grok-test-support` 作为 dev-dependency（该 crate 不依赖三者，无环）。多步读改写包进一次 `with_write_lock` 保证成对写入原子：fast-worktree `GrokHomeFixture::drop`（一次恢复 `GROK_HOME`+`XDG_DATA_HOME`+`GROVE_DATA_DIR`+`HOME` 四个键，避免读者看到半恢复状态）、update `InstallerEnvGuard::isolate`/`drop`（四个安装器变量的批量清空/恢复）、pager `EnvVarGuard::drop`。**保留裸 `unsafe` 的 5 处生产写入**并各自补上 SAFETY 依据，原因是它们位于 library/bin 的非 `cfg(test)` 代码里，而加锁 helper 所在的 `xai-grok-test-support` 只是 dev-dependency，正常构建不可达：pager `app/mod.rs` 的 `GROK_LOG_SAMPLING`（CLI 启动、spawn 线程前）、`app/event_loop.rs` 的 `GROK_OPEN_DASHBOARD_AT_STARTUP`（event loop 之前的 init）、`app/screen_mode_relaunch.rs` 的 `GROK_SCREEN_MODE_ENV`（子进程不得继承屏幕模式覆盖，原有 SAFETY 注释已扩写）、workspace `bin/workspace_server.rs` 的 `RESET_CHILD_OOM_ENV`（tokio runtime 构建前的原始终启动线程）、fast-worktree `auto_gc.rs` 的 `clear_auto_gc_env_for_test`（`#[doc(hidden)] pub unsafe fn`，跨 crate 测试复用，caller contract 已写入 `# Safety` 段并说明为何不走 helper）。**验证**：`cargo check --workspace --all-targets` 干净；`xai-grok-workspace --lib` 1934 passed / 0 failed，`--bins` 26 passed；`xai-fast-worktree --lib` 504 passed / 4 ignored，`xai-grok-update --lib` 164 passed；`xai-grok-pager --lib` 9196 passed / 0 failed / 25 ignored；四 crate `--all-targets` strict Clippy `-D warnings` 与 `cargo fmt` 干净。（2026-10-02；`crates/codegen/xai-grok-workspace/src/{lib.rs,status_config.rs,handle_tests.rs,session/tool_config.rs,bin/workspace_server.rs}`、`crates/codegen/xai-grok-update/src/auto_update_tests.rs`、`crates/codegen/xai-fast-worktree/src/{db/mod.rs,auto_gc.rs,nfs/remove.rs}`、`crates/codegen/xai-grok-pager/src/test_util.rs` 等 27 个文件；未留存日志 `workspace-lib-tests.log`、`workspace-bin-tests.log`、`update-fw-lib-tests.log`、`pager-lib-tests.log`、`check-ws-env3.log`、`clippy-env4.log`）
- [~] A 批 unwrap 治理：`xai-grok-shell` 的大批量清理仍需逐批实施和审查。已移除 `claude_import::apply_hooks_to_dir` 在新 hook JSON 对象构造上的两个结构性 `unwrap()`，改为直接构造 `serde_json::Map`，序列化结构不变；helper 的实际临时文件写入/去重/timeout tests 覆盖此路径。另移除 `SessionSignalsActor::GetSnapshot` 对非空 ITL buffer 的 `max().unwrap()`，用 Option `max()` 直接守住非空分支，新增 actor 测试提交 inference metrics 后经真实 snapshot 验证 buffered mean/max。再移除过期内存凭据路径上 `run_auth_flow` 对已校验磁盘凭据的 `unwrap()`，由真实临时 auth-store regression 验证复用兼容且未过期的磁盘凭据。另将 stop-gate 转换测试读取 description 的测试专用 unwrap 改为带语义提示的 expect，并复跑映射器测试；这不计入生产 unwrap 减量。附加的 session-updates 本地正确性切片将 tail offset 的 `i64::MIN` 安全化，并对 turn limit 使用饱和加法，避免极端合法参数导致 signed overflow/panic；实际 `x.ai/session/updates` handler 的持久化 JSONL 回归和整个扩展模块 13 项测试通过。该增强与 unwrap 数量无关，不作为 A 批计数或关闭证据；随后亦以真实 handler 回归补测 `limit: usize::MAX` 与 `i64::MIN` 组合及非 rewind turnIndex，13 项扩展模块测试、fmt 和 strict Clippy 均通过。（2026-09-30；`crates/codegen/xai-grok-shell/src/extensions/session_updates.rs`、not-retained log `verification/session-updates-bounds-regression.log`、`session-updates-limit-bound.log`、`session-updates-tests-verified.log`、`session-updates-fmt-verified.log`、`session-updates-clippy-verified.log`）三处生产局部 unwrap 批次有 shell crate strict Clippy 和 fmt 证据，不代表全 crate 完成。（2026-09-30；`crates/codegen/xai-grok-shell/src/claude_import.rs`、`src/session/signals.rs`、`src/session/signals_tests.rs`、`src/auth/flow.rs`；未留存日志 `verification/claude-import-hooks-test.log`、`signals-buffered-itl-test.log`、`auth-disk-token-test.log`）
- [~] 已将单个 `xai-grok-update` 错误分支收敛写入审计跟进报告并实测 update crate lib 与集成路径；其余批次完成后仍须用当前源代码重算生产 unwrap/unsafe 分布，不能沿用 2026-08 旧计数。另，GitHub run `36165469964` 暴露 `xai-grok-shell` current-thread actor tests 在 test harness 默认小栈下溢出；统一提高 CI `RUST_MIN_STACK` 至 16 MiB 后 package lib tests（6,804 passed）、full workspace tests 和 run `36178108811` 均通过。其中"重算生产 unwrap/unsafe 分布"这半条已于 2026-10-02 完成，见下面两行与 `docs/audit-followup-report.md` §2.5。（2026-09-25；`docs/audit-followup-report.md`、scratch `mt6-update-unwrap-test.log`、`xai-grok-shell-lib-16m-after-cleanup.log`、`rust-workspace-16m-after-ime.log`）
- [x] 生产 unwrap / unsafe 分布**已按当前源码重算并固化为可复现口径**，`docs/audit-followup-report.md` §2.1–§2.4 的 2026-08 数字正式作废（该节顶部已标注作废并指向 §2.5，§1.8/§5/§6 同步）。新增 `scripts/ci/panic-site-census.py`：口径是"模块图可达 + cfg 区间判定"而不是 grep 关键字——先对注释/字符串/字符字面量做保位空白化，再按（文件位置 `tests|benches|examples` / 整文件 `#![cfg(test)]` / 命中点落在提及 `test` 的 `cfg(...)` 区间内，`cfg_attr(not(test), …)` 明确不算）三分法判定测试归属，最后只统计被 crate root（cargo 约定 root + `Cargo.toml` 的 `path = "*.rs"`）经 `mod` 链可达的文件；crate root 的 `mod x;` 找 `../x.rs`、普通模块找 `<stem>/x.rs`、`mod.rs` 用父目录，被 test 门控的边不参与"生产可达"但参与"任意构建可达"。2026-10-02 实测：**31,621 个 `.unwrap()` 中 351 个在生产（1.1%）**，生产 `expect` 596、生产 `panic!/unreachable!/todo!/unimplemented!` 132，unsafe 位点 654（596 block / 25 fn / 7 impl / 26 extern）其中生产 420；此前引用的 2,292 生产 unwrap 无法复现且明显偏高一个数量级。固化为两条 CI 棘轮（`.github/workflows/ci.yml` guards step）：`--check-baseline scripts/ci/panic-site-baseline.tsv`（97 个 crate 的生产位点只许降不许升，round-trip 输出 `baseline holds: 97 crates, 0 fewer production sites than recorded`）与 `--check-uncompiled scripts/ci/uncompiled-sources.txt`（见下条）。**扫描器自身经过 6 组注入变异证明非空转**（`scripts/ci/test-panic-site-census.py` 生成含 `src/bin/tool.rs` 兄弟子模块、`#[path]` 别名声明、`cfg(all(test, unix))`、`cfg_attr(not(test), …)`、`#![cfg(test)]` 整文件、`tests/common/mod.rs`、故意不被声明文件的 fixture crate）：不承认任何文件是 crate root → 9 条检查失败；忽略哪些 `mod` 边被 test 门控 → 5；把每个文件都当 root → 5；把"无构建编译"的文件计入位点 → 4；同文件有 test 声明就把该文件全部声明判为 test 门控 → 4；忽略普通声明 → 4。修口径过程中生产数从 617 纠正到 351，方向已按危险侧核过：变动最大的都是 `#[cfg(test)] #[path = "…_tests.rs"] mod tests;` 形状（`queue_tests.rs` 309、`auto_update_tests.rs` 305、`git_restore_code_tests.rs` 312、`manager_tests.rs` 261），逐个回读确认 `#[cfg(test)]` 确实在场。（2026-10-02；`scripts/ci/panic-site-census.py`、`scripts/ci/test-panic-site-census.py`、`scripts/ci/panic-site-baseline.tsv`、`.github/workflows/ci.yml`、`docs/audit-followup-report.md` §1.8/§2.5/§5/§6；`docs/verification/panic-site-census-2026-10-02.txt`）
- [x] 扫描器暴露出一类此前任何检查都看不见的真实缺陷：**12 个 `.rs` 文件没有任何构建会编译它们**（没有 crate root 能通过 `mod` 链到达），编译器和 clippy 都不报错，测试还能照常绿，作者和台账都以为那份覆盖存在。清单固化为 `scripts/ci/uncompiled-sources.txt` 并进 CI（集合一字不许变：新增文件、或某条其实又已被编译，都失败；当前输出 `uncompiled set holds: 12 files, exactly as recorded`）。其中 **`/adhd` 是已确认的用户可见回归并已修复**：`crates/codegen/xai-grok-pager/src/slash/commands/adhd.rs`（106 行）随 release `480aa28a "release 0.2.123: #15 #16 /fallback /adhd"` 发出去过，但 `slash/commands/mod.rs` 里从来没有 `mod adhd;`（`git log -S"mod adhd"` 对该文件返回空），发布构建里该命令根本不存在，而 `[adhd].enabled` 仍被 `xai-grok-shell/src/agent/config.rs::AdhdConfig` 解析并在 `xai-grok-pager/src/acp/mod.rs` 注入规则——即配置能开、命令找不到的半接线状态。现已补 `pub mod adhd;` + `builtin_commands()` 注册，并按 shell 侧既有契约把 `adhd` 加进 `xai-grok-shell/src/session/slash_commands.rs::PAGER_COMMAND_KEYS`（否则同名 skill 会遮蔽/被遮蔽，`pager_builtin_triggers_are_reserved_in_shell` 就是这么发现的）。新增守护测试三条，实跑 `xai-grok-pager --lib slash::commands::tests` **48 passed / 0 failed**：`every_command_file_in_this_directory_is_declared()`（目录内每个 `*.rs` 必须被某个兄弟文件的 `mod x;` 或 `#[path = "x.rs"]` 引用，例外必须写进 `DELIBERATELY_UNDECLARED` 并附理由，例外若指向已不存在的文件同样失败）、`the_adhd_toggle_is_reachable_through_the_registry()`（真实 `CommandRegistry::new(builtin_commands()).get("adhd")` 查找路径）、以及 `adhd.rs` 内四条替换掉原 theater 测试（原 `toggle_logic` 只是 `assert!(!false)`）的真实回归：缺文件/坏 TOML/非 bool 一律读成 off、`/adhd` 与 `on|true|1|yes|off|false|0|no` 在真实 `config.toml` 上往返、切换保留其他键（`[model] name` 仍在）、未知参数经**真实 `AdhdCommand::run` 入口**返回带用法的错误且不创建配置文件；持久化逻辑改为接受显式路径的 `apply_toggle`/`load_adhd_enabled_from`/`persist_adhd_enabled_in`，`run` 只是用真实 config home 调它，因此测试不碰用户 `~/.chaos`。非空转证明：往该目录放一个无人声明的 `zz_undeclared_probe.rs` → 守护测试失败并点名该文件，删掉即恢复通过。`/fallback` **故意不复活**（在全仓无任何代码解析 `[fallback] models`、采样器无 fallback 链的事实下，声明它等于 advertised 一个静默无操作的命令），理由写进 `DELIBERATELY_UNDECLARED`。（2026-10-02；`crates/codegen/xai-grok-pager/src/slash/commands/{mod.rs,adhd.rs}`、`crates/codegen/xai-grok-shell/src/session/slash_commands.rs`、`scripts/ci/{panic-site-census.py,uncompiled-sources.txt}`、`.github/workflows/ci.yml`；未留存日志 `pager-slash-commands-tests2.log`、`pager-adhd-and-guard.log`）
- [x] 剩余"无构建编译"的文件**已逐个判定完毕，`scripts/ci/uncompiled-sources.txt` 现在是空集**（CI 输出 `uncompiled set holds: 0 files, exactly as recorded`；`every_command_file_in_this_directory_is_declared` 的 `DELIBERATELY_UNDECLARED` 例外表同时清空）。判据统一为两条：功能在当前源码里是否真实存在，以及该文件是否还带着别处没有的覆盖。逐个结论——
  **接上（5 个）**：`xai-grok-pager/src/slash/commands/fallback.rs`（读取方 `MvpAgent::select_fallback_model` 已落地，见下面那条）；`xai-grok-shell/src/session/acp_session_impl/incomplete_end_turn.rs`（opt-in 重试环）；`xai-grok-shell/src/session/acp_session_impl/selective_compaction.rs` 与 `src/session/dcp_config.rs`（DCP 复活）；`crates/common/xai-grok-compaction/src/strategies/{mod.rs,deduplication.rs,purge_errors.rs}`（`lib.rs:51` 已 `pub mod strategies;`）。上一条记的"注入 `fn deliberately_broken(({{{ not rust` 后 `cargo check -p xai-grok-compaction` 仍干净"这一实测，现在方向反过来：同样的破坏会让 check 失败。
  **删除·被内联测试取代（2 个）**：`xai-grok-pager/src/views/dashboard/render_tests.rs`（4,333 行）与 `state_tests.rs`（6,101 行）。两者都不是"坏了所以编译不过"——把它们临时声明进宿主后 `cargo check -p xai-grok-pager --lib --tests` 报 **0 个错误位点**，也就是说它们本可以编译。删它们的依据是覆盖集合：`render_tests.rs` 的 120 个函数名与宿主 `render.rs`（8,774 行，277 处 `fn`/`#[test]` 命中）内联 `mod tests` 的名字集合**完全重合、orphan-only = 0**；`state_tests.rs` 265 个函数名 vs 宿主 `state.rs`（10,652 行）384 个，**orphan-only = 0**。留着只会让同一批断言跑两遍。
  **删除·驱动已不存在的代码（1 个）**：`xai-grok-pager/src/app/dispatch/tests/usage_partial_failure.rs`（297 行）。这条先前被我误判过：第一次探针把它挂到 `app/dispatch/mod.rs`，得到 27 个错误位点——宿主找错了，真实宿主是 `app/dispatch/tests/mod.rs`（`app/dispatch/mod.rs:71` 是普通 `mod tests;`）。按正确宿主重测并做名字集合比较：11 个 `#[test]` **全部 orphan-only**（`tests/` 目录 20 个兄弟模块 0 覆盖），所以"重复覆盖"不成立。删它的依据是它们驱动的对象已整体消失：`fill_session_usage_detail` / `fill_aggregate_usage_detail` 及其 `_failed` 变体在当前源码里搜不到任何定义，`/usage` 改由 `views::usage_modal`（`open_usage_info_modal`）承担，`x.ai/usage/aggregate` 侧连抓取 effect 都不存在。这条连带挖出一个真实缺陷，见下一条。
  **部分接上（1 个）**：`xai-grok-pager/src/views/shortcuts_help_tests.rs`（2,316 行）。编译探针 3 个错误位点，全是 E0425（`UNDO_LONG_HELP` / `REDO_LONG_HELP` / `SCROLLBACK_SEARCH_LONG_HELP` 已不存在）。86 个函数名里 12 个 orphan-only（6 个测试），逐条回读源码判性质：`enter_on_search_pseudo_row_opens_detail`、`search_pseudo_row_expands`、`build_entries_includes_find_search_in_simple_mode` 三条断言的行为已被**有意反转**，活文件里对应的是 `enter_on_search_pseudo_row_does_not_open_detail`、`search_pseudo_row_does_not_expand`、`build_entries_omits_scrollback_search_in_simple_mode`；undo/redo/history 三条对应的常量与 `ActionId` 变体都已不存在（`shortcuts_help.rs` 里 `*_LONG_HELP` 只剩 `PASTE_LONG_HELP`，全文再无一处 undo/redo/history 字样）。唯一仍成立的是 `build_entries_lists_prompt_stash_with_ctrl_s_and_alt_s`——`ActionId::StashPrompt` 至今带着 Ctrl+S / Alt+S 的完整 `ActionDef`（`actions/defaults.rs:713`）并在 `app/agent_view/prompt.rs:629` 有处理——已按活文件的断言风格移植回 `shortcuts_help.rs` 的内联 `mod tests`。
  顺手补上这批文件掩盖的那一类真实缺口：`build_entries` 是 registry 驱动（函数文档原文 "All registered actions are included"），但**没有任何测试遍历 registry**，所以一个绑了键的新命令可以永远不出现在快捷键窗口里而全绿。新增 `every_keybound_registry_action_gets_a_cheatsheet_row`，遍历真实 `ActionRegistry::defaults()`，允许缺席的三种情形都写成有依据的条件（无键的 slash-only、voice 门控关闭时的 `VoiceToggle`、被同类目同键 dedup 让位且让位对象确实上榜），其余一律红。实跑 `xai-grok-pager --lib views::shortcuts_help` **67 passed / 0 failed**。非空转注入变异（文件按字节 `cmp` 还原）：给 `build_entries` 加一行 `if def.id == ActionId::StashPrompt { continue; }` → 守护与被移植的 stash 用例**同时**红，报错原文 `keybound actions with no row in the shortcuts cheatsheet: ["StashPrompt (label \"暂存\")"]`。（2026-10-02；`crates/codegen/xai-grok-pager/src/views/shortcuts_help.rs`、`scripts/ci/uncompiled-sources.txt`；未留存日志 `cheatsheet-guard.log`、`mut-cheatsheet.log`、`census-final.txt`）
- [x] 上一条追查 `usage_partial_failure.rs` 时挖出的真实缺陷：**状态栏"累计 token" chip 悬停会高亮，但点了没有任何反应**。`hit_total_tokens` 的 rect 每帧在 `app/agent_view/render.rs` 写入、`app/agent_view/input.rs` 每帧更新 hover 并据此改变 chip 配色，而全仓唯一的点击处理分支写在 `if self.usage_detail.is_some() { … }` 里；`usage_detail` 这个字段在生产代码里**只有 `= None` 一处写入**（`close_usage_detail`），从未被置成 `Some`，`usage_detail_generation` 也只被初始化为 0、从不自增。也就是说 `views/usage_detail.rs`（849 行）、它的 `[✗]` 关闭按钮 hit-rect、Esc/`q`/滚轮吞键分支、以及 6 条手动 `agent.usage_detail = Some(UsageDetail::Loading)` 的测试，构成一整套"看起来在用、实际任何操作路径都进不去"的死面；那 6 条测试尤其误导——它们手工摆出生产状态来测处理逻辑，而这个状态生产根本造不出来。修法按现有产品事实来：不复活已被 `views::usage_modal` 取代的弹层，而是让 chip 兑现自己的承诺——`app/mouse.rs` 里紧邻 `hit_context` 的分支加一条，点击 `hit_total_tokens` 返回 `InputOutcome::Action(Action::ShowUsage)`，即 `/usage` 本身走的那条 dispatch；沿用 `CONTEXT_CLICK_DEBOUNCE_MS` 但用独立的 `last_usage_chip_click_at` 字段，避免两个 chip 共用时间戳导致先点其中一个会吞掉另一个 300ms 内的点击。同时删除死面：`views/usage_detail.rs` 与其 `pub mod` 声明、`usage_detail` / `hit_usage_close` / `usage_detail_generation` 三个字段及初始化、`close_usage_detail`、render 的弹层分支、`notices.rs` 提示遮挡判定、`input.rs` 的吞键分支与 Esc 消费者判定、`panes.rs` 的滚轮吞掉、`minimal/api.rs` 的表面可用性判定，以及那 6 条自摆状态的测试；`render.rs` 里指涉该模块的注释改为直接说明宽字符伪空格这件事本身。全仓 `grep usage_detail` 命中 0。新增 3 条测试经**真实 `handle_mouse` 入口**驱动：chip 内点击必须返回 `Action::ShowUsage`、连点第二次必须被 debounce 吞成 `Unchanged`、chip 右边界外一格不得触发。实跑 `xai-grok-pager --lib app::mouse::tests` **14 passed / 0 failed**。（2026-10-02；`crates/codegen/xai-grok-pager/src/app/{mouse.rs,agent_view/mod.rs,agent_view/session.rs,agent_view/input.rs,agent_view/viewer.rs,agent_view/render.rs,agent_view/notices.rs,agent_view/panes.rs}`、`crates/codegen/xai-grok-pager/src/minimal/api.rs`、`crates/codegen/xai-grok-pager/src/views/mod.rs`；未留存日志 `usage-chip.log`、`pager-lib-after-usage-retire.log`）
- [x] `/fallback` 的实质功能：`[fallback] models` 现在有了读取方。新增 `agent::config::FallbackConfig { models: Vec<String> }`（由 `Config::new_from_toml_cfg` 解析），挑选逻辑集中在 `agent::models::first_selectable_fallback`——逐条按 persisted 模型同一套 catalog key / 路由 slug 解析（复用 `selectable_catalog_key_for_persisted`），跳过账号当前选不中的、也跳过它正要替换的那个模型，取第一个真能服务的；接线在"模型不见了"被解决的两处：`restore_persisted_model` 排在内置同族自选**之前**（用户显式说过要什么，不该先被一次 HashMap 迭代顺序的偶然选择代替），以及 `prompt()` 里"模型在 load 时就不可用"的阻塞路径（换不上就重新 `set_unavailable_model` latch，绝不带着 catalog 里没有的模型继续跑）。`fallback.rs` 已按上一条的前提接进 `pub mod fallback;` 与 `builtin_commands()`，`fallback` 同步进 `PAGER_COMMAND_KEYS`（否则同名 skill 会遮蔽它），`26-config-reference.md` 补 `### fallback` 文档行。范围按事实收窄并在命令提示语与文档行里写明：这条链管**可用性**，不是单请求重试——turn 中途跨 family 切换会让历史里 model-minted 的 reasoning 条目失效（`encrypted_content_mismatch` 就是那个形状），限流与 5xx 仍走各自的 retry 与终态路径。测试：pager 7 项（含跨 crate 的 `the_written_chain_is_read_by_the_shell_config_loader`，直接调 `Config::new_from_toml_cfg`，写方与读方形状对不上就红）+ shell 3 项 + 2 条守护，实跑 pager `slash::commands::fallback slash::commands::tests` **57 passed / 0 failed**、shell `fallback_wiring fallback_chain` **4 passed / 0 failed**。非空转由五处注入变异证明（每次一处、文件按字节还原）：去掉"跳过被替换模型"→ slug 用例红；只取链首不 walk → walk 与 slug 两条红；把链分支挪到同族自选之后 → 结构守护红；写方键名改成 `chain` → 往返 / 报告 / 跨 crate 读取三条红；删掉 `Arc::new(…)` 注册 → 注册表守护红。覆盖限制：两个接线的调用点都需要活的 `MvpAgent` 与真 catalog，只有结构守护测试，未端到端驱动。（2026-10-02；`crates/codegen/xai-grok-shell/src/agent/{config.rs,models/resolution.rs,models/tests.rs,mvp_agent/session_setup.rs,mvp_agent/acp_agent.rs}`、`crates/codegen/xai-grok-shell/src/session/slash_commands.rs`、`crates/codegen/xai-grok-pager/src/slash/commands/{mod.rs,fallback.rs}`、`crates/codegen/xai-grok-pager/docs/user-guide/26-config-reference.md`；未留存日志 `check-fallback-pager.log`、`check-fallback-shell.log`、`mutate-fallback.sh`）

---

## MT-7：上游同步节奏与仓库卫生

**Owner**：TBD  
**阻断级别**：P3

- [x] 新增门禁 `scripts/ci/cwd-change-census.py`：把「谁可以改整个进程的当前目录、改完谁负责放回去」变成一张带理由的清单加四条失败形状。动机是本轮查明的那个偶发——`gix_discover::is_git` 在函数开头无条件调用 `current_dir()`，进程 cwd 一旦被 unlink，同进程每一次 `gix::open` 都会以 `NotARepository` 失败，于是「A 测试把自己站着的目录删了」表现为同二进制里三十几条无关测试红而 A 自己绿（`docs/verification/fast-worktree-safety-gate-flake-2026-10-03.log` 第 5 节）。运行时抓不到：报错的测试不是闯祸的测试，且单跑永远复现不出来。清单实测 7 个调用点（2 产品：`--cwd` 启动参数、GUI「更改位置」；5 测试：两条 `Drop` 恢复实现 + 三条绑 `CwdGuard`/`RestoreCwd` 的测试）；四条失败形状 = 新调用点不在清单 / 清单里的行漂了 / **测试位置的 chdir 前面没绑 guard（结构规则，不吃清单）** / role·guard 列与实测不符或理由列是空的。角色判定不是数属性行数：`git/safety_tests/gate.rs` 自己一个属性都没有，靠模块图解析 `git/safety.rs` 里的 `#[cfg(test)] #[path = "safety_tests.rs"]` 才判成测试-only。**真实树上 5 个变异逐个红、逐个还原**（G1 抽掉 `auto_gc.rs` 的 guard 绑定（同行数替换，行号不动）→ 报「binds no guard」+ 结构规则两条；G2 把危险站点在清单里声明成 `product` → role 漂移；G3 新加一条无 guard 测试 chdir → 点名新行；G4 理由列改成 `TODO` → 理由不足；G5 反方向：同名函数只写进行注释与块注释 → **不得**多报，清单仍 7 条）；夹具 `test-cwd-change-census.py` 14 条全绿，其中两条是承重的：`test_prose_mention_is_load_bearing` 先断言注释不算、再把其中一处改成真代码断言立刻多一行（否则前半段只是「扫描器什么都没找到」），`test_guard_bound_after_the_call_is_not_a_restore` 把 guard 与 chdir 两行互换（行数行号都不变）仍必须红。**夹具当场抓出扫描器自己的两个真缺陷**：普通字符串的终止符被写成查找 `""`，第一个字符串起把文件后半全抹掉（`auto_gc.rs:898` 明明存在却报 0 个站点）；`#[path = "..."]` 里的文件名被自家抹除器抹掉，`cases.rs` 判成 product——真实仓库当时被「父目录名里有 tests」这条捷径救了一次，是补上「关掉目录名捷径只留模块图」那条测试才暴露的。扫描范围是仓库根全部 `.rs`（跳过 `target/` 等），因为工作区成员含 `crates/` 之外的 `prod/mc/cli-chat-proxy-types` 与四个 `third_party/*`，只从 `crates/` 走等于把 cargo 真编的一部分当不存在。另记一条性能事实：`re.match(p, text[i:])` 每步拷贝剩余文本，480 KB 文件要 9 秒，改 `p.match(text, i)` 后全仓 3 秒——慢到没人跑本身就是一条缺陷。接线双向由 `check-guard-wiring.py` 把关（`ci.yml` 脚本检查步骤 + `verify-in-docker.sh` 的 `gates`），门禁只用标准库，`check-workflow-toolchain.py` 规则数不变。边界写清楚：它不看数据流（自造恢复机制会被误报，方向保守），不查「谁删了目录」，也不证明那 7 个站点当下行为正确；`std::process::exit()` 会绕过 `Drop`，这三条测试路径上没有 exit 是读代码得出的、不是门禁保证的。（2026-10-03；`scripts/ci/cwd-change-census.py`、`scripts/ci/cwd-change-baseline.tsv`、`scripts/ci/test-cwd-change-census.py`、`.github/workflows/ci.yml`、`scripts/verify-in-docker.sh`、`CHANGELOG.md`、`docs/verification/cwd-change-census-2026-10-03.log`）
- [ ] 仓库内**指向不存在文件**的路径引用需要一次定性后再决定是否上门禁。本轮探针（正则提取 repo 相对路径并检查存在性）在文档里报出 32 个不同路径 / 61 处命中，逐个看过之后**故意不做存在性门禁**：其中相当比例是正则误报（如把 `scripts/ci/release-integrity-` 截断、把 `docs/tutorial/01` 这类带序号的写法切错），另有 `.agents/skills/chaos-upstream-sync/references/port-playbook.md` 里**当作示例**的占位路径（`…/src/foo.rs`、`X.Y.Z.md`）与明确记载为「本仓库不存在」的 `docs/internal/*`。要做的是先把这三类（误报 / 示例占位 / 有意记载为缺失）从真缺陷里分出来，再决定门禁的白名单语义；否则门禁第一条规则就得先容纳三种例外，等于没有门禁。（2026-10-03 探针记录）
- [x] 中文化守卫 `scripts/l10n-guard.sh` 的判定粒度：从「按路径判中文消失」改成「按内容判发生了什么」，并给它补上真正会跑的自测。起因是本轮自己的树被判 6 regressed + 2 fortress breach，而 `scripts/verify-in-docker.sh:134` 把它当门禁，交付被自己的守卫挡住。旧实现只比对「哪些文件含汉字」，把三件事压成同一个 FAIL：文件改名（中文跟着搬走）、故意删死代码、以及它真正要抓的「文件还在、中文被换回英文」。六个文件先量存活率（该文件独有的中文行有多少在别处逐行原样出现，语料 = 全仓 4628 条唯一汉字行）再逐个对代码核实：`xai-grok-update/src/signature.rs` 1.00（20/20，拆出的 `xai-grok-signature/src/lib.rs` 里 30 行中文都在）；`views/dashboard/{state,render}_tests.rs` 1.00（6/6、2/2，中文是测试输入 `"中\r\n文"`，原样活在宿主的内联 `mod tests`）；`scrollback/blocks/credit_limit.rs` 0.00 但 HEAD 的 `blocks/mod.rs` **从未声明它**（不编译），额度上限卡片的中文在实际发射处 `app/dispatch/billing.rs::CreditLimitCopy`（「已达到消费上限。」「已达到当前计划的额度上限。」「提高限额」「按量付费」）；`views/usage_detail.rs` 与 `dispatch/tests/usage_partial_failure.rs` 随不可达用量覆盖层删除。**结论是没有一条用户可见文案失去翻译**，是守卫的问题。改法：`before ∖ after` 拆成 `moved`（≥ 90 % 且 ≥ 2 行中文在别处逐行原样出现，由内容判定）/ `removed-recorded`（路径确实不在且登记在 `scripts/ci/l10n-removed-allowlist.tsv`，`<path>\t<理由>`，缺理由直接失败）/ `regressed`（其余），**allowlist 只对「路径真的不在了」生效**——文件还在、中文没了仍无条件 exit 1；fortress 检查跳过已判 moved/recorded 的文件，否则在强保护目录里改名或删死代码永远过不去。守卫自身新增 `scripts/l10n-guard-selftest.py`（13 用例，各自建临时 git 仓库跑真实脚本，同时断言退出码与落在哪一节；只断言 exit 0 的自测对一个什么都不查的守卫也会通过），承重用例是 `clobber_cannot_be_allow_listed`：把被抽掉中文的路径本身登记进 allowlist，仍要求 exit 1 且该路径出现在 `regressed`。三条注入变异各自让对应用例变红（颠倒「先查存在再查登记」→ 2 用例红；去掉改名阈值 → 阈值用例红；关掉失效检查 → 失效用例红），每次按字节 `cmp` 还原。过程中修掉两个自己写出的错：`grep -c -Fxf -f` 组合的 `-f` 把后一个 `-f` 当文件名吃掉、模式文件成了字面量 `-f` 导致匹配数恒为 0（报告里赫然写着「0 of 20 行在别处出现」，靠与独立 python 实测对比才发现）；`is_excluded` 在 `set -u` 下展开空数组，macOS 自带 bash 3.2 会判未绑定，改 `${EXCLUDE[@]+"${EXCLUDE[@]}"}`。登记项的失效口径改为看**工作树**而非 `--after`：`--before HEAD --after HEAD` 曾把每条登记都判成失效（这是给 CI 加步骤、本地先跑时暴露的），而 ref 口径又漏掉「文件在比对范围之外被还原」这种真腐烂。另清掉一类「有守卫却没人跑」的洞：`scripts/check-doc-l10n-selftest.py`（51 用例）此前**没有任何地方调用**（`rg -n selftest .github/workflows/*.yml scripts/*.sh` 零命中），断言就算早已咬不动也照样绿；现在两份自测都进 `ci.yml` 的 `docs-l10n` 作业与 `verify-in-docker.sh` 门禁，CI 另跑一次 `l10n-guard.sh --before HEAD --after HEAD`（遍历真实 393 个中文文件，是 bash 3.2 那类结构会落脚的现场检查）。文档同步：`sync/doc-l10n-conventions.md` 验收口径第 6 条、`chaos-upstream-sync` skill 的报告表（六份输出、登记要求、以及「登记拦不住就地换成英文」这条边界）。同一类噪音在文档检查器里也有一处：把 `check-doc-l10n.py --links` 扩到 CI 从未跑过的
`sync/**/*.md` 报出 8 条死链（`diff` 链接行确认全部早于本轮），其中 7 条是**行内代码里的链接语法**
——约定文档把 `](...)`、`](NN-xxx.md#...)` 当「要盯的写法」列出来，`doc-claims-verification.md:190`
更是**在描述某锚点已死**时引用它；`strip_code` 已让围栏代码块失效，只是没覆盖一个反引号的跨度。
新增 `mask_inline()` 原位抹平行内代码（字符数不变，行列报告与其它模式不受影响），只给
`check_links` 用，`--before/--after` 那几项**故意不抹**（它们正是靠比对行内 span 发现标识符被改）。
自测补一条双向用例（同一目录跑两次：只有示例必须干净，补一条正文真链必须点名失败），总数
**52/52**；删掉 `mask_inline` 调用该用例即红（变异后 `cmp` 还原）。第 8 条是真错：约定文档引用
权威表述那段带着 `[CHAOS.md](../../../../CHAOS.md)`，发行指南实际是五层
`../../../../../CHAOS.md`，这个深度两边都解析不到，已改为 `../CHAOS.md`。`--english` 侧 8 行里
6 行同源——`sync/recon/*.md` 是 `scripts/upstream-recon.sh` 生成的而模板是英文：模板改中文并对
真实 API 重跑生成。**结果**：`sync/**` 由 `--links` 8 → **0**、`--english` 8 → 2（剩下两行是
`doc-claims-verification.md:80/:287` 的「…」内上游原文引用，按约定保留原文不译）；CI 的
`crates/codegen/xai-grok-pager/docs/**/*.md` 与默认 `docs/` glob 仍各 0 条。`sync/**` 有意
**不**并入 CI（维护者笔记、非编译进二进制的文本），但现在扩成一行改动即可，而不是先做一次清理。
**验证**：`python3 scripts/l10n-guard-selftest.py` **13/13**；`check-doc-l10n-selftest.py` **52/52**；真实树 `l10n-guard.sh` 由 6/2 红转为 moved 3 + removed-recorded 3 + regressed 0 + fortress-breach 0 **PASS**，`--before HEAD --after HEAD` 亦 PASS；`check-script-portability.py` OK(23)、`check-workflow-shells.py` OK、`ci.yml` YAML 解析通过；同一轮 `cargo clippy -p xai-grok-pager --all-targets -- -D warnings` 干净（3m14s，0 诊断）、`cargo test -p xai-grok-pager --lib` **9200 passed / 0 failed**。（2026-10-02；`scripts/l10n-guard.sh`、`scripts/ci/l10n-removed-allowlist.tsv`、`scripts/l10n-guard-selftest.py`、`.github/workflows/ci.yml`、`scripts/verify-in-docker.sh`、`sync/doc-l10n-conventions.md`、`.agents/skills/chaos-upstream-sync/SKILL.md`、`scripts/check-doc-l10n.py`、
`scripts/check-doc-l10n-selftest.py`、`scripts/upstream-recon.sh`、
`sync/recon/2026-10-02-2bdd1d6a6.md`；`docs/verification/l10n-guard-classification-2026-10-02.log`）
- [x] 上游侦察已固化为可重复入口：`scripts/upstream-recon.sh` 只读查询 `git ls-remote` 与 GitHub compare API，把 `SOURCE_REV` 与上游 tip 的差距写进 `sync/recon/<date>-<tip>.md`；无网络时退出 1 且**不写记录**（负向验证：伪造 API 返回 404 → exit=1、记录数不变）。2026-10-02 实测：`SOURCE_REV=72a61251f`、上游 tip=`2bdd1d6a6`、`status=ahead ahead=9 behind=0`，记录见 `sync/recon/2026-10-02-2bdd1d6a6.md`；节奏按 `CONTRIBUTING.md` “Upstream reconnaissance” 执行，侦察只登记差距，移植仍需 curated-port 评审。（2026-10-02；`scripts/upstream-recon.sh`、`CONTRIBUTING.md`） **2026-10-03 第二次执行**：上游 tip 未动（`gh api repos/xai-org/grok-build/commits/main` 与 `git rev-parse upstream/main` 都给 `2bdd1d6a6`，committer date 2026-09-29T16:57:09Z），`SOURCE_REV` 仍是上游祖先、`ahead=0 behind=9`，记录见 `sync/recon/2026-10-03-2bdd1d6a6.md`。同轮更正两处测量错误：上一轮会话里记的「上游 tip `7c9459373…`、ahead=1 behind=124」是错的——该 SHA 在对上游完整 fetch 的对象库里不存在（`git cat-file -t 7c9459373` → `Not a valid object name`），而 124 由任何口径都算不出来（`HEAD..upstream/main`=9，`upstream/main..HEAD`=969 是分叉规模不是落后量），这条误记连同口径一起写进了记录；另外本轮发现 `refs/remotes/upstream/main` 停在 `07e35a3d`（比真 tip 落后 3 个提交，因为上一轮那次 fetch 没跑完），所以记录里写明确复现命令并要求先 `git rev-parse upstream/main` 与 `gh api` 对一遍。本轮新增的是移植评估而非新提交：窗口规模 3325 files / +480025 / −173343，其中 290 个文件落在 `l10n-guard.sh` 强保护路径；上游 `xai-grok-update` 新增 `winget.rs`（`winget install --id xAI.GrokBuild`，信任根换成 winget 源，绕开本分叉 `require-sig` 的 ed25519 fail-closed 链，且包 id 是上游品牌）与 `windows_payload.rs`（只比 `.zip.sha256` sidecar，摘要由发送方自算，正是本轮 remote provenance 要消除的形态）；上游同样删了 `views/usage_detail.rs`、`scrollback/blocks/credit_limit.rs`、`app/dispatch/tests/usage_partial_failure.rs`，与本分叉本轮独立得出的死面结论一致；而 `views/dashboard/{render,state}_tests.rs`、`views/shortcuts_help_tests.rs` 这条**第一轮判断是 错的、本轮实测更正**：上游把这三块测试外部化并用 `#[cfg(test)] #[path = "…_tests.rs"] mod tests;` 声明（`render.rs:3168`、`state.rs:4509`、`shortcuts_help.rs:1462`），但本分叉从来没 有这三个文件——同名模块内联在宿主里（`render.rs:4029`/`state.rs:4512`/`shortcuts_help.rs:1400`， `#[test]` 116/246/67 个），`panic-site-census.py --check-uncompiled` 报 0 files、全仓 41 条 `#[path]` 声明零缺失，测试一直在跑；两侧内容已分叉（模块内 `fn` 共有 95/261/75，上游独有 29/30/12），整文件覆盖上游会撞 `E0428` 同名 `mod tests`。这条连同「只搬文件不搬声明 → 一条 测试都不跑」的同类事故一起写进 `port-playbook.md` 的常见失败表，探测器就是 `panic-site-census.py --check-uncompiled`。（2026-10-03；`sync/recon/2026-10-03-2bdd1d6a6.md`、`sync/recon/2026-10-03-upstream-adjudication.md`、.agents/skills/chaos-upstream-sync/references/port-playbook.md）
- [~] 当前 GUI/TUI 修改已在相关切片执行格式、GUI/engine tests 和文档检查；下一轮真实上游同步仍需按规则运行 `scripts/l10n-guard.sh` 前后对照。 **2026-10-03 量化了这个前置条件**：当前窗口（`SOURCE_REV` `72a61251f` → 上游 `2bdd1d6a6`）里 **290 个文件落在中文化强保护路径**（`slash/commands` 28、`views/dashboard` 25、`views/welcome` 9…），合计 3325 files / +480025 / −173343，不是 cherry-pick 的量级，必须单开合并窗口并在移植前拍 `--before` 快照。窗口打开前还需一个决策：上游 `xai-grok-update` 的 winget 更新路径收不收 —— 它按 `winget install --id xAI.GrokBuild` 交给 winget 源，既不是 Chaos 的产物，也不走本分叉的 ed25519 `.sig` 验签，默认建议 skip（评估全文见 `sync/recon/2026-10-03-upstream-adjudication.md`）。（2026-09-24；2026-10-03 量化）
- [~] 维持“分叉层内一律不搬”的判定：`sync/fork-layer-inventory.md` 已登记根 `Cargo.toml` 和 GUI fork 区段；每次继续上游同步仍需执行 l10n/fork-layer review。（2026-09-24）
- [x] `scripts/` 下的 shell 脚本不再依赖 bash 4 / GNU userland，macOS 自带环境可直接跑仓库入口。实测发现三处只在 Linux 开发机上成立的写法：`scripts/l10n-guard.sh` 与 `scripts/ci/secret-scan.sh` 用 `mapfile`（bash 4.0+），`scripts/ci/local-publish-host.sh` 用 `${platform^^}` + `${key//-/_}`（bash 4.0+）；macOS 仍发行 bash 3.2，这三处会以 `mapfile: command not found` 或错误的变量名失败，而 CI 的 ubuntu runner 永远看不见。全部改成 3.2 可用写法（`while IFS= read -r` 读取列表、`tr '[:lower:]' '[:upper:]' | tr - _`），并为 `set -u` 下 bash 3.2 会把空数组 `"${files[@]}"` 判为 unbound 的已知行为加了提前返回。新增 `scripts/ci/check-script-portability.py` 门禁（25 条规则：`mapfile`/`readarray`/`declare -A`/nameref/`${var^^}`/`${var@Q}`/`EPOCHSECONDS`/`globstar`/`wait -n`/`nproc`/`readlink -f`/`realpath`/`date -d`/`stat -c`/GNU `sed -i`/`grep -P`/`grep --include`/`find -printf`/`xargs -r`/`install -D`/`cp --reflink`/`base64 -w`/`timeout`/`tac`/`wc -L`），**故意不设豁免表**——命中就改脚本；并在 `ci.yml` 的 `workflows-present` job 与 `check-workflow-shells.py` 一起执行。`scripts/ci/test-script-portability.py` 对每条规则各注入一次违规并断言退出 1，同时断言干净脚本、注释里提到关键字、`portability-check:ignore` 单行豁免与空目录 fail-closed 四种情形——写不出违规的门禁和仓库已修好长得一模一样，其中两条规则（`sed -i` 的否定预查、`base64 -w0` 的词边界）正是被这组夹具测出漏报后修正的。**验证**：`python3 scripts/ci/test-script-portability.py` 5 tests OK；`check-script-portability.py` 对 15 个脚本 OK；`bash -n` 覆盖 `scripts/*.sh`、`scripts/ci/*.sh`、`scripts/hooks/pre-commit` 全部通过；`secret-scan.sh` 三种模式（全量 3842 files、`--stdin` 单文件、`--stdin` 空输入）行为不变；`l10n-guard.sh --before HEAD --after WORKTREE` 实测 before/after 各 388 个含汉字文件、regressed 0、fortress-breach 0、退出 0；`local-publish-host.sh` 的键名推导对 6 个 platform 逐一与旧 `${var^^}` 结果比对相同；`check-workflow-shells.py` 与 `git diff --check` 干净。（2026-10-02；`scripts/l10n-guard.sh`、`scripts/ci/secret-scan.sh`、`scripts/ci/local-publish-host.sh`、`scripts/ci/check-script-portability.py`、`scripts/ci/test-script-portability.py`、`.github/workflows/ci.yml`、`CONTRIBUTING.md`；未留存日志 `l10n-guard-portable.log`、`test-platform-ps1-portable.log`）
- [x] 根目录六个调试脚本（`capture_listener.py`、`mock_server.py`、`run_mock_server.sh`、`single_mock_server.py`、`test_simple_wb.py`、`test_workbuddy_headers.py`、`test_workbuddy_headers_v2.py`）：**保持原位，不搬也不删**。它们是**上游自己放在仓库根**的（上游提交 `f380bbca`「test(tools): add mock inference server and WorkBuddy header capture scripts」，本地同一提交），不是散落的本地文件；挪进 `scripts/dev/` 会让每次上游同步都在这条路径上冲突，属于"碰架构"。
- [x] 两项延后的上游变更已按当前源码实测复核并给出结论。`oniguruma` 2→3：**本仓库已不存在该依赖**（`Cargo.lock` 14,215 行内无任何 `onig` 包，全仓 `Cargo.toml`/`.rs` 也无引用），因此上游这条升级在此分叉无对应改动，关闭而非“继续延后”。MCP admission 放宽：**维持不移植（决定，非待办）**——分叉已实现并测试受管准入闭环（`crates/codegen/xai-grok-shell/src/session/managed_mcp.rs` 在合并点丢弃命中 `deniedMcpServers`/非空 `allowedMcpServers`/`allowManagedMcpServersOnly` 的服务器并记录 `MCP server blocked by managed settings policy`；`crates/codegen/xai-grok-workspace/src/permission/managed_policy/mcp.rs` 实现正向 allowlist 才放行），放宽会削弱这道 fail-closed 安全闸，且用户指南 `07-mcp-servers.md#被组织策略拦截` 一节已移植；若将来要放宽须另立安全评审。（2026-10-02；`Cargo.lock`、`managed_mcp.rs`、`managed_policy/mcp.rs`）
- [~] Implemented read-only `chaos telemetry status [--json]`; loads effective on-disk config through the policy-aware loader and existing telemetry resolvers, reporting mode/source, trace-upload source, Mixpanel enabled state without token, and external OTEL activation/exporter names without collector URLs or headers. Durable integration tests invoke the built `chaos` binary with isolated config/env and assert JSON fields, config-root/source, human output, external OTEL env activation, omission of both configured and environment-provided endpoints/token/headers, and failure on malformed TOML; CLI parser tests cover human/JSON modes (2 integration + 1 parser test pass, 2026-10-01). `disable`/`enable` remain unimplemented until a reviewed contract for requirements pins, precedence and atomic config edits exists; scope clarification is recorded in ADR-007. (`crates/codegen/xai-grok-pager-bin/src/telemetry_status.rs`, `tests/telemetry_status.rs`; offline targeted Cargo tests)
- [ ] `docs/known-issues/wsl-p9io-crash-20260728.md` 明确为未外发本地存档；Windows 主机/wsl 版本、最小复现和完整 prior-boot logs 缺失。实际补充需 Windows host; 是否上报 external issue 由报告 owner 决策。
- [x] 在贡献文档里固化 WSL/低内存机器的 `CARGO_BUILD_JOBS=4` 建议，避免并发编译耗尽内存/磁盘；注明 `target/` 清理仅限可再生成的 debug/incremental 产物并链接已归档事故。（2026-09-25；`CONTRIBUTING.md` §Low-memory builds）
- [x] TODO 状态文档与 `TODO.md` 的计数首次真正对齐，并第一次变成 CI 门禁。`docs/architecture/todo-open-item-classification.md` 从建立起就写着「改完请重算」，却没有任何检查比对过它：文档声称 51 unchecked / 98 partial / 149 行，组计数写 `M0 0/13`、`M3 8/9`、`M4 24/3`、maintenance 5/14，而同一个文件正文写的是 maintenance 5/13，真实值是 `TOTAL unchecked=23 partial=113 rows=136`；它还把三份未留存日志（`open-items-current.tsv` 等）当证据路径，并沿用已作废的 429/218 ignore 数字。`scripts/ci/classify-open-todos.py` 现在有三种模式：默认逐行清单（行号 / 状态 / 最近标题 / 原文 + `TOTAL`）、`--groups` 里程碑分组（**任何一行不属于任何组就直接失败并列出 `line N: <heading>`**，新增 `### M6.` 段不会被静默排除在全部计数之外）、`--check-doc` 把文档表格与 `TODO.md` 逐格比对并以退出码表态。文档表格、两处过时正文数字（含 429/218 → 当时实测 428 个 `#[ignore]` 属性 / 0 条裸属性；2026-10-03 重扫为 429 / 0，见 MT-5 与 `docs/ignored-audit-2026q4-summary.md` 的「Rescan on 2026-10-03」节）与 scratch 指针全部改掉，逐行导出改为随仓库提交的 `docs/verification/todo-open-items-2026-10-03.tsv`。8 条 fixture：逐行输出与 `TOTAL` 精确匹配、`--groups` 精确匹配、无人认领的标题在两种模式下都被拒绝、真实文档必须通过、把某一组两列对调必须失败并点名该组、缺行 / 重行 / 非数字各自拒绝；**用例里 TODO 侧的数字是现场跑 `--groups` 读出来的，不是从文档抄的**，否则两侧一起错也不会红。7 个变异（`check_doc` 永不报漂移、静默丢弃无主行、容忍重复行、容忍缺行、非数字读成 0、把真实文档改一个数字、从 `GROUP_PREFIXES` 删掉 `## M3.3`）逐个转红并点名对应用例，还原后两份文件 `cmp` 字节一致。接线：`ci.yml` 的 docs-l10n 作业新增「TODO status document matches TODO.md」步骤，`scripts/verify-in-docker.sh` 增加同名 gate。（2026-10-03；`scripts/ci/classify-open-todos.py`、`scripts/ci/test-classify-open-todos.py`、`docs/architecture/todo-open-item-classification.md`、`docs/verification/todo-open-items-2026-10-03.tsv`、`docs/verification/ci-guard-wiring-2026-10-03.log`）
- [x] 新增 `scripts/ci/check-guard-wiring.py`：**`scripts/ci/` 里不允许再存在没人调用的守卫**。本轮实测有三个守卫提交后从未被执行——`classify-open-todos.py`、它的 fixture `test-classify-open-todos.py`、以及 `test-brand-protocol.py`；后者是 brand 守卫的自测，`ci.yml` 只跑被检对象，自测红绿无人知晓。规则是可达性而不是名单：根 = `.github/workflows/*.yml` + `scripts/verify-in-docker.sh`，再沿「已被可达文件点名的 `scripts/` 内文件」做不动点传播（`release-integrity-serve.py` 这类实验室助手因此算可达：CI 跑 `install-integrity-in-docker.sh`，后者启动它）。**散文不算调用**：`#` 行、行尾注释、Python docstring 三种叙述在匹配前一律抹掉——第一版只在非 Python 侧过滤 `#`，结果它自己的 docstring 里点了四个守卫的名字，于是在 `test-brand-protocol.py` 确实没接线的真实仓库上照样报 OK；`install.sh` 的头部注释提到 `test-installer-signature-policy.py` 也是同一类假安慰。文档同样不算调用者（`ignored-tests.sh` 被两份审计报告引用却无人执行，就是这条规则要拒绝的舒适区）。反向也查：workflow 或 `verify-in-docker.sh` 点名的 `scripts/...` 路径必须存在，改名后留下的调用点在门禁这里红，而不是在作业中途以「No such file or directory」收场。白名单必须仍在描述一个存在的文件，防止它烂成退役守卫的墓地。9 条 fixture（真实仓库必须 OK、未接线守卫被点名、经实验室脚本可达算数、`#` 注释不算、docstring 不算、`docs/` 提及不算、悬空调用点被点名、为已删文件保留的豁免被拒、只接在 Docker 入口也算）全绿；7 个变异（把 `#` 注释当调用、去掉传递跳、容忍悬空调用点、容忍豁免腐烂、把 `docs/` 当调用者、把 docstring 当代码、以及在真实仓库把 `test-brand-protocol.py` 的两处接线删掉）全部转红，最后一个正是这个检查存在的理由本身；还原后 `cmp` 字节一致。同轮把 `test-brand-protocol.py` 接进 brand 守卫步骤、两个新检查接进 `ci.yml` 与 `verify-in-docker.sh`。实测覆盖：`scripts/ci/` 32 个文件全部可达，其中 16 个同时被 Docker 入口跑，1 个豁免。（2026-10-03；`scripts/ci/check-guard-wiring.py`、`scripts/ci/test-check-guard-wiring.py`、`.github/workflows/ci.yml`、`scripts/verify-in-docker.sh`、`docs/verification/ci-guard-wiring-2026-10-03.log`）
- [x] `scripts/l10n-guard.sh` 从 fail-open 改成 fail-closed，并修掉它在本仓库 Docker 入口里的接线缺陷。起因：`scripts/verify-in-docker.sh --full` 整轮只有 `docs localization` 一个门禁红，而那一节的输出止于「report dir」——既没有报告也没有报错；同一轮 `cargo test --workspace` 全绿、守卫自测 13/13 通过，所以坏的不是判定逻辑而是守卫的**输入**。根因：`verify-in-docker.sh` 里每个用 git 的门禁都带 `bootstrap` 前缀（`git config --global --add safe.directory /src`），唯独这条没有；仓库以宿主 uid 1003 bind-mount 进容器、容器内进程是 uid 0，git 直接拒绝打开仓库，而旧守卫把列清单的 stderr 吞掉又不看退出码，两侧清单都是空集——「仪器坏了」和「测出了不合格」在日志里长得一模一样。CI 永远看不见这个洞（GitHub runner 的 checkout 归 runner 用户所有）。**修两层**。接线层：`GIT_CONFIG_COUNT=1 / GIT_CONFIG_KEY_0=safe.directory / GIT_CONFIG_VALUE_0=/src` 提到 `run_args` 让所有门禁共用（故意不设 `safe.directory=*`，只信 `/src` 这一个明确路径），`docs localization` 这条也补上与其它 git 门禁相同的 `bootstrap` 前缀（「门禁清单不一致」本身就是这次事故的成因），并在门禁循环之前加 preflight——容器里的 git 读不出 `/src` 就整轮直接退出、说明去查 bind mount 与 `GIT_CONFIG_*`，否则同类接线漂移的表现依旧是「十几个门禁一起红，没人说为什么」。守卫层堵掉四处 fail-open：① 列清单走 `< <(list_rs_files "$ref" | sort -u)` 进程替换，其退出码不进 `set -e`，git 一死该侧就是空集，而空集在本守卫语义里等于「没有文件消失 = 通过」，改成先捕获再判状态、失败即点名是哪一侧；② 空清单曾被当作合法输入（`if [ ${#files[@]} -eq 0 ]; then return; fi`），395 个含汉字文件的仓库任一侧列出 0 个 `.rs` 只可能是仪器坏了，现在直接拒绝；③ 计数用 `… | rg … | wc -l`，管道末端的 `wc` 把 rg 的死活（137）盖成 0，而「文件从一侧消失」恰是本守卫要报的信号，坏仪器于是产出**看起来合理的错判决**——现在分别取 rg 的退出码，>1（rg 自己失败）报错退出，1 保留为「没有匹配」这个真答案；④ `--before` 侧读不出文件内容时同样静默计 0，现在 `git show` 失败即报出路径并终止。另加 `set -E` + ERR trap（死点常在子 shell 里，进程内变量传不回来，所以用一次性 marker 文件 + `trap … EXIT` 清理），任何非预期退出打印 `NO VERDICT -- died at line <N> (exit <rc>) while running: <命令>`；顺带删掉无调用者的死函数 `count_han_in()`（内部正是 ③④ 的写法，留着迟早被复用）。`scripts/l10n-guard-selftest.py` 由 13 例增至 **17 例**，四个新例各把一个 helper 换成敌意替身（PATH 前置 shim），断言「非零退出 + 点名坏在哪 + 不产出报告」：`death_is_announced`（git 一律 exit 137，OOM 的形状）、`killed_counter_is_announced`（rg 通过预检探针、对真实文件 exit 137）、`unreadable_at_ref_is_announced`（只让 `git show` 失败，模拟对象读不出）、`empty_listing_is_refused`（git 成功但列出空集，pathspec/checkout 位置不对）。五处硬化逐一反向改回去做变异证明：M1 注释掉 ERR trap → 三例红（14/17）、M2 忽略清单退出码 → `death_is_announced` 红、M3 删掉 rg 退出码检查 → `killed_counter_is_announced` 红、M4 删掉 `git show` 检查 → `unreadable_at_ref_is_announced` 红、M5 接受空侧 → `empty_listing_is_refused` 红；每次写回原文并 `cmp` 确认字节一致。**反向复现**（改造前 vs 改造后，同一容器同一仓库）：把 `HEAD:scripts/l10n-guard.sh` 抽出来挂进容器原样跑，退出 1、只有两行头、报告目录 8 个产物**全是 0 字节**（不是「测出 0 个问题」，是根本没测）；换改造后的守卫且仍不带 `GIT_CONFIG_*`，第一次把 `fatal: detected dubious ownership in repository at '/src'` + `cannot list .rs files at HEAD -- refusing to compare against a broken side` + `NO VERDICT -- died at line 322` 写进日志；补上 `GIT_CONFIG_*` 之后该门禁在容器里第一次真跑出判定（before/after 各 395 个含汉字文件、`0 broken link(s)`、`0 English prose line(s)`、gate exit=0）。**验证**：`l10n-guard-selftest.py` **17/17**、`check-doc-l10n-selftest.py` 52/52、`check-script-portability.py` OK(23)、`check-workflow-shells.py` OK、`bash -n` 两份脚本通过；真实树 `bash scripts/l10n-guard.sh`（HEAD vs WORKTREE）与 `--before HEAD --after HEAD` 均 PASS 395/395。门禁接线没有新增（`l10n-guard-selftest.py` 早已在 `ci.yml` 的 `docs-l10n` 作业与 `verify-in-docker.sh` 的「localization guard self-tests」里跑）。边界：ERR trap 只把「没有结论」变成有位置、有命令名的一条错误，不改变判定，也不改变通过条件——真正的 fail-closed 是上面那四处。（2026-10-03；`scripts/l10n-guard.sh`、`scripts/l10n-guard-selftest.py`、`scripts/verify-in-docker.sh`、`.agents/skills/chaos-upstream-sync/SKILL.md`、`CHANGELOG.md`、`docs/verification/l10n-guard-fail-closed-2026-10-03.log`）
- [x] Docker 入口目前只镜像了 CI 的一部分：`scripts/ci/` 的 32 个守卫里 16 个同时出现在 `scripts/verify-in-docker.sh` 的 `gates` 数组，其余（版本 lockstep、panic-site 两个棘轮、installer 签名策略、npm 侧若干、GUI 协议漂移等）只在 `ci.yml` 跑。`check-guard-wiring.py` 只保证「有人跑」，不保证两边一致——它会把同时被跑的数量打印出来但不据此失败。需要逐个决定哪些属于本地无凭据也能跑的镜像项、哪些因需要 npm 发布 / 目标 OS runner / 外网而必须留在 CI，并把结论固化成一条门禁或一份明确的不镜像清单。（2026-10-03 提出；`scripts/verify-in-docker.sh`、`.github/workflows/ci.yml`） **2026-10-03 逐条定策完成**：这条现在不是数量问题而是规则问题——`scripts/ci/check-guard-wiring.py` 把每个守卫二选一分类：**被镜像**（`gates` 数组点名它，或入口已经跑的脚本在路径位置点名它，与可达性同一条传递规则），或登记进 `scripts/ci/docker-entry-ci-only.tsv` 并写明它需要的是干净容器没有的什么东西；两边都不在就报红，`--list-mirror` 打印整份分类。镜像项 16 → **26**：新增 version lockstep（`check-versions.sh` + `check-version-lockstep.py`）、panic-site 的两条棘轮连同它的夹具、`test-script-portability.py`（守卫早就在跑、它的自测不在——正是这条规则该抓的形状）、installer 签名策略、以及 npm 侧 `node --check` ×3 + `test-publish-npm.sh`，为此 `docker/verify.Dockerfile` 装上 Debian 的 `nodejs`（发布用的不是这个版本，Dockerfile 注释里写明它只用于解析与假 npm 夹具）。不镜像的只剩 5 条，理由同形：`check-gui-protocol.sh` 要 dev profile 编译 `chaos-engine`（入口的 cargo 门禁止于 check/clippy），`check-powershell-syntax.py --require` 要真 PowerShell（bookworm-slim 装它得引 Microsoft 源），三条 release-integrity 实验室脚本要装配好的发布产物。**两条新规则是被实测和变异逼出来的，不是设计出来的**：其一，可达性一直按**裸文件名**子串匹配，代价在真实树上看得见——本检查自己的 `EXEMPT` 那一行也算“点名”，于是上一轮报的「32/32 可达」里有 1 个是它自己点名自己（同一棵树上跑新旧两套规则实测：32 → 31，丢掉的正是 `ignored-tests.sh`，它本来就该靠豁免而不是靠自点名）；同一类误判也发生在本轮新写的真实树用例上——它断言 `check-gui-protocol.sh`、`check-powershell-syntax.py` 属于 CI-only，而那行断言本身把两者“点名”成了镜像。现在要求名字出现在**路径位置**（前面必须有 `/`），`"$(dirname "$0")/helper.py"` 仍算，真实树随之变成 31 可达 + 1 豁免（那个豁免就是 `ignored-tests.sh`，本来就该靠豁免而不是靠自点名）。其二，`scripts/ci/` 下还有 `.tsv`/`.txt` 数据文件，把数据当调用者会让本检查新加的 ci-only 清单把自己豁免的守卫**全部**标成“入口在跑”——第一版实测把两份规则同时放宽时，5 条 CI-only 守卫全部被标成“入口在跑”（镜像数从 26 涨到 32），只放宽一条也会误判 3 条，检查当场把自己的清单咬了一遍；现在只有 `.py`/`.sh`/`.mjs`/`.yml` 能点名别的脚本，两条规则各自都能挡住这一类，合起来误判数为 0。另外把入口的 `workflow shells` 门禁改成不带参数（与 CI 完全一致）：以前只查 `ci.yml`，而这个检查存在的理由——Windows 矩阵——在 `release.yml` 里。**验证**：`test-check-guard-wiring.py` 16 例全绿（新增 6 例：未镜像且未登记被点名、登记已删除的守卫被点名、登记一个入口其实会跑的守卫被点名、缺理由的行点名行号并拒绝、数据文件点名不算调用、经镜像守卫传递到达算镜像、真实树的分类必须与清单一致）；六个变异各让对应用例转红、还原后 `cmp` 字节一致（不再因镜像缺口失败 / 容忍已删除的行 / 容忍与入口冲突的行 / 跳过缺理由的行 / 把数据文件当调用者 / 退回裸名匹配）；新镜像的每条门禁都在容器里真跑过（version lockstep 7 check 0 failure、portability 23 + 5 tests、workflow shells 两份 OK、installer 8 check + 2 OK、`node -v` v18 下 `node --check` ×3 + `publish-npm guards: OK`、panic-site 97 crates baseline holds + uncompiled 0 files），且 `test-publish-npm.sh` 跑完后 `git status --porcelain` 与跑之前逐字节相同——npm 夹具不会写开发者的树，这一点是这条门禁能进绑挂载入口的前提。边界：镜像说的是“入口能到达这个脚本”，不等于“入口把它的每条分支都跑一遍”——`publish-npm.sh`、`local-publish-host.sh`、`stamp-npm-version.mjs` 就是经 `test-publish-npm.sh` 到达的，其中后两条只在缺二进制的早退分支和 `--version` 分支上被碰到，理由写在清单的文件头注释里。（2026-10-03；`scripts/ci/check-guard-wiring.py`、`scripts/ci/test-check-guard-wiring.py`、`scripts/ci/docker-entry-ci-only.tsv`、`scripts/verify-in-docker.sh`、`docker/verify.Dockerfile`、`CONTRIBUTING.md`、`CHANGELOG.md`、`docs/verification/docker-gate-mirror-2026-10-03.log`）
- [x] `GitGate` 的「解析不出仓库」失效分支是个空转：一次 `invalidate()` 拦不住正在飞的 git walk，那条 walk 的结果会在 invalidate **之后**被当作新读返回给调用方。发现方式不是读代码——是本地 Docker 入口第一次跑 `scripts/verify-in-docker.sh --full` 时红的：前 17 个门禁全绿，最后的 `cargo test` 报 `session::git_gate::tests::invalidate_during_inflight_does_not_return_stale_walk` 失败于 `timed out waiting for walk count 2 (have 1)`（`1933 passed; 1 failed`）。`have 1` 的含义是第二次 `run` 根本没起新 walk，它并进了 invalidate 之前就开始的那一条。**先排除环境问题**：单独跑 `git_gate` 模块 15 次全绿，说明不是这条测试自己的时序，而是同进程里别的测试把它依赖的状态弄坏了——把「不稳定的测试」当输入而不是结论，是这一轮能找到真 bug 的唯一原因。**根因**：`invalidate(root)` 先尝试把路径解析成 git root，解析不出来时走 `None` 分支，其意图（写在 tracing 里：`invalidate-all (root unresolved)`）是保守地作废这个 gate 知道的一切，做法是把 `state.epochs` 里每个 epoch +1。但 `state.epochs` **只有这个函数自己会写**——`decide()` 读 epoch 用 `state.epochs.get(&key.root).copied().unwrap_or(0)`，从不插入。于是对一个从没被 invalidate 过的仓库，`None` 分支遍历的是空 map，什么都没改：epoch 仍是 0，`decide()` 认为在飞的那条 walk epoch 匹配，返回 `Decision::Join`；清快照也救不了，因为快照本来就没写，能被复用的是在飞的那一个。**这条分支在生产里是常规路径而非边角**：`ROOT_CACHE` 是进程级 `static`，`ROOT_CACHE_TTL` 生产取 30 s（`cfg(test)` 下 80 ms），另有 `MAX_ROOT_CACHE = 1024` 满表时 `retain` 过期项、装不下就整表 `clear()`。会话开着仓库、读了一次 git 状态、三十秒后改了工作区再 `invalidate(git_root)`——`session/git.rs:111`、`:2773`、`:3159`、`:3353` 四个调用点走的正是这条路——缓存项已过期，于是 invalidate 空转，恰好在飞的 walk 拦不住。**修法**不引入新机制，用的还是该模块已有的 epoch 比较：`None` 分支在整体 +1 之前，先给**每一个当前有 slot 的 root** `epochs.entry(root).or_insert(0)`，让这次 bump 落到真实存在的在飞请求上；`evict_slots` 本来就保留 `inflight.is_some()` 的 slot，补出来的条目不会被顺手清掉，等待中的 waiter 仍由 `finish_wait` 的 `epoch != current_epoch → WaitEnd::Retry` 处理。`Some(canon)` 分支不动（它自己已经 `or_insert(0)`，行为本来就对）。**回归测试** `invalidate_with_an_unresolved_root_still_supersedes_an_inflight_walk` 不靠 sleep 撞 TTL——那正是上一轮 l10n 事故里「坏仪器看起来像测出了结果」的同族写法：它拿一个本 gate **从没打开过**的第二个临时仓库去 `invalidate`，使 `lookup_cached_root` 结构上必然 miss、`epochs` 结构上必然为空，从而必然走 `None` 分支；断言第二次 `run` 起了新 walk 且交回的是新 walk 的结果（`2` 而非 `1`）。**非空证明**是两个变异，各自写回原文并 `cmp` 确认字节一致：A 删掉补 epoch 的那段（即修复前发布的代码）、B 算出列表但一条都不 apply，两者都让新测试转红，且失败信息与 `--full` 里那条**逐字相同**——`timed out waiting for walk count 2 (have 1)`；这就是这条测试的主要论据，它不是「某个断言变红了」，而是把同一次故障在受控条件下重造了出来。第一次跑变异时容器报 `failed to load source for dependency async-openai`，是我的脚本少挂了 registry/git 两个 named volume，与被检对象无关，补卷后才得到上述结果。**验证**：`cargo fmt --all -- --check` 第一次就抓出我手写的三行换行不合 rustfmt（100 列并成一行），改后 `fmt exit=0`；`cargo clippy -p xai-grok-workspace --all-targets --locked -- -D warnings` 无诊断；整包 `cargo test -p xai-grok-workspace --lib --locked` **连跑 8 次全绿**（`1935 passed; 0 failed` ×8，1935 = 修复前 1934 + 新增 1），`session::git_gate` 模块 16 例（原 15 + 新增 1）全绿；跑完 `git status --porcelain` 只剩本轮两个源文件。边界：没有端到端驱动 `git.rs` 那四个生产调用点，本节证明的是 `GitGate` 单元在「root 解析不出来」时的语义，不是「真实工作区改动后 GUI 看到的 git 状态一定是新」（后者需要贯穿 Engine 的会话级回归）；`MAX_ROOT_CACHE` 撑满触发的整表 `clear()` 与 `store_cached_root` 的 `retain` 分支仍无任何测试驱动（新测试是结构性走到 `None` 分支，不是制造缓存压力）；变异与 8 次连跑只在 Linux/x86_64 容器，macOS 上 `TMPDIR` 经 `/var`→`/private/var`、`dunce::canonicalize` 之后路径形状不同，那条路径走到 `None` 分支的概率只高不低但未实测。（2026-10-03；`crates/codegen/xai-grok-workspace/src/session/git_gate.rs`、`crates/codegen/xai-grok-workspace/src/session/git_gate_tests.rs`、`docs/verification/git-gate-invalidate-all-2026-10-03.log`）
- [x] Web 主机重启后浏览器无法自愈：UI 把协议里「没有活动工作区」的占位 UUID 当成真工作区 id 回传，且 `session_not_found` 之后无人接管，输入框静默失灵。（2026-10-03；`apps/chaos-ui/src/workspace-ui.ts`、`apps/chaos-ui/src/session.ts`、`apps/chaos-ui/src/main.tsx`、`crates/codegen/chaos-engine/src/lib.rs`、`crates/codegen/chaos-engine/tests/workspace_session.rs`、`docs/verification/web-host-reconnect-2026-10-03.log`）根因、修法、7 处变异与边界都记在与本行同名的证据文件里，此处只留三条会被再次问到的事实：（1）占位符是**协议约定**而不是某一侧的笔误——`active_workspace_id` 字段不是 optional，主机无工作区时必须给个值，所以 UI 与 engine 两侧都要翻译它，只改一侧会留下「别的客户端发同一字符串仍然红」；（2）`workspace_unavailable` 与 `workspace_session_mismatch` 两个错误的产生点都在主机侧，因此 engine 那处不是防御性冗余；（3）`submit()` 在缺 session id 时是 early-return，所以「按发送键没反应」不是渲染问题，这类静默失灵比崩溃更难被用户报告，值得在别处也照这个模式排查一次（凡 early-return 于「状态不完整」的入口，问一句有没有东西会让状态永久不完整）。
- [x] CI 的 `docs localization` 作业每天都红，根因是它**需要的工具那个 runner 上根本没有**：`scripts/l10n-guard.sh` 要 `rg`，而 `docs-l10n` 作业从来不装 ripgrep，前置检查命中后 `exit 64`、17 例夹具全红。全仓扫一遍供给：`rust` 作业 apt 装了 `ripgrep`、`platform tests` 下载它的 release 包、`gui`/`gui-browser-e2e`/`npm-scripts` 都有 `actions/setup-node`——**只有 `docs-l10n` 什么都没装**，而它需要的恰好是别的作业为了完全不同的原因顺手装上的那两个之一，所以这个洞在任何一次「照着别的作业抄一段」里都不会被发现。必须写清因果方向：**这个红是上一轮 l10n fail-closed 改造的成果，不是新引入的坏**——改造之前 `rg` 缺失会让列清单的管道吐出空集，而空集在本守卫语义里等于「没有中文丢失 = 通过」，那天的 CI 于是绿着、什么也没测；改造之后它拒绝出结论，作业才红。红是对的，缺的是那个 apt 步骤（已加在 checkout 之后）。为了让这类「作业跑不了自己守的东西」不再靠人记得，新增守卫 `scripts/ci/check-workflow-toolchain.py`：**作业需要的工具，必须在触发它的那个步骤之前装好**，同时接进 `workflows-present` 作业与 `scripts/verify-in-docker.sh` 的 `gates` 数组（`check-guard-wiring.py` 会拒绝任何没人跑的守卫，所以接线不是可选项）。规则只读 `run:` 正文：整行注释与 `#` 尾注在匹配前一律去掉（由 `strip_trailing_comment()` 一个函数负责，它跳过引号内的 `#`，`…/releases/#anchor` 这种 URL 不能被剪断）、步骤 `name:` 不算脚本、`node|npm|npx` 必须出现在行首或 `;`/`&`/`|` 之后、跨步骤比索引且同一脚本内比行号（`apt-get install ripgrep` 写在守卫之后仍然算「跑不了守卫」）、`uses: actions/setup-node` 算 node 的供给、点名的 workflow 不存在或一个都没找到都直接退出 1。工具表**故意只有两条**（ripgrep、node），每条都是已经咬过一口的，没有一条规则来自「看起来像需要」。`scripts/ci/test-check-workflow-toolchain.py` 15 例，第一条就是真实仓库，且它在修 `ci.yml` 之前红而**只红这一条**——其余 14 例全绿说明红的是仓库缺工具，不是新检查算错。**11 处变异，0 存活，0 处未还原**（逐字记录见证据文件 §4.1）；其中 W6/W7/W9 顺带把真实仓库那条弄红，正是「放宽规则会误伤真实 workflow」的直接证据。两轮等价变异揪出守卫自己的两个缺陷，都先于那张全红的表发生：W3 关掉的注释过滤是**死代码**（`strip_trailing_comment()` 对整行注释本来就先在 `#` 处截成空串，那层过滤永远轮不到起作用），处置是删掉而不是补测试；W5 不红是**夹具写错**——那条夹具的步骤名原本少了 `scripts/` 前缀，而触发词是路径 `scripts/l10n-guard.sh`，所以它无论守卫怎么写都绿，补齐触发串后 W5 立刻转红；「步骤 `name:` 里的守卫名不算需求」这句话在写下的时候并没有被验证过。另记一条 CI 语义以免被再次误读：连续 push 后大量 run 显示 `cancelled` 且**一个 job 都没被创建**，不是仓库配置坏了——`cancel-in-progress: false` 只保证正在跑的作业不被掐掉，不保证排队中的作业活着，GitHub 在同组排进新作业时会取消组里已在排队的旧 run。边界见证据文件 §5：工具表只有两条（`protoc`/`pwsh`/`cargo` 尚无「装了 ≠ 装在前面」的事故支撑，加规则的正确时机是它咬到第二次）、只看 job 内部顺序（job 之间不共享文件系统，故 `needs: rust` 不算供给）、不检查「装了但没人用」。（2026-10-03；`.github/workflows/ci.yml`、`scripts/ci/check-workflow-toolchain.py`、`scripts/ci/test-check-workflow-toolchain.py`、`scripts/verify-in-docker.sh`、`docs/verification/workflow-toolchain-2026-10-03.log`）
- [x] Windows 那条腿**通过了 `Parse the PowerShell installers`**，然后停在本仓库自己写的夹具里——上一条修复是对的但不完整。（2026-10-03；`scripts/ci/test-installer-bash-resolution.py`、`docs/verification/platform-ci-2026-10-02.log` 的 2026-10-03 一节）run `37061975515`（head `22683f8c`）的 step 3 现在拿到了可用解释器（`ok executing install.sh's detect_platform with C:\Program Files\Git\bin\bash.exe`、`8 check(s), 0 failure(s)`），说明 `find_bash()` 生效；红的是紧接着跑的 `test_the_rest_of_the_check_still_runs_when_bash_is_found`，原文 `AssertionError: "this host's own uname" not found in`，而它打印的 actual 里明明有一行 `note  this host's uname is a Windows one, so detect_platform refuses, as designed`。**根因**：install.sh 那一腿有两句终点语，由主机决定走哪句（POSIX 主机 `ok this host's own uname picks '<asset>', which is published`；Windows 主机 `note this host's uname is a Windows one, …`，因为 `install.sh` 对 Windows 用户的正确行为就是让他去跑 `install.ps1`），而夹具**无条件断言 POSIX 那一句**——这是一条挂着跨平台名字的 Linux-only 测试，它 12 个同批兄弟在 Windows 上全过，只有它从没在 Linux 之外跑过。为「防止 Windows 再被环境绊倒」而加的那条测试自己成了新的绊倒点，这件事本身要记：**在 Windows runner 上跑过之前，任何「Windows 已解锁」的说法都不成立**。**修法**是承认主机腿的结论不由测试决定：改成要求**恰好报告了两句之一**（`assertHostOutcome`，0 句或 2 句都判失败），并把两句话分别钉住——探针臂调 `run_detect_platform`、主机腿调 `run_shipped`，只 monkeypatch 后者就能在任意主机上选中目标分支而不惊动其它检查。**证据不是推测出来的**：把 CI 上抓到的 Windows 原文喂给新断言，旧断言 `present? False`、新断言接受并选出 Windows 那句；缺句子与被同时选出两句两种情况都被拒（`0 != 1`、`2 != 1`）。**5 处变异，0 存活，0 处未还原**：B1 钉错的 Windows 措辞、B2 让假 bash 永不拒绝（分支不可达）、B3 改守卫侧措辞、B4 把 Windows 拒绝从 note 改成 failure，四条都让新夹具转红——这四条才是「Windows 分支在这里真的被驱动、且两侧任一改词都会在 Linux 上被抓到而不是第五次 push 才在 Windows runner 上爆」的论据；B5 让选择器不再按存在性过滤，新夹具与真实主机那条同时转红。本机 `Ran 14 tests … OK`（原 13 条 + 钉分支那条）。**边界不变**：`cargo test (target-OS crates)` 在 Windows 上仍然一次也没执行过（本次 step 9 依旧是 `skipped`），修好 step 3 只是让 step 9 有机会跑，macOS 已有真实结论、Windows 没有，本行不能靠本文件里的任何记录关闭。
- [x] 新增 `scripts/ci/check-protocol-mirror.py`：浏览器那份协议类型漏了 **6 条引擎一直在说的消息**，而已有检查结构上看不见。`scripts/ci/check-gui-protocol.sh` 把 `apps/chaos-ui/src/generated/protocol.ts` 与 `crates/codegen/chaos-engine/src/protocol_schema.rs` 里手写的 `pub const TYPESCRIPT` 逐字节比对——**两个输入是同一段手写文本**，所以它证明的仅是「文件被重新生成过」，镜像本身漏写了什么它无从得知。漏的正是附件上传：客户端要发 `begin_attachment`/`attachment_chunk`/`cancel_attachment`、主机要回 `attachment_started`/`attachment_progress`/`attachment_cancelled`，浏览器类型里一条都没有，而引擎那侧有一整组协议测试在发这些消息。把镜像退回漏写前那 6 行跑新守卫，得到 `ClientMessage: 29 of 32 / ServerMessage: 37 of 40` 加逐条点名；**同一次运行里旧检查照样打印 `GUI protocol types are up to date`**——这句话在漏了 6 条消息的树上照样打印，就是本守卫存在的理由。新守卫解析真的 Rust 枚举（花括号配平取 body、变体→字段名集合、复刻 heck 的 `rename_all = "snake_case"`，所以 `HTTPStatus` → `http_status`），与镜像里两个 union **双向**比对：缺 tag、多 tag、每条消息缺字段/多字段、重复 tag、带 tag 条目写在 union 之外、两个变体在 snake_case 下塌成同一 tag、union 整个缺失。字段**类型**刻意不在范围内（`UUID` 对 `Uuid` 是另一个量级的翻译问题，且类型不符会在反序列化处当场失败；会静默的只有「有这条消息」与「有这些字段」）。两处解析细节是被夹具逼出来的：字段名只认行首声明**且冒号不能是路径分隔符**（`(?!:)`），因为 rustfmt 会把放不下的泛型换行，`serde_json::Value,` 也是「小写词 + 冒号」开头，早先版本据此凭空造出一个字段 `serde_json` 并报「镜像缺字段 serde_json」；`//` 也要跳过——设计稿里抄一份 `pub enum ClientMessage` 是常见写法，读到注释那份会让守卫在真枚举缺消息时仍然绿。夹具 **26 条**，承重的那组**从真镜像里逐条删掉这 6 条中的每一条**并要求守卫变红且点名那条，证明它读的是 CI 真正检查的文件而不是自己的夹具。**17 个变异体 16 个被抓**，唯一存活的 `field-anywhere-in-line`（`re.match` → `re.search`）是有据可查的等价变异：在「守卫会看到的输入」这个集合上两者行为相同，而它第一轮同样存活时**没有** `(?!:)`，那时换行类型真的会造出幽灵字段——那个真实缺陷现由 `test_a_wrapped_type_line_does_not_become_a_wire_field` 钉住；`comments-not-stripped` 第一轮也存活，补了「注释里抄了一份枚举」这条双向夹具（镜像完好必须绿 + 真 tag 缺失仍要点名）之后被抓。还原全部由 `cmp` 逐字节确认（pristine 与 after 的 sha256 同为 `114f154f4930…f5af`）。接线两个入口都有：`ci.yml` GUI job 的「Protocol mirror coverage」步骤与 `scripts/verify-in-docker.sh` 同名 gate（`check-guard-wiring.py` 会拒绝写了没人跑的守卫，故接线不是可选项）。**边界**：字段类型与嵌套对象的形状不在范围内；镜像里同名 tag 同时出现在收发两侧是合法的，所以比对按 union 分命名空间而不是全局比。（2026-10-03；`scripts/ci/check-protocol-mirror.py`、`scripts/ci/test-check-protocol-mirror.py`、`crates/codegen/chaos-engine/src/protocol_schema.rs`、`apps/chaos-ui/src/generated/protocol.ts`、`.github/workflows/ci.yml`、`scripts/verify-in-docker.sh`、`docs/verification/protocol-mirror-coverage-2026-10-03.log`）
- [x] 新增 `scripts/ci/check-workflow-yaml.py`：**workflow 在 CI 跑之前必须先证明自己是合法 YAML**。起因是本轮到 `.github/workflows/ci.yml` 加拆分步骤时写了 `- name: cargo test (target-OS crates: xai-grok-tools)`——未加引号的 `": "` 是 YAML 的映射分隔符，PyYAML 在第 293 行直接拒绝该文件；而**解析不过的 workflow 一个 job 都不会跑**，比任何一条测试失败都严重。已有的两个 workflow 守卫当时都报 OK，因为它们只用正则抠 `run:` 块，从不问这份文档是不是一个合法的映射。新守卫自己跟踪块标量状态（`run: |` / `>-` 的内容行不检查）、按 YAML 的规则剥注释（引号内的 `#` 不算注释、引号外要求前置空格），再对剩余纯标量里的 `": "` 与结尾 `:` 报错；它**刻意不带引号感知**，因为 `run: echo "a: b"` 在 YAML 里同样是纯标量、同样会被拒。12 条夹具带期望退出码，另有一条夹具把 12 份样本逐个交给真实 `yaml.safe_load` 交叉核对裁决（环境无 PyYAML 时打印说明并跳过），外加块标量终止条件两条；已接入 `.github/workflows/ci.yml` 的 `workflows-present` 与 `scripts/verify-in-docker.sh`，由 `check-guard-wiring.py` 双向锁定。（2026-10-03；`scripts/ci/check-workflow-yaml.py`、`scripts/ci/test-check-workflow-yaml.py`、`.github/workflows/ci.yml`、`scripts/verify-in-docker.sh`、`CHANGELOG.md`）
- [x] `scripts/ci/check-spawn-cwd-portability.py` 补掉两个盲区，其中一个正是它本该拦住本轮 Windows 挂起的那一类。**(1) 只扫 `working_directory` 字段与 `.current_dir()` 调用，看不见 `TaskToolInput` 的 `cwd:` 字段**——而平台腿的挂起就写在 `cwd:` 里；现新增第三个 sink，并按「顶级目录是否只存在于 POSIX 主机」（`tmp`/`var`/`home`/`usr`/`private`/`Users`/`Library` 等一组根名）过滤，因此 `/nonexistent/...`、`/old`、`/new/dir` 这类任何主机都不存在的路径仍是合法夹具，只有把语义绑到某个真实存在的根上才判失败。**(2) 它按步骤名只取到一个 `-p` 列表**，而 `-p` 的正则要求前置行首或续行反斜杠，单行写法的 `cargo test … -p xai-grok-tools` 被静默漏掉——守卫扫 5 个 crate 却报「全绿」。现改为**并集**平台作业里所有名字匹配 `cargo test (target-OS crates` 的步骤（于是本轮的步骤拆分不会把某个 crate 悄悄排除在扫描之外），并在四种情况下 fail closed：没有 `platform-tests:` 作业、没有匹配的步骤、crate 列表为空、以及平台上存在一条 `-p` 的 `cargo test` 步骤而步骤名前缀覆盖不到它（消息直接点名该步骤）。解析器自身踩过的三个坑各有夹具钉住：作业终止条件不能写成 `^  \S`（下一步之间夹着两空格缩进的注释块）、步骤条目缩进是 `steps:` + 2、`-p` 的左边界要允许空格。19 条夹具全绿，含「拆开的两步都会被扫到（并证明那步独有的 crate 真的被扫）」「改名平台作业必须 fail closed」「`cwd:` 写真实目录必须红」「`cwd:` 写任何主机都没有的路径必须绿」与「随仓库发布的树本身必须干净」。（2026-10-03；`scripts/ci/check-spawn-cwd-portability.py`、`scripts/ci/test-check-spawn-cwd-portability.py`、`CHANGELOG.md`、`docs/verification/platform-ci-2026-10-02.log`）
- [x] ignored 盘点重扫并修掉三处过期数字：**428 → 429**（`--require-reasons` 与 `--check-baseline` 两条命令现在都报 429），`docs/ci-test-debt.md` 两处、`docs/architecture/todo-open-item-classification.md` 两处、`docs/ignored-audit-2026q4-summary.md` 的族表与状态行同步。差异是**净增 1 条**：`xai-fast-worktree/src/git/safety_tests/gate.rs` 的 `a_child_that_loses_its_cwd_still_checks_the_worktree`，它属于既有的「根本不是测试：子进程入口」族（该族 7 → 8），ignore 只为把它挡在父进程之外（它会 unlink 进程当前目录，而 libtest 让同二进制的所有测试共享一个进程），**覆盖本身每次都随 `cargo test` 真跑**——父测试用 `--exact --ignored` 重新执行测试二进制并断言 stdout 含 `1 passed`，注册一旦丢失父测试就红。`docs/ignored-audit-2026q4.csv` 由同一次扫描重新生成，标准 `csv` reader 回读为 429 行数据 × 5 列；按 (crate, file, function) 逐行 diff 旧 CSV 才能看清真相：**新增 1、删除 0、仅行号漂移 9**（LSP e2e 2、cgroup OOM 5、blitz fuzz 1、`gate.rs` 2）——直接 diff 文本会把这 9 条误读成实质变化。（2026-10-03；`docs/ignored-audit-2026q4.csv`、`docs/ignored-audit-2026q4-summary.md`、`docs/ci-test-debt.md`、`docs/architecture/todo-open-item-classification.md`）

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

- [~] 发版前重跑第 7 章当前实测状态：2026-10-02 重新实测并推翻了 2026-09-24 记录里的三项。签名：`CHAOS_SIGNING_PRIVATE_KEY`（secret，created 2026-08-14）与 `CHAOS_SIGNING_PUBLIC_KEY`（公开 variable）**均已配置**，`v0.4.2` 六个产物全部带 `.sig` 并被 shipped 验证器逐个接受（`scripts/verify-release-signature.sh --tag v0.4.2 --all`，16 项全绿），「待配置」不再成立。安装器策略：三者已统一为 fail closed；本轮把安装脚本真正跑起来之后又修掉三处真实缺陷——README 头条 `curl|bash` 因只从环境变量取公钥而必然失败（改为内置公钥 + 可覆盖 + 下载前完成前置检查）、`install.sh` 的 `verify_checksum()` 没有调用点（`SHA256SUMS` 比对形同虚设）、以及 `install.ps1` 自 `21f5a186` 起连解析都过不了（`4d3eb266` 修复 + `scripts/ci/check-powershell-syntax.py` 门禁）。干净 Debian 容器按 README 命令安装真实 `v0.4.2` 现为 19 项全绿（`docs/verification/install-sh-linux-2026-10-02.log`）。npm Windows 占位：仍成立且是真实缺陷——`chaos-code-win32-{x64,arm64}` 只有 npm 的 `0.0.1-security`，元包 pin 的 `0.2.110` 从不存在，而 npm 对不可解析的 optional 依赖静默跳过，所以 Windows 上 `npm install` 会成功但 `chaos` 每次执行都失败；现在由 `scripts/npm-install-in-docker.sh` 报出（该脚本 13 项里其余 12 项绿）并由 `release.yml` 的同名发布闸门拦住。仍需 release 负责人执行的只剩带 Windows runner 的 dry-run、npm 名字回收/改名决策、以及 tag 动作。（2026-10-02；`docs/verification/release-signature-v0.4.2-2026-10-02.log`、`docs/verification/npm-install-linux-2026-10-02.log`、`docs/verification/install-sh-linux-2026-10-02.log`、`docs/audit-followup-report.md` 4.1b/4.1c）
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
