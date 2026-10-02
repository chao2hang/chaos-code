//! DCP wiring on the real turn loop.
//!
//! `[compaction] strategy` decides whether the session advertises the
//! model-driven `compress` tool at all, and a `compress` call must be executed
//! by the session (creating a compression block) instead of reaching the tool
//! bridge as an unknown tool.

use super::incomplete_end_turn_loop_tests::{on_session_stack, run_realtime};
use super::rate_limit_backoff_tests::{SessionKind, actor_under_test_with, pump_local_tasks};
use super::*;
use crate::session::acp_session::selective_compaction::COMPRESS_TOOL_NAME;
use crate::session::dcp_config::CompactionStrategy;
use std::time::Duration;
use xai_grok_test_support::MockInferenceServer;
use xai_grok_test_support::MockModelEntry;
use xai_grok_test_support::ScriptedResponse;
use xai_grok_test_support::sse::responses_api_reasoning_then_tool_call_events;
use xai_grok_test_support::sse::responses_api_script_exact;

/// No sampler-internal retries: the scripted responses map 1:1 to submissions.
fn sampler_no_retries() -> xai_grok_sampler::RetryPolicy {
    xai_grok_sampler::RetryPolicy {
        max_retries: 0,
        rate_limit_retry_threshold: xai_grok_sampler::RATE_LIMIT_RETRY_DISABLED,
        ..Default::default()
    }
}

