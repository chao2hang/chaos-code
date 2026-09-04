//! `/session-info`: show current session info (instant, not queued).

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand, slash_meta};

/// 显示会话信息 (session ID, cwd, model, context usage).
pub struct SessionInfoCommand;

impl SlashCommand for SessionInfoCommand {
    slash_meta! {
        name: "session-info",
        description: "显示会话信息",
        usage: "/session-info",
        session_scoped: true,
    }

    fn run(&self, ctx: &mut CommandExecCtx, _args: &str) -> CommandResult {
        if ctx.session_id.is_none() {
            return CommandResult::Error("No active session".to_string());
        }

        CommandResult::Action(Action::ShowSessionInfo)
    }
}
