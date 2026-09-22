//! Default action definitions for the MVP.
//!
//! All key bindings are defined here, not scattered across event handlers.

use crate::key;
use crate::terminal::{TerminalName, terminal_context};

use super::{ActionDef, ActionId, Category, When};

/// True when `Ctrl+.` is not a reliable primary key for the shortcuts cheatsheet.
///
/// Callers pick an alternate primary that can arrive (`Ctrl+X` on the agent screen, `?` on the dashboard).
/// Both keys stay registered either way; this only chooses which the UI advertises.
///
/// Driven by [`crate::terminal::TerminalContext::ctrl_dot_unreliable`] (any KKP skip: brand, tmux `extended-keys off`, screen, unknown host).
/// Host-OS signals add to it: native Windows on a non-branded console, or a Linux binary inside Win32's console pipeline (WSL).
pub fn ctrl_dot_unreliable() -> bool {
    terminal_context().ctrl_dot_unreliable() || cfg!(target_os = "windows") || crate::host::is_wsl()
}

/// Choose the one agent-screen action that owns Ctrl+G for this mode.
fn mode_ctrl_g_action(screen_mode: crate::app::ScreenMode) -> ActionDef {
    if screen_mode.is_minimal() {
        ActionDef {
            id: ActionId::EditPromptExternal,
            label: "编辑提示",
            description: "在外部编辑器中编辑提示",
            default_key: key!('g', CONTROL),
            alt_keys: vec![],
            category: Category::Input,
            context: When::AgentScreen,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "在 $VISUAL 或 $EDITOR 中打开当前提示草稿，两者都未设置时回退到 vi。\n保存并关闭编辑器后，更新过的文本回到输入框；不会直接发送提示。\n在最小模式下，对未附带附件的普通草稿同样可用。",
            ),
        }
    } else {
        ActionDef {
            id: ActionId::ToggleTasks,
            label: "任务",
            description: "切换任务面板",
            default_key: key!('g', CONTROL),
            alt_keys: vec![],
            category: Category::Panels,
            context: When::AgentScreen,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "显示或隐藏任务面板，其中列出后台任务及其状态。\n用它查看或回到你用 Ctrl+B 送到后台的工作。\n这是侧边面板；关掉可回收宽度。",
            ),
        }
    }
}

