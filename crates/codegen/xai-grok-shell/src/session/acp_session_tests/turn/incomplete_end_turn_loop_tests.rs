//! Coverage of the incomplete-`end_turn` retry wrapper on the real turn loop.
//!
//! The detector itself is unit-tested in `acp_session_impl/incomplete_end_turn.rs`;
//! what has to be proven here is that the shipped wrapper sits on the live prompt
//! path at all: that a qualifying round (read-only tool, then a plan-shaped
//! `end_turn`) actually re-submits with the recovery reminder injected, and that the
//! `[session] auto_retry_incomplete_end_turn` switch gates it.

use super::rate_limit_backoff_tests::{SessionKind, actor_under_test, pump_local_tasks};
use super::support::test_agent_with_tools;
use super::*;
use std::time::Duration;
use xai_grok_test_support::MockInferenceServer;
use xai_grok_test_support::MockModelEntry;
use xai_grok_test_support::ScriptedResponse;
use xai_grok_test_support::sse::responses_api_reasoning_then_tool_call_events;
use xai_grok_test_support::sse::responses_api_script_exact;

/// The turn future needs a session-sized stack (spawn.rs: 8 MiB); default test stacks overflow.
pub(super) fn on_session_stack(test: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(test)
        .expect("spawn test thread")
        .join()
        .expect("test thread panicked");
}

pub(super) fn run_realtime<F: std::future::Future>(fut: impl FnOnce() -> F) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    let local = tokio::task::LocalSet::new();
    rt.block_on(local.run_until(async move {
        fut().await;
    }));
}

/// No sampler-internal retries: request counts map 1:1 to turn submissions.
fn sampler_no_retries() -> xai_grok_sampler::RetryPolicy {
    xai_grok_sampler::RetryPolicy {
        max_retries: 0,
        rate_limit_retry_threshold: xai_grok_sampler::RATE_LIMIT_RETRY_DISABLED,
        ..Default::default()
    }
}

/// `TurnOutcome` carries a boxed snapshot and is not `Debug`; tests need the variant name.
fn outcome_label(outcome: &Result<TurnOutcome, agent_client_protocol::Error>) -> &'static str {
    match outcome {
        Ok(TurnOutcome::Completed { .. }) => "Ok(Completed)",
        Ok(TurnOutcome::Cancelled { .. }) => "Ok(Cancelled)",
        Ok(TurnOutcome::MaxTurnsReached { .. }) => "Ok(MaxTurnsReached)",
        Ok(TurnOutcome::StationarityEnded { .. }) => "Ok(StationarityEnded)",
        Err(_) => "Err(inference terminal)",
    }
}

/// Plan-only trailing message: reads as "I'll change it next", not as a summary.
/// The detector's `IntentWithoutWrite` heuristic keys on 接下来 / 我会 / 改成中文.
const PLAN_ONLY_TEXT: &str = "接下来我会把配置文件改成中文，然后再跑一遍测试。";

/// The same script for both arms of the flag: read one file (read-only tool, so the
/// round is write-free), then end the turn on a plan.
async fn enqueue_plan_only_round(server: &MockInferenceServer, read_target: &std::path::Path) {
    let args = serde_json::json!({ "target_file": read_target.display().to_string() }).to_string();
    server.enqueue_response(
        "/v1/responses",
        ScriptedResponse::sse(responses_api_reasoning_then_tool_call_events(
            "read the file first",
            "call_read_1",
            "read_file",
            &args,
            "test",
        )),
    );
    server.enqueue_response(
        "/v1/responses",
        ScriptedResponse::sse(responses_api_script_exact(PLAN_ONLY_TEXT, "test")),
    );
    // The reply the retry samples, if the retry happens.
    server.enqueue_response(
        "/v1/responses",
        ScriptedResponse::sse(responses_api_script_exact("已修改完毕。", "test")),
    );
}

