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
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("读取配置文件失败: {e}"))?;
    content
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("解析配置文件失败: {e}"))
}

/// 保存配置文件
pub(crate) fn save_config(doc: &toml_edit::DocumentMut) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    fs::write(&path, doc.to_string())
        .map_err(|e| format!("写入配置文件失败: {e}"))
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
pub(crate) fn provider_field(doc: &toml_edit::DocumentMut, provider: &str, field: &str) -> Option<String> {
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
        "/provider [list|add|edit|set-key|models|set-model|manual-model|configure-model|refresh] [参数]"
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
            _ => {
                return CommandResult::Error(format!(
                    "未知子命令: {subcmd}\n可用: （无参数打开列表）, list, add, edit, set-key, models, set-model, manual-model, configure-model, refresh"
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
    let headers = table.entry("extra_headers").or_insert_with(toml_edit::table);
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
    let keys = register_provider_models(provider, std::slice::from_ref(&model_id.to_string()), Some(model_id))?;
    keys.into_iter().next().ok_or_else(|| "注册模型失败：空结果".into())
}

/// 将渠道拉取到的模型批量写入 `[model."provider/id"]`，供 `/model` 与会话 catalog 使用。
///
/// - `model_ids`：API 返回的原始 model id（不是 catalog key）
/// - `set_default`：若 `Some(id)`，同时把 `[models].default` 设为该模型的 catalog key
///
/// 返回写入的 catalog keys（与输入顺序一致，去重后）。
pub(crate) fn register_provider_models(
    provider: &str,
    model_ids: &[String],
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
    for raw in model_ids.iter().take(MAX_PROVIDER_CATALOG_MODELS) {
        let model_id = raw.trim();
        if model_id.is_empty() || !seen.insert(model_id.to_string()) {
            continue;
        }
        keys.push(upsert_provider_model_entry(&mut doc, provider, model_id));
    }

    if let Some(default_id) = set_default.map(str::trim).filter(|s| !s.is_empty()) {
        // 确保默认模型条目存在（即使不在本批 model_ids 里）。
        if !seen.contains(default_id) {
            keys.push(upsert_provider_model_entry(
                &mut doc, provider, default_id,
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
fn upsert_provider_model_entry(
    doc: &mut toml_edit::DocumentMut,
    provider: &str,
    model_id: &str,
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
    if let Some(t) = temp {
        if !(0.0..=2.0).contains(&t) {
            return Err("temperature 建议范围 0–2".into());
        }
    }
    let tp = parse_optional_f64(top_p, "top_p")?;
    if let Some(p) = tp {
        if !(0.0..=1.0).contains(&p) {
            return Err("top_p 必须在 0–1 之间".into());
        }
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
    let catalog_key = upsert_provider_model_entry(&mut doc, provider, model_id);
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


/// 获取渠道可用模型列表。供 `views/provider_modal` 调用。
pub(crate) fn fetch_provider_models(name: &str) -> Result<Vec<String>, String> {
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

/// 解析 /v1/models 响应，提取模型 ID 列表
fn parse_models_response(body: &str) -> Result<Vec<String>, String> {
    let json: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("JSON 解析失败: {e}"))?;

    if let Some(data) = json.get("data").and_then(|v| v.as_array()) {
        let mut models: Vec<String> = data
            .iter()
            .filter_map(|item| item.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .collect();
        models.sort();
        return Ok(models);
    }

    if let Some(arr) = json.as_array() {
        let mut models: Vec<String> = arr
            .iter()
            .filter_map(|item| item.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .collect();
        models.sort();
        return Ok(models);
    }

    if let Some(models) = json.get("models").and_then(|v| v.as_array()) {
        let mut result: Vec<String> = models
            .iter()
            .filter_map(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .or_else(|| v.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
            })
            .collect();
        result.sort();
        return Ok(result);
    }

    Err("无法识别的响应格式".into())
}

use crate::app::actions::Action;
