# TODO 全量完成路线

> 日期：2026-09-29  
> 目的：把 [`TODO.md`](../../TODO.md) 的开放工作分解成可执行阶段，并说明哪些可以由仓库内开发完成，哪些必须等负责人决策或外部环境。本文是**排程与依赖指南**，不是第二份勾选清单；状态只在 `TODO.md` 更新。

## 完成目标与现实边界

目标是让 `TODO.md` 中所有仍被接受的工作达到其自身 Definition of Done，或经授权明确标为延期/不适用；不能靠把部分完成改成完成、降低验收标准、假装外部测试已运行来达到“全绿”。截至 2026-10-02，`TODO.md` 分类器输出 52 条未完成、98 条部分完成状态行（分类脚本逐次重算；行数不是功能数）。Q4 ignore inventory 本日刷新为 429 条（218 条无 reason）；其中五个 xai-grok-update 的上游 URL 假设改为 Chaos 安装器实际行为测试，当前该 crate 只剩 opt-in stress ignore。此源码修正不替代 owner/reviewer 按源文件逐条复核。

下文的阶段顺序是依赖顺序，不是工期承诺。每个阶段开始前，应在 TODO 模板中填入具体 Owner、Reviewer、目标日期、issue/ADR、预算和运行环境。缺这些信息的阶段只做准备/小型无风险修复，不做产品或安全策略猜测。

## 统一执行规则

1. **先确认要交付什么。** 查清 TODO 中剩余条件、当前代码和最近证据，防止重做已完成内容。一个 PR/提交只关闭经测试证实的范围。
2. **把真实产品路线与维护路线分开。** 新 GUI/Web 功能不能抢占 TUI 的 P0/P1 安全、发布和数据完整性修复。
3. **先做架构与信任决策，再写执行代码。** 多根目录、SSH、Provider 凭据、MCP/插件来源信任、远程 Agent、桌面支持矩阵、npm 包名和发版策略都需要对应 Owner/安全 Reviewer 签字。
4. **每个可操作路径都要验收。** Rust handler/adapter 测试；生成协议漂移检查；前端单测、类型检查、构建；真实 WebSocket/HTTP 或 Playwright 用户流；桌面能力则在真实 OS/WebView runner；性能项跑固定基线；负向测试要证明 fail-closed。
5. **外部门禁要有 Owner 和证据。** 负责人须提供测试机、临时凭据、服务端点、签名资源或 CI telemetry 权限；证据记录运行 ID/平台/结果。准备 fixture 不等于真实 endpoint，Linux Web 测试不等于 Tauri/macOS/Windows 验收。
6. **可延期项目必须明确处理。** 由产品/安全 Owner 批准后，将范围改为 Deferred/Unsupported，写明原因、影响、重新评估日期并更新兼容矩阵；不得为了归零删除仍被承诺的交付目标。
7. **每阶段收尾重算。** `python3 scripts/ci/classify-open-todos.py`、`git diff --check`、相关包检查；更新 TODO 证据和本文阶段状态说明。分类数字只作快照，禁止以行数差直接代表进度。

## 阶段 0：立项与决策冻结

**范围**：M-1 未决项；M4/M5 的支撑决策；维护线 Owner 和当前发版阻断。

**具体工作**：

- 指派 GUI 产品 Owner、Rust/协议 Owner、安全 Reviewer、TUI 发布 Owner；对 M-1/M0/M1～M5、MT-1～MT-7 分别确认负责人、目标日期和 issue。
- 完成产品功能矩阵：Web、桌面、远程、交互 PTY、独立 workspace roots、Provider、MCP/插件/工作流、更新分别是首发、降级、延期还是不支持。
- 安全/架构决策至少覆盖：Tauri v2 与 OS 矩阵；密钥方案；Provider 合同测试策略；扩展来源、签名和批准策略；远程 Agent/工具拓扑、认证、RPC server 所有权；项目 workspace root 管理与信任来源；部署 TLS/反向代理边界；TUI↔GUI 数据迁移；版本锁步/独立发布。
- 维护线先检查 MT-1 npm Windows 占位名、MT-2 真正签名 dry run、P0/P1；不绕过 npm owner 或签名安全门禁发版。
- 完成季度测试债和上游延期项的责任分配；2026-10 ignore review 需在到期周期逐项签字；本文件更新日 2026-10-01 已到期，库存刷新不等于逐项负责人审批。

**通过条件**：有已批准的产品矩阵和 ADR/decision record；每个待办有 Owner 与接受状态；外部资源有负责人和预计可用日期；所有明确 Deferred/Unsupported 项都有理由与复审日。没有批准时，后续阶段不得实现依赖该决策的危险路径。

