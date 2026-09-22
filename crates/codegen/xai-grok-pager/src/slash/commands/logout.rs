//! `/logout` -- Chaos does not use session login (kept for path compatibility).
//!
//! Not registered in `builtin_commands()`. Clearing credentials is done by
//! editing `config.toml` / env keys, not browser logout.

use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand, slash_meta};

pub struct LogoutCommand;

impl SlashCommand for LogoutCommand {
    slash_meta! {
        name: "logout",
        description: "Chaos 无需退出登录；请修改 config.toml 中的 Provider 配置",
        usage: "/logout",
    }

    fn run(&self, _ctx: &mut CommandExecCtx, _args: &str) -> CommandResult {
        CommandResult::Message(logout_message())
    }
}

/// The `/logout` body, split out so a test can assert the citation without
/// standing up a `CommandExecCtx`.
///
/// The auth chapter is extracted to the resolved home by
/// `docs.rs::extract_user_guide_docs`, so it resolves offline. Naming the
/// repo-root `CHAOS.md` here would send an installed user to a file that only
/// exists in a checkout — the same "unreachable advice" defect this message was
/// rewritten to avoid.
fn logout_message() -> String {
    format!(
        "Chaos 不使用 Grok 登录会话。请编辑 {} 中的 \
             model_providers / env_key，或运行 /provider。详见 {}。",
        crate::util::display_user_grok_path(xai_grok_config::USER_CONFIG_FILENAME),
        crate::util::display_user_grok_path("docs/user-guide/02-authentication.md")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_cites_an_extracted_doc_not_the_repo_root_file() {
        let msg = logout_message();
        assert!(
            msg.contains("docs/user-guide/02-authentication.md"),
            "must cite the user guide chapter that ships to the config home, got: {msg}"
        );
        assert!(
            !msg.contains("CHAOS.md"),
            "CHAOS.md only exists in a checkout, so it is unreachable advice, got: {msg}"
        );
    }

    #[test]
    fn message_points_at_the_provider_config_path() {
        let msg = logout_message();
        assert!(
            msg.contains(xai_grok_config::USER_CONFIG_FILENAME),
            "must name the config file to edit, got: {msg}"
        );
        assert!(
            !msg.contains("grok login"),
            "must not point at a sign-in Chaos does not offer, got: {msg}"
        );
    }
}
