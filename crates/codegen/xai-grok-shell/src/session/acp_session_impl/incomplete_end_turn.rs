//! Incomplete `end_turn` auto-retry: detect premature model stops and
//! inject a recovery reminder so the outer prompt loop samples again.
//!
//! Opt-in via `[session] auto_retry_incomplete_end_turn` (default off).
//! See issue #6: model ends with a plan-only message after tools, no writes.

/// Reminder injected as [`ConversationItem::auto_recovery`] when retrying.
pub(crate) const INCOMPLETE_END_TURN_RECOVERY_PROMPT: &str = "\
[System] The previous response ended before finishing the user's request \
(tools were used earlier, but no write/edit tools ran, and the last message \
looked like a plan rather than a completed action). Continue the work now. \
Do not only restate the plan — call tools to make the change when edits are needed.";

/// Inputs for the pure incomplete-`end_turn` detector.
#[derive(Debug, Clone)]
pub(crate) struct IncompleteEndTurnInput<'a> {
    pub enabled: bool,
    pub retries_so_far: u8,
    pub max_retries: u8,
    /// Tools invoked during the completed conversation turn (this outer round).
    pub tools_called: &'a [String],
    /// Last assistant visible text for this completed round.
    pub last_assistant_text: &'a str,
}

/// Why a retry was requested (telemetry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IncompleteEndTurnReason {
    /// Intent / "I'll do X next" phrasing after tools, no writes.
    IntentWithoutWrite,
    /// Short trailing message after multiple tools, no writes.
    ShortAfterTools,
}

impl IncompleteEndTurnReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::IntentWithoutWrite => "intent_without_write",
            Self::ShortAfterTools => "short_after_tools",
        }
    }
}

/// Canonical write/edit tool names that count as "work landed on disk".
/// Chaos primary mutators first; then codex/opencode aliases and Claude-style ids.
const WRITE_TOOL_NAMES: &[&str] = &[
    "search_replace",
    "write",
    "hashline_edit",
    "apply_patch",
    "edit",
    "Edit",
    "Write",
    "MultiEdit",
    "NotebookEdit",
];

/// Max character length for the "short after tools" heuristic.
const SHORT_TEXT_MAX_CHARS: usize = 80;

/// Minimum tools in the round before the short-text heuristic applies.
const SHORT_TEXT_MIN_TOOLS: usize = 2;

/// Whether any tool name is a write/edit tool.
pub(crate) fn tools_include_write(tools: &[String]) -> bool {
    tools.iter().any(|name| is_write_tool_name(name))
}

pub(crate) fn is_write_tool_name(name: &str) -> bool {
    WRITE_TOOL_NAMES.iter().any(|w| *w == name)
}

/// Pure decision: should the outer loop inject recovery and sample again?
pub(crate) fn should_retry_incomplete_end_turn(
    input: &IncompleteEndTurnInput<'_>,
) -> Option<IncompleteEndTurnReason> {
    if !input.enabled {
        return None;
    }
    if input.retries_so_far >= input.max_retries {
        return None;
    }
    if input.tools_called.is_empty() {
        return None;
    }
    if tools_include_write(input.tools_called) {
        return None;
    }

    let text = input.last_assistant_text.trim();
    if text.is_empty() {
        // Empty text with tools already ran is odd; leave to other recovery paths.
        return None;
    }

    if looks_like_intent_to_continue(text) {
        return Some(IncompleteEndTurnReason::IntentWithoutWrite);
    }

    if input.tools_called.len() >= SHORT_TEXT_MIN_TOOLS
        && text.chars().count() <= SHORT_TEXT_MAX_CHARS
    {
        return Some(IncompleteEndTurnReason::ShortAfterTools);
    }

    None
}

/// Phrases that indicate the work is already done, not a plan to continue.
/// When present, suppress the intent-to-continue match to avoid false positives
/// like "接下来我已完成所有修改".
const COMPLETION_MARKERS: &[&str] = &[
    // Chinese
    "已完成",
    "已修改",
    "已更新",
    "已创建",
    "已删除",
    "已写入",
    "已保存",
    "成功了",
    "搞定了",
    "做完了",
    // English
    "done",
    "finished",
    "completed",
    "successfully ",
    "has been ",
    "i've updated",
    "i've modified",
    "i've created",
    "i've written",
];

/// Conservative bilingual intent / plan-only markers.
fn looks_like_intent_to_continue(text: &str) -> bool {
    // If the text contains a completion marker, it's likely a summary, not a plan.
    let lower = text.to_lowercase();
    if COMPLETION_MARKERS.iter().any(|m| {
        if m.chars().all(|c| c.is_ascii()) {
            lower.contains(m)
        } else {
            text.contains(m)
        }
    }) {
        return false;
    }

    const MARKERS: &[&str] = &[
        // Chinese
        "接下来",
        "接着",
        "我来",
        "我会",
        "将把",
        "将改",
        "准备改",
        "改成中文",
        "与其它命令一致",
        // English
        "i'll ",
        "i will ",
        "let me ",
        "going to ",
        "next i'll",
        "next i will",
        "i'm going to",
        "now i'll",
        "now i will",
        "proceed to",
        "continue to",
    ];
    MARKERS.iter().any(|m| {
        if m.chars().all(|c| c.is_ascii()) {
            lower.contains(m)
        } else {
            text.contains(m)
        }
    })
}

