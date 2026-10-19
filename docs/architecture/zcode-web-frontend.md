# ZCode Web 前端与 Chaos 宿主

- 状态：当前 Web 产品路径（2026-10-17）
- 范围：说明本项目管理的 ZCode Web 前端源码 checkout 与 Rust Web 宿主之间的构建边界。

## 当前架构

Chaos Web 使用 **由本项目管理的 ZCode 官方 Web 前端**。旧的自研 `apps/chaos-ui` 已于 2026-10-09 退役并从工作树移除；它不是当前 UI，也没有对应的本仓库前端构建入口。

| 组件 | 位置 | 职责 |
|---|---|---|
| ZCode Web 客户端 | 本项目管理的 ZCode 前端源码 checkout（与 `chaos-code` Cargo workspace 分开） | React/TypeScript UI；构建为静态 Web 资源 |
| Chaos Web 宿主 | `crates/codegen/xai-grok-web`，二进制名 `chaos-web` | 登录与鉴权、静态资源托管、`/ws/zcode` RPC 接入、V4 协议桥接到 Chaos Engine |
| 引擎与 Agent | `crates/codegen/chaos-engine`、`xai-grok-pager-bin`（二进制名 `chaos`） | 会话状态、Provider/Agent 调用、流式事件与工具能力 |
| 部署资源 | 宿主配置的 `CHAOS_WEB_ASSETS_DIR` | ZCode 构建产物；不含 ZCode 源码，也不是旧 Chaos UI |

**不要因为名称把 `chaos-web` 当成旧前端删除。** ZCode 浏览器页面由它提供静态资源和 `/ws/zcode` RPC；移除宿主会同时关闭登录、静态页面和与 Chaos Engine 的连接。`xai-grok-web` 中如仍有旧 JSON/`/ws` 兼容路由，那是服务端接口，不是前端 bundle；移除前须先核查兼容客户端、测试和部署使用情况。

## 前端源码、构建与部署

ZCode 前端源码仍由本项目管理。为隔离 Node/pnpm 工具链，它位于单独的 frontend 源码树/checkout，而不是 `chaos-code` Rust workspace 的成员；“单独 checkout”只描述目录和构建边界，不表示交由外部项目维护。编辑前端时，在项目管理的 ZCode 源码根目录使用 Node/pnpm 工具链（当前要求 Node.js 24 或更新版本）。每次前端部署应记录实际 Git revision、工作树是否有未提交改动及构建产物校验值。当前 Chaos Cargo CI 不会自动构建这棵前端源码树：

```sh
pnpm --filter @zcode/ui lint
pnpm --filter @zcode/web build
```

Web build 输出 `packages/web/dist/`。部署时将该目录的**内容**同步到已确认专用于静态资源的 `CHAOS_WEB_ASSETS_DIR`，使该目录的 `index.html` 与所有引用的 hash 资源来自同一次 build。若用 `rsync --delete` 清理旧 hash，必须再次确认目标是专用静态目录；不得对其父目录、用户数据目录或任意根据未核实变量拼出的路径执行删除。

改动 Chaos Rust 宿主、桥或引擎时，构建相应的 `chaos-web`（`cargo build -p xai-grok-web --bin chaos-web`）；改动 headless CLI 事件/usage wire 时还需构建 `chaos`（`cargo build -p xai-grok-pager-bin --bin chaos`）。部署到已有服务后，按它配置的工作目录、二进制和 `CHAOS_WEB_ASSETS_DIR` 更新并重启该服务；不要另起一个占用不同端口的服务器来代替正在服务的实例。

## 验证

1. ZCode 前端改动：在本项目管理的 ZCode frontend checkout 运行其 lint、typecheck/test（若该改动适用）和 Web build；该源码不在 `chaos-code` Cargo workspace 中，Chaos Rust CI 的通过不能替代前端检查。
2. Rust 桥接/引擎改动：运行 `cargo test -p chaos-engine`、`cargo test -p xai-grok-web`，并运行相关 WebSocket 集成用例。
3. 部署后：检查 `chaos-zcode-web.service`（或实际使用的服务）、宿主 `/health`、登录后页面引用的 `/assets/index-<hash>.js` 与该静态目录的入口一致；再用真实浏览器强制刷新并走一轮对话。
4. 历史会话只读显示旧转录事实；实时输出估算值只在新生成的回复期间产生，Provider 真实 usage 只能在其报告后出现。

## 来源与许可证

ZCode 前端源码由本项目在自己的 frontend checkout 中管理，生成资源及其依赖通知随该源码版本维护。本仓库不把它们描述为 Chaos clean-room 前端；前端来源、许可证及第三方 notices 以项目管理的 ZCode checkout 固定 revision 和构建产物为准。见 [`ui-source-baseline.md`](../legal/ui-source-baseline.md)。
