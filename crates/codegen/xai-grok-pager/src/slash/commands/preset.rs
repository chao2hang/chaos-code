//! `/preset` — list available agent presets and show the active one.
//!
//! A preset bundles a toolset, persona, and prompt sections into a named
//! composition, switchable at session start via `--preset <name>` or in
//! `config.toml` under `[agent] preset`. Built-in presets:
//!
//! - `standard` — the full coding agent (default)
//! - `minimal` — bash + str_replace_editor only (low-token fast path)
//! - `grok-build` / `grok-build-concise` / `grok-build-plan` — Grok Build variants
//! - `codex` — Codex toolset and prompt
//! - `explore` — read-only exploration
//! - `plan` — planning agent
//!
//! Usage:
//! - `/preset` — list all presets
//! - `/preset <name>` — describe one preset (resolution happens at session start;
//!   use `--preset <name>` to apply)
//!
//! Mid-session switching requires an agent rebuild and is deferred; the
//! `--preset` flag and `[agent] preset` config are the application path, mirroring
//! how `--agent-profile` works.

use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

/// List agent presets or describe one.
pub struct PresetCommand;

impl SlashCommand for PresetCommand {
    fn name(&self) -> &str {
        "preset"
    }

    fn description(&self) -> &str {
        "列出可用 agent preset（工具集+persona+prompt 组合）"
    }

    fn session_scoped(&self) -> bool {
        true
    }

    fn usage(&self) -> &str {
        "/preset [name]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn args_required(&self) -> bool {
        false
    }

    fn arg_placeholder(&self) -> Option<&str> {
        Some("[standard]")
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let names = xai_grok_agent::agent_preset_names();
        let arg = args.trim();
        if arg.is_empty() {
            let mut lines = String::from(
                "可用 preset（用 --preset <name> 或 [agent] preset = \"<name>\" 应用）：\n",
            );
            for name in &names {
                let desc = describe_preset(name);
                lines.push_str(&format!("  • {name} — {desc}\n"));
            }
            lines.push_str(
                "\n切换需在会话启动时指定（--preset），或写入 config.toml 的 [agent] preset。",
            );
            return CommandResult::Message(lines);
        }
        let normalized = arg.trim().to_ascii_lowercase().replace([' ', '_'], "-");
        if let Some(def) = xai_grok_agent::agent_definition_for_preset(&normalized) {
            let tool_count = def.tool_config.tools.len();
            let body = if def.prompt_body.is_some() {
                "（含自定义 prompt body）"
            } else {
                ""
            };
            return CommandResult::Message(format!(
                "preset「{normalized}」：{desc}\n工具数：{tool_count} {body}\n\n用 --preset {normalized} 或 [agent] preset = \"{normalized}\" 应用。",
                desc = def.description
            ));
        }
        CommandResult::Message(format!("未知 preset「{arg}」。可用：{}", names.join(", ")))
    }
}

/// Short description for a preset id, falling back to the definition's
/// description for unknown names.
fn describe_preset(name: &str) -> &'static str {
    match name {
        "standard" => "全功能编码 agent（默认）",
        "minimal" => "仅 bash + 编辑器，低 token 快速路径",
        "grok-build" => "Grok Build 标准 agent",
        "grok-build-concise" => "Grok Build 精简输出格式",
        "grok-build-plan" => "Grok Build + 计划模式",
        "codex" => "Codex 工具集与提示词",
        "explore" => "只读探索 agent",
        "plan" => "规划 agent",
        _ => "（自定义 preset）",
    }
}
