//! Plan-mode missing-exit guard: a plan-mode turn that ends with plain
//! text — the model neither called `exit_plan_mode` nor `ask_user_question`
//! — used to silently strand the session in plan mode (the user saw "计划已
//! 就绪，退出计划模式" and then nothing). `handle_completion` now queues a
//! synthetic `plan-missing-exit-nudge-*` turn carrying the missing-exit
//! reminder, bounded per activation by `MAX_MISSING_EXIT_NUDGES`; once the
//! budget is spent it posts a visible notice and hands control back.

use super::support::*;
use super::*;

use tokio::sync::mpsc;

async fn nudge_actor() -> (
    SessionActor,
    mpsc::UnboundedReceiver<SessionEvent>,
    mpsc::UnboundedReceiver<PersistenceMsg>,
) {
    let (gateway_tx, _) = mpsc::unbounded_channel();
    let (persistence_tx, persistence_rx) = mpsc::unbounded_channel();
    let (actor, event_rx) = create_test_actor_ex(0, 256_000, 85, gateway_tx, persistence_tx).await;
    (actor, event_rx, persistence_rx)
}

/// A minimal front pending input matching `prompt_id`, mirroring the helper
/// in `turn_completion_emit_tests`. `queue_meta` is `None` so
/// `handle_completion` does not also broadcast a `queue/changed`.
fn pending_input(prompt_id: &str) -> InputItem {
    let (respond_to, _rx) = oneshot::channel();
    InputItem {
        prompt_id: prompt_id.to_string(),
        prompt_blocks: vec![],
        prompt_mode: PromptMode::Agent,
        trace_gcs_config: None,
        artifact_tracker: None,
        client_identifier: None,
        screen_mode: None,
        verbatim: false,
        json_schema: None,
        origin: crate::session::PromptOrigin::User,
        task_wake_fallback: None,
        tool_overrides_update: None,
        respond_to,
        persist_ack: None,
        parsed_prompt_tx: None,
        queue_meta: None,
        send_now: false,
    }
}

/// Queue `prompt_id` as the running front and complete it with a plain
/// `Completed` + `EndTurn` — the "model replied with text only" shape that
/// strands plan mode. Returns whether the completion was owned.
async fn run_plain_turn(actor: &SessionActor, prompt_id: &str) -> bool {
    *actor
        .current_prompt_id
        .lock()
        .expect("current_prompt_id mutex poisoned") = Some(prompt_id.to_string());
    actor
        .state
        .lock()
        .await
        .pending_inputs
        .push_back(pending_input(prompt_id));
    actor
        .handle_completion(
            prompt_id.to_string(),
            crate::session::commands::ok_end_turn(0, None),
        )
        .await
}

/// Complete the already-queued front prompt with a plain EndTurn (the nudge
/// queued by the guard needs no re-push) and return its prompt id.
async fn run_queued_front_plain(actor: &SessionActor) -> String {
    let front_id = {
        let state = actor.state.lock().await;
        state
            .pending_inputs
            .front()
            .expect("a queued front prompt")
            .prompt_id
            .clone()
    };
    *actor
        .current_prompt_id
        .lock()
        .expect("current_prompt_id mutex poisoned") = Some(front_id.clone());
    assert!(
        actor
            .handle_completion(
                front_id.clone(),
                crate::session::commands::ok_end_turn(0, None),
            )
            .await,
        "the queued nudge turn must own its completion"
    );
    front_id
}

async fn queued_prompt_ids(actor: &SessionActor) -> Vec<String> {
    actor
        .state
        .lock()
        .await
        .pending_inputs
        .iter()
        .map(|i| i.prompt_id.clone())
        .collect()
}

/// Collect the text of every `AgentMessageChunk` the actor has emitted so far.
fn agent_message_texts(rx: &mut mpsc::UnboundedReceiver<SessionEvent>) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(SessionEvent::Notification(notification)) = rx.try_recv() {
        let SessionNotification::Acp(notification) = notification else {
            continue;
        };
        if let acp::SessionUpdate::AgentMessageChunk(chunk) = &notification.update
            && let acp::ContentBlock::Text(text) = &chunk.content
        {
            out.push(text.text.clone());
        }
    }
    out
}