## 阶段 1：基础运行闭环与安全底座（M0）

**范围**：协议、共享 Engine、Web skeleton、Desktop host seam、鉴权与安全运行模式。

**顺序**：

1. 复核并冻结共享协议 envelope、确认/错误/去重/恢复状态机和生成类型流程。
2. 完成 headless Agent adapter 的安全/兼容评审；只有评审后才决定是否物理迁移 ACP glue。
3. 保持 Web loopback/Host/Origin/token/Safe Web Mode 默认 fail-closed；定义 token 生成/轮换、反向代理和生产部署要求。
4. 根据决策接入 Tauri executable、WebView IPC/transport；若首期延期桌面，则更新矩阵，不将 Engine library test 称桌面交付。
5. 补全 Web/桌面的启动、会话创建、发送、stream、cancel、断线恢复、设置读取的真实入口测试。

**通过条件**：协议代码生成无漂移；认证/Origin/Host/mutation 正负例通过；Web真实 Chromium 与 Engine 连通；Desktop 若在范围内，真实 executable/WebView 覆盖重启、cancel、stream/reconnect；Linux/macOS/Windows 按冻结矩阵通过。

## 阶段 2：核心对话与批准操作（M1）

**范围**：时间线、session projection、工具/问题/审批、审计、Diff。

**顺序**：

1. 先闭合现有真实 ToolAdapter 事件的 UI 投影，验证 activity bounds、关联 session 和重复/乱序事件行为。
2. 定义 turn grouping、长会话容量、虚拟列表、滚动锚点和行高测量合同；再实现性能结构，不先猜阈值。
3. 定义 reasoning 内容的来源/权限后再展示；禁止把模拟文案当真实推理内容。
4. 补审批超时、撤销、重连、跨标签竞争和恢复；破坏性操作必须显示后端第二次确认条件，后端保持唯一权威。
5. 接入生产 Diff/hunk adapter 前补外部文件变化、部分 hunk、回滚失败、二进制场景；测试磁盘事实与 UI 状态一致。

**通过条件**：实际生产 adapter（或明确标识的 contract adapter）通过批准/拒绝/取消/超时/重复/重连；长对话在冻结数据集达到 p50/p95 指标；审批/Diff UI 操作后重新读磁盘验证结果；Web/桌面及支持的视口/OS 实测。

## 阶段 3：本地工作台和数据（M2）

**范围**：多个 workspace、文件树/搜索/写入、终端、Git、附件、SQLite/TUI 导入。

**顺序**：

1. Owner 冻结 workspace registry 与物理根映射策略：根由 Host 配置/选择，浏览器不得提交任意物理路径；定义新 workspace 创建、导入、归档、路径变更的授权和清理规则。
2. 建立 host-owned `workspace_id → canonical root` registry；所有文件、附件、Git、终端、Diff 与审计请求共用映射。双根隔离要有同名文件、越界 symlink、归档 session、附件和 Git 仓库对照测试。
3. 完成文件 UX：文件类型与错误状态、搜索、`@` 候选、写入审批/冲突、Diff；完成实际 Engine→UI→磁盘用户流。
4. 若承诺交互 PTY，先实现 stdin/resize/cancel/断线/子进程所有权，复用审批与 Safe Web Mode；否则在 UI/能力矩阵显式说明不支持。
5. 完成 Git 非破坏与破坏操作确认、失败保留 staged state、差异展示/恢复路径。
6. 完成 SQLite fault suite：并发进程、busy contention、磁盘满、迁移中断/恢复；实现批准范围的 TUI ACP session 完整转换和回滚（或经 Owner 延期）。

**通过条件**：两个以上临时根实际运行并证明 files/Git/attachment/terminal 请求只落在授权根；无 browser-supplied path；SQLite 故障恢复无数据损坏；附件失败清理/取消/批准均验证；桌面/Web 路径按矩阵通过。

## 阶段 4：设置、Provider 与扩展（M3）

**前置条件**：密钥存储、Provider 合同、扩展信任/签名、权限 UX 的 ADR 获批，并提供可撤销临时测试凭据或 Owner 批准的合同端点。

**顺序**：

1. 先接 OS keyring/secret store，不将 API Key 回传 UI/log/audit；测试缺失、拒绝访问、轮换、恢复和清理。
2. 做设置 schema、字段校验、非法保存不覆盖旧值、导入/备份/迁移；一般设置、外观、模型、Provider、权限、安全、快捷键、远程和更新按批准范围交付。
3. 实际跑 OpenAI-compatible Provider 的连接、模型选择、请求、新会话、重启恢复、401/429/5xx/timeout/cancel/网络错误脱敏。
4. 依据批准的信任/签名策略接 MCP、插件、skills；覆盖扫描、批准安装、执行、撤销/禁用、来源变化和恶意包拒绝。
5. 等工作流/subagent Engine 有真实 lifecycle event contract 后实现状态、产物、通知、取消、部分失败、父取消、超时及应用重启恢复；不以 prompt mock 代替。
6. 完成本地化、长文案、屏幕阅读器及各支持平台 GUI 无障碍验收。

