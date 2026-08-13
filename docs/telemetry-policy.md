# Chaos 遥测策略（Telemetry Policy）

> 本文件是 Chaos 项目遥测行为的**单一可信源**。如果实现和本文档不一致，以
> 实现为准，然后开个 issue 改文档。
>
> 关联代码：`crates/codegen/xai-grok-telemetry/src/`
> 解析逻辑：`crates/codegen/xai-grok-shell/src/agent/config.rs::resolve_telemetry_mode`

## 一句话总结

**Chaos 默认不发任何遥测。** 任何"看起来像遥测"的网络请求，都是在你
显式 opt-in 后才会发生。

---

## 1. 三个独立开关

遥测系统由三个**互相独立**的子系统组成，每一个都有自己的开关。任何一
个开关关闭，相关的网络请求就不会发生。

| 子系统 | 干什么 | 主开关 | 默认 |
|---|---|---|---|
| **产品遥测**（mixpanel + events endpoint） | 上报生命周期事件、crash、性能指标 | `[features] telemetry` / `GROK_TELEMETRY_ENABLED` | **关** |
| **GCS trace 上传** | 上传调用 trace / span | `[telemetry] trace_upload` / `GROK_TELEMETRY_TRACE_UPLOAD` | **关** |
| **外部 OTEL** | 把指标 / 日志发到**你自己的** OTEL collector | `GROK_EXTERNAL_OTEL` + 标准 `OTEL_*` | **关** |

每个子系统**不依赖**另外两个的状态。比如 `telemetry = true` 不会自动开
trace upload；反过来 `trace_upload = true` 不会自动开 mixpanel。

## 2. 模式层级（产品遥测）

`[features] telemetry` 接受三种值（TOML bool 兼容）：

| 值 | 含义 |
|---|---|
| `false` / `"disabled"` / `"off"` | 什么都不发 |
| `"session_metrics"` | 只发 lifecycle 元事件（`session_metrics::*`），不写 Mixpanel profile（`engage` 不触发），不带任何 prompt/response/tool 内容 |
| `true` / `"enabled"` / `"on"` | 完整产品遥测：所有事件 + Mixpanel profile sync |

来源优先级（从高到低）：

1. **Requirements（管理员锁定）** — `[requirements] telemetry` 不可被用户
   覆盖
2. **环境变量** `GROK_TELEMETRY_ENABLED`
3. **`config.toml`** `[features] telemetry`
4. **远程 settings**（如果 fork 接入）
5. **默认值** — `Disabled`（关）

## 3. 谁收、收什么

### 3.1 产品遥测（`Enabled` 模式）

| 字段 | 用途 |
|---|---|
| `agent_id` / `deployment_id` / `team_id` | 聚合去重 |
| `shell_version` / `app_name` / `client_type` / `client_version` | 版本统计 |
| `subscription_tier` | 套餐维度（仅当上游提供，Chaos fork 通常为空） |
| `country` / `language` / `locale` | 地理语言维度 |

**不**发：prompt 内容、response 内容、工具调用参数、文件路径、API key、
任何用户输入输出。

mixpanel profile sync (`sync_profile`) 仅在 `Enabled` 模式下触发；`SessionMetrics`
模式**不**触发 `engage`。

### 3.2 GCS trace upload

仅在 `trace_upload = true` 时把 trace 写到 S3 / GCS。endpoint 默认指向
GCS 桶；如要换 endpoint 见 `[telemetry] trace_upload_endpoint_url`。`trace_upload`
会被 `[features] telemetry` 的**关闭**态压制（`mode.value.is_disabled()`），
但反过来不成立。

### 3.3 外部 OTEL

发到**用户自己**配置的 OTLP endpoint。**不**走任何 xAI 后端；本模块的代码
路径不持有 `AuthCredentialProvider`，`OTEL_EXPORTER_OTLP_HEADERS` 之外不附
带任何 header。schema 见 `xai-grok-telemetry/src/external/schema.rs`。

## 4. 怎么永久关

任选一个：

```toml
# config.toml
[features]
telemetry = false          # 也接受 "disabled" / "off"
telemetry = "session_metrics"  # 想留 lifecycle 就用这个

[telemetry]
trace_upload = false
```

或环境变量：

```sh
export GROK_TELEMETRY_ENABLED=false
export GROK_TELEMETRY_TRACE_UPLOAD=false
unset GROK_EXTERNAL_OTEL    # 完全不设 = 完全关
```

或管理员锁定（在受管部署场景，**用户改不了**）：

```toml
[requirements]
telemetry = false
```

## 5. 怎么验证当前状态

Chaos 0.2.137+ 计划新增 `chaos telemetry status` 子命令，输出类似：

```text
telemetry mode:  disabled           (source: config.toml [features])
mixpanel:         disabled           (no token configured)
trace upload:     disabled           (source: default)
external otel:    disabled           (GROK_EXTERNAL_OTEL unset)
```

如果你**没有看到这一行**就说明命令还没实装，可用如下方法手动确认：

```sh
# 1. 看配置生效值
chaos config show | grep -A2 telemetry

# 2. 看运行时是否真的没出站
# 注入 GROK_TELEMETRY_ENABLED=true 然后跑 session，
# 用 tcpdump / mitmproxy 看 chaos 是否真的没出网：
sudo tcpdump -i any -A 'host <mixpanel-host> or host <events-host>'
```

## 6. 数据删除 / 退出

如果你之前开启过遥测想撤回：

- **本地**：见 `chaos telemetry status` 输出里的"本地数据"段（计划中）；
  或直接 `rm -rf ~/.chaos/telemetry/`（如有）。
- **服务端**：因为 Chaos fork 默认关，**默认情况下服务端不会收到任何
  数据**。如果你曾主动开启过产品遥测并希望从服务端撤回，联系对应接收
  方的 privacy / DPA 流程（mixpanel 端 / 自建 OTEL collector / GCS 桶
  owner）。

## 7. 威胁模型声明

Chaos fork 的遥测设计前提：

- 用户**有能力**检查本地配置（能打开 `config.toml`）
- 用户**有能力**看网络流量（可信 OS / 能装 tcpdump）
- 用户**不**信任 GitHub Release 上游账号能完美不泄露（参见
  `docs/release-process.md` 的代码签名计划）

在这些前提都不成立的环境（比如不可审计的受管部署）里，应当**走 requirements
锁定 + 全局防火墙**，而不是依赖客户端开关。

## 8. 变更历史

| 版本 | 变更 |
|---|---|
| 0.2.137 | 初版。固化现有默认值与解析顺序；为后续 `chaos telemetry status` / `disable` 子命令铺路。 |
