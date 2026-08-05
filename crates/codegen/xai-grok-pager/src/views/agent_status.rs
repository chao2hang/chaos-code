//! Agent status bar — composable right-aligned status items with separators.
//!
//! Provides [`AgentStatusBar`] which collects items as `Line<'static>` spans,
//! lays them out right-aligned with dim `│` separators, and renders into a
//! buffer row.  Returns hit-test areas keyed by item ID.
//!
//! # Example
//!
//! ```ignore
//! let mut status = AgentStatusBar::new(&theme);
//! status.push("context", context_line);
//! status.push("badge", badge_line);
//! let areas = status.render(buf, status_bar_rect);
//! let context_area = areas.get("context");
//! ```

use std::collections::HashMap;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use super::context_bar::SEPARATOR;
use super::turn_status::SPINNER_DIVISOR;
use crate::app::agent::{GoalDisplayPhase, GoalDisplayState, GoalDisplayStatus};
use crate::app::agent_view::McpInitProgress;
use crate::theme::Theme;

/// A named status bar item.
struct StatusEntry {
    /// Identifier for hit-test lookup (e.g., "context", "badge").
    id: &'static str,
    /// Pre-built styled content.
    line: Line<'static>,
    /// Display width in columns.
    width: u16,
}

/// Builder for the agent status bar.
///
/// Collect items with [`push`], then call [`render`] to lay them out
/// right-aligned with separators and get back hit-test areas.
pub struct AgentStatusBar<'a> {
    items: Vec<StatusEntry>,
    theme: &'a Theme,
    /// Padding from the right edge of the status bar area.
    right_pad: u16,
}

impl<'a> AgentStatusBar<'a> {
    /// Create a new empty status bar.
    pub fn new(theme: &'a Theme) -> Self {
        Self {
            items: Vec::new(),
            theme,
            right_pad: 0,
        }
    }

    /// Add an item to the status bar.
    ///
    /// Items are rendered left-to-right in push order, but the entire
    /// group is right-aligned within the status bar area.
    pub fn push(&mut self, id: &'static str, line: Line<'static>) {
        let width = line.width() as u16;
        self.items.push(StatusEntry { id, line, width });
    }

    /// Build a separator span: ` │ ` in dim color.
    fn separator(&self) -> Span<'static> {
        Span::styled(
            format!(" {SEPARATOR} "),
            Style::default()
                .fg(self.theme.gray_dim)
                .bg(self.theme.bg_base),
        )
    }

    /// Render all items right-aligned into the given area.
    ///
    /// Layout: `··· item0 │ item1 │ item2` — separators appear only *between*
    /// items, never before the first or after the last.
    ///
    /// Returns a map of item ID → screen `Rect` for hit-testing.
    pub fn render(self, buf: &mut Buffer, area: Rect) -> HashMap<&'static str, Rect> {
        if area.height == 0 || area.width == 0 || self.items.is_empty() {
            return HashMap::new();
        }

        // Fill background
        buf.set_style(area, Style::default().bg(self.theme.bg_base));

        let sep = self.separator();
        let sep_w = sep.width() as u16; // 3

        // Total width: items plus the separators *between* them only — no
        // leading separator before the first item or trailing one after the
        // last.
        let items_width: u16 = self.items.iter().map(|e| e.width).sum();
        let num_seps = (self.items.len() as u16).saturating_sub(1);
        let total_width = items_width + num_seps * sep_w;

        // Right-align: compute starting x
        let start_x = area
            .x
            .saturating_add(area.width.saturating_sub(self.right_pad + total_width));

        let mut x = start_x;
        let mut areas = HashMap::new();

        for (i, entry) in self.items.iter().enumerate() {
            // Separator before every item except the first.
            if i > 0 {
                buf.set_span(x, area.y, &sep, sep_w);
                x += sep_w;
            }

            // Render item
            buf.set_line(x, area.y, &entry.line, entry.width);
            areas.insert(
                entry.id,
                Rect {
                    x,
                    y: area.y,
                    width: entry.width,
                    height: 1,
                },
            );
            x += entry.width;
        }

        areas
    }
}

// ---------------------------------------------------------------------------
// Goal status line
// ---------------------------------------------------------------------------

/// Format a token count compactly: `500`, `1.5k`, `50k`, `1.5M`.
pub(crate) fn format_tokens_compact(tokens: i64) -> String {
    let sign = if tokens < 0 { "-" } else { "" };
    let abs = tokens.unsigned_abs();
    if abs >= 1_000_000 {
        let m = abs as f64 / 1_000_000.0;
        format!("{sign}{}", format!("{m:.1}M").replace(".0M", "M"))
    } else if abs >= 1_000 {
        let k = abs as f64 / 1_000.0;
        format!("{sign}{}", format!("{k:.1}k").replace(".0k", "k"))
    } else {
        tokens.to_string()
    }
}

/// Format elapsed milliseconds compactly: `5s`, `3m`, `2h`.
fn format_elapsed_compact(ms: u64) -> String {
    let secs = ms / 1000;
    if secs >= 3600 {
        format!("{}h", secs / 3600)
    } else if secs >= 60 {
        format!("{}m", secs / 60)
    } else {
        format!("{}s", secs)
    }
}

/// Build the status-chip label. Paused variants render their
/// `pause_label()`, Budget → "Budget", Done → "Done"; an Active goal
/// uses the shared [`active_phase_label`] suffix.
fn goal_phase_label(goal: &GoalDisplayState) -> String {
    match goal.status {
        GoalDisplayStatus::UserPaused
        | GoalDisplayStatus::BackOffPaused
        | GoalDisplayStatus::NoProgressPaused
        | GoalDisplayStatus::InfraPaused
        | GoalDisplayStatus::Blocked => goal.status.pause_label().into(),
        GoalDisplayStatus::Failed => "失败".into(),
        GoalDisplayStatus::Interrupted => "已中断".into(),
        GoalDisplayStatus::BudgetLimited => "预算".into(),
        GoalDisplayStatus::Complete => "完成".into(),
        GoalDisplayStatus::Active => active_phase_label(goal),
    }
}

