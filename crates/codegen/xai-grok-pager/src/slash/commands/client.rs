//! `/client` — request-client profile selection and management.
//!
//! Client profiles describe the public identity used on requests. They never
//! contain API keys; credentials remain in the provider/model configuration.

use std::collections::HashSet;

use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};
use crate::views::client_modal::ClientModalMode;
use xai_grok_shell::agent::client_profiles::{
    by_id as builtin_profile_by_id, BuiltinClientProfile, ClientProfile,
    BUILTIN_CLIENT_PROFILES,
};

const VALID_PROTOCOLS: &[&str] = &["responses", "chat_completions", "messages"];
const VALID_AUTH_SCHEMES: &[&str] = &["bearer", "x_api_key", "none"];

/// Read the `[clients]` section and return built-ins followed by custom
/// profiles. Invalid custom entries are omitted from the picker rather than
/// making a malformed optional profile prevent the TUI from opening.
pub(crate) fn list_client_profiles(
    doc: &toml_edit::DocumentMut,
) -> Vec<ClientProfile> {
    let mut profiles = BUILTIN_CLIENT_PROFILES
        .iter()
        .map(builtin_to_owned)
        .collect::<Vec<_>>();

    let builtin_ids: HashSet<&str> = BUILTIN_CLIENT_PROFILES.iter().map(|p| p.id).collect();
    let mut custom = Vec::new();
    if let Some(table) = doc
        .get("clients")
        .and_then(|item| item.as_table())
        .and_then(|clients| clients.get("custom"))
        .and_then(|item| item.as_table())
    {
        for (id, item) in table {
            if builtin_ids.contains(id) || builtin_profile_by_id(id).is_some() {
                continue;
            }
            if let Some(profile) = custom_profile_from_item(id, item) {
                custom.push(profile);
            }
        }
    }
    custom.sort_by(|a, b| a.id.cmp(&b.id));
    profiles.extend(custom);
    profiles
}

fn builtin_to_owned(profile: &BuiltinClientProfile) -> ClientProfile {
    ClientProfile {
        id: profile.id.to_owned(),
        name: profile.name.to_owned(),
        protocol: profile.protocol.to_owned(),
        auth_scheme: profile.auth_scheme.to_owned(),
        env_key: profile.env_key.to_owned(),
        client_identifier: profile.client_identifier.to_owned(),
    }
}

/// Resolve a profile from the same document used by the picker.
pub(crate) fn profile_by_id(
    doc: &toml_edit::DocumentMut,
    id: &str,
) -> Option<ClientProfile> {
    if let Some(profile) = builtin_profile_by_id(id) {
        return Some(profile);
    }
    let item = doc
        .get("clients")
        .and_then(|item| item.as_table())
        .and_then(|clients| clients.get("custom"))
        .and_then(|item| item.as_table())
        .and_then(|custom| custom.get(id.trim()))?;
    custom_profile_from_item(id.trim(), item)
}

fn custom_profile_from_item(id: &str, item: &toml_edit::Item) -> Option<ClientProfile> {
    let table = item.as_table()?;
    let text = |key: &str| {
        table
            .get(key)
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .trim()
            .to_owned()
    };
    Some(ClientProfile {
        id: id.to_owned(),
        name: non_empty_or(text("name"), id),
        protocol: non_empty_or(text("protocol"), "responses"),
        auth_scheme: non_empty_or(text("auth_scheme"), "bearer"),
        env_key: text("env_key"),
        client_identifier: non_empty_or(text("client_identifier"), id),
    })
}

fn non_empty_or(value: String, fallback: &str) -> String {
    if value.is_empty() {
        fallback.to_owned()
    } else {
        value
    }
}

/// Return the canonical configured default, or `None` when it is unset or
/// points at a profile that no longer exists.
pub(crate) fn configured_default_client(
    doc: &toml_edit::DocumentMut,
) -> Option<String> {
    let id = doc
        .get("clients")
        .and_then(|item| item.as_table())
        .and_then(|clients| clients.get("default"))
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty())?;
    profile_by_id(doc, id).map(|profile| profile.id)
}

