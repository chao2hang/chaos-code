use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand, slash_meta};

pub struct RewindCommand;

impl SlashCommand for RewindCommand {
    slash_meta! {
        name: "rewind",
        aliases: ["undo"],
        description: "回退到先前一轮",
        usage: "/rewind",
        session_scoped: true,
    }

    fn run(&self, _ctx: &mut CommandExecCtx, _args: &str) -> CommandResult {
        CommandResult::Action(Action::RewindShowPicker)
    }
}