/// Live phase suffix for an Active goal — the single source of truth
/// shared by the status chip and the goal-detail modal so they cannot
/// disagree. The transient `verifying_completion` overlay wins, then
/// `planning`, then the steady-state phase.
pub fn active_phase_label(goal: &GoalDisplayState) -> String {
    if goal.verifying_completion {
        let attempts = classifier_attempts_label(goal);
        // Omit the "(n/m)" suffix until the first counter arrives so the
        // chip reads "校验中" instead of a confusing "校验中 (0/0)".
        return if attempts.is_empty() {
            "校验中".into()
        } else {
            format!("校验中（{attempts}）")
        };
    }
    if goal.planning {
        return "规划中".into();
    }
    match goal.phase {
        GoalDisplayPhase::Idle => "空闲".into(),
        GoalDisplayPhase::Planning => "规划中".into(),
        GoalDisplayPhase::Executing => "执行中".into(),
    }
}

/// Format the classifier "attempts: n/m" counter for both the
/// status chip and the modal so the two displays cannot drift.
/// Returns the empty string when both fields are absent / zero — no
/// classifier run has been reserved yet, so there is no meaningful
/// counter. Callers render it only when non-empty: the chip drops the
/// `(n/m)` suffix, the modal falls back to an em-dash.
pub fn classifier_attempts_label(goal: &GoalDisplayState) -> String {
    let attempt = goal.classifier_runs_attempted.unwrap_or(0);
    let max = goal.classifier_max_runs.unwrap_or(0);
    if attempt == 0 && max == 0 {
        return String::new();
    }
    format!("{attempt}/{max}")
}

/// 状态栏 tok/s 芯片：给 `ContextInfo` 里已经带出来的两个速率口径拼一行紧凑显示。
///
/// 展示优先级：
/// 1. **实时流式速率**：来自 tracker 的 `LiveStreamingRate`，使用
///    `cl100k_base` BPE 计数并按 chunk 间隔计算 EMA；有正数样本时使用
///    [`speed_tier`] 的速度档位表情；
/// 2. **对话平均输出速率**：实时样本尚未形成、静默衰减为 0 或 turn 已结束时，
///    回退到 `ContextInfo.avg_output_tokens_per_sec`，以 `📊 平均` 明确区分；
/// 3. **稳态解码速率**：旧 agent 尚未提供对话平均值时，才使用
///    `ContextInfo.decode_tokens_per_sec` 作为兼容兜底，同样按平均态显示。
/// 4. **回合平均速率**：以上都没有、但当前 turn 已运行 ≥2 秒并产出过 token
///    （思考 / 工具 / 委派阶段 live 静默）时，用本回合累计均值兜底，让右上角
///    chip 在 turn 运行期间保持常驻，而不是凭空消失。
///
/// 所有速率都缺失时才返回 `None`。不再显示与 context bar 重复的剩余百分比。
pub fn tokens_per_sec_line(
    ctx: &xai_grok_shell::session::ContextInfo,
    streaming: Option<crate::acp::tracker::LiveStreamingRate>,
    theme: &Theme,
) -> Option<Line<'static>> {
    let live_tps = streaming
        .map(|rate| rate.tokens_per_sec())
        .filter(|tps| tps.is_finite() && *tps > 0.0);
    let average_tps = ctx
        .avg_output_tokens_per_sec
        .filter(|tps| tps.is_finite() && *tps > 0.0)
        .or_else(|| {
            ctx.decode_tokens_per_sec
                .filter(|tps| tps.is_finite() && *tps > 0.0)
        });
    // 回合累计平均：live 静默但本回合已产出 token 时兜底。≥2s 门限避免
    // 首个 chunk 之后（elapsed 极小）把「total/elapsed」放大成尖峰。
    let turn_mean_tps = streaming
        .filter(|rate| rate.started_at.elapsed() >= std::time::Duration::from_secs(2))
        .map(|rate| rate.mean_tokens_per_sec())
        .filter(|tps| tps.is_finite() && *tps > 0.0);

    let (tps, emoji, label, color) = if let Some(tps) = live_tps {
        let (emoji, color) = speed_tier(tps, theme);
        (tps, emoji, "", color)
    } else if let Some(tps) = average_tps {
        (tps, "📊", "平均 ", theme.gray)
    } else if let Some(tps) = turn_mean_tps {
        (tps, "📊", "回合均 ", theme.gray)
    } else {
        return None;
    };
    let rendered = if f64::from(tps) < 1.0 {
        format!("{emoji} {label}{tps:.1} tok/s")
    } else {
        format!("{emoji} {label}{} tok/s", f64::from(tps).round() as u64)
    };
    let style = Style::default().fg(color).bg(theme.bg_base);
    Some(Line::from(Span::styled(rendered, style)))
}

/// 把一个 token/s 速率映射到 7 档 (emoji, fg color) 中的某一档。
///
/// 阈值采用「保守（低阈）」档位 — 对慢速本地模型更宽容；
/// 从 `< 2` 的 🐢 开始，逐档提升到 `≥ 150` 的 🛸。
///
/// | tok/s 范围 | emoji | 含义      | 颜色           |
/// |------------|-------|-----------|----------------|
/// | `< 2`      | 🐢    | 龟速      | `accent_error` |
/// | `< 5`      | 🚲    | 自行车    | `accent_error` |
/// | `< 15`     | 🚗    | 汽车      | `warning`      |
/// | `< 40`     | 🚂    | 火车      | `warning`      |
/// | `< 80`     | ✈️    | 飞机      | `command`      |
/// | `< 150`    | 🚀    | 火箭      | `accent_success` |
/// | `≥ 150`    | 🛸    | 飞船      | `accent_success` |
///
/// 调用方需要保证 `tps` 已经是有限正数（[`tokens_per_sec_line`] 已经过滤过）。
pub fn speed_tier(tps: f32, theme: &Theme) -> (&'static str, Color) {
    let t = f64::from(tps);
    if t < 2.0 {
        ("🐢", theme.accent_error)
    } else if t < 5.0 {
        ("🚲", theme.accent_error)
    } else if t < 15.0 {
        ("🚗", theme.warning)
    } else if t < 40.0 {
        ("🚂", theme.warning)
    } else if t < 80.0 {
        ("✈️", theme.command)
    } else if t < 150.0 {
        ("🚀", theme.accent_success)
    } else {
        ("🛸", theme.accent_success)
    }
}

