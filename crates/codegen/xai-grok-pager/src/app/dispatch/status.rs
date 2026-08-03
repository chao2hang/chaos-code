//! Session status, sharing, privacy, usage, and info dispatchers.

use agent_client_protocol as acp;

use super::ctx::get_active_agent;
use super::settings::ui::refresh_open_settings_modals;
use crate::app::actions::Effect;
use crate::app::agent::AgentId;
use crate::app::agent_view::AgentView;
use crate::app::app_view::{ActiveView, AppView};
use crate::notifications::{NotificationEvent, NotificationEventKind};
use crate::scrollback::block::RenderBlock;

/// Share the current session via a public URL.
///
/// Produces Effect::ShareSession which spawns an async ACP ext request.
/// On completion, TaskResult::ShareSessionComplete shows the URL in scrollback.
pub(super) fn dispatch_share_session(app: &mut AppView) -> Vec<Effect> {
    if !app.sharing_enabled {
        app.show_toast("Sharing is disabled");
        return vec![];
    }
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    let Some(session_id) = agent.session.session_id.clone() else {
        // No active session — error should have been caught by slash command,
        // but guard here just in case.
        return vec![];
    };

    vec![Effect::ShareSession {
        agent_id: id,
        session_id,
    }]
}

/// Show session info: fetch via x.ai/session/info and display in scrollback.
///
/// Produces Effect::ShowSessionInfo which spawns an async ACP ext request.
/// On completion, TaskResult::SessionInfoComplete shows the formatted info.
pub(super) fn dispatch_show_session_info(app: &mut AppView) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    let Some(session_id) = agent.session.session_id.clone() else {
        // No active session — error should have been caught by slash command,
        // but guard here just in case.
        return vec![];
    };

    vec![Effect::ShowSessionInfo {
        agent_id: id,
        session_id,
        show_resolved_model: app.show_resolved_model,
    }]
}

/// Show privacy and data retention status as a system message in scrollback.
///
/// Three-state display: Enterprise ZDR, coding data sharing opted out,
/// or opted in. Labels align with `CODING_DATA_SHARING_CHOICES` in
/// `settings/defs.rs` and the `coding_data_sharing_toast` format.
///
/// Also lists config knobs that `/privacy` does not change (technical
/// pointers only; no policy claims).
pub(super) fn dispatch_show_privacy_info(app: &mut AppView) -> Vec<Effect> {
    let mut lines = Vec::new();

    if app.is_zdr {
        // Enterprise ZDR -- the team has disabled retention entirely.
        lines.push("  Zero Data Retention: enabled");
        lines.push("  Your data is not retained or used for training (ZDR enabled).");
    } else if app.coding_data_retention_opt_out {
        // Coding data sharing opted out -- matches desktop's "Privacy mode" state.
        lines.push("  Privacy: privacy mode");
        lines.push("  Your code data will not be trained on or used to improve the product.");
        lines.push("");
        lines.push("  Use /privacy opt-in to share data and help improve the product.");
    } else {
        // Coding data sharing opted in -- matches desktop's "Share data" state.
        lines.push("  Privacy: share data");
        lines.push("  Usage and code data may be used by SpaceXAI to improve the product.");
        lines.push("");
        lines.push("  Use /privacy opt-out to enable privacy mode.");
    }

    // Config keys only; do not describe retention/training/analytics policy here.
    lines.push("");
    lines.push("  Other settings (not changed by /privacy):");
    lines.push("  - [features] telemetry / GROK_TELEMETRY_ENABLED");
    lines.push("  - [telemetry] trace_upload / GROK_TELEMETRY_TRACE_UPLOAD");
    lines.push("  - GROK_EXTERNAL_OTEL / OTEL_*");
    lines.push("");
    lines.push("  Learn more: https://x.ai/legal");
    let text = lines.join("\n");
    push_system_to_any_agent(app, &text);
    vec![]
}

/// State-only mutation for `coding_data_sharing`. SHELL-owned.
pub(super) fn set_coding_data_sharing_inner(app: &mut AppView, opted_in: bool) {
    app.coding_data_retention_opt_out = !opted_in;
}

