# ZCode Web 前端与 Chaos 宿主

- 状态：单仓库集成路径（2026-10-19）
- 范围：说明纳入 Chaos Code 仓库的 ZCode Web 源码与 Rust Web 宿主之间的构建和部署边界。

## 当前架构

Chaos Web 使用 **本仓库集成的 ZCode 官方 Web 前端**。旧的自研 `apps/chaos-ui` 已于 2026-10-09 退役并从工作树移除；它不是当前 UI。

| 组件 | 位置 | 职责 |
|---|---|---|
| ZCode Web 客户端 | `frontend/zcode/`（独立 pnpm workspace，不属于 Cargo workspace） | React/TypeScript UI；构建为静态 Web 资源 |
| Chaos Web 宿主 | `crates/codegen/xai-grok-web`，二进制名 `chaos-web` | 登录与鉴权、静态资源托管、`/ws/zcode` RPC 接入、V4 协议桥接到 Chaos Engine |
| 引擎与 Agent | `crates/codegen/chaos-engine`、`xai-grok-pager-bin`（二进制名 `chaos`） | 会话状态、Provider/Agent 调用、流式事件与工具能力 |
| 部署资源 | 宿主配置的 `CHAOS_WEB_ASSETS_DIR` | ZCode Web build 产物；不含源码副本 |

**不要因为名称把 `chaos-web` 当成旧前端删除。** ZCode 浏览器页面由它提供静态资源和 `/ws/zcode` RPC；移除宿主会同时关闭登录、静态页面和与 Chaos Engine 的连接。`xai-grok-web` 中如仍有旧 JSON/`/ws` 兼容路由，那是服务端接口，不是前端 bundle；移除前须先核查兼容客户端、测试和部署使用情况。

## 集成来源

- 源码目录：`frontend/zcode/`，纳入 Cargo 仓库但保留 ZCode 自己的 pnpm workspace、lockfile、构建脚本和第三方声明；不将前端包声明成 Cargo crate。
- 初始集成源提交：`e106a0ae94fbbb0846a2b5051dac4baf36ab58b3`。它包含 ZCode `v3.15.1` (`origin/main` `aac47556`) 与 `origin/feat/ui-plugin` (`662c30be`) 的合并提交，以及原有本地 UI 改动和终端输出改进。ZCode 合并历史保存在 Chaos Git 对象图的 `zcode/source-integration` 父提交中；备份分支和原始 stash 暂保留在 ZCode checkout。
- ZCode Web/共享 UI 源码、图片/字体/图标及 `THIRD-PARTY-NOTICES.md` 仍属于 ZCode Apache-2.0 源码及其依赖；不要把它们描述为 Chaos clean-room 实现。修改时保留对应 upstream 归属与 notices。

## 构建与部署

使用 Node.js 24+ 和 pnpm 10；从仓库根执行：

```sh
pnpm --dir frontend/zcode install --frozen-lockfile
pnpm --dir frontend/zcode typecheck
pnpm --dir frontend/zcode lint
pnpm --dir frontend/zcode exec vitest run packages/ui/test
pnpm --dir frontend/zcode --filter @zcode/web build
```

Web build 输出 `frontend/zcode/packages/web/dist/`。部署时将该目录的**内容**同步到已确认专用于静态资源的 `CHAOS_WEB_ASSETS_DIR`，使该目录的 `index.html` 与所有引用的 hash 资源来自同一次 build。只有再次核实目标目录确为专用静态目录时才清理旧 hash；不得对其父目录、用户数据目录或由未核实变量拼出的路径执行 `--delete`/删除。

改动 Chaos Rust 宿主、桥或引擎时，构建相应的 `chaos-web`（`cargo build -p xai-grok-web --bin chaos-web`）；改动 headless CLI 事件/usage wire 时还需构建 `chaos`（`cargo build -p xai-grok-pager-bin --bin chaos`）。部署到已有服务后，按它配置的工作目录、二进制和 `CHAOS_WEB_ASSETS_DIR` 更新并重启该服务；不要另起一个占用不同端口的服务器来代替正在服务的实例。

## 验证

1. ZCode 前端改动：在 `frontend/zcode/` 运行 lint、typecheck、适用的 UI tests 和 Web build；它不在 Cargo workspace 中，Rust tests 不能替代前端检查。
2. Rust 桥接/引擎改动：运行 `cargo test -p chaos-engine`、`cargo test -p xai-grok-web`，并运行相关 WebSocket 集成用例。
3. 部署后：检查 `chaos-zcode-web.service`（或实际使用的服务）、宿主 `/health`、登录后页面引用的 `/assets/index-<hash>.js` 与该静态目录的入口一致；再用真实浏览器强制刷新并走一轮对话。
4. 历史会话只读显示旧转录事实；实时输出估算值只在新生成的回复期间产生，Provider 真实 usage 只能在其报告后出现。

## 来源与许可证

ZCode 前端源码现位于本仓库 `frontend/zcode/`，依赖 notice 与三方许可证随同源代码维护；Chaos 仓库中的集成不改变上游 Apache-2.0 授权和归属。见 [`ui-source-baseline.md`](../legal/ui-source-baseline.md)。