/// An old read round that is safe to compress: the pair is closed, its output is
/// long, and nothing after it depends on the raw text.
fn seed_compressible_history(actor: &Arc<SessionActor>) {
    use xai_grok_sampling_types::conversation::ToolCall;
    let long_output = "compiled artifact line 0123456789abcdef\n".repeat(240);
    actor.chat_state_handle.replace_conversation(vec![
        ConversationItem::system("system prompt"),
        ConversationItem::user("first task"),
        ConversationItem::assistant_tool_calls(vec![ToolCall {
            id: Arc::from("call_old_read"),
            name: "read_file".to_string(),
            arguments: Arc::from(r#"{"target_file":"build.log"}"#),
        }]),
        ConversationItem::tool_result("call_old_read", long_output),
        ConversationItem::assistant("noted"),
        ConversationItem::user("second task"),
    ]);
}

/// The `compress` call the model makes: indices 2..=3 are the read round above.
fn compress_arguments() -> String {
    serde_json::json!({
        "topic": "旧的构建日志探索",
        "ranges": [{
            "start": 2,
            "end": 3,
            "summary": "读取了 build.log，确认全部为已通过的编译输出，无需修改。"
        }]
    })
    .to_string()
}

struct CompressTurn {
    /// Whether the request the model actually saw offered `compress`.
    advertised: bool,
    /// Compression blocks the session committed.
    active_blocks: usize,
    /// The `tool_result` the session fed back for the compress call.
    compress_results: Vec<String>,
}

async fn run_compress_call(strategy: CompactionStrategy) -> CompressTurn {
    let server = MockInferenceServer::start_with_models(vec![MockModelEntry::new("test")])
        .await
        .expect("mock inference server");
    let (actor, _retries) = actor_under_test_with(
        &server,
        SessionKind::Main,
        sampler_no_retries(),
        false,
        |actor| {
            actor.compaction.strategy = strategy;
            // Automatic strategies must not be able to create the block under test.
            actor.compaction.dcp.strategies_enabled = false;
            // Keep the seeded read round compressible regardless of how the
            // fixture counts turns.
            actor.compaction.dcp.protected.turn_protection = 0;
            actor.compaction.dcp.protected.protect_user_messages = false;
            actor.compaction.dcp.protected.protected_tools.clear();
        },
    )
    .await;
    seed_compressible_history(&actor);

    server.enqueue_response(
        "/v1/responses",
        ScriptedResponse::sse(responses_api_reasoning_then_tool_call_events(
            "compress the old read",
            "call_compress_1",
            "compress",
            &compress_arguments(),
            "test",
        )),
    );
    server.enqueue_response(
        "/v1/responses",
        ScriptedResponse::sse(responses_api_script_exact(
            "已基于压缩后的上下文继续。",
            "test",
        )),
    );

    let outcome = tokio::time::timeout(
        Duration::from_secs(300),
        actor.process_conversation_turn_with_recovery(
            "req-dcp-compress",
            None,
            None,
            None,
            &mut length_salvage::LengthSalvage::new(None),
        ),
    )
    .await
    .expect("turn must finish within timeout");
    pump_local_tasks().await;
    assert!(
        matches!(outcome, Ok(TurnOutcome::Completed { .. })),
        "the compress round must complete the turn, not abort it: {:?}",
        match &outcome {
            Ok(_) => "non-completed outcome",
            Err(_) => "terminal error",
        }
    );

    // Only the request's `tools` array counts: the model's own `compress`
    // tool-call is echoed back inside later request bodies as conversation
    // history, so a whole-body text search would be a false positive.
    let advertised = server.request_bodies().iter().any(|body| {
        body.get("tools")
            .and_then(|tools| tools.as_array())
            .is_some_and(|tools| {
                tools.iter().any(|tool| {
                    tool.get("name")
                        .or_else(|| tool.get("function").and_then(|f| f.get("name")))
                        .and_then(|name| name.as_str())
                        == Some(COMPRESS_TOOL_NAME)
                })
            })
    });
    let conversation = actor.chat_state_handle.get_conversation().await;
    let compress_results = conversation
        .iter()
        .filter_map(|item| match item {
            ConversationItem::ToolResult(result) => {
                let text = item.text_content();
                (result.tool_call_id == "call_compress_1").then_some(text)
            }
            _ => None,
        })
        .collect();
    let active_blocks = actor
        .chat_state_handle
        .get_selective_compaction()
        .await
        .active_blocks()
        .count();
    CompressTurn {
        advertised,
        active_blocks,
        compress_results,
    }
}

#[test]
fn dynamic_strategy_offers_compress_and_the_session_executes_it() {
    on_session_stack(|| {
        run_realtime(|| async {
            let turn = run_compress_call(CompactionStrategy::Dynamic).await;
            assert!(
                turn.advertised,
                "strategy=dynamic must put the compress tool in the request"
            );
            assert_eq!(
                turn.active_blocks, 1,
                "the session must execute the compress call itself; a 0 means the \
                 call fell through to the tool bridge as an unknown tool"
            );
            assert_eq!(
                turn.compress_results.len(),
                1,
                "the model must get a tool_result back for its compress call"
            );
            assert!(
                turn.compress_results[0].contains("已创建"),
                "the result should report the created blocks, got {:?}",
                turn.compress_results[0]
            );
        })
    });
}

#[test]
fn the_default_strategy_never_offers_or_executes_compress() {
    on_session_stack(|| {
        run_realtime(|| async {
            let turn = run_compress_call(CompactionStrategy::Threshold).await;
            assert!(
                !turn.advertised,
                "the shipped default must not advertise the model-driven compress tool"
            );
            assert_eq!(
                turn.active_blocks, 0,
                "a strategy that is off must not commit compression"
            );
        })
    });
}

/// The `compress` definition, the nudges and the dispatch are only reachable
/// when the strategy enables DCP; pin the seams so the driver cannot be
/// orphaned again and cannot leak into the shipped default configuration.
#[test]
fn the_turn_loop_calls_the_dcp_hooks_only_behind_the_switch() {
    let read = |rel: &str| {
        std::fs::read_to_string(format!(
            "{}/src/session/acp_session_impl/{rel}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap_or_else(|e| panic!("{rel} is readable from the crate source: {e}"))
    };
    let turn = read("turn.rs");
    let driver = read("selective_compaction.rs");

    // Every session-side DCP hook in the turn loop sits behind the switch.
    for hook in [
        "self.maybe_inject_selective_compaction_nudge()",
        "selective_compaction::compress_tool_definition()",
        "self.run_session_compress_calls(&mut tool_calls)",
    ] {
        let at = turn
            .find(hook)
            .unwrap_or_else(|| panic!("turn.rs must drive {hook}"));
        let window = &turn[at.saturating_sub(700)..at];
        assert!(
            window.contains("self.compaction.dcp_active()"),
            "{hook} must be gated by `self.compaction.dcp_active()`"
        );
    }

    // The dispatch hook is the only path to the executor, so gating the one
    // call site in turn.rs gates the whole subsystem.
    let exec = "self.execute_compress_tool(call)";
    assert_eq!(
        driver.matches(exec).count(),
        1,
        "execute_compress_tool must have exactly one caller, otherwise a call site \
         could bypass the strategy switch"
    );
    let dispatch_at = driver
        .find("async fn run_session_compress_calls")
        .expect("run_session_compress_calls is defined in the driver");
    assert!(
        driver[dispatch_at..].contains(exec),
        "the single execute_compress_tool call must live inside run_session_compress_calls"
    );
    assert!(
        !turn.contains(exec),
        "turn.rs must go through run_session_compress_calls, not the executor directly"
    );
}
