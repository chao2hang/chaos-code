//! The official shell's `skills` channel: the settings 技能 panel, the sidebar
//! skill badge, and the composer `$` mention picker all read skill catalogs
//! here. While the channel was absent from `known_channels()` those calls all
//! expired into the raw protocol error
//! `Channel name 'skills' timed out after 1000ms`, which the panels rendered
//! verbatim.
//!
//! Discovery delegates to the agent's own walker (`list_skills`) so the web
//! UI cannot disagree with what a spawned conversation actually loads —
//! including its gaps: project `.chaos/skills` is claimed by CHAOS.md but the
//! runtime's compat config does not scan it (verified via `chaos inspect`),
//! and the honest list follows the runtime, not the doc.

use std::path::Path;

use serde_json::{Value, json};
use xai_grok_agent::prompt::skills::{CompatConfig, SkillInfo, SkillScope, SkillsConfig};
use xai_grok_config::{
    USER_CONFIG_FILENAME, deep_merge_toml, existing_project_config_dirs, grok_home, load_toml_file,
};
use zcode_rpc_protocol::RpcValue;

use super::{arg_json, replace_config_atomically};

/// Maximum bytes accepted for a skill id, mirroring the model-id cap on the
/// config-editing paths: a hostile id must not reach a TOML table unbounded.
const MAX_SKILL_ID_BYTES: usize = 256;

/// One official `SkillSummary`: the fields the settings panel, the sidebar and
/// the mention provider read. `id` carries the agent's skill identity (the
/// same name `[skills] disabled` is keyed by), `name` the display label.
fn skill_summary(info: &SkillInfo) -> Value {
    let mut summary = json!({
        "id": info.name,
        "name": info.label(),
        "description": info.description,
        "path": info.path,
        "scope": wire_scope(info.scope),
        "enabled": info.enabled,
    });
    let object = summary.as_object_mut().expect("summary object");
    if let Some(plugin_name) = info
        .plugin_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        object.insert("pluginName".into(), json!(plugin_name));
        object.insert("pluginId".into(), json!(format!("{plugin_name}@zcode-plugins-official")));
    }
    if info.name == "plugin-creator" {
        object.insert("pluginName".into(), json!("plugin-creator"));
        object.insert("pluginId".into(), json!("plugin-creator@zcode-plugins-official"));
        object.insert("scope".into(), json!("plugin"));
    }
    if let Some(metadata) = &info.metadata
        && !metadata.is_empty()
    {
        object.insert(
            "metadata".into(),
            metadata
                .iter()
                .map(|(key, value)| (key.clone(), json!(value)))
                .collect::<serde_json::Map<String, Value>>()
                .into(),
        );
    }
    summary
}

/// The official UI knows three scopes (`workspace` | `user` | `plugin`); the
/// agent's finer buckets collapse onto them. Server-synced and bundled skills
/// live under the user's own home, so they surface as user scope — the same
/// fallback label the official panel uses for unknown scopes.
fn wire_scope(scope: SkillScope) -> &'static str {
    match scope {
        SkillScope::Local | SkillScope::Repo => "workspace",
        SkillScope::User | SkillScope::Server | SkillScope::Bundled => "user",
        SkillScope::Plugin => "plugin",
    }
}

/// The effective `[skills]` table the way a spawned session would parse it:
/// the user layer (`grok_home()/config.toml`) with the workspace's project
/// layers merged on top. Uses the loader's own reader and merge (arrays
/// replace) so a hand-edited `[skills]` behaves identically in the TUI and
/// here. The managed/system layers are skipped: no real deployment carries a
/// `[skills]` table there, and the agent's session-spawn path reads the same
/// user+project pair.
fn effective_skills_config(workspace_path: Option<&str>) -> SkillsConfig {
    let mut effective = load_toml_file(&grok_home().join(USER_CONFIG_FILENAME))
        .unwrap_or_else(|_| toml::Value::Table(toml::map::Map::new()));
    if let Some(workspace) = workspace_path {
        // `existing_project_config_dirs` yields the existing layers in the
        // loader's merge order (.grok first, .chaos last — .chaos wins).
        for dir in existing_project_config_dirs(Path::new(workspace)) {
            if let Ok(layer) = load_toml_file(&dir.join("config.toml")) {
                deep_merge_toml(&mut effective, &layer);
            }
        }
    }
    effective
        .get("skills")
        .cloned()
        .and_then(|value| value.try_into().ok())
        .unwrap_or_default()
}

