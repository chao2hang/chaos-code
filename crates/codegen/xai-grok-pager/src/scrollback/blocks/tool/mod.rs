mod edit;
mod execute;
pub(crate) mod hook;
mod lifecycle;
pub mod list_dir;
pub(crate) mod memory_search;
mod other;
mod read;
pub mod search;
mod search_tool;
mod sent_message;
mod use_tool;
mod web_fetch;
mod web_search;

pub use edit::{
    DiffLineOutput, DiffRenderConfig, EDIT_HL_MAX_BYTES, EDIT_HL_MAX_LINES, EditHighlightPhase,
    EditLineStyles, EditToolCallBlock, compute_file_scoped_styles, file_text_within_hl_caps,
    render_diff_hunk_highlighted, render_diff_hunks_highlighted, render_diff_hunks_with_styles,
};
pub use execute::ExecuteToolCallBlock;
pub use hook::{HookPhase, HookRunEntry, HookRunStatus, ToolCallHookData};
pub use lifecycle::LifecycleEventBlock;
pub use list_dir::ListDirToolCallBlock;
pub use memory_search::MemorySearchToolCallBlock;
pub use other::OtherToolCallBlock;
pub use read::{ReadMediaKind, ReadToolCallBlock};
pub use search::{
    SearchFileMatch, SearchInputMeta, SearchLineMatch, SearchOutputMode, SearchToolCallBlock,
};
pub use search_tool::{
    DiscoveredTool, SearchToolCallBlock as IntegrationSearchToolCallBlock, discovered_tool_action,
};
pub use sent_message::{SentMessagePresentation, SentMessageToolCallBlock};
pub use use_tool::UseToolCallBlock;
pub use web_fetch::WebFetchToolCallBlock;
pub use web_search::WebSearchToolCallBlock;

use crate::appearance::AppearanceConfig;
use crate::scrollback::block::{BlockContent, join_searchable};
use crate::scrollback::types::{
    AccentStyle, BlockBackground, BlockContext, BlockOutput, DisplayMode,
};
use std::fmt;

/// Shared selection-range id for tool-call header lines.
///
/// Headers are single logical selection targets (path/query/url/command);
/// using one id across tool kinds keeps multi-line drag/copy grouping simple.
pub(crate) const TOOL_HEADER_RANGE: u16 = 0;

/// 1-based inclusive line range for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    /// Start line (1-based).
    pub start: usize,
    /// End line (1-based, inclusive).
    pub end: usize,
}

impl LineRange {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

impl fmt::Display for LineRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.start, self.end)
    }
}

/// Names what a verb-groupable (non-destructive) run member touched.
/// A folded run of consecutive rows renders as "Read 3 files" or "Searched 4 patterns".
/// Most kinds classify tool blocks via [`ToolCallBlock::verb_group_kind`].
/// `Subagent` classifies subagent lifecycle render blocks, which are not tool calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VerbGroupKind {
    /// Plain file reads.
    File,
    /// Skill reads and `Skill` invocations (distinct noun from plain files).
    Skill,
    /// Pattern searches (grep/glob).
    Search,
    /// Directory listings.
    Dir,
    /// Web fetches.
    WebFetch,
    /// Web searches, including X search.
    WebSearch,
    /// Memory searches.
    MemorySearch,
    /// MCP tool discovery (`search_tool`).
    IntegrationSearch,
    /// Subagent lifecycle rows (`RenderBlock::Subagent`).
    Subagent,
    /// Shell commands. Label-only: commands never fold eagerly ([`ToolCallBlock::verb_group_kind`] excludes them).
    /// A truncation header describing hidden rows still buckets them ("Ran 6 commands").
    Command,
    /// File edits. Label-only, like [`Self::Command`].
    EditFile,
    /// MCP tool dispatches (`use_tool`). Label-only, like [`Self::Command`].
    McpCall,
    /// Sent subagent messages. Label-only, like [`Self::Command`].
    Message,
    /// Unclassified tools. Label-only, like [`Self::Command`].
    OtherTool,
}

