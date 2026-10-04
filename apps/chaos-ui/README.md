# Chaos GUI walking skeleton

本目录是 Chaos Web/Desktop 主线的 React 客户端。当前客户端通过 WebSocket
连接本地 `chaos-web`，支持创建会话、纯文本 Prompt、流式 delta、取消、断线
重连和历史恢复。

## 开发

```sh
cargo run -p xai-grok-web
npm ci
npm run dev
```

默认服务地址是 `http://127.0.0.1:8787`，开发页面固定运行在 `http://127.0.0.1:5174`。Vite 会把 `/api`、`/health` 和 `/ws` 代理到同一 loopback Web host。
`CHAOS_WEB_SQLITE=/path/to/gui.db` 启用 canonical SQLite session store（与
`CHAOS_WORKSPACE_ROOT` 互斥）；`CHAOS_WEB_STATE=/path/to/sessions.json` 保留为
transitional JSON 会话快照。设置
`CHAOS_AGENT_BINARY=/path/to/chaos` 后，Web engine 会通过受控的
`chaos --no-auto-update --output-format json --single PROMPT` 入口接入真实
headless Agent；可用 `CHAOS_AGENT_CWD` 固定工作目录。该进程边界不会把浏览器
请求转换成任意 shell 命令。

未配置 `CHAOS_AGENT_BINARY` 时，可改用任意 OpenAI 兼容推理端点，凭据只从
Web host 进程环境读取，浏览器协议里不出现 API Key：

```bash
CHAOS_PROVIDER_BASE_URL=http://127.0.0.1:8080/v1 \
CHAOS_PROVIDER_MODEL=qwen2.5-7b-instruct \
CHAOS_PROVIDER_API_KEY=sk-... \
cargo run -p xai-grok-web
```

`CHAOS_PROVIDER_BASE_URL` 必须以 `/v1` 之类的版本路径结尾，adapter 在其上拼接
`/chat/completions` 与 `/models`；`http:` 明文只允许 loopback（`127.0.0.1`、
`::1`、`localhost` 及 `*.localhost`），其余主机必须使用 `https:`，URL 里带
`user:pass@`、query 或 fragment 会被拒绝。`CHAOS_PROVIDER_API_KEY` 可省略，
省略时不发送 `Authorization` 头。启动时 host 会请求 `/models` 并打印一行结果
（`Provider ... ready: N model(s) listed, configured model <slug> is listed`
或 `not reachable: <原因>`），配置无效则直接以非零码退出、不监听端口。TLS 校验
走仓库统一的 rustls 策略，因此企业代理或私有 CA 可用 `GROK_EXTRA_CA_BUNDLE`
追加根证书。401/403/404/429/5xx、连接失败、超时、坏帧、端内错误帧和空响应都会
转成 `agent_failed` 错误事件并附带可操作中文提示，日志与错误文本里的 Key 一律
替换为 `[redacted]`。GUI 配置表单与操作系统钥匙串存储仍未实现，因此这条路径
目前只由服务端环境变量配置。

生产静态部署可设置 `CHAOS_WEB_ASSETS_DIR=/path/to/vite-dist`，由 Web binary 提供该目录资产并对未知页面路径回退 `index.html`；`/api`、`/health`、`/ws` 始终使用后端受保护路由。静态目录支持 gzip/Brotli 预压缩协商、按资源字节生成 ETag 与条件请求；资产目录仍需显式配置，release pipeline/内嵌 assets 与 CDN 缓存失效策略尚未接线。

## 浏览器端到端测试

```sh
cargo build --locked -p xai-grok-web --bin chaos-web
npm ci
npx playwright install --with-deps chromium
npm run build
npm run test:e2e
```

`npm run build` 是必需的：`static-host.pw.ts`、`reconnect-snapshot.pw.ts` 与
`commit-message-safe-mode.pw.ts` 三份规格自己拉起宿主，用
`CHAOS_WEB_ASSETS_DIR` 指向 `dist`，所以它们量的是构建产物而不是 Vite dev server。
`npm run test:e2e` 依次跑两份 Playwright 配置，端口由 `e2e-runner.mjs` 现场分配，
所以一条命令就是全量。同时只跑一条：两份配置共用同一份 git 工作区与端点的 prompt
日志，端口各自动态分配，文件系统不会，两个 runner 并发时各自读到的暂存区与日志都不
属于自己那一轮。两份配置分开是因为宿主的环境变量是进程级的：

- `playwright.config.ts`：不带 Provider 启动宿主，走内置的演示应答，绝大多数
  规格（对话、附件、审批、差异回滚、手机端外壳、可达性扫描）都在这里。
