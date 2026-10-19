use serde_json::Value;
use zcode_rpc_protocol::RpcValue;

#[test]
fn skills_list_always_includes_valid_plugin_creator() {
    let result = xai_grok_web::chaos_bridge::handle_call("skills", "list", &RpcValue::Undefined)
        .expect("skills.list must succeed");
    let RpcValue::Json(Value::Object(map)) = result else {
        panic!("expected json object");
    };
    let skills = map.get("skills").and_then(Value::as_array).expect("skills array");
    let plugin_creator = skills
        .iter()
        .find(|s| s.get("name").and_then(Value::as_str) == Some("plugin-creator"))
        .expect("plugin-creator must be present");

    assert_eq!(
        plugin_creator.get("pluginId").and_then(Value::as_str),
        Some("plugin-creator@zcode-plugins-official")
    );
    assert_eq!(
        plugin_creator.get("scope").and_then(Value::as_str),
        Some("plugin")
    );
    assert_eq!(
        plugin_creator.get("enabled").and_then(Value::as_bool),
        Some(true)
    );
    let path = plugin_creator
        .get("path")
        .and_then(Value::as_str)
        .expect("path must be string");
    assert!(!path.trim().is_empty(), "path must not be empty");
}

#[test]
fn plugin_management_overview_supports_plugin_creator() {
    let result = xai_grok_web::chaos_bridge::handle_call(
        "plugin-management",
        "getPluginsOverview",
        &RpcValue::Undefined,
    )
    .expect("getPluginsOverview must succeed");
    let RpcValue::Json(Value::Object(map)) = result else {
        panic!("expected json object");
    };
    let capability = map.get("capability").expect("capability object");
    assert_eq!(
        capability.get("supported").and_then(Value::as_bool),
        Some(true)
    );

    let installed = map.get("installedPlugins").and_then(Value::as_array).expect("installedPlugins array");
    assert!(
        installed
            .iter()
            .any(|p| p.get("name").and_then(Value::as_str) == Some("plugin-creator")),
        "plugin-creator must be in installedPlugins"
    );
}

#[test]
fn read_workspace_presentation_contains_chaos_slash_commands() {
    let result = xai_grok_web::chaos_bridge::handle_call(
        "zcode-session",
        "readWorkspacePresentation",
        &RpcValue::Undefined,
    )
    .expect("readWorkspacePresentation must succeed");
    let RpcValue::Json(Value::Object(map)) = result else {
        panic!("expected json object");
    };
    let slash_commands = map
        .get("slashCommands")
        .and_then(Value::as_array)
        .expect("slashCommands array");
    assert!(
        slash_commands.len() >= 20,
        "should have comprehensive slash commands, got {}",
        slash_commands.len()
    );

    let command_names: Vec<&str> = slash_commands
        .iter()
        .filter_map(|c| c.get("name").and_then(Value::as_str))
        .collect();

    for required in ["help", "clear", "compact", "plan", "model", "effort", "doctor", "skills", "remember"] {
        assert!(
            command_names.contains(&required),
            "slashCommands must contain /{required}"
        );
    }
}

#[test]
fn commands_channel_handles_list_and_get_primary_dir() {
    let list_res = xai_grok_web::chaos_bridge::handle_call("commands", "list", &RpcValue::Undefined)
        .expect("commands.list must succeed");
    let RpcValue::Json(Value::Object(map)) = list_res else {
        panic!("expected json object");
    };
    let commands = map.get("commands").and_then(Value::as_array).expect("commands array");
    assert!(!commands.is_empty(), "commands array must not be empty");

    let dir_res = xai_grok_web::chaos_bridge::handle_call(
        "commands",
        "getPrimaryUserCommandsDirectory",
        &RpcValue::Undefined,
    )
    .expect("commands.getPrimaryUserCommandsDirectory must succeed");
    let RpcValue::Json(Value::Object(dir_map)) = dir_res else {
        panic!("expected json object");
    };
    assert!(dir_map.get("path").is_some());
}

#[test]
fn memory_channel_lists_and_reads_project_memories() {
    let list_res = xai_grok_web::chaos_bridge::handle_call("memory", "listProjectMemories", &RpcValue::Undefined)
        .expect("memory.listProjectMemories must succeed");
    let RpcValue::Json(Value::Array(workspaces)) = list_res else {
        panic!("expected array of workspaces");
    };
    assert!(!workspaces.is_empty(), "must return at least one workspace memory entry");
}

#[test]
fn model_trajectory_query_returns_an_empty_list_instead_of_unknown_method() {
    let result = xai_grok_web::chaos_bridge::handle_call(
        "zcode-task",
        "getModelTrajectory",
        &RpcValue::Json(serde_json::json!({ "taskId": "session-1" })),
    )
    .expect("getModelTrajectory must be a recognized read-only method");

    assert_eq!(result, RpcValue::Json(serde_json::json!([])));
}
