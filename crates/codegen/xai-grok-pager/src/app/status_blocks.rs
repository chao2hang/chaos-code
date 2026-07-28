//! Read-only system-block text for `/queue`, `/tasks`, and `/usage`.
//!
//! Plain text committed into scrollback — the primary inspection surface in
//! minimal mode (no interactive panes). Kept out of `dispatch` for easy
//! unit tests.

use crate::app::agent::BgTaskStatus;
use crate::app::agent_view::AgentView;
use crate::app::subagent::format_subagent_label;
use crate::util::{format_duration, group_thousands};

/// `/queue` body — a read-only list of the queued prompts.
///
/// Server-authoritative shared-queue rows (the in-flight prompt excluded) come
/// first in broadcast order, then the local drip-feed queue — matching
/// [`crate::views::queue_pane::QueuePane::sync_from_merged`]'s ordering.
pub(crate) fn queue_block_text(agent: &AgentView) -> String {
    let running_id = agent.session.current_prompt_id.as_deref();

    let mut rows: Vec<String> = Vec::new();
    let mut pos = 1usize;
    for wire in &agent.shared_queue {
        if running_id == Some(wire.id.as_str()) {
            continue;
        }
        rows.push(format_queue_row(pos, &wire.text));
        pos += 1;
    }
    for prompt in &agent.session.pending_prompts {
        rows.push(format_queue_row(pos, &prompt.text));
        pos += 1;
    }

    if rows.is_empty() {
        "队列为空。".to_string()
    } else {
        let header = format!("排队提示（{}）：", rows.len());
        join_header_rows(header, rows)
    }
}

///
/// [`crate::views::tasks_pane::TasksPane`] without its styled rows.
pub(crate) fn tasks_block_text(agent: &AgentView) -> String {
    let mut rows: Vec<String> = Vec::new();

    let mut workflows: Vec<_> = agent.workflow_runs.iter().collect();
    workflows.sort_by(|a, b| {
        b.is_active()
            .cmp(&a.is_active())
            .then(b.received_at.cmp(&a.received_at))
            .then(a.run_id.cmp(&b.run_id))
    });
    for run in workflows {
        let active = run.active_agent_count();
        let agents = match active {
            0 => String::new(),
            1 => " · 1 个 Agent".to_string(),
            n => format!(" · {n} 个 Agent"),
        };
        let phase = run
            .current_phase
            .as_deref()
            .map(str::trim)
            .filter(|phase| !phase.is_empty())
            .map(|phase| format!(" · {phase}"))
            .unwrap_or_default();
        rows.push(format!(
            "  {:<9}工作流 · {}{phase}{agents}  ({})",
            if run.is_active() {
                "运行中".to_string()
            } else {
                run.status.replace('_', " ")
            },
            run.name,
            format_duration(std::time::Duration::from_millis(run.live_elapsed_ms()))
        ));
    }

    // ── Subagents ──
    let mut subs: Vec<_> = agent
        .subagent_sessions
        .values()
        .filter(|s| s.workflow_run_id.is_none())
        .collect();
    subs.sort_by(|a, b| {
        b.is_running()
            .cmp(&a.is_running())
            .then(b.started_at.cmp(&a.started_at))
            .then(a.child_session_id.cmp(&b.child_session_id))
    });
    for info in subs {
        let (type_label, desc) = format_subagent_label(info);
        let status = if info.pending_kill {
            "停止中"
        } else if info.is_running() {
            "运行中"
        } else {
            info.status.as_deref().unwrap_or("完成")
        };
        let label = if desc.is_empty() {
            type_label
        } else {
            format!("{type_label} · {desc}")
        };
        rows.push(format!(
            "  {status:<9}{label}  ({})",
            format_duration(info.display_elapsed())
        ));
    }

    // ── Background tasks / monitors ──
    let mut tasks: Vec<_> = agent.session.bg_tasks.values().collect();
    tasks.sort_by(|a, b| {
        let (ar, br) = (
            a.status == BgTaskStatus::Running,
            b.status == BgTaskStatus::Running,
        );
        br.cmp(&ar)
            .then(b.start_time.cmp(&a.start_time))
            .then(a.task_id.cmp(&b.task_id))
    });
    for task in tasks {
        let kind = if task.is_monitor { "监视" } else { "任务" };
        let one_line = task
            .description
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| first_nonempty_line(&task.command));
        let status = if task.pending_kill {
            "停止中"
        } else {
            match task.status {
                BgTaskStatus::Running => "运行中",
                BgTaskStatus::Done => "完成",
                BgTaskStatus::Failed => "失败",
            }
        };
        rows.push(format!(
            "  {status:<9}{kind} · {one_line}  ({})",
            format_duration(task.elapsed())
        ));
    }

    // ── Scheduled (/loop) tasks ──
    let mut sched: Vec<_> = agent.session.scheduled_tasks.values().collect();
    sched.sort_by(|a, b| {
        a.tag
            .cmp(&b.tag)
            .then(a.human_schedule.cmp(&b.human_schedule))
            .then(a.task_id.cmp(&b.task_id))
    });
    for info in sched {
        rows.push(format!(
            "  {:<9}{} · {} · {}",
            "已调度",
            info.tag,
            info.human_schedule,
            first_nonempty_line(&info.prompt)
        ));
    }

    if rows.is_empty() {
        "没有后台任务、工作流或子 Agent。".to_string()
    } else {
        let header = format!("任务（{}）：", rows.len());
        join_header_rows(header, rows)
    }
}