/// Build a compact goal status `Line` for the agent status bar.
///
/// Format: `[Goal: {label}]  {tokens}  {elapsed}`
///
/// When `hovered` is true the label is bolded/underlined to signal
/// clickability.  When the goal is `Active`, a braille spinner driven
/// by `tick` is prepended.
pub fn goal_status_line(
    goal: &GoalDisplayState,
    theme: &Theme,
    hovered: bool,
    tick: usize,
    context_used: Option<u64>,
    active_subagent_tokens: u64,
) -> Line<'static> {
    let label = goal_phase_label(goal);

    let tokens_str =
        format_tokens_compact(goal.live_tokens_used(context_used, active_subagent_tokens));
    let tokens_display = match goal.token_budget {
        Some(budget) if budget > 0 => {
            format!("{}/{} tokens", tokens_str, format_tokens_compact(budget))
        }
        _ => format!("{} tokens", tokens_str),
    };

    let elapsed_str = format_elapsed_compact(goal.live_elapsed_ms());

    let dim_style = Style::default().fg(theme.gray_dim).bg(theme.bg_base);
    // Paused goals use an inverted warning-colour chip so the chip background
    // visually matches the modal's `theme.warning` status row.
    let mut label_style = if goal.status.is_paused() {
        Style::default().fg(theme.bg_base).bg(theme.warning)
    } else if matches!(
        goal.status,
        GoalDisplayStatus::Failed | GoalDisplayStatus::Interrupted
    ) {
        Style::default().fg(theme.bg_base).bg(theme.accent_error)
    } else {
        Style::default().fg(theme.accent_plan).bg(theme.bg_base)
    };

    if hovered {
        label_style = label_style
            .add_modifier(ratatui::style::Modifier::BOLD)
            .add_modifier(ratatui::style::Modifier::UNDERLINED);
    }

    let is_active = matches!(goal.status, GoalDisplayStatus::Active);

    let chip_name = "目标";
    let goal_text = if is_active {
        let frames = crate::glyphs::dot_spinner_frames();
        let frame = frames[(tick / 4) % frames.len()];
        format!("{frame} {chip_name}：{label}")
    } else {
        format!("{chip_name}：{label}")
    };

    Line::from(vec![
        Span::styled("[", dim_style),
        Span::styled(goal_text, label_style),
        Span::styled("]", dim_style),
        Span::styled(format!("  {tokens_display}  {elapsed_str}"), dim_style),
    ])
}

// ---------------------------------------------------------------------------
// MCP connecting indicator
// ---------------------------------------------------------------------------

/// Build a compact accumulated-token chip for the agent status bar.
///
/// Format: `Token 12.5k` — the label is fixed-width and the count uses the
/// same compact formatter as the context bar so the chip stays small in the
/// already-crowded top-right corner.
///
/// The chip is clickable (opens the token-usage detail overlay), so `hovered`
/// brightens it the same way the other clickable chips signal affordance.
pub fn total_tokens_line(total_tokens: u64, hovered: bool, theme: &Theme) -> Line<'static> {
    let style = if hovered {
        Style::default()
            .fg(theme.text_primary)
            .bg(theme.bg_base)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.gray_dim).bg(theme.bg_base)
    };
    Line::from(vec![
        Span::styled("Token ", style),
        Span::styled(crate::views::context_bar::fmt_tokens(total_tokens), style),
    ])
}