async fn run_plan_only_turn(
    auto_retry_incomplete_end_turn: bool,
) -> (
    Result<TurnOutcome, agent_client_protocol::Error>,
    usize,
    Vec<String>,
    usize,
) {
    let server = MockInferenceServer::start_with_models(vec![MockModelEntry::new("test")])
        .await
        .expect("mock inference server");
    let dir = tempfile::tempdir().expect("tempdir");
    let read_target = dir.path().join("notes.txt");
    std::fs::write(&read_target, "first line\nsecond line\n").expect("write fixture file");

    let (actor, _retries) = actor_under_test(
        &server,
        SessionKind::Main,
        sampler_no_retries(),
        false,
        auto_retry_incomplete_end_turn,
    )
    .await;
    *actor.agent.borrow_mut() = test_agent_with_tools(vec![
        xai_grok_tools::registry::types::ToolConfig::for_tool::<
            xai_grok_tools::implementations::grok_build::ReadFileTool,
        >(),
    ])
    .await;
    assert_eq!(
        actor.registered_tool_names().await,
        vec!["read_file".to_string()],
        "the scripted tool must be registered for the round to execute it"
    );
    enqueue_plan_only_round(&server, &read_target).await;

    let requests_before = server.request_count();
    let outcome = tokio::time::timeout(
        Duration::from_secs(300),
        actor.process_conversation_turn_with_incomplete_end_turn_retry(
            "req-incomplete-end-turn",
            None,
            None,
            None,
            &mut length_salvage::LengthSalvage::new(None),
        ),
    )
    .await
    .expect("turn must finish within timeout");
    pump_local_tasks().await;

    let submissions = usize::try_from(server.request_count() - requests_before)
        .expect("request delta fits usize");
    let conversation = actor.chat_state_handle.get_conversation().await;
    let tools_called = match &outcome {
        Ok(TurnOutcome::Completed { tools_called, .. }) => tools_called.clone(),
        other => {
            assert!(
                other.is_ok(),
                "the turn must complete, not error: {}",
                outcome_label(other)
            );
            Vec::new()
        }
    };
    let recovery_rows = conversation
        .iter()
        .filter(|item| {
            matches!(
                item,
                ConversationItem::User(user)
                    if user.synthetic_reason
                        == Some(xai_grok_sampling_types::SyntheticReason::AutoRecovery)
            )
        })
        .count();
    (outcome, submissions, tools_called, recovery_rows)
}

#[test]
fn the_prompt_loop_enters_through_the_retry_wrapper() {
    // The loop tests above call the wrapper directly, so they cannot see the call
    // site being re-pointed at the unwrapped recovery function. This pins the seam.
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/session/acp_session_impl/turn.rs"
    ))
    .expect("turn.rs is readable from the crate source");

    let wrapper_sig =
        "pub(super) async fn process_conversation_turn_with_incomplete_end_turn_retry(";
    let wrapper_at = src
        .find(wrapper_sig)
        .expect("the retry wrapper is defined in turn.rs");
    let wrapper_body = &src[wrapper_at..];
    let wrapper_end = wrapper_body[wrapper_sig.len()..]
        .find("\n    #[tracing::instrument")
        .or_else(|| {
            wrapper_body[wrapper_sig.len()..]
                .find("\n    pub(super) async fn ")
                .or_else(|| wrapper_body[wrapper_sig.len()..].find("\n    async fn "))
        })
        .map(|rel| wrapper_at + wrapper_sig.len() + rel)
        .expect("the wrapper is followed by another method");
    let wrapper_span = wrapper_at..wrapper_end;

    let raw_call = ".process_conversation_turn_with_recovery(";
    let outside: Vec<usize> = src
        .match_indices(raw_call)
        .map(|(at, _)| at)
        .filter(|at| !wrapper_span.contains(at))
        .collect();
    assert!(
        outside.is_empty(),
        "the prompt loop must not sample through the unwrapped recovery function; \
         raw calls outside the wrapper at byte offsets {outside:?} \
         (lines {:?})",
        outside
            .iter()
            .map(|at| src[..*at].lines().count())
            .collect::<Vec<_>>()
    );
    assert!(
        src[wrapper_span.clone()].contains(raw_call),
        "the wrapper must actually call the recovery function it wraps"
    );
    assert!(
        src[..wrapper_span.start]
            .contains(".process_conversation_turn_with_incomplete_end_turn_retry("),
        "something before the wrapper's own definition must call the wrapper"
    );
}

