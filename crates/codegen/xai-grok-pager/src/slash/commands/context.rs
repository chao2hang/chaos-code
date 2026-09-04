//! `/context` — show or set the session context window.
//!
//! - `/context` — show detailed usage (progress bar, token categories).
//! - `/context set <size>` — dynamically resize the current session window.
//!   Size accepts raw tokens (`128000`) or suffixes (`128k`, `1m`).
//!   Shrinking below current usage (or auto-compact threshold) triggers
//!   compaction so the conversation fits the new budget.
//! - `/context set <size> --no-compact` — only change the budget, never compact.

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand, slash_meta};

/// Show context usage breakdown, or set the window size.
pub struct ContextCommand;

impl SlashCommand for ContextCommand {
    slash_meta! {
        name: "context",
        aliases: ["ctx", "cw"],
        description: "查看或设置当前会话上下文窗口（可动态调小并压缩）",
        usage: "/context [set <size>] [--no-compact]",
        session_scoped: true,
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn args_required(&self) -> bool {
        false
    }

    fn arg_placeholder(&self) -> Option<&str> {
        Some("[set 128k]")
    }

    fn run(&self, ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        if ctx.session_id.is_none() {
            return CommandResult::Error("当前没有活动会话".to_string());
        }

        let trimmed = args.trim();
        if trimmed.is_empty() {
            return CommandResult::Action(Action::ShowContextInfo);
        }

        let (sub, rest) = split_first_token(trimmed);
        match sub {
            "set" | "window" | "size" => {
                let rest = rest.trim();
                if rest.is_empty() {
                    return CommandResult::Error(
                        "用法: /context set <size> [--no-compact]\n\
                         示例: /context set 128k\n\
                         单位: 纯数字、k/K（×1000）、m/M（×1_000_000）"
                            .into(),
                    );
                }
                let mut compact_if_needed = true;
                let mut size_token: Option<&str> = None;
                for part in rest.split_whitespace() {
                    if part == "--no-compact" || part == "-n" {
                        compact_if_needed = false;
                        continue;
                    }
                    if size_token.is_none() {
                        size_token = Some(part);
                    } else {
                        return CommandResult::Error(format!(
                            "无法识别参数: {part}\n用法: /context set <size> [--no-compact]"
                        ));
                    }
                }
                let Some(raw) = size_token else {
                    return CommandResult::Error("请提供上下文大小，例如 128k".into());
                };
                let tokens = match parse_token_size(raw) {
                    Ok(t) => t,
                    Err(e) => return CommandResult::Error(e),
                };
                CommandResult::Action(Action::SetContextWindow {
                    tokens,
                    compact_if_needed,
                })
            }
            "show" | "info" | "status" => CommandResult::Action(Action::ShowContextInfo),
            _ => {
                // Bare size: `/context 128k` as a convenience.
                if let Ok(tokens) = parse_token_size(sub) {
                    let rest_trim = rest.trim();
                    let compact_if_needed = match rest_trim {
                        "" | "--no-compact" | "-n" => rest_trim.is_empty(),
                        _ => {
                            return CommandResult::Error(format!(
                                "未知参数: {rest_trim}\n用法: /context [set] <size> [--no-compact]"
                            ));
                        }
                    };
                    return CommandResult::Action(Action::SetContextWindow {
                        tokens,
                        compact_if_needed,
                    });
                }
                CommandResult::Error(format!(
                    "未知子命令: {sub}\n可用: （无参数查看）, set <size>, show"
                ))
            }
        }
    }
}

fn split_first_token(s: &str) -> (&str, &str) {
    let s = s.trim();
    match s.split_once(char::is_whitespace) {
        Some((first, rest)) => (first, rest.trim()),
        None => (s, ""),
    }
}

/// Parse token sizes: `128000`, `128k`, `128K`, `1m`, `1.5m` (rounded).
pub(crate) fn parse_token_size(raw: &str) -> Result<u64, String> {
    let s = raw.trim().replace('_', "");
    if s.is_empty() {
        return Err("上下文大小不能为空".into());
    }
    let lower = s.to_ascii_lowercase();
    let (num_part, mult) = if let Some(rest) = lower.strip_suffix('m') {
        (rest, 1_000_000f64)
    } else if let Some(rest) = lower.strip_suffix('k') {
        (rest, 1_000f64)
    } else {
        (lower.as_str(), 1f64)
    };
    if num_part.is_empty() {
        return Err(format!("无效大小: {raw}"));
    }
    // Integer path first (no float rounding for pure ints).
    if mult == 1.0 {
        let n = num_part
            .parse::<u64>()
            .map_err(|_| format!("无效大小: {raw}（需要正整数，或带 k/m 后缀）"))?;
        if n == 0 {
            return Err(format!("上下文大小必须为正数: {raw}"));
        }
        if n > 10_000_000 {
            return Err(format!(
                "上下文大小过大 ({n})；上限 10M。请检查是否多写了位数。"
            ));
        }
        return Ok(n);
    }
    let n: f64 = num_part
        .parse()
        .map_err(|_| format!("无效大小: {raw}（需要数字，或带 k/m 后缀）"))?;
    if !n.is_finite() || n <= 0.0 {
        return Err(format!("上下文大小必须为正数: {raw}"));
    }
    let tokens = (n * mult).round() as u64;
    if tokens == 0 {
        return Err(format!("上下文大小太小: {raw}"));
    }
    if tokens > 10_000_000 {
        return Err(format!(
            "上下文大小过大 ({tokens})；上限 10M。请检查是否多写了位数。"
        ));
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plain_and_suffixes() {
        assert_eq!(parse_token_size("128000").unwrap(), 128_000);
        assert_eq!(parse_token_size("128k").unwrap(), 128_000);
        assert_eq!(parse_token_size("128K").unwrap(), 128_000);
        assert_eq!(parse_token_size("200_000").unwrap(), 200_000);
        assert_eq!(parse_token_size("1m").unwrap(), 1_000_000);
        assert_eq!(parse_token_size("1.5m").unwrap(), 1_500_000);
        assert_eq!(parse_token_size("0.5k").unwrap(), 500);
    }

    #[test]
    fn parse_rejects_bad() {
        assert!(parse_token_size("").is_err());
        assert!(parse_token_size("abc").is_err());
        assert!(parse_token_size("0").is_err());
        assert!(parse_token_size("100m").is_err()); // 100M > 10M cap
    }
}