/// Extract the last non-empty assistant text from a conversation snapshot
/// (newest first). Skips synthetic system/user recovery items.
pub(crate) fn last_assistant_text_from_conversation(
    items: &[xai_grok_sampling_types::ConversationItem],
) -> String {
    use xai_grok_sampling_types::{ConversationItem, Role};
    for item in items.iter().rev() {
        if item.role() != Role::Assistant {
            continue;
        }
        // Prefer pure Assistant text; skip reasoning-only siblings already handled by role.
        if matches!(item, ConversationItem::Reasoning(_) | ConversationItem::BackendToolCall(_)) {
            continue;
        }
        let text = item.text_content();
        if !text.trim().is_empty() {
            return text;
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(
        tools: &'a [String],
        text: &'a str,
        enabled: bool,
        retries: u8,
        max: u8,
    ) -> IncompleteEndTurnInput<'a> {
        IncompleteEndTurnInput {
            enabled,
            retries_so_far: retries,
            max_retries: max,
            tools_called: tools,
            last_assistant_text: text,
        }
    }

    #[test]
    fn disabled_never_retries() {
        let tools = vec!["grep".into()];
        assert!(
            should_retry_incomplete_end_turn(&input(
                &tools,
                "接下来把说明改成中文",
                false,
                0,
                1
            ))
            .is_none()
        );
    }

    #[test]
    fn no_tools_never_retries() {
        assert!(
            should_retry_incomplete_end_turn(&input(
                &[],
                "接下来改代码",
                true,
                0,
                1
            ))
            .is_none()
        );
    }

    #[test]
    fn write_tool_blocks_retry() {
        let tools = vec!["grep".into(), "search_replace".into()];
        assert!(
            should_retry_incomplete_end_turn(&input(
                &tools,
                "接下来再检查一下",
                true,
                0,
                1
            ))
            .is_none()
        );
    }

    #[test]
    fn issue6_chinese_intent_retries() {
        let tools = vec!["grep".into(), "read_file".into(), "run_terminal_command".into()];
        let reason = should_retry_incomplete_end_turn(&input(
            &tools,
            "把 `/doctor` 的 slash 说明和补全文案改成中文，与其它命令一致。",
            true,
            0,
            1,
        ));
        assert_eq!(reason, Some(IncompleteEndTurnReason::IntentWithoutWrite));
    }

    #[test]
    fn english_let_me_retries() {
        let tools = vec!["grep".into()];
        let reason = should_retry_incomplete_end_turn(&input(
            &tools,
            "Let me update the doctor description next.",
            true,
            0,
            1,
        ));
        assert_eq!(reason, Some(IncompleteEndTurnReason::IntentWithoutWrite));
    }

    #[test]
    fn finished_summary_does_not_retry() {
        let tools = vec!["grep".into(), "search_replace".into()];
        // has write — blocked even with intent-ish words
        assert!(
            should_retry_incomplete_end_turn(&input(
                &tools,
                "已完成 doctor 说明的中文化。",
                true,
                0,
                1
            ))
            .is_none()
        );
        let tools_no_write = vec!["grep".into()];
        // long finished-looking text without intent markers
        assert!(
            should_retry_incomplete_end_turn(&input(
                &tools_no_write,
                "根据代码，`/doctor` 的 description 仍是英文：Check this session。其它 slash 命令已是中文。若要汉化，需要改 doctor.rs 中的 description 字段。",
                true,
                0,
                1
            ))
            .is_none()
        );
    }

    #[test]
    fn completion_marker_suppresses_intent_match() {
        // "接下来" would normally trigger IntentWithoutWrite, but the
        // completion marker "已完成" suppresses it.
        let tools = vec!["grep".into()];
        assert!(
            should_retry_incomplete_end_turn(&input(
                &tools,
                "接下来我已完成所有修改",
                true,
                0,
                1
            ))
            .is_none()
        );
        // English completion marker suppresses "I'll" intent.
        assert!(
            should_retry_incomplete_end_turn(&input(
                &tools,
                "I'll verify the changes - done.",
                true,
                0,
                1
            ))
            .is_none()
        );
    }

    #[test]
    fn short_after_multiple_tools_retries() {
        let tools = vec!["grep".into(), "read_file".into()];
        let reason = should_retry_incomplete_end_turn(&input(
            &tools,
            "好的，准备动手。",
            true,
            0,
            1,
        ));
        assert_eq!(reason, Some(IncompleteEndTurnReason::ShortAfterTools));
    }

    #[test]
    fn max_retries_respected() {
        let tools = vec!["grep".into()];
        assert!(
            should_retry_incomplete_end_turn(&input(
                &tools,
                "Let me fix it.",
                true,
                1,
                1
            ))
            .is_none()
        );
    }

    #[test]
    fn is_write_tool_names() {
        assert!(is_write_tool_name("search_replace"));
        assert!(is_write_tool_name("write"));
        assert!(is_write_tool_name("hashline_edit"));
        assert!(is_write_tool_name("apply_patch"));
        assert!(is_write_tool_name("edit"));
        assert!(!is_write_tool_name("grep"));
        assert!(!is_write_tool_name("run_terminal_command"));
    }
}