**通过条件**：真实但可撤销 Provider/MCP 合同闭环；secret 不出现在协议、日志、审计、错误中；未批准或撤销扩展不可运行；workflow UI 只投影真实状态；所有支持平台辅助技术/键盘验收通过。

## 阶段 5：远程开发（M4，条件启动）

**不得先于阶段 0 的远程 ADR/威胁模型开工。** 需要 approved auth、host-key 策略、RPC/server owner、server artifact 来源、支持系统矩阵和受控远程 runner。

**顺序**：

1. 选定且审查 SSH library/auth；明确是否支持 ProxyCommand，默认禁止未审计执行通道；添加凭据不进入日志/remote payload 的录制端点测试。
2. 实现版本协商、签名 server 部署、loopback/stdio/Unix socket 绑定、一次性握手、replay 防护和回滚。
3. 按批准策略实现连接状态、heartbeat、backoff、cancel、重连、凭据失效；在真实受控主机故障注入。
4. 把文件、Range、搜索、写入审批、Git、Diff、附件与 Agent 请求挂在同一已认证远程 workspace session；路径类型防止远程路径进入本机 API/deep link。
5. 端口转发独立明确 local vs remote forwarding，处理冲突、随机本地端口、WebSocket、Host/Origin/Cookie/auth 和 session teardown。
6. WSL 先做 capability/transport spike；Docker/Podman 只有单独 Owner/威胁模型/runner 获批才加入。

**通过条件**：由独立受控 Linux host 完成安装/升级/回滚/文件/Git/Diff/审批流程；错误 host key、错误凭据、断线、磁盘满、服务器升级失败均拒绝并恢复；本地端口/子进程/临时凭据在结束后清理；Unsupported PTY/detached Agent 明确显示不支持。

**外部门禁**：此阶段当前需要 Owner 提供测试机、临时账号/密钥、允许的 SSH 策略、签名 server source 与部署权限。没有这些输入时，只能做不触网的协议/模型单测，不能勾远程验收。

## 阶段 6：发布工程、跨平台和性能（M5 + MT）

**并行线 A：GUI/Web release**

1. 先确定 CLI/Web/Desktop 版本关系和支持平台矩阵。
2. Web host 加载构建产物、SPA fallback、压缩、ETag/cache invalidation；保留 API/WebSocket routes 与安全边界。
3. Linux 安装格式、Wayland/X11、WebKitGTK；Windows WebView2/MSI/Authenticode；macOS arm64/x64/Universal、entitlements、公证、Gatekeeper 和升级/卸载/回滚各自独立通过。
4. 更新 feed 包含 hash、签名、版本索引；模拟下载截断、篡改、错误签名、迁移失败和回滚。
5. 建立机器/OS/browser/WebView/release build/data set 固定的 benchmark，测冷/热启动、RSS、长对话、150 tps、十万文件搜索、大 Diff；先采基线再定 p50/p95 和回归阈值。
6. 发布前生成 SBOM、license/vulnerability audit、THIRD-PARTY-NOTICES、checksums、用户指南/故障/安全说明；召开 go/no-go，P0/P1 清零或获批准豁免。

**并行线 B：现有 TUI 维护**

1. MT-1 在下一次发行前由包所有者处理 Windows npm placeholder/reclaim 或批准包名迁移并在干净系统实装验证。
2. MT-2 由 release owner 在受控 secrets/Windows runner 做正式签名 dry-run 和安装器验证。
3. MT-5 在 2026-10 到期时逐条复核 ignored tests，决定恢复/删除/带 owner 日期续期。
4. MT-6 先做 `xai-grok-sandbox`、`xai-tty-utils` P0 unsafe 审计，再按小批次审 `xai-grok-shell` unsafe/unwrap；每批 AST/审查、真实测试、Clippy、fmt 和计数重算。
5. MT-7 维护 upstream read-only reconnaissance 节奏；`oniguruma`/MCP admission 和 telemetry 命令由对应 owner/security reviewer 决策；WSL crash 是否对外报告由 issue owner 负责。
6. 每次 TUI 用户可见改动使用 `scripts/l10n-guard.sh` 前后比较；代码文案 sweep 先穷举真实输出点，再分类，避免改历史/协议/兼容标识。