impl VerbGroupKind {
    /// Verb-group row verb: present tense while running, past otherwise.
    pub fn verb(self, running: bool) -> &'static str {
        let (past, present) = match self {
            VerbGroupKind::File | VerbGroupKind::Skill => ("读取", "读取中"),
            VerbGroupKind::Search
            | VerbGroupKind::WebSearch
            | VerbGroupKind::MemorySearch
            | VerbGroupKind::IntegrationSearch => ("搜索", "搜索中"),
            VerbGroupKind::Dir => ("列出", "列出中"),
            VerbGroupKind::WebFetch => ("抓取", "抓取中"),
            VerbGroupKind::Subagent | VerbGroupKind::Command | VerbGroupKind::OtherTool => {
                ("运行", "运行中")
            }
            VerbGroupKind::EditFile => ("编辑", "编辑中"),
            VerbGroupKind::McpCall => ("调用", "调用中"),
            VerbGroupKind::Message => ("发送", "发送中"),
        };
        if running { present } else { past }
    }

    /// Verb-group row noun, pluralized by `count`.
    ///
    /// Chinese uses measure words; singular/plural share the same form.
    pub fn noun(self, count: usize) -> &'static str {
        let (one, many) = match self {
            VerbGroupKind::File | VerbGroupKind::EditFile => ("个文件", "个文件"),
            VerbGroupKind::Skill => ("个技能", "个技能"),
            VerbGroupKind::Search => ("个模式", "个模式"),
            VerbGroupKind::Dir => ("个目录", "个目录"),
            VerbGroupKind::WebFetch | VerbGroupKind::WebSearch => ("个网站", "个网站"),
            VerbGroupKind::MemorySearch => ("条记忆", "条记忆"),
            VerbGroupKind::IntegrationSearch | VerbGroupKind::McpCall => {
                ("个 MCP 工具", "个 MCP 工具")
            }
            VerbGroupKind::Subagent => ("个子代理", "个子代理"),
            VerbGroupKind::Command => ("个命令", "个命令"),
            VerbGroupKind::OtherTool => ("个工具", "个工具"),
            VerbGroupKind::Message => ("条消息", "条消息"),
        };
        let _ = count;
        if count == 1 { one } else { many }
    }
}

/// BlockContent is implemented via match-based delegation so we can intercept `output()` to prepend the tool bullet configured in appearance.
#[derive(Debug, Clone)]
pub enum ToolCallBlock {
    Execute(ExecuteToolCallBlock),
    Read(ReadToolCallBlock),
    /// Edit a file (with diff).
    Edit(EditToolCallBlock),
    ListDir(ListDirToolCallBlock),
    Search(SearchToolCallBlock),
    WebFetch(WebFetchToolCallBlock),
    /// Web search with citations.
    WebSearch(WebSearchToolCallBlock),
    /// MCP integration tool discovery (search_tool).
    IntegrationSearch(IntegrationSearchToolCallBlock),
    /// MCP integration tool dispatch (use_tool).
    UseTool(UseToolCallBlock),
    MemorySearch(MemorySearchToolCallBlock),
    SentMessage(SentMessageToolCallBlock),
    /// Skill invocation (user skills / slash commands via the Skill tool).
    Skill(OtherToolCallBlock),
    Other(OtherToolCallBlock),
    /// Lifecycle event (e.g. `user_prompt_submit`, `session_start`).
    /// Not a real tool call, so `last_tool_call_entry_id()` skips it.
    Lifecycle(LifecycleEventBlock),
}

/// Delegate to inner variant, with tool bullet prepended to output.
macro_rules! delegate_tool {
    ($self:expr, $method:ident ( $($arg:expr),* )) => {
        match $self {
            ToolCallBlock::Execute(b) => b.$method($($arg),*),
            ToolCallBlock::Read(b) => b.$method($($arg),*),
            ToolCallBlock::Edit(b) => b.$method($($arg),*),
            ToolCallBlock::ListDir(b) => b.$method($($arg),*),
            ToolCallBlock::Search(b) => b.$method($($arg),*),
            ToolCallBlock::WebFetch(b) => b.$method($($arg),*),
            ToolCallBlock::WebSearch(b) => b.$method($($arg),*),
            ToolCallBlock::IntegrationSearch(b) => b.$method($($arg),*),
            ToolCallBlock::UseTool(b) => b.$method($($arg),*),
            ToolCallBlock::MemorySearch(b) => b.$method($($arg),*),
            ToolCallBlock::SentMessage(b) => b.$method($($arg),*),
            ToolCallBlock::Skill(b) => b.$method($($arg),*),
            ToolCallBlock::Other(b) => b.$method($($arg),*),
            ToolCallBlock::Lifecycle(b) => b.$method($($arg),*),
        }
    };
}

