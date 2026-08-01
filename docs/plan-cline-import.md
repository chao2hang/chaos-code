# 计划：/provider 添加渠道时支持从 Cline 导入

**分支**：`feat/provider-cline-import`　**目标版本**：v0.2.128（暂定）

## 1. 目标与范围（Scope）

在 `/provider` 的 **添加渠道（Add）** 流程中，**仅当检测到本机已安装 Cline 且能读到可用配置**时，在“预设选择”列表里多出一个选项「从 Cline 导入」。用户选择后，直接读取 Cline 的接口信息与 API Key，落成 Chaos 的一个渠道（`[model_providers.*]`），并自动补一个 `[model.*]` 条目，之后即可直接使用。

**不在本次范围**：
- 不改 Cline 的任何数据（严格只读）。
- 不处理 Cline 里被 `safeStorage` 加密的 Key（标记为“不可自动导入”，提示手动填）。
- 不做 MCP / skills / hooks 的 Cline 导入（那些属于别的需求）。

## 2. Cline 数据在哪、怎么读

Cline（扩展 ID `saoudrizwan.claude-dev`）把接口配置 + Key 存在编辑器 `globalState`，落盘为 SQLite：

| 编辑器 | 常见路径 |
|--------|----------|
| VS Code | `~/.config/Code/User/globalStorage/state.vscdb` |
| Cursor | `~/.config/Cursor/User/globalStorage/state.vscdb` |
| Windsurf | `~/.config/Windsurf/User/globalStorage/state.vscdb` |
| VSCodium | `~/.config/VSCodium/User/globalStorage/state.vscdb` |
| Windows | `%APPDATA%\{Editor}\User\globalStorage\state.vscdb` |

`state.vscdb` 的 `ItemTable` 中，Cline 相关的键形如 `<extId>.<key>`，值为 JSON 字符串，例如：

- `...cline_apiProvider`：`anthropic` / `openai` / `openrouter` / `custom`…
- `...cline_anthropicApiKey` / `...cline_openAiApiKey` / 自定义 `...cline_customApiProviders`
- `...cline_anthropicBaseUrl` / `...cline_openAiBaseUrl`
- `...cline_apiModelId`（当前模型）

读取方式：用 `rusqlite`（`xai-grok-shell` 已有 bundled 版本）**只读打开**（`SQLITE_OPEN_READ_ONLY`），查询 `ItemTable` 中 `key LIKE '<extId>.%'`，逐键解析 JSON。若值形如 `v1:` 开头（safeStorage 密文）→ 标记为“加密不可导入”。

> 复用先例：`xai-grok-workspace/src/foreign_sessions/codex/db.rs` 已是“只读打开别家 SQLite 库”的成熟做法，`claude_import.rs` 是“扫描别家工具配置”的范式。

## 3. 字段映射（Cline → Chaos config.toml）

| Cline | Chaos 渠道字段 | 说明 |
|-------|---------------|------|
| `apiProvider` | 决定下面的 scheme / backend | 也决定渠道默认名（`cline-anthropic` 等） |
| `anthropicBaseUrl` / `openAiBaseUrl` / 自定义 `baseUrl` | `model_providers.<id>.base_url` | |
| `anthropicApiKey` / `openAiApiKey` / 自定义 `apiKey` | `model_providers.<id>.api_key` | 明文才写；加密则留空并提示 |
| apiProvider∈{anthropic, claude…} | `auth_scheme="x_api_key"`, `api_backend="messages"` | 且补 `anthropic-version: 2023-06-01`（复用现有 `sync_anthropic_version_header`） |
| apiProvider∈{openai, openrouter, custom…} | `auth_scheme="bearer"`, `api_backend="responses"`（或按模型 `chat_completions`） | |
| `apiModelId` | 自动生成 `[model."<id>/<model>"]` 条目 | 便于直接当默认模型 |

写入复用现有 `add_provider()`（`slash/commands/provider.rs`）。

## 4. 代码改动点

### 4.1 `xai-grok-shell` 新增读取模块
新文件 `crates/codegen/xai-grok-shell/src/cline_import.rs`（与 `claude_import.rs` 并列），导出：

- `pub struct ClineCandidate { display_name, provider_id, base_url, auth_scheme, api_backend, api_key: Option<String>, model: Option<String>, key_encrypted: bool }`
- `pub fn detect_cline_installs() -> Vec<ClineInstall>`：扫描各编辑器 `globalStorage/state.vscdb`，能读且含 `cline_` 项即算“检测到”。
- `pub fn list_cline_providers() -> Vec<ClineCandidate>`：汇总所有可导入渠道（含加密标记）。

