//! `/effort`: set reasoning effort on the current model without re-picking it.
//!
//! Thin wrapper over `Action::SwitchModel` with the session's current model id and the chosen effort (same wire path as `/model <name> <effort>`).

use crate::app::actions::Action;
use crate::slash::command::{
    AppCtx, ArgItem, CommandExecCtx, CommandResult, SlashCommand, slash_meta,
};
use crate::slash::commands::effort_levels::build_effort_arg_items;

/// Set reasoning effort for the active model.
pub struct EffortCommand;

impl SlashCommand for EffortCommand {
    slash_meta! {
        name: "effort",
        aliases: ["think"],
        description: "设置当前模型的推理强度",
        // Levels are model-specific; empty-args and UnknownToken errors list the active model's offered option ids instead of a hardcoded set.
        usage: "/effort <level>",
        takes_args: true,
        // 裸 `/effort` 是合法调用：列出当前模型可用等级（文本 picker），
        // 不要求参数。框架的 `is_command_complete` 会在 args_required=true
        // 时拦截空参数 Enter。
        args_required: false,
        session_scoped: true,
        arg_placeholder: "<level>",
    }

    fn suggest_args(&self, ctx: &AppCtx, _args_query: &str) -> Option<Vec<ArgItem>> {
        let options = ctx.models.reasoning_effort_options();
        if options.is_empty() {
            return None;
        }
        Some(build_effort_arg_items(
            &options,
            ctx.models.reasoning_effort,
            true,
            |option| option.id.clone(),
        ))
    }