impl BlockContent for ToolCallBlock {
    fn output(&self, ctx: &BlockContext) -> BlockOutput {
        // Bullet prepending is handled by RenderBlock::output() via has_bullet().
        delegate_tool!(self, output(ctx))
    }

    fn accent(&self, ctx: &BlockContext) -> Option<AccentStyle> {
        delegate_tool!(self, accent(ctx))
    }

    fn bullet(&self, ctx: &BlockContext) -> Option<AccentStyle> {
        delegate_tool!(self, bullet(ctx))
    }

    fn accent_background(&self, ctx: &BlockContext) -> bool {
        delegate_tool!(self, accent_background(ctx))
    }

    fn background(&self, ctx: &BlockContext) -> BlockBackground {
        delegate_tool!(self, background(ctx))
    }

    fn has_vpad_for(&self, appearance: &AppearanceConfig) -> bool {
        delegate_tool!(self, has_vpad_for(appearance))
    }

    fn has_raw_mode(&self) -> bool {
        delegate_tool!(self, has_raw_mode())
    }

    fn is_foldable(&self) -> bool {
        delegate_tool!(self, is_foldable())
    }

    fn next_fold_mode(&self, current: DisplayMode, is_running: bool) -> DisplayMode {
        delegate_tool!(self, next_fold_mode(current, is_running))
    }

    fn collapse_mode(&self, is_running: bool) -> DisplayMode {
        delegate_tool!(self, collapse_mode(is_running))
    }

    fn default_display_mode(&self) -> DisplayMode {
        delegate_tool!(self, default_display_mode())
    }

    fn finished_display_mode(&self) -> Option<DisplayMode> {
        delegate_tool!(self, finished_display_mode())
    }

    fn is_selectable(&self) -> bool {
        delegate_tool!(self, is_selectable())
    }

    fn has_bullet(&self, ctx: &BlockContext) -> bool {
        ctx.appearance
            .scrollback
            .blocks
            .tool
            .bullet
            .char()
            .is_some()
    }

    fn is_groupable(&self) -> bool {
        true
    }

    fn image_references(&self) -> &[crate::prompt_images::ScrollbackImageRef] {
        delegate_tool!(self, image_references())
    }

    fn video_references(&self) -> &[crate::prompt_images::ScrollbackVideoRef] {
        delegate_tool!(self, video_references())
    }

    fn inline_media(&self) -> Option<crate::prompt_images::InlineMediaInfo> {
        delegate_tool!(self, inline_media())
    }

    fn inline_open_button(&self) -> Option<(std::path::PathBuf, bool)> {
        delegate_tool!(self, inline_open_button())
    }

    fn preamble(&self, ctx: &BlockContext) -> Option<ratatui::text::Text<'static>> {
        delegate_tool!(self, preamble(ctx))
    }
}

impl ToolCallBlock {
    /// Transfer timing data from another block of the same variant.
    ///
    /// Used when a running block is replaced with its completed version (e.g., in the `handle_tool_call_update` completion path).
    /// The new block inherits `started_at` from the old block so `finish()` can compute real elapsed time.
    pub fn transfer_timing_from(&mut self, old: &ToolCallBlock) {
        match (self, old) {
            (ToolCallBlock::Execute(new), ToolCallBlock::Execute(old)) => {
                new.started_at = old.started_at;
            }
            (ToolCallBlock::Read(new), ToolCallBlock::Read(old)) => {
                new.started_at = old.started_at;
            }
            (ToolCallBlock::Edit(new), ToolCallBlock::Edit(old)) => {
                new.started_at = old.started_at;
            }
            (ToolCallBlock::Search(new), ToolCallBlock::Search(old)) => {
                new.started_at = old.started_at;
            }
            (ToolCallBlock::ListDir(new), ToolCallBlock::ListDir(old)) => {
                new.started_at = old.started_at;
            }
            (ToolCallBlock::WebFetch(new), ToolCallBlock::WebFetch(old)) => {
                new.started_at = old.started_at;
            }
            (ToolCallBlock::WebSearch(new), ToolCallBlock::WebSearch(old)) => {
                new.started_at = old.started_at;
            }
            (ToolCallBlock::IntegrationSearch(new), ToolCallBlock::IntegrationSearch(old)) => {
                new.started_at = old.started_at;
            }
            (ToolCallBlock::UseTool(new), ToolCallBlock::UseTool(old)) => {
                new.started_at = old.started_at;
            }
            (ToolCallBlock::SentMessage(new), ToolCallBlock::SentMessage(old)) => {
                new.started_at = old.started_at;
            }
            (ToolCallBlock::Skill(new), ToolCallBlock::Skill(old)) => {
                new.started_at = old.started_at;
            }
            (ToolCallBlock::Other(new), ToolCallBlock::Other(old)) => {
                new.started_at = old.started_at;
            }
            (
                ToolCallBlock::Execute(_)
                | ToolCallBlock::Read(_)
                | ToolCallBlock::Edit(_)
                | ToolCallBlock::ListDir(_)
                | ToolCallBlock::Search(_)
                | ToolCallBlock::WebFetch(_)
                | ToolCallBlock::WebSearch(_)
                | ToolCallBlock::IntegrationSearch(_)
                | ToolCallBlock::UseTool(_)
                | ToolCallBlock::MemorySearch(_)
                | ToolCallBlock::SentMessage(_)
                | ToolCallBlock::Skill(_)
                | ToolCallBlock::Other(_)
                | ToolCallBlock::Lifecycle(_),
                _,
            ) => {}
        }
    }

