//! `/logout` -- Chaos does not use session login (kept for path compatibility).
//!
//! Not registered in `builtin_commands()`. Clearing credentials is done by
//! editing `config.toml` / env keys, not browser logout.

use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

pub struct LogoutCommand;

impl SlashCommand for LogoutCommand {
    fn name(&self) -> &str {
        "logout"
    }

    fn description(&self) -> &str {
        "Chaos 无需退出登录；请修改 config.toml 中的 Provider 配置"
    }

    fn usage(&self) -> &str {
        "/logout"
    }

    fn run(&self, _ctx: &mut CommandExecCtx, _args: &str) -> CommandResult {
        CommandResult::Message(
            "Chaos 不使用 Grok 登录会话。请编辑 ~/.grok/config.toml 中的 \
             model_providers / env_key，或运行 /provider。详见 CHAOS.md。"
                .into(),
        )
    }
}