    fn run(&self, ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let trimmed = args.trim();
        let Some(model_id) = ctx.models.current.clone() else {
            return CommandResult::Error("No active model".into());
        };

        if trimmed.is_empty() {
            // Issue #14.3：裸 `/effort` 不再回 Usage 错误，改为列出当前
            // 模型可用等级 + 当前值。限制：尚无 popup picker，用户仍需
            // 手动键入 `/effort <level>`（文本引导，非交互选择器）。
            // Issue #14.4：若模型明确禁用 reasoning effort，给出可操作的
            // 引导（告知在哪里开启）。缺失声明本身会走内置回退菜单。
            // Issue #14.5：回退菜单（provider 不返回 reasoningEfforts）
            // 附「等级为猜测」风险提示。
            if !ctx.models.available.contains_key(&model_id) {
                return CommandResult::Error(format!(
                    "当前模型不在会话 catalog 中（可能尚未 /provider models 注册或 agent 未 reload）。\n模型：{model_id}"
                ));
            }

            let offered = ctx.models.reasoning_effort_options_for(&model_id);
            let current = ctx
                .models
                .reasoning_effort
                .map(|e| format!(" (current: {e})"))
                .unwrap_or_default();

            if offered.is_empty() {
                // 模型在 catalog 中且明确禁用了 supportsReasoningEffort。
                return CommandResult::Error(format!(
                    "当前模型明确禁用 reasoning effort。若该模型支持推理强度，请在 [model.<id>] 中设置 supports_reasoning_effort = true（并可选地用 reasoning_efforts = [...] 指定可用等级）。\n模型：{model_id}"
                ));
            }

            let levels = offered
                .iter()
                .map(|opt| opt.id.clone())
                .collect::<Vec<_>>()
                .join("|");

            // #14.5：options 来自 legacy 回退（meta 无可用 reasoningEfforts）时
            // 附风险提示。`reasoning_effort_options_for` 在 supports=true 且
            // parse 失败/缺省时走 legacy，此时 offered 非空。
            let is_legacy_fallback = {
                use xai_grok_shell::sampling::types::parse_reasoning_efforts_meta;
                let meta = ctx
                    .models
                    .available
                    .get(&model_id)
                    .and_then(|m| m.meta.as_ref());
                parse_reasoning_efforts_meta(meta).is_none()
            };

            if is_legacy_fallback {
                return CommandResult::Message(format!(
                    "/effort <{levels}>{current}\n\
                     ⚠ provider 未返回 reasoningEfforts，以上为内置回退菜单（猜测），\
                     provider 不支持的等级会 400。建议在 [model.<id>] 用 reasoning_efforts = [...] 写明真实等级。\n\
                     （裸 /effort 为文本引导，尚无 popup picker；键入级别后回车即可）"
                ));
            }

            return CommandResult::Message(format!(
                "/effort <{levels}>{current}\n（裸 /effort 为文本引导，尚无 popup picker；键入级别后回车即可）"
            ));
        }

        // Same gate-first policy as the CLI (`--effort`) and headless.
        match ctx.models.resolve_effort_for_model(&model_id, trimmed) {
            Ok(effort) => CommandResult::Action(Action::SwitchModel {
                model_id,
                effort: Some(effort),
            }),
            Err(err) => CommandResult::Error(err.message()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::model_state::ModelState;
    use crate::slash::commands::effort_levels::EFFORT_LEVELS;
    use agent_client_protocol as acp;
    use std::sync::Arc;
    use xai_grok_shell::sampling::types::ReasoningEffort;

    fn model_with_reasoning(id: &str, name: &str) -> (acp::ModelId, acp::ModelInfo) {
        let id = acp::ModelId::new(Arc::from(id));
        let mut meta = serde_json::Map::new();
        meta.insert(
            "supportsReasoningEffort".into(),
            serde_json::Value::Bool(true),
        );
        let info = acp::ModelInfo::new(id.clone(), name.to_string())
            .meta(serde_json::Value::Object(meta).as_object().cloned());
        (id, info)
    }

    fn plain_model(id: &str, name: &str) -> (acp::ModelId, acp::ModelInfo) {
        let id = acp::ModelId::new(Arc::from(id));
        let info = acp::ModelInfo::new(id.clone(), name.to_string());
        (id, info)
    }

    static EMPTY_BUNDLE: crate::app::bundle::BundleState = crate::app::bundle::BundleState {
        has_cache: false,
        version: String::new(),
        personas: Vec::new(),
        roles: Vec::new(),
        agents: Vec::new(),
        skills: Vec::new(),
        persona_details: Vec::new(),
        role_details: Vec::new(),
    };

    fn dummy_exec_ctx(models: &ModelState) -> CommandExecCtx<'_> {
        CommandExecCtx {
            models,
            session_id: None,
            bundle_state: &EMPTY_BUNDLE,
            screen_mode: crate::app::ScreenMode::Inline,
            billing_surface_visible: true,
            usage_command_visible: true,
            pager_state: crate::settings::PagerLocalSnapshot {
                multiline_mode: false,
                yolo_mode: false,
                ..crate::settings::PagerLocalSnapshot::default()
            },
        }
    }

    #[test]
    fn empty_args_returns_message_with_levels() {
        // Issue #14.3：裸 /effort 不再回 Error，改为 Message 列出当前
        // 模型可用等级 + 当前值。supports=true 且无 reasoningEfforts → legacy
        // 回退，应附 #14.5 风险提示。
        let mut state = ModelState::default();
        let (id, info) = model_with_reasoning("reasoning-x", "Reasoning X");
        state.available.insert(id.clone(), info);
        state.current = Some(id);
        state.reasoning_effort = Some(ReasoningEffort::Medium);
        let mut ctx = dummy_exec_ctx(&state);
        let result = EffortCommand.run(&mut ctx, "");
        match result {
            CommandResult::Message(msg) => {
                assert!(msg.contains("/effort <"), "msg={msg}");
                // Legacy menu option ids only — not none/minimal.
                assert!(msg.contains("max|xhigh|high|medium|low"), "msg={msg}");
                assert!(msg.contains("current: medium"), "msg={msg}");
                assert!(!msg.contains("none"), "msg={msg}");
                assert!(!msg.contains("minimal"), "msg={msg}");
                assert!(
                    msg.contains("回退") || msg.contains("猜测"),
                    "legacy fallback must warn: {msg}"
                );
            }
            other => panic!("expected Message, got {other:?}"),
        }
    }

    #[test]
    fn empty_args_with_explicit_menu_omits_fallback_warning() {
        let mut state = ModelState::default();
        let id = acp::ModelId::new(Arc::from("menu-x"));
        let info = acp::ModelInfo::new(id.clone(), "Menu X".to_string()).meta(
            serde_json::json!({
                "supportsReasoningEffort": true,
                "reasoningEfforts": ["low", "high"],
            })
            .as_object()
            .cloned(),
        );
        state.available.insert(id.clone(), info);
        state.current = Some(id);
        let mut ctx = dummy_exec_ctx(&state);
        match EffortCommand.run(&mut ctx, "") {
            CommandResult::Message(msg) => {
                assert!(msg.contains("/effort <low|high>"), "msg={msg}");
                assert!(!msg.contains("猜测"), "msg={msg}");
                assert!(!msg.contains("回退"), "msg={msg}");
            }
            other => panic!("expected Message, got {other:?}"),
        }
    }

    #[test]
    fn empty_args_on_unsupported_model_hints_to_enable_flag() {
        // Issue #14.4：不可发现时给引导。
        let mut state = ModelState::default();
        let id = acp::ModelId::new(Arc::from("plain-x"));
        let info = acp::ModelInfo::new(id.clone(), "Plain X".to_string()).meta(
            serde_json::json!({ "supportsReasoningEffort": false })
                .as_object()
                .cloned(),
        );
        state.available.insert(id.clone(), info);
        state.current = Some(id);
        let mut ctx = dummy_exec_ctx(&state);
        let result = EffortCommand.run(&mut ctx, "");
        match result {
            CommandResult::Error(msg) => {
                assert!(
                    msg.contains("supports_reasoning_effort = true"),
                    "expected hint about the config key, got: {msg}"
                );
                assert!(msg.contains("plain-x"), "msg={msg}");
            }
            other => panic!("expected Error with hint, got {other:?}"),
        }
    }

    #[test]
    fn empty_args_on_missing_catalog_model_explains_not_found() {
        let mut state = ModelState::default();
        let id = acp::ModelId::new(Arc::from("ghost-x"));
        state.current = Some(id);
        // deliberately not inserted into available
        let mut ctx = dummy_exec_ctx(&state);
        match EffortCommand.run(&mut ctx, "") {
            CommandResult::Error(msg) => {
                assert!(msg.contains("不在会话 catalog"), "msg={msg}");
                assert!(msg.contains("ghost-x"), "msg={msg}");
                assert!(
                    !msg.contains("supports_reasoning_effort"),
                    "must not claim missing support flag: {msg}"
                );
            }
            other => panic!("expected Error about missing catalog, got {other:?}"),
        }
    }

    #[test]
    fn args_required_is_false_for_bare_effort() {
        assert!(!EffortCommand.args_required());
    }

    #[test]
    fn think_is_an_alias_for_effort() {
        assert_eq!(EffortCommand.aliases(), &["think"]);
    }

    #[test]
    fn unknown_level_errors() {
        let mut state = ModelState::default();
        let (id, info) = model_with_reasoning("reasoning-x", "Reasoning X");
        state.available.insert(id.clone(), info);
        state.current = Some(id);
        let mut ctx = dummy_exec_ctx(&state);
        let result = EffortCommand.run(&mut ctx, "turbo");
        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("unknown effort level 'turbo'"), "msg={msg}");
                assert!(msg.contains("use one of:"), "msg={msg}");
                assert!(msg.contains("xhigh"), "msg={msg}");
                assert!(!msg.contains("none"), "msg={msg}");
                assert!(!msg.contains("minimal"), "msg={msg}");
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn valid_level_dispatches_switch_model_on_current() {
        let mut state = ModelState::default();
        let (id, info) = model_with_reasoning("reasoning-x", "Reasoning X");
        state.available.insert(id.clone(), info);
        state.current = Some(id.clone());
        let mut ctx = dummy_exec_ctx(&state);
        let result = EffortCommand.run(&mut ctx, "high");
        match result {
            CommandResult::Action(Action::SwitchModel { model_id, effort }) => {
                assert_eq!(model_id, id);
                assert_eq!(effort, Some(ReasoningEffort::High));
            }
            other => panic!("expected SwitchModel with effort, got {other:?}"),
        }
    }

    #[test]
    fn unadvertised_model_accepts_max() {
        let mut state = ModelState::default();
        let (id, info) = plain_model("custom-x", "Custom X");
        state.available.insert(id.clone(), info);
        state.current = Some(id.clone());
        let mut ctx = dummy_exec_ctx(&state);

        match EffortCommand.run(&mut ctx, "max") {
            CommandResult::Action(Action::SwitchModel { model_id, effort }) => {
                assert_eq!(model_id, id);
                assert_eq!(effort, Some(ReasoningEffort::Max));
            }
            other => panic!("expected unadvertised model to accept max, got {other:?}"),
        }
    }

    #[test]
    fn none_and_minimal_rejected_when_model_menu_omits_them() {
        // The legacy fallback menu is max..low; `none`/`minimal` used to pass through and 400 on grok-4.5, so reject at the TUI instead
        let mut state = ModelState::default();
        let (id, info) = model_with_reasoning("reasoning-x", "Reasoning X");
        state.available.insert(id.clone(), info);
        state.current = Some(id);
        let mut ctx = dummy_exec_ctx(&state);
        for token in ["none", "minimal"] {
            let result = EffortCommand.run(&mut ctx, token);
            match result {
                CommandResult::Error(ref msg) => {
                    assert!(
                        msg.contains(&format!("unknown effort level '{token}'")),
                        "expected Error for {token}, got {msg}"
                    );
                    // The error must not re-advertise the rejected token as a valid choice (aside from quoting it in "unknown effort level '…'")
                    let after_prefix = msg
                        .split_once("; ")
                        .map(|(_, rest)| rest)
                        .unwrap_or(msg.as_str());
                    assert!(
                        !after_prefix.contains(token),
                        "error must not list {token} as offered: {msg}"
                    );
                    assert!(!msg.contains("unset"), "msg={msg}");
                }
                other => panic!("expected Error for {token}, got {other:?}"),
            }
        }
    }

    #[test]
    fn none_accepted_when_model_menu_offers_it() {
        let mut state = ModelState::default();
        let id = acp::ModelId::new(Arc::from("voice-dual"));
        let info = acp::ModelInfo::new(id.clone(), "Voice Dual".to_string()).meta(
            serde_json::json!({
                "supportsReasoningEffort": true,
                "reasoningEfforts": [
                    { "value": "none", "label": "None", "default": true },
                    { "value": "high", "label": "High" },
                ],
            })
            .as_object()
            .cloned(),
        );
        state.available.insert(id.clone(), info);
        state.current = Some(id.clone());
        let mut ctx = dummy_exec_ctx(&state);
        let result = EffortCommand.run(&mut ctx, "none");
        match result {
            CommandResult::Action(Action::SwitchModel { model_id, effort }) => {
                assert_eq!(model_id, id);
                assert_eq!(effort, Some(ReasoningEffort::None));
            }
            other => panic!("expected SwitchModel with none, got {other:?}"),
        }
    }

    #[test]
    fn remap_id_dispatches_mapped_canonical_effort() {
        let mut state = ModelState::default();
        let id = acp::ModelId::new(Arc::from("reasoning-x"));
        let info = acp::ModelInfo::new(id.clone(), "Reasoning X".to_string()).meta(
            serde_json::json!({
                "supportsReasoningEffort": true,
                "reasoningEfforts": [{ "id": "deep", "value": "xhigh", "label": "Deep" }],
            })
            .as_object()
            .cloned(),
        );
        state.available.insert(id.clone(), info);
        state.current = Some(id.clone());
        let mut ctx = dummy_exec_ctx(&state);
        // The rendered row inserts the id; `/effort deep` must send `xhigh`.
        match EffortCommand.run(&mut ctx, "deep") {
            CommandResult::Action(Action::SwitchModel { model_id, effort }) => {
                assert_eq!(model_id, id);
                assert_eq!(effort, Some(ReasoningEffort::Xhigh));
            }
            other => panic!("expected SwitchModel with remapped effort, got {other:?}"),
        }
    }

    #[test]
    fn non_reasoning_model_errors() {
        let mut state = ModelState::default();
        let id = acp::ModelId::new(Arc::from("grok-4.5"));
        let info = acp::ModelInfo::new(id.clone(), "Grok 4.5".to_string()).meta(
            serde_json::json!({ "supportsReasoningEffort": false })
                .as_object()
                .cloned(),
        );
        state.available.insert(id.clone(), info);
        state.current = Some(id);
        let mut ctx = dummy_exec_ctx(&state);
        let result = EffortCommand.run(&mut ctx, "high");
        assert!(matches!(
            result,
            CommandResult::Error(msg) if msg.contains("does not support reasoning effort")
        ));
    }

    #[test]
    fn no_current_model_errors() {
        let mut state = ModelState::default();
        let (id, info) = model_with_reasoning("reasoning-x", "Reasoning X");
        state.available.insert(id, info);
        let mut ctx = dummy_exec_ctx(&state);
        let result = EffortCommand.run(&mut ctx, "high");
        assert!(matches!(result, CommandResult::Error(msg) if msg.contains("No active model")));
    }

    #[test]
    fn suggest_args_none_without_current_or_support() {
        let cmd = EffortCommand;
        let empty = ModelState::default();
        let ctx = AppCtx {
            models: &empty,
            cwd: std::path::Path::new("."),
            has_session_announcements: false,
            billing_surface_visible: true,
            usage_command_visible: true,
            workflows_available: true,
            saved_workflows: &[],
            workflow_runs: &[],
            screen_mode: crate::app::ScreenMode::Fullscreen,
            current_title: None,
        };
        assert!(cmd.suggest_args(&ctx, "").is_none());

        let mut plain = ModelState::default();
        let id = acp::ModelId::new(Arc::from("grok-4.5"));
        let info = acp::ModelInfo::new(id.clone(), "Grok 4.5".to_string()).meta(
            serde_json::json!({ "supportsReasoningEffort": false })
                .as_object()
                .cloned(),
        );
        plain.available.insert(id.clone(), info);
        plain.current = Some(id);
        let ctx = AppCtx {
            models: &plain,
            cwd: std::path::Path::new("."),
            has_session_announcements: false,
            billing_surface_visible: true,
            usage_command_visible: true,
            workflows_available: true,
            saved_workflows: &[],
            workflow_runs: &[],
            screen_mode: crate::app::ScreenMode::Fullscreen,
            current_title: None,
        };
        assert!(cmd.suggest_args(&ctx, "").is_none());
    }

    #[test]
    fn suggest_args_lists_levels_with_active_marker() {
        let mut state = ModelState::default();
        let (id, info) = model_with_reasoning("reasoning-x", "Reasoning X");
        state.available.insert(id.clone(), info);
        state.current = Some(id);
        state.reasoning_effort = Some(ReasoningEffort::High);

        let cmd = EffortCommand;
        let ctx = AppCtx {
            models: &state,
            cwd: std::path::Path::new("."),
            has_session_announcements: false,
            billing_surface_visible: true,
            usage_command_visible: true,
            workflows_available: true,
            saved_workflows: &[],
            workflow_runs: &[],
            screen_mode: crate::app::ScreenMode::Fullscreen,
            current_title: None,
        };
        let items = cmd.suggest_args(&ctx, "").unwrap();
        assert_eq!(items.len(), EFFORT_LEVELS.len());
        assert_eq!(items[0].insert_text, "max");
        assert_eq!(items[1].insert_text, "xhigh");
        assert_eq!(items[2].insert_text, "high");
        assert_eq!(items[2].display, "high (active)");
        assert_eq!(items[3].insert_text, "medium");
        assert_eq!(items[4].insert_text, "low");
        assert!(items[0].match_text.starts_with("a "));
        assert!(items[4].match_text.starts_with("e "));
    }
}