    /// Whether the tool call finished without an error.
    pub fn is_success(&self) -> bool {
        match self {
            ToolCallBlock::Execute(b) => b.is_success(),
            ToolCallBlock::Read(b) => b.is_success(),
            ToolCallBlock::Edit(b) => b.is_success(),
            ToolCallBlock::Search(b) => b.is_success(),
            ToolCallBlock::ListDir(b) => b.is_success(),
            ToolCallBlock::WebFetch(b) => b.is_success(),
            ToolCallBlock::WebSearch(b) => b.is_success(),
            ToolCallBlock::IntegrationSearch(b) => b.is_success(),
            ToolCallBlock::UseTool(b) => b.is_success(),
            ToolCallBlock::MemorySearch(b) => b.is_success(),
            ToolCallBlock::SentMessage(b) => b.is_success(),
            ToolCallBlock::Skill(b) => b.is_success(),
            ToolCallBlock::Other(b) => b.is_success(),
            ToolCallBlock::Lifecycle(_) => true,
        }
    }

    /// Set `started_at` on the inner variant block.
    ///
    /// Unlike `transfer_timing_from`, this works across variant boundaries.
    /// For example, it can set `started_at` on a `Search` block from a value captured when the block was still `Other`.
    pub fn set_started_at(&mut self, instant: std::time::Instant) {
        match self {
            ToolCallBlock::Execute(b) => b.started_at = Some(instant),
            ToolCallBlock::Read(b) => b.started_at = Some(instant),
            ToolCallBlock::Edit(b) => b.started_at = Some(instant),
            ToolCallBlock::Search(b) => b.started_at = Some(instant),
            ToolCallBlock::ListDir(b) => b.started_at = Some(instant),
            ToolCallBlock::WebFetch(b) => b.started_at = Some(instant),
            ToolCallBlock::WebSearch(b) => b.started_at = Some(instant),
            ToolCallBlock::IntegrationSearch(b) => b.started_at = Some(instant),
            ToolCallBlock::UseTool(b) => b.started_at = Some(instant),
            ToolCallBlock::MemorySearch(b) => b.started_at = Some(instant),
            ToolCallBlock::SentMessage(b) => b.started_at = Some(instant),
            ToolCallBlock::Skill(b) => b.started_at = Some(instant),
            ToolCallBlock::Other(b) => b.started_at = Some(instant),
            // Lifecycle events have no timing.
            ToolCallBlock::Lifecycle(_) => {}
        }
    }

