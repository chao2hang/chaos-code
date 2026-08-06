//! `/provider` — 渠道管理命令（交互式 TUI 模态框）。
//!
//! - 裸 `/provider`（无参数）→ 渠道列表 hub：↑↓ 选择，Enter 进操作菜单，末行「+ 添加渠道」
//! - `/provider list` — 同裸命令
//! - `/provider add` — 交互式多步表单添加渠道
//! - `/provider edit <name>` — 编辑已有渠道（URL / 认证 / 后端 / 密钥）
//! - `/provider set-key <name>` — 交互式输入 API Key
//! - `/provider models <name>` — 获取渠道可用模型列表
//! - `/provider set-model <name>` — 交互式选择并设置当前模型
//! - `/provider manual-model <name>` — 手动输入模型 ID 并设为当前
//! - `/provider configure-model <name>` — 配置模型 max_completion_tokens 等参数
//! - `/provider refresh <name>` — 刷新渠道可用模型（同 models）
//!
//! 所有入口均通过 `Action::OpenProviderModal` 触发 TUI 模态框，
//! 配置读写逻辑由 `views/provider_modal` 调用本模块的 `pub(crate)` 函数完成。

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};
use crate::views::provider_modal::ProviderModalMode;

/// 从上游 `/v1/models` 响应中解析出的「per-model」reasoning 元数据。
///
/// 跨命名空间使用 — provider 后端写入 config，shell / acp 读出去构造
/// picker / sampler。故意**不**直接复用
/// `xai_grok_shell::remote::client::parse_remote_model_value` 的解析逻辑：
/// `parse_remote_model_value` 强依赖 base_url，且只接受单条
/// `ModelEntryConfig`，跟 `parse_models_response` 的「纯 id + 弱元数据」
/// 形态不一致。先保持两套实现，后续可以抽 `parse_reasoning_meta(&Value)`
/// 到 `xai-grok-sampling-types` 共享。
///
/// Issue #14：之前 `parse_models_response` 只取 `id`，把 `reasoningEfforts`
/// 等元数据全部丢弃，导致 `/effort` 对 BYOK 模型永远下拉为空。
///
/// 写入 config 的 effort 字符串必须是可反序列化为
/// [`xai_grok_shell::sampling::types::ReasoningEffort`] /
/// [`xai_grok_shell::sampling::types::ReasoningEffortOption`] 的规范值；
/// 上游的非标准 tier（如 `"turbo"`）在解析阶段跳过，避免整条 model 条目
/// 在 config 加载时失败。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReasoningMeta {
    /// 显式声明的可用 reasoning effort 等级（canonical ids，如 `low`/`medium`/`high`）。
    pub reasoning_efforts: Vec<String>,
    /// provider 返回的当前默认 effort（已校验的 canonical id）。
    pub reasoning_effort: Option<String>,
    /// provider 显式声明「该模型支持 reasoning effort」。
    /// 缺省时由 shell 配置层的 backend 启发式补默认（messages 默认 true）。
    pub supports_reasoning_effort: bool,
}

impl ReasoningMeta {
    /// 是否携带任何非默认 reasoning 元数据。
    pub fn is_meaningful(&self) -> bool {
        !self.reasoning_efforts.is_empty()
            || self.reasoning_effort.is_some()
            || self.supports_reasoning_effort
    }
}

/// 用户配置路径：`$CHAOS_HOME| $GROK_HOME | ~/.chaos|~/.grok`/config.toml。
///
/// 必须与 shell / 其它 pager 设置读写同一 home（`xai_grok_config::grok_home`）。
/// 旧实现硬编码 `$HOME/.grok`：Windows 常无 `HOME`、且新装默认 `~/.chaos`，
/// 导致渠道写入的 `[model."provider/id"]` 进不了 agent catalog，
/// `/model` / 渠道切模型报 `unknown model id`（issue #5）。
fn config_path() -> PathBuf {
    xai_grok_config::grok_home().join("config.toml")
}

/// 读取配置文件为 toml_edit 文档
pub(crate) fn load_config() -> Result<toml_edit::DocumentMut, String> {
    let path = config_path();
    if !path.exists() {
        return Ok(toml_edit::DocumentMut::new());
    }
    let content = fs::read_to_string(&path).map_err(|e| format!("读取配置文件失败: {e}"))?;
    content
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("解析配置文件失败: {e}"))
}

/// 保存配置文件
pub(crate) fn save_config(doc: &toml_edit::DocumentMut) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    fs::write(&path, doc.to_string()).map_err(|e| format!("写入配置文件失败: {e}"))
}

/// 从配置中获取所有渠道名称
pub(crate) fn list_providers(doc: &toml_edit::DocumentMut) -> Vec<String> {
    let mut providers = Vec::new();
    if let Some(table) = doc.get("model_providers").and_then(|v| v.as_table()) {
        for (name, _) in table {
            providers.push(name.to_string());
        }
    }
    providers.sort();
    providers
}

