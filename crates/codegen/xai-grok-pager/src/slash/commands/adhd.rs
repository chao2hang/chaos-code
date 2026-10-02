//! `/adhd` -- toggle ADHD skill integration.
//!
//! When enabled, the ADHD skill's system-prompt rules (from
//! https://github.com/uditakhourii/adhd) are appended to the agent's `rules`
//! and injected into the system prompt. `[adhd].enabled` is read once per agent
//! start (`acp::connect`), so a flip only takes effect after restarting Chaos.
//! The toggle is persisted to `[adhd].enabled` in the `config.toml` under the
//! Chaos home (`$CHAOS_HOME`, default `~/.chaos`).
//!
//! Usage:
//! - `/adhd` — toggle on/off
//! - `/adhd on` / `/adhd off` — set explicitly

use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

/// Toggle ADHD skill integration via `/adhd`.
pub struct AdhdCommand;

impl SlashCommand for AdhdCommand {
    fn name(&self) -> &str {
        "adhd"
    }

    fn description(&self) -> &str {
        "切换 ADHD 技能集成（开启后注入 ADHD 辅助规则）"
    }

    fn usage(&self) -> &str {
        "/adhd [on|off]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        apply_toggle(&adhd_config_path(), args)
    }
}

// ── Config persistence ───────────────────────────────────────────────

/// The `config.toml` holding `[adhd].enabled`.
fn adhd_config_path() -> std::path::PathBuf {
    xai_grok_tools::util::grok_home::grok_home().join("config.toml")
}

/// Handle one `/adhd [on|off]` invocation against the `[adhd].enabled` key in
/// `config`. [`AdhdCommand::run`] passes the real config home; taking the path
/// as an argument keeps the whole toggle testable without rewriting the user's
/// own `config.toml`.
fn apply_toggle(config: &std::path::Path, args: &str) -> CommandResult {
    let desired = match args.trim() {
        "" => Some(!load_adhd_enabled_from(config)),
        "on" | "true" | "1" | "yes" => Some(true),
        "off" | "false" | "0" | "no" => Some(false),
        _ => None,
    };
    let Some(new) = desired else {
        return CommandResult::Error(format!("未知参数「{}」。用法：/adhd [on|off]", args.trim()));
    };
    match persist_adhd_enabled_in(config, new) {
        Ok(()) => {
            if new {
                CommandResult::Message(
                    "ADHD 技能集成已开启（已写入 config.toml）。\n\
                     来源：https://github.com/uditakhourii/adhd\n\
                     ADHD 辅助规则在 agent 启动时注入，需要重启 Chaos 后才会生效。"
                        .into(),
                )
            } else {
                CommandResult::Message(
                    "ADHD 技能集成已关闭（已写入 config.toml）。\n\
                     已运行的 agent 仍保留原有规则，重启 Chaos 后不再注入。"
                        .into(),
                )
            }
        }
        Err(e) => CommandResult::Error(format!("保存失败：{e}")),
    }
}

/// Load the ADHD-enabled flag from `[adhd].enabled` in `config`.
///
/// A missing file, an unparsable document, and a non-bool value all read as
/// off: a settings toggle must never be able to wedge config loading.
pub fn load_adhd_enabled_from(config: &std::path::Path) -> bool {
    let Ok(content) = std::fs::read_to_string(config) else {
        return false;
    };
    let Ok(doc) = content.parse::<toml_edit::DocumentMut>() else {
        return false;
    };
    doc.get("adhd")
        .and_then(|t| t.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Set `[adhd].enabled` in `config`, leaving every other key untouched.
fn persist_adhd_enabled_in(config: &std::path::Path, enabled: bool) -> std::io::Result<()> {
    if let Some(parent) = config.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = std::fs::read_to_string(config).unwrap_or_default();
    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .unwrap_or_else(|_| toml_edit::DocumentMut::new());

    doc["adhd"]["enabled"] = toml_edit::value(enabled);

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
    fn absent_or_unparsable_config_reads_as_off() {
        let (_dir, path) = scratch_config();
        assert!(
            !load_adhd_enabled_from(&path),
            "a config that does not exist yet must read as off"
        );

        std::fs::write(&path, "this is = = not toml").expect("write a broken config");
        assert!(
            !load_adhd_enabled_from(&path),
            "an unparsable config must read as off rather than panic"
        );

        std::fs::write(&path, "[adhd]\nenabled = \"yes\"\n").expect("write a mistyped config");
        assert!(
            !load_adhd_enabled_from(&path),
            "a non-bool `enabled` must read as off"
        );
    }

    #[test]
    fn the_toggle_round_trips_through_the_real_config_file() {
        let (_dir, path) = scratch_config();

        let on = apply_toggle(&path, "");
        let CommandResult::Message(on_message) = on else {
            panic!("bare `/adhd` from off must turn it on, got {on:?}");
        };
        // The rules are injected once per agent start, so the receipt must not
        // promise a per-session effect the reader cannot deliver.
        assert!(
            on_message.contains("重启") && on_message.contains("config.toml"),
            "the on-receipt must state where the toggle was written and that it \
             needs an agent restart: {on_message}"
        );
        assert!(load_adhd_enabled_from(&path), "/adhd did not persist");

        let off = apply_toggle(&path, "");
        assert!(matches!(off, CommandResult::Message(_)));
        assert!(
            !load_adhd_enabled_from(&path),
            "a second bare `/adhd` must flip back off"
        );

        for arg in ["on", "true", "1", "yes"] {
            apply_toggle(&path, arg);
            assert!(load_adhd_enabled_from(&path), "`/adhd {arg}` must enable");
        }
        for arg in ["off", "false", "0", "no"] {
            apply_toggle(&path, arg);
            assert!(!load_adhd_enabled_from(&path), "`/adhd {arg}` must disable");
        }
    }

    #[test]
    fn toggling_preserves_every_other_key() {
        let (_dir, path) = scratch_config();
        std::fs::write(
            &path,
            "[model]\nname = \"chaos\"\n\n[adhd]\nenabled = false\n",
        )
        .expect("seed a config with other settings");

        apply_toggle(&path, "on");

        let text = std::fs::read_to_string(&path).expect("the config is readable back");
        assert!(
            text.contains("name = \"chaos\""),
            "the toggle rewrote the config and dropped unrelated keys: {text}"
        );
        assert!(load_adhd_enabled_from(&path));
    }

    #[test]
    fn an_unknown_argument_is_rejected_without_touching_the_config() {
        let (_dir, path) = scratch_config();
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        let result = AdhdCommand.run(&mut ctx, "banana");
        match result {
            CommandResult::Error(message) => assert!(
                message.contains("/adhd [on|off]"),
                "the rejection must state the usage, got {message}"
            ),
            other => panic!("expected a usage error, got {other:?}"),
        }
        assert!(
            !path.exists(),
            "a rejected argument must not create or modify config.toml"
        );
    }
}