    /// Start timing for this block (sets `started_at = now`).
    ///
    /// Called when a block enters running UI state.
    /// Only blocks that actually run in the UI get meaningful timing.
    /// Pre-completed blocks keep `started_at = None` and show no timing data.
    pub fn start_timing(&mut self) {
        match self {
            ToolCallBlock::Execute(b) => {
                if b.started_at.is_none() {
                    b.started_at = Some(std::time::Instant::now());
                }
            }
            ToolCallBlock::Read(b) => {
                if b.started_at.is_none() {
                    b.started_at = Some(std::time::Instant::now());
                }
            }
            ToolCallBlock::Edit(b) => {
                if b.started_at.is_none() {
                    b.started_at = Some(std::time::Instant::now());
                }
            }
            ToolCallBlock::Search(b) => {
                if b.started_at.is_none() {
                    b.started_at = Some(std::time::Instant::now());
                }
            }
            ToolCallBlock::ListDir(b) => {
                if b.started_at.is_none() {
                    b.started_at = Some(std::time::Instant::now());
                }
            }
            ToolCallBlock::WebFetch(b) => {
                if b.started_at.is_none() {
                    b.started_at = Some(std::time::Instant::now());
                }
            }
            ToolCallBlock::WebSearch(b) => {
                if b.started_at.is_none() {
                    b.started_at = Some(std::time::Instant::now());
                }
            }
            ToolCallBlock::IntegrationSearch(b) => {
                if b.started_at.is_none() {
                    b.started_at = Some(std::time::Instant::now());
                }
            }
            ToolCallBlock::UseTool(b) => {
                if b.started_at.is_none() {
                    b.started_at = Some(std::time::Instant::now());
                }
            }
            ToolCallBlock::MemorySearch(b) => {
                if b.started_at.is_none() {
                    b.started_at = Some(std::time::Instant::now());
                }
            }
            ToolCallBlock::SentMessage(b) => {
                if b.started_at.is_none() {
                    b.started_at = Some(std::time::Instant::now());
                }
            }
            ToolCallBlock::Skill(b) => {
                if b.started_at.is_none() {
                    b.started_at = Some(std::time::Instant::now());
                }
            }
            ToolCallBlock::Other(b) => {
                if b.started_at.is_none() {
                    b.started_at = Some(std::time::Instant::now());
                }
            }
            // Lifecycle events have no timing.
            ToolCallBlock::Lifecycle(_) => {}
        }
    }

    /// Create from tool name string (for parsing ACP tool calls).
    pub fn from_name(name: &str, summary: impl Into<String>) -> Self {
        match name.to_lowercase().as_str() {
            "run_terminal_command" | "run_terminal_cmd" | "bash" | "shell" | "execute" => {
                ToolCallBlock::Execute(ExecuteToolCallBlock::new(summary))
            }
            "read_file" | "read" => ToolCallBlock::Read(ReadToolCallBlock::new(summary)),
            "search_replace" | "edit" | "apply_patch" | "strreplace" => {
                ToolCallBlock::Edit(EditToolCallBlock::new(summary, Vec::new()))
            }
            "write" => ToolCallBlock::Edit(
                EditToolCallBlock::new(summary, Vec::new()).with_prefix("Creating "),
            ),
            "list_dir" | "ls" => ToolCallBlock::ListDir(ListDirToolCallBlock::new(summary)),
            "grep" | "search" | "glob" => {
                ToolCallBlock::Search(SearchToolCallBlock::new(summary.into()))
            }
            "web_fetch" | "fetch" => ToolCallBlock::WebFetch(WebFetchToolCallBlock::new(summary)),
            "web_search" => ToolCallBlock::WebSearch(WebSearchToolCallBlock::new(summary)),
            "search_tool" => {
                ToolCallBlock::IntegrationSearch(IntegrationSearchToolCallBlock::new(summary))
            }
            "use_tool" => ToolCallBlock::UseTool(UseToolCallBlock::new(summary)),
            "skill" => ToolCallBlock::Skill(OtherToolCallBlock::new("Skill", summary)),
            _ => ToolCallBlock::Other(OtherToolCallBlock::new(name, summary)),
        }
    }

