//! `/resume`: open the session picker overlay to resume a previous session.

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand, slash_meta};

pub struct ResumeCommand;

impl SlashCommand for ResumeCommand {
    slash_meta! {
        name: "resume",
        description: "恢复先前会话",
        usage: "/resume",
    }

    fn run(&self, _ctx: &mut CommandExecCtx, _args: &str) -> CommandResult {
        CommandResult::Action(Action::ShowSessionPicker)
    }
}