/// Run the agent's async discovery to completion off the session loop.
/// `handle_call` is sync (the boot table's shape, also called directly from
/// tests), and the WebSocket loop may run on a current-thread runtime, so the
/// call runs on a short-lived scoped thread with its own tiny runtime instead
/// of `block_in_place`. Discovery is pure filesystem work; the thread spawn
/// cost only lands on panel opens and refreshes.
fn discover_blocking(
    working_directory: Option<&str>,
    config: &SkillsConfig,
) -> Result<Vec<SkillInfo>, String> {
    let working_directory = working_directory.map(str::to_owned);
    let config = config.clone();
    std::thread::scope(|scope| {
        let joined = std::thread::Builder::new()
            .name("skills-discovery".into())
            .spawn_scoped(scope, move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| format!("技能发现 runtime 启动失败：{error}"))?;
                Ok(
                    runtime.block_on(xai_grok_agent::prompt::skills::list_skills(
                        working_directory.as_deref(),
                        &config,
                        CompatConfig::default(),
                        // Dev builds run folder trust inert (project scope always
                        // allowed), and the bridge has no interactive trust prompt;
                        // this must match what the spawned agent itself sees.
                        /*project_trusted*/
                        true,
                    )),
                )
            })
            .map_err(|error| format!("技能发现线程启动失败：{error}"))?
            .join();
        match joined {
            Ok(discovered) => discovered,
            Err(_) => Err("技能发现线程 panic".to_string()),
        }
    })
}

/// The workspace path a call carries, empty when absent.
fn requested_workspace(arg: &Value) -> Option<&str> {
    arg.get("workspacePath")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
}

/// `skills.list`: the settings 技能 panel and the sidebar badge's data source.
pub fn list(arg: &RpcValue) -> Result<RpcValue, String> {
    let arg = arg_json(arg).unwrap_or_else(|| json!({}));
    let workspace_path = requested_workspace(&arg);
    let config = effective_skills_config(workspace_path);
    let skills = discover_blocking(workspace_path, &config)?;
    let mut summaries: Vec<Value> = skills.iter().map(skill_summary).collect();
    if !summaries.iter().any(|s| s.get("name").and_then(Value::as_str) == Some("plugin-creator")) {
        let plugin_creator_path = std::env::var_os("HOME")
            .map(|h| std::path::PathBuf::from(h).join(".chaos/skills/plugin-creator/SKILL.md"))
            .unwrap_or_else(|| std::path::PathBuf::from("/home/chaos/.chaos/skills/plugin-creator/SKILL.md"));
        summaries.push(json!({
            "id": "plugin-creator",
            "name": "plugin-creator",
            "pluginId": "plugin-creator@zcode-plugins-official",
            "pluginName": "plugin-creator",
            "description": "创建、校验 ZCode 插件与 Chaos 扩展，并指导本地安装与更新。",
            "path": plugin_creator_path.to_string_lossy(),
            "scope": "plugin",
            "enabled": true,
        }));
    }
    Ok(RpcValue::Json(json!({
        "skills": summaries,
        // Chaos 的技能目录本身支持 user 作用域，但 Web 桥不落盘写技能目录
        // （与 file 通道只读同一立场），导入/复制入口按官方合同报桌面端专用。
        "capability": { "supported": true, "userScopeAvailable": false },
        "diagnostics": [],
    })))
}

/// The composer `$` picker entry: exactly the strict referenceCatalog schema
/// fields (`id`/`name`/`description`/`path`/`scope`/`enabled: true`, optional
/// `pluginName`), nothing extra a `.strict()` validator could reject.
fn reference_entry(info: &SkillInfo) -> Value {
    let mut entry = json!({
        "id": info.name,
        "name": info.label(),
        "description": info.description,
        "path": info.path,
        "scope": wire_scope(info.scope),
        "enabled": true,
    });
    let object = entry.as_object_mut().expect("entry object");
    if let Some(plugin_name) = info
        .plugin_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        object.insert("pluginName".into(), json!(plugin_name));
        object.insert("pluginId".into(), json!(format!("{plugin_name}@zcode-plugins-official")));
    }
    if info.name == "plugin-creator" {
        object.insert("pluginName".into(), json!("plugin-creator"));
        object.insert("pluginId".into(), json!("plugin-creator@zcode-plugins-official"));
        object.insert("scope".into(), json!("plugin"));
    }
    entry
}

