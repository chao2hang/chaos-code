//! Default settings catalog: every user-tunable preference registered in the settings modal.
//!
//! Defaults come from `UiConfig::default()` for SHELL/SHARED settings.
//! The `defaults_match_ui_config_default` test enforces this.

use super::registry::{
    DynamicEnumSource, EnumChoice, SettingCategory, SettingKind, SettingMeta, SettingOwner,
};
use crate::appearance::ScrollMode;
use crate::appearance::TextSelection;
use crate::appearance::permission_cursor::DefaultSelectedPermission;

use xai_grok_shell::agent::config::UiConfig;
use xai_grok_shell::util::config::DISPLAY_REFRESH_DEFAULT_AUTO_CADENCE_ENABLED;
use xai_grok_tools::implementations::grok_build::ask_user_question;

// ---------------------------------------------------------------------------
// Int bounds for `max_thoughts_width`.
//
// Stored as `u16` in `UiConfig`, exposed as `i64` for registry uniformity.
// 40 is the minimum readable width on an 80-col terminal; 500 is the cap before "obviously wrong" territory
// `pub(crate)` so the dispatcher's clamp and the shell helper's defensive clamp share these bounds
pub(crate) const MAX_THOUGHTS_WIDTH_MIN: i64 = 40;
pub(crate) const MAX_THOUGHTS_WIDTH_MAX: i64 = 500;

/// Registry key for `max_thoughts_width`; it is shared between the registry definition and the live-wrap-preview gate in the int stepper.
pub(crate) const MAX_THOUGHTS_WIDTH_KEY: &str = "max_thoughts_width";

// ---------------------------------------------------------------------------
// Theme choice catalogs.
//
// Canonical names MUST match `ThemeKind::display_name()`.
// The catalogs are shared by `theme`, `auto_dark_theme`, and `auto_light_theme`; the auto-* sub-pickers drop "auto" to avoid a circular reference
// The lists are bounded by `MAX_PICKER_CHOICES`
// ---------------------------------------------------------------------------

/// Full theme catalog including the "auto" meta-variant; only `theme` uses it.
const THEME_CHOICES: &[EnumChoice] = &[
    EnumChoice {
        canonical: "auto",
        display: "自动",
        description: "跟随系统深色/浅色外观。",
    },
    EnumChoice {
        canonical: "groknight",
        display: "Chaos Night",
        description: "中性深色，品红强调色。",
    },
    EnumChoice {
        canonical: "grokday",
        display: "Chaos Day",
        description: "适合明亮环境的浅色主题。",
    },
    EnumChoice {
        canonical: "tokyonight",
        display: "Tokyo Night",
        description: "深蓝调深色；需要真彩色。",
    },
    // The display name is ASCII "Rose Pine Moon" (not "Rosé") for cross-terminal compatibility
    EnumChoice {
        canonical: "rosepine-moon",
        display: "Rose Pine Moon",
        description: "柔和深色带淡紫强调；需要真彩色。",
    },
    EnumChoice {
        canonical: "oscura-midnight",
        display: "Oscura Midnight",
        description: "深黑带暖色强调；需要真彩色。",
    },
];

// ---------------------------------------------------------------------------
// Permission-mode catalog.
//
// Persisted values map onto runtime flags:
//   "always-approve" ↔ yolo_mode = true  (auto-approve all)
//   "auto"           ↔ auto_mode = true  (LLM classifier; not full yolo)
//   "ask"            ↔ both false (interactive prompts)
//   "default"        ↔ both false (agent's default, currently Ask)
//
// Canonical strings match `load_permission_mode`
// `supports_preview: false` because toggling YOLO drains the permission queue (unsafe for per-keystroke preview)
//
// Adding new modes requires: (1) a `PermissionModeKind` variant, (2) an `EnumChoice` here,
// (3) a `set_yolo_mode_inner` update, (4) a `load_permission_mode` arm, (5) tests
// `Plan` is excluded; it lives on its own `plan_mode` setting
// ---------------------------------------------------------------------------

// Choice order runs safe to unsafe: Default, Ask, Auto, Always approve
// "Always approve" at the end creates a speed bump against accidental selection
const PERMISSION_MODE_CHOICES: &[EnumChoice] = &[
    // "default" is the agent's default behavior: the same as "ask" at runtime, but distinct on disk and in the modal indicator
    EnumChoice {
        canonical: "default",
        display: "默认",
        description: "使用 Agent 默认权限行为（目前等同于询问）。",
    },
    EnumChoice {
        canonical: "ask",
        display: "询问",
        description: "工具操作前请求权限确认。",
    },
    EnumChoice {
        canonical: "auto",
        display: "自动",
        description: "LLM 分类器批准安全工具；危险操作仍可能询问或拒绝。",
    },
    EnumChoice {
        canonical: "always-approve",
        display: "总是批准",
        description: "自动批准所有工具操作。跳过全部权限确认。",
    },
];

// ---------------------------------------------------------------------------
// Coding-data-sharing catalog.
//
// Persisted in auth metadata (`AuthEntry::coding_data_retention_opt_out`), NOT config.toml
// Two choices only: the pager has no `Option`/`Unset` representation for this field
//
// `supports_preview: false` because toggling fires an async ACP call that can fail. Commit on Enter only.
// ---------------------------------------------------------------------------

const CODING_DATA_SHARING_CHOICES: &[EnumChoice] = &[
    EnumChoice {
        canonical: "opt-in",
        display: "选择加入",
        description: "允许保留编码会话数据用于模型训练与产品改进。",
    },
    EnumChoice {
        canonical: "opt-out",
        display: "选择退出",
        description: "不保留编码会话数据用于训练。不会关闭产品分析。",
    },
];

// ---------------------------------------------------------------------------
// Plan-mode catalog.
//
// PAGER-owned and per-session, set over ACP via `session/set_mode`
// NOT persisted to config.toml; it resets every session start
//
// Uses `on`/`off` canonical strings (not the shell's `plan`/`default` wire ids)
// `Ask` mode is not exposed here; it is only reachable via Shift+Tab
//
// `supports_preview: false` because toggling fires an ACP request that gates tool dispatch. Commit on Enter only.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Default-selected-permission catalog.
//
// Persisted to `[ui].default_selected_permission` in config.toml
// It controls which row the cursor preselects on the FIRST permission prompt of a session
// After the user confirms any prompt, the cursor sticks to the last-used option kind
// `always_allow_all_sessions` (the effective default) lands the cursor on the "Always allow on all sessions" (enable-always-approve) row
// That targeting goes through `is_enable_always_approve_option`, not index 0
// The other three map onto `acp::PermissionOptionKind::{AllowOnce, AllowAlways, Reject*}`
//
// `supports_preview: false` because permission prompts aren't open in the modal background, so there is nothing to live-preview
// ---------------------------------------------------------------------------

