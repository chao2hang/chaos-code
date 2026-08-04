//! Tests for partial-failure handling in the dual-fetch `/usage` overlay.
//!
//! The overlay opens with `Loading`, then fills in as `x.ai/session/usage`
//! and `x.ai/usage/aggregate` return. When only one of the two fails we
//! want a graceful degrade (show what we have plus a dim note) rather than
//! blanking the whole popup. Both failing collapses to `Failed(combined)`.

use super::super::status::{
    fill_aggregate_usage_detail, fill_aggregate_usage_detail_failed, fill_session_usage_detail,
    fill_session_usage_detail_failed,
};
use super::*;
use crate::views::usage_detail::UsageDetail;
use xai_grok_shell::extensions::notification::{PromptUsage, PromptUsageModel};

fn empty_usage() -> PromptUsage {
    PromptUsage::default()
}

fn usage_with_calls(n: u64) -> PromptUsage {
    PromptUsage {
        totals: PromptUsageModel {
            model_calls: n,
            ..PromptUsageModel::default()
        },
        ..PromptUsage::default()
    }
}

fn ready_both(session: PromptUsage, aggregate: PromptUsage) -> UsageDetail {
    UsageDetail::Ready {
        session: Some(Box::new(session)),
        aggregate: Some(Box::new(aggregate)),
        partial_failure: None,
    }
}

fn sid() -> acp::SessionId {
    acp::SessionId::new("sess-1")
}

fn detail(app: &AppView) -> &UsageDetail {
    app.agents
        .get(&AgentId(0))
        .and_then(|a| a.usage_detail.as_ref())
        .expect("overlay state")
}

/// Build an app with the given overlay state, pre-bound to a single agent
/// whose `session_id` matches `sid()` so the dispatch handlers don't drop
/// the message on the session-mismatch check.
fn app_with(detail: UsageDetail) -> AppView {
    let mut app = test_app_with_agent();
    if let Some(agent) = app.agents.get_mut(&AgentId(0)) {
        agent.session.session_id = Some(sid());
        agent.usage_detail = Some(detail);
    }
    app
}

#[test]
fn session_failure_with_aggregate_ready_keeps_overlay() {
    let mut app = app_with(ready_both(empty_usage(), usage_with_calls(5)));
    fill_session_usage_detail_failed(&mut app, AgentId(0), &sid(), "boom".into());
    match detail(&app) {
        UsageDetail::Ready {
            session,
            aggregate,
            partial_failure,
        } => {
            assert!(session.is_none(), "session must be None after failure");
            assert!(aggregate.is_some(), "aggregate must survive");
            assert!(partial_failure.is_some(), "partial note required");
        }
        other => panic!("expected Ready partial, got {other:?}"),
    }
}

#[test]
fn aggregate_failure_with_session_ready_keeps_overlay() {
    let mut app = app_with(ready_both(usage_with_calls(7), empty_usage()));
    fill_aggregate_usage_detail_failed(&mut app, AgentId(0), "boom".into());
    match detail(&app) {
        UsageDetail::Ready {
            session,
            aggregate,
            partial_failure,
        } => {
            assert!(session.is_some(), "session must survive");
            assert!(aggregate.is_none(), "aggregate must be None after failure");
            assert!(partial_failure.is_some());
        }
        other => panic!("expected Ready partial, got {other:?}"),
    }
}