#[test]
fn incomplete_end_turn_retry_resubmits_with_the_recovery_reminder() {
    on_session_stack(|| {
        run_realtime(|| async {
            let (outcome, submissions, tools_called, recovery_rows) =
                run_plan_only_turn(true).await;

            assert!(
                matches!(
                    &outcome,
                    Ok(TurnOutcome::Completed {
                        stop: CompletedStop::EndTurn,
                        ..
                    })
                ),
                "the retried round must still end on end_turn: {}",
                outcome_label(&outcome)
            );
            assert_eq!(
                tools_called,
                Vec::<String>::new(),
                "the returned round is the retry, which called no tools; the plan-only round's tools are its own"
            );
            assert_eq!(
                submissions, 3,
                "tool round + plan-only round + one resubmit after the reminder; \
                 2 means the retry never fired, 4+ means it looped past the detector"
            );
            assert_eq!(
                recovery_rows, 1,
                "exactly one auto-recovery reminder must be in the conversation"
            );
        })
    });
}

#[test]
fn incomplete_end_turn_retry_is_inert_when_the_switch_is_off() {
    on_session_stack(|| {
        run_realtime(|| async {
            let (_outcome, submissions, tools_called, recovery_rows) =
                run_plan_only_turn(false).await;

            assert_eq!(
                tools_called,
                vec!["read_file".to_string()],
                "the switch-off arm must return the plan-only round itself"
            );
            assert_eq!(submissions, 2, "tool round + plan-only round, no resubmit");
            assert_eq!(
                recovery_rows, 0,
                "no reminder may be injected while the switch is off"
            );
        })
    });
}

#[test]
fn a_tool_free_round_never_triggers_the_incomplete_end_turn_retry() {
    on_session_stack(|| {
        run_realtime(|| async {
            let server = MockInferenceServer::start_with_models(vec![MockModelEntry::new("test")])
                .await
                .expect("mock inference server");
            server.enqueue_response(
                "/v1/responses",
                ScriptedResponse::sse(responses_api_script_exact(PLAN_ONLY_TEXT, "test")),
            );

            let (actor, _retries) = actor_under_test(
                &server,
                SessionKind::Main,
                sampler_no_retries(),
                false,
                true,
            )
            .await;
            let requests_before = server.request_count();
            let outcome = tokio::time::timeout(
                Duration::from_secs(300),
                actor.process_conversation_turn_with_incomplete_end_turn_retry(
                    "req-no-tools",
                    None,
                    None,
                    None,
                    &mut length_salvage::LengthSalvage::new(None),
                ),
            )
            .await
            .expect("turn must finish within timeout");
            pump_local_tasks().await;

            let submissions = usize::try_from(server.request_count() - requests_before)
                .expect("request delta fits usize");
            let tools_called = match &outcome {
                Ok(TurnOutcome::Completed { tools_called, .. }) => tools_called.clone(),
                other => panic!("expected a completed turn: {}", outcome_label(other)),
            };
            let recovery_rows = actor
                .chat_state_handle
                .get_conversation()
                .await
                .iter()
                .filter(|item| {
                    matches!(
                        item,
                        ConversationItem::User(user)
                            if user.synthetic_reason
                                == Some(xai_grok_sampling_types::SyntheticReason::AutoRecovery)
                    )
                })
                .count();

            assert!(
                tools_called.is_empty(),
                "no tool ran, so the round must report no tools"
            );
            assert_eq!(submissions, 1, "a tool-free round is not retry-eligible");
            assert_eq!(recovery_rows, 0, "no reminder for a tool-free round");
        })
    });
}
