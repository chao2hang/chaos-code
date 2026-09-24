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
`CHAOS_WEB_STATE=/path/to/sessions.json` 可启用原子 JSON 会话快照。

当前限制：engine 使用 deterministic demo responder，尚未接入真实 provider；
Desktop host 尚未接入 Tauri；远程 workspace、终端、文件写入和公共 Web 部署
均未启用。