/// Validate and normalize a custom profile before it is written to disk.
pub(crate) fn validate_custom_profile(
    profile: &ClientProfile,
) -> Result<ClientProfile, String> {
    let mut normalized = profile.clone();
    normalized.id = normalized.id.trim().to_owned();
    normalized.name = normalized.name.trim().to_owned();
    normalized.protocol = normalized.protocol.trim().to_ascii_lowercase();
    normalized.auth_scheme = normalized.auth_scheme.trim().to_ascii_lowercase();
    normalized.env_key = normalized.env_key.trim().to_owned();
    normalized.client_identifier = normalized.client_identifier.trim().to_owned();

    if !valid_client_id(&normalized.id) {
        return Err(
            "客户端 ID 必须以字母或数字开头，且只能包含字母、数字、.、_、-（最多 64 个字符）"
                .into(),
        );
    }
    if builtin_profile_by_id(&normalized.id).is_some()
        || matches!(normalized.id.as_str(), "grok-pager" | "grok-shell")
    {
        return Err(format!("客户端 ID \"{}\" 是内置保留名称", normalized.id));
    }
    if normalized.name.is_empty() || normalized.name.chars().count() > 80 {
        return Err("客户端名称不能为空且不能超过 80 个字符".into());
    }
    if normalized.name.chars().any(char::is_control) {
        return Err("客户端名称不能包含控制字符".into());
    }
    if !VALID_PROTOCOLS.contains(&normalized.protocol.as_str()) {
        return Err(format!(
            "协议必须是 {} 之一",
            VALID_PROTOCOLS.join("、")
        ));
    }
    if !VALID_AUTH_SCHEMES.contains(&normalized.auth_scheme.as_str()) {
        return Err(format!(
            "认证方式必须是 {} 之一",
            VALID_AUTH_SCHEMES.join("、")
        ));
    }
    if !normalized.env_key.is_empty() && !valid_env_key(&normalized.env_key) {
        return Err("环境变量名必须匹配 [A-Z_][A-Z0-9_]*".into());
    }
    if !valid_client_identifier(&normalized.client_identifier) {
        return Err(
            "客户端标识只能包含字母、数字、.、_、-，且不能为空（最多 128 个字符）".into(),
        );
    }
    if normalized.auth_scheme != "none" && normalized.env_key.is_empty() {
        return Err("非 none 认证方式需要填写 API Key 环境变量名".into());
    }

    Ok(normalized)
}