    /// Full stored SOURCE text of this tool call for full-text scrollback search.
    ///
    /// Reads stored source fields and the `copy_text` accessors that read source data.
    /// It never lays out (`output()`, word-wrap) or syntax-highlights, so indexing stays cheap.
    pub(crate) fn searchable_text(&self) -> Option<String> {
        match self {
            ToolCallBlock::Execute(b) => join_searchable([
                Some(b.command.clone()),
                b.description.clone(),
                b.output.clone(),
                b.error.clone(),
            ]),
            ToolCallBlock::Read(b) => {
                join_searchable([Some(b.path.clone()), b.content.clone(), b.error.clone()])
            }
            ToolCallBlock::Edit(b) => join_searchable([Some(b.copy_text()), b.error.clone()]),
            ToolCallBlock::ListDir(b) => join_searchable([
                Some(b.path.clone()),
                Some(b.output.clone()),
                b.error.clone(),
            ]),
            ToolCallBlock::Search(b) => {
                // Each file group contributes its path plus every matched line.
                let file_matches = join_searchable(b.file_matches.iter().flat_map(|fm| {
                    std::iter::once(Some(fm.path.clone()))
                        .chain(fm.matches.iter().map(|m| Some(m.content.clone())))
                }));
                let file_paths = join_searchable(b.file_paths.iter().cloned().map(Some));
                join_searchable([
                    Some(b.pattern.clone()),
                    b.meta.path.clone(),
                    b.meta.glob.clone(),
                    b.meta.file_type.clone(),
                    file_paths,
                    file_matches,
                    b.error.clone(),
                ])
            }
            ToolCallBlock::WebFetch(b) => {
                join_searchable([Some(b.url.clone()), b.output.clone(), b.error.clone()])
            }
            ToolCallBlock::WebSearch(b) => {
                let citations = join_searchable(b.citations.iter().cloned().map(Some));
                join_searchable([
                    Some(b.query.clone()),
                    b.content.clone(),
                    citations,
                    b.label.clone(),
                    b.error.clone(),
                ])
            }
            ToolCallBlock::IntegrationSearch(b) => {
                join_searchable([Some(b.copy_text()), b.content.clone(), b.error.clone()])
            }
            ToolCallBlock::UseTool(b) => join_searchable([Some(b.copy_text()), b.error.clone()]),
            ToolCallBlock::MemorySearch(b) => {
                // Flatten each result's source, path, and snippet.
                let results = join_searchable(b.results.iter().flat_map(|r| {
                    [
                        Some(r.source.clone()),
                        Some(r.path.clone()),
                        Some(r.snippet.clone()),
                    ]
                }));
                join_searchable([Some(b.query.clone()), results, b.error.clone()])
            }
            ToolCallBlock::SentMessage(b) => b.searchable_text(),
            ToolCallBlock::Skill(b) | ToolCallBlock::Other(b) => join_searchable([
                Some(b.name.clone()),
                Some(b.summary.clone()),
                b.output.clone(),
                b.error.clone(),
            ]),
            ToolCallBlock::Lifecycle(b) => join_searchable([Some(b.name.clone())]),
        }
    }

    /// Verb-group kind; `None` renders standalone and splits verb-group runs (still dense-packs via `is_groupable`).
    pub fn verb_group_kind(&self) -> Option<VerbGroupKind> {
        match self {
            ToolCallBlock::Read(b) => Some(if b.is_skill_read() {
                VerbGroupKind::Skill
            } else {
                VerbGroupKind::File
            }),
            ToolCallBlock::ListDir(_) => Some(VerbGroupKind::Dir),
            ToolCallBlock::Search(_) => Some(VerbGroupKind::Search),
            ToolCallBlock::WebFetch(_) => Some(VerbGroupKind::WebFetch),
            ToolCallBlock::WebSearch(_) => Some(VerbGroupKind::WebSearch),
            ToolCallBlock::IntegrationSearch(_) => Some(VerbGroupKind::IntegrationSearch),
            ToolCallBlock::MemorySearch(_) => Some(VerbGroupKind::MemorySearch),
            ToolCallBlock::Skill(_) => Some(VerbGroupKind::Skill),
            ToolCallBlock::Execute(_)
            | ToolCallBlock::Edit(_)
            | ToolCallBlock::UseTool(_)
            | ToolCallBlock::SentMessage(_)
            | ToolCallBlock::Other(_)
            | ToolCallBlock::Lifecycle(_) => None,
        }
    }

