//! `/personas` -- open the agents modal on the Personas tab.

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand, slash_meta};
use crate::views::agents_modal::AgentsTab;

/// Open the agents modal directly on the Personas tab.
pub struct PersonasCommand;

impl SlashCommand for PersonasCommand {
    slash_meta! {
        name: "personas",
        description: "管理角色（创建、编辑、删除）",
        usage: "/personas",
    }

    fn run(&self, _ctx: &mut CommandExecCtx, _args: &str) -> CommandResult {
        CommandResult::Action(Action::OpenConfigAgentsModal(Some(AgentsTab::Personas)))
    }
}
