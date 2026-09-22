//! Onboarding tutorial content (embedded markdown).
//!
//! Short, curated topics shown by the `/tutorial` overlay (strictly opt-in; nothing auto-shows).
//! Deliberately separate from [`crate::docs`] (the full how-to guides): these pages are bite-size intros that point at the guides for depth.

/// A compile-time tutorial topic. All fields are `&'static str`.
#[derive(Debug)]
pub struct TutorialTopic {
    /// Row title in the topic list.
    pub title: &'static str,
    /// Short right-column blurb in the topic list.
    pub blurb: &'static str,
    /// Embedded markdown page content.
    pub content: &'static str,
    /// Title of the primary how-to guide this page's "Go deeper" points at (must match a [`crate::docs`] title); `d` opens it in the overlay.
    pub go_deeper: Option<&'static str>,
}

macro_rules! topic {
    ($file:literal, $title:literal, $blurb:literal, $go_deeper:expr) => {
        TutorialTopic {
            title: $title,
            blurb: $blurb,
            content: include_str!(concat!("../docs/tutorial/", $file)),
            go_deeper: $go_deeper,
        }
    };
}

/// The tutorial topics, in display order, as a linear flow (the topic screen's `→` advances through them).
/// The flow: what carries over from other tools, send a prompt, feed it context, learn the screen, then the bigger features.
pub static TUTORIAL_TOPICS: &[TutorialTopic] = &[
    topic!(
        "01-coming-from-another-tool.md",
        "从 Claude、Cursor 或 Codex 迁移过来？",
        "你的设置、规则与技能都能沿用",
        Some("项目规则 (AGENTS.md)")
    ),
    topic!(
        "02-first-prompt.md",
        "第一次发提示",
        "发送、排队、取消",
        Some("快速上手")
    ),
    topic!(
        "03-attach-and-paste.md",
        "附加文件、图片与粘贴",
        "@文件、行范围、截图",
        Some("快速上手")
    ),
    topic!(
        "04-navigation.md",
        "熟悉界面导航",
        "焦点、滚动回溯、面板",
        Some("键盘快捷键")
    ),
    topic!(
        "05-slash-commands.md",
        "斜杠命令",
        "/help  /model  /resume 与 Ctrl+P",
        Some("斜杠命令")
    ),
    topic!(
        "06-worktrees.md",
        "并行工作：worktree",
        "在同一仓库上开隔离会话",
        Some("会话管理")
    ),
    topic!(
        "07-plan-and-permissions.md",
        "计划模式与权限",
        "动手前先审阅方案",
        Some("计划模式")
    ),
    topic!(
        "08-make-it-yours.md",
        "把它变成你的",
        "直接开口：AGENTS.md、记忆、主题",
        Some("项目规则 (AGENTS.md)")
    ),
    topic!("09-where-next.md", "接下来去哪", "指南、反馈与好习惯", None),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topics_are_valid() {
        for t in TUTORIAL_TOPICS {
            assert!(!t.title.is_empty(), "topic has empty title");
            assert!(!t.blurb.is_empty(), "topic {} has empty blurb", t.title);
            assert!(!t.content.is_empty(), "topic {} is empty", t.title);
            assert!(
                t.content.starts_with('#'),
                "topic {} should start with a markdown header",
                t.title
            );
        }
    }

    #[test]
    fn go_deeper_titles_resolve_to_real_guides() {
        // `d` on a topic page opens this guide; a typo'd title would turn the shortcut into a silent no-op
        for t in TUTORIAL_TOPICS {
            if let Some(title) = t.go_deeper {
                assert!(
                    crate::docs::find_doc(title).is_some(),
                    "topic {}: go_deeper {title:?} matches no how-to guide",
                    t.title
                );
            }
        }
    }

    #[test]
    fn topics_have_unique_titles() {
        let mut seen = std::collections::HashSet::new();
        for t in TUTORIAL_TOPICS {
            assert!(seen.insert(t.title), "duplicate topic title: {}", t.title);
        }
    }

    #[test]
    fn topics_stay_bite_size() {
        // The tutorial promises quick reads; keep each page short
        // Bump this limit only after re-checking a page still reads in under a minute
        for t in TUTORIAL_TOPICS {
            let lines = t.content.lines().count();
            assert!(
                lines <= 50,
                "topic {} is {} lines; keep tutorial pages bite-size (≤50)",
                t.title,
                lines
            );
        }
    }
}
