# Chaos fork 冲突地图

移植上游 `xai-org/grok-build` 时，优先保护下列区域。路径以本仓库为准，上游同名路径若存在则以 **手工合流** 对待。

## 产品定位

| 主题 | Chaos 立场 | 上游常见立场 |
|---|---|---|
| 登录 | 不依赖 Grok/OIDC；用户自带模型凭证 | 浏览器登录 xAI / 订阅 |
| 品牌 | Chaos / 中文 UI | Grok Build / 英文 |
| 二进制 | `chaos`（`xai-grok-pager-bin`） | `grok` / `xai-grok-pager` |
| 配置目录 | 双读 `~/.chaos` / `~/.grok`（不覆盖任一侧） | `~/.grok` |

## 高风险路径（改前必读 diff 两侧）

### 认证与配置

- `CHAOS.md`（仅本 fork）
- `crates/codegen/xai-grok-shell/src/auth/**`
- `crates/codegen/xai-grok-shell/src/agent/config.rs` 及 model_providers 相关
- `crates/codegen/xai-grok-pager/src/slash/commands/login.rs` / `logout.rs`
- 任何强制跳转登录墙 / 订阅检查的 dispatch 路径

### UI 文案与资源

- `crates/codegen/xai-grok-pager/src/slash/commands/**`（`description()` 中文）
- `crates/codegen/xai-grok-pager/src/settings/defs.rs` / `registry.rs`
- `crates/codegen/xai-grok-pager/src/views/settings_modal/**`
- `crates/codegen/xai-grok-pager/src/views/extensions_modal.rs`
- `crates/codegen/xai-grok-pager/src/views/shortcuts_help.rs`
- `crates/codegen/xai-grok-pager/src/actions/defaults.rs`
- `crates/codegen/xai-grok-pager/assets/logo/logo05.txt` / `logo07.txt`
- `crates/codegen/xai-grok-pager/src/views/welcome/**`

### 更新日志

- `crates/codegen/xai-grok-shell/changelogs/**`
- `crates/codegen/xai-grok-shell/CHANGELOG.md`
- `crates/codegen/xai-grok-shell-base/src/util/changelog.rs`（CDN base、缓存路径）
- 本机 `~/.grok/CHANGELOG.{md,json}`（运行时缓存，不进 git）

### 二进制入口

- `crates/codegen/xai-grok-pager-bin/**`（`[[bin]] name = "chaos"`）

## 相对安全（可优先合上游）

- `crates/codegen/xai-grok-tools/**` 工具实现与协议修复  
- `crates/codegen/xai-grok-workspace/**`  
- `crates/codegen/xai-grok-agent/**`、sampler、MCP 协议修补  
- 纯引擎：queue / interjection / scrollback 逻辑（**若测试大量中文断言，合完后改测试不是改回英文文案**）  
- `third_party/**`  vendored 依赖  

## 测试注意

- 合入上游测试时：若断言英文 UI 字符串，应改为 Chaos 中文（或测 key/结构不测文案）  
- CJK 宽字符在 ratatui buffer 里可能有 continuation cell；测试读行应用 Unicode 宽度跳进（参考 `settings_modal` 的 `buf_row_text`）

## 版本 lockstep

改版本号时同时检查（非完整列表，以 workspace 为准）：

- `crates/codegen/xai-grok-version/Cargo.toml`
- `crates/codegen/xai-grok-pager/Cargo.toml`
- `crates/codegen/xai-grok-pager-bin/Cargo.toml`
- `crates/codegen/xai-grok-shell/Cargo.toml`

`SOURCE_REV` 记录的是 monorepo 同步指纹，**不一定**等于公开 GitHub commit SHA；对齐时在同步报告里写清用的是哪一种。