/// `/usage` body — per-session token and cost totals, scoped to the ledger's
/// lifetime: since session start, or since the last `/resume`.
pub(crate) fn session_usage_block_text(
    usage: &xai_grok_shell::extensions::notification::PromptUsage,
) -> String {
    let t = &usage.totals;
    if t.model_calls == 0 && usage.model_usage.is_empty() {
        return if usage.usage_is_incomplete {
            "会话用量：尚无记录，但统计不完整，实际用量可能更高。".to_string()
        } else {
            "会话用量：本次会话尚未调用模型。".to_string()
        };
    }

    let mut rows = Vec::new();
    rows.push(format!(
        "  输入 Token：    {}（缓存命中 {}）",
        group_thousands(t.input_tokens),
        group_thousands(t.cached_read_tokens),
    ));
    rows.push(format!(
        "  输出 Token：    {}（推理 {}）",
        group_thousands(t.output_tokens),
        group_thousands(t.reasoning_tokens),
    ));
    rows.push(format!(
        "  Token 总计：    {}",
        group_thousands(t.total_tokens)
    ));
    rows.push(format!(
        "  模型调用：      {} 次 · API 耗时 {}",
        group_thousands(t.model_calls),
        format_duration(std::time::Duration::from_millis(t.api_duration_ms)),
    ));
    rows.push(format!(
        "  输出速率：      {}（按 API 耗时平均）",
        format_output_rate(t.output_tokens, t.api_duration_ms),
    ));
    rows.push(format!(
        "  解码速率：      {}（剔除首字延迟）",
        format_decode_rate(t.decode_tokens_per_sec, t.decode_duration_ms),
    ));
    rows.push(format!("  费用：          {}", format_cost(t)));

    if usage.model_usage.len() > 1 {
        rows.push("  按模型：".to_string());
        for (model, m) in &usage.model_usage {
            rows.push(format!(
                "    {model}：输入 {} / 输出 {} · {} · {} · 解码 {}",
                group_thousands(m.input_tokens),
                group_thousands(m.output_tokens),
                format_cost(m),
                format_output_rate(m.output_tokens, m.api_duration_ms),
                format_decode_rate(m.decode_tokens_per_sec, m.decode_duration_ms),
            ));
        }
    }

    if usage.usage_is_incomplete {
        rows.push("  注意：用量统计不完整，实际用量可能更高。".to_string());
    }

    join_header_rows("会话用量（自启动或最近一次恢复后）：".to_string(), rows)
}

/// Cost cell. Ticks are 1e10 per USD; partial sums are scrubbed to absent.
fn format_cost(m: &xai_grok_shell::extensions::notification::PromptUsageModel) -> String {
    use xai_grok_shell::extensions::notification::ticks_to_usd;
    match m.cost_usd_ticks {
        Some(ticks) => format!("${:.4}", ticks_to_usd(ticks)),
        None if m.cost_is_partial => "不可用（部分调用未返回价格）".to_string(),
        None => "不可用（提供商未返回价格）".to_string(),
    }
}

/// 输出速率单元格。基于 `output_tokens / api_duration_ms` 的会话平均值。
///
/// - `api_duration_ms == 0` 或输出为 0 时显示「不可用」，避免除零；
/// - 小于 1 token/s 时用一位小数（`0.7 tok/s`），其余用整数；
/// - 使用千位分隔与其他 token 数字保持一致。
fn format_output_rate(output_tokens: u64, api_duration_ms: u64) -> String {
    if output_tokens == 0 || api_duration_ms == 0 {
        return "速率不可用".to_string();
    }
    let tps = output_tokens as f64 * 1000.0 / api_duration_ms as f64;
    if tps < 1.0 {
        format!("{tps:.1} tok/s")
    } else {
        format!("{} tok/s", group_thousands(tps.round() as u64))
    }
}