// Order matches the live permission prompt rendering (YOLO, always-allow, allow-once, reject) so the picker mirrors the real prompt
// Canonicals and display labels come from `DefaultSelectedPermission`, the single source of truth
// This table therefore can never drift from the parser, the dispatch toast, or the cursor logic
const DEFAULT_SELECTED_PERMISSION_CHOICES: &[EnumChoice] = &[
    EnumChoice {
        canonical: DefaultSelectedPermission::AlwaysAllowAllSessions.as_canonical(),
        display: DefaultSelectedPermission::AlwaysAllowAllSessions.display(),
        description: "",
    },
    EnumChoice {
        canonical: DefaultSelectedPermission::AllowCommandAlways.as_canonical(),
        display: DefaultSelectedPermission::AllowCommandAlways.display(),
        description: "",
    },
    EnumChoice {
        canonical: DefaultSelectedPermission::AllowOnce.as_canonical(),
        display: DefaultSelectedPermission::AllowOnce.display(),
        description: "",
    },
    EnumChoice {
        canonical: DefaultSelectedPermission::Reject.as_canonical(),
        display: DefaultSelectedPermission::Reject.display(),
        description: "",
    },
];

const PLAN_MODE_CHOICES: &[EnumChoice] = &[
    EnumChoice {
        canonical: "off",
        display: "关",
        description: "Agent 直接运行工具并编辑文件（默认）。",
    },
    EnumChoice {
        canonical: "on",
        display: "开",
        description: "Agent 先总结计划，经批准后再运行工具。",
    },
];

// Mid-turn follow-up routing. SHARED-owned, persisted to `[ui].follow_up_behavior`.
// Canonicals match `FollowUpBehavior::as_canonical`
const FOLLOW_UP_BEHAVIOR_CHOICES: &[EnumChoice] = &[
    EnumChoice {
        canonical: "queue",
        display: "Queue",
        description: "Hold follow-ups until the current turn finishes.",
    },
    EnumChoice {
        canonical: "steer",
        display: "Steer",
        description: "Inject follow-ups mid-turn at the next tool or model step.",
    },
];

// ---------------------------------------------------------------------------
// Mermaid-rendering catalog.
//
// SHELL-owned: persisted to `[ui].render_mermaid`
// A pager-side process-wide cache mirror (`appearance::cache::*_render_mermaid`) serves the render hot path
// Canonicals match `RenderMermaid::as_canonical`
// ---------------------------------------------------------------------------

const RENDER_MERMAID_CHOICES: &[EnumChoice] = &[
    EnumChoice {
        canonical: "auto",
        display: "自动",
        description: "显示图表，并提供可点击行以打开/复制渲染图。",
    },
    EnumChoice {
        canonical: "on",
        display: "开",
        description: "与自动相同：始终显示可点击操作行。",
    },
    EnumChoice {
        canonical: "off",
        display: "关",
        description: "始终以代码块显示原始 Mermaid 源码。",
    },
];

// Scroll-input catalog. SHELL-owned, persisted to `[ui].scroll_mode`.
// Canonical strings match `ScrollMode::as_canonical` (pinned by test).
const SCROLL_MODE_CHOICES: &[EnumChoice] = &[
    EnumChoice {
        canonical: ScrollMode::Auto.as_canonical(),
        display: "自动检测",
        description: "按事件时序检测滚轮与触控板。默认。",
    },
    EnumChoice {
        canonical: ScrollMode::Wheel.as_canonical(),
        display: "鼠标滚轮",
        description: "始终按滚轮刻度滚动（每次固定行数）。",
    },
    EnumChoice {
        canonical: ScrollMode::Trackpad.as_canonical(),
        display: "触控板",
        description: "始终按触控板滚动（分数累积）。",
    },
];

const TEXT_SELECTION_CHOICES: &[EnumChoice] = &[
    EnumChoice {
        canonical: TextSelection::Flash.as_canonical(),
        display: "复制后闪烁",
        description: "鼠标抬起时短暂高亮后清除。双击切换折叠。默认。",
    },
    EnumChoice {
        canonical: TextSelection::Hold.as_canonical(),
        display: "保持到关闭",
        description: "选区保持可见直到 Esc、点击或滚动。双击切换折叠。",
    },
    EnumChoice {
        canonical: TextSelection::WordSelect.as_canonical(),
        display: "选词（类终端）",
        description: "双击选中并复制单词，三击选中整行；选区保持到关闭。",
    },
];

// Hunk-tracker-mode catalog. SHELL-owned, persisted to `[ui].hunk_tracker_mode`.
// `disabled` is accepted as an alias for `off` at parse time but not shown as a choice
const HUNK_TRACKER_MODE_CHOICES: &[EnumChoice] = &[
    EnumChoice {
        canonical: "agent_only",
        display: "仅 Agent",
        description: "仅跟踪 Agent 编辑的文件（默认）。",
    },
    EnumChoice {
        canonical: "all_dirty",
        display: "全部脏文件",
        description: "跟踪所有 git 脏文件，包括外部编辑。",
    },
    EnumChoice {
        canonical: "off",
        display: "关",
        description: "完全禁用块跟踪。同时禁用代码行统计。",
    },
];

const SCREEN_MODE_CHOICES: &[EnumChoice] = &[
    EnumChoice {
        canonical: "fullscreen",
        display: "全屏",
        description: "以标准全屏 TUI 打开。未设置时的默认值。",
    },
    EnumChoice {
        canonical: "minimal",
        display: "极简",
        description: "以原生滚动（极简）模式打开。",
    },
];

// Voice-capture-mode catalog. SHELL-owned, persisted to `[ui].voice_capture_mode`.
// `hold` is only offered on terminals that report key releases (Kitty keyboard
// protocol); `effective_enum_choices` hides it elsewhere, and it falls back to
// `toggle` at runtime.
const VOICE_CAPTURE_MODE_CHOICES: &[EnumChoice] = &[
    EnumChoice {
        canonical: "toggle",
        display: "切换",
        description: "Ctrl+Space / F8 开始听写；再按（或 Esc/Enter）停止。",
    },
    EnumChoice {
        canonical: "hold",
        display: "按住说话",
        description: "按住 Ctrl+Space / F8 录音，松开停止。需要 Kitty 协议终端。",
    },
];