/// Build the default action definitions for a screen mode.
///
/// `mouse_reporting_toggle_enabled` gates the opt-in `ToggleMouseCapture` shortcut (see below); pass `false` for the standard set.
pub(super) fn default_actions(
    screen_mode: crate::app::ScreenMode,
    mouse_reporting_toggle_enabled: bool,
) -> Vec<ActionDef> {
    let ctx = terminal_context();
    // xterm.js embeds have no KKP and the host often steals Ctrl+I
    // Share one family flag for quit / half-page / interject so VS Code-family embeds match VS Code
    let in_vscode_family = ctx.brand.is_vscode_family();
    let in_vscode = in_vscode_family;
    let in_apple_terminal = ctx.brand == TerminalName::AppleTerminal;
    // Shared by ToggleQueue (Ctrl+4 primary) and OpenDashboard (omit Ctrl+4 alt).
    let local_mac_vscode = in_vscode_family && !ctx.is_ssh && cfg!(target_os = "macos");
    let ctrl_dot_unreliable = ctrl_dot_unreliable();
    let send_to_background_help = if screen_mode.is_minimal() {
        "让正在前台运行的 Execute 脱离，转而在后台继续工作，同时你可以阅读、排队提示或开始别的事。\n用 /tasks 跟踪后台工作。\n仅当前台确实有 Execute 在运行时才有意义。"
    } else {
        "让正在前台运行的 Execute 脱离，转而在后台继续工作，同时你可以阅读、排队提示或开始别的事。\n在任务面板（Ctrl+G）中跟踪并恢复它。\n仅当前台确实有 Execute 在运行时才有意义。"
    };

    let mut actions = vec![
        // ── Navigation (scrollback) ─────────────────────────────────
        ActionDef {
            id: ActionId::SelectNext,
            label: "导航",
            description: "选择下一项",
            default_key: key!('j'),
            alt_keys: vec![key!(Down)],
            category: Category::ConversationNav,
            context: When::ScrollbackFocused,
            hint_priority: Some(0),
            hint_key_display: Some("j/k"),
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::SelectPrev,
            label: "导航",
            description: "选择上一项",
            default_key: key!('k'),
            alt_keys: vec![key!(Up)],
            category: Category::ConversationNav,
            context: When::ScrollbackFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::NextTurn,
            label: "轮次",
            description: "下一轮",
            default_key: key!('L'),
            alt_keys: vec![key!(Right, SHIFT)],
            category: Category::ConversationNav,
            context: When::ScrollbackFocused,
            hint_priority: Some(1),
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::PrevTurn,
            label: "轮次",
            description: "上一轮",
            default_key: key!('H'),
            alt_keys: vec![key!(Left, SHIFT)],
            category: Category::ConversationNav,
            context: When::ScrollbackFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::NextResponse,
            label: "回复",
            description: "下一条回复",
            default_key: key!('J'),
            alt_keys: vec![],
            category: Category::ConversationNav,
            context: When::ScrollbackFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::PrevResponse,
            label: "回复",
            description: "上一条回复",
            default_key: key!('K'),
            alt_keys: vec![],
            category: Category::ConversationNav,
            context: When::ScrollbackFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::GotoTop,
            label: "顶/底",
            description: "跳到顶部",
            default_key: key!('g'),
            alt_keys: vec![],
            category: Category::ConversationNav,
            context: When::ScrollbackFocused,
            hint_priority: Some(4),
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::GotoBottom,
            label: "底部",
            description: "跳到底部",
            default_key: key!('G'),
            alt_keys: vec![],
            category: Category::ConversationNav,
            context: When::ScrollbackFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::ScrollUp,
            label: "向上滚动",
            description: "向上滚动一行",
            default_key: key!('k', CONTROL),
            alt_keys: vec![],
            category: Category::ConversationNav,
            context: When::ScrollbackFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::ScrollDown,
            label: "向下滚动",
            description: "向下滚动一行",
            default_key: key!('j', CONTROL),
            alt_keys: vec![],
            category: Category::ConversationNav,
            context: When::ScrollbackFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::HalfPageUp,
            label: "上半页",
            description: "向上滚动半页",
            default_key: key!('u', CONTROL),
            alt_keys: vec![],
            category: Category::ConversationNav,
            context: When::ScrollbackFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::HalfPageDown,
            label: "下半页",
            description: "向下滚动半页",
            default_key: if in_vscode {
                key!('D')
            } else {
                key!('d', CONTROL)
            },
            alt_keys: vec![],
            category: Category::ConversationNav,
            context: When::ScrollbackFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::PageUp,
            label: "上一页",
            description: "向上滚动一页",
            default_key: key!(PageUp),
            alt_keys: vec![],
            category: Category::ConversationNav,
            context: When::ScrollbackFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::PageDown,
            label: "下一页",
            description: "向下滚动一页",
            default_key: key!(PageDown),
            alt_keys: vec![],
            category: Category::ConversationNav,
            context: When::ScrollbackFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        // ── View (scrollback) ───────────────────────────────────────
        ActionDef {
            id: ActionId::Collapse,
            label: "折叠",
            description: "折叠选中项",
            default_key: key!('h'),
            alt_keys: vec![key!(Left)],
            category: Category::ConversationAction,
            context: When::ScrollbackFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::Expand,
            label: "折叠",
            description: "展开选中项",
            default_key: key!('l'),
            alt_keys: vec![key!(Right)],
            category: Category::ConversationAction,
            context: When::ScrollbackFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::ToggleFold,
            label: "折叠",
            description: "展开 / 折叠",
            default_key: key!('e'),
            alt_keys: vec![],
            category: Category::ConversationAction,
            context: When::ScrollbackFocused,
            hint_priority: Some(3),
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "折叠或展开选中的历史条目，以隐藏或显示其完整正文。\n适合快速浏览很长的工具输出或推理过程。\n相关：E 折叠/展开全部条目，Ctrl+E 切换所有思考块。",
            ),
        },
        ActionDef {
            id: ActionId::ToggleExpandAll,
            label: "全部",
            description: "全部展开 / 全部折叠",
            default_key: key!('E'),
            alt_keys: vec![],
            category: Category::ConversationAction,
            context: When::ScrollbackFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "一次折叠或展开所有历史条目，而小写 e 只切换选中的那一行。\n可以先把长记录折叠起来只扫标题，再整体展开。\n思考块有自己的开关：Ctrl+E。",
            ),
        },
        ActionDef {
            id: ActionId::ExpandAllThinking,
            label: "展开/折叠思考",
            description: "切换全部思考块",
            default_key: key!('e', CONTROL),
            alt_keys: vec![],
            category: Category::ConversationAction,
            context: When::ScrollbackFocused,
            hint_priority: Some(3),
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "按一次键，就显示或隐藏整段记录中代理的推理（思考）块。\n既能看清代理如何得出结论，也能隐藏推理专注结果。\n与 E 不同：E 折叠每个条目，不分类型。",
            ),
        },
        ActionDef {
            id: ActionId::ToggleRaw,
            label: "原始",
            description: "切换原始 Markdown",
            default_key: key!('r'),
            alt_keys: vec![],
            category: Category::ConversationAction,
            context: When::ScrollbackFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "在选中条目的渲染后 markdown 与原始源文之间切换。\n可用来复制精确的 markdown、查看链接目标，或看到渲染器隐藏的格式。\n再按一次回到渲染视图。",
            ),
        },
        // ── Block content ────────────────────────────────────────────
        ActionDef {
            id: ActionId::CopyBlockContent,
            label: "复制",
            description: "复制内容",
            default_key: key!('y'),
            alt_keys: vec![],
            category: Category::ConversationAction,
            context: When::ScrollbackFocused,
            hint_priority: None, // shown dynamically when block supports copy
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "把选中块的正文复制到剪贴板：消息文本、完整工具输出，或代码块内容。\n只在支持复制的块上提供。\n若只要命令或文件路径，请改用 Y。",
            ),
        },
        ActionDef {
            id: ActionId::CopyBlockMeta,
            label: "复制命令",
            description: "复制命令 / 路径",
            default_key: key!('Y'),
            alt_keys: vec![],
            category: Category::ConversationAction,
            context: When::ScrollbackFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "只复制块的标识：工具调用的命令行，或文件块的路径，不含正文。\n适合重新执行命令，或把路径粘贴到别处。\n要复制完整内容请改用小写 y。",
            ),
        },
        ActionDef {
            id: ActionId::OpenBlockViewer,
            label: "查看",
            description: "在查看器中打开",
            default_key: key!(Enter),
            alt_keys: vec![key!('f', CONTROL)],
            category: Category::ConversationAction,
            context: When::ScrollbackFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "在聚焦的可滚动全屏查看器中打开选中块。\n最适合很长的工具输出、大文件，或想脱离上下文单独阅读的代码。\nEsc 返回对话。",
            ),
        },
        // ── Link navigation ─────────────────────────────────────────
        ActionDef {
            id: ActionId::OpenNextLink,
            label: "链接",
            description: "下一个链接",
            default_key: key!('o'),
            alt_keys: vec![],
            category: Category::ConversationNav,
            context: When::ScrollbackFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::OpenPrevLink,
            label: "链接",
            description: "上一个链接",
            default_key: key!('O'),
            alt_keys: vec![],
            category: Category::ConversationNav,
            context: When::ScrollbackFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        // ── Scrollback (contextual, block-type-dependent) ────────────
        ActionDef {
            id: ActionId::Rewind,
            label: "回退",
            description: "回退到选中轮次",
            default_key: key!(Null),
            alt_keys: vec![],
            category: Category::ConversationAction,
            context: When::ScrollbackFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "把对话回退到更早的轮次，恢复当时的文件快照，并丢弃其后的改动。\n从列表中选择轮次并决定恢复范围（全部、仅对话或仅文件）；若有轮次正在运行，会先询问是否取消，冲突或错误会在执行后报告。\n破坏性操作：之后的轮次会被丢弃。\n提示为空且空闲时，连按 Esc Esc（800 毫秒内）也可触发，与 `/rewind` 相同。",
            ),
        },
        ActionDef {
            id: ActionId::KillBgTask,
            label: "终止",
            description: "终止后台任务",
            default_key: key!('x'),
            alt_keys: vec![],
            category: Category::ConversationAction,
            context: When::ScrollbackFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "终止选中任务块所拥有的后台任务（例如送到后台的长 shell 命令）。\n用来停掉失控或已不需要的进程。\n只对仍在运行的任务有效；已完成的不会受影响。",
            ),
        },
        // ── Essentials ────────────────────────────────────────────────
        ActionDef {
            id: ActionId::SendPrompt,
            label: "发送",
            description: "发送",
            default_key: key!(Enter),
            alt_keys: vec![],
            category: Category::GettingStarted,
            context: When::PromptFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::FocusPrompt,
            label: "提示",
            description: "聚焦提示输入",
            default_key: key!(Tab),
            alt_keys: vec![key!('i'), key!(' ')],
            category: Category::GettingStarted,
            context: When::ScrollbackFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::FocusScrollback,
            label: "滚动历史",
            description: "聚焦滚动历史",
            default_key: key!(Tab),
            alt_keys: vec![],
            category: Category::GettingStarted,
            context: When::PromptFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "把焦点从提示框移到历史记录，以便浏览对话。\n在简单与 vim 两种历史模式下 Tab 都可用。\nEsc 保留给清空/回退（空闲时）策略，不用于切换焦点。",
            ),
        },
        ActionDef {
            id: ActionId::CancelTurn,
            label: "取消",
            description: "取消当前轮次",
            default_key: key!('c', CONTROL),
            alt_keys: vec![],
            category: Category::GettingStarted,
            context: When::AgentScreen,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "打断代理当前轮次并停止生成，会话仍保持打开。\n提示为空时 Ctrl+C 直接取消；草稿非空时会先清空提示，轮次继续运行。\n它停止的是轮次而非程序；退出请用退出快捷键。",
            ),
        },
        ActionDef {
            id: ActionId::CycleMode,
            label: "模式",
            description: "循环模式（普通 / 计划 / 总是批准）",
            // All Shift+Tab encodings — see `input::key::shift_tab_keys()`.
            default_key: crate::input::key::shift_tab_keys()[0],
            alt_keys: crate::input::key::shift_tab_keys()[1..].to_vec(),
            category: Category::GettingStarted,
            context: When::PromptFocused,
            hint_priority: None,
            hint_key_display: Some("Shift+Tab"),
            requires_confirmation: false,
            long_help: Some(
                "循环切换会话模式：普通 -> 计划 -> 始终批准 -> 普通。\n计划模式让代理先规划，不写文件；始终批准则不再询问就执行每次工具调用。\nCtrl+O 可直接切换自动批准。",
            ),
        },
        // ── Panes (agent-level: toggle side panes) ─────────────────
        mode_ctrl_g_action(screen_mode),
        ActionDef {
            id: ActionId::ToggleTodos,
            label: "待办",
            description: "切换待办面板",
            default_key: key!('t', CONTROL),
            alt_keys: vec![],
            category: Category::Panels,
            context: When::AgentScreen,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "显示或隐藏待办面板：代理当前工作的实时任务清单。\n观察这一轮里它打算做什么、还剩什么。\n这是侧边面板；关掉可回收宽度。",
            ),
        },
        ActionDef {
            id: ActionId::ToggleQueue,
            label: "队列",
            description: "切换提示队列",
            // Local macOS VS Code family only: ; / ' often never arrive (saw
            // Ctrl+4 in input-debug). SSH and non-Mac keep ; (+ ' alt). Win/Linux
            // VS maps Ctrl+4 to focusFourthEditorGroup.
            default_key: if in_vscode_family && !ctx.is_ssh && cfg!(target_os = "macos") {
                key!('4', CONTROL)
            } else {
                key!(';', CONTROL)
            },
            // Apostrophe alt for consoles that drop Ctrl on `;`
            // Local Mac VS also keeps ; / ' as alts alongside primary Ctrl+4
            alt_keys: if local_mac_vscode {
                vec![key!(';', CONTROL), key!('\'', CONTROL)]
            } else {
                vec![key!('\'', CONTROL)]
            },
            category: Category::Panels,
            context: When::AgentScreen,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "显示或隐藏提示队列。\n队列让你在轮次运行时排入后续提示；代理完成后会自动逐个发送。\nmacOS VS Code 系列本地端：主键 Ctrl+4（备用 Ctrl+; / Ctrl+'）。其他情况主键 Ctrl+;，备用 Ctrl+'。",
            ),
        },
        ActionDef {
            id: ActionId::OpenSessions,
            label: "会话",
            description: "打开会话列表",
            default_key: key!(F(3)),
            alt_keys: vec![],
            category: Category::Panels,
            context: When::AgentScreen,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "打开会话浏览器，以恢复或在过去的对话之间切换。\n选中一个即可重新接上它的完整历史。`/resume` 作用相同。\n与代理看板（Ctrl+\\）不同：看板同时管理多个在跑的代理。",
            ),
        },
        ActionDef {
            id: ActionId::OpenExtensions,
            label: "扩展",
            description: "打开扩展",
            // VS Code family: Ctrl+L is interject; plugins via /plugins (no chord here).
            default_key: if in_vscode_family {
                key!(Null)
            } else {
                key!('l', CONTROL)
            },
            alt_keys: vec![],
            category: Category::Panels,
            context: When::AgentScreen,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "打开扩展管理器，管理 MCP 服务器与插件：查看已连接的内容及其提供的工具。\n可用它确认某个集成是否加载，或浏览可用工具。\n与设置不同，设置里放的是应用通用选项。",
            ),
        },
        ActionDef {
            id: ActionId::SendToBackground,
            label: "后台运行",
            description: "将运行中的任务放到后台",
            default_key: key!('b', CONTROL),
            alt_keys: vec![],
            category: Category::Panels,
            context: When::AgentScreen,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(send_to_background_help),
        },
        // ── Prompt ───────────────────────────────────────────────────
        ActionDef {
            id: ActionId::InterjectPrompt,
            // "send now" label: Enter queues a follow-up while a turn runs;
            // this chord is cancel-and-send — stop the current turn and run
            // the message as the next one ("send now").
            label: "立即发送",
            description: "运行中立即发送（取消当前轮次）",
            default_key: if in_apple_terminal {
                key!('o', CONTROL)
            } else if in_vscode_family {
                // Ctrl+L is a stable C0 form feed on xterm.js; the user guide's interject section explains the choice
                key!('l', CONTROL)
            } else {
                key!(Enter, CONTROL)
            },
            // Windows: Ctrl+Enter may drop Ctrl, so Ctrl+I is an alt
            // VS Code family: no alts (Ctrl+L sole chord; OpenExtensions unbound so it does not steal)
            alt_keys: if in_apple_terminal {
                vec![key!(Enter, CONTROL), key!('i', CONTROL)]
            } else if in_vscode_family {
                vec![]
            } else {
                vec![key!('i', CONTROL)]
            },
            category: Category::Input,
            context: When::PromptFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "在轮次运行中给代理发消息而不取消它（插话），这样能在它继续工作时纠偏或补充上下文。\n轮次运行时单独按 Enter 会把后续内容排队；这个组合键则把输入框文本并入当前轮次。\n输入框为空时，单独按 Enter（或这个组合键）会强制发送提示队列中最上面的一条后续提示：无需先聚焦队列面板。在队列面板上，这个组合键强制发送选中行。\n想在不丢掉本轮进展的情况下纠偏，就用它。",
            ),
        },
        ActionDef {
            id: ActionId::EnableVoiceMode,
            label: "语音模式",
            description: "开始语音听写（Ctrl+Space / F8）",
            // No key binding (`KeyCode::Null`): dispatched directly by the voice
            // chord's hold-to-talk press in the event loop, not via the registry.
            default_key: key!(Null),
            alt_keys: vec![],
            category: Category::Input,
            context: When::AgentScreen,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            // Voice capture chord (the same capture as `/voice`; Esc/Enter stop)
            // Bound to both Ctrl+Space and F8
            // Ctrl+Space decodes on every terminal (without the Kitty protocol it collapses to NUL, reported as `Char(' ')`+CONTROL)
            // F8 is a fallback for OSes/terminals that intercept Ctrl+Space (e.g. macOS input-source switching; use Fn+F8 on a laptop).
            // The event loop maps a press to hold-to-talk or tap-toggle per `[ui].voice_capture_mode` before normal routing
            id: ActionId::VoiceToggle,
            label: "麦克风",
            description: "语音听写（Ctrl+Space / F8）",
            default_key: key!(' ', CONTROL),
            alt_keys: vec![key!(F(8))],
            category: Category::Input,
            // `Always` so the toggle key works on the agent screen and the session-less dashboard (resolved via the global fallthrough)
            context: When::Always,
            hint_priority: Some(11),
            hint_key_display: Some("Ctrl+Space / F8"),
            requires_confirmation: false,
            long_help: Some(
                "用于听写的麦克风采集，绑定 Ctrl+Space（或 F8：在 Ctrl+Space 被占用时更方便，例如 macOS 的输入法切换；笔记本上请用 Fn+F8）。\n行为取决于「语音采集」设置：切换（按一下开始、再按一下停止）或按住说话（按住录音、松开停止），后者需要 Kitty 协议的终端，否则回退为切换。`/voice` 在所有地方都可切换。\n语音会直接转写进提示框。",
            ),
        },
        // Prompt history has no key chord (Ctrl+R is deliberately unbound):
        // `/history` opens the search panel; Up on an empty prompt browses.
        ActionDef {
            id: ActionId::ToggleMultiline,
            label: "多行",
            description: "切换多行输入",
            default_key: key!('m', CONTROL),
            alt_keys: vec![],
            category: Category::Input,
            context: When::PromptFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "切换常驻多行提示，使编辑器保持展开以便撰写更长的消息。\n用 Shift+Enter 或 Alt+Enter（或行尾反斜杠）插入换行；单独按 Enter 仍然发送。\nCtrl+M 在提示框内切换多行；在提示框外则打开模型选择器。",
            ),
        },
        ActionDef {
            id: ActionId::StashPrompt,
            label: "暂存",
            description: "暂存 / 恢复提示草稿",
            default_key: key!('s', CONTROL),
            // The escape hatch for terminals that swallow Ctrl+S as XOFF.
            alt_keys: vec![key!('s', ALT)],
            category: Category::Input,
            context: When::PromptFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "把当前提示暂存为草稿。\nCtrl+S 收起草稿并清空输入框。输入框为空时按 Ctrl+S 会恢复草稿。发送下一条提示后草稿也会自行恢复。若终端吞掉 Ctrl+S，请用 Alt+S。\n一次只保留一份草稿：新的暂存会覆盖旧的。",
            ),
        },
        ActionDef {
            id: ActionId::BashMode,
            label: "Shell 模式",
            description: "Shell 模式（空提示输入 !）",
            default_key: key!('!'),
            alt_keys: vec![],
            category: Category::Input,
            context: When::PromptFocused,
            hint_priority: None,
            hint_key_display: Some("!"),
            requires_confirmation: false,
            long_help: Some(
                "不用离开对话就能执行 shell 命令：在空提示开头输入 !，然后输入命令。\n命令输出会被收录到历史记录。\n删掉开头的 ! 即可回到普通提示。",
            ),
        },
        // ── Agent ────────────────────────────────────────────────────
        ActionDef {
            id: ActionId::ToggleYolo,
            label: "总是批准",
            description: "切换总是批准",
            default_key: key!('o', CONTROL),
            alt_keys: vec![],
            category: Category::Session,
            context: When::AgentScreen,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "为本次会话打开或关闭自动批准（YOLO）。\n开启后代理会执行每次工具调用（编辑、shell、删除），不再逐项确认。\n与 Shift+Tab 循环里的「始终批准」是同一状态；请谨慎使用。",
            ),
        },
        ActionDef {
            id: ActionId::NewSession,
            label: "新建",
            description: "新建会话",
            default_key: key!('n', CONTROL),
            alt_keys: vec![],
            category: Category::Session,
            context: When::Always,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: true,
            long_help: Some(
                "以空的历史和上下文开始新会话。\n需要确认：连按两次（第一次进入待确认，第二次才真正开始），\n以免误丢弃当前对话。",
            ),
        },
        ActionDef {
            id: ActionId::Quit,
            label: "退出",
            description: "退出",
            default_key: if in_vscode {
                key!('d', CONTROL)
            } else {
                key!('q', CONTROL)
            },
            alt_keys: if in_vscode {
                vec![]
            } else {
                vec![key!('d', CONTROL)]
            },
            category: Category::GettingStarted,
            context: When::Always,
            hint_priority: Some(10),
            hint_key_display: None,
            requires_confirmation: true,
            long_help: Some(
                "退出程序。需要确认：需快速连按两次；\n只按一次会被视作误触而忽略。\n绑定 Ctrl+Q，Ctrl+D 为别名（在 VS Code 的终端里 Ctrl+D 是主键）。",
            ),
        },
        ActionDef {
            id: ActionId::CommandPalette,
            label: "命令",
            description: "命令面板",
            default_key: key!('p', CONTROL),
            alt_keys: vec![key!('?')],
            category: Category::GettingStarted,
            context: When::AgentScreen,
            hint_priority: Some(5),
            hint_key_display: Some("?"),
            requires_confirmation: false,
            long_help: Some(
                "模糊搜索所有操作与斜杠命令，然后按名称执行。\n记不住键位时很好用。\n历史记录聚焦时也可用 ? 打开。",
            ),
        },
        ActionDef {
            id: ActionId::ShortcutsHelp,
            label: "快捷键",
            description: "键盘快捷键",
            default_key: if ctrl_dot_unreliable {
                key!('x', CONTROL)
            } else {
                key!('.', CONTROL)
            },
            alt_keys: vec![if ctrl_dot_unreliable {
                key!('.', CONTROL)
            } else {
                key!('x', CONTROL)
            }],
            category: Category::GettingStarted,
            context: When::AgentScreen,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "打开这份键盘速查表。\n用 j/k 浏览，按 e 展开某一行的内联说明，或按 Enter 查看某个快捷键的完整详情页。\n同时绑定 Ctrl+. 与 Ctrl+X；状态栏会提示你的终端能可靠传出的那个。",
            ),
        },
        ActionDef {
            id: ActionId::ModelPicker,
            label: "模型",
            description: "选择模型",
            default_key: key!('m', CONTROL),
            alt_keys: vec![],
            category: Category::Session,
            context: When::AgentScreen,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "打开模型选择器，切换本次会话的模型；选择对之后的轮次生效。\n绑定 Ctrl+M，但在提示框聚焦时该组合键改为切换多行。\n可从历史记录或命令面板打开。",
            ),
        },
        ActionDef {
            id: ActionId::OpenSettings,
            label: "设置",
            description: "打开设置弹窗",
            default_key: key!(F(2)),
            alt_keys: vec![key!(',', CONTROL), key!(',', SUPER)],
            category: Category::GettingStarted,
            context: When::AgentScreen,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
    ];

    // Toggle terminal mouse reporting (mouse capture). Opt-in via `[ui] mouse_reporting_toggle = true` in config.toml.
    // Disabling capture hands mouse selection back to the terminal for native click-drag copy/paste; re-enabling restores in-app mouse support
    //
    // Single binding: Ctrl+R on scrollback only (not prompt, where Ctrl+R remains prompt history search)
    // Plain Ctrl+letter passes through Apple Terminal; this avoids Ctrl+Shift+… chords that Terminal.app often swallows
    // Under Panels (not Essentials): advanced/opt-in only
    if mouse_reporting_toggle_enabled {
        actions.push(ActionDef {
            id: ActionId::ToggleMouseCapture,
            label: "鼠标上报",
            description: "切换鼠标上报（原生复制/粘贴）",
            default_key: key!('r', CONTROL),
            alt_keys: vec![],
            category: Category::Panels,
            context: When::ScrollbackFocused,
            hint_priority: None,
            hint_key_display: Some("Ctrl+r"),
            requires_confirmation: false,
            long_help: None,
        });
    }

    // Agent Dashboard ----------------------------------------------------
    //
    // The `Ctrl+\` entry point and every in-dashboard shortcut are registered here
    // They all share the dedicated `Category::Dashboard` section so the cheatsheet groups them under a single "Dashboard" header
    // That keeps them out of Panels / Session / Navigation
    //
    // `Ctrl+\` (OpenDashboard) is registered against `Always` (global) so it works from any view, including the dashboard itself (which Esc closes)
    // Configurable through the standard config.toml mechanism
    actions.extend([
        ActionDef {
            id: ActionId::OpenDashboard,
            label: "仪表盘",
            description: "打开 Agent 仪表盘",
            default_key: key!('\\', CONTROL),
            // Classic C0 FS (0x1c): without KKP, Ctrl+\ arrives as Char('4')+CONTROL (e.g. Apple Terminal).
            // Omit when ToggleQueue already owns Ctrl+4
            alt_keys: if local_mac_vscode {
                vec![]
            } else {
                vec![key!('4', CONTROL)]
            },
            category: Category::Dashboard,
            context: When::Always,
            hint_priority: None,
            hint_key_display: Some("Ctrl+\\"),
            requires_confirmation: false,
            long_help: Some(
                "打开代理看板：列出所有在跑和最近的代理，便于监控与切换。\n在任何位置都可用，包括欢迎界面和会话内部。\n在那里你可以派发、接入、停止、分组和调整代理顺序。",
            ),
        },
        // Register all in-dashboard shortcuts through
        // the registry under `When::DashboardFocused`. The dispatch
        // path in `dashboard::state::handle_key` looks these up via
        // `registry.lookup(key, When::DashboardFocused)` so users can
        // rebind any of them through `~/.chaos/config.toml` (or `~/.grok/`).
        ActionDef {
            id: ActionId::DashboardSelectNext,
            label: "下一个",
            description: "选择下一行",
            default_key: key!(Down),
            alt_keys: vec![key!('j')],
            category: Category::Dashboard,
            context: When::DashboardFocused,
            hint_priority: None,
            hint_key_display: Some("\u{2191}\u{2193}"),
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::DashboardSelectPrev,
            label: "上一个",
            description: "选择上一行",
            default_key: key!(Up),
            alt_keys: vec![key!('k')],
            category: Category::Dashboard,
            context: When::DashboardFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::DashboardTogglePin,
            label: "固定",
            description: "固定 / 取消固定 Agent",
            default_key: key!('t', CONTROL),
            alt_keys: vec![],
            category: Category::Dashboard,
            context: When::DashboardFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "置顶或取消置顶选中的代理，使其无论排序或分组如何都留在列表顶部。\n在别的代理来来去去时，让你关心的代理始终可见。\n置顶在看板会话之间保持。",
            ),
        },
        ActionDef {
            id: ActionId::DashboardBeginRename,
            label: "重命名",
            description: "重命名 Agent",
            default_key: key!('r', CONTROL),
            alt_keys: vec![],
            category: Category::Dashboard,
            context: When::DashboardFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::DashboardStop,
            label: "停止",
            description: "停止 / 关闭 Agent",
            default_key: key!('x', CONTROL),
            alt_keys: vec![],
            category: Category::Dashboard,
            context: When::DashboardFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "停止选中的代理并从看板移除其行；正在运行的轮次会先被打断。\n可用来清理已完成或不需要的代理，无需接入它们。\n弹层内的对应操作（Ctrl+X）在停止前会确认。",
            ),
        },
        ActionDef {
            id: ActionId::DashboardCycleMode,
            label: "模式",
            description: "循环调度模式",
            // All Shift+Tab encodings — see `input::key::shift_tab_keys()`.
            // Registry `matches` is exact-modifier, so the SHIFT-bearing
            // forms must be alts.
            default_key: crate::input::key::shift_tab_keys()[0],
            alt_keys: crate::input::key::shift_tab_keys()[1..].to_vec(),
            category: Category::Dashboard,
            context: When::DashboardFocused,
            hint_priority: None,
            hint_key_display: Some("Shift+Tab"),
            requires_confirmation: false,
            long_help: Some(
                "循环切换从看板派发的代理所使用的模式：普通、计划、始终批准。\n计划模式下新代理会先规划再改文件；始终批准则不再询问就执行其工具。\n与会话内的 Shift+Tab 循环一致，只是作用于新派发的代理。",
            ),
        },
        ActionDef {
            id: ActionId::DashboardToggleGrouping,
            label: "分组",
            description: "切换行分组",
            // `Ctrl+G` ("group"). `Ctrl+S` was reassigned to the peek /
            // dispatch "send + open" chord so `Shift+Enter` could be
            // freed for newline insertion. (`Ctrl+G` also has a
            // mode-specific `When::AgentScreen` action, a context that never
            // overlaps the dashboard.)
            default_key: key!('g', CONTROL),
            alt_keys: vec![],
            category: Category::Dashboard,
            context: When::DashboardFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: Some(
                "在看板的平铺列表与按状态（如工作中与空闲）分组的行之间切换。\n分组能凸显需要关注的代理；平铺列表则保持稳定顺序。\n你的选择在会话之间保持。",
            ),
        },
        ActionDef {
            id: ActionId::DashboardReorderUp,
            label: "上移",
            description: "将 Agent 上移",
            default_key: key!(Up, SHIFT),
            alt_keys: vec![],
            category: Category::Dashboard,
            context: When::DashboardFocused,
            hint_priority: None,
            hint_key_display: Some("Shift+\u{2191}"),
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::DashboardReorderDown,
            label: "下移",
            description: "将 Agent 下移",
            default_key: key!(Down, SHIFT),
            alt_keys: vec![],
            category: Category::Dashboard,
            context: When::DashboardFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::DashboardShortcutsHelp,
            label: "快捷键",
            description: "显示快捷键浮层",
            // Ctrl+. / `?` dual-bound; primary follows ctrl_dot_unreliable.
            // Ctrl+X is DashboardStop, never an alt here
            default_key: if ctrl_dot_unreliable {
                key!('?')
            } else {
                key!('.', CONTROL)
            },
            alt_keys: vec![if ctrl_dot_unreliable {
                key!('.', CONTROL)
            } else {
                key!('?')
            }],
            category: Category::Dashboard,
            context: When::DashboardFocused,
            hint_priority: None,
            hint_key_display: None,
            requires_confirmation: false,
            long_help: None,
        },
        // `DashboardExit` is registered as a discoverable action with its default key set to Esc
        // The in-dashboard Esc behaviour is a multi-tier cascade (peek, then input/filter, then exit) that no single action can express
        // The Esc cascade in `state::handle_key` runs before this registry lookup, so Esc always cascades
        // A user who rebinds Esc to something else gains a discoverable exit shortcut for the rebound key
        // The original Esc cascade still works because the cascade is keyed on `KeyCode::Esc` directly
        // The contract is therefore: "Esc always cascades; any other key bound to `DashboardExit` exits directly."
        // The hint key shows the effective binding via `Esc` as a fallback
        ActionDef {
            id: ActionId::DashboardExit,
            label: "退出",
            description: "关闭仪表盘",
            default_key: key!(Esc),
            alt_keys: vec![],
            category: Category::Dashboard,
            context: When::DashboardFocused,
            hint_priority: None,
            hint_key_display: Some("Esc"),
            requires_confirmation: false,
            long_help: Some(
                "关闭看板并回到原处。\nEsc 是级联的：先关掉打开的预览或清除生效中的筛选，只有其余都处理完了才退出。\n把此操作改绑到别的键即可直接退出。",
            ),
        },
        // Mirror of `ToggleYolo` (Ctrl+O) but scoped to the dashboard: flips the selected row's agent's always-approve / YOLO mode
        // Reachable from the dashboard view (and from inside the session overlay)
        ActionDef {
            id: ActionId::DashboardToggleAutoApprove,
            label: "总是批准",
            description: "切换总是批准",
            default_key: key!('o', CONTROL),
            alt_keys: vec![],
            category: Category::Dashboard,
            context: When::DashboardFocused,
            hint_priority: None,
            hint_key_display: Some("Ctrl+O"),
            requires_confirmation: false,
            long_help: Some(
                "直接在看板上为选中的代理切换自动批准（YOLO），无需接入它。\n开启后该代理会执行每次工具调用，不再逐项确认。\n会话内的对应操作是 Ctrl+O。",
            ),
        },
        // Open the location picker, a floating modal to change the working directory new dashboard sessions spawn in
        // Ctrl+L ("location") is free under `DashboardFocused` (it only binds OpenExtensions under `AgentScreen`, a different context)
        ActionDef {
            id: ActionId::DashboardOpenLocationPicker,
            label: "位置",
            description: "更改新 Agent 的工作目录",
            default_key: key!('l', CONTROL),
            alt_keys: vec![],
            category: Category::Dashboard,
            context: When::DashboardFocused,
            hint_priority: None,
            hint_key_display: Some("Ctrl+l"),
            requires_confirmation: false,
            long_help: Some(
                "打开选择器，设置新派发的看板代理所运行的工作目录。\n不用离开看板就能让代理在别的仓库或目录中启动。\n只影响新派发的代理，不影响已在运行的代理。",
            ),
        },
        // Toggle worktree-dispatch mode
        // Ctrl+W ("worktree") makes the next dashboard-dispatched session spawn in a fresh git worktree
        // The dispatcher gates it on the cwd being a git repo
        // Free under `DashboardFocused` (Ctrl+W only binds the overlay-exit fallback under `DashboardOverlay`, a different context)
        ActionDef {
            id: ActionId::DashboardToggleWorktree,
            label: "工作树",
            description: "切换新 Agent 的工作树模式",
            default_key: key!('w', CONTROL),
            alt_keys: vec![],
            category: Category::Dashboard,
            context: When::DashboardFocused,
            hint_priority: None,
            hint_key_display: Some("Ctrl+w"),
            requires_confirmation: false,
            long_help: Some(
                "让下一个由看板派发的代理在新的 git worktree 中启动，把它的工作隔离在独立检出里。\n仅当工作目录是 git 仓库时适用。\n影响新派发的代理，不影响已在运行的代理。",
            ),
        },
        // Session overlay (attaching to an agent from the dashboard) bindings
        // They use `When::DashboardOverlay`: the agent-side overlay intercept (`app_view`) looks them up in that context
        // The cheatsheet uses it to dim them on the dashboard list (where they don't apply) while keeping them lit inside the overlay
        ActionDef {
            id: ActionId::DashboardOverlayExit,
            label: "关闭浮层",
            description: "返回仪表盘",
            // The primary back-out shortcuts are reached through
            // different routes:
            //   - Ctrl+\\ → OpenDashboard (registered separately above);
            //     the overlay-input intercept treats it as overlay-exit.
            //   - `q` when scrollback is focused — handled by the
            //     overlay intercept directly.
            //   - Esc when the agent is in a "neutral" state
            //     Neutral means no modals or viewers, no text selection, no link highlight, and no question/goal/rewind/permission overlays
            //     Per-pane Esc consumers still take precedence; see the `overlay_esc_*` tests in `app_view`
            //   - A `[✗]` click, routed via this action by the mouse handler
            // The `default_key` mirrors the primary route, Ctrl+\ (OpenDashboard, treated as overlay-exit), so the cheatsheet hint is accurate
            // (Ctrl+W is not used here; it's the dashboard's worktree toggle.)
            default_key: key!('\\', CONTROL),
            alt_keys: vec![],
            category: Category::Dashboard,
            context: When::DashboardOverlay,
            hint_priority: None,
            hint_key_display: Some("Ctrl+\\"),
            requires_confirmation: false,
            long_help: Some(
                "离开接入的会话弹层，回到看板列表，但不停止代理。\n在历史记录上按 q、中性 Esc 或关闭按钮也能达到同样效果。\n想停止代理而不只是脱离，请用 Ctrl+X。",
            ),
        },
        ActionDef {
            id: ActionId::DashboardOverlayPrev,
            label: "上一会话",
            description: "上一会话",
            default_key: key!('[', CONTROL),
            alt_keys: vec![],
            category: Category::Dashboard,
            context: When::DashboardOverlay,
            hint_priority: None,
            hint_key_display: Some("Ctrl+["),
            requires_confirmation: false,
            long_help: None,
        },
        ActionDef {
            id: ActionId::DashboardOverlayNext,
            label: "下一会话",
            description: "下一会话",
            default_key: key!(']', CONTROL),
            alt_keys: vec![],
            category: Category::Dashboard,
            context: When::DashboardOverlay,
            hint_priority: None,
            hint_key_display: Some("Ctrl+]"),
            requires_confirmation: false,
            long_help: None,
        },
        // Stop with dashboard parity inside the session overlay; the state machine is documented at `dispatch_dashboard_overlay_stop`
        // Intentionally shadows the agent view's `ShortcutsHelp` alt binding (Ctrl+X) inside the overlay; Ctrl+. still opens the cheatsheet there.
        ActionDef {
            id: ActionId::DashboardOverlayStop,
            label: "停止",
            description: "停止 Agent、关闭会话（返回仪表盘）",
            default_key: key!('x', CONTROL),
            alt_keys: vec![],
            category: Category::Dashboard,
            context: When::DashboardOverlay,
            hint_priority: None,
            hint_key_display: Some("Ctrl+x"),
            requires_confirmation: true,
            long_help: Some(
                "在会话弹层内停止所接入的代理并关闭它，回到看板列表。\n需要确认：连按两次 Ctrl+X。\nCtrl+. 在这里仍会打开速查表；只有 Ctrl+X 被停止操作占用。",
            ),
        },
    ]);

    // Minimal has no interactive scrollback and no dashboard
    // Keep its logical prompt, agent-screen, and legitimate global actions
    // Do not register bindings whose target UI cannot exist in this process mode
    if screen_mode.is_minimal() {
        actions.retain(|def| {
            !matches!(
                def.context,
                When::ScrollbackFocused | When::DashboardFocused | When::DashboardOverlay
            ) && !matches!(def.id, ActionId::OpenDashboard | ActionId::FocusScrollback)
        });
    }

    actions
}