/// Build the compact MCP-connecting indicator for the agent status bar.
///
/// Format: `⠋ MCP (1/4)` — a braille spinner (driven by `tick`, same cadence as
/// the turn-status spinner) followed by the connected/total server count.
/// Rendered in `theme.gray_dim` so it reads as dim, matching the directory path
/// shown on the same row.
///
/// Returns `None` while `progress.total == 0` (a startup seed). That state
/// renders `⠋ Starting session…` above the prompt (see
/// [`crate::views::turn_status`]) rather than as a chip here — the top-bar chip
/// only shows real server counts once the shell reports `total > 0`.
pub fn mcp_status_line(
    progress: &McpInitProgress,
    tick: u64,
    theme: &Theme,
) -> Option<Line<'static>> {
    if progress.total == 0 {
        return None;
    }
    let frames = crate::glyphs::braille_spinner_frames();
    let frame_idx = (tick / SPINNER_DIVISOR) as usize % frames.len();
    let style = Style::default().fg(theme.gray_dim).bg(theme.bg_base);
    Some(Line::from(vec![
        Span::styled(format!("{} ", frames[frame_idx]), style),
        Span::styled(
            format!("MCP ({}/{})", progress.connected, progress.total),
            style,
        ),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_compact_sub_thousand() {
        assert_eq!(format_tokens_compact(0), "0");
        assert_eq!(format_tokens_compact(500), "500");
        assert_eq!(format_tokens_compact(999), "999");
    }

    #[test]
    fn tokens_compact_thousands() {
        assert_eq!(format_tokens_compact(1000), "1k");
        assert_eq!(format_tokens_compact(1500), "1.5k");
        assert_eq!(format_tokens_compact(12300), "12.3k");
        assert_eq!(format_tokens_compact(50000), "50k");
        assert_eq!(format_tokens_compact(100000), "100k");
    }

    #[test]
    fn tokens_compact_negative() {
        assert_eq!(format_tokens_compact(-500), "-500");
        assert_eq!(format_tokens_compact(-1500), "-1.5k");
        assert_eq!(format_tokens_compact(-1_000_000), "-1M");
    }

    #[test]
    fn tokens_compact_millions() {
        assert_eq!(format_tokens_compact(1_000_000), "1M");
        assert_eq!(format_tokens_compact(1_500_000), "1.5M");
        assert_eq!(format_tokens_compact(10_000_000), "10M");
    }

    #[test]
    fn elapsed_compact_seconds() {
        assert_eq!(format_elapsed_compact(0), "0s");
        assert_eq!(format_elapsed_compact(5_000), "5s");
        assert_eq!(format_elapsed_compact(59_999), "59s");
    }

    #[test]
    fn elapsed_compact_minutes() {
        assert_eq!(format_elapsed_compact(60_000), "1m");
        assert_eq!(format_elapsed_compact(180_000), "3m");
        assert_eq!(format_elapsed_compact(3_599_000), "59m");
    }

    #[test]
    fn elapsed_compact_hours() {
        assert_eq!(format_elapsed_compact(3_600_000), "1h");
        assert_eq!(format_elapsed_compact(7_200_000), "2h");
    }

    fn make_goal(
        status: GoalDisplayStatus,
        phase: GoalDisplayPhase,
        idx: Option<u32>,
        total: u32,
        completed: u32,
    ) -> GoalDisplayState {
        GoalDisplayState {
            goal_id: "g-1".into(),
            objective: "Build widget".into(),
            status,
            phase,
            token_budget: Some(50_000),
            tokens_used: 12_300,
            elapsed_ms: 180_000,
            total_deliverables: total,
            completed_deliverables: completed,
            current_deliverable_id: idx,
            current_deliverable_title: Some("Add CSS vars".into()),
            current_subagent_role: None,
            total_worker_rounds: 3,
            total_verify_rounds: 1,
            live_subagent_tokens: None,
            live_tokens_by_model: Vec::new(),
            live_context_pct: None,
            live_turn_count: None,
            live_tool_call_count: None,
            last_event: None,
            last_event_detail: None,
            last_event_timestamp: None,
            token_baseline: 0,
            finished_subagent_tokens: 0,
            deliverables: vec![],
            pause_message: None,
            classifier_runs_attempted: None,
            classifier_max_runs: None,
            last_classifier_verdict: None,
            last_classifier_details_path: None,
            last_classifier_details_exists: false,
            verifying_completion: false,
            planning: false,
            received_at: std::time::Instant::now(),
            elapsed_floor_ms: 0,
        }
    }

    #[test]
    fn phase_label_active_executing() {
        let g = make_goal(
            GoalDisplayStatus::Active,
            GoalDisplayPhase::Executing,
            None,
            0,
            0,
        );
        assert_eq!(goal_phase_label(&g), "执行中");
    }

    #[test]
    fn phase_label_active_planning() {
        let g = make_goal(
            GoalDisplayStatus::Active,
            GoalDisplayPhase::Planning,
            None,
            0,
            0,
        );
        assert_eq!(goal_phase_label(&g), "规划中");
    }

    #[test]
    fn phase_label_paused_variants() {
        for (status, expected) in [
            (GoalDisplayStatus::UserPaused, "已暂停"),
            (GoalDisplayStatus::BackOffPaused, "已暂停（退避中）"),
            (GoalDisplayStatus::NoProgressPaused, "已暂停（无进展）"),
            (GoalDisplayStatus::InfraPaused, "已暂停（错误）"),
            (GoalDisplayStatus::Blocked, "已暂停（校验受阻）"),
        ] {
            let g = make_goal(status, GoalDisplayPhase::Executing, Some(0), 2, 0);
            assert_eq!(goal_phase_label(&g), expected, "for {status:?}");
        }
    }

    #[test]
    fn goal_line_contains_pause_label_for_infra_paused() {
        let g = make_goal(
            GoalDisplayStatus::InfraPaused,
            GoalDisplayPhase::Executing,
            Some(1),
            2,
            0,
        );
        let t = Theme::current();
        let line = goal_status_line(&g, &t, false, 0, None, 0);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("已暂停（错误）"));
    }

    #[test]
    fn status_chip_shows_verifying_completion_when_flag_set() {
        // Status-chip behaviour: an Active goal with
        // `verifying_completion = true` renders the "Verifying (n/m)"
        // label instead of the regular phase label so the user can see
        // the classifier run.
        let mut g = make_goal(
            GoalDisplayStatus::Active,
            GoalDisplayPhase::Executing,
            None,
            0,
            0,
        );
        g.verifying_completion = true;
        g.classifier_runs_attempted = Some(2);
        g.classifier_max_runs = Some(3);
        assert_eq!(goal_phase_label(&g), "校验中（2/3）");

        let t = Theme::current();
        let line = goal_status_line(&g, &t, false, 0, None, 0);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("校验中（2/3）"));
    }

    #[test]
    fn status_chip_verifying_omits_counter_when_counts_absent() {
        // Before the first counter arrives (both fields None) the chip reads
        // "Verifying", not "Verifying (0/0)".
        let mut g = make_goal(
            GoalDisplayStatus::Active,
            GoalDisplayPhase::Executing,
            None,
            0,
            0,
        );
        g.verifying_completion = true;
        g.classifier_runs_attempted = None;
        g.classifier_max_runs = None;
        assert_eq!(goal_phase_label(&g), "校验中");
    }

    #[test]
    fn classifier_attempts_label_empty_when_both_absent_or_zero() {
        let mut g = make_goal(
            GoalDisplayStatus::Active,
            GoalDisplayPhase::Executing,
            None,
            0,
            0,
        );
        // Both None → empty (no run reserved yet).
        assert_eq!(classifier_attempts_label(&g), "");
        // Explicit zeros also count as "no counter".
        g.classifier_runs_attempted = Some(0);
        g.classifier_max_runs = Some(0);
        assert_eq!(classifier_attempts_label(&g), "");
        // A configured cap (max > 0) makes the counter meaningful.
        g.classifier_max_runs = Some(3);
        assert_eq!(classifier_attempts_label(&g), "0/3");
        g.classifier_runs_attempted = Some(2);
        assert_eq!(classifier_attempts_label(&g), "2/3");
    }

    #[test]
    fn live_elapsed_ms_clamps_to_carried_floor() {
        // The displayed clock must never tick below the carried monotonic
        // floor, even when the latest authoritative base is lower (the
        // pager's extrapolation outran the shell's flush point).
        let mut g = make_goal(
            GoalDisplayStatus::UserPaused,
            GoalDisplayPhase::Idle,
            None,
            0,
            0,
        );
        g.elapsed_ms = 1_000;
        g.elapsed_floor_ms = 5_000;
        assert_eq!(g.live_elapsed_ms(), 5_000);
    }

    #[test]
    fn live_elapsed_ms_uses_live_value_when_above_floor() {
        let mut g = make_goal(
            GoalDisplayStatus::UserPaused,
            GoalDisplayPhase::Idle,
            None,
            0,
            0,
        );
        g.elapsed_ms = 9_000;
        g.elapsed_floor_ms = 5_000;
        assert_eq!(g.live_elapsed_ms(), 9_000);
    }

    #[test]
    fn status_chip_shows_planning_when_flag_set() {
        // An Active goal with `planning = true` renders the "Planning"
        // label instead of the regular phase label so the user can see
        // the planner subagent run while it executes.
        let mut g = make_goal(
            GoalDisplayStatus::Active,
            GoalDisplayPhase::Idle,
            None,
            0,
            0,
        );
        g.planning = true;
        assert_eq!(goal_phase_label(&g), "规划中");

        let t = Theme::current();
        let line = goal_status_line(&g, &t, false, 0, None, 0);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("规划中"));
    }

    #[test]
    fn status_chip_verifying_wins_over_planning() {
        // Deterministic precedence: the two flags never overlap in
        // practice, but if both were set `verifying_completion` wins.
        let mut g = make_goal(
            GoalDisplayStatus::Active,
            GoalDisplayPhase::Idle,
            None,
            0,
            0,
        );
        g.planning = true;
        g.verifying_completion = true;
        g.classifier_runs_attempted = Some(1);
        g.classifier_max_runs = Some(2);
        assert_eq!(goal_phase_label(&g), "校验中（1/2）");
    }

    #[test]
    fn status_chip_planning_suppressed_on_non_active_status() {
        // The "Planning" label is gated on `Active`: a paused goal that
        // somehow still carries `planning = true` shows its terminal
        // label, not the in-flight one.
        let mut g = make_goal(
            GoalDisplayStatus::UserPaused,
            GoalDisplayPhase::Idle,
            None,
            0,
            0,
        );
        g.planning = true;
        assert_eq!(goal_phase_label(&g), "已暂停");
    }

    #[test]
    fn status_chip_verifying_suppressed_on_non_active_status() {
        // The chip text is gated on `Active`: a paused / complete /
        // budget-limited goal that somehow still carries
        // `verifying_completion = true` must show its terminal label,
        // not the in-flight one.
        let mut g = make_goal(
            GoalDisplayStatus::UserPaused,
            GoalDisplayPhase::Executing,
            None,
            0,
            0,
        );
        g.verifying_completion = true;
        assert_eq!(goal_phase_label(&g), "已暂停");
    }

    #[test]
    fn goal_line_paused_chip_uses_warning_background() {
        // Paused chips render with the
        // `theme.warning` background to visually warn the user. Pin the
        // background colour on the label span so a regression that drops
        // the chip-vs-modal colour alignment gets caught.
        //
        // Use the unquantized `groknight()` theme directly so warning and
        // bg_base remain distinguishable in the test env — `Theme::current()`
        // collapses both to ANSI `Reset` on 16-colour terminals, which
        // would defeat the assertion.
        let g = make_goal(
            GoalDisplayStatus::UserPaused,
            GoalDisplayPhase::Executing,
            Some(0),
            2,
            0,
        );
        let t = Theme::groknight();
        let line = goal_status_line(&g, &t, false, 0, None, 0);
        // The label span is the one whose content starts with "目标：".
        let label_span = line
            .spans
            .iter()
            .find(|s| s.content.contains("目标："))
            .expect("label span with `Goal:` prefix");
        assert_eq!(label_span.style.bg, Some(t.warning));
        assert_ne!(label_span.style.bg, Some(t.bg_base));
    }

    #[test]
    fn goal_line_active_chip_does_not_use_warning_background() {
        // Negative companion: an Active goal must keep the standard
        // accent-plan-on-bg-base chip style.
        let g = make_goal(
            GoalDisplayStatus::Active,
            GoalDisplayPhase::Executing,
            Some(0),
            2,
            0,
        );
        let t = Theme::groknight();
        let line = goal_status_line(&g, &t, false, 0, None, 0);
        let label_span = line
            .spans
            .iter()
            .find(|s| s.content.contains("目标："))
            .expect("label span with `Goal:` prefix");
        assert_eq!(label_span.style.bg, Some(t.bg_base));
        assert_ne!(label_span.style.bg, Some(t.warning));
    }

    #[test]
    fn phase_label_failed() {
        let g = make_goal(
            GoalDisplayStatus::Failed,
            GoalDisplayPhase::Idle,
            None,
            0,
            0,
        );
        assert_eq!(goal_phase_label(&g), "失败");
    }

    #[test]
    fn phase_label_budget_limited() {
        let g = make_goal(
            GoalDisplayStatus::BudgetLimited,
            GoalDisplayPhase::Executing,
            Some(1),
            2,
            0,
        );
        assert_eq!(goal_phase_label(&g), "预算");
    }

    #[test]
    fn phase_label_complete() {
        let g = make_goal(
            GoalDisplayStatus::Complete,
            GoalDisplayPhase::Idle,
            None,
            3,
            3,
        );
        assert_eq!(goal_phase_label(&g), "完成");
    }

    #[test]
    fn phase_label_active_idle() {
        let g = make_goal(
            GoalDisplayStatus::Active,
            GoalDisplayPhase::Idle,
            None,
            0,
            0,
        );
        assert_eq!(goal_phase_label(&g), "空闲");
    }

    #[test]
    fn phase_label_executing_ignores_deliverables() {
        let g = make_goal(
            GoalDisplayStatus::Active,
            GoalDisplayPhase::Executing,
            None,
            4,
            2,
        );
        assert_eq!(goal_phase_label(&g), "执行中");
    }

    // The old deliverable-index parity test is removed because deliverables
    // are no longer part of the simplified goal model.

    #[test]
    fn goal_line_contains_expected_text() {
        let g = make_goal(
            GoalDisplayStatus::Active,
            GoalDisplayPhase::Executing,
            Some(1),
            4,
            1,
        );
        let t = Theme::current();
        let line = goal_status_line(&g, &t, false, 0, None, 0);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("目标：执行"));
        assert!(text.contains("12.3k/50k tokens"));
        assert!(text.contains("3m"));
    }

    #[test]
    fn goal_line_without_budget() {
        let mut g = make_goal(
            GoalDisplayStatus::Active,
            GoalDisplayPhase::Planning,
            None,
            0,
            0,
        );
        g.token_budget = None;
        g.tokens_used = 500;
        let t = Theme::current();
        let line = goal_status_line(&g, &t, false, 0, None, 0);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("500 tokens"));
        assert!(!text.contains('/'));
    }

    #[test]
    fn goal_line_no_title_in_status_bar() {
        let g = make_goal(
            GoalDisplayStatus::Active,
            GoalDisplayPhase::Executing,
            None,
            1,
            0,
        );
        let t = Theme::current();
        let line = goal_status_line(&g, &t, false, 0, None, 0);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        // Title should NOT appear in status bar (only in the modal)
        assert!(!text.contains("Add CSS vars"));
        assert!(!text.contains("Build widget"));
    }

    #[test]
    fn mcp_status_line_renders_compact_count() {
        // total > 0 renders the compact `MCP (connected/total)` chip.
        let progress = McpInitProgress {
            total: 4,
            connected: 1,
            started_at: std::time::Instant::now(),
        };
        let t = Theme::current();
        let line = mcp_status_line(&progress, 0, &t).expect("total > 0 must render a line");
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.contains("MCP (1/4)"),
            "expected 'MCP (1/4)', got: {text:?}"
        );
    }

    #[test]
    fn mcp_status_line_uses_dim_directory_color() {
        // The chip must render in `theme.gray_dim` to match the directory path.
        let t = Theme::groknight();
        let progress = McpInitProgress {
            total: 2,
            connected: 0,
            started_at: std::time::Instant::now(),
        };
        let line = mcp_status_line(&progress, 0, &t).expect("total > 0 must render a line");
        for span in &line.spans {
            assert_eq!(
                span.style.fg,
                Some(t.gray_dim),
                "MCP chip spans must use theme.gray_dim"
            );
        }
    }

    #[test]
    fn mcp_status_line_hidden_for_zero_total() {
        // total == 0 (startup seed) renders nothing in the top bar — that state
        // shows "Starting session…" above the prompt instead.
        let progress = McpInitProgress {
            total: 0,
            connected: 0,
            started_at: std::time::Instant::now(),
        };
        let t = Theme::current();
        assert!(mcp_status_line(&progress, 0, &t).is_none());
    }

    /// Separators appear only *between* items — never before the first item or
    /// after the last (no leading/trailing divider).
    #[test]
    fn status_bar_separators_only_between_items() {
        let theme = Theme::current();
        let mut bar = AgentStatusBar::new(&theme);
        bar.push("a", Line::from("AA"));
        bar.push("b", Line::from("BB"));
        bar.push("c", Line::from("CC"));

        let area = Rect::new(0, 0, 40, 1);
        let mut buf = Buffer::empty(area);
        bar.render(&mut buf, area);

        let row: String = (0..area.width).map(|x| buf[(x, 0)].symbol()).collect();
        let trimmed = row.trim();

        // Exactly two dividers (between the three items), none at the ends.
        assert_eq!(trimmed.matches(SEPARATOR).count(), 2, "row = {trimmed:?}");
        assert!(
            !trimmed.starts_with(SEPARATOR),
            "no leading divider, row = {trimmed:?}"
        );
        assert!(
            !trimmed.ends_with(SEPARATOR),
            "no trailing divider, row = {trimmed:?}"
        );
        assert_eq!(trimmed, format!("AA {SEPARATOR} BB {SEPARATOR} CC"));
    }

    /// A single item renders with no separators at all (it is both first and
    /// last).
    #[test]
    fn status_bar_single_item_has_no_separators() {
        let theme = Theme::current();
        let mut bar = AgentStatusBar::new(&theme);
        bar.push("only", Line::from("XX"));

        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        bar.render(&mut buf, area);

        let row: String = (0..area.width).map(|x| buf[(x, 0)].symbol()).collect();
        assert_eq!(row.trim(), "XX");
        assert!(!row.contains(SEPARATOR));
    }

    #[test]
    fn total_tokens_line_formats_compact_counts() {
        let theme = Theme::current();
        let line = total_tokens_line(12_345, false, &theme);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "Token 12K");

        let line = total_tokens_line(1_234, false, &theme);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "Token 1.2K");
    }

    #[test]
    fn total_tokens_line_uses_dim_style() {
        let theme = Theme::groknight();
        let line = total_tokens_line(500, false, &theme);
        for span in &line.spans {
            assert_eq!(span.style.fg, Some(theme.gray_dim));
            assert_eq!(span.style.bg, Some(theme.bg_base));
        }
    }

    /// The chip is clickable, so hover must signal the affordance — same
    /// brighten-and-bold treatment the other clickable chips use.
    #[test]
    fn total_tokens_line_brightens_on_hover() {
        let theme = Theme::groknight();
        let line = total_tokens_line(500, true, &theme);
        for span in &line.spans {
            assert_eq!(span.style.fg, Some(theme.text_primary));
            assert!(span.style.add_modifier.contains(Modifier::BOLD));
        }
    }

    /// 状态栏 tok/s 芯片：有稳态或平均速率时渲染，缺样本时返回 None，
    /// 保证状态栏不会出现「🐢 0 tok/s」的噪音行。
    #[test]
    fn tokens_per_sec_line_hides_when_absent() {
        let theme = Theme::default();
        let ctx = xai_grok_shell::session::ContextInfo::default();
        assert!(tokens_per_sec_line(&ctx, None, &theme).is_none());
    }

    #[test]
    fn tokens_per_sec_line_prefers_conversation_average_when_idle() {
        let theme = Theme::default();
        let ctx = xai_grok_shell::session::ContextInfo {
            avg_output_tokens_per_sec: Some(100.0),
            decode_tokens_per_sec: Some(240.0),
            ..Default::default()
        };
        let line = tokens_per_sec_line(&ctx, None, &theme).expect("line should exist");
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "📊 平均 100 tok/s");
    }

    #[test]
    fn tokens_per_sec_line_uses_average_emoji_for_fractional_rate() {
        let theme = Theme::default();
        let ctx = xai_grok_shell::session::ContextInfo {
            avg_output_tokens_per_sec: Some(0.6),
            decode_tokens_per_sec: Some(0.0),
            ..Default::default()
        };
        let line = tokens_per_sec_line(&ctx, None, &theme).expect("line should exist");
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "📊 平均 0.6 tok/s");
    }

    #[test]
    fn tokens_per_sec_line_uses_decode_as_legacy_average_fallback() {
        let theme = Theme::default();
        let ctx = xai_grok_shell::session::ContextInfo {
            avg_output_tokens_per_sec: None,
            decode_tokens_per_sec: Some(42.0),
            ..Default::default()
        };
        let line = tokens_per_sec_line(&ctx, None, &theme).expect("line should exist");
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "📊 平均 42 tok/s");
    }

    /// Live streaming rate takes priority over both `decode_tokens_per_sec`
    /// and `avg_output_tokens_per_sec`. We mock the rate by constructing a
    /// `LiveStreamingRate` directly and feeding it in — the chip should
    /// reflect it instead of the stale post-hoc numbers.
    #[test]
    fn tokens_per_sec_line_prefers_live_over_post_hoc() {
        use crate::acp::tracker::LiveStreamingRate;
        use std::time::{Duration, Instant};

        let theme = Theme::default();
        // Post-hoc says 240 tok/s (post-response); live says 50 tok/s.
        // Live should win.
        let ctx = xai_grok_shell::session::ContextInfo {
            avg_output_tokens_per_sec: Some(100.0),
            decode_tokens_per_sec: Some(240.0),
            ..Default::default()
        };
        let started = Instant::now() - Duration::from_millis(200);
        // Live rate is now BPE-token based (tiktoken-rs). With 50 tokens
        // accumulated over 200ms the rate is 250 tok/s — rounds to 250
        // (≥ 150 → 🛸).
        let live = LiveStreamingRate {
            started_at: started,
            total_tokens: 50,
            last_event_at: Some(started),
            smoothed_rate: 250.0,
            rate_window_started_at: started,
            rate_window_tokens: 0,
        };
        let line = tokens_per_sec_line(&ctx, Some(live), &theme).expect("line should exist");
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("tok/s"), "expected tok/s suffix in {text:?}");
        assert!(
            !text.contains("240 tok/s"),
            "post-hoc rate must be hidden when live is present: {text:?}"
        );
    }

    /// When the current live sample has decayed to zero, keep the chip useful by
    /// showing the conversation-wide average with an explicit average marker.
    #[test]
    fn live_zero_falls_back_to_conversation_average() {
        use crate::acp::tracker::LiveStreamingRate;
        use std::time::{Duration, Instant};

        let theme = Theme::default();
        let ctx = xai_grok_shell::session::ContextInfo {
            decode_tokens_per_sec: Some(240.0),
            avg_output_tokens_per_sec: Some(100.0),
            ..Default::default()
        };
        let stale = Instant::now() - Duration::from_secs(2);
        let live = LiveStreamingRate {
            started_at: stale,
            total_tokens: 50,
            last_event_at: Some(stale),
            smoothed_rate: 250.0,
            rate_window_started_at: stale,
            rate_window_tokens: 0,
        };
        let line = tokens_per_sec_line(&ctx, Some(live), &theme)
            .expect("quiet live state should fall back to conversation average");
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "📊 平均 100 tok/s");
    }

    #[test]
    fn tokens_per_sec_line_renders_live_with_default_context() {
        use crate::acp::tracker::LiveStreamingRate;
        use std::time::{Duration, Instant};

        let theme = Theme::default();
        // No model bound, no post-hoc data: ContextInfo::default().
        let ctx = xai_grok_shell::session::ContextInfo::default();
        let started = Instant::now() - Duration::from_millis(100);
        // 20 tokens in 100ms → 200 tok/s.
        let live = LiveStreamingRate {
            started_at: started,
            total_tokens: 20,
            last_event_at: Some(started),
            smoothed_rate: 200.0,
            rate_window_started_at: started,
            rate_window_tokens: 0,
        };
        let line = tokens_per_sec_line(&ctx, Some(live), &theme)
            .expect("live must render even when context is default");
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.contains("200 tok/s"),
            "expected live rate to render: {text:?}"
        );
    }

    /// 没有对话平均数据、live 已静默但本回合产出过 token（思考 / 工具 /
    /// 委派阶段）时，用回合累计均值兜底，让 chip 保持常驻而不是消失。
    #[test]
    fn turn_mean_keeps_chip_resident_without_conversation_average() {
        use crate::acp::tracker::LiveStreamingRate;
        use std::time::{Duration, Instant};

        let theme = Theme::default();
        let ctx = xai_grok_shell::session::ContextInfo::default();
        let started = Instant::now() - Duration::from_secs(5);
        let quiet = Instant::now() - Duration::from_secs(3);
        let live = LiveStreamingRate {
            started_at: started,
            total_tokens: 100,
            last_event_at: Some(quiet),
            smoothed_rate: 250.0,
            rate_window_started_at: started,
            rate_window_tokens: 0,
        };
        // live 静默 3s → tokens_per_sec()==0；无对话平均。
        assert_eq!(live.tokens_per_sec(), 0.0);
        let line = tokens_per_sec_line(&ctx, Some(live), &theme)
            .expect("chip must stay resident via turn mean");
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.contains("回合均"),
            "expected turn-mean marker in {text:?}"
        );
        // 100 tokens / 5s = 20 tok/s
        assert!(text.contains("20 tok/s"), "expected ~20 tok/s in {text:?}");
    }

    // ---- speed_tier 映射表 ----
    //
    // 每档都跑一次确保阈值正确；边界用 `.prev`/`.next` 浮点贴近以防 off-by-one。

    fn tier_for(tps: f32) -> &'static str {
        speed_tier(tps, &Theme::default()).0
    }

    fn color_for(tps: f32) -> Color {
        speed_tier(tps, &Theme::default()).1
    }

    #[test]
    fn speed_tier_turtle_under_two() {
        assert_eq!(tier_for(0.0), "\u{1f422}"); // 守卫在 tokens_per_sec_line 里，不在这里测
        assert_eq!(tier_for(0.1), "\u{1f422}");
        assert_eq!(tier_for(1.0), "\u{1f422}");
        assert_eq!(tier_for(1.99), "\u{1f422}");
    }

    #[test]
    fn speed_tier_bicycle_under_five() {
        assert_eq!(tier_for(2.0), "\u{1f6b2}");
        assert_eq!(tier_for(3.5), "\u{1f6b2}");
        assert_eq!(tier_for(4.99), "\u{1f6b2}");
    }

    #[test]
    fn speed_tier_car_under_fifteen() {
        assert_eq!(tier_for(5.0), "\u{1f697}");
        assert_eq!(tier_for(10.0), "\u{1f697}");
        assert_eq!(tier_for(14.99), "\u{1f697}");
    }

    #[test]
    fn speed_tier_train_under_forty() {
        assert_eq!(tier_for(15.0), "\u{1f682}");
        assert_eq!(tier_for(25.0), "\u{1f682}");
        assert_eq!(tier_for(39.99), "\u{1f682}");
    }

    #[test]
    fn speed_tier_airplane_under_eighty() {
        assert_eq!(tier_for(40.0), "\u{2708}\u{fe0f}");
        assert_eq!(tier_for(60.0), "\u{2708}\u{fe0f}");
        assert_eq!(tier_for(79.99), "\u{2708}\u{fe0f}");
    }

    #[test]
    fn speed_tier_rocket_under_one_fifty() {
        assert_eq!(tier_for(80.0), "\u{1f680}");
        assert_eq!(tier_for(100.0), "\u{1f680}");
        assert_eq!(tier_for(149.99), "\u{1f680}");
    }

    #[test]
    fn speed_tier_spaceship_at_or_above_one_fifty() {
        assert_eq!(tier_for(150.0), "\u{1f6f8}");
        assert_eq!(tier_for(240.0), "\u{1f6f8}");
        assert_eq!(tier_for(1_000.0), "\u{1f6f8}");
        assert_eq!(tier_for(f32::INFINITY), "\u{1f6f8}");
    }

    #[test]
    fn speed_tier_colors_go_red_to_green() {
        let theme = Theme::default();
        // 慢速 → accent_error (red)
        assert_eq!(color_for(0.5), theme.accent_error);
        assert_eq!(color_for(3.0), theme.accent_error);
        // 中速 → warning (orange/amber)
        assert_eq!(color_for(10.0), theme.warning);
        assert_eq!(color_for(25.0), theme.warning);
        // 中高速 → command (yellow)
        assert_eq!(color_for(60.0), theme.command);
        // 高速 → accent_success (green)
        assert_eq!(color_for(100.0), theme.accent_success);
        assert_eq!(color_for(500.0), theme.accent_success);
    }

    /// 跨档位显示的完整 live Line：真实速率使用对应速度档位表情。
    #[test]
    fn tokens_per_sec_line_live_emoji_matches_tier() {
        use crate::acp::tracker::LiveStreamingRate;
        use std::time::Instant;

        let theme = Theme::default();
        for (tps, expected_emoji) in [
            (0.5_f32, "\u{1f422}"),
            (3.0, "\u{1f6b2}"),
            (10.0, "\u{1f697}"),
            (25.0, "\u{1f682}"),
            (60.0, "\u{2708}\u{fe0f}"),
            (100.0, "\u{1f680}"),
            (240.0, "\u{1f6f8}"),
        ] {
            let now = Instant::now();
            let live = LiveStreamingRate {
                started_at: now,
                total_tokens: 1,
                last_event_at: Some(now),
                smoothed_rate: tps,
                rate_window_started_at: now,
                rate_window_tokens: 0,
            };
            let line = tokens_per_sec_line(
                &xai_grok_shell::session::ContextInfo::default(),
                Some(live),
                &theme,
            )
            .expect("live line should exist");
            let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            assert!(
                text.starts_with(expected_emoji),
                "tps={tps}: expected leading emoji {expected_emoji:?} in {text:?}"
            );
            let fg = line.spans[0].style.fg.expect("fg should be set");
            let expected_fg = speed_tier(tps, &theme).1;
            assert_eq!(fg, expected_fg, "tps={tps} fg mismatch");
        }
    }
}
