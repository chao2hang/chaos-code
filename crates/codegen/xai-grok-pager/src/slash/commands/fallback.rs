//! `/fallback` -- manage the fallback model chain.
//!
//! When a session's model can no longer be served -- it left the catalog, or the
//! account lost access to it -- the session is moved to another model instead of
//! blocking. Without a chain that choice is made for the user (any selectable
//! model in the same family); with one, the first entry that can actually be
//! served wins. The chain is persisted to `[fallback] models` in the
//! `config.toml` under the Chaos home (`$CHAOS_HOME`, default `~/.chaos`) and is
//! read by `MvpAgent::select_fallback_model`, which both the resume path and the
//! blocked-prompt path consult.
//!
//! The chain covers model *availability*. It is not a per-request retry list: a
//! rate-limited or failing request is retried or surfaces as a turn error, and
//! switching a live conversation to another family mid-turn would invalidate the
//! model-minted reasoning items already in it.
//!
//! Usage:
//! - `/fallback` — show the current chain
//! - `/fallback set model1,model2,model3` — replace the entire chain
//! - `/fallback add model1,model2` — append to the end of the chain
//! - `/fallback remove model1` — remove a model from the chain
//! - `/fallback clear` — empty the chain

use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

/// Manage the fallback model chain.
pub struct FallbackCommand;

impl SlashCommand for FallbackCommand {
    fn name(&self) -> &str {
        "fallback"
    }

    fn description(&self) -> &str {
        "查看或设置备用模型链（当前模型不可用时按顺序切换）"
    }

    fn usage(&self) -> &str {
        "/fallback [set|add|remove|clear] [model1,model2,...]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        apply_chain_command(&fallback_config_path(), args)
    }
}

// ── Config persistence ───────────────────────────────────────────────

/// The `config.toml` holding `[fallback].models`.
fn fallback_config_path() -> std::path::PathBuf {
    xai_grok_tools::util::grok_home::grok_home().join("config.toml")
}

/// Handle one `/fallback …` invocation against `[fallback].models` in `config`.
/// [`FallbackCommand::run`] passes the real config home; taking the path as an
/// argument keeps the whole chain testable without rewriting the user's own
/// `config.toml`.
fn apply_chain_command(config: &std::path::Path, args: &str) -> CommandResult {
    let args = args.trim();
    if args.is_empty() {
        return show_current_chain(load_fallback_models_from(config));
    }

    let (sub, rest) = split_subcommand(args);
    match sub {
        "set" => {
            let models = parse_models(rest);
            if models.is_empty() {
                return CommandResult::Error("用法：/fallback set model1,model2,...".into());
            }
            match persist_fallback_models_in(config, &models) {
                Ok(()) => {
                    CommandResult::Message(format!("备用模型链已设为：{}", models.join(" → ")))
                }
                Err(e) => CommandResult::Error(format!("保存失败：{e}")),
            }
        }
        "add" => {
            let to_add = parse_models(rest);
            if to_add.is_empty() {
                return CommandResult::Error("用法：/fallback add model1,model2,...".into());
            }
            let mut current = load_fallback_models_from(config);
            for m in &to_add {
                if !current.contains(m) {
                    current.push(m.clone());
                }
            }
            match persist_fallback_models_in(config, &current) {
                Ok(()) => CommandResult::Message(format!(
                    "已添加。当前备用模型链：{}",
                    chain_display(&current)
                )),
                Err(e) => CommandResult::Error(format!("保存失败：{e}")),
            }
        }
        "remove" => {
            let to_remove = parse_models(rest);
            if to_remove.is_empty() {
                return CommandResult::Error("用法：/fallback remove model1".into());
            }
            let mut current = load_fallback_models_from(config);
            current.retain(|m| !to_remove.contains(m));
            match persist_fallback_models_in(config, &current) {
                Ok(()) => CommandResult::Message(format!(
                    "已移除。当前备用模型链：{}",
                    chain_display(&current)
                )),
                Err(e) => CommandResult::Error(format!("保存失败：{e}")),
            }
        }
        "clear" => match persist_fallback_models_in(config, &[]) {
            Ok(()) => CommandResult::Message("备用模型链已清空。".into()),
            Err(e) => CommandResult::Error(format!("保存失败：{e}")),
        },
        _ => CommandResult::Error(format!(
            "未知子命令「{sub}」。可用：set / add / remove / clear"
        )),
    }
}