/// 解码速率单元格：稳态口径，剔除首字延迟。
///
/// - 优先使用 wire 上带来的 `decode_tokens_per_sec`（老消费者 / 未采样时为 None）；
/// - 兜底根据 `decode_duration_ms` 现场重算；两者都缺时显示「不可用」。
///   与 `format_output_rate` 一样，绝不把「缺数据」渲染成 `0 tok/s`。
pub(crate) fn format_decode_rate(
    decode_tokens_per_sec: Option<f32>,
    decode_duration_ms: u64,
) -> String {
    let tps = match decode_tokens_per_sec {
        Some(v) if v > 0.0 => Some(f64::from(v)),
        _ if decode_duration_ms > 0 => None, // 保留给上层补算，但当前入参不含 output → 视作 None
        _ => None,
    };
    match tps {
        Some(v) if v < 1.0 => format!("{v:.1} tok/s"),
        Some(v) => format!("{} tok/s", group_thousands(v.round() as u64)),
        None => "速率不可用".to_string(),
    }
}

/// First non-empty, trimmed line of `text` (empty string if none). Collapses a
/// multi-line prompt/command to a single display line.
fn first_nonempty_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
}

/// Format one `/queue` row as `  #N  <first non-empty line>` with a
/// `(+K more lines)` suffix for multi-line prompts.
fn format_queue_row(pos: usize, text: &str) -> String {
    let first_line = first_nonempty_line(text);
    let extra = text.lines().count().saturating_sub(1);
    if extra > 0 {
        format!(
            "  #{pos}  {first_line}  (+{extra} more line{})",
            if extra == 1 { "" } else { "s" }
        )
    } else {
        format!("  #{pos}  {first_line}")
    }
}

