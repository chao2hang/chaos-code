//! `/provider` — 渠道管理命令（交互式 TUI 模态框）。
//!
//! - 裸 `/provider`（无参数）→ 渠道列表 hub：↑↓ 选择，Enter 进操作菜单，末行「+ 添加渠道」
//! - `/provider list` — 同裸命令
//! - `/provider add` — 交互式多步表单添加渠道
//! - `/provider set-key <name>` — 交互式输入 API Key
//! - `/provider models <name>` — 获取渠道可用模型列表
//! - `/provider set-model <name>` — 交互式选择并设置当前模型
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
        "/provider [list|add|set-key|models|set-model|refresh] [参数]"
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
            _ => {
                return CommandResult::Error(format!(
                    "未知子命令: {subcmd}\n可用: （无参数打开列表）, list, add, set-key, models, set-model, refresh"
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
        provider_table["api_key"] = toml_edit::value(api_key);
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

    if auth_scheme == "x_api_key" && name == "anthropic" {
        let mut headers = toml_edit::Table::new();
        headers["anthropic-version"] = toml_edit::value("2023-06-01");
        provider_table["extra_headers"] = toml_edit::Item::Table(headers);
    }

    save_config(&doc)
}

/// 设置渠道的 API Key。供 `views/provider_modal` 调用。
pub(crate) fn set_provider_key(name: &str, key: &str) -> Result<(), String> {
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
    table["api_key"] = toml_edit::value(key);

    save_config(&doc)
}

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

    let catalog_key = provider_model_catalog_key(provider, model_id);

    if !doc.contains_key("model") {
        doc["model"] = toml_edit::table();
    }
    // 清理历史误写的顶层标量，避免 parse 警告
    if let Some(table) = doc["model"].as_table_mut() {
        table.remove("provider");
        table.remove("id");
    }

    let entry = &mut doc["model"][catalog_key.as_str()];
    if !entry.is_table() {
        *entry = toml_edit::table();
    }
    let entry = entry.as_table_mut().unwrap();
    entry["model"] = toml_edit::value(model_id);
    entry["model_provider"] = toml_edit::value(provider);
    entry["name"] = toml_edit::value(format!("{provider}/{model_id}"));

    if !doc.contains_key("models") {
        doc["models"] = toml_edit::table();
    }
    if let Some(models) = doc["models"].as_table_mut() {
        models["default"] = toml_edit::value(catalog_key.as_str());
    }

    save_config(&doc)?;
    Ok(catalog_key)
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