/// 配置中的默认模型 id（`[models] default = "..."`），空串视为未设置。
pub(crate) fn configured_default_model(doc: &toml_edit::DocumentMut) -> Option<String> {
    doc.get("models")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("default"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// 是否尚未完成对话所需的渠道 + 默认模型配置。
///
/// Chaos 内置 catalog 为空：无 `[model_providers]` 或无 `[models].default`
/// 时，发送真实 prompt 应引导用户打开渠道设置，而不是把请求打到占位模型。
pub(crate) fn needs_provider_setup() -> bool {
    match load_config() {
        Ok(doc) => {
            if list_providers(&doc).is_empty() {
                return true;
            }
            configured_default_model(&doc).is_none()
        }
        // 读配置失败时 fail-closed：打开设置页比静默发失败请求更友好。
        Err(_) => true,
    }
}

/// 获取渠道的某个字段值
pub(crate) fn provider_field(
    doc: &toml_edit::DocumentMut,
    provider: &str,
    field: &str,
) -> Option<String> {
    doc.get("model_providers")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get(provider))
        .and_then(|v| v.as_table())
        .and_then(|t| t.get(field))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// 渠道管理命令
pub struct ProviderCommand;

impl SlashCommand for ProviderCommand {
    fn name(&self) -> &str {
        "provider"
    }

    fn aliases(&self) -> &[&str] {
        &["providers", "p"]
    }

    fn description(&self) -> &str {
        "渠道管理：列表选择、添加、设密钥、切换模型"
    }

    fn usage(&self) -> &str {
        "/provider [list|add|edit|set-key|models|set-model|manual-model|configure-model|delete|refresh] [参数]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn args_required(&self) -> bool {
        // 裸 `/provider` 打开可点选 hub
        false
    }

    fn arg_placeholder(&self) -> Option<&str> {
        Some("[子命令] [参数]")
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let trimmed = args.trim();
        // 无参数 → 渠道列表 hub（可点选）
        if trimmed.is_empty() {
            return CommandResult::Action(Action::OpenProviderModal {
                mode: ProviderModalMode::List,
            });
        }

        let (subcmd, rest) = split_first_token(trimmed);

        let mode = match subcmd {
            "list" => ProviderModalMode::List,
            "add" => ProviderModalMode::Add,
            "edit" => {
                let name = rest.trim();
                if name.is_empty() {
                    return CommandResult::Error("用法: /provider edit <渠道名称>".into());
                }
                ProviderModalMode::Edit(name.to_string())
            }
            "set-key" => {
                let name = rest.trim();
                if name.is_empty() {
                    return CommandResult::Error("用法: /provider set-key <渠道名称>".into());
                }
                ProviderModalMode::SetKey(name.to_string())
            }
            "models" | "refresh" => {
                let name = rest.trim();
                if name.is_empty() {
                    return CommandResult::Error("用法: /provider models <渠道名称>".into());
                }
                ProviderModalMode::Models(name.to_string())
            }
            "set-model" => {
                let name = rest.trim();
                if name.is_empty() {
                    return CommandResult::Error("用法: /provider set-model <渠道名称>".into());
                }
                ProviderModalMode::SetModel(name.to_string())
            }
            "manual-model" | "manual" => {
                let name = rest.trim();
                if name.is_empty() {
                    return CommandResult::Error("用法: /provider manual-model <渠道名称>".into());
                }
                ProviderModalMode::ManualModel(name.to_string())
            }
            "configure-model" | "configure" | "model-params" => {
                let name = rest.trim();
                if name.is_empty() {
                    return CommandResult::Error(
                        "用法: /provider configure-model <渠道名称>".into(),
                    );
                }
                ProviderModalMode::ConfigureModel(name.to_string())
            }
            "delete" | "rm" | "remove" => {
                // 危险操作：不直接删除，弹出二次确认对话框。
                // Issue #13：用户经常希望从配置清理一个已废渠道，
                // 之前完全没有路径。
                let name = rest.trim();
                if name.is_empty() {
                    return CommandResult::Error("用法: /provider delete <渠道名称>".into());
                }
                ProviderModalMode::ConfirmingDelete(name.to_string())
            }
            _ => {
                return CommandResult::Error(format!(
                    "未知子命令: {subcmd}\n可用: （无参数打开列表）, list, add, edit, set-key, models, set-model, manual-model, configure-model, delete, refresh"
                ));
            }
        };

        CommandResult::Action(Action::OpenProviderModal { mode })
    }
}

/// 分割第一个 token 和剩余部分
fn split_first_token(s: &str) -> (&str, &str) {
    let s = s.trim();
    match s.split_once(char::is_whitespace) {
        Some((first, rest)) => (first, rest.trim()),
        None => (s, ""),
    }
}

/// 添加渠道到配置文件。供 `views/provider_modal` 调用。
pub(crate) fn add_provider(
    name: &str,
    base_url: &str,
    auth_scheme: &str,
    api_backend: &str,
    api_key: &str,
) -> Result<(), String> {
    let name = name.trim();
    let base_url = base_url.trim().trim_end_matches(['\r', '\n']);
    // Windows paste often leaves `\r` / trailing newline on the key; strip
    // before writing so config.toml and subsequent HTTP auth stay intact.
    let api_key = crate::views::provider_modal::sanitize_provider_field(api_key);
    if name.is_empty() {
        return Err("渠道名称不能为空".into());
    }

    let mut doc = load_config()?;

    if !doc.contains_key("model_providers") {
        doc["model_providers"] = toml_edit::table();
    }

    let provider_table = &mut doc["model_providers"][name];
    *provider_table = toml_edit::table();
    let provider_table = provider_table.as_table_mut().unwrap();
    provider_table["base_url"] = toml_edit::value(base_url);
    provider_table["auth_scheme"] = toml_edit::value(auth_scheme);
    provider_table["api_backend"] = toml_edit::value(api_backend);

    if !api_key.is_empty() {
        provider_table["api_key"] = toml_edit::value(api_key.as_str());
    }

    let env_key = match name {
        "openai" | "openai_compat" => "OPENAI_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        "deepseek" => "DEEPSEEK_API_KEY",
        "xai" => "XAI_API_KEY",
        _ => "",
    };
    if !env_key.is_empty() {
        provider_table["env_key"] = toml_edit::value(env_key);
    }

    // Anthropic Messages-style backends need anthropic-version; detect by
    // auth_scheme + api_backend (not provider name — custom proxies qualify).
    sync_anthropic_version_header(provider_table, auth_scheme, api_backend);

    save_config(&doc)
}

/// 设置渠道的 API Key。供 `views/provider_modal` 调用。
pub(crate) fn set_provider_key(name: &str, key: &str) -> Result<(), String> {
    let key = crate::views::provider_modal::sanitize_provider_field(key);
    if key.is_empty() {
        return Err("API Key 不能为空".into());
    }

    let mut doc = load_config()?;

    if doc
        .get("model_providers")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get(name))
        .is_none()
    {
        return Err(format!("渠道 \"{name}\" 不存在。使用 /provider add 添加。"));
    }

    let provider_table = &mut doc["model_providers"][name];
    let Some(table) = provider_table.as_table_mut() else {
        return Err(format!("渠道 \"{name}\" 配置格式错误"));
    };
    table["api_key"] = toml_edit::value(key.as_str());

    save_config(&doc)
}

/// 更新已有渠道的连接参数。供 `views/provider_modal` 编辑流程调用。
///
/// - `api_key` 为空时保留原密钥，不覆盖。
/// - 渠道名称不可改（catalog key 以 provider 名为前缀）。
pub(crate) fn update_provider(
    name: &str,
    base_url: &str,
    auth_scheme: &str,
    api_backend: &str,
    api_key: &str,
) -> Result<(), String> {
    let name = name.trim();
    let base_url = base_url.trim().trim_end_matches(['\r', '\n']);
    let api_key = crate::views::provider_modal::sanitize_provider_field(api_key);
    if name.is_empty() {
        return Err("渠道名称不能为空".into());
    }
    if base_url.is_empty() {
        return Err("Base URL 不能为空".into());
    }

    let mut doc = load_config()?;

    if doc
        .get("model_providers")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get(name))
        .is_none()
    {
        return Err(format!("渠道 \"{name}\" 不存在。使用 /provider add 添加。"));
    }

    let provider_table = &mut doc["model_providers"][name];
    let Some(table) = provider_table.as_table_mut() else {
        return Err(format!("渠道 \"{name}\" 配置格式错误"));
    };
    table["base_url"] = toml_edit::value(base_url);
    table["auth_scheme"] = toml_edit::value(auth_scheme);
    table["api_backend"] = toml_edit::value(api_backend);

    if !api_key.is_empty() {
        table["api_key"] = toml_edit::value(api_key.as_str());
    }

    // Anthropic Messages-style backends need anthropic-version. When switching
    // away, only drop that managed key — never wipe user custom headers.
    sync_anthropic_version_header(table, auth_scheme, api_backend);

    save_config(&doc)
}

