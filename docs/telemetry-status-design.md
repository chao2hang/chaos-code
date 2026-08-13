# `chaos telemetry status` / `disable` 设计

> 状态：设计草稿 v0.1，**尚未实现**。等本设计评审通过后再开工。
> 关联：[`telemetry-policy.md`](./telemetry-policy.md) 7.2 段。

## 1. 目标

让用户用一行命令看清：

1. 当前三个子系统的**有效状态**（产品遥测 / trace upload / 外部 OTEL）
2. 状态**从哪儿来**（config / env / requirement / default）
3. 怎么**永久关掉**（`disable` 子命令）

**非目标**：

- 不实现遥测**控制平面**（开/关、采样率等运行时切换），那是另一条线
- 不向任何远端上报本次 status 调用本身
- 不改 `requirements.*`（管理员锁定的字段，用户命令不该越权）

## 2. CLI 形态

```
$ chaos telemetry status
telemetry mode:   disabled             (source: default)
                  └─ resolution: [features].telemetry=∅, env=∅, requirements=∅
mixpanel:         disabled             (no token configured)
trace upload:     disabled             (source: [telemetry] trace_upload=∅, default=false)
external otel:    disabled             (GROK_EXTERNAL_OTEL unset, OTEL_EXPORTER_OTLP_ENDPOINT unset)
config root:      /home/u/.chaos       (CHAOS_HOME)
auth module:      enabled              (auth.json not loaded, but AuthManager constructed)
```

```
$ chaos telemetry status --json
{
  "config_root": "/home/u/.chaos",
  "subsystems": {
    "telemetry": {
      "mode": "disabled",
      "source": "default",
      "resolution_chain": [
        {"layer": "requirement", "value": null, "winner": false},
        {"layer": "env", "name": "GROK_TELEMETRY_ENABLED", "value": null, "winner": false},
        {"layer": "config", "name": "[features].telemetry", "value": null, "winner": false},
        {"layer": "remote", "value": null, "winner": false},
        {"layer": "default", "value": "disabled", "winner": true}
      ]
    },
    "mixpanel": {"enabled": false, "reason": "no token configured"},
    "trace_upload": {"enabled": false, "source": "default"},
    "external_otel": {"enabled": false, "master_switch": false, "endpoint": null}
  },
  "auth_module": {"constructed": true, "auth_json_loaded": false}
}
```

```
$ chaos telemetry disable
Wrote ~/.chaos/config.toml:
  [features]
  telemetry = false
  [telemetry]
  trace_upload = false
External OTEL not touched (uses env vars, not config).

$ chaos telemetry disable --local-only
# 只写 ~/.chaos/config.toml，不动项目级 .chaos/config.toml
# 防止污染共享 repo

$ chaos telemetry disable --project
# 写当前 cwd 下的 .chaos/config.toml
# 不写 ~/.chaos
```

## 3. 实现要点

### 3.1 复用现有解析路径

`xai-grok-shell::agent::config` 里已经有：

- `resolve_telemetry_mode() -> Resolved<TelemetryMode>`（带 source 标签）
- `resolve_trace_upload() -> Resolved<bool>`
- `TelemetryConfig` 字段（events_url / mixpanel_token / mixpanel_enabled）

`status` 子命令**不**自己重新解析；它调上面这些方法，输出
`Resolved.value` 和 `Resolved.source`。这样 status 看到的和实际生效的
永远一致，不会出现"status 说关，实际在发"的分裂。

### 3.2 三个子系统的真实来源

| 子系统 | 怎么判 | 文件 |
|---|---|---|
| 产品遥测 mode | `cfg.resolve_telemetry_mode()` | `xai-grok-shell/src/agent/config.rs:2680` |
| mixpanel enabled | `cfg.telemetry.mixpanel_enabled && cfg.telemetry.mixpanel_token.is_some()` | `xai-grok-telemetry/src/config.rs:99` |
| trace upload | `cfg.resolve_trace_upload()` | `xai-grok-shell/src/agent/config.rs:2702` |
| external otel | `std::env::var_os("GROK_EXTERNAL_OTEL").is_some() && (OTEL_EXPORTER_OTLP_ENDPOINT \| OTEL_EXPORTER_OTLP_TRACES_ENDPOINT)` | `xai-grok-telemetry/src/external/config.rs` |