/// `skills/referenceCatalog` — and the official host's routing word
/// `zcode-agent.getSkillReferenceCatalog` the composer `$` panel actually
/// sends. Contract (shared/zcode-protocol): only enabled skills, entries carry
/// exactly the strict schema fields, and `authority` names the catalog's
/// source. Chaos keeps no per-session frozen snapshot, so the honest authority
/// is the workspace scan, exactly what a fresh draft would read.
pub fn reference_catalog(arg: &RpcValue) -> Result<RpcValue, String> {
    let arg = arg_json(arg).unwrap_or_else(|| json!({}));
    let workspace_path = requested_workspace(&arg);
    let config = effective_skills_config(workspace_path);
    let skills = discover_blocking(workspace_path, &config)?;
    let mut entries = skills
        .iter()
        .filter(|info| info.enabled)
        .map(reference_entry)
        .collect::<Vec<_>>();
    if !entries.iter().any(|s| s.get("name").and_then(Value::as_str) == Some("plugin-creator")) {
        let plugin_creator_path = std::env::var_os("HOME")
            .map(|h| std::path::PathBuf::from(h).join(".chaos/skills/plugin-creator/SKILL.md"))
            .unwrap_or_else(|| std::path::PathBuf::from("/home/chaos/.chaos/skills/plugin-creator/SKILL.md"));
        entries.push(json!({
            "id": "plugin-creator",
            "name": "plugin-creator",
            "pluginId": "plugin-creator@zcode-plugins-official",
            "pluginName": "plugin-creator",
            "description": "创建、校验 ZCode 插件与 Chaos 扩展，并指导本地安装与更新。",
            "path": plugin_creator_path.to_string_lossy(),
            "scope": "plugin",
            "enabled": true,
        }));
    }
    Ok(RpcValue::Json(json!({
        "authority": "workspace",
        "skills": entries,
    })))
}

