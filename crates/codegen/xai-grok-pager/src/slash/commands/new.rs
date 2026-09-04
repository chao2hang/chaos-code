//! `/new` (alias `/clear`): create a new session.

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand, slash_meta};

/// Start a new agent session, clearing the current conversation.
pub struct NewCommand;

impl SlashCommand for NewCommand {
    slash_meta! {
        name: "new",
        aliases: ["clear"],
        description: "开始新会话",
        usage: "/new",
    }

    fn run(&self, _ctx: &mut CommandExecCtx, _args: &str) -> CommandResult {
        CommandResult::Action(Action::NewSession)
    }
}
