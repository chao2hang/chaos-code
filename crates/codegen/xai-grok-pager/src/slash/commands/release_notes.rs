use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand, slash_meta};

pub struct ReleaseNotesCommand;

impl SlashCommand for ReleaseNotesCommand {
    slash_meta! {
        name: "release-notes",
        aliases: ["changelog"],
        description: "查看当前版本的更新说明",
        usage: "/release-notes",
    }

    fn run(&self, _ctx: &mut CommandExecCtx, _args: &str) -> CommandResult {
        let changelog = xai_grok_shell::util::changelog::ChangelogManager::new().fetch();
        match changelog.markdown {
            Some(content) => CommandResult::Action(Action::ShowReleaseNotes {
                title: "更新日志".to_string(),
                content: content.trim().to_string(),
            }),
            None => CommandResult::Error("暂无更新日志（离线且无本地缓存）。".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_notes_metadata() {
        let cmd = ReleaseNotesCommand;
        assert_eq!(cmd.name(), "release-notes");
        assert_eq!(cmd.aliases(), &["changelog"]);
        assert!(!cmd.takes_args());
    }

    #[test]
    fn release_notes_returns_action_or_error() {
        let models = crate::acp::model_state::ModelState::default();
        let mut ctx = super::super::tests::make_ctx(&models);
        let result = ReleaseNotesCommand.run(&mut ctx, "");
        assert!(
            matches!(result, CommandResult::Action(_) | CommandResult::Error(_)),
            "expected Action or Error, got {result:?}"
        );
    }
}
