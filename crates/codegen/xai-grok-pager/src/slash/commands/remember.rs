use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand, slash_meta};

/// 保存一条记忆笔记 inline or enter remember mode.
pub struct RememberCommand;

impl SlashCommand for RememberCommand {
    slash_meta! {
        name: "remember",
        description: "保存一条记忆笔记",
        usage: "/remember [text]",
        takes_args: true,
        arg_placeholder: "[memory note text]",
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let trimmed = args.trim();
        if trimmed.is_empty() {
            CommandResult::Action(Action::EnterRememberMode)
        } else {
            CommandResult::Action(Action::SendRememberNote(trimmed.to_string()))
        }
    }
}