fn chain_display(models: &[String]) -> String {
    if models.is_empty() {
        "（空）".into()
    } else {
        models.join(" → ")
    }
}

fn show_current_chain(models: Vec<String>) -> CommandResult {
    if models.is_empty() {
        CommandResult::Message(
            "当前未设置备用模型链。\n用法：/fallback set model1,model2,...".into(),
        )
    } else {
        CommandResult::Message(format!(
            "当前备用模型链：{}\n\
             只在当前模型不可用时按顺序切换，不会在单次请求失败时重试。\n\
             子命令：set（替换）/ add（追加）/ remove（移除）/ clear（清空）",
            models.join(" → ")
        ))
    }
}

fn split_subcommand(args: &str) -> (&str, &str) {
    let mut parts = args.splitn(2, char::is_whitespace);
    let sub = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();
    (sub, rest)
}

fn parse_models(s: &str) -> Vec<String> {
    let mut seen = Vec::new();
    for entry in s.split(',').map(|t| t.trim()).filter(|t| !t.is_empty()) {
        if !seen.iter().any(|existing| existing == entry) {
            seen.push(entry.to_string());
        }
    }
    seen
}

/// Load the fallback model chain from `[fallback].models` in `config`.
///
/// A missing file, an unparsable document, and a non-array value all read as an
/// empty chain: a settings list must never be able to wedge config loading, and
/// an empty chain means "let the product pick", the behaviour from before the
/// setting existed.
pub fn load_fallback_models_from(config: &std::path::Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(config) else {
        return Vec::new();
    };
    let Ok(doc) = content.parse::<toml_edit::DocumentMut>() else {
        return Vec::new();
    };
    doc.get("fallback")
        .and_then(|t| t.get("models"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Persist the fallback model chain to `[fallback].models` in `config`, leaving
/// every other key untouched.
fn persist_fallback_models_in(config: &std::path::Path, models: &[String]) -> std::io::Result<()> {
    if let Some(parent) = config.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = std::fs::read_to_string(config).unwrap_or_default();
    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .unwrap_or_else(|_| toml_edit::DocumentMut::new());

    let arr = toml_edit::Array::from_iter(models.iter().map(|s| s.as_str()));
    doc["fallback"]["models"] = toml_edit::value(arr);

    std::fs::write(config, doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::model_state::ModelState;

    static DEFAULT_BUNDLE_STATE: crate::app::bundle::BundleState =
        crate::app::bundle::BundleState {
            has_cache: false,
            version: String::new(),
            personas: Vec::new(),
            roles: Vec::new(),
            agents: Vec::new(),
            skills: Vec::new(),
            persona_details: Vec::new(),
            role_details: Vec::new(),
        };

    fn make_ctx<'a>(models: &'a ModelState) -> CommandExecCtx<'a> {
        CommandExecCtx {
            models,
            session_id: None,
            bundle_state: &DEFAULT_BUNDLE_STATE,
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

    /// A `config.toml` under a throwaway directory, never the real config home.
    fn scratch_config() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("a scratch directory");
        let path = dir.path().join("config.toml");
        (dir, path)
    }

    #[test]
    fn absent_or_malformed_config_reads_as_an_empty_chain() {
        let (_dir, path) = scratch_config();
        assert!(
            load_fallback_models_from(&path).is_empty(),
            "a config that does not exist yet must read as an empty chain"
        );

        std::fs::write(&path, "this is = = not toml").expect("write a broken config");
        assert!(
            load_fallback_models_from(&path).is_empty(),
            "an unparsable config must read as an empty chain rather than panic"
        );

        std::fs::write(&path, "[fallback]\nmodels = \"grok-4\"\n").expect("write a bad type");
        assert!(
            load_fallback_models_from(&path).is_empty(),
            "a non-array `models` must read as an empty chain"
        );
    }

    #[test]
    fn set_add_and_remove_round_trip_through_the_real_config_file() {
        let (_dir, path) = scratch_config();

        let set = apply_chain_command(&path, "set a,b,c");
        assert!(
            matches!(set, CommandResult::Message(_)),
            "`set` must succeed, got {set:?}"
        );
        assert_eq!(
            load_fallback_models_from(&path),
            vec!["a", "b", "c"],
            "`set` did not persist the chain"
        );

        apply_chain_command(&path, "add c,d");
        assert_eq!(
            load_fallback_models_from(&path),
            vec!["a", "b", "c", "d"],
            "`add` must append in order without duplicating an entry already in the chain"
        );

        apply_chain_command(&path, "remove b,d");
        assert_eq!(
            load_fallback_models_from(&path),
            vec!["a", "c"],
            "`remove` must drop exactly the named entries"
        );

        apply_chain_command(&path, "clear");
        assert!(
            load_fallback_models_from(&path).is_empty(),
            "`clear` must empty the chain rather than delete the key's shape"
        );
    }

    #[test]
    fn editing_the_chain_preserves_every_other_key() {
        let (_dir, path) = scratch_config();
        std::fs::write(
            &path,
            "[model]\nname = \"chaos\"\n\n[adhd]\nenabled = true\n",
        )
        .expect("seed a config with other settings");

        apply_chain_command(&path, "set backup-model");

        let text = std::fs::read_to_string(&path).expect("the config is readable back");
        assert!(
            text.contains("name = \"chaos\"") && text.contains("enabled = true"),
            "editing the chain rewrote the config and dropped unrelated keys: {text}"
        );
    }

    /// The chain is only real if the shell's shipped config loader sees what this
    /// writer wrote: same table, same key, same element type. `new_from_toml_cfg`
    /// is the path every real config load funnels through, so the writer and the
    /// reader cannot drift apart silently.
    #[test]
    fn the_written_chain_is_read_by_the_shell_config_loader() {
        let (_dir, path) = scratch_config();
        apply_chain_command(&path, "set grok-4-fast,grok-4");

        let text = std::fs::read_to_string(&path).expect("the config is readable back");
        let doc: toml::Value = toml::from_str(&text).expect("the writer must produce valid TOML");
        let cfg = xai_grok_shell::agent::config::Config::new_from_toml_cfg(&doc)
            .expect("the config the writer produced must load");
        assert_eq!(
            cfg.fallback.models,
            vec!["grok-4-fast", "grok-4"],
            "`/fallback` wrote a chain the shell-side reader cannot parse: {text}"
        );
    }

    #[test]
    fn the_no_argument_form_reports_the_chain_and_its_limits() {
        let (_dir, path) = scratch_config();
        apply_chain_command(&path, "set m1,m2");

        let shown = apply_chain_command(&path, "");
        let CommandResult::Message(message) = shown else {
            panic!("bare `/fallback` must show the chain, got {shown:?}");
        };
        assert!(
            message.contains("m1 → m2"),
            "the report must list the chain: {message}"
        );
        assert!(
            message.contains("不可用"),
            "the report must state that the chain only applies when the model is \
             unavailable, rather than implying per-request retries: {message}"
        );
    }

    #[test]
    fn a_rejected_argument_leaves_the_config_alone() {
        let (_dir, path) = scratch_config();
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);

        let unknown = FallbackCommand.run(&mut ctx, "banana");
        match unknown {
            CommandResult::Error(message) => assert!(
                message.contains("set / add / remove / clear"),
                "the rejection must list the subcommands, got {message}"
            ),
            other => panic!("expected a usage error, got {other:?}"),
        }
        let empty_set = FallbackCommand.run(&mut ctx, "set");
        assert!(
            matches!(empty_set, CommandResult::Error(_)),
            "`set` with no models must be rejected, got {empty_set:?}"
        );
        assert!(
            !path.exists(),
            "a rejected argument must not create or modify config.toml"
        );
    }

    #[test]
    fn parse_models_deduplicates_and_trims() {
        assert_eq!(parse_models("a, b ,c"), vec!["a", "b", "c"]);
        assert_eq!(
            parse_models("a,a, b"),
            vec!["a", "b"],
            "a duplicated entry would be tried twice and tells the user nothing"
        );
        assert!(parse_models("").is_empty());
        assert!(parse_models(" , , ").is_empty());
    }

    #[test]
    fn split_subcommand_basic() {
        assert_eq!(split_subcommand("set a,b"), ("set", "a,b"));
        assert_eq!(split_subcommand("clear"), ("clear", ""));
        assert_eq!(split_subcommand(""), ("", ""));
    }
}