/// Set coding-data-sharing preference. SHELL-owned, auth-metadata-backed
/// (persists via ACP ext-request, NOT `~/.grok/config.toml`).
pub(super) fn set_coding_data_sharing(app: &mut AppView, opted_in: bool) -> Vec<Effect> {
    // ── Guard 1: Enterprise ZDR ──────────────────────────────────────
    if app.is_zdr {
        app.show_toast("\u{2717} Cannot change: Zero Data Retention enabled");
        return vec![];
    }
    // ── Guard 2: Non-admin team member ───────────────────────────────
    if app.team_name.is_some() {
        let is_admin = app
            .team_role
            .as_deref()
            .is_some_and(|r| r.eq_ignore_ascii_case("admin"));
        if !is_admin {
            app.show_toast("\u{2717} Data sharing is controlled by your team admin");
            return vec![];
        }
    }
    // ── Guard 3: an agent must exist to thread the ACP call through ──
    let agent_id = match app.active_view {
        crate::app::app_view::ActiveView::Agent(id) => id,
        _ => match app.agents.keys().next().copied() {
            Some(id) => id,
            None => {
                tracing::warn!(
                    target: "settings",
                    key = "coding_data_sharing",
                    opted_in,
                    "set_coding_data_sharing called with no agents — unreachable in \
                     practice; returning empty (no toast: app.show_toast would no-op)",
                );
                return vec![];
            }
        },
    };

    let prev = !app.coding_data_retention_opt_out;

    // ── Idempotent path: toast but skip the ACP round-trip. ──────────
    if prev == opted_in {
        app.show_toast(&coding_data_sharing_toast(opted_in));
        return vec![];
    }

    // ── Optimistic mutation: state, then UI feedback, then effect. ───
    set_coding_data_sharing_inner(app, opted_in);
    refresh_open_settings_modals(app);
    app.show_toast(&coding_data_sharing_toast(opted_in));

    tracing::info!(
        target: "settings",
        key = "coding_data_sharing",
        opted_in,
        "setting changed",
    );

    vec![Effect::SetCodingDataSharing {
        agent_id,
        opted_in,
        rollback_to_opted_in: prev,
    }]
}

/// Format the `Coding data sharing` toast. Asymmetric: opt-in
/// (privacy-degrading) uses ⚠ + consequence text; opt-out (safe
/// default) uses ✓. Uses display names from the registry catalog.
pub(super) fn coding_data_sharing_toast(opted_in: bool) -> String {
    let display = display_for_coding_data_sharing_canonical(opted_in);
    if opted_in {
        // Privacy-degrading: warn glyph + spelled-out consequence.
        format!(
            "\u{26A0} Coding data sharing: {display} \u{2014} code samples may be retained \
             for training"
        )
    } else {
        // Safe default — uniform ✓ glyph.
        format!("\u{2713} Coding data sharing: {display}")
    }
}

/// Display string for the canonical bool. Keep aligned with
/// `CODING_DATA_SHARING_CHOICES` in `settings/defs.rs`.
fn display_for_coding_data_sharing_canonical(opted_in: bool) -> &'static str {
    if opted_in { "Opt in" } else { "Opt out" }
}

/// Scrub an untrusted error string for toast display. Substitutes a
/// generic placeholder when the input exceeds 120 chars or contains
/// control / bidi-override characters (prevents escape-sequence
/// injection and visual spoofing). Full error stays in tracing logs.
pub(super) fn scrub_error_for_toast(error: &str) -> String {
    const MAX_TOAST_ERROR_LEN: usize = 120;
    if error.len() > MAX_TOAST_ERROR_LEN
        || error
            .chars()
            .any(crate::render::line_utils::is_unsafe_display_char)
    {
        "server error (see logs for details)".to_string()
    } else {
        error.to_string()
    }
}

/// Push a system message to the active agent's scrollback, or to any available
/// agent if on the welcome screen.
fn push_system_to_any_agent(app: &mut AppView, msg: &str) {
    let block = crate::scrollback::block::RenderBlock::system(msg.to_string());
    if let ActiveView::Agent(id) = app.active_view
        && let Some(agent) = app.agents.get_mut(&id)
    {
        agent.scrollback.push_block(block);
        return;
    }
    if let Some(agent) = app.agents.values_mut().next() {
        agent.scrollback.push_block(block);
    }
}

/// Show context info: fetch via x.ai/session/info and display rich breakdown.
///
/// Produces Effect::ShowContextInfo which spawns an async ACP ext request.
/// On completion, TaskResult::ContextInfoComplete shows the formatted info.
pub(super) fn dispatch_show_context_info(app: &mut AppView) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    let Some(session_id) = agent.session.session_id.clone() else {
        return vec![];
    };

    vec![Effect::ShowContextInfo {
        agent_id: id,
        session_id,
    }]
}