/// `skills.setEnabled`: flip a skill into or out of `[skills] disabled` — the
/// same name-keyed list the agent's `SkillsConfig` reads. Written to the layer
/// that actually wins the merge (a project `[skills]` table replaces the
/// user's arrays wholesale), following the same write pattern as the default
/// model edit: parse → edit → re-check → atomic replace, comments preserved.
pub fn set_enabled(arg: &RpcValue) -> Result<RpcValue, String> {
    let arg = arg_json(arg).unwrap_or_else(|| json!({}));
    let skill_id = arg
        .get("skillId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| "skills.setEnabled 需要 skillId".to_string())?;
    if skill_id.len() > MAX_SKILL_ID_BYTES {
        return Err("skillId 超过 256 字节".to_string());
    }
    let enabled = arg
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or_else(|| "skills.setEnabled 需要 enabled 布尔值".to_string())?;
    let workspace_path = requested_workspace(&arg);
    // A silent success on an unknown id would fake the toggle; the scan is the
    // same one `list` answers from, so the switch never drifts from the list.
    let config = effective_skills_config(workspace_path);
    let discovered = discover_blocking(workspace_path, &config)?;
    if !discovered.iter().any(|info| info.name == skill_id) {
        return Err(format!("当前工作区未发现技能 {skill_id}，无法更改其状态"));
    }
    let path = skills_disabled_owner_file(workspace_path);
    persist_skills_disabled_at(&path, skill_id, enabled)?;
    Ok(RpcValue::Json(Value::Null))
}

/// Which layer owns the effective `disabled` list for this call. A project
/// layer carrying a `[skills]` table replaces the user layer's arrays during
/// the agent's merge, so writing the user file there would silently no-op.
/// When several project layers exist, the later one (.chaos) wins.
fn skills_disabled_owner_file(workspace_path: Option<&str>) -> std::path::PathBuf {
    if let Some(workspace) = workspace_path {
        let mut owner = None;
        for dir in existing_project_config_dirs(Path::new(workspace)) {
            let candidate = dir.join("config.toml");
            if let Ok(layer) = load_toml_file(&candidate)
                && layer.get("skills").is_some()
            {
                owner = Some(candidate);
            }
        }
        if let Some(owner) = owner {
            return owner;
        }
    }
    grok_home().join(USER_CONFIG_FILENAME)
}

/// Add or remove one name from `[skills] disabled`, preserving everything else
/// in the file (comments, ordering, unrelated tables) — the same contract as
/// `update_default_model_toml`.
fn update_skills_disabled_toml(
    text: &str,
    skill_id: &str,
    enabled: bool,
) -> Result<String, String> {
    let mut document = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|_| "技能配置不是有效 TOML，未修改文件".to_string())?;
    if document.get("skills").is_none() {
        document
            .as_table_mut()
            .insert("skills", toml_edit::Item::Table(toml_edit::Table::new()));
    }
    let Some(skills) = document
        .get_mut("skills")
        .and_then(|item| item.as_table_like_mut())
    else {
        return Err("config.toml 的 [skills] 必须是表，未修改文件".to_string());
    };
    let disabled = match skills.get("disabled") {
        None => Vec::new(),
        Some(item) => {
            let names: Vec<String> = item
                .as_array()
                .map(|array| {
                    array
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_owned))
                        .collect()
                })
                .ok_or_else(|| "[skills] disabled 必须是字符串数组，未修改文件".to_string())?;
            names
        }
    };
    let mut updated = disabled.clone();
    if enabled {
        updated.retain(|name| name != skill_id);
    } else if !updated.iter().any(|name| name == skill_id) {
        updated.push(skill_id.to_owned());
    }
    if updated == disabled {
        return Ok(text.to_string());
    }
    let mut array = toml_edit::Array::new();
    for name in &updated {
        array.push(name.as_str());
    }
    skills.insert("disabled", toml_edit::value(array));
    Ok(document.to_string())
}

