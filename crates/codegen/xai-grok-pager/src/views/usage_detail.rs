//! Token-usage detail overlay — full-screen popup showing the session's
//! token/cost ledger with a per-model breakdown.
//!
//! Rendered as a centered overlay when `AgentView::usage_detail` is `Some`.
//! Opened by clicking the accumulated-token chip in the status bar (see
//! `views::agent_status::total_tokens_line`), dismissed by `Esc`, `q`, or the
//! `[✗]` close button.
//!
//! The same ledger backs the `/usage` scrollback block
//! ([`crate::app::status_blocks::session_usage_block_text`]) — this is the
//! interactive surface for it, so the two must stay semantically in sync:
//! `input_tokens` already includes cache reads, and a cost is only
//! trustworthy when `cost_usd_ticks` is present.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use xai_grok_shell::extensions::notification::{PromptUsage, PromptUsageModel, ticks_to_usd};

use crate::render::SafeBuf;
use crate::theme::Theme;
use crate::util::{format_duration, group_thousands};
use crate::views::goal_detail::truncate_to_width;

/// Maximum per-model rows displayed before a `+N 个其他模型` overflow row.
const MAX_MODEL_ROWS: usize = 12;

/// Width of each numeric column in the per-model table.
const NUM_COL_W: usize = 11;
/// Width of the model-call-count column.
const CALLS_COL_W: usize = 7;
/// Gap between table columns.
const COL_GAP: usize = 2;
/// Narrowest model-name column worth rendering the `调用` column for.
const MIN_NAME_W_WITH_CALLS: usize = 10;
/// Display width of the label column in the totals rows.
const TOTALS_LABEL_W: usize = 14;

/// State of the token-usage overlay.
///
/// The ledger is fetched asynchronously over ACP, so the overlay opens in
/// [`UsageDetail::Loading`] and is filled in when the task result lands. It
/// stays open across that transition so a slow fetch never looks like a
/// dropped click.
///
/// The overlay now displays both the current session's usage and the user's
/// all-time aggregate usage across every Chaos session.
#[derive(Debug, Clone, PartialEq)]
pub enum UsageDetail {
    /// Fetch in flight.
    Loading,
    /// At least one side has resolved. Either side may be `None` while its
    /// fetch is still pending or after it failed; `partial_failure` records
    /// which missing side actually failed. This keeps the two independent
    /// requests distinct without fabricating one ledger into both sections.
    Ready {
        /// Usage for the active session. `None` while pending or after
        /// failure; `partial_failure` distinguishes the failed case.
        session: Option<Box<PromptUsage>>,
        /// All-time aggregate usage across sessions. `None` while pending or
        /// after failure (e.g. older shell, OIDC stripped); `partial_failure`
        /// distinguishes the failed case and carries the user-safe reason.
        aggregate: Option<Box<PromptUsage>>,
        /// Single-line note shown at the top of the overlay when only one
        /// side of the dual-fetch succeeded. `None` when both succeeded or
        /// both failed (the latter is `Failed`).
        partial_failure: Option<String>,
    },
    /// Fetch failed; the string is already user-safe (sanitized upstream).
    Failed(String),
}

impl UsageDetail {
    /// Rows the body contributes, excluding borders and the footer hint.
    fn body_rows(&self) -> u16 {
        match self {
            UsageDetail::Loading | UsageDetail::Failed(_) => 1,
            UsageDetail::Ready {
                session,
                aggregate,
                partial_failure,
            } => {
                // 1 row for the dim partial-failure note when present.
                let note_rows = u16::from(partial_failure.is_some());
                // A missing ledger collapses to 1 row ("本次会话用量暂不可用：…")
                // rather than expanding into the full 5+ model section.
                let section_rows = |usage: &Option<Box<PromptUsage>>| -> u16 {
                    match usage {
                        None => 1,
                        Some(s) if is_empty_ledger(s) => 1,
                        Some(s) => 5 + u16::from(s.usage_is_incomplete) + per_model_section_rows(s),
                    }
                };
                let session_rows = section_rows(session);
                let aggregate_rows = match aggregate {
                    None => 1,
                    Some(s) if is_empty_ledger(s) => 1,
                    Some(s) => {
                        // 5 totals rows + note + per-model section (always shown
                        // for aggregate because the user explicitly wants the
                        // breakdown).
                        5 + u16::from(s.usage_is_incomplete) + forced_per_model_rows(s)
                    }
                };
                // Section header rows: "本次会话" + blank + "累计使用 Chaos 以来".
                // Missing-side headers still render so the dim placeholder row
                // has somewhere to live.
                let headers = 3;
                session_rows + aggregate_rows + headers + note_rows
            }
        }
    }
}