在 `xai-grok-shell/src/lib.rs` 登记 `pub mod cline_import;`。（依赖：已有 `rusqlite`、`serde_json`。）

### 4.2 `/provider` Add 表单：预设多一项
`crates/codegen/xai-grok-pager/src/views/provider_modal/state.rs`
- 构造 `ProviderModalState::new(Add)` 时同步 `list_cline_providers()`；若有有效候选，把「从 Cline 导入」作为**预设列表末尾（自定义行之后）的附加项**，并缓存 `cline_candidates`。

`provider_modal/input.rs`（`handle_add` 的 `FormStep::Preset` 分支 + 行数 `max`）
- 行数上界从 `PROVIDER_PRESETS.len()` 变为“预设 + 自定义 + 可选 Cline 项”的动态值。
- `KeyCode::Enter` 命中 Cline 项时：`state.current_step = FormStep::ClinePick`（新步骤），`selected` 复位到候选列表。

新增 `FormStep::ClinePick`：
- 列出检测到的 Cline 渠道（显示 `display_name`、`base_url`、`has_key`/`🔒 已加密` 标记）。
- 选中 + Enter → 把候选字段填入 `state`（`name`/`base_url`/`auth_scheme_idx`/`api_backend_idx`/`api_key`），然后直接 `ProviderKeyOutcome::Commit`（走现有 `add_provider` 落盘）。
- 加密项可选中但提交时提示“该 Key 已被 Cline 加密，请在 API Key 步骤手动粘贴”。

`provider_modal/render.rs`
- 预设列表末尾渲染 Cline 项（「从 Cline 导入 (N)」）。
- 新增 `ClinePick` 子列表渲染。

### 4.3 落盘与收尾
- 复用 `add_provider()`；写入后自动帮用户 `SetModel`（若 Cline 有 `apiModelId`），流程与现有 hub 添加后一致。
- 若检测到 Cline 但**没有**可读明文 Key（全加密或为空）→ 列表里隐藏该渠道，仅在无任何可导入项时不显示入口。

## 5. 边界与降级

| 情形 | 表现 |
|------|------|
| 未安装 Cline / 读不到 `state.vscdb` | 完全不影响原 Add 流程，无 Cline 项 |
| Key 为 `safeStorage` 密文 | 显示 🔒，选中后引导手动粘贴 Key |
| 多编辑器同时有 Cline | 按 Code → Cursor → Windsurf 顺序去重，取第一个读到的 |
| 自定义 provider（`customApiProviders`） | 同样读取 baseUrl/apiKey，`auth_scheme=bearer` |
| Windows 主线程栈 | 沿用 v0.2.127 的 `/STACK:8388608` 约束，读取放短任务 |

## 6. 安全

- 全程**只读**打开 Cline 库（`SQLITE_OPEN_READ_ONLY`），不落日志里的 Key，不写回 Cline。
- 明文 Key 复制进 `config.toml` 符合 Chaos BYOK 理念；导入动作需用户显式选择确认。
- 文档注明：Cline 的 Key 可能被系统钥匙串加密，明文方案不保证所有环境可用。

## 7. 测试

- `xai-grok-shell`：`detect_cline_installs` 用临时 `state.vscdb`（`rusqlite` 写入）测读取+去重；加密值 `v1:` 判定；自定义 provider 解析。
- `xai-grok-pager`：`handle_add` 在 Cline 项上的导航与提交；无 Cline 时行为回归。
- 冒烟：`cargo check -p xai-grok-pager --lib`、`cargo test -p xai-grok-shell --lib -- cline_import`；TUI 手工验证 `/provider add` 出现 Cline 项。

## 8. 里程碑

1. **M1**：`xai-grok-shell` 的 `cline_import.rs`（扫描 + 解析 + 单测）+ `cargo check` 通过。
2. **M2**：`/provider` Add 表单接入 ClinePick 步骤 + 渲染 + 落盘，`cargo check` / 现有测试回归通过。
3. **M3**：手工 TUI 验证（含加密 Key 降级）+ CHANGELOG 记录，提交分支。

## 9. 待确认问题

- 导入后是否**默认帮用户注册为默认模型**，还是仅新增渠道（我建议默认新增渠道，不擅自改 `[models].default`）。
- 渠道命名：建议 `cline-<provider>`（如 `cline-openai`、`cline-anthropic`），可编辑。