// Voice STT language choices for the settings modal.
//
// Concrete codes must match `xai_grok_voice::STT_LANGUAGES` (upstream STT
// catalog — https://docs.x.ai/developers/model-capabilities/audio/speech-to-text).
// `auto` is client-only; the voice crate resolves it to a concrete code before
// the STT handshake. Order: English (default), System, then remaining languages
// A–Z by English name. A registry unit test locks this list to the voice crate.
const VOICE_STT_LANGUAGE_CHOICES: &[EnumChoice] = &[
    EnumChoice {
        canonical: "en",
        display: "English",
        description: "",
    },
    EnumChoice {
        canonical: "auto",
        display: "系统",
        description: "系统语言为支持的 STT 语言时使用系统语言；否则使用英语。",
    },
    EnumChoice {
        canonical: "ar",
        display: "Arabic",
        description: "",
    },
    EnumChoice {
        canonical: "cs",
        display: "Czech",
        description: "",
    },
    EnumChoice {
        canonical: "da",
        display: "Danish",
        description: "",
    },
    EnumChoice {
        canonical: "nl",
        display: "Dutch",
        description: "",
    },
    EnumChoice {
        canonical: "fil",
        display: "Filipino",
        description: "",
    },
    EnumChoice {
        canonical: "fr",
        display: "French",
        description: "",
    },
    EnumChoice {
        canonical: "de",
        display: "German",
        description: "",
    },
    EnumChoice {
        canonical: "hi",
        display: "Hindi",
        description: "",
    },
    EnumChoice {
        canonical: "id",
        display: "Indonesian",
        description: "",
    },
    EnumChoice {
        canonical: "it",
        display: "Italian",
        description: "",
    },
    EnumChoice {
        canonical: "ja",
        display: "Japanese",
        description: "",
    },
    EnumChoice {
        canonical: "ko",
        display: "Korean",
        description: "",
    },
    EnumChoice {
        canonical: "mk",
        display: "Macedonian",
        description: "",
    },
    EnumChoice {
        canonical: "ms",
        display: "Malay",
        description: "",
    },
    EnumChoice {
        canonical: "fa",
        display: "Persian",
        description: "",
    },
    EnumChoice {
        canonical: "pl",
        display: "Polish",
        description: "",
    },
    EnumChoice {
        canonical: "pt",
        display: "Portuguese",
        description: "",
    },
    EnumChoice {
        canonical: "ro",
        display: "Romanian",
        description: "",
    },
    EnumChoice {
        canonical: "ru",
        display: "Russian",
        description: "",
    },
    EnumChoice {
        canonical: "es",
        display: "Spanish",
        description: "",
    },
    EnumChoice {
        canonical: "sv",
        display: "Swedish",
        description: "",
    },
    EnumChoice {
        canonical: "th",
        display: "Thai",
        description: "",
    },
    EnumChoice {
        canonical: "tr",
        display: "Turkish",
        description: "",
    },
    EnumChoice {
        canonical: "vi",
        display: "Vietnamese",
        description: "",
    },
];

/// Concrete-only theme catalog (excludes "auto"), used by both `auto_dark_theme` and `auto_light_theme`.
/// There is no dark/light filtering: the user can pair any theme with any system-appearance bucket.
const CONCRETE_THEME_CHOICES: &[EnumChoice] = &[
    EnumChoice {
        canonical: "groknight",
        display: "Chaos Night",
        description: "中性深色，品红强调色。",
    },
    EnumChoice {
        canonical: "grokday",
        display: "Chaos Day",
        description: "适合明亮环境的浅色主题。",
    },
    EnumChoice {
        canonical: "tokyonight",
        display: "Tokyo Night",
        description: "深蓝调深色；需要真彩色。",
    },
    EnumChoice {
        canonical: "rosepine-moon",
        display: "Rose Pine Moon",
        description: "柔和深色带淡紫强调；需要真彩色。",
    },
    EnumChoice {
        canonical: "oscura-midnight",
        display: "Oscura Midnight",
        description: "深黑带暖色强调；需要真彩色。",
    },
];

/// Child settings shown inside the "Show contextual hints" group sub-sheet.
/// Keys match the `[ui.contextual_hints]` serde fields.
/// The namespace keeps them globally unique: bare `plan_mode` collides with the plan-mode enum row.
/// They are registered as normal Bool settings but hidden from the top-level list (`build_rows` skips any key that is a group child).
const CONTEXTUAL_HINTS_CHILDREN: &[&str] = &[
    "contextual_hints.undo",
    "contextual_hints.plan_mode",
    "contextual_hints.image_input",
    "contextual_hints.send_now",
    "contextual_hints.small_screen",
    "contextual_hints.word_select",
    "contextual_hints.export_copy",
    "contextual_hints.ssh_wrap",
];