/// The failure shape this guard exists for: plan mode active, the model
/// ends its turn with plain text, no closing tool — the shell queues a
/// synthetic nudge turn instead of stranding the session.
#[tokio::test(flavor = "current_thread")]
async fn plain_text_plan_turn_queues_missing_exit_nudge() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (actor, _event_rx, _persistence_rx) = nudge_actor().await;
            assert!(actor.plan_mode.lock().activate_from_tool());

            assert!(run_plain_turn(&actor, "p1").await);

            let queued = queued_prompt_ids(&actor).await;
            assert_eq!(queued.len(), 1, "exactly one nudge turn queued: {queued:?}");
            assert!(
                queued[0].starts_with("plan-missing-exit-nudge-"),
                "queued turn is the synthetic nudge: {}",
                queued[0]
            );
            let origin = crate::session::PromptOrigin::from_prompt_id(&queued[0]);
            assert!(origin.is_synthetic());
            assert!(origin.hide_user_echo_from_scrollback());
            assert_eq!(
                actor.plan_mode.lock().missing_exit_nudge_count(),
                1,
                "the nudge spend is recorded on the tracker"
            );
            // The nudge carries the rendered missing-exit reminder.
            let state = actor.state.lock().await;
            let nudge = state.pending_inputs.front().expect("nudge still queued");
            let acp::ContentBlock::Text(text) = &nudge.prompt_blocks[0] else {
                panic!("nudge prompt is a single text block");
            };
            assert!(text.text.contains("Plan mode is still active"));
            assert_eq!(nudge.prompt_mode, PromptMode::Plan);
        })
        .await;
}

/// The retry loop is self-driving but bounded: the nudge turn ending plain
/// re-arms exactly one more nudge, and the NEXT plain ending spends the
/// budget — a visible notice replaces a third nudge.
#[tokio::test(flavor = "current_thread")]
async fn repeated_plain_turns_hit_the_nudge_cap_and_notify() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (actor, mut event_rx, _persistence_rx) = nudge_actor().await;
            assert!(actor.plan_mode.lock().activate_from_tool());

            assert!(run_plain_turn(&actor, "p1").await);
            let nudge1 = run_queued_front_plain(&actor).await;
            let nudge2 = run_queued_front_plain(&actor).await;
            assert!(nudge1.starts_with("plan-missing-exit-nudge-"));
            assert!(nudge2.starts_with("plan-missing-exit-nudge-"));
            assert_ne!(nudge1, nudge2, "nudge prompt ids are unique");
            assert_eq!(
                actor.plan_mode.lock().missing_exit_nudge_count(),
                crate::session::plan_mode::MAX_MISSING_EXIT_NUDGES + 1,
            );

            // Budget spent: no third nudge, and the user is told.
            assert!(
                queued_prompt_ids(&actor).await.is_empty(),
                "cap reached — control returns to the user"
            );
            let notices = agent_message_texts(&mut event_rx);
            assert!(
                notices.iter().any(|t| t.contains("Plan mode is still on")),
                "a visible notice explains the give-up: {notices:?}"
            );
        })
        .await;
}

/// A turn that touched a closing tool (`exit_plan_mode` requested — even if
/// the user rejected it — or `ask_user_question`) is a proper plan-mode
/// ending; no nudge.
#[tokio::test(flavor = "current_thread")]
async fn turn_with_a_closing_tool_does_not_nudge() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (actor, _event_rx, _persistence_rx) = nudge_actor().await;
            assert!(actor.plan_mode.lock().activate_from_tool());
            actor
                .turn_had_exit_or_ask
                .store(true, std::sync::atomic::Ordering::Relaxed);

            assert!(run_plain_turn(&actor, "p1").await);

            assert!(queued_prompt_ids(&actor).await.is_empty());
            assert_eq!(actor.plan_mode.lock().missing_exit_nudge_count(), 0);
        })
        .await;
}