/// Dynamically set the session context window size.
pub(super) fn dispatch_set_context_window(
    app: &mut AppView,
    tokens: u64,
    compact_if_needed: bool,
) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    let Some(session_id) = agent.session.session_id.clone() else {
        // No live session: still update local UI override so status bar reflects it.
        agent.session.models.override_context_window(tokens);
        agent
            .scrollback
            .push_block(crate::scrollback::block::RenderBlock::system(format!(
                "上下文窗口已设为 {}（本地预览；会话建立后生效并可能压缩）",
                format_token_count(tokens)
            )));
        return vec![];
    };

    // Optimistic UI update so status bar /context shows the new window immediately.
    agent.session.models.override_context_window(tokens);

    vec![Effect::SetContextWindow {
        agent_id: id,
        session_id,
        tokens,
        compact_if_needed,
    }]
}

fn format_token_count(n: u64) -> String {
    if n >= 1_000_000 && n.is_multiple_of(1_000_000) {
        format!("{}M", n / 1_000_000)
    } else if n >= 1_000 && n.is_multiple_of(1_000) {
        format!("{}K", n / 1_000)
    } else if n >= 10_000 {
        format!("{:.0}K", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

pub(super) fn handle_set_context_window_complete(
    app: &mut AppView,
    agent_id: AgentId,
    result: Result<crate::app::actions::SetContextWindowOutcome, String>,
) -> Vec<Effect> {
    let Some(agent) = app.agents.get_mut(&agent_id) else {
        return vec![];
    };
    match result {
        Ok(outcome) => {
            agent.session.models.override_context_window(outcome.tokens);
            let mut msg = format!(
                "上下文窗口: {} → {} · 已用 {} ({}%)",
                format_token_count(outcome.previous_tokens),
                format_token_count(outcome.tokens),
                format_token_count(outcome.tokens_used),
                outcome.usage_percent,
            );
            if outcome.compacted {
                msg.push_str(" · 已压缩对话以适配新窗口");
            } else if outcome.tokens < outcome.previous_tokens
                && outcome.tokens_used > outcome.tokens * 85 / 100
            {
                msg.push_str(" · 用量仍较高，可手动 /compact");
            }
            agent
                .scrollback
                .push_block(crate::scrollback::block::RenderBlock::system(msg));
        }
        Err(error) => {
            agent
                .scrollback
                .push_block(crate::scrollback::block::RenderBlock::system(format!(
                    "设置上下文窗口失败: {error}"
                )));
        }
    }
    vec![]
}

/// `/usage` — session token/cost, then consumer credits when visible.
/// Credits are chained after the session block so layout stays ordered.
pub(super) fn dispatch_show_usage(app: &mut AppView) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let session_id = {
        let Some(agent) = app.agents.get_mut(&id) else {
            return vec![];
        };
        agent.session.session_id.clone()
    };
    match session_id {
        Some(session_id) => vec![Effect::FetchSessionUsage {
            agent_id: id,
            session_id,
            for_overlay: false,
            overlay_generation: None,
        }],
        None => {
            if let Some(agent) = app.agents.get_mut(&id) {
                agent.scrollback.push_block(RenderBlock::system(
                    "Session usage is unavailable until the session starts.".to_string(),
                ));
            }
            append_consumer_billing_surface(app, id)
        }
    }
}

/// Click on the accumulated-token status chip — open the usage detail overlay
/// and fetch the ledgers into it.
///
/// The overlay opens immediately in `Loading` so the click is acknowledged on
/// the next frame rather than after the round-trip. A second click while the
/// overlay is open closes it (the chip is a toggle, like the goal chip), and
/// any in-flight fetch then lands on a closed overlay and is dropped.
///
/// Two ledgers are fetched in parallel:
///   - the current session's usage (`x.ai/session/usage`)
///   - the all-time aggregate usage across every Chaos session
///     (`x.ai/usage/aggregate`)
pub(super) fn dispatch_show_usage_detail(app: &mut AppView) -> Vec<Effect> {
    use crate::views::usage_detail::UsageDetail;

    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    if agent.usage_detail.is_some() {
        agent.close_usage_detail();
        return vec![];
    }

    agent.usage_detail_generation = agent.usage_detail_generation.wrapping_add(1);
    let generation = agent.usage_detail_generation;
    agent.usage_detail = if agent.session.session_id.is_some() {
        Some(UsageDetail::Loading)
    } else {
        Some(UsageDetail::Ready {
            session: None,
            aggregate: None,
            partial_failure: Some("本次会话用量加载失败：会话尚未开始".to_string()),
        })
    };

    let mut effects = vec![Effect::FetchAggregateUsage {
        agent_id: id,
        for_overlay: true,
        overlay_generation: Some(generation),
    }];
    if let Some(session_id) = agent.session.session_id.clone() {
        effects.push(Effect::FetchSessionUsage {
            agent_id: id,
            session_id,
            for_overlay: true,
            overlay_generation: Some(generation),
        });
    }
    effects
}

/// Commit a session-usage block if still on `session_id`, then consumer credits.
pub(super) fn commit_session_usage_block(
    app: &mut AppView,
    agent_id: AgentId,
    session_id: &acp::SessionId,
    text: String,
) -> Vec<Effect> {
    let Some(agent) = app.agents.get_mut(&agent_id) else {
        return vec![];
    };
    if agent.session.session_id.as_ref() != Some(session_id) {
        return vec![];
    }
    agent.scrollback.push_block(RenderBlock::system(text));
    append_consumer_billing_surface(app, agent_id)
}

/// Merge a fetched session ledger into the usage detail overlay.
///
/// Dropped when the agent is gone, the session was rebound under the fetch, or
/// the user already dismissed the overlay — a late result must never re-open a
/// popup the user closed. Unlike [`commit_session_usage_block`] this does not
/// chain the consumer billing surface: the overlay is a token/cost read-out,
/// not the `/usage` command flow.
///
/// Single-side success overwrites `session`, drops the matching part of the
/// partial-failure note, and preserves the aggregate side intact. If it is
/// the first result to arrive, the aggregate side remains `None` until its
/// own request completes; we never duplicate one ledger into both columns.
pub(super) fn usage_overlay_generation_is_current(
    app: &AppView,
    agent_id: AgentId,
    generation: Option<u64>,
) -> bool {
    let Some(generation) = generation else {
        return false;
    };
    app.agents.get(&agent_id).is_some_and(|agent| {
        agent.usage_detail.is_some() && agent.usage_detail_generation == generation
    })
}

pub(super) fn fill_session_usage_detail(
    app: &mut AppView,
    agent_id: AgentId,
    session_id: &acp::SessionId,
    usage: xai_grok_shell::extensions::notification::PromptUsage,
) -> Vec<Effect> {
    let Some(agent) = app.agents.get_mut(&agent_id) else {
        return vec![];
    };
    if agent.session.session_id.as_ref() != Some(session_id) {
        return vec![];
    }
    if agent.usage_detail.is_none() {
        return vec![];
    }
    match agent.usage_detail.take() {
        Some(crate::views::usage_detail::UsageDetail::Ready {
            aggregate,
            partial_failure,
            ..
        }) => {
            // Session arrived; clear any "session failed" portion of the
            // partial note (it was the leading segment), keep the rest.
            let trimmed = partial_failure.and_then(strip_session_failure_note);
            agent.usage_detail = Some(crate::views::usage_detail::UsageDetail::Ready {
                session: Some(Box::new(usage)),
                aggregate,
                partial_failure: trimmed,
            });
        }
        Some(crate::views::usage_detail::UsageDetail::Loading) => {
            agent.usage_detail = Some(crate::views::usage_detail::UsageDetail::Ready {
                session: Some(Box::new(usage)),
                aggregate: None,
                partial_failure: None,
            });
        }
        Some(failed @ crate::views::usage_detail::UsageDetail::Failed(_)) => {
            // Don't recover a Failed overlay with a late single-side success:
            // both sides were already untrustworthy.
            agent.usage_detail = Some(failed);
        }
        None => unreachable!("is_none guard above prevents this"),
    }
    vec![]
}

/// Mark the session-ledger portion of the overlay as failed.
///
/// If the aggregate ledger is still pending or already arrived, we degrade
/// to `Ready { session: None, ... }` with a single-line `partial_failure`
/// note instead of blanking the whole popup. We only fall back to
/// `Failed(error)` when the aggregate side had already failed before us.
pub(super) fn fill_session_usage_detail_failed(
    app: &mut AppView,
    agent_id: AgentId,
    session_id: &acp::SessionId,
    error: String,
) -> Vec<Effect> {
    let Some(agent) = app.agents.get_mut(&agent_id) else {
        return vec![];
    };
    if agent.session.session_id.as_ref() != Some(session_id) {
        return vec![];
    }
    if agent.usage_detail.is_none() {
        return vec![];
    }
    match agent.usage_detail.take() {
        Some(crate::views::usage_detail::UsageDetail::Ready {
            aggregate,
            partial_failure,
            ..
        }) => {
            let aggregate_failed = partial_failure
                .as_deref()
                .is_some_and(|note| failure_note_has_side(note, Side::Aggregate));
            let new_note = merge_partial_failure(partial_failure, Side::Session, &error);
            if aggregate_failed {
                // The aggregate side already failed, so this is the second
                // independent failure and the overlay can collapse.
                agent.usage_detail =
                    Some(crate::views::usage_detail::UsageDetail::Failed(new_note));
            } else {
                // Aggregate is either ready or still pending. Keep the partial
                // overlay open rather than treating `None` as a failure.
                agent.usage_detail = Some(crate::views::usage_detail::UsageDetail::Ready {
                    session: None,
                    aggregate,
                    partial_failure: Some(new_note),
                });
            }
        }
        Some(crate::views::usage_detail::UsageDetail::Loading) => {
            // Session failed while aggregate still pending: stash a partial
            // note so a later aggregate success can replace it cleanly.
            agent.usage_detail = Some(crate::views::usage_detail::UsageDetail::Ready {
                session: None,
                aggregate: None,
                partial_failure: Some(format!("本次会话用量加载失败：{error}")),
            });
        }
        Some(failed @ crate::views::usage_detail::UsageDetail::Failed(_)) => {
            agent.usage_detail = Some(failed);
        }
        None => unreachable!("is_none guard above prevents this"),
    }
    vec![]
}

/// Merge the all-time aggregate ledger into the usage detail overlay.
///
/// Mirror of [`fill_session_usage_detail`] for the aggregate side.
pub(super) fn fill_aggregate_usage_detail(
    app: &mut AppView,
    agent_id: AgentId,
    usage: xai_grok_shell::extensions::notification::PromptUsage,
) -> Vec<Effect> {
    let Some(agent) = app.agents.get_mut(&agent_id) else {
        return vec![];
    };
    if agent.usage_detail.is_none() {
        return vec![];
    }
    match agent.usage_detail.take() {
        Some(crate::views::usage_detail::UsageDetail::Ready {
            session,
            partial_failure,
            ..
        }) => {
            let trimmed = partial_failure.and_then(strip_aggregate_failure_note);
            agent.usage_detail = Some(crate::views::usage_detail::UsageDetail::Ready {
                session,
                aggregate: Some(Box::new(usage)),
                partial_failure: trimmed,
            });
        }
        Some(crate::views::usage_detail::UsageDetail::Loading) => {
            agent.usage_detail = Some(crate::views::usage_detail::UsageDetail::Ready {
                session: None,
                aggregate: Some(Box::new(usage)),
                partial_failure: None,
            });
        }
        Some(failed @ crate::views::usage_detail::UsageDetail::Failed(_)) => {
            agent.usage_detail = Some(failed);
        }
        None => unreachable!("is_none guard above prevents this"),
    }
    vec![]
}

/// Mark the aggregate-ledger portion of the overlay as failed.
///
/// Mirror of [`fill_session_usage_detail_failed`] for the aggregate side.
pub(super) fn fill_aggregate_usage_detail_failed(
    app: &mut AppView,
    agent_id: AgentId,
    error: String,
) -> Vec<Effect> {
    let Some(agent) = app.agents.get_mut(&agent_id) else {
        return vec![];
    };
    if agent.usage_detail.is_none() {
        return vec![];
    }
    match agent.usage_detail.take() {
        Some(crate::views::usage_detail::UsageDetail::Ready {
            session,
            partial_failure,
            ..
        }) => {
            let session_failed = partial_failure
                .as_deref()
                .is_some_and(|note| failure_note_has_side(note, Side::Session));
            let new_note = merge_partial_failure(partial_failure, Side::Aggregate, &error);
            if session_failed {
                agent.usage_detail =
                    Some(crate::views::usage_detail::UsageDetail::Failed(new_note));
            } else {
                // Session is either ready or still pending; a missing value by
                // itself is not evidence that the request failed.
                agent.usage_detail = Some(crate::views::usage_detail::UsageDetail::Ready {
                    session,
                    aggregate: None,
                    partial_failure: Some(new_note),
                });
            }
        }
        Some(crate::views::usage_detail::UsageDetail::Loading) => {
            agent.usage_detail = Some(crate::views::usage_detail::UsageDetail::Ready {
                session: None,
                aggregate: None,
                partial_failure: Some(format!("累计用量加载失败：{error}")),
            });
        }
        Some(failed @ crate::views::usage_detail::UsageDetail::Failed(_)) => {
            agent.usage_detail = Some(failed);
        }
        None => unreachable!("is_none guard above prevents this"),
    }
    vec![]
}

/// Which side a partial-failure note refers to.
#[derive(Debug, Clone, Copy)]
enum Side {
    Session,
    Aggregate,
}

impl Side {
    fn label(self) -> &'static str {
        match self {
            Side::Session => "本次会话",
            Side::Aggregate => "累计",
        }
    }
}

fn failure_note_has_side(note: &str, side: Side) -> bool {
    let prefix = format!("{}用量加载失败：", side.label());
    note.starts_with(&prefix) || note.contains(&format!("; {prefix}"))
}

/// Append `prefix: <error>` to `existing`. The caller already decided which
/// side this note is for, so we just build the single-side message.
fn merge_partial_failure(existing: Option<String>, side: Side, error: &str) -> String {
    let mine = format!("{}用量加载失败：{}", side.label(), error);
    let other = match side {
        Side::Session => Side::Aggregate,
        Side::Aggregate => Side::Session,
    };
    match existing.and_then(|note| strip_side_failure_note(note, side)) {
        Some(prev) if prev.starts_with(&format!("{}用量加载失败：", other.label())) => {
            match side {
                Side::Session => format!("{mine}; {prev}"),
                Side::Aggregate => format!("{prev}; {mine}"),
            }
        }
        _ => mine,
    }
}

/// Drop the leading "本次会话用量加载失败：…" segment from a partial note
/// so a successful session-arrival call clears just the session portion.
fn strip_session_failure_note(note: String) -> Option<String> {
    strip_side_failure_note(note, Side::Session)
}

/// Drop the leading "累计用量加载失败：…" segment.
fn strip_aggregate_failure_note(note: String) -> Option<String> {
    strip_side_failure_note(note, Side::Aggregate)
}

/// Strip the leading `<side>用量加载失败：<err>` segment. Returns:
///   * `None` if the note was *only* the session-side segment (so clearing
///     that side leaves the overlay note-free);
///   * `Some(rest)` if a trailing aggregate-side segment survives, or the
///     note didn't start with the side's prefix at all (we pass it through
///     unchanged — clearing a side that didn't own the prefix is a no-op
///     but shouldn't drop unrelated notes).
fn strip_side_failure_note(note: String, side: Side) -> Option<String> {
    let prefix = format!("{}用量加载失败：", side.label());
    let other = match side {
        Side::Session => Side::Aggregate,
        Side::Aggregate => Side::Session,
    };
    let other_prefix = format!("{}用量加载失败：", other.label());
    let side_marker = format!("; {prefix}");
    let other_marker = format!("; {other_prefix}");

    if note.starts_with(&prefix) {
        return note
            .find(&other_marker)
            .map(|idx| note[idx + 2..].to_string());
    }
    if note.starts_with(&other_prefix)
        && let Some(idx) = note.find(&side_marker)
    {
        return Some(note[..idx].to_string());
    }
    Some(note)
}

/// Consumer credit follow-up for `/usage` (redirect or non-silent billing fetch).
pub(super) fn append_consumer_billing_surface(app: &mut AppView, agent_id: AgentId) -> Vec<Effect> {
    if !app.usage_visible {
        return vec![];
    }
    // Remote-settings kill switch (`grok_build_usage_redirect_url`): link out
    // instead of fetching billing from the backend.
    if let Some(url) = app.usage_billing_redirect_url.clone() {
        if let Some(agent) = app.agents.get_mut(&agent_id) {
            agent.scrollback.push_block(RenderBlock::System(
                crate::scrollback::blocks::SystemMessageBlock::new(format!(
                    "Please check your usage on {url}"
                )),
            ));
        }
        return vec![];
    }
    if !app.agents.contains_key(&agent_id) {
        return vec![];
    }
    // Non-silent: the effect also pulls the auto top-up rule so the summary
    // renders usage, prepaid credits, and auto top-up together.
    vec![Effect::FetchBilling {
        agent_id,
        silent: false,
    }]
}

/// `/usage manage` — open consumer billing. No-op when the surface is hidden.
pub(super) fn dispatch_manage_billing(app: &mut AppView) -> Vec<Effect> {
    if !app.usage_visible {
        return vec![];
    }
    super::router::dispatch(crate::app::actions::Action::OpenUrl(String::new()), app)
}

/// Commit a one-line "update available" notice into the active agent's
/// scrollback. Minimal mode has no welcome screen (the full TUI's update
/// surface), so the background update check's result is shown here instead
/// No-op when there is no active agent.
pub(crate) fn commit_minimal_update_notice(app: &mut AppView, latest_version: &str) {
    if let ActiveView::Agent(id) = app.active_view
        && let Some(agent) = app.agents.get_mut(&id)
    {
        agent.scrollback.push_block(RenderBlock::system(format!(
            "Update available: v{latest_version} — restart to apply."
        )));
    }
}

/// `/queue` — commit a read-only list of the queued prompts as a system block.
/// The text is built by [`crate::app::status_blocks::queue_block_text`]; this
/// just resolves the active agent and pushes it. Works in every render mode; the
/// primary inspection surface in minimal, which has no interactive `QueuePane`.
pub(super) fn dispatch_show_queue(app: &mut AppView) -> Vec<Effect> {
    if let ActiveView::Agent(id) = app.active_view
        && let Some(agent) = app.agents.get_mut(&id)
    {
        let text = crate::app::status_blocks::queue_block_text(agent);
        agent.scrollback.push_block(RenderBlock::system(text));
    }
    vec![]
}

/// `/tasks` — commit a read-only list of background tasks, subagents, and
/// scheduled (`/loop`) tasks as a system block. The text is built by
/// [`crate::app::status_blocks::tasks_block_text`]; this just resolves the
/// active agent and pushes it. Works in every render mode; the primary snapshot
/// surface in minimal, which has no interactive `TasksPane`.
pub(super) fn dispatch_show_tasks(app: &mut AppView) -> Vec<Effect> {
    if let ActiveView::Agent(id) = app.active_view
        && let Some(agent) = app.agents.get_mut(&id)
    {
        let text = crate::app::status_blocks::tasks_block_text(agent);
        agent.scrollback.push_block(RenderBlock::system(text));
    }
    vec![]
}

/// Open the hidden `/gboom` easter egg as a modal over the active agent
/// view. Requires a graphics-capable terminal (kitty protocol or iTerm2);
/// otherwise a toast explains why nothing happened. On session-less
/// surfaces (dashboard, welcome) this is a silent no-op.
///
/// Targets the top-level agent view (where the prompt lives), not a
/// focused subagent view: the modal's tick/draw plumbing runs on the
/// top-level view, mirroring the video viewer.
pub(super) fn dispatch_open_gboom(app: &mut AppView) -> Vec<Effect> {
    use crate::terminal::image::{GraphicsProtocol, detect_graphics_protocol};
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    if detect_graphics_protocol() == GraphicsProtocol::None {
        agent.show_toast(
            "No demons here \u{2014} GBOOM needs a graphics-capable terminal \
             (kitty, Ghostty, WezTerm, iTerm2)",
        );
        return vec![];
    }
    // Close other media modals: they share the kitty placement id. Drop the
    // image viewer's in-flight loader too (its close path clears both —
    // a leaked rx would mis-feed the next image viewer's poll loop).
    agent.image_viewer = None;
    agent.image_load_rx = None;
    agent.video_viewer = None;
    agent.gboom = Some(crate::gboom::GboomState::new());
    vec![]
}

/// Open the onboarding tutorial overlay (top-level modal — works over both
/// the welcome screen and an agent session). Toggles: dispatching while
/// open closes instead of stacking.
pub(super) fn dispatch_open_tutorial(app: &mut AppView) -> Vec<Effect> {
    // Minimal mode has no modal host: the overlay would render nothing
    // while the app-level intercept swallowed all input.
    if app.screen_mode.is_minimal() {
        return vec![];
    }
    if app.tutorial.is_some() {
        app.tutorial = None;
        return vec![];
    }
    app.tutorial = Some(crate::views::tutorial::TutorialState::new());
    vec![]
}

/// Emit a `SessionReady` notification for the given agent.
///
/// Takes `&NotificationService` separately from `&AgentView` to avoid
/// borrow-checker conflicts when `agent` is borrowed from `app.agents`.
pub(super) fn notify_session_ready(
    notification_service: &crate::notifications::NotificationService,
    agent: &AgentView,
) {
    notification_service.notify(NotificationEvent {
        kind: NotificationEventKind::SessionReady,
        title: "Chaos".into(),
        body: "会话已就绪".into(),
        session_id: agent.session.session_id.as_ref().map(|s| s.0.to_string()),
    });
}

// TaskResult handlers.

pub(super) fn handle_coding_data_sharing_updated(
    app: &mut AppView,
    agent_id: AgentId,
    opted_in: bool,
) -> Vec<Effect> {
    // Re-anchor mirror to server-confirmed value (defense-in-
    // depth against server reshaping the boolean). `agent_id`
    // discarded — privacy is app-level, not per-agent.
    set_coding_data_sharing_inner(app, opted_in);
    refresh_open_settings_modals(app);
    // Re-toast on confirmation. Without this, a slow ACP
    // round-trip would leave the user with only the
    // optimistic toast (already faded) and no
    // server-confirmed feedback.
    app.show_toast(&coding_data_sharing_toast(opted_in));
    tracing::info!(
        target: "settings",
        key = "coding_data_sharing",
        ?agent_id,
        opted_in,
        "ACP update confirmed; mirror re-anchored",
    );
    vec![]
}

pub(super) fn handle_coding_data_sharing_failed(
    app: &mut AppView,
    agent_id: AgentId,
    error: String,
    rollback_to_opted_in: bool,
) -> Vec<Effect> {
    // Revert optimistic mutation: inner → refresh → toast.
    //
    // `agent_id` discarded — privacy is global.
    set_coding_data_sharing_inner(app, rollback_to_opted_in);
    refresh_open_settings_modals(app);
    // Scrub long/unsafe error strings before toasting.
    let scrubbed = scrub_error_for_toast(&error);
    app.show_toast(&format!(
        "\u{2717} Couldn't update coding data sharing: {scrubbed}"
    ));
    tracing::warn!(
        target: "settings",
        key = "coding_data_sharing",
        ?agent_id,
        rollback_to_opted_in,
        %error,
        "ACP update failed; reverted optimistic mutation",
    );
    vec![]
}

pub(super) fn handle_context_info_complete(
    app: &mut AppView,
    agent_id: AgentId,
    info: Box<xai_grok_shell::session::SessionInfoResponse>,
) -> Vec<Effect> {
    if let Some(agent) = app.agents.get_mut(&agent_id) {
        let model = info.data.model.as_deref().unwrap_or("unknown").to_string();
        // Take ownership of the snapshot once, hand a clone to the
        // agent's running counters, then move the original into the
        // scrollback block (which keeps it for theme-reactive
        // re-rendering). This still costs one clone but reads as
        // "the agent needs a copy" rather than "the block needs a
        // copy", which matches the lifetime story.
        let snapshot = info.data.context;
        agent.apply_full_context_info(snapshot.clone());
        agent
            .scrollback
            .push_block(crate::scrollback::block::RenderBlock::context_info(
                snapshot, model,
            ));
    }
    vec![]
}

// Action handlers.

pub(super) fn dispatch_copy_session_id(app: &mut AppView, index: usize) -> Vec<Effect> {
    use crate::views::modal::ActiveModal;
    // Try agent modal first, then fall back to app fields (welcome screen).
    let id = get_active_agent(app)
        .and_then(|agent| {
            if let Some(ActiveModal::SessionPicker {
                entries: Some(ref e),
                ..
            }) = agent.active_modal
            {
                e.get(index).map(|entry| entry.id.clone())
            } else {
                None
            }
        })
        .or_else(|| {
            app.session_picker_entries
                .as_ref()
                .and_then(|s| s.get(index))
                .map(|e| e.id.clone())
        });
    if let Some(id) = id {
        let delivery = crate::clipboard::copy_text_or_file(&id);
        app.show_toast(delivery.toast_message().as_ref());
    }
    vec![]
}

pub(super) fn dispatch_show_release_notes(
    app: &mut AppView,
    title: String,
    content: String,
) -> Vec<Effect> {
    match app.active_view {
        ActiveView::Agent(id) => {
            if let Some(agent) = app.agents.get_mut(&id) {
                agent.active_modal = Some(crate::views::modal::ActiveModal::DocViewer {
                    title,
                    content,
                    scroll: 0,
                    window: crate::views::modal_window::ModalWindowState::new(),
                    cached_lines: None,
                    previous_palette: None,
                    standalone: true,
                });
            }
        }
        ActiveView::Welcome => {
            app.welcome_doc_viewer = Some(crate::views::modal::ActiveModal::DocViewer {
                title,
                content,
                scroll: 0,
                window: crate::views::modal_window::ModalWindowState::new(),
                cached_lines: None,
                previous_palette: None,
                standalone: true,
            });
        }
        _ => {}
    }
    vec![]
}