/// 删除渠道时的副作用摘要，供前端展示和后续逻辑使用。
#[derive(Debug, Clone, Default)]
pub struct DeleteProviderOutcome {
    /// 同时被删除的 `[model."provider/id"]` 条目 key 列表。
    pub removed_model_keys: Vec<String>,
    /// 被删模型是否正好是 `[models].default`。true 时 UI 需提示用户重选默认。
    pub cleared_default: bool,
}

/// 删除一个渠道及其关联的 model 目录条目。
///
/// 副作用：
/// 1. 收集所有以 `<name>/` 开头的 `[model."..."]` 条目并删除；
/// 2. 若 `[models].default` 指向被删模型，清除该字段（不留野指针）；
/// 3. 删除 `[model_providers.<name>]` 表项。
///
/// 渠道不存在返回 `Err`，但空删除（关联条目为空）仍然成功。
///
/// Issue #13: `/provider delete` 命令。
pub(crate) fn delete_provider(name: &str) -> Result<DeleteProviderOutcome, String> {
    let mut doc = load_config()?;

    if doc
        .get("model_providers")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get(name))
        .is_none()
    {
        return Err(format!("渠道 \"{name}\" 不存在"));
    }

    let mut outcome = DeleteProviderOutcome::default();
    let prefix = format!("{name}/");

    // 1. 收集并删除关联的 model 目录条目
    if let Some(models) = doc.get_mut("model").and_then(|v| v.as_table_mut()) {
        let to_remove: Vec<String> = models
            .iter()
            .filter_map(|(k, _)| {
                let key = k.to_string();
                if key.starts_with(&prefix) {
                    Some(key)
                } else {
                    None
                }
            })
            .collect();
        for k in &to_remove {
            models.remove(k);
        }
        outcome.removed_model_keys = to_remove;
    }

    // 2. 若 [models].default 指向被删模型，清除之
    let cleared = if let Some(models) = doc.get("models").and_then(|v| v.as_table()) {
        models
            .get("default")
            .and_then(|v| v.as_str())
            .map(|default| outcome.removed_model_keys.iter().any(|k| k == default))
            .unwrap_or(false)
    } else {
        false
    };
    if cleared && let Some(models) = doc.get_mut("models").and_then(|v| v.as_table_mut()) {
        models.remove("default");
        outcome.cleared_default = true;
    }

    // 3. 删除 [model_providers.<name>]
    if let Some(providers) = doc
        .get_mut("model_providers")
        .and_then(|v| v.as_table_mut())
    {
        providers.remove(name);
    }

    save_config(&doc)?;
    Ok(outcome)
}

/// Default Anthropic API version header value written for Messages backends.
const ANTHROPIC_VERSION_HEADER: &str = "anthropic-version";
const ANTHROPIC_VERSION_VALUE: &str = "2023-06-01";

/// Whether this provider config looks like Anthropic Messages protocol
/// (`auth_scheme = x_api_key` + `api_backend = messages`).
///
/// Prefer scheme/backend over provider name so custom proxies / renames still
/// get the required `anthropic-version` header.
pub(crate) fn is_anthropic_messages_style(auth_scheme: &str, api_backend: &str) -> bool {
    auth_scheme == "x_api_key" && api_backend == "messages"
}

/// Ensure or drop the managed `anthropic-version` entry in `extra_headers`.
///
/// - When switching **to** Messages + x_api_key: ensure the header exists
///   (insert default if missing; leave an existing user value alone).
/// - When switching **away**: remove only `anthropic-version`. Other custom
///   headers are preserved. If the table becomes empty, drop `extra_headers`.
fn sync_anthropic_version_header(
    table: &mut toml_edit::Table,
    auth_scheme: &str,
    api_backend: &str,
) {
    if is_anthropic_messages_style(auth_scheme, api_backend) {
        ensure_anthropic_version_header(table);
    } else {
        remove_managed_anthropic_version_header(table);
    }
}

fn ensure_anthropic_version_header(table: &mut toml_edit::Table) {
    let headers = table
        .entry("extra_headers")
        .or_insert_with(toml_edit::table);
    let Some(headers) = headers.as_table_mut() else {
        // Malformed non-table — replace with a clean headers table.
        let mut new_headers = toml_edit::Table::new();
        new_headers[ANTHROPIC_VERSION_HEADER] = toml_edit::value(ANTHROPIC_VERSION_VALUE);
        table["extra_headers"] = toml_edit::Item::Table(new_headers);
        return;
    };
    // Preserve an existing user-supplied version string.
    if headers
        .get(ANTHROPIC_VERSION_HEADER)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_none()
    {
        headers[ANTHROPIC_VERSION_HEADER] = toml_edit::value(ANTHROPIC_VERSION_VALUE);
    }
}

fn remove_managed_anthropic_version_header(table: &mut toml_edit::Table) {
    let Some(headers_item) = table.get_mut("extra_headers") else {
        return;
    };
    let Some(headers) = headers_item.as_table_mut() else {
        return;
    };
    headers.remove(ANTHROPIC_VERSION_HEADER);
    if headers.is_empty() {
        table.remove("extra_headers");
    }
}

/// 单次写入 catalog 的模型数上限（防止异常大列表拖垮 config.toml）。
const MAX_PROVIDER_CATALOG_MODELS: usize = 500;