/// Whether the ledger has nothing to report (no calls and no per-model rows).
fn is_empty_ledger(usage: &PromptUsage) -> bool {
    usage.totals.model_calls == 0 && usage.model_usage.is_empty()
}

/// Rows the per-model section contributes: blank separator + header + column
/// header + capped model rows + optional overflow row. Zero when a single
/// model (or none) makes the breakdown a redundant repeat of the totals.
///
/// Shared by the height calc and the render loop so they stay in lockstep.
fn per_model_section_rows(usage: &PromptUsage) -> u16 {
    if usage.model_usage.len() < 2 {
        return 0;
    }
    forced_per_model_rows(usage)
}

/// Rows for a per-model table that is forced to render even with a single
/// model. Zero when there are no models at all.
fn forced_per_model_rows(usage: &PromptUsage) -> u16 {
    if usage.model_usage.is_empty() {
        return 0;
    }
    let shown = usage.model_usage.len().min(MAX_MODEL_ROWS);
    let overflow = usize::from(usage.model_usage.len() > MAX_MODEL_ROWS);
    (3 + shown + overflow) as u16
}

/// Centered rect for the overlay, sized to its content.
pub fn usage_detail_area(screen: Rect, detail: &UsageDetail) -> Rect {
    let preferred_w = (screen.width as f32 * 0.90) as u16;
    let w = preferred_w
        .clamp(66, 110)
        .min(screen.width.saturating_sub(4));

    // 2 border + body + 1 blank + 1 footer hint.
    let content_h = 2 + detail.body_rows() + 2;
    let v_margin = 2u16;
    let h = content_h.min(screen.height.saturating_sub(v_margin * 2));

    let x = screen.x + (screen.width.saturating_sub(w)) / 2;
    let y = screen.y + (screen.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

/// Format a model row's cost cell. Mirrors `status_blocks::format_cost`:
/// absence means untrustworthy or unknown, never free.
fn format_cost(m: &PromptUsageModel) -> String {
    match m.cost_usd_ticks {
        Some(ticks) => format!("${:.4}", ticks_to_usd(ticks)),
        None if m.cost_is_partial => "部分".to_string(),
        None => "—".to_string(),
    }
}

/// Right-align `text` in `width` display columns, truncating from the left
/// edge of the cell when it does not fit (numbers stay readable at the units
/// end). Width is measured in terminal columns, not chars.
fn pad_left(text: &str, width: usize) -> String {
    let w = UnicodeWidthStr::width(text);
    if w >= width {
        return text.to_string();
    }
    format!("{}{text}", " ".repeat(width - w))
}

/// Right-pad `text` to `width` display columns (model names, left-aligned).
fn pad_right(text: &str, width: usize) -> String {
    let w = UnicodeWidthStr::width(text);
    if w >= width {
        return text.to_string();
    }
    format!("{text}{}", " ".repeat(width - w))
}

/// Column widths for the per-model table given the usable inner width:
/// `(name, show_calls)`. The `调用` column is dropped first when space is
/// tight so the model name never collapses to nothing.
fn table_layout(w: usize) -> (usize, bool) {
    let numeric_with_calls = NUM_COL_W * 3 + CALLS_COL_W + COL_GAP * 4;
    let numeric_no_calls = NUM_COL_W * 3 + COL_GAP * 3;
    let name_with_calls = w.saturating_sub(numeric_with_calls);
    if name_with_calls >= MIN_NAME_W_WITH_CALLS {
        (name_with_calls, true)
    } else {
        (w.saturating_sub(numeric_no_calls).max(6), false)
    }
}

/// Build one per-model table row (or the header row when `header` is set).
fn table_row(
    name: &str,
    input: &str,
    output: &str,
    calls: Option<&str>,
    cost: &str,
    w: usize,
) -> String {
    let (name_w, show_calls) = table_layout(w);
    let gap = " ".repeat(COL_GAP);
    let mut row = format!(
        "{}{gap}{}{gap}{}",
        pad_right(&truncate_to_width(name, name_w), name_w),
        pad_left(input, NUM_COL_W),
        pad_left(output, NUM_COL_W),
    );
    if show_calls {
        row.push_str(&gap);
        row.push_str(&pad_left(calls.unwrap_or(""), CALLS_COL_W));
    }
    row.push_str(&gap);
    row.push_str(&pad_left(cost, NUM_COL_W));
    row
}

/// Render the token-usage overlay. Returns the close-button rect so the
/// caller can cache it for hit-testing, or `None` when the area is too small
/// to draw into.
pub fn render_usage_detail(
    buf: &mut Buffer,
    area: Rect,
    detail: &UsageDetail,
    close_hovered: bool,
) -> Option<Rect> {
    let theme = Theme::current();
    if area.width < 20 || area.height < 5 {
        return None;
    }

    // Clear the popup area.
    let clear_style = Style::default().bg(theme.bg_base);
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            if let Some(cell) = buf.cell_mut(ratatui::layout::Position::new(x, y)) {
                cell.reset();
                cell.set_style(clear_style);
            }
        }
    }

    let border_style = Style::default().fg(theme.gray).bg(theme.bg_base);
    let block = ratatui::widgets::Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(border_style)
        .style(Style::default().bg(theme.bg_base));
    let inner = block.inner(area);
    ratatui::widgets::Widget::render(block, area, buf);

    // Close button [✗] in the top-right (ASCII `[x]` on legacy ConHost).
    // Measured in display columns so the hit-rect matches the painted cells.
    let close_text = format!("[{}]", crate::glyphs::ballot_x());
    let close_w = UnicodeWidthStr::width(close_text.as_str()) as u16;
    let close_x = area.x + area.width.saturating_sub(close_w + 1);
    let title_cols = close_x.saturating_sub(area.x + 3); // 1-col gap before [✗]
    let title_style = Style::default()
        .fg(theme.accent_plan)
        .bg(theme.bg_base)
        .add_modifier(Modifier::BOLD);
    buf.set_span_safe(
        area.x + 2,
        area.y,
        &Span::styled(" Token 用量统计 ", title_style),
        title_cols,
    );

    let close_style = if close_hovered {
        Style::default()
            .fg(theme.text_primary)
            .bg(theme.bg_base)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.gray).bg(theme.bg_base)
    };
    buf.set_span_safe(
        close_x,
        area.y,
        &Span::styled(close_text, close_style),
        close_w,
    );
    let close_rect = Rect::new(close_x, area.y, close_w, 1);

    let mut y = inner.y;
    let x = inner.x + 1;
    let w = inner.width.saturating_sub(2);
    let bottom = inner.y + inner.height;

    // The footer hint owns the last inner row; the body stops one row above it
    // so a content overflow truncates the table rather than the hint.
    let body_bottom = bottom.saturating_sub(1);

    let label_style = Style::default().fg(theme.gray).bg(theme.bg_base);
    let value_style = Style::default().fg(theme.text_primary).bg(theme.bg_base);
    let dim_style = Style::default().fg(theme.gray_dim).bg(theme.bg_base);
    let section_style = Style::default()
        .fg(theme.accent_plan)
        .bg(theme.bg_base)
        .add_modifier(Modifier::BOLD);

    // Macro that emulates the `push` helper. Used in place of a closure because
    // the body below also declares `render_section` as a closure, and nested
    // closures interacting with the outer `&UsageDetail` lifetime trigger a
    // spurious `'1: 'static` requirement under HRTB inference. A macro sidesteps
    // the issue entirely — the borrow checker sees straight-line code.
    //
    // `$y:expr` (rather than `:ident`) so both `y` (inside the inner closure
    // where it's already `&mut u16`) and `&mut y` (the outer call sites where
    // we pass a fresh borrow of the local) work.
    macro_rules! push {
        ($buf:ident, $y:expr, $line:expr) => {{
            let __y: &mut u16 = $y;
            if *__y < body_bottom {
                $buf.set_line_safe(x, *__y, &$line, w);
                *__y += 1;
            }
        }};
    }

    // Render the totals + per-model breakdown for one usage ledger.
    // `force_model_breakdown` renders the per-model table even for a single
    // model, which is what the aggregate section wants.
    let render_section =
        |buf: &mut Buffer, y: &mut u16, usage: &PromptUsage, force_model_breakdown: bool| {
            if is_empty_ledger(usage) {
                let msg = if usage.usage_is_incomplete {
                    "尚无记录，但统计不完整，实际用量可能更高。"
                } else {
                    "暂无记录。"
                };
                push!(buf, y, Line::from(Span::styled(msg, dim_style)));
                return;
            }

            let t = &usage.totals;
            let row = |label: &str, value: String, note: Option<String>| {
                // Pad by DISPLAY width, not char count: `模型调用` is 4 chars
                // but 8 columns, so `{:<14}` would push its value 4 columns
                // past the ASCII rows' values.
                let mut spans = vec![
                    Span::styled(pad_right(label, TOTALS_LABEL_W), label_style),
                    Span::styled(value, value_style),
                ];
                if let Some(note) = note {
                    spans.push(Span::styled(note, dim_style));
                }
                Line::from(spans)
            };
            push!(
                buf,
                y,
                row(
                    "输入 Token",
                    group_thousands(t.input_tokens),
                    Some(format!(
                        "（缓存命中 {}）",
                        group_thousands(t.cached_read_tokens)
                    )),
                )
            );
            push!(
                buf,
                y,
                row(
                    "输出 Token",
                    group_thousands(t.output_tokens),
                    Some(format!("（推理 {}）", group_thousands(t.reasoning_tokens))),
                )
            );
            push!(
                buf,
                y,
                row("Token 总计", group_thousands(t.total_tokens), None)
            );
            push!(
                buf,
                y,
                row(
                    "模型调用",
                    format!("{} 次", group_thousands(t.model_calls)),
                    Some(format!(
                        " · API 耗时 {}",
                        format_duration(std::time::Duration::from_millis(t.api_duration_ms))
                    )),
                )
            );
            push!(buf, y, row("费用", format_cost(t), None));

            let show_models = force_model_breakdown || per_model_section_rows(usage) > 0;
            if show_models {
                push!(buf, y, Line::from(""));
                push!(
                    buf,
                    y,
                    Line::from(Span::styled(
                        "按模型：",
                        Style::default()
                            .fg(theme.text_primary)
                            .bg(theme.bg_base)
                            .add_modifier(Modifier::BOLD),
                    ))
                );
                push!(
                    buf,
                    y,
                    Line::from(Span::styled(
                        table_row("模型", "输入", "输出", Some("调用"), "费用", w as usize),
                        label_style,
                    ))
                );
                for (model, m) in usage.model_usage.iter().take(MAX_MODEL_ROWS) {
                    push!(
                        buf,
                        y,
                        Line::from(Span::styled(
                            table_row(
                                model,
                                &group_thousands(m.input_tokens),
                                &group_thousands(m.output_tokens),
                                Some(&group_thousands(m.model_calls)),
                                &format_cost(m),
                                w as usize,
                            ),
                            value_style,
                        ))
                    );
                }
                if usage.model_usage.len() > MAX_MODEL_ROWS {
                    let more = usage.model_usage.len() - MAX_MODEL_ROWS;
                    push!(
                        buf,
                        y,
                        Line::from(Span::styled(format!("+{more} 个其他模型"), dim_style))
                    );
                }
            }

            if usage.usage_is_incomplete {
                push!(
                    buf,
                    y,
                    Line::from(Span::styled(
                        "注意：用量统计不完整，实际用量可能更高。",
                        Style::default().fg(theme.warning).bg(theme.bg_base),
                    ))
                );
            }
        };

    match detail {
        UsageDetail::Loading => {
            push!(
                buf,
                &mut y,
                Line::from(Span::styled("正在加载用量…", dim_style))
            );
        }
        UsageDetail::Failed(error) => {
            push!(
                buf,
                &mut y,
                Line::from(Span::styled(
                    format!("加载用量失败：{error}"),
                    Style::default().fg(theme.accent_error).bg(theme.bg_base),
                ))
            );
        }
        UsageDetail::Ready {
            session,
            aggregate,
            partial_failure,
        } => {
            // Single-line note at the very top so the user sees we partially
            // succeeded before scanning the sections below.
            if let Some(note) = partial_failure {
                push!(buf, &mut y, Line::from(Span::styled(note, dim_style)));
            }
            push!(
                buf,
                &mut y,
                Line::from(Span::styled("本次会话", section_style))
            );
            match session {
                Some(usage) => render_section(buf, &mut y, usage, false),
                None => {
                    let failed = partial_failure.as_deref().is_some_and(|note| {
                        note.starts_with("本次会话用量加载失败：")
                            || note.contains("; 本次会话用量加载失败：")
                    });
                    let (message, color) = if failed {
                        ("本次会话用量暂不可用。", theme.accent_error)
                    } else {
                        ("本次会话用量加载中…", theme.gray)
                    };
                    push!(
                        buf,
                        &mut y,
                        Line::from(Span::styled(
                            message,
                            Style::default().fg(color).bg(theme.bg_base),
                        ))
                    );
                }
            }

            push!(buf, &mut y, Line::from(""));
            push!(
                buf,
                &mut y,
                Line::from(Span::styled("累计使用 Chaos 以来", section_style))
            );
            match aggregate {
                Some(usage) => render_section(buf, &mut y, usage, true),
                None => {
                    let failed = partial_failure.as_deref().is_some_and(|note| {
                        note.starts_with("累计用量加载失败：")
                            || note.contains("; 累计用量加载失败：")
                    });
                    let (message, color) = if failed {
                        ("累计用量暂不可用。", theme.accent_error)
                    } else {
                        ("累计用量加载中…", theme.gray)
                    };
                    push!(
                        buf,
                        &mut y,
                        Line::from(Span::styled(
                            message,
                            Style::default().fg(color).bg(theme.bg_base),
                        ))
                    );
                }
            }
        }
    }

    // Footer hint pinned to the last inner row.
    if bottom > inner.y {
        buf.set_line_safe(
            x,
            bottom - 1,
            &Line::from(Span::styled("Esc: 关闭", dim_style)),
            w,
        );
    }

    Some(close_rect)
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;

    fn buffer_text(buf: &Buffer) -> String {
        let area = *buf.area();
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Substring check that ignores spacing.
    ///
    /// A wide (CJK) glyph occupies two cells: the glyph, then a reset
    /// continuation cell whose symbol is a single space. Reading the buffer
    /// back cell-by-cell therefore yields `关 闭` for `关闭`, so needles must
    /// be matched with all spaces stripped from both sides.
    fn contains(buf: &Buffer, needle: &str) -> bool {
        let strip = |s: &str| s.replace([' ', '\n'], "");
        strip(&buffer_text(buf)).contains(&strip(needle))
    }

    fn model(input: u64, output: u64, calls: u64, ticks: Option<i64>) -> PromptUsageModel {
        PromptUsageModel {
            input_tokens: input,
            output_tokens: output,
            total_tokens: input + output,
            cached_read_tokens: input / 2,
            reasoning_tokens: output / 4,
            model_calls: calls,
            api_duration_ms: 1_000,
            cost_usd_ticks: ticks,
            cost_is_partial: false,
            cost_missing_calls: 0,
            decode_duration_ms: 0,
            decode_tokens_per_sec: None,
        }
    }

    fn usage_with_models(models: &[(&str, PromptUsageModel)]) -> PromptUsage {
        let mut model_usage = IndexMap::new();
        let mut totals = PromptUsageModel::default();
        for (name, m) in models {
            totals.input_tokens += m.input_tokens;
            totals.output_tokens += m.output_tokens;
            totals.total_tokens += m.total_tokens;
            totals.cached_read_tokens += m.cached_read_tokens;
            totals.reasoning_tokens += m.reasoning_tokens;
            totals.model_calls += m.model_calls;
            totals.api_duration_ms += m.api_duration_ms;
            model_usage.insert((*name).to_string(), m.clone());
        }
        totals.cost_usd_ticks = Some(10_000_000_000);
        PromptUsage {
            totals,
            model_usage,
            num_turns: 1,
            usage_is_incomplete: false,
        }
    }

    fn render(detail: &UsageDetail, screen: Rect) -> (Buffer, Option<Rect>) {
        let area = usage_detail_area(screen, detail);
        let mut buf = Buffer::empty(screen);
        let close = render_usage_detail(&mut buf, area, detail, false);
        (buf, close)
    }

    /// Build a `Ready` detail with the same ledger used for both session and
    /// aggregate. Most render tests only care about one shape; this keeps the
    /// assertions focused.
    fn ready(usage: PromptUsage) -> UsageDetail {
        UsageDetail::Ready {
            session: Some(Box::new(usage.clone())),
            aggregate: Some(Box::new(usage)),
            partial_failure: None,
        }
    }

    #[test]
    fn loading_state_renders_placeholder_and_hint() {
        let (buf, close) = render(&UsageDetail::Loading, Rect::new(0, 0, 100, 30));
        let text = buffer_text(&buf);
        assert!(contains(&buf, "正在加载用量…"), "{text}");
        assert!(contains(&buf, "Esc: 关闭"), "{text}");
        assert!(contains(&buf, "Token用量统计"), "{text}");
        assert!(close.is_some());
    }

    #[test]
    fn failed_state_shows_error() {
        let detail = UsageDetail::Failed("connection reset".to_string());
        let (buf, _) = render(&detail, Rect::new(0, 0, 100, 30));
        let text = buffer_text(&buf);
        assert!(contains(&buf, "加载用量失败：connectionreset"), "{text}");
    }

    #[test]
    fn missing_side_without_failure_note_renders_loading() {
        let detail = UsageDetail::Ready {
            session: Some(Box::new(PromptUsage::default())),
            aggregate: None,
            partial_failure: None,
        };
        let (buf, _) = render(&detail, Rect::new(0, 0, 100, 30));
        let text = buffer_text(&buf);
        assert!(contains(&buf, "累计用量加载中…"), "{text}");
        assert!(!contains(&buf, "累计用量暂不可用。"), "{text}");
    }

    #[test]
    fn missing_side_with_matching_failure_note_renders_unavailable() {
        let detail = UsageDetail::Ready {
            session: None,
            aggregate: Some(Box::new(PromptUsage::default())),
            partial_failure: Some("本次会话用量加载失败：会话尚未开始".to_string()),
        };
        let (buf, _) = render(&detail, Rect::new(0, 0, 100, 30));
        let text = buffer_text(&buf);
        assert!(contains(&buf, "本次会话用量暂不可用。"), "{text}");
        assert!(!contains(&buf, "本次会话用量加载中…"), "{text}");
    }

    #[test]
    fn empty_ledger_reads_as_no_calls() {
        let detail = ready(PromptUsage::default());
        let (buf, _) = render(&detail, Rect::new(0, 0, 100, 30));
        assert!(contains(&buf, "暂无记录。"));
    }

    #[test]
    fn per_model_breakdown_lists_each_model() {
        let usage = usage_with_models(&[
            ("grok-4", model(1_000_000, 30_000, 40, Some(9_000_000_000))),
            (
                "grok-4-fast",
                model(200_000, 5_000, 12, Some(1_000_000_000)),
            ),
        ]);
        let detail = ready(usage);
        let (buf, _) = render(&detail, Rect::new(0, 0, 100, 30));
        let text = buffer_text(&buf);
        assert!(contains(&buf, "按模型："), "{text}");
        assert!(text.contains("grok-4"), "{text}");
        assert!(text.contains("grok-4-fast"), "{text}");
        assert!(text.contains("1,000,000"), "{text}");
        assert!(text.contains("$0.9000"), "{text}");
    }

    /// A single-model session ledger collapses to the totals — repeating the same
    /// numbers under a "按模型" header would be pure noise. The aggregate section
    /// still forces the breakdown.
    #[test]
    fn single_model_suppresses_session_breakdown_but_shows_aggregate() {
        let usage = usage_with_models(&[("grok-4", model(1_000, 100, 2, Some(1)))]);
        let detail = ready(usage);
        let (buf, _) = render(&detail, Rect::new(0, 0, 100, 30));
        // Session section has one model → no "按模型" there.
        // Aggregate section always shows the per-model table.
        assert!(contains(&buf, "按模型："));
    }

    #[test]
    fn overflow_row_caps_model_list() {
        let rows: Vec<(String, PromptUsageModel)> = (0..MAX_MODEL_ROWS + 3)
            .map(|i| (format!("model-{i}"), model(1_000, 100, 1, Some(1))))
            .collect();
        let borrowed: Vec<(&str, PromptUsageModel)> =
            rows.iter().map(|(n, m)| (n.as_str(), m.clone())).collect();
        let usage = usage_with_models(&borrowed);
        let detail = ready(usage);
        let (buf, _) = render(&detail, Rect::new(0, 0, 120, 40));
        let text = buffer_text(&buf);
        assert!(contains(&buf, "+3 个其他模型"), "{text}");
        assert!(!text.contains("model-13"), "{text}");
    }

    #[test]
    fn incomplete_ledger_warns() {
        let mut usage = usage_with_models(&[("grok-4", model(1_000, 100, 2, None))]);
        usage.usage_is_incomplete = true;
        usage.totals.cost_usd_ticks = None;
        let detail = ready(usage);
        let (buf, _) = render(&detail, Rect::new(0, 0, 100, 30));
        assert!(contains(&buf, "用量统计不完整"));
    }

    /// A missing cost must never read as free.
    #[test]
    fn absent_cost_renders_as_dash_not_zero() {
        let mut m = model(1_000, 100, 2, None);
        m.cost_is_partial = false;
        assert_eq!(format_cost(&m), "—");
        m.cost_is_partial = true;
        assert_eq!(format_cost(&m), "部分");
    }

    /// Totals labels are padded by display width, so every value starts in
    /// the same column no matter how many CJK glyphs its label has.
    /// `{:<14}` (char count) would push `模型调用`'s value 4 columns right.
    #[test]
    fn totals_values_align_across_cjk_and_ascii_labels() {
        let usage = usage_with_models(&[
            ("grok-4", model(1_000_000, 30_000, 40, Some(1))),
            ("grok-4-fast", model(200_000, 5_000, 12, Some(1))),
        ]);
        let detail = ready(usage);
        let screen = Rect::new(0, 0, 100, 30);
        let area = usage_detail_area(screen, &detail);
        let mut buf = Buffer::empty(screen);
        render_usage_detail(&mut buf, area, &detail, false);

        // The five totals rows sit directly under the "本次会话" section title.
        let value_cols: Vec<u16> = (area.y + 2..area.y + 7)
            .map(|y| {
                (area.x..area.x + area.width)
                    .find(|&x| {
                        let sym = buf[(x, y)].symbol();
                        sym.chars()
                            .next()
                            .is_some_and(|c| c.is_ascii_digit() || c == '$')
                    })
                    .unwrap_or_else(|| panic!("no value found on row {y}"))
            })
            .collect();
        assert!(
            value_cols.windows(2).all(|w| w[0] == w[1]),
            "totals values must share one column, got {value_cols:?}\n{}",
            buffer_text(&buf)
        );
    }

    /// Every body row must fit inside the popup — the height calc and the
    /// render loop share `per_model_section_rows`, so a drift here would
    /// silently truncate the table.
    #[test]
    fn area_height_fits_all_rows() {
        let usage = usage_with_models(&[
            ("grok-4", model(1_000_000, 30_000, 40, Some(1))),
            ("grok-4-fast", model(200_000, 5_000, 12, Some(1))),
            ("grok-3", model(50_000, 1_000, 3, Some(1))),
        ]);
        let detail = ready(usage);
        let screen = Rect::new(0, 0, 100, 40);
        let area = usage_detail_area(screen, &detail);
        // 2 border + body + blank + footer
        assert_eq!(area.height, 2 + detail.body_rows() + 2);
        let (buf, _) = render(&detail, screen);
        let text = buffer_text(&buf);
        assert!(text.contains("grok-3"), "last model row must be painted");
        assert!(contains(&buf, "Esc: 关闭"), "footer must survive: {text}");
    }

    /// The table drops the call-count column before it starves the model name.
    #[test]
    fn narrow_table_drops_calls_column() {
        let (name_w, show_calls) = table_layout(100);
        assert!(show_calls && name_w >= MIN_NAME_W_WITH_CALLS);
        let (narrow_name_w, narrow_calls) = table_layout(50);
        assert!(!narrow_calls);
        assert!(narrow_name_w >= 6);
    }

    #[test]
    fn area_is_centered_and_clamped() {
        let screen = Rect::new(0, 0, 200, 50);
        let area = usage_detail_area(screen, &UsageDetail::Loading);
        assert_eq!(area.width, 110, "wide screens clamp to the max width");
        assert_eq!(area.x, (200 - 110) / 2);

        // A tiny terminal must not produce a rect wider than the screen.
        let tiny = Rect::new(0, 0, 40, 10);
        let area = usage_detail_area(tiny, &UsageDetail::Loading);
        assert!(area.width <= tiny.width);
        assert!(area.height <= tiny.height);
    }
}