### 3.3 `disable` 写 config 的安全约束

| 约束 | 怎么做 |
|---|---|
| 不覆盖已有 `[[requirements]]` 块 | 解析时如果检测到 `requirements.telemetry` 存在，**直接报错退出**，提示用户找管理员 |
| 不破坏 TOML 结构 | 用 `toml_edit` crate 做 in-place edit，而不是 toml::to_string 全量重写 |
| 不覆盖用户已有 `[features]` 块里其他字段 | toml_edit 天然支持局部更新 |
| 默认只动 `~/.chaos/config.toml` | `--local-only` 是默认行为；`--project` 需显式 |
| 写完回读校验 | 写完重新 load config，确认 `resolve_telemetry_mode() == Disabled && resolve_trace_upload() == false`，否则回滚并报错 |
| backup 旧 config | 写前 `cp ~/.chaos/config.toml ~/.chaos/config.toml.bak.<unix-ts>` |

### 3.4 输出 / 日志

- `status` 默认走 stdout，`--json` 走 stdout（仍人类可读）
- `disable` 的"wrote ..."行走 stdout，警告（如 requirements 锁定）走 stderr
- **不**写 chaos 自己的日志文件（避免自我遥测）
- **不**触发 `track_event` / `log_event`（避免自我引用循环）

## 4. 依赖

| 用途 | crate |
|---|---|
| 局部 TOML 编辑 | `toml_edit`（workspace 已有，confirm） |
| 时间戳 | `chrono` 或 `time`（workspace 已有，确认用哪个） |
| JSON 输出 | `serde_json`（已有） |

## 5. 测试

| 类别 | 覆盖 |
|---|---|
| 单元 | `resolve_telemetry_mode` 5 层 source 标签各一例 |
| 单元 | `disable` 在 requirements 锁定时拒绝并报错 |
| 单元 | `disable` 用 `toml_edit` 不破坏已有 `[features]` 其他字段 |
| 单元 | `disable` 后回读校验失败时回滚（mock 失败注入） |
| 集成 | `chaos telemetry status --json` 输出 schema 稳定（snapshot test） |
| 集成 | `--local-only` 不创建 `.chaos/config.toml` 在 cwd |
| 集成 | `--project` 创建 `.chaos/config.toml` 而不动 `~/.chaos/config.toml` |

## 6. 文档

- `chaos telemetry --help` 内嵌简明说明
- `docs/telemetry-policy.md` 5 节链过来
- CHAOS.md 提一句
- `crates/codegen/xai-grok-pager/docs/user-guide/` 加一节（视翻译时间表）

## 7. 发版策略

| 版本 | 内容 |
|---|---|
| 0.2.137 | 本设计落地。**只**加 `status` 子命令（只读） |
| 0.2.138 | 加 `disable` 子命令（带 backup + rollback） |
| 0.2.139 | 加 `chaos telemetry enable`（高级用户；走 [telemetry] 段写出明确的 opt-in 注释，避免用户误开） |

`enable` 放最后、单独发版，因为"教用户怎么开"比"教用户怎么关"风险高，
需要更多 review。

## 8. 开放问题

1. `disable` 是否应该**也**清空 `~/.chaos/telemetry/`（本地缓存）？目前
   `xai-grok-telemetry` 似乎只在内存里，没有磁盘缓存——待确认。
2. `status` 是否需要 `--watch` 模式（每秒刷新）？这要拉 socket 之类的，
   复杂度不低，**默认不做**，如有需求加 `--watch=N`。
3. `auth.json` 状态（"constructed: true, auth_json_loaded: false"）
   是否要暴露给用户？目前设计里有，但可能泄露信息（让攻击者知道
   配置根）。**倾向**只输出 "auth module: enabled|disabled"，不暴露
   auth.json 是否被加载过。
