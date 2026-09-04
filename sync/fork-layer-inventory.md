# Fork 专属层清单（上游同步使用）

merge 前基线 = 本仓库 main（上次上游同步合并点）；目标 = `upstream/main`。
冲突解决时：**本清单内的文件禁止无脑 theirs**；merge 后逐一核对 fork 改动未被静默覆盖。

## 1. 认证 / 凭证（去 OIDC，用户自带模型凭证）
- `crates/codegen/xai-grok-shell/src/auth/**`
- `crates/codegen/xai-grok-shell/src/agent/config.rs`（模型凭证解析）
- `crates/codegen/xai-grok-shell/src/agent/model_providers.rs`
- `crates/codegen/xai-grok-shell/src/agent/config_model_override_parse.rs`
- `crates/codegen/xai-grok-shell/src/agent/auth_method.rs`
- `crates/codegen/xai-grok-shell/src/util/config/load.rs`（双读 `~/.chaos`/`~/.grok`）
- `crates/codegen/xai-grok-pager/src/slash/commands/login.rs` / `logout.rs`
- 任何订阅检查 / 强制登录墙 dispatch 路径

## 2. Provider 管理（catpaw 已删，保留无 catpaw 形态）
- `crates/codegen/xai-grok-pager/src/slash/commands/provider.rs`
- `crates/codegen/xai-grok-pager/src/views/provider_modal/**`
- `crates/codegen/xai-grok-pager/src/app/actions.rs`（OpenProviderModal 等）
- `crates/codegen/xai-grok-pager/src/app/effects/mod.rs`
- `crates/codegen/xai-grok-pager/src/app/dispatch/task_result.rs`
- `crates/codegen/xai-grok-pager/src/app/dispatch/router.rs`
- `crates/codegen/xai-grok-pager/src/app/dispatch/settings/ui.rs`
- `crates/codegen/xai-grok-pager/src/app/agent_view/mod.rs`（provider outcome）

## 3. 品牌 / 中文 / UI
- `crates/codegen/xai-grok-pager/src/slash/commands/**`（中文 description）
- `crates/codegen/xai-grok-pager/src/settings/defs.rs` / `registry.rs`
- `crates/codegen/xai-grok-pager/src/views/settings_modal/**`
- `crates/codegen/xai-grok-pager/src/views/extensions_modal.rs`（中文 + 上游分组功能 → 手工）
- `crates/codegen/xai-grok-pager/src/views/shortcuts_help.rs`
- `crates/codegen/xai-grok-pager/src/actions/defaults.rs`
- `crates/codegen/xai-grok-pager/assets/logo/logo0{5,7}.txt`
- `crates/codegen/xai-grok-pager/src/views/welcome/**`
- `crates/codegen/xai-grok-pager-render/src/theme/mod.rs`（中文主题名）
- `crates/codegen/xai-grok-pager/src/views/agent_status.rs`（rate chip + 中文）
- `crates/codegen/xai-grok-pager/src/app/agent_view/render.rs`（rate chip 挂载）

## 4. rate chip（fork 独有）
- `crates/codegen/xai-grok-pager/src/acp/tracker.rs`
- `crates/codegen/xai-grok-pager/src/app/agent_view/session.rs`（live_rate_for_chip）
- `crates/codegen/xai-grok-pager/src/views/agent_status.rs`（tokens_per_sec_line + 0 占位）
- `crates/codegen/xai-grok-pager/src/app/agent_view/render.rs`

## 5. workbuddy（fork 修复）
- `crates/codegen/xai-grok-sampler/src/client.rs`
- `crates/codegen/xai-grok-sampler/src/retry.rs`
- `crates/codegen/xai-grok-sampler/src/shared_http.rs`
- `crates/codegen/xai-grok-sampler/src/attribution.rs`
- `crates/codegen/xai-grok-shell/src/agent/config.rs`（is_workbuddy / marker 注入）

## 6. 版本 / 二进制 / 日志
- `crates/codegen/xai-grok-pager-bin/**`（`[[bin]] name = "chaos"`）
- `crates/codegen/xai-grok-version/Cargo.toml`、`xai-grok-pager/Cargo.toml`、
  `xai-grok-pager-bin/Cargo.toml`、`xai-grok-shell/Cargo.toml`（lockstep 0.3.1）
- `CHANGELOG.md`（仓库根，fork 中文）
- `crates/codegen/xai-grok-shell/CHANGELOG.md` + `changelogs/**`（fork 版本线）
- `crates/codegen/xai-grok-shell-base/src/util/changelog.rs`（CDN base / 缓存路径）

## 7. 遥测弱化
- `crates/codegen/xai-grok-shell/src/agent/otel_gate.rs`
- 其它强制遥测点（上游若重新打开，需要评估）

## 8. 已移植项的再核对
- `crates/codegen/xai-grok-workspace/src/session/git.rs`（默认分支检测，两边都改）
- `crates/codegen/xai-grok-markdown/**`（表格换行）
- `crates/codegen/xai-sqlite-journal/src/lib.rs`（busy retry）

## 9. 文档
- `crates/codegen/xai-grok-pager/docs/**`（中文；上游新增章节视情况补译）

## 10. 内置模型目录（BYOK 空目录，2026-09 新增）
- `crates/codegen/xai-grok-models/default_models.json` — 必须保持
  `{"default": "chaos-default", "models": []}`；上游 merge 冲突一律取本侧空目录，
  **禁止带回 grok 模型**（上游 tip 含 grok-4.6 等条目）
- `crates/codegen/xai-grok-models/src/lib.rs`（空目录 + 中性兜底 slug 语义）
- `crates/codegen/xai-grok-shell/src/util/config/resolve/features.rs`（`remote_fetch`
  代码默认必须为 **false**，与 CHAOS.md / user-guide 文档一致；上游默认 true 不合入）
- 守卫测试：`bundled_default_models_catalog_is_empty`（`agent/config.rs`）、
  `remote_fetch_defaults_to_false_when_absent`（`features.rs`）——merge 后必跑
