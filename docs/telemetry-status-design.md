# `chaos telemetry status` / `disable` 设计

> 状态：`chaos telemetry status` 只读子命令已实现；本设计后续版本规划的 `disable`/`enable` 写配置命令仍未实现，且不得在安全评审前加入。实现范围/当前 JSON schema 见 `docs/architecture/todo-open-item-classification.md` 和 `TODO.md` 的 MT-7。
> 关联：[`telemetry-policy.md`](./telemetry-policy.md) 7.2 段。

## 1. 目标

让用户用一行命令看清：

1. 当前有效状态：产品遥测 mode、Mixpanel、trace upload 与 external OTEL exporter
2. 状态**从哪儿来**（config / env / requirement / default）
3. 怎么**永久关掉**（`disable` 子命令）

**非目标**：

- 不实现遥测**控制平面**（开/关、采样率等运行时切换），那是另一条线
- 不向任何远端上报本次 status 调用本身
- 不改 `requirements.*`（管理员锁定的字段，用户命令不该越权）

## 2. CLI 形态

```
$ chaos telemetry status
config root: /home/u/.chaos
telemetry: disabled (source: default)
mixpanel: disabled (disabled_or_no_token)
trace upload: disabled (source: default)
external OTEL: disabled (source: disabled_or_not_configured)
```

```
$ chaos telemetry status --json
{
  "config_root": "/home/u/.chaos",
  "subsystems": {
    "telemetry": {"mode": "disabled", "source": "default"},
    "mixpanel": {"enabled": false, "reason": "disabled_or_no_token"},
    "trace_upload": {"enabled": false, "source": "default"},
    "external_otel": {
      "enabled": false,
      "source": "disabled_or_not_configured",
      "metrics_exporter": null,
      "logs_exporter": null
    }
  }
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
| mixpanel enabled | `cfg.telemetry.mixpanel_enabled && token is non-empty` | `xai-grok-pager-bin/src/telemetry_status.rs` |
| trace upload | `cfg.resolve_trace_upload()` | `xai-grok-shell/src/agent/config.rs:2702` |
| external OTEL | `resolve_external_otel_config`，完整复用双重 opt-in 与 exporter 选择规则；仅报告开关和 exporter 名称 | `xai-grok-telemetry/src/external/config.rs` |

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

- `status` 默认走 stdout，`--json` 走 stdout（JSON object）
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
| 集成 | `chaos telemetry status --json` 真实 binary test 验证解析字段、配置来源和密钥/collector URL 不泄露 |
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
| 当前分支 | 只读 `status` 子命令已实现；`--json` schema 与隔离配置实测由 `crates/codegen/xai-grok-pager-bin/tests/telemetry_status.rs` 回归测试保护 |
| 后续评审后 | `disable`/`enable` 写配置命令仍未实现；先冻结 precedence pin、写入原子性、保留用户注释与 rollback contract，再进行安全评审 |


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