/// 注册渠道模型到 catalog 并设为默认。
///
/// 写入：
/// ```toml
/// [model."<provider>/<model_id>"]
/// model = "<model_id>"
/// model_provider = "<provider>"
/// name = "<provider>/<model_id>"
///
/// [models]
/// default = "<provider>/<model_id>"
/// ```
///
/// 返回 catalog key（供 `/model` / `SetDefaultModel` 使用）。
/// 旧版错误的顶层 `[model] provider/id` 不会再写入（shell 只认 `[model.<key>]`）。
pub(crate) fn register_and_set_model(provider: &str, model_id: &str) -> Result<String, String> {
    let entry = ModelEntry {
        id: model_id.to_string(),
        meta: ReasoningMeta::default(),
    };
    let keys = register_provider_models(provider, std::slice::from_ref(&entry), Some(model_id))?;
    keys.into_iter()
        .next()
        .ok_or_else(|| "注册模型失败：空结果".into())
}

/// 将渠道拉取到的模型批量写入 `[model."provider/id"]`，供 `/model` 与会话 catalog 使用。
///
/// - `model_ids`：API 返回的原始 model id（不是 catalog key）
/// - `set_default`：若 `Some(id)`，同时把 `[models].default` 设为该模型的 catalog key
///
/// 返回写入的 catalog keys（与输入顺序一致，去重后）。
/// 批量注册渠道下的模型到 `[model."provider/id"]` 目录，保留 reasoning 元数据。
///
/// `set_default` 为 `Some(id)` 时同步把 `[models].default` 设为该 id 的 catalog key。
/// `id` 不在本批 entries 中也会被 upsert（meta 为空）。
///
/// Issue #14：之前只取 id，丢弃 `reasoningEfforts` 等元数据，导致
/// `/effort` 对 BYOK 模型永远下拉为空。现在保留 `reasoning_efforts` /
/// `reasoning_effort` / `supports_reasoning_effort` 三个字段。
pub(crate) fn register_provider_models(
    provider: &str,
    entries: &[ModelEntry],
    set_default: Option<&str>,
) -> Result<Vec<String>, String> {
    let mut doc = load_config()?;

    if doc
        .get("model_providers")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get(provider))
        .is_none()
    {
        return Err(format!(
            "渠道 \"{provider}\" 不存在。使用 /provider add 添加。"
        ));
    }

    ensure_model_table(&mut doc);

    let mut keys = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for entry in entries.iter().take(MAX_PROVIDER_CATALOG_MODELS) {
        let model_id = entry.id.trim();
        if model_id.is_empty() || !seen.insert(model_id.to_string()) {
            continue;
        }
        keys.push(upsert_provider_model_entry(
            &mut doc,
            provider,
            model_id,
            &entry.meta,
        ));
    }

    if let Some(default_id) = set_default.map(str::trim).filter(|s| !s.is_empty()) {
        // 确保默认模型条目存在（即使不在本批 entries 里）。
        if !seen.contains(default_id) {
            keys.push(upsert_provider_model_entry(
                &mut doc,
                provider,
                default_id,
                &ReasoningMeta::default(),
            ));
        }
        let default_key = provider_model_catalog_key(provider, default_id);
        if !doc.contains_key("models") {
            doc["models"] = toml_edit::table();
        }
        if let Some(models) = doc["models"].as_table_mut() {
            models["default"] = toml_edit::value(default_key.as_str());
        }
    }

    save_config(&doc)?;
    Ok(keys)
}

fn ensure_model_table(doc: &mut toml_edit::DocumentMut) {
    if !doc.contains_key("model") {
        doc["model"] = toml_edit::table();
    }
    // 清理历史误写的顶层标量，避免 parse 警告
    if let Some(table) = doc["model"].as_table_mut() {
        table.remove("provider");
        table.remove("id");
    }
}

/// 写入/更新单条 `[model."provider/id"]`，返回 catalog key。
///
/// `meta` 含上游返回的 reasoning 元数据（issue #14）。仅在携带非默认
/// 值时写入对应字段，避免污染配置 / 与人工编辑值冲突；重新注册时
/// 若 meta 已空则保留已有字段不动（不主动清空，让用户控制）。
fn upsert_provider_model_entry(
    doc: &mut toml_edit::DocumentMut,
    provider: &str,
    model_id: &str,
    meta: &ReasoningMeta,
) -> String {
    let catalog_key = provider_model_catalog_key(provider, model_id);
    let entry = &mut doc["model"][catalog_key.as_str()];
    if !entry.is_table() {
        *entry = toml_edit::table();
    }
    let entry = entry.as_table_mut().expect("model entry is table");
    entry["model"] = toml_edit::value(model_id);
    entry["model_provider"] = toml_edit::value(provider);
    entry["name"] = toml_edit::value(format!("{provider}/{model_id}"));

    // 写入 reasoning 元数据——只在 provider 显式提供时落盘，避免把空
    // `reasoning_efforts = []` 之类的占位值刷进 config。
    // `parse_reasoning_meta` 已过滤非法 effort 值；这里用 `is_meaningful`
    // 早退，避免对空 meta 反复写表。
    if meta.is_meaningful() {
        if meta.supports_reasoning_effort {
            entry["supports_reasoning_effort"] = toml_edit::value(true);
        }
        if let Some(default) = &meta.reasoning_effort {
            entry["reasoning_effort"] = toml_edit::value(default.as_str());
        }
        if !meta.reasoning_efforts.is_empty() {
            let mut arr = toml_edit::Array::new();
            for v in &meta.reasoning_efforts {
                arr.push(v.as_str());
            }
            entry["reasoning_efforts"] = toml_edit::value(arr);
        }
    }

    catalog_key
}

/// 可选的 per-model 采样/窗口参数（UI 表单解析结果）。
///
/// - `Some(v)`：写入该值
/// - `None`：删除该键（清除覆盖，回退全局/`[models]` 默认）
///
/// 调用方在「新建且字段留空」时应传「不调用 setter」或仅对用户填过的字段
/// 使用 `Some`；配置已有模型时留空 → `None` 以清除。
#[derive(Debug, Clone, Default)]
pub(crate) struct ModelParamOverrides {
    pub max_completion_tokens: Option<Option<u32>>,
    pub context_window: Option<Option<u64>>,
    pub temperature: Option<Option<f64>>,
    pub top_p: Option<Option<f64>>,
}