**本地执行记录（2026-09-29 至 2026-10-01）**：在外部决策到位前，继续完成了下列本地安全切片：Web 的固定 workspace root Git/受限一次性终端 adapter 装配与浏览器批准流程；文件写入 approval/reject/re-read；键盘可操作的 `/`/`@` 候选；Provider 非法 URL 浏览器拒绝；对动态测试 Origin 收紧到 debug build + 显式启用 + loopback HTTP；修正 approval-resolved 事件不覆盖后续 Git/终端实际结果的 UI 状态顺序；静态 Web router 可选托管构建 assets 并覆盖 SPA fallback，独立 Playwright test 用 Vite build→启动真实 binary→访问页面/assets/SPA/API/health/WS 并创建 session；静态资源压缩协商、ETag/If-None-Match 与缓存策略已在真实 handler/browser 路径验证，CDN invalidation 和发布接线仍待后续。Q4 ignored inventory 于 2026-10-01 刷新为 429 项、218 项既有裸属性、baseline 零漂移；五条过时 updater 安装 URL ignored 测试已替换成真实 fork installer 行为测试，updater stress 测试有仓库脚本入口且 120 次循环已通过；默认 100k run 于 2026-10-01 通过仓库脚本完成（1 passed、0 failed，4306.69 秒；goal 私有 scratch `blitz-stress-100k.log`），但 owner/reviewer 逐项审查仍未完成。M5.4 已补记本机 Linux/Chromium 的可复用 benchmark 环境和报告协议（`docs/performance/benchmark-environment.md`），本地 Vite-only collector 的真实浏览器 smoke 通过并验证了机器可读报告/限制说明；尚无所需的 Engine streaming/文件搜索/大 Diff 指标、性能基线、稳定 runner 或性能门禁。相关测试与全 workspace 验证日志留存于 goal 私有 scratch。只读 `chaos telemetry status [--json]` 已在真实二进制路径实现和实测；config-writing telemetry disable/enable 未纳入。正式 Provider/MCP/SSH、Tauri/OS package、签名/npm owner、multi-root、CI telemetry 与 release/performance owner 决策仍未提供，因此相关待办继续开放。

**通过条件**：每个发布格式在其 OS runner 安装/启动/更新/失败/回滚/卸载；签名资产实证；性能门禁可复现且 artifact 保存；TUI/npm/签名硬门禁解决或按产品决策不再承诺；全部 MT 行完成、有期续期或获批准延期。

## 阶段 7：总验收与清单关闭

1. 重新运行整个 TODO 分类器，逐行复核 62/90 等开放记录，不继承过期计数。
2. 按冻结支持矩阵执行真实用户路径清单：安装→首次启动→设置→Provider/会话→文件/Git/工具审批/Diff→断线恢复→升级/失败回滚→卸载/数据保留。
3. 运行全 workspace `fmt/check/clippy/test`、协议 drift、前端 test/typecheck/build、desktop/Web E2E、lint/localization/brand/secret/ignored/SBOM/signature/performance gates。
4. Owner 对所有 Deferred/Unsupported 做显式产品确认；残留只允许有 Owner、理由、风险、复审日期的延期项。发布 go/no-go 在证据归档后进行。
5. 完成后更新 TODO 顶部状态日期、分类报告、兼容矩阵、README/user guide/known issues 和 release notes；每条完成都有 PR/commit 和可重跑证据。

## 剩余外部门禁

首个稳定版范围和首版明确不支持项已由 [ADR-007](adr-007-first-stable-product-scope.md) 记录；远程边界见 [ADR-004](adr-004-remote-topology.md)。这只冻结产品范围，不把接受的 Tauri、Provider、MCP/workflow、发布和维护工作改成已完成或自动延期。

以下事项仍需有权限的维护者/安全 reviewer 或真实环境，执行者不能代替：

1. **Owner 与独立审核**：TUI/release owner、独立安全 reviewer、telemetry/issue/package owners 对应的正式审批。
2. **安全决策**：SSH/auth/ProxyCommand、Provider/keyring、MCP/插件来源和签名、workspace roots 的正式合同；没有批准前危险路径继续 fail-closed。
3. **测试资源**：Windows/macOS runner、签名证书/发布密钥（由 Owner 在 GitHub secrets 管理，不在聊天传递）、可撤销 Provider/MCP 测试凭据/端点、受控 SSH host、CI telemetry 权限。
4. **发行决策**：CLI/npm 包是否必须保持当前名字；若 Windows npm 名称不能取回，是否批准版本化 rename/pin migration；发行版本号与 go/no-go owner。

这些资源不是本地实现的借口：不依赖它们的前端、Engine、Rust 维护任务继续实施；依赖项保持开放并说明具体证据要求，不得伪报通过或因为缺 owner 而擅自解除安全门禁。