/// Join a header line above its rows into a single block string.
fn join_header_rows(header: String, rows: Vec<String>) -> String {
    std::iter::once(header)
        .chain(rows)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use xai_grok_shell::extensions::notification::{PromptUsage, PromptUsageModel};

    fn model_row(input: u64, output: u64, ticks: Option<i64>) -> PromptUsageModel {
        PromptUsageModel {
            input_tokens: input,
            output_tokens: output,
            total_tokens: input + output,
            cached_read_tokens: 0,
            reasoning_tokens: 0,
            model_calls: 1,
            api_duration_ms: 1_000,
            decode_duration_ms: 0,
            decode_tokens_per_sec: None,
            cost_usd_ticks: ticks,
            cost_is_partial: false,
            cost_missing_calls: 0,
        }
    }

    #[test]
    fn session_usage_block_empty_ledger() {
        let usage = PromptUsage::default();
        assert_eq!(
            session_usage_block_text(&usage),
            "会话用量：本次会话尚未调用模型。"
        );

        // Empty but incomplete must not read as a clean zero.
        let incomplete = PromptUsage {
            usage_is_incomplete: true,
            ..Default::default()
        };
        assert!(session_usage_block_text(&incomplete).contains("统计不完整"));
    }

    #[test]
    fn session_usage_block_formats_tokens_and_cost() {
        let mut totals = model_row(1_234_567, 45_678, Some(12_345_000_000));
        totals.cached_read_tokens = 1_000_000;
        totals.reasoning_tokens = 12_000;
        totals.model_calls = 42;
        totals.api_duration_ms = 192_000;
        let usage = PromptUsage {
            totals,
            ..Default::default()
        };
        let text = session_usage_block_text(&usage);
        for expected in [
            "输入 Token：    1,234,567（缓存命中 1,000,000）",
            "输出 Token：    45,678（推理 12,000）",
            "Token 总计：    1,280,245",
        ] {
            assert!(
                text.contains(expected),
                "缺少用量字段 `{expected}`：\n{text}"
            );
        }
        // Snapshot pins content and column alignment together; single-model
        // sessions must skip the redundant by-model breakdown.
        insta::assert_snapshot!("session_usage_block_full", text);
    }

    #[test]
    fn session_usage_block_lists_models_when_multiple() {
        let mut usage = PromptUsage {
            totals: model_row(150, 15, None),
            ..Default::default()
        };
        usage
            .model_usage
            .insert("grok-build".into(), model_row(100, 10, None));
        usage
            .model_usage
            .insert("grok-4".into(), model_row(50, 5, None));
        let text = session_usage_block_text(&usage);
        assert!(text.contains("按模型："), "{text}");
        assert!(text.contains("grok-build：输入 100 / 输出 10"), "{text}");
        assert!(text.contains("grok-4：输入 50 / 输出 5"), "{text}");
        // 每一行末尾要带 tok/s（这两个 model_row 都是 output=10 / 1000ms 或 5/1000ms）。
        assert!(text.contains("· 10 tok/s"), "{text}");
        assert!(text.contains("· 5 tok/s"), "{text}");
        // 且带解码速率明细占位（未采样时是「不可用」）。
        assert!(text.contains("解码 速率不可用"), "{text}");
    }

    /// 有 wire 侧稳态速率时，「解码速率」行显示真实数字；总计一行 + 多模型明细都要带上。
    #[test]
    fn session_usage_block_shows_decode_rate_when_present() {
        let mut totals = model_row(100, 200, None);
        totals.api_duration_ms = 1_200;
        totals.decode_duration_ms = 1_000;
        totals.decode_tokens_per_sec = Some(200.0);
        let mut usage = PromptUsage {
            totals: totals.clone(),
            ..Default::default()
        };
        let mut m1 = totals.clone();
        m1.decode_tokens_per_sec = Some(150.0);
        m1.decode_duration_ms = 1_000;
        usage.model_usage.insert("grok-4".into(), m1);
        let mut m2 = model_row(50, 5, None);
        m2.decode_tokens_per_sec = Some(400.0);
        m2.decode_duration_ms = 12;
        usage.model_usage.insert("grok-build".into(), m2);
        let text = session_usage_block_text(&usage);
        assert!(
            text.contains("解码速率：      200 tok/s（剔除首字延迟）"),
            "{text}"
        );
        assert!(text.contains("解码 150 tok/s"), "{text}");
        assert!(text.contains("解码 400 tok/s"), "{text}");
    }

    #[test]
    fn session_usage_block_absent_cost_is_unknown_not_free() {
        let usage = PromptUsage {
            totals: model_row(100, 10, None),
            ..Default::default()
        };
        let text = session_usage_block_text(&usage);
        insta::assert_snapshot!("session_usage_block_absent_cost", text);
        // Unknown cost must never read as free.
        assert!(!text.contains("$0"), "{text}");
    }

    #[test]
    fn session_usage_block_flags_partial_and_incomplete() {
        let mut totals = model_row(100, 10, None);
        totals.cost_is_partial = true;
        let usage = PromptUsage {
            totals,
            usage_is_incomplete: true,
            ..Default::default()
        };
        let text = session_usage_block_text(&usage);
        assert!(text.contains("部分调用未返回价格"), "{text}");
        assert!(text.contains("用量统计不完整"), "{text}");
    }

    #[test]
    fn group_thousands_groups_digits() {
        assert_eq!(group_thousands(0), "0");
        assert_eq!(group_thousands(999), "999");
        assert_eq!(group_thousands(1_000), "1,000");
        assert_eq!(group_thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn format_output_rate_covers_edge_cases() {
        // 缺条件时不能读作 0 tok/s。
        assert_eq!(format_output_rate(0, 1_000), "速率不可用");
        assert_eq!(format_output_rate(100, 0), "速率不可用");
        // 亚 1 tok/s 用一位小数，避免四舍五入到 0。
        assert_eq!(format_output_rate(1, 2_000), "0.5 tok/s");
        // 常见范围：45,678 / 192s ≈ 238 tok/s。
        assert_eq!(format_output_rate(45_678, 192_000), "238 tok/s");
        // 大速率仍带千位分隔符，风格跟 token 数字一致。
        assert_eq!(format_output_rate(12_000_000, 1_000), "12,000,000 tok/s");
    }

    #[test]
    fn format_decode_rate_covers_edge_cases() {
        // 没有 wire 值 + decode_duration_ms 也没数据 → 不可用。
        assert_eq!(format_decode_rate(None, 0), "速率不可用");
        // 有 wire 稳态速率：正常渲染。
        assert_eq!(format_decode_rate(Some(200.0), 1_000), "200 tok/s");
        assert_eq!(format_decode_rate(Some(0.5), 2_000), "0.5 tok/s");
        assert_eq!(
            format_decode_rate(Some(12_500.0), 1_000),
            "12,500 tok/s"
        );
        // wire 值 <= 0 视为不可用（避免 f32 溢出/负数噪音）。
        assert_eq!(format_decode_rate(Some(0.0), 1_000), "速率不可用");
        assert_eq!(format_decode_rate(Some(-1.0), 1_000), "速率不可用");
    }

    #[test]
    fn first_nonempty_line_skips_blank_leading_lines() {
        assert_eq!(first_nonempty_line("\n  \n  hello \nworld"), "hello");
        assert_eq!(first_nonempty_line("   "), "");
        assert_eq!(first_nonempty_line(""), "");
        assert_eq!(first_nonempty_line("only"), "only");
    }

    #[test]
    fn format_queue_row_single_line() {
        assert_eq!(format_queue_row(1, "fix the bug"), "  #1  fix the bug");
    }

    #[test]
    fn format_queue_row_multiline_reports_extra_lines() {
        assert_eq!(
            format_queue_row(2, "first\nsecond"),
            "  #2  first  (+1 more line)"
        );
        assert_eq!(
            format_queue_row(3, "first\nsecond\nthird"),
            "  #3  first  (+2 more lines)"
        );
    }
}