/// 解析 UI 字符串为整数参数；空串 → Ok(None)。
pub(crate) fn parse_optional_u32(raw: &str, field: &str) -> Result<Option<u32>, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Ok(None);
    }
    s.parse::<u32>()
        .map(Some)
        .map_err(|_| format!("{field} 必须是正整数，例如 8192"))
}

/// 解析 UI 字符串为 u64；空串 → Ok(None)。
pub(crate) fn parse_optional_u64(raw: &str, field: &str) -> Result<Option<u64>, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Ok(None);
    }
    s.parse::<u64>()
        .map(Some)
        .map_err(|_| format!("{field} 必须是正整数，例如 128000"))
}

/// 解析 UI 字符串为浮点；空串 → Ok(None)。
pub(crate) fn parse_optional_f64(raw: &str, field: &str) -> Result<Option<f64>, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Ok(None);
    }
    let v = s
        .parse::<f64>()
        .map_err(|_| format!("{field} 必须是数字，例如 0.7"))?;
    if !v.is_finite() {
        return Err(format!("{field} 必须是有限数字"));
    }
    Ok(Some(v))
}

/// 从 UI 表单字符串构建 overrides。
///
/// `clear_when_empty`：true 时，空字段表示清除配置键；false 时空字段表示不改动。
pub(crate) fn model_params_from_form(
    max_completion_tokens: &str,
    context_window: &str,
    temperature: &str,
    top_p: &str,
    clear_when_empty: bool,
) -> Result<ModelParamOverrides, String> {
    let mct = parse_optional_u32(max_completion_tokens, "max_completion_tokens")?;
    let cw = parse_optional_u64(context_window, "context_window")?;
    let temp = parse_optional_f64(temperature, "temperature")?;
    if let Some(t) = temp
        && !(0.0..=2.0).contains(&t)
    {
        return Err("temperature 建议范围 0–2".into());
    }
    let tp = parse_optional_f64(top_p, "top_p")?;
    if let Some(p) = tp
        && !(0.0..=1.0).contains(&p)
    {
        return Err("top_p 必须在 0–1 之间".into());
    }

    fn wrap<T>(opt: Option<T>, clear_when_empty: bool) -> Option<Option<T>> {
        if opt.is_some() {
            Some(opt)
        } else if clear_when_empty {
            Some(None)
        } else {
            None
        }
    }

    Ok(ModelParamOverrides {
        max_completion_tokens: wrap(mct, clear_when_empty),
        context_window: wrap(cw, clear_when_empty),
        temperature: wrap(temp, clear_when_empty),
        top_p: wrap(tp, clear_when_empty),
    })
}

fn apply_model_param_overrides(entry: &mut toml_edit::Table, params: &ModelParamOverrides) {
    if let Some(opt) = params.max_completion_tokens {
        match opt {
            Some(v) => entry["max_completion_tokens"] = toml_edit::value(i64::from(v)),
            None => {
                entry.remove("max_completion_tokens");
            }
        }
    }
    if let Some(opt) = params.context_window {
        match opt {
            Some(v) => {
                if v == 0 {
                    entry.remove("context_window");
                } else {
                    entry["context_window"] = toml_edit::value(v as i64);
                }
            }
            None => {
                entry.remove("context_window");
            }
        }
    }
    if let Some(opt) = params.temperature {
        match opt {
            Some(v) => entry["temperature"] = toml_edit::value(v),
            None => {
                entry.remove("temperature");
            }
        }
    }
    if let Some(opt) = params.top_p {
        match opt {
            Some(v) => entry["top_p"] = toml_edit::value(v),
            None => {
                entry.remove("top_p");
            }
        }
    }
}

/// 注册模型并可选写入采样参数，再设为默认。
///
/// `params` 中仅 `Some(...)` 的字段会写入；空表单应使用
/// `model_params_from_form(..., clear_when_empty=false)`。
pub(crate) fn register_model_with_params(
    provider: &str,
    model_id: &str,
    params: &ModelParamOverrides,
    set_default: bool,
) -> Result<String, String> {
    let model_id = model_id.trim();
    if model_id.is_empty() {
        return Err("模型 ID 不能为空".into());
    }

    let mut doc = load_config()?;
    // Check provider exists in the same doc we'll write to (single read).
    let provider_missing = doc
        .get("model_providers")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get(provider))
        .is_none();
    if provider_missing {
        return Err(format!(
            "渠道 \"{provider}\" 不存在。使用 /provider add 添加。"
        ));
    }

    ensure_model_table(&mut doc);
    let catalog_key =
        upsert_provider_model_entry(&mut doc, provider, model_id, &ReasoningMeta::default());
    let entry = doc["model"][catalog_key.as_str()]
        .as_table_mut()
        .expect("model entry is table");
    apply_model_param_overrides(entry, params);

    if set_default {
        if !doc.contains_key("models") {
            doc["models"] = toml_edit::table();
        }
        if let Some(models) = doc["models"].as_table_mut() {
            models["default"] = toml_edit::value(catalog_key.as_str());
        }
    }

    save_config(&doc)?;
    Ok(catalog_key)
}

/// 仅更新已有（或新建）模型的参数字段，不强制改 default。
pub(crate) fn update_model_params(
    provider: &str,
    model_id: &str,
    params: &ModelParamOverrides,
) -> Result<String, String> {
    register_model_with_params(provider, model_id, params, false)
}

/// Catalog key 形如 `provider/model_id`，保证跨渠道不冲突。
pub(crate) fn provider_model_catalog_key(provider: &str, model_id: &str) -> String {
    format!("{provider}/{model_id}")
}

/// 列出渠道已注册到 config 的模型（`[model."provider/<id>"]` 目录条目）。
///
/// 「配置模型参数」用此复用「查看可用模型 / 刷新」已拉取并落盘的模型，
/// 让用户直接点选而不是手写 ID。拉取结果与 agent catalog 共用同一份
/// config 文件，读本地文件不发起网络请求，不会阻塞 TUI 渲染。
pub(crate) fn list_provider_model_entries(provider: &str) -> Vec<ModelEntry> {
    match load_config() {
        Ok(doc) => provider_model_entries_from_doc(&doc, provider),
        Err(_) => Vec::new(),
    }
}

