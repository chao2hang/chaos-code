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

生产静态部署可设置 `CHAOS_WEB_ASSETS_DIR=/path/to/vite-dist`，由 Web binary 提供该目录资产并对未知页面路径回退 `index.html`；`/api`、`/health`、`/ws` 始终使用后端受保护路由。静态目录支持 gzip/Brotli 预压缩协商、按资源字节生成 ETag 与条件请求；资产目录仍需显式配置，release pipeline/内嵌 assets 与 CDN 缓存失效策略尚未接线。

## 性能采集

本地可执行 `CHAOS_PERF_REPORT_DIR=/path/to/reports npm run perf:collect` 生成 JSON 性能报告；默认在可用本地端口启动 Vite 和 Playwright Chromium。输出目录为必填项。要让 UI 连接实际 Web Engine，需另行运行 `cargo run -p xai-grok-web` 并设置 `CHAOS_PERF_BASE_URL` 指向经过 Vite 代理的 UI 页面；报告会记录 Web host URL 是否由调用方提供，但不会自动启动 Web host。先运行 `npm run build`。`CHAOS_PERF_SAMPLES` 可设置页面就绪采样数（默认 20，范围 1–100）。

该采集器当前报告页面就绪时间、合成 DOM 添加调度延迟和 Node runner RSS，不是完整 WebSocket/React streaming、Chromium/Web 子进程 RSS、冷启动、10 万文件搜索或大 Diff 基线。它不定义回归阈值，也不上传 CI artifact；不要把本地结果解释为稳定版性能门禁。完整采样范围与限制见 [`../../docs/performance/benchmark-environment.md`](../../docs/performance/benchmark-environment.md)。

当前限制：真实 provider 仍由 headless Agent 的本地配置负责；adapter 是同步进程边界，取消/超时尚需异步 Agent 适配器；
Desktop host 尚未接入 Tauri；远程 workspace 与公共 Web 部署均未启用。文件浏览、搜索、Git、设置和市场面板通过现有 protocol 暴露；文件列表/读取/搜索、审批写入和工具进度/结果可见。配置 `CHAOS_WORKSPACE_ROOT` 时 Web host 装配固定在该 canonical 根目录的 Git/终端 adapter；未配置 workspace 时不会开启这些 adapter。终端命令仍须 Engine 审批并受 Safe Web Mode 限制，输出上限 256 KiB。工作区列表尚未映射为多个独立物理文件根。
