//! `/login` -- Chaos does not support browser login (kept for path compatibility).
//!
//! Not registered in `builtin_commands()`. Prefer `/provider` and `CHAOS.md`.

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

pub struct LoginCommand;

impl SlashCommand for LoginCommand {
    fn name(&self) -> &str {
        "login"
    }

    fn description(&self) -> &str {
        "Chaos 不支持账号登录；请使用 /provider 配置 API Key"
    }

    fn usage(&self) -> &str {
        "/login"
    }

    fn run(&self, _ctx: &mut CommandExecCtx, _args: &str) -> CommandResult {
        // Fail closed: never start browser OIDC. Send users to provider config.
        CommandResult::Action(Action::OpenProviderModal {
            mode: crate::views::provider_modal::ProviderModalMode::List,
        })
    }
}
