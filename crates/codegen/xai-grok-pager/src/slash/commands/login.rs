//! `/login` -- Chaos does not support browser login (kept for path compatibility).
//!
//! Not registered in `builtin_commands()`. Prefer `/provider` and `CHAOS.md`.

use crate::app::actions::Action;
use crate::slash::command::{AppCtx, CommandExecCtx, CommandResult, SlashCommand, slash_meta};
use xai_grok_config::{Capability, Distribution};

pub struct LoginCommand;

impl SlashCommand for LoginCommand {
    slash_meta! {
        name: "login",
        description: "Chaos 不支持账号登录；请使用 /provider 配置 API Key",
        usage: "/login",
    }

    /// Not offered where the build has no account; the router refuses a typed one.
    fn visible(&self, _ctx: &AppCtx) -> bool {
        Distribution::current().allows(Capability::AccountLogin)
    }

    fn run(&self, _ctx: &mut CommandExecCtx, _args: &str) -> CommandResult {
        // Fail closed: never start browser OIDC. Send users to provider config.
        CommandResult::Action(Action::OpenProviderModal {
            mode: crate::views::provider_modal::ProviderModalMode::List,
        })
    }
}