/// Cancelled / errored / capped endings never self-wake.
#[tokio::test(flavor = "current_thread")]
async fn non_plain_endings_do_not_nudge() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (actor, _event_rx, _persistence_rx) = nudge_actor().await;
            assert!(actor.plan_mode.lock().activate_from_tool());

            let cancelled = Ok(PromptTurnOk {
                stop_reason: acp::StopReason::Cancelled,
                total_tokens: 0,
                turn_snapshot: None,
                completion_kind: PromptCompletionKind::Cancelled {
                    category: None,
                    context: None,
                },
                structured_output: None,
                usage: None,
                tool_overrides: None,
            });
            *actor
                .current_prompt_id
                .lock()
                .expect("current_prompt_id mutex poisoned") = Some("p-cancel".to_string());
            actor
                .state
                .lock()
                .await
                .pending_inputs
                .push_back(pending_input("p-cancel"));
            assert!(
                actor
                    .handle_completion("p-cancel".to_string(), cancelled)
                    .await
            );
            assert!(
                queued_prompt_ids(&actor).await.is_empty(),
                "a cancelled turn must not nudge"
            );

            // Refusal + EndTurn still isn't `Completed`.
            let refused = Ok(PromptTurnOk {
                stop_reason: acp::StopReason::Refusal,
                total_tokens: 0,
                turn_snapshot: None,
                completion_kind: PromptCompletionKind::Completed,
                structured_output: None,
                usage: None,
                tool_overrides: None,
            });
            *actor
                .current_prompt_id
                .lock()
                .expect("current_prompt_id mutex poisoned") = Some("p-refuse".to_string());
            actor
                .state
                .lock()
                .await
                .pending_inputs
                .push_back(pending_input("p-refuse"));
            assert!(
                actor
                    .handle_completion("p-refuse".to_string(), refused)
                    .await
            );
            assert!(
                queued_prompt_ids(&actor).await.is_empty(),
                "a refused turn must not nudge"
            );
        })
        .await;
}

/// Background wakes (task completions, notification drains, goal summaries)
/// run read-only in plan mode and end with text by design; they must never
/// push the model toward exiting plan mode.
#[tokio::test(flavor = "current_thread")]
async fn other_synthetic_turns_do_not_nudge() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (actor, _event_rx, _persistence_rx) = nudge_actor().await;
            assert!(actor.plan_mode.lock().activate_from_tool());

            assert!(run_plain_turn(&actor, "task-completed-abc").await);
            assert!(run_plain_turn(&actor, "notifications-1").await);
            assert!(run_plain_turn(&actor, "goal-summary-1").await);

            assert!(
                queued_prompt_ids(&actor).await.is_empty(),
                "background wakes must not nudge"
            );
            assert_eq!(actor.plan_mode.lock().missing_exit_nudge_count(), 0);
        })
        .await;
}

/// A user prompt already waiting behind the finished turn reconciles the
/// mode itself — queueing a nudge ahead of it would fire a stale reminder
/// after the user already steered.
#[tokio::test(flavor = "current_thread")]
async fn a_waiting_user_prompt_suppresses_the_nudge() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (actor, _event_rx, _persistence_rx) = nudge_actor().await;
            assert!(actor.plan_mode.lock().activate_from_tool());

            *actor
                .current_prompt_id
                .lock()
                .expect("current_prompt_id mutex poisoned") = Some("p1".to_string());
            {
                let mut state = actor.state.lock().await;
                state.pending_inputs.push_back(pending_input("p1"));
                state.pending_inputs.push_back(pending_input("p2"));
            }
            assert!(
                actor
                    .handle_completion(
                        "p1".to_string(),
                        crate::session::commands::ok_end_turn(0, None),
                    )
                    .await
            );

            assert_eq!(
                queued_prompt_ids(&actor).await,
                vec!["p2".to_string()],
                "only the user's own prompt remains queued"
            );
            assert_eq!(actor.plan_mode.lock().missing_exit_nudge_count(), 0);
        })
        .await;
}

/// Outside plan mode a plain turn is perfectly normal — no guard at all.
#[tokio::test(flavor = "current_thread")]
async fn plain_turn_outside_plan_mode_does_not_nudge() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (actor, _event_rx, _persistence_rx) = nudge_actor().await;
            assert!(run_plain_turn(&actor, "p1").await);
            assert!(queued_prompt_ids(&actor).await.is_empty());
        })
        .await;
}
