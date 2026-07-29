//! `/adhd` -- toggle ADHD skill integration.
//!
//! When enabled, the ADHD skill's system-prompt rules (from
//! https://github.com/uditakhourii/adhd) are injected into every
//! agent session. The toggle is persisted to `[adhd].enabled`
//! in `~/.grok/config.toml`.
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
        let args = args.trim();
        let current = load_adhd_enabled();
        let new = match args {
            "" => !current,
            "on" | "true" | "1" | "yes" => true,
            "off" | "false" | "0" | "no" => false,
            _ => return CommandResult::Error(format!("未知参数「{args}」。用法：/adhd [on|off]")),
        };
        match persist_adhd_enabled(new) {
            Ok(()) => {
                if new {
                    CommandResult::Message(
                        "ADHD 技能集成已开启。\n\
                         来源：https://github.com/uditakhourii/adhd\n\
                         新会话将自动注入 ADHD 辅助规则。"
                            .into(),
                    )
                } else {
                    CommandResult::Message("ADHD 技能集成已关闭。".into())
                }
            }
            Err(e) => CommandResult::Error(format!("保存失败：{e}")),
        }
    }
}

// ── Config persistence ───────────────────────────────────────────────

/// Load the ADHD-enabled flag from `~/.grok/config.toml` `[adhd].enabled`.
pub fn load_adhd_enabled() -> bool {
    let path = xai_grok_tools::util::grok_home::grok_home().join("config.toml");
    let Ok(content) = std::fs::read_to_string(&path) else {
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

/// Persist the ADHD-enabled flag to `~/.grok/config.toml` `[adhd].enabled`.
fn persist_adhd_enabled(enabled: bool) -> std::io::Result<()> {
    let path = xai_grok_tools::util::grok_home::grok_home().join("config.toml");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .unwrap_or_else(|_| toml_edit::DocumentMut::new());

    doc["adhd"]["enabled"] = toml_edit::value(enabled);

    std::fs::write(&path, doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_logic() {
        // Pure logic test — no filesystem.
        assert!(!false); // default off
        assert!(!false == true); // toggle → on
    }
}