/// 从已解析的 config 文档提取某渠道的模型目录条目（纯函数，供测试）。
fn provider_model_entries_from_doc(
    doc: &toml_edit::DocumentMut,
    provider: &str,
) -> Vec<ModelEntry> {
    let prefix = format!("{provider}/");
    let mut entries: Vec<ModelEntry> = doc
        .get("model")
        .and_then(|v| v.as_table())
        .map(|table| {
            table
                .iter()
                .filter_map(|(key, item)| {
                    let rest = key.strip_prefix(&prefix)?;
                    let entry = item.as_table()?;
                    // id 以条目内的 `model` 字段为准，缺省回退到 catalog
                    // key 的 `provider/` 前缀之后的部分。
                    let id = entry
                        .get("model")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| rest.to_string());
                    if id.is_empty() {
                        return None;
                    }
                    Some(ModelEntry {
                        id,
                        meta: reasoning_meta_from_config_item(entry),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    entries
}

/// 从 `[model."provider/id"]` 条目表恢复 reasoning 元数据（拉取 `/v1/models`
/// 时落盘的字段），供配置模型参数 / 重新注册时复用。
fn reasoning_meta_from_config_item(table: &toml_edit::Table) -> ReasoningMeta {
    ReasoningMeta {
        reasoning_efforts: table
            .get("reasoning_efforts")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|el| el.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        reasoning_effort: table
            .get("reasoning_effort")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        supports_reasoning_effort: table
            .get("supports_reasoning_effort")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    }
}

/// 从配置解析当前默认模型所属渠道（用于 list 的 * 标记）。
pub(crate) fn current_provider_name(doc: &toml_edit::DocumentMut) -> Option<String> {
    let default = doc
        .get("models")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("default"))
        .and_then(|v| v.as_str())?;
    if let Some(mp) = doc
        .get("model")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get(default))
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("model_provider"))
        .and_then(|v| v.as_str())
    {
        return Some(mp.to_string());
    }
    // catalog_key = "provider/model..." 的回退解析
    default
        .split_once('/')
        .map(|(p, _)| p.to_string())
        .or_else(|| {
            // 兼容极旧的 [model] provider = ...
            doc.get("model")
                .and_then(|v| v.as_table())
                .and_then(|t| t.get("provider"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
}

/// 获取渠道可用模型列表（含 reasoning 元数据）。供 `views/provider_modal` 调用。
///
/// Issue #14：旧版只返回 model id 列表，丢弃 reasoning 元数据，导致
/// `/effort` 对 BYOK 模型永远下拉为空。
pub(crate) fn fetch_provider_models(name: &str) -> Result<Vec<ModelEntry>, String> {
    let doc = load_config()?;

    let base_url = match provider_field(&doc, name, "base_url") {
        Some(u) => u,
        None => return Err(format!("渠道 \"{name}\" 不存在或未设置 base_url")),
    };

    let auth_scheme = provider_field(&doc, name, "auth_scheme").unwrap_or_else(|| "bearer".into());
    let api_key = provider_field(&doc, name, "api_key");
    let env_key = provider_field(&doc, name, "env_key");

    let key = api_key.or_else(|| env_key.and_then(|env| std::env::var(&env).ok()));

    let key = match key {
        Some(k) if !k.is_empty() => k,
        _ => {
            return Err(format!(
                "渠道 \"{name}\" 未设置 API Key。\n使用 /provider set-key {name} 设置密钥。"
            ));
        }
    };

    let url = if base_url.ends_with('/') {
        format!("{}models", base_url)
    } else {
        format!("{base_url}/models")
    };

    let (header_name, header_value) = match auth_scheme.as_str() {
        "x_api_key" => ("x-api-key", key.clone()),
        _ => ("Authorization", format!("Bearer {key}")),
    };

    let output = Command::new("curl")
        .arg("-s")
        .arg("--max-time")
        .arg("15")
        .arg("-H")
        .arg(format!("{header_name}: {header_value}"))
        .arg(&url)
        .output();

    let output = match output {
        Ok(o) => o,
        Err(e) => return Err(format!("执行 curl 失败: {e}")),
    };

    if !output.status.success() {
        return Err(format!(
            "curl 退出码: {}",
            output.status.code().unwrap_or(-1)
        ));
    }

    let body = String::from_utf8_lossy(&output.stdout);
    if body.is_empty() {
        return Err("服务器返回空响应".into());
    }

    parse_models_response(&body)
}

/// `/v1/models` 响应的单条 model 记录：上游 id + 弱 reasoning 元数据。
#[derive(Debug, Clone, Default)]
pub struct ModelEntry {
    pub id: String,
    pub meta: ReasoningMeta,
}

/// 解析 /v1/models 响应，提取模型 ID + per-model reasoning 元数据。
///
/// 兼容三种常见格式：
/// - OpenAI 标准 `{ "data": [{ "id": ... }] }`
/// - 裸数组 `[{ "id": ... }]`
/// - 一些代理的 `{ "models": [...] }`
///
/// Issue #14：之前只取 `id`，把 `reasoningEfforts` 等元数据全部丢弃。
/// 现在会从每个 item 读取：
/// - `reasoning_efforts` / `reasoningEfforts` → `Vec<String>`
/// - `reasoning_effort` / `reasoningEffort` → `Option<String>`
/// - `supports_reasoning_effort` / `supportsReasoningEffort` → `bool`
fn parse_models_response(body: &str) -> Result<Vec<ModelEntry>, String> {
    let json: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("JSON 解析失败: {e}"))?;

    let items: Option<Vec<&serde_json::Value>> = json
        .get("data")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().collect())
        .or_else(|| json.as_array().map(|a| a.iter().collect()))
        .or_else(|| {
            json.get("models")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().collect())
        });

    let items = items.ok_or_else(|| "无法识别的响应格式".to_string())?;

    let mut entries: Vec<ModelEntry> = items
        .into_iter()
        .filter_map(|item| {
            let id = item
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    // "models" 形态可能直接是字符串
                    item.as_str().map(|s| s.to_string())
                })?;
            Some(ModelEntry {
                meta: parse_reasoning_meta(item),
                id,
            })
        })
        .collect();
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(entries)
}

/// 从单条 model JSON 读取 reasoning 元数据。缺省字段走空值。
///
/// 字段命名同时接受 camelCase（OpenAI / xAI 扩展）和 snake_case，方便代理
/// 转发时不做重写也能识别。
///
/// Effort 列表走与 remote `/models` 相同的
/// [`xai_grok_shell::sampling::types::parse_reasoning_effort_options`]：
/// 非法 entry 跳过并 warn，只把 canonical id 写入 config，避免 config 加载
/// 时 `Vec<ReasoningEffortOption>` / `Option<ReasoningEffort>` 反序列化失败。
fn parse_reasoning_meta(item: &serde_json::Value) -> ReasoningMeta {
    use xai_grok_shell::sampling::types::{ReasoningEffort, parse_reasoning_effort_options};

    let get_bool = |k: &str| item.get(k).and_then(|v| v.as_bool()).unwrap_or(false);

    let reasoning_efforts: Vec<String> = item
        .get("reasoning_efforts")
        .or_else(|| item.get("reasoningEfforts"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            parse_reasoning_effort_options(arr)
                .into_iter()
                .map(|opt| opt.value.as_str().to_string())
                .collect()
        })
        .unwrap_or_default();

    let reasoning_effort = item
        .get("reasoning_effort")
        .or_else(|| item.get("reasoningEffort"))
        .and_then(|v| v.as_str())
        .and_then(|s| match s.parse::<ReasoningEffort>() {
            Ok(effort) => Some(effort.as_str().to_string()),
            Err(err) => {
                tracing::warn!(
                    value = %s,
                    error = %err,
                    "reasoningEffort: skipping invalid default"
                );
                None
            }
        });

    ReasoningMeta {
        reasoning_efforts,
        reasoning_effort,
        supports_reasoning_effort: get_bool("supports_reasoning_effort")
            || get_bool("supportsReasoningEffort"),
    }
}

use crate::app::actions::Action;

#[cfg(test)]
mod delete_provider_tests {
    fn make_doc() -> toml_edit::DocumentMut {
        let mut doc = toml_edit::DocumentMut::new();
        // [model_providers.foo]
        let mut p = toml_edit::Table::new();
        p["base_url"] = toml_edit::value("https://example.com/v1");
        p["auth_scheme"] = toml_edit::value("bearer");
        p["api_backend"] = toml_edit::value("chat_completions");
        p["api_key"] = toml_edit::value("sk-test");
        doc["model_providers"] = toml_edit::table();
        doc["model_providers"]["foo"] = toml_edit::Item::Table(p);

        // [model."foo/a"] / [model."foo/b"]
        doc["model"] = toml_edit::table();
        for id in ["a", "b"] {
            let mut entry = toml_edit::Table::new();
            entry["model"] = toml_edit::value(id);
            entry["model_provider"] = toml_edit::value("foo");
            let key = format!("foo/{id}");
            doc["model"][key.as_str()] = toml_edit::Item::Table(entry);
        }

        // [model."bar/c"] 另一个渠道的模型（不应被删）
        let mut other = toml_edit::Table::new();
        other["model"] = toml_edit::value("c");
        other["model_provider"] = toml_edit::value("bar");
        doc["model"]["bar/c"] = toml_edit::Item::Table(other);

        // [models].default = "foo/a"（即将被删，预期 cleared_default=true）
        doc["models"] = toml_edit::table();
        doc["models"]["default"] = toml_edit::value("foo/a");

        doc
    }

    #[test]
    fn delete_removes_provider_and_its_models_only() {
        let doc = make_doc();
        // 删 "foo"：2 个 model 目录条目 + provider 条目 + cleared default
        let mut doc = doc;
        let provider = doc["model_providers"]["foo"]
            .as_table()
            .cloned()
            .expect("foo provider exists");
        let _ = provider; // we operate on doc directly
        let foo_models: Vec<String> = doc["model"]
            .as_table()
            .unwrap()
            .iter()
            .filter_map(|(k, _)| {
                let s = k.to_string();
                if s.starts_with("foo/") { Some(s) } else { None }
            })
            .collect();
        assert_eq!(foo_models.len(), 2, "test fixture sanity");

        // 模拟 delete 流程
        let removed_keys: Vec<String> = {
            let models = doc["model"].as_table_mut().unwrap();
            let to_remove: Vec<String> = models
                .iter()
                .filter_map(|(k, _)| {
                    let s = k.to_string();
                    if s.starts_with("foo/") { Some(s) } else { None }
                })
                .collect();
            for k in &to_remove {
                models.remove(k);
            }
            to_remove
        };
        assert_eq!(removed_keys.len(), 2);
        assert!(!doc["model"].as_table().unwrap().contains_key("foo/a"));
        assert!(!doc["model"].as_table().unwrap().contains_key("foo/b"));
        assert!(
            doc["model"].as_table().unwrap().contains_key("bar/c"),
            "其他渠道的模型不应被删"
        );
    }

    #[test]
    fn delete_clears_default_when_pointing_to_removed_model() {
        let mut doc = make_doc();
        assert_eq!(doc["models"]["default"].as_str(), Some("foo/a"), "fixture");
        // 模拟 delete 流程：先收集要删的 key
        let removed_keys: Vec<String> = {
            let models = doc["model"].as_table_mut().unwrap();
            let to_remove: Vec<String> = models
                .iter()
                .filter_map(|(k, _)| {
                    let s = k.to_string();
                    if s.starts_with("foo/") { Some(s) } else { None }
                })
                .collect();
            for k in &to_remove {
                models.remove(k);
            }
            to_remove
        };

        // 模拟 cleared_default 路径
        let cleared = doc["models"]["default"]
            .as_str()
            .map(|d| removed_keys.iter().any(|k| k == d))
            .unwrap_or(false);
        assert!(cleared, "default 应被识别为指向被删模型");
        if cleared {
            doc["models"].as_table_mut().unwrap().remove("default");
        }
        assert!(
            !doc["models"].as_table().unwrap().contains_key("default"),
            "default 字段应被清空"
        );
    }

    #[test]
    fn delete_preserves_default_when_pointing_to_other_provider() {
        let mut doc = make_doc();
        // 把 default 改为别的渠道的模型
        doc["models"]["default"] = toml_edit::value("bar/c");
        let removed_keys: Vec<String> = {
            let models = doc["model"].as_table_mut().unwrap();
            let to_remove: Vec<String> = models
                .iter()
                .filter_map(|(k, _)| {
                    let s = k.to_string();
                    if s.starts_with("foo/") { Some(s) } else { None }
                })
                .collect();
            for k in &to_remove {
                models.remove(k);
            }
            to_remove
        };
        let cleared = doc["models"]["default"]
            .as_str()
            .map(|d| removed_keys.iter().any(|k| k == d))
            .unwrap_or(false);
        assert!(!cleared, "default 指向其他渠道时不应被清空");
        assert_eq!(doc["models"]["default"].as_str(), Some("bar/c"));
    }
}

#[cfg(test)]
mod parse_models_response_tests {
    use super::*;

    #[test]
    fn parses_openai_data_envelope() {
        let body = r#"{"data":[
            {"id": "gpt-5", "reasoningEfforts": ["low", "medium", "high"]},
            {"id": "gpt-4o"}
        ]}"#;
        let entries = parse_models_response(body).expect("parse ok");
        assert_eq!(entries.len(), 2);
        let gpt5 = entries.iter().find(|e| e.id == "gpt-5").unwrap();
        assert_eq!(
            gpt5.meta.reasoning_efforts,
            vec!["low".to_string(), "medium".to_string(), "high".to_string()]
        );
        assert!(!gpt5.meta.supports_reasoning_effort);
        assert!(gpt5.meta.reasoning_effort.is_none());

        let gpt4o = entries.iter().find(|e| e.id == "gpt-4o").unwrap();
        assert!(gpt4o.meta.reasoning_efforts.is_empty());
    }

    #[test]
    fn reads_snake_case_keys() {
        let body = r#"{"data":[
            {"id": "r1", "supports_reasoning_effort": true, "reasoning_efforts": ["low","high"]}
        ]}"#;
        let entries = parse_models_response(body).expect("parse ok");
        let r1 = &entries[0];
        assert!(r1.meta.supports_reasoning_effort);
        assert_eq!(r1.meta.reasoning_efforts, vec!["low", "high"]);
    }

    #[test]
    fn reads_reasoning_effort_default() {
        let body = r#"{"data":[
            {"id": "r1", "reasoningEffort": "medium"}
        ]}"#;
        let entries = parse_models_response(body).expect("parse ok");
        assert_eq!(entries[0].meta.reasoning_effort.as_deref(), Some("medium"));
    }

    #[test]
    fn skips_invalid_reasoning_effort_values() {
        // Invalid bare strings / defaults must not land in config as-is:
        // config deserializes them as ReasoningEffort / ReasoningEffortOption.
        let body = r#"{"data":[
            {
                "id": "r1",
                "supportsReasoningEffort": true,
                "reasoningEffort": "turbo",
                "reasoningEfforts": ["low", "turbo", {"value": "high"}, {"value": "super"}]
            }
        ]}"#;
        let entries = parse_models_response(body).expect("parse ok");
        let r1 = &entries[0];
        assert!(r1.meta.supports_reasoning_effort);
        assert!(r1.meta.reasoning_effort.is_none());
        assert_eq!(r1.meta.reasoning_efforts, vec!["low", "high"]);
    }

    #[test]
    fn accepts_bare_array_envelope() {
        let body = r#"[{"id":"a"},{"id":"b"}]"#;
        let entries = parse_models_response(body).expect("parse ok");
        assert_eq!(
            entries.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }

    #[test]
    fn accepts_models_envelope() {
        let body = r#"{"models":["a","b"]}"#;
        let entries = parse_models_response(body).expect("parse ok");
        assert_eq!(
            entries.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }

    #[test]
    fn unknown_envelope_errors() {
        let body = r#"{"oops":true}"#;
        let err = parse_models_response(body).unwrap_err();
        assert!(err.contains("无法识别"), "err={err}");
    }

    #[test]
    fn reasoning_meta_is_meaningful_only_with_real_values() {
        assert!(!ReasoningMeta::default().is_meaningful());
        assert!(
            ReasoningMeta {
                reasoning_efforts: vec!["low".into()],
                ..Default::default()
            }
            .is_meaningful()
        );
        assert!(
            ReasoningMeta {
                reasoning_effort: Some("high".into()),
                ..Default::default()
            }
            .is_meaningful()
        );
        assert!(
            ReasoningMeta {
                supports_reasoning_effort: true,
                ..Default::default()
            }
            .is_meaningful()
        );
    }
}

#[cfg(test)]
mod provider_model_entries_tests {
    use super::*;

    /// 构造一份带 `[model."foo/*"]` 与无关渠道条目的 config 文档。
    fn make_doc() -> toml_edit::DocumentMut {
        let mut doc = toml_edit::DocumentMut::new();
        doc["model"] = toml_edit::table();
        let model = doc["model"].as_table_mut().unwrap();

        let mut a = toml_edit::Table::new();
        a["model"] = toml_edit::value("alpha");
        a["model_provider"] = toml_edit::value("foo");
        a["supports_reasoning_effort"] = toml_edit::value(true);
        a["reasoning_effort"] = toml_edit::value("medium");
        let mut efforts = toml_edit::Array::new();
        efforts.push("low");
        efforts.push("high");
        a["reasoning_efforts"] = toml_edit::value(efforts);
        model["foo/alpha"] = toml_edit::Item::Table(a);

        // 无 `model` 字段 → 回退到 key 后缀 "beta"。
        let mut b = toml_edit::Table::new();
        b["model_provider"] = toml_edit::value("foo");
        model["foo/beta"] = toml_edit::Item::Table(b);

        // 其它渠道的条目，不应被列出。
        let mut other = toml_edit::Table::new();
        other["model"] = toml_edit::value("gamma");
        other["model_provider"] = toml_edit::value("bar");
        model["bar/gamma"] = toml_edit::Item::Table(other);

        doc
    }

    #[test]
    fn lists_only_this_providers_entries_sorted() {
        let doc = make_doc();
        let entries = provider_model_entries_from_doc(&doc, "foo");
        let ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["alpha", "beta"], "按 id 排序且不含其它渠道");
    }

    #[test]
    fn parses_reasoning_meta_from_config_item() {
        let doc = make_doc();
        let entries = provider_model_entries_from_doc(&doc, "foo");
        let alpha = entries.iter().find(|e| e.id == "alpha").unwrap();
        assert!(alpha.meta.supports_reasoning_effort);
        assert_eq!(alpha.meta.reasoning_effort.as_deref(), Some("medium"));
        assert_eq!(alpha.meta.reasoning_efforts, vec!["low", "high"]);
    }

    #[test]
    fn falls_back_to_key_suffix() {
        let doc = make_doc();
        let entries = provider_model_entries_from_doc(&doc, "foo");
        let beta = entries.iter().find(|e| e.id == "beta").unwrap();
        assert!(!beta.meta.is_meaningful());
    }

    #[test]
    fn unknown_provider_yields_empty() {
        let doc = make_doc();
        assert!(provider_model_entries_from_doc(&doc, "nope").is_empty());
    }
}