/// Build the catalog; called once at process start via `SettingsRegistry::defaults()`.
pub fn default_settings() -> Vec<SettingMeta> {
    // The shell schema defaults are the registry's source of truth
    let ui_default = UiConfig::default();

    vec![
        SettingMeta {
            key: "compact_mode",
            category: SettingCategory::Appearance,
            owner: SettingOwner::Shared,
            label: "紧凑模式",
            description: "减少消息周围边距以提高内容密度。\
                          终端高度不超过 20 行时自动启用。",
            keywords: &[
                "compact", "density", "padding", "tight", "small", "screen", "auto",
            ],
            kind: SettingKind::Bool {
                default: ui_default.compact_mode,
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        SettingMeta {
            key: "screen_mode",
            category: SettingCategory::Appearance,
            owner: SettingOwner::Shell,
            label: "默认屏幕模式",
            description: "下次启动时的界面模式：全屏（未设置时的默认）或极简。写入 config.toml 的 [ui] screen_mode。需重启。\
                          仅本会话切换可用 /minimal 或 /fullscreen。",
            keywords: &[
                "screen",
                "mode",
                "minimal",
                "fullscreen",
                "full",
                "scrollback",
                "native",
                "alt-screen",
                "render",
                "default",
            ],
            kind: SettingKind::Enum {
                default: "fullscreen",
                choices: SCREEN_MODE_CHOICES,
                supports_preview: false,
            },
            restart_required: true,
            hidden_in_minimal: false,
        },
        SettingMeta {
            key: "show_timestamps",
            category: SettingCategory::Appearance,
            owner: SettingOwner::Shared,
            label: "显示时间戳",
            description: "在用户消息与 Agent 回复旁显示时钟时间。",
            keywords: &["timestamps", "time", "clock", "date"],
            kind: SettingKind::Bool {
                // `Option<bool>`: `None` is treated as `true`
                default: ui_default.show_timestamps.unwrap_or(true),
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        SettingMeta {
            key: "show_timeline",
            category: SettingCategory::Appearance,
            owner: SettingOwner::Shared,
            label: "时间线侧栏",
            description: "用每轮刻度轨代替滚动条：悬停预览该轮，点击跳转。",
            keywords: &["timeline", "sidebar", "ticks", "turns", "navigator", "rail"],
            kind: SettingKind::Bool {
                // Single source: UiConfig::SHOW_TIMELINE_DEFAULT (opt-in).
                default: ui_default.show_timeline_enabled(),
            },
            restart_required: false,
            // Minimal mode has no interactive scrollback pane for the rail.
            hidden_in_minimal: true,
        },
        SettingMeta {
            key: "page_flip_on_send",
            category: SettingCategory::Appearance,
            owner: SettingOwner::Shared,
            label: "发送时将提示顶到顶部",
            description: "发送提示时将其滚到屏幕顶部，使回复从新一页开始（默认）。关闭则发送时保持滚动位置不变。",
            keywords: &[
                "page", "flip", "send", "prompt", "scroll", "top", "jump", "auto", "snap",
            ],
            kind: SettingKind::Bool {
                default: ui_default.page_flip_on_send_enabled(),
            },
            restart_required: false,
            hidden_in_minimal: true,
        },
        SettingMeta {
            key: "combine_queued_prompts",
            category: SettingCategory::Editor,
            owner: SettingOwner::Shared,
            label: "合并排队提示",
            description: "将连续的普通跟进合并为同一模型轮次（TUI 仍各显示一个气泡）。遇到 bash、斜杠命令、\
                          定时任务、展开的技能、带图跟进或正在编辑的行时停止。\
                          默认关闭；在本地排空与 shell promote 时生效。",
            keywords: &["queue", "combine", "batch", "follow-up", "merge", "pending"],
            kind: SettingKind::Bool {
                default: ui_default.combine_queued_prompts.unwrap_or(false),
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        SettingMeta {
            key: "follow_up_behavior",
            category: SettingCategory::Editor,
            owner: SettingOwner::Shared,
            label: "Follow-up behavior",
            description: "What to do with messages you send while a turn is \
                          running. Queue waits for the turn to finish; Steer \
                          injects them mid-turn at the next tool batch or \
                          model step. Default: Queue.",
            keywords: &[
                "queue",
                "steer",
                "interject",
                "follow-up",
                "followup",
                "send",
                "immediate",
            ],
            kind: SettingKind::Enum {
                default: ui_default.follow_up_behavior(),
                choices: FOLLOW_UP_BEHAVIOR_CHOICES,
                supports_preview: false,
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        SettingMeta {
            key: "confirm_before_rewind",
            category: SettingCategory::Editor,
            owner: SettingOwner::Shared,
            label: "Confirm before rewind",
            description: "Ask before rewinding conversation history. Turn off to rewind \
                          immediately when you pick a turn.",
            keywords: &["rewind", "confirm", "undo", "history", "ask", "prompt"],
            kind: SettingKind::Bool {
                default: ui_default.confirm_before_rewind_enabled(),
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        SettingMeta {
            // The persisted key stays `simple_mode`
            // The user-facing label distinguishes the PROMPT vim-mode (this setting) from the scrollback `vim_mode` keybindings below
            key: "simple_mode",
            category: SettingCategory::Appearance,
            owner: SettingOwner::Shared,
            label: "禁用 vim 输入模式",
            description: "在提示输入中使用普通 readline 风格，而非 vim 键位。实验性功能。",
            keywords: &[
                "simple",
                "ascii",
                "minimal",
                "plain",
                "vim",
                "readline",
                "experimental",
                "editor",
                "input",
                "prompt",
            ],
            kind: SettingKind::Bool {
                // `Option<bool>`: `None` is treated as `true`
                default: ui_default.simple_mode.unwrap_or(true),
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // SHELL-owned, persisted to `[ui].vim_mode` in config.toml.
        // Defaults to the same value main's `appearance::persist::VIM_MODE_DEFAULT` shipped with
        // Bundled next to `simple_mode` because they pair up: simple_mode controls the input editor's vim behaviour, vim_mode controls the scrollback's
        SettingMeta {
            key: "vim_mode",
            category: SettingCategory::Appearance,
            owner: SettingOwner::Shell,
            label: "Vim 滚动导航",
            description: "用 vim 键（h/j/k/l、gg/G、/）导航滚动历史。不影响输入提示。",
            keywords: &[
                "vim",
                "scrollback",
                "navigation",
                "hjkl",
                "keys",
                "keybindings",
                "scroll",
            ],
            kind: SettingKind::Bool {
                default: ui_default.vim_mode.unwrap_or(false),
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // --- theme and auto themes -------------------------------------------
        SettingMeta {
            key: "theme",
            category: SettingCategory::Appearance,
            owner: SettingOwner::Shared,
            label: "主题",
            description: "分页器界面的配色主题。",
            keywords: &[
                "theme",
                "color",
                "colour",
                "palette",
                "appearance",
                "dark",
                "light",
            ],
            kind: SettingKind::Enum {
                // `Option<String>`: `None` resolves to "groknight"
                default: "groknight",
                choices: THEME_CHOICES,
                supports_preview: true,
            },
            restart_required: false,
            hidden_in_minimal: true,
        },
        SettingMeta {
            key: "auto_dark_theme",
            category: SettingCategory::Appearance,
            owner: SettingOwner::Shared,
            label: "自动深色主题",
            description: "系统处于深色模式时使用的主题（仅当 theme=auto）。",
            keywords: &["auto", "dark", "theme", "system", "appearance", "night"],
            kind: SettingKind::Enum {
                // `Option<String>`: `None` falls back to "groknight"
                default: "groknight",
                choices: CONCRETE_THEME_CHOICES,
                supports_preview: true,
            },
            restart_required: false,
            hidden_in_minimal: true,
        },
        SettingMeta {
            key: "auto_light_theme",
            category: SettingCategory::Appearance,
            owner: SettingOwner::Shared,
            label: "自动浅色主题",
            description: "系统处于浅色模式时使用的主题（仅当 theme=auto）。",
            keywords: &["auto", "light", "theme", "system", "appearance", "day"],
            kind: SettingKind::Enum {
                // `Option<String>`: `None` falls back to "grokday"
                default: "grokday",
                choices: CONCRETE_THEME_CHOICES,
                supports_preview: true,
            },
            restart_required: false,
            hidden_in_minimal: true,
        },
        // SHELL-owned: persisted to `[ui].render_mermaid`, with a pager-side process-wide cache mirror (like `vim_mode`)
        // The default is pinned to "auto" by `defaults_match_ui_config_default`
        SettingMeta {
            key: "render_mermaid",
            category: SettingCategory::Appearance,
            owner: SettingOwner::Shell,
            label: "渲染 Mermaid 图表",
            description: "```mermaid 代码块的显示方式：自动/开 增加可点击行以打开渲染图；关 则显示原始源码。",
            keywords: &[
                "mermaid",
                "diagram",
                "diagrams",
                "render",
                "flowchart",
                "graph",
                "chart",
            ],
            kind: SettingKind::Enum {
                default: "auto",
                choices: RENDER_MERMAID_CHOICES,
                supports_preview: false,
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // Security-relevant: "always-approve" bypasses all permission prompts.
        // The modal reads live state from `PagerLocalSnapshot.yolo_mode` (not `ui.permission_mode`) to reflect Ctrl+O toggles immediately
        SettingMeta {
            key: "permission_mode",
            category: SettingCategory::Agent,
            owner: SettingOwner::Shell,
            label: "权限模式",
            description: "默认使用 Agent 内置行为；询问 会在每次工具操作前提示；\
                          自动 用 LLM 分类器处理有风险的工具；总是批准 自动授予全部权限。",
            keywords: &[
                "permission",
                "approve",
                "yolo",
                "agent",
                "always",
                "ask",
                "auto",
                "classifier",
                "tool",
                "danger",
            ],
            kind: SettingKind::Enum {
                default: "ask",
                choices: PERMISSION_MODE_CHOICES,
                supports_preview: false,
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // SHELL-owned `[ui].remember_tool_approvals`. It gates the per-tool "Always allow …" prompt options.
        // `restart_required` because the value is resolved at permission-manager spawn (also fed by env/requirements/managed/remote settings)
        SettingMeta {
            key: "remember_tool_approvals",
            category: SettingCategory::Agent,
            owner: SettingOwner::Shell,
            label: "记住工具批准",
            description: "在权限提示中显示「总是允许」选项，避免对同一命令或工具反复确认。适用于询问与自动模式；总是批准仍会跳过全部提示。需重启。",
            keywords: &[
                "permission",
                "approve",
                "approval",
                "always",
                "allow",
                "remember",
                "tool",
                "command",
                "kubectl",
                "ask",
                "again",
                "whitelist",
            ],
            kind: SettingKind::Bool {
                // The const is shared with the resolver, so the modal shows the effective default when the user layer is unset
                default: xai_grok_shell::util::config::DEFAULT_REMEMBER_TOOL_APPROVALS,
            },
            restart_required: true,
            hidden_in_minimal: false,
        },
        // PAGER-owned; default pinned by `defaults_match_pager_state`.
        SettingMeta {
            key: "multiline_mode",
            category: SettingCategory::Editor,
            owner: SettingOwner::Pager,
            label: "多行输入",
            description: "开启后 Enter 插入换行，Shift+Enter 发送。每会话重置。",
            keywords: &["multiline", "newline", "input", "editor", "enter"],
            kind: SettingKind::Bool { default: false },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // SHELL-owned. It reads from `pager.current_model_name` (not `cfg.models.default`) so the modal reflects `/model` switches.
        // The empty-string default means "no opinion": the shell's resolution applies
        SettingMeta {
            key: "default_model",
            category: SettingCategory::Models,
            owner: SettingOwner::Shell,
            label: "默认模型",
            description: "新会话使用的模型。更改也会切换当前会话。选「（不覆盖）」可清除。",
            keywords: &["model", "default", "agent", "llm", "grok", "switch"],
            kind: SettingKind::DynamicEnum {
                default: "",
                source: DynamicEnumSource::ActiveModelCatalog,
                supports_preview: false,
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // SHARED. `u16` in UiConfig, widened to `i64` for registry.
        // Width changes apply on the next render frame.
        SettingMeta {
            key: MAX_THOUGHTS_WIDTH_KEY,
            category: SettingCategory::Appearance,
            owner: SettingOwner::Shared,
            label: "思考区最大宽度",
            description: "Agent 思考面板的列宽预算（40–500，默认 120）。",
            keywords: &[
                "thoughts",
                "width",
                "max",
                "thinking",
                "panel",
                "reasoning",
                "columns",
            ],
            kind: SettingKind::Int {
                default: ui_default.max_thoughts_width as i64,
                min: MAX_THOUGHTS_WIDTH_MIN,
                max: MAX_THOUGHTS_WIDTH_MAX,
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // SHELL-owned: `[ui].show_thinking_blocks` with a process-wide cache. Default ON.
        SettingMeta {
            key: "show_thinking_blocks",
            category: SettingCategory::Appearance,
            owner: SettingOwner::Shell,
            label: "显示思考块",
            description: "流式输出时在滚动历史中显示 Agent 思考/推理块。",
            keywords: &[
                "thinking",
                "reasoning",
                "thoughts",
                "blocks",
                "show",
                "hide",
            ],
            kind: SettingKind::Bool {
                default: ui_default.show_thinking_blocks.unwrap_or(true),
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // SHELL-owned: `[ui].prompt_suggestions` with a process-wide cache. Default ON.
        // The `GROK_PROMPT_SUGGESTIONS` env var overrides at runtime.
        SettingMeta {
            key: "prompt_suggestions",
            category: SettingCategory::Editor,
            owner: SettingOwner::Shell,
            label: "提示建议",
            description: "每轮结束后预测你可能的下一条提示，并以幽灵文字显示在输入框中（Tab 接受）。\
                          每轮会调用一次小模型。",
            keywords: &[
                "prompt",
                "suggestion",
                "suggestions",
                "autocomplete",
                "ghost",
                "tab",
                "predict",
                "next",
            ],
            kind: SettingKind::Bool {
                default: ui_default.prompt_suggestions.unwrap_or(true),
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // PAGER-owned, persisted to `[scrollback.scroll].respect_manual_folds` in pager.toml (NOT config.toml)
        // The live value is the appearance config (`AppView::set_appearance` fans changes out to every agent)
        // The flag is read at use time, so no restart
        SettingMeta {
            key: "respect_manual_folds",
            category: SettingCategory::Appearance,
            owner: SettingOwner::Pager,
            label: "保留手动折叠",
            description: "流式输出时保持手动折叠的块不变；展开块时停止自动滚动。实验性功能。",
            keywords: &[
                "fold", "pin", "collapse", "expand", "thinking", "follow", "scroll",
            ],
            kind: SettingKind::Bool {
                default: crate::appearance::ScrollConfig::default().respect_manual_folds,
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // SHELL-owned: `[ui].group_tool_verbs` with a process-wide cache. Default ON.
        SettingMeta {
            key: "group_tool_verbs",
            category: SettingCategory::Appearance,
            owner: SettingOwner::Shell,
            label: "合并工具调用",
            description: "将连续的读/搜/列工具调用与子 Agent 行折叠为一行摘要；已完成的思考也会并入该组。",
            keywords: &[
                "group", "tool", "verbs", "fold", "collapse", "read", "search", "summary",
                "thinking", "subagent",
            ],
            kind: SettingKind::Bool {
                default: ui_default.group_tool_verbs.unwrap_or(true),
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // SHELL-owned: `[ui].collapsed_edit_blocks` with a process-wide cache
        // Default OFF (rollout flag; remote settings / managed config can enable).
        SettingMeta {
            key: "collapsed_edit_blocks",
            category: SettingCategory::Appearance,
            owner: SettingOwner::Shell,
            label: "折叠编辑块",
            description: "将编辑显示为一行 +N/-M diffstat 摘要，并把同一文件的连续编辑合并为一块；\
                          展开行可查看 diff。",
            keywords: &[
                "edit",
                "edits",
                "diff",
                "diffstat",
                "collapse",
                "collapsed",
                "summary",
                "expand",
                "one-line",
                "merge",
                "coalesce",
            ],
            kind: SettingKind::Bool {
                default: ui_default.collapsed_edit_blocks.unwrap_or(false),
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // SHELL-owned: `[ui.display_refresh].auto_cadence_enabled`. Restart-required (cadence pinned at startup); hidden in minimal.
        SettingMeta {
            key: "display_refresh_auto_cadence",
            category: SettingCategory::Appearance,
            owner: SettingOwner::Shell,
            label: "匹配显示器刷新率",
            description: "在高刷新率显示器上，TUI 会更快地流式输出/滚动以匹配刷新率。关闭则保持约 60 Hz 节奏。\
                          需重启。",
            keywords: &[
                "display", "refresh", "rate", "hz", "cadence", "fps", "smooth", "scroll", "stream",
                "high", "120", "144",
            ],
            kind: SettingKind::Bool {
                // Nested Option: None inherits DISPLAY_REFRESH_DEFAULT_AUTO_CADENCE_ENABLED.
                default: ui_default
                    .display_refresh
                    .auto_cadence_enabled
                    .unwrap_or(DISPLAY_REFRESH_DEFAULT_AUTO_CADENCE_ENABLED),
            },
            restart_required: true,
            hidden_in_minimal: true,
        },
        // SHELL-owned, persisted to `[ui].scroll_speed` in config.toml.
        SettingMeta {
            key: "scroll_speed",
            category: SettingCategory::Mouse,
            owner: SettingOwner::Shell,
            label: "滚动速度",
            description: "鼠标滚轮与触控板滚动速度倍率（1–100）。越大越快。",
            keywords: &[
                "scroll", "speed", "mouse", "wheel", "trackpad", "fast", "slow",
            ],
            kind: SettingKind::Int {
                default: ui_default.scroll_speed.unwrap_or(50) as i64,
                min: 1,
                max: 100,
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // SHELL-owned `auto` | `wheel` | `trackpad` on `[ui].scroll_mode`.
        SettingMeta {
            key: "scroll_mode",
            category: SettingCategory::Mouse,
            owner: SettingOwner::Shell,
            label: "滚动输入",
            description: "当自动检测误判设备时，强制按滚轮或触控板行为滚动。",
            keywords: &[
                "scroll", "mode", "wheel", "trackpad", "mouse", "detect", "force", "input",
            ],
            kind: SettingKind::Enum {
                default: ui_default
                    .scroll_mode
                    .as_deref()
                    .and_then(ScrollMode::from_canonical)
                    .unwrap_or_default()
                    .as_canonical(),
                choices: SCROLL_MODE_CHOICES,
                supports_preview: false,
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // SHELL-owned, persisted to `[ui].scroll_lines`. One knob covers BOTH wheel and trackpad lines-per-tick.
        // The registered default 3 matches most terminal profiles
        // Until the user first commits a value, the per-terminal profile stays in charge (an unset cache means no override)
        SettingMeta {
            key: "scroll_lines",
            category: SettingCategory::Mouse,
            owner: SettingOwner::Shell,
            label: "滚动行数",
            description: "滚轮与触控板每次滚动的行数（1–10）。未设置前沿用各终端自身配置。",
            keywords: &[
                "scroll", "lines", "tick", "notch", "wheel", "trackpad", "mouse",
            ],
            kind: SettingKind::Int {
                default: ui_default.scroll_lines.map(i64::from).unwrap_or(3),
                min: 1,
                max: 10,
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // SHELL-owned: `[ui].invert_scroll` with a process-wide cache. Default OFF.
        SettingMeta {
            key: "invert_scroll",
            category: SettingCategory::Mouse,
            owner: SettingOwner::Shell,
            label: "反转滚动",
            description: "反转垂直滚动方向（自然滚动）。",
            keywords: &[
                "invert",
                "scroll",
                "natural",
                "direction",
                "reverse",
                "mouse",
                "trackpad",
            ],
            kind: SettingKind::Bool {
                default: ui_default.invert_scroll.unwrap_or(false),
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // SHELL-owned `flash` | `hold` | `word_select` on `[ui].keep_text_selection`. The compile-time default is `flash`.
        // The default can be set remotely via the `keep_text_selection_default` soft-default
        // That staged rollout applies at startup and is not reflected in this static default
        SettingMeta {
            key: "keep_text_selection",
            category: SettingCategory::Mouse,
            owner: SettingOwner::Shell,
            label: "文本选择",
            description: "应用内选区在屏幕上保留多久，以及双击行为（折叠 vs 选中并复制单词）。终端或多路复用器自带选区请在拖动时按住 Shift（原生复制）。",
            keywords: &[
                "selection",
                "drag",
                "copy",
                "flash",
                "hold",
                "shift",
                "native",
                "mouse",
                "tmux",
                "double",
                "double-click",
                "word",
                "terminal",
            ],
            kind: SettingKind::Enum {
                default: TextSelection::Flash.as_canonical(),
                choices: TEXT_SELECTION_CHOICES,
                supports_preview: false,
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // SHELL-owned. Persisted in auth metadata (not config.toml).
        // Reads from `PagerLocalSnapshot.coding_data_sharing_opt_out`.
        // The default "opt-out" matches `AuthEntry::coding_data_retention_opt_out = true`
        // That is the safer consumer default; server enrichment may still opt the user in
        // ZDR / non-admin guards are enforced at dispatch time.
        // Do not put "telemetry" in keywords: that word is the config-file analytics toggle (Monitoring / Configuration docs)
        SettingMeta {
            key: "coding_data_sharing",
            category: SettingCategory::Privacy,
            owner: SettingOwner::Shell,
            label: "编码数据共享",
            description: "控制是否允许保留编码会话数据用于模型训练。不影响产品分析；详见配置与监控文档。",
            keywords: &[
                "privacy",
                "data",
                "sharing",
                "coding",
                "retention",
                "training",
                "opt-in",
                "opt-out",
            ],
            kind: SettingKind::Enum {
                default: "opt-out",
                choices: CODING_DATA_SHARING_CHOICES,
                supports_preview: false,
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // SHELL-owned, persisted to `[ui].default_selected_permission` in config.toml
        // Read by the pager via `appearance::permission_cursor`
        // Canonical `always_allow_all_sessions` (the effective default) lands the first prompt's cursor on the enable-always-approve row
        // Subsequent prompts stick to the last-used kind
        SettingMeta {
            key: "default_selected_permission",
            category: SettingCategory::Agent,
            owner: SettingOwner::Shell,
            label: "默认选中的权限项",
            description: "权限提示中光标默认选中的行。",
            keywords: &[
                "permission",
                "approval",
                "cursor",
                "preselect",
                "default",
                "sticky",
                "last",
                "used",
                "yes",
                "no",
                "reject",
                "allow",
            ],
            kind: SettingKind::Enum {
                default: DefaultSelectedPermission::AlwaysAllowAllSessions.as_canonical(),
                choices: DEFAULT_SELECTED_PERMISSION_CHOICES,
                supports_preview: false,
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // SHELL-owned `[toolset.ask_user_question].timeout_enabled`
        // This row edits the user-config layer of the tiered timeout gate
        // Requirements, env, managed, and remote settings feed the effective value at agent build
        // The default is the const shared with the resolver
        // `restart_required` because the value is resolved when an agent is built, like `remember_tool_approvals`
        SettingMeta {
            key: "toolset.ask_user_question.timeout_enabled",
            category: SettingCategory::Agent,
            owner: SettingOwner::Shell,
            label: "提问超时",
            description: "开启后，ask_user_question 工具会在设定时间后超时，而不是无限阻塞。",
            keywords: &[
                "ask",
                "question",
                "questionnaire",
                "timeout",
                "ask_user_question",
                "block",
                "wait",
                "forever",
                "tool",
            ],
            kind: SettingKind::Bool {
                default: ask_user_question::DEFAULT_ASK_USER_QUESTION_TIMEOUT_ENABLED,
            },
            restart_required: true,
            hidden_in_minimal: false,
        },
        // SHELL-owned `[session].auto_retry_incomplete_end_turn`. Opt-in
        // recovery when the model ends a turn with a plan-only message after
        // tools but no writes (issue #6). Default off; applies to new sessions.
        SettingMeta {
            key: "session.auto_retry_incomplete_end_turn",
            category: SettingCategory::Agent,
            owner: SettingOwner::Shell,
            label: "未完成回合自动重试",
            description: "模型在调用工具后仅输出计划、未真正改文件就结束回合时，自动注入提醒并再采样一次。默认关闭；开启可能增加模型调用。",
            keywords: &[
                "retry",
                "auto",
                "incomplete",
                "end_turn",
                "end turn",
                "premature",
                "interrupted",
                "recovery",
                "plan only",
                "重试",
                "中断",
                "未完成",
                "自动",
            ],
            kind: SettingKind::Bool { default: false },
            restart_required: true,
            hidden_in_minimal: false,
        },
        // PAGER-owned, ACP-mediated. Reads from
        // `PagerLocalSnapshot.plan_mode_active`. Default "off" matches
        // `AgentView::new`'s `plan_mode_active = false`.
        SettingMeta {
            key: "plan_mode",
            category: SettingCategory::Agent,
            owner: SettingOwner::Pager,
            label: "计划模式",
            description: "开启后，Agent 在运行工具或编辑前会先总结计划。",
            keywords: &[
                "plan", "mode", "agent", "summary", "approval", "review", "session",
            ],
            kind: SettingKind::Enum {
                default: "off",
                choices: PLAN_MODE_CHOICES,
                supports_preview: false,
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // SHELL-owned startup-time settings (restart_required: true).
        // The running pager doesn't re-read these mid-session.
        SettingMeta {
            key: "show_tips",
            category: SettingCategory::Advanced,
            owner: SettingOwner::Shell,
            label: "显示提示",
            description: "启动时显示每日提示横幅。需重启。",
            keywords: &[
                "tips", "tip", "show", "banner", "welcome", "startup", "launch",
            ],
            kind: SettingKind::Bool { default: true },
            restart_required: true,
            hidden_in_minimal: false,
        },
        // Contextual hints: one Advanced row that opens a sub-sheet of per-tip toggles
        // It applies live (restart_required: false); the group carries no value and its children are hidden from the top-level list
        SettingMeta {
            key: "contextual_hints",
            category: SettingCategory::Advanced,
            owner: SettingOwner::Shell,
            label: "显示情境提示",
            description: "工作时显示简短的情境快捷键提示；可逐项开关。",
            keywords: &[
                "contextual",
                "hints",
                "tips",
                "undo",
                "plan",
                "nudge",
                "image",
                "clipboard",
                "ephemeral",
                "send",
                "interject",
                "queue",
                // Child-specific terms: the per-tip children are hidden from the top-level list, so their search words are mirrored here
                // A query like "ctrl+z" or "shift+tab" would otherwise dead-end
                "ctrl+z",
                "draft",
                "wipe",
                "mode",
                "shift+tab",
                "paste",
                "input",
                "enter",
                "follow-up",
                "small",
                "screen",
                "compact",
                "ssh",
                "wrap",
                "remote",
                // copy/export/transcript stay on the export_copy child so a "copy" query does not match the group.
            ],
            kind: SettingKind::Group {
                children: CONTEXTUAL_HINTS_CHILDREN,
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        SettingMeta {
            key: "auto_update",
            category: SettingCategory::Advanced,
            owner: SettingOwner::Shell,
            label: "自动更新",
            description: "启动时自动下载并安装分页器更新。需重启。",
            keywords: &[
                "auto",
                "update",
                "updates",
                "auto-update",
                "upgrade",
                "version",
                "install",
                "channel",
            ],
            kind: SettingKind::Bool { default: true },
            restart_required: true,
            hidden_in_minimal: false,
        },
        // SHELL-owned, persisted to `[ui].hunk_tracker_mode`. Restart-required: the mode is read once when the session connects.
        SettingMeta {
            key: "hunk_tracker_mode",
            category: SettingCategory::Advanced,
            owner: SettingOwner::Shell,
            label: "变更块跟踪",
            description: "Agent 将哪些文件变更作为块跟踪。关闭则完全禁用跟踪（及代码行统计）。 \
                          Restart required.",
            keywords: &[
                "hunk", "tracker", "tracking", "diff", "changes", "git", "loc", "off", "disable",
            ],
            kind: SettingKind::Enum {
                default: "off",
                choices: HUNK_TRACKER_MODE_CHOICES,
                supports_preview: false,
            },
            restart_required: true,
            hidden_in_minimal: false,
        },
        // SHELL-owned, persisted to `[ui].voice_keybind_enabled`. Default ON: `None` (inherit) reads as `true`.
        // Off disables only the Ctrl+Space / F8 chord; `/voice` (and Esc / the recording-row `[stop]`) keep working
        SettingMeta {
            key: "voice_keybind_enabled",
            category: SettingCategory::Editor,
            owner: SettingOwner::Shell,
            label: "Voice shortcut",
            description: "Enable the Ctrl+Space / F8 shortcut for voice dictation. \
                          When off, the keys are ignored; /voice still starts \
                          dictation.",
            keywords: &[
                "voice",
                "dictation",
                "mic",
                "microphone",
                "speech",
                "stt",
                "keybinding",
                "hotkey",
                "ctrl+space",
                "f8",
                "disable",
            ],
            kind: SettingKind::Bool {
                default: ui_default.voice_keybind_enabled.unwrap_or(true),
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // SHELL-owned, persisted to `[ui].voice_capture_mode`
        // The `hold` choice is hidden on terminals without key-release reporting (see `effective_enum_choices`)
        // It falls back to `toggle` at runtime
        SettingMeta {
            key: "voice_capture_mode",
            category: SettingCategory::Editor,
            owner: SettingOwner::Shell,
            label: "语音捕获",
            description: "语音快捷键（Ctrl+Space / F8）的行为：切换（再按开始/停止）或按住说话\
                          （按住录音、松开停止；需要 Kitty 协议终端）。",
            keywords: &[
                "voice",
                "dictation",
                "dictate",
                "mic",
                "microphone",
                "speech",
                "stt",
                "toggle",
                "hold",
                "ctrl+space",
                "f8",
                "push-to-talk",
            ],
            kind: SettingKind::Enum {
                default: "hold",
                choices: VOICE_CAPTURE_MODE_CHOICES,
                supports_preview: false,
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // SHELL-owned, persisted to `[ui].voice_stt_language`. Applied live to the next voice capture (no restart).
        // Default English; System (`auto`) follows the process locale when it maps to a Grok STT language
        // The catalog is the official STT languages (see xai_grok_voice::STT_LANGUAGES)
        SettingMeta {
            key: "voice_stt_language",
            category: SettingCategory::Editor,
            owner: SettingOwner::Shell,
            label: "语音语言",
            description: "语音听写的语音转文字语言。默认英语；系统选项在受支持时跟随区域设置。\
                          同时决定数字与货币的格式语言。",
            keywords: &["voice", "language", "locale", "dictation", "stt", "speech"],
            kind: SettingKind::Enum {
                default: "en",
                choices: VOICE_STT_LANGUAGE_CHOICES,
                supports_preview: false,
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // Contextual-hint children (hidden from the top-level list; reached via the group sub-sheet)
        // Default ON: `None` (inherit) reads as `true`
        SettingMeta {
            key: "contextual_hints.undo",
            category: SettingCategory::Advanced,
            owner: SettingOwner::Shell,
            label: "撤销",
            description: "清空提示后提醒你可用 Ctrl+Z 恢复。",
            keywords: &["undo", "ctrl+z", "draft", "wipe", "hint"],
            kind: SettingKind::Bool {
                default: ui_default.contextual_hints.undo.unwrap_or(true),
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        SettingMeta {
            key: "contextual_hints.plan_mode",
            category: SettingCategory::Advanced,
            owner: SettingOwner::Shell,
            label: "计划模式",
            description: "当提示像规划请求时，建议使用计划模式（Shift+Tab）。",
            keywords: &["plan", "mode", "nudge", "shift+tab", "hint"],
            kind: SettingKind::Bool {
                default: ui_default.contextual_hints.plan_mode.unwrap_or(true),
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        SettingMeta {
            key: "contextual_hints.image_input",
            category: SettingCategory::Advanced,
            owner: SettingOwner::Shell,
            label: "图片输入",
            description: "剪贴板有图片且模型支持时，提示粘贴图片。",
            keywords: &["image", "clipboard", "paste", "input", "hint"],
            kind: SettingKind::Bool {
                default: ui_default.contextual_hints.image_input.unwrap_or(true),
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        SettingMeta {
            key: "contextual_hints.send_now",
            category: SettingCategory::Advanced,
            owner: SettingOwner::Shell,
            label: "立即发送",
            description: "在轮次中途排队跟进后，提醒你在空提示上按 Enter 可立即发送队首项。",
            keywords: &[
                "send",
                "now",
                "interject",
                "queue",
                "follow-up",
                "enter",
                "empty",
                "hint",
            ],
            kind: SettingKind::Bool {
                default: ui_default.contextual_hints.send_now.unwrap_or(true),
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        SettingMeta {
            key: "contextual_hints.small_screen",
            category: SettingCategory::Advanced,
            owner: SettingOwner::Shell,
            label: "小屏幕",
            description: "终端行数不足时，每次运行提示一次 /compact-mode。",
            keywords: &["small", "screen", "compact", "space", "rows", "hint"],
            kind: SettingKind::Bool {
                default: ui_default.contextual_hints.small_screen.unwrap_or(true),
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        SettingMeta {
            key: "contextual_hints.word_select",
            category: SettingCategory::Advanced,
            owner: SettingOwner::Shell,
            label: "选词",
            description: "在文本选择为折叠/导航时双击会话文本后，提醒你可在设置中切换为选词。",
            keywords: &[
                "word",
                "select",
                "double",
                "double-click",
                "click",
                "fold",
                "selection",
                "settings",
                "hint",
            ],
            kind: SettingKind::Bool {
                default: ui_default.contextual_hints.word_select.unwrap_or(true),
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        SettingMeta {
            key: "contextual_hints.export_copy",
            category: SettingCategory::Advanced,
            owner: SettingOwner::Shell,
            label: "Copy and export",
            description: "After three nearby drag-copies of conversation text, \
                          remind you that /copy and /export exist.",
            keywords: &["copy", "export", "transcript", "clipboard", "hint"],
            kind: SettingKind::Bool {
                default: ui_default.contextual_hints.export_copy.unwrap_or(true),
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        SettingMeta {
            key: "contextual_hints.ssh_wrap",
            category: SettingCategory::Advanced,
            owner: SettingOwner::Shell,
            label: "SSH 包装",
            description: "通过 SSH 加载会话时，建议使用 `chaos wrap ssh` 以获得剪贴板转发与终端恢复。",
            keywords: &[
                "ssh",
                "wrap",
                "remote",
                "clipboard",
                "restore",
                "startup",
                "hint",
            ],
            kind: SettingKind::Bool {
                default: ui_default.contextual_hints.ssh_wrap.unwrap_or(true),
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
        // ── TodoGate (runtime turn-end backstop) ──────────────────────
        //
        // Only the CLI flag (`--todo-gate`) is wired
        // Settings-modal entries for `[reminder.todo_gate]` are deferred
        // The modal dispatcher requires per-key action arms in `settings_modal.rs`, `app/dispatch.rs`, and `settings/registry.rs`
        // Those arms don't yet have a place to land
        // SHELL-owned. `restart_required: false` because the config-reloader rebroadcasts UI changes; mid-session forks pick up new values.
        // The empty-string default means "no opinion": the shell's resolution applies
        SettingMeta {
            key: "fork_secondary_model",
            category: SettingCategory::Models,
            owner: SettingOwner::Shell,
            label: "分叉副模型",
            description: "分叉时副 Agent 使用的模型。选「（不覆盖）」可清除。",
            keywords: &[
                "fork",
                "secondary",
                "model",
                "agent",
                "subagent",
                "branch",
                "models",
            ],
            kind: SettingKind::DynamicEnum {
                default: "",
                source: DynamicEnumSource::ActiveModelCatalog,
                supports_preview: false,
            },
            restart_required: false,
            hidden_in_minimal: false,
        },
    ]
}