- `playwright.git.config.ts`：提交信息那两份规格专用。它额外拉起
  `e2e/support/mock-provider.mjs`（自建的 OpenAI 兼容端点，SSE 增量、要求
  Bearer Key、把收到的 prompt 逐条落盘供规格断言），并用
  `CHAOS_WORKSPACE_ROOT` + `CHAOS_PROVIDER_*` 启动宿主。若把这两个变量加进第一
  份配置，其余规格断言的演示应答就没了。`commit-message.pw.ts` 在这里的宿主上跑
  通建议与提交；`commit-message-safe-mode.pw.ts` 另用一个临时端口自己拉起
  `CHAOS_SAFE_WEB_MODE=1` 的宿主（模式是进程级环境变量，两份规格没法共用同一个
  进程），逐个点被拒的控件，断言每个面板都说明被拒并且不再显示「处理中」。

两份规格共用 `e2e/support/commit-form.ts` 准备的仓库与控件定位，也共用
`.chaos/e2e-git-workspace/`：每次运行都在里面重建一个真实 git 仓库
（一次 commit + 一处已暂存改动），提交动作真的会写 `git log`，端点收到的 prompt
写在 `.chaos/e2e-provider/prompts.jsonl`；两者都不进版本库。可用
`CHAOS_E2E_GIT_WORKSPACE`、`CHAOS_E2E_PROVIDER_STATE_DIR`、
`CHAOS_E2E_BACKEND_PORT`、`CHAOS_E2E_UI_PORT`、`CHAOS_E2E_PROVIDER_PORT` 覆盖位置与
端口；不设置时端口自动分配。只想跑这一组时可显式指定配置：
`npx playwright test --config playwright.git.config.ts`。

新增一份 `*.pw.ts` 必须同时改两处手写清单，否则它一次也不会跑，而 `npm run test:e2e` 照旧全绿：
Playwright 只收集 `testMatch` 匹配上的文件，匹配不到的那些它不说、也不报错，退出码仍是 0。
`playwright.config.ts` 尤其容易踩空——它在顶层写了一份 `testMatch`，三个 project 又各自重写一份，而
project 的 `testMatch` 是**替换**顶层而不是收窄它：只往顶层加名字，等于什么都没登记，那条 pattern 仍
然匹配其余十二份规格，看上去毫无异样。这条规则由 `scripts/ci/check-e2e-registration.py` 钉住（宿主与
容器两条门禁里都有它）：它按 Playwright 的方式读这两份配置，逐个规格问「谁会收集我」，并拒收没人认领
的规格、所有 project 都替换掉的顶层名单、收集不到任何东西的 project、已经指向不存在文件的旧名字、
runner 没点名的配置，以及不再调用 `e2e-runner.mjs` 的 `test:e2e`。它读不出来的东西一律算失败而不是跳
过（例如用 glob 字符串或用 `process.env` 现拼出来的 `testMatch`），因为这个门禁存在的理由正是怀疑一
次绿色的运行。 `python3 scripts/ci/check-e2e-registration.py --list` 会打印现在这张对照表，一行一份
规格，后面列出是哪个 project 收集了它。

## 性能采集

本地可执行 `CHAOS_PERF_REPORT_DIR=/path/to/reports npm run perf:collect` 生成 JSON 性能报告；默认在可用本地端口启动 Vite 和 Playwright Chromium。输出目录为必填项。要让 UI 连接实际 Web Engine，需另行运行 `cargo run -p xai-grok-web` 并设置 `CHAOS_PERF_BASE_URL` 指向经过 Vite 代理的 UI 页面；报告会记录 Web host URL 是否由调用方提供，但不会自动启动 Web host。先运行 `npm run build`。`CHAOS_PERF_SAMPLES` 可设置页面就绪采样数（默认 20，范围 1–100）。

该采集器当前报告页面就绪时间、合成 DOM 添加调度延迟和 Node runner RSS，不是完整 WebSocket/React streaming、Chromium/Web 子进程 RSS、冷启动、10 万文件搜索或大 Diff 基线。它不定义回归阈值，也不上传 CI artifact；不要把本地结果解释为稳定版性能门禁。完整采样范围与限制见 [`../../docs/performance/benchmark-environment.md`](../../docs/performance/benchmark-environment.md)。

当前限制：provider 由 Web host 进程环境配置（headless Agent 或 OpenAI 兼容端点二选一），GUI 内没有配置表单，凭据也未接入系统钥匙串；headless adapter 是同步进程边界，取消/超时尚需异步 Agent 适配器；
Desktop host 尚未接入 Tauri；远程 workspace 与公共 Web 部署均未启用。文件浏览、搜索、Git、设置和市场面板通过现有 protocol 暴露；文件列表/读取/搜索、审批写入和工具进度/结果可见。配置 `CHAOS_WORKSPACE_ROOT` 时 Web host 装配固定在该 canonical 根目录的 Git/终端 adapter；未配置 workspace 时不会开启这些 adapter。终端命令仍须 Engine 审批并受 Safe Web Mode 限制，输出上限 256 KiB。工作区列表尚未映射为多个独立物理文件根。
