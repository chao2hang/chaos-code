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

默认服务地址是 `http://127.0.0.1:8787`，开发页面运行在 Vite 默认端口。
`CHAOS_WEB_SQLITE=/path/to/gui.db` 启用 canonical SQLite session store（与
`CHAOS_WORKSPACE_ROOT` 互斥）；`CHAOS_WEB_STATE=/path/to/sessions.json` 保留为
transitional JSON 会话快照。设置
`CHAOS_AGENT_BINARY=/path/to/chaos` 后，Web engine 会通过受控的
`chaos --no-auto-update --output-format json --single PROMPT` 入口接入真实
headless Agent；可用 `CHAOS_AGENT_CWD` 固定工作目录。该进程边界不会把浏览器
请求转换成任意 shell 命令。

当前限制：真实 provider 仍由 headless Agent 的本地配置负责；adapter 是同步进程边界，取消/超时尚需异步 Agent 适配器；
Desktop host 尚未接入 Tauri；远程 workspace、终端、文件写入和公共 Web 部署
均未启用。