fn valid_client_id(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    value.len() <= 64
        && first.is_ascii_alphanumeric()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn valid_env_key(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_uppercase() || first == '_')
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn valid_client_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Insert or update one custom profile in an already-loaded TOML document.
pub(crate) fn upsert_custom_client_in_document(
    doc: &mut toml_edit::DocumentMut,
    profile: &ClientProfile,
    editing_id: Option<&str>,
) -> Result<ClientProfile, String> {
    let profile = validate_custom_profile(profile)?;
    let editing_id = editing_id.map(str::trim).filter(|id| !id.is_empty());
    if let Some(old_id) = editing_id
        && old_id != profile.id
    {
        return Err("客户端 ID 创建后不可修改；请删除后重新新增".into());
    }
    if editing_id.is_none() && profile_by_id(doc, &profile.id).is_some() {
        return Err(format!("客户端 ID \"{}\" 已存在", profile.id));
    }

    ensure_clients_custom_table(doc)?;
    let entry = &mut doc["clients"]["custom"][profile.id.as_str()];
    if !entry.is_table() {
        *entry = toml_edit::table();
    }
    let table = entry
        .as_table_mut()
        .ok_or_else(|| "客户端配置格式错误".to_owned())?;
    table["name"] = toml_edit::value(profile.name.as_str());
    table["protocol"] = toml_edit::value(profile.protocol.as_str());
    table["auth_scheme"] = toml_edit::value(profile.auth_scheme.as_str());
    table["env_key"] = toml_edit::value(profile.env_key.as_str());
    table["client_identifier"] = toml_edit::value(profile.client_identifier.as_str());

    Ok(profile)
}

/// Delete a custom profile and clear dangling default/model references.
pub(crate) fn delete_custom_client_from_document(
    doc: &mut toml_edit::DocumentMut,
    id: &str,
) -> Result<(), String> {
    let id = id.trim();
    if builtin_profile_by_id(id).is_some() {
        return Err("内置客户端不能删除".into());
    }
    let Some(custom) = doc
        .get_mut("clients")
        .and_then(|item| item.as_table_mut())
        .and_then(|clients| clients.get_mut("custom"))
        .and_then(|item| item.as_table_mut())
    else {
        return Err(format!("自定义客户端 \"{id}\" 不存在"));
    };
    if custom.remove(id).is_none() {
        return Err(format!("自定义客户端 \"{id}\" 不存在"));
    }

    if doc
        .get("clients")
        .and_then(|item| item.as_table())
        .and_then(|clients| clients.get("default"))
        .and_then(|item| item.as_str())
        .is_some_and(|default| default.trim() == id)
    {
        if let Some(clients) = doc.get_mut("clients").and_then(|item| item.as_table_mut()) {
            clients.remove("default");
        }
    }

    if let Some(models) = doc.get_mut("model").and_then(|item| item.as_table_mut()) {
        for (_, item) in models.iter_mut() {
            if let Some(table) = item.as_table_mut()
                && table
                    .get("client")
                    .and_then(|value| value.as_str())
                    .is_some_and(|client| client.trim() == id)
            {
                table.remove("client");
            }
        }
    }
    Ok(())
}

pub(crate) fn set_default_client_in_document(
    doc: &mut toml_edit::DocumentMut,
    id: &str,
) -> Result<String, String> {
    let profile = profile_by_id(doc, id).ok_or_else(|| {
        format!("客户端 \"{}\" 不存在，请先新增或检查配置", id.trim())
    })?;
    if !doc.contains_key("clients") {
        doc["clients"] = toml_edit::table();
    }
    let clients = doc["clients"]
        .as_table_mut()
        .ok_or_else(|| "[clients] 配置格式错误".to_owned())?;
    clients["default"] = toml_edit::value(profile.id.as_str());
    Ok(profile.id)
}

fn ensure_clients_custom_table(doc: &mut toml_edit::DocumentMut) -> Result<(), String> {
    if !doc.contains_key("clients") {
        doc["clients"] = toml_edit::table();
    }
    let clients = doc["clients"]
        .as_table_mut()
        .ok_or_else(|| "[clients] 配置格式错误".to_owned())?;
    if !clients.contains_key("custom") {
        clients["custom"] = toml_edit::table();
    }
    if !clients.get("custom").is_some_and(|item| item.is_table()) {
        return Err("[clients.custom] 配置格式错误".into());
    }
    Ok(())
}

pub(crate) fn upsert_custom_client(
    profile: &ClientProfile,
    editing_id: Option<&str>,
) -> Result<ClientProfile, String> {
    let mut doc = super::provider::load_config()?;
    let profile = upsert_custom_client_in_document(&mut doc, profile, editing_id)?;
    super::provider::save_config(&doc)?;
    Ok(profile)
}

pub(crate) fn delete_custom_client(id: &str) -> Result<(), String> {
    let mut doc = super::provider::load_config()?;
    delete_custom_client_from_document(&mut doc, id)?;
    super::provider::save_config(&doc)
}

pub(crate) fn set_default_client(id: &str) -> Result<String, String> {
    let mut doc = super::provider::load_config()?;
    let id = set_default_client_in_document(&mut doc, id)?;
    super::provider::save_config(&doc)?;
    Ok(id)
}

pub struct ClientCommand;

impl SlashCommand for ClientCommand {
    fn name(&self) -> &str {
        "client"
    }

    fn aliases(&self) -> &[&str] {
        &["clients"]
    }

    fn description(&self) -> &str {
        "选择、添加或管理当前对话使用的请求客户端"
    }

    fn usage(&self) -> &str {
        "/client"
    }

    fn takes_args(&self) -> bool {
        false
    }

    fn run(&self, _ctx: &mut CommandExecCtx, _args: &str) -> CommandResult {
        CommandResult::Action(crate::app::actions::Action::OpenClientModal {
            mode: ClientModalMode::List,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: &str) -> ClientProfile {
        ClientProfile {
            id: id.into(),
            name: "Test Client".into(),
            protocol: "responses".into(),
            auth_scheme: "bearer".into(),
            env_key: "TEST_API_KEY".into(),
            client_identifier: "test-client".into(),
        }
    }

    #[test]
    fn validates_profile_fields_without_exposing_secrets() {
        let normalized = validate_custom_profile(&profile("my-client")).unwrap();
        assert_eq!(normalized.id, "my-client");
        assert!(validate_custom_profile(&profile("bad/id")).is_err());

        let mut no_key = profile("no-key");
        no_key.auth_scheme = "none".into();
        no_key.env_key.clear();
        assert!(validate_custom_profile(&no_key).is_ok());
    }

    #[test]
    fn custom_profile_round_trip_preserves_unrelated_config() {
        let mut doc = "title = \"keep\"\n[model.demo]\nmodel = \"demo\"\n"
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        upsert_custom_client_in_document(&mut doc, &profile("my-client"), None).unwrap();
        assert_eq!(doc["title"].as_str(), Some("keep"));
        assert_eq!(profile_by_id(&doc, "my-client").unwrap().name, "Test Client");

        set_default_client_in_document(&mut doc, "my-client").unwrap();
        assert_eq!(configured_default_client(&doc).as_deref(), Some("my-client"));
        delete_custom_client_from_document(&mut doc, "my-client").unwrap();
        assert!(configured_default_client(&doc).is_none());
        assert!(profile_by_id(&doc, "my-client").is_none());
    }

    #[test]
    fn builtins_are_not_editable_or_deletable_as_custom_profiles() {
        let mut doc = toml_edit::DocumentMut::new();
        assert!(upsert_custom_client_in_document(&mut doc, &profile("codex"), None).is_err());
        assert!(delete_custom_client_from_document(&mut doc, "codex").is_err());
    }
}