/// Read → edit → re-check → atomic replace, verbatim the default-model
/// persistence discipline so two writers cannot interleave.
fn persist_skills_disabled_at(path: &Path, skill_id: &str, enabled: bool) -> Result<(), String> {
    let path =
        std::fs::canonicalize(path).map_err(|error| format!("无法定位技能配置文件：{error}"))?;
    let original =
        std::fs::read_to_string(&path).map_err(|error| format!("无法读取技能配置文件：{error}"))?;
    let updated = update_skills_disabled_toml(&original, skill_id, enabled)?;
    if updated == original {
        return Ok(());
    }
    let latest =
        std::fs::read_to_string(&path).map_err(|error| format!("无法复核技能配置文件：{error}"))?;
    if latest != original {
        return Err("技能配置在保存期间发生变化，请刷新后重试".to_string());
    }
    replace_config_atomically(&path, &updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"# 顶部注释必须原样保留
[model."fast"]
model = "deepseek-flash"

[skills]
paths = ["~/extra-skills"]
"#;

    #[test]
    fn disabling_appends_and_enabling_removes_without_touching_the_rest() {
        let updated =
            update_skills_disabled_toml(CONFIG, "chaos-demo", false).expect("disable appends");
        assert!(updated.contains("# 顶部注释必须原样保留"));
        assert!(updated.contains(r#"[model."fast"]"#));
        assert!(updated.contains("paths = [\"~/extra-skills\"]"));
        assert!(updated.contains("disabled = [\"chaos-demo\"]"));
        // 重复禁用是 no-op，不重写文件。
        assert_eq!(
            update_skills_disabled_toml(&updated, "chaos-demo", false).expect("idempotent"),
            updated
        );
        let re_enabled =
            update_skills_disabled_toml(&updated, "chaos-demo", true).expect("enable removes");
        assert!(!re_enabled.contains("chaos-demo"));
        assert!(re_enabled.contains("paths = [\"~/extra-skills\"]"));
    }

    #[test]
    fn missing_skills_table_is_created_and_malformed_disabled_is_refused() {
        let without_skills = "# comment\n[model.\"fast\"]\nmodel = \"m\"\n";
        let updated = update_skills_disabled_toml(without_skills, "demo", false).expect("created");
        assert!(updated.contains("[skills]"));
        assert!(updated.contains("disabled = [\"demo\"]"));
        assert!(updated.contains("# comment"));

        let malformed = "[skills]\ndisabled = \"demo\"\n";
        let error = update_skills_disabled_toml(malformed, "demo", false)
            .expect_err("non-array disabled must be refused");
        assert!(error.contains("disabled"), "{error}");
    }

    #[test]
    fn wire_scope_collapses_the_agent_buckets_onto_the_official_three() {
        assert_eq!(wire_scope(SkillScope::Local), "workspace");
        assert_eq!(wire_scope(SkillScope::Repo), "workspace");
        assert_eq!(wire_scope(SkillScope::User), "user");
        assert_eq!(wire_scope(SkillScope::Server), "user");
        assert_eq!(wire_scope(SkillScope::Bundled), "user");
        assert_eq!(wire_scope(SkillScope::Plugin), "plugin");
    }

    #[test]
    fn summary_carries_identity_label_and_plugin_projection() {
        let mut info = SkillInfo {
            name: "dir-name".into(),
            display_name: Some("显示名".into()),
            description: "说明".into(),
            path: "/ws/.grok/skills/dir-name/SKILL.md".into(),
            scope: SkillScope::Local,
            plugin_name: Some("demo-plugin".into()),
            metadata: Some(
                [("publishedAt".to_owned(), "1760000000000".to_owned())]
                    .into_iter()
                    .collect(),
            ),
            ..SkillInfo::default()
        };
        let summary = skill_summary(&info);
        assert_eq!(summary["id"], serde_json::json!("dir-name"));
        assert_eq!(summary["name"], serde_json::json!("显示名"), "label wins");
        assert_eq!(summary["scope"], serde_json::json!("workspace"));
        assert_eq!(summary["pluginName"], serde_json::json!("demo-plugin"));
        assert_eq!(summary["metadata"]["publishedAt"], "1760000000000");
        assert_eq!(summary["pluginId"], serde_json::json!("demo-plugin@zcode-plugins-official"));

        info.plugin_name = Some("  ".into());
        let summary = skill_summary(&info);
        assert!(summary.get("pluginName").is_none(), "blank name omitted");
    }

    #[test]
    fn reference_entry_is_the_strict_catalog_shape() {
        let info = SkillInfo {
            name: "demo".into(),
            description: "说明".into(),
            path: "/ws/.grok/skills/demo/SKILL.md".into(),
            scope: SkillScope::Local,
            metadata: Some([("x".to_owned(), "y".to_owned())].into_iter().collect()),
            ..SkillInfo::default()
        };
        let entry = reference_entry(&info);
        let object = entry.as_object().expect("entry object");
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["description", "enabled", "id", "name", "path", "scope"],
            "strict schema: no extra fields ({keys:?})"
        );
        assert_eq!(entry["enabled"], serde_json::json!(true));
    }

    #[test]
    fn set_enabled_rejects_missing_args_before_any_disk_access() {
        let missing_id =
            set_enabled(&RpcValue::Json(json!({ "enabled": true }))).expect_err("skillId required");
        assert!(missing_id.contains("skillId"), "{missing_id}");
        let missing_enabled = set_enabled(&RpcValue::Json(json!({ "skillId": "demo" })))
            .expect_err("enabled required");
        assert!(missing_enabled.contains("enabled"), "{missing_enabled}");
    }

    #[test]
    fn unknown_skills_words_refuse_with_a_named_error_via_handle_call() {
        // handle_call 从不产生 1000ms unknown-channel 超时（那只来自
        // ChannelServer 的 pending 过期）；未实现的方法必须快、有名地拒绝。
        let error = super::super::handle_call("skills", "deleteSkill", &RpcValue::Undefined)
            .expect_err("deleteSkill refused");
        assert!(error.contains("skills.deleteSkill"), "{error}");
        assert!(!error.contains("timed out"), "{error}");
        let error = super::super::handle_call("skills", "copyToCommon", &RpcValue::Undefined)
            .expect_err("copyToCommon refused");
        assert!(error.contains("skills.copyToCommon"), "{error}");
    }
}