    /// The bucket a row falls into for aggregated header LABELS; a superset of [`Self::verb_group_kind`].
    /// The action kinds excluded from eager verb folding still get a bucket when a truncation header describes the rows it hides.
    /// `None` is returned only for lifecycle rows, which are never worth labeling.
    /// Variants are listed explicitly so a new `ToolCallBlock` variant must decide here too.
    pub fn label_kind(&self) -> Option<VerbGroupKind> {
        match self {
            ToolCallBlock::Execute(_) => Some(VerbGroupKind::Command),
            ToolCallBlock::Edit(_) => Some(VerbGroupKind::EditFile),
            ToolCallBlock::UseTool(_) => Some(VerbGroupKind::McpCall),
            ToolCallBlock::SentMessage(_) => Some(VerbGroupKind::Message),
            ToolCallBlock::Other(_) => Some(VerbGroupKind::OtherTool),
            ToolCallBlock::Lifecycle(_) => None,
            ToolCallBlock::Read(_)
            | ToolCallBlock::ListDir(_)
            | ToolCallBlock::Search(_)
            | ToolCallBlock::WebFetch(_)
            | ToolCallBlock::WebSearch(_)
            | ToolCallBlock::IntegrationSearch(_)
            | ToolCallBlock::MemorySearch(_)
            | ToolCallBlock::Skill(_) => self.verb_group_kind(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verb_is_tense_aware() {
        assert_eq!(VerbGroupKind::File.verb(false), "读取");
        assert_eq!(VerbGroupKind::File.verb(true), "读取中");
        assert_eq!(VerbGroupKind::Skill.verb(false), "读取");
        assert_eq!(VerbGroupKind::Search.verb(false), "搜索");
        assert_eq!(VerbGroupKind::Search.verb(true), "搜索中");
        assert_eq!(VerbGroupKind::Dir.verb(false), "列出");
        assert_eq!(VerbGroupKind::Dir.verb(true), "列出中");
        assert_eq!(VerbGroupKind::WebFetch.verb(false), "抓取");
        assert_eq!(VerbGroupKind::WebFetch.verb(true), "抓取中");
        assert_eq!(VerbGroupKind::WebSearch.verb(false), "搜索");
        assert_eq!(VerbGroupKind::MemorySearch.verb(false), "搜索");
        assert_eq!(VerbGroupKind::IntegrationSearch.verb(true), "搜索中");
        assert_eq!(VerbGroupKind::Subagent.verb(false), "运行");
        assert_eq!(VerbGroupKind::Subagent.verb(true), "运行中");
        assert_eq!(VerbGroupKind::Command.verb(false), "运行");
        assert_eq!(VerbGroupKind::Command.verb(true), "运行中");
        assert_eq!(VerbGroupKind::EditFile.verb(false), "编辑");
        assert_eq!(VerbGroupKind::EditFile.verb(true), "编辑中");
        assert_eq!(VerbGroupKind::McpCall.verb(false), "调用");
        assert_eq!(VerbGroupKind::McpCall.verb(true), "调用中");
        assert_eq!(VerbGroupKind::OtherTool.verb(false), "运行");
        assert_eq!(VerbGroupKind::Message.verb(false), "发送");
        assert_eq!(VerbGroupKind::Message.verb(true), "发送中");
    }

    #[test]
    fn noun_pluralizes_by_count() {
        // Chinese measure-word forms; singular/plural share the same text.
        assert_eq!(VerbGroupKind::File.noun(1), "个文件");
        assert_eq!(VerbGroupKind::File.noun(2), "个文件");
        assert_eq!(VerbGroupKind::Skill.noun(2), "个技能");
        assert_eq!(VerbGroupKind::Search.noun(1), "个模式");
        assert_eq!(VerbGroupKind::Dir.noun(2), "个目录");
        assert_eq!(VerbGroupKind::WebFetch.noun(1), "个网站");
        assert_eq!(VerbGroupKind::WebSearch.noun(2), "个网站");
        assert_eq!(VerbGroupKind::MemorySearch.noun(1), "条记忆");
        assert_eq!(VerbGroupKind::MemorySearch.noun(2), "条记忆");
        assert_eq!(VerbGroupKind::IntegrationSearch.noun(1), "个 MCP 工具");
        assert_eq!(VerbGroupKind::IntegrationSearch.noun(2), "个 MCP 工具");
        assert_eq!(VerbGroupKind::Subagent.noun(1), "个子代理");
        assert_eq!(VerbGroupKind::Subagent.noun(2), "个子代理");
        assert_eq!(VerbGroupKind::Command.noun(1), "个命令");
        assert_eq!(VerbGroupKind::Command.noun(2), "个命令");
        assert_eq!(VerbGroupKind::EditFile.noun(2), "个文件");
        assert_eq!(VerbGroupKind::McpCall.noun(1), "个 MCP 工具");
        assert_eq!(VerbGroupKind::OtherTool.noun(1), "个工具");
        assert_eq!(VerbGroupKind::OtherTool.noun(2), "个工具");
        assert_eq!(VerbGroupKind::Message.noun(1), "条消息");
        assert_eq!(VerbGroupKind::Message.noun(2), "条消息");
    }

    #[test]
    fn every_variant_has_a_group_decision() {
        let blocks = [
            ToolCallBlock::Execute(ExecuteToolCallBlock::new("ls")),
            ToolCallBlock::Read(ReadToolCallBlock::new("src/main.rs")),
            ToolCallBlock::Read(ReadToolCallBlock::new("/x/skills/deploy/SKILL.md")),
            ToolCallBlock::Edit(EditToolCallBlock::new("src/main.rs", Vec::new())),
            ToolCallBlock::ListDir(ListDirToolCallBlock::new("src")),
            ToolCallBlock::Search(SearchToolCallBlock::new("todo")),
            ToolCallBlock::WebFetch(WebFetchToolCallBlock::new("https://example.com")),
            ToolCallBlock::WebSearch(WebSearchToolCallBlock::new("grok")),
            ToolCallBlock::IntegrationSearch(IntegrationSearchToolCallBlock::new("linear")),
            ToolCallBlock::UseTool(UseToolCallBlock::new("linear__save_issue")),
            ToolCallBlock::MemorySearch(MemorySearchToolCallBlock::new("auth")),
            ToolCallBlock::SentMessage(SentMessageToolCallBlock::new(
                SentMessagePresentation::Sent,
                Some("sub-123".into()),
                Some("hello".into()),
            )),
            ToolCallBlock::Skill(OtherToolCallBlock::new("Skill", "deploy")),
            ToolCallBlock::Other(OtherToolCallBlock::new("todo_write", "update")),
            ToolCallBlock::Lifecycle(LifecycleEventBlock::new("session_start")),
        ];
        for block in &blocks {
            // Exhaustive on purpose: a new variant fails compilation here until it gets an explicit verb-grouping decision
            let expected = match block {
                ToolCallBlock::Read(b) if b.is_skill_read() => Some(VerbGroupKind::Skill),
                ToolCallBlock::Read(_) => Some(VerbGroupKind::File),
                ToolCallBlock::ListDir(_) => Some(VerbGroupKind::Dir),
                ToolCallBlock::Search(_) => Some(VerbGroupKind::Search),
                ToolCallBlock::WebFetch(_) => Some(VerbGroupKind::WebFetch),
                ToolCallBlock::WebSearch(_) => Some(VerbGroupKind::WebSearch),
                ToolCallBlock::IntegrationSearch(_) => Some(VerbGroupKind::IntegrationSearch),
                ToolCallBlock::MemorySearch(_) => Some(VerbGroupKind::MemorySearch),
                ToolCallBlock::Skill(_) => Some(VerbGroupKind::Skill),
                ToolCallBlock::Execute(_)
                | ToolCallBlock::Edit(_)
                | ToolCallBlock::UseTool(_)
                | ToolCallBlock::SentMessage(_)
                | ToolCallBlock::Other(_)
                | ToolCallBlock::Lifecycle(_) => None,
            };
            assert_eq!(block.verb_group_kind(), expected, "block: {block:?}");
        }
    }

    #[test]
    fn label_kind_extends_verb_kinds_to_action_tools() {
        assert_eq!(
            ToolCallBlock::Execute(ExecuteToolCallBlock::new("ls")).label_kind(),
            Some(VerbGroupKind::Command)
        );
        assert_eq!(
            ToolCallBlock::Edit(EditToolCallBlock::new("src/main.rs", Vec::new())).label_kind(),
            Some(VerbGroupKind::EditFile)
        );
        assert_eq!(
            ToolCallBlock::UseTool(UseToolCallBlock::new("linear__save_issue")).label_kind(),
            Some(VerbGroupKind::McpCall)
        );
        assert_eq!(
            ToolCallBlock::SentMessage(SentMessageToolCallBlock::new(
                SentMessagePresentation::Sent,
                None,
                None,
            ))
            .label_kind(),
            Some(VerbGroupKind::Message)
        );
        assert_eq!(
            ToolCallBlock::Other(OtherToolCallBlock::new("todo_write", "update")).label_kind(),
            Some(VerbGroupKind::OtherTool)
        );
        assert_eq!(
            ToolCallBlock::Lifecycle(LifecycleEventBlock::new("session_start")).label_kind(),
            None
        );
        // Verb-groupable kinds defer to the fold's own classification.
        assert_eq!(
            ToolCallBlock::Read(ReadToolCallBlock::new("src/main.rs")).label_kind(),
            Some(VerbGroupKind::File)
        );
        assert_eq!(
            ToolCallBlock::Read(ReadToolCallBlock::new("/x/skills/deploy/SKILL.md")).label_kind(),
            Some(VerbGroupKind::Skill)
        );
    }
}