#[test]
fn both_fail_collapses_to_failed_overlay() {
    let mut app = app_with(ready_both(empty_usage(), empty_usage()));
    fill_session_usage_detail_failed(&mut app, AgentId(0), &sid(), "session-err".into());
    fill_aggregate_usage_detail_failed(&mut app, AgentId(0), "agg-err".into());
    match detail(&app) {
        UsageDetail::Failed(msg) => {
            assert!(
                msg.contains("session-err") && msg.contains("agg-err"),
                "combined error must include both, got: {msg}"
            );
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn both_fail_from_loading_collapses_in_either_order() {
    let mut session_first = app_with(UsageDetail::Loading);
    fill_session_usage_detail_failed(
        &mut session_first,
        AgentId(0),
        &sid(),
        "session-first".into(),
    );
    fill_aggregate_usage_detail_failed(&mut session_first, AgentId(0), "aggregate-second".into());
    assert!(
        matches!(detail(&session_first), UsageDetail::Failed(message)
            if message.contains("session-first") && message.contains("aggregate-second"))
    );

    let mut aggregate_first = app_with(UsageDetail::Loading);
    fill_aggregate_usage_detail_failed(&mut aggregate_first, AgentId(0), "aggregate-first".into());
    fill_session_usage_detail_failed(
        &mut aggregate_first,
        AgentId(0),
        &sid(),
        "session-second".into(),
    );
    assert!(
        matches!(detail(&aggregate_first), UsageDetail::Failed(message)
            if message.contains("aggregate-first") && message.contains("session-second"))
    );
}

#[test]
fn repeated_single_side_failure_does_not_claim_pending_side_failed() {
    let mut app = app_with(UsageDetail::Loading);
    fill_session_usage_detail_failed(&mut app, AgentId(0), &sid(), "first".into());
    fill_session_usage_detail_failed(&mut app, AgentId(0), &sid(), "retry".into());
    match detail(&app) {
        UsageDetail::Ready {
            session,
            aggregate,
            partial_failure,
        } => {
            assert!(session.is_none());
            assert!(aggregate.is_none(), "aggregate is still pending");
            let note = partial_failure.as_deref().expect("session failure note");
            assert!(note.contains("retry"));
            assert!(!note.contains("累计用量加载失败"));
        }
        other => panic!("same-side repeat must remain partial Ready, got {other:?}"),
    }
}

#[test]
fn session_failure_while_loading_stashes_partial_note() {
    // Session failed while aggregate is still in flight. A later aggregate
    // success must be able to fill in without losing the partial note
    // context.
    let mut app = app_with(UsageDetail::Loading);
    fill_session_usage_detail_failed(&mut app, AgentId(0), &sid(), "session-err".into());
    match detail(&app) {
        UsageDetail::Ready {
            session,
            aggregate,
            partial_failure,
        } => {
            assert!(session.is_none());
            assert!(aggregate.is_none(), "aggregate still pending");
            assert!(partial_failure.is_some());
        }
        other => panic!("expected Ready with note, got {other:?}"),
    }
}

#[test]
fn later_success_clears_matching_partial_note_segment() {
    // Session failed first (note = "本次会话用量加载失败：transient"), then a
    // *late* session success arrives. The note's session portion must be
    // cleared, leaving Ready with both Some.
    let mut app = app_with(ready_both(empty_usage(), usage_with_calls(2)));
    fill_session_usage_detail_failed(&mut app, AgentId(0), &sid(), "transient".into());
    let note_before = match detail(&app) {
        UsageDetail::Ready {
            partial_failure, ..
        } => partial_failure.clone(),
        _ => unreachable!(),
    };
    assert!(note_before.is_some());

    // Late success for the session side.
    fill_session_usage_detail(&mut app, AgentId(0), &sid(), usage_with_calls(99));
    match detail(&app) {
        UsageDetail::Ready {
            session,
            aggregate,
            partial_failure,
        } => {
            assert!(session.is_some(), "late success must fill session");
            assert!(aggregate.is_some());
            assert!(
                partial_failure.is_none(),
                "session-side note should be cleared: was {partial_failure:?}"
            );
        }
        other => panic!("expected Ready no-note, got {other:?}"),
    }
}

#[test]
fn first_session_success_does_not_fabricate_aggregate_usage() {
    let mut app = app_with(UsageDetail::Loading);
    fill_session_usage_detail(&mut app, AgentId(0), &sid(), usage_with_calls(1));
    match detail(&app) {
        UsageDetail::Ready {
            session,
            aggregate,
            partial_failure,
        } => {
            assert_eq!(
                session.as_ref().map(|usage| usage.totals.model_calls),
                Some(1)
            );
            assert!(aggregate.is_none(), "aggregate request is still pending");
            assert!(partial_failure.is_none());
        }
        other => panic!("expected session-only Ready, got {other:?}"),
    }
}

#[test]
fn first_aggregate_success_does_not_fabricate_session_usage() {
    let mut app = app_with(UsageDetail::Loading);
    fill_aggregate_usage_detail(&mut app, AgentId(0), usage_with_calls(2));
    match detail(&app) {
        UsageDetail::Ready {
            session,
            aggregate,
            partial_failure,
        } => {
            assert!(session.is_none(), "session request is still pending");
            assert_eq!(
                aggregate.as_ref().map(|usage| usage.totals.model_calls),
                Some(2)
            );
            assert!(partial_failure.is_none());
        }
        other => panic!("expected aggregate-only Ready, got {other:?}"),
    }
}

#[test]
fn fresh_overlay_with_both_successes_keeps_distinct_ledgers() {
    let mut app = app_with(UsageDetail::Loading);
    fill_session_usage_detail(&mut app, AgentId(0), &sid(), usage_with_calls(1));
    fill_aggregate_usage_detail(&mut app, AgentId(0), usage_with_calls(2));
    match detail(&app) {
        UsageDetail::Ready {
            session,
            aggregate,
            partial_failure,
        } => {
            assert_eq!(
                session.as_ref().map(|usage| usage.totals.model_calls),
                Some(1)
            );
            assert_eq!(
                aggregate.as_ref().map(|usage| usage.totals.model_calls),
                Some(2)
            );
            assert!(partial_failure.is_none());
        }
        other => panic!("expected Ready no-note, got {other:?}"),
    }
}

#[test]
fn failure_notes_preserve_semicolons_and_clear_only_matching_side() {
    let mut app = app_with(UsageDetail::Loading);
    fill_session_usage_detail_failed(&mut app, AgentId(0), &sid(), "session; detail".into());
    fill_aggregate_usage_detail_failed(&mut app, AgentId(0), "aggregate; detail".into());
    match detail(&app) {
        UsageDetail::Failed(message) => {
            assert!(message.contains("session; detail"), "{message}");
            assert!(message.contains("aggregate; detail"), "{message}");
        }
        other => panic!("expected combined Failed, got {other:?}"),
    }
}
