//! Session/plan-mode concern for `SessionActor` (`handle_session_mode`,
//! plan-mode reminders and persistence, active-template detection).
use super::*;
pub(super) fn prompt_mode_from_session_mode_id(session_mode_id: &acp::SessionModeId) -> PromptMode {
    use xai_grok_tools::types::SessionMode;
    match SessionMode::from_id(session_mode_id.0.as_ref()) {
        SessionMode::Plan => PromptMode::Plan,
        SessionMode::Ask => PromptMode::Ask,
        SessionMode::Default => PromptMode::Agent,
    }
}
/// Inverse of [`prompt_mode_from_session_mode_id`]: the mode id a client
/// displays for a prompt mode. Needed wherever a transition the client did not
/// drive has to be reported back to it.
pub(super) fn session_mode_id_from_prompt_mode(prompt_mode: PromptMode) -> acp::SessionModeId {
    use xai_grok_tools::types::SessionMode;
    let mode = match prompt_mode {
        PromptMode::Plan => SessionMode::Plan,
        PromptMode::Ask => SessionMode::Ask,
        PromptMode::Agent => SessionMode::Default,
    };
    acp::SessionModeId::new(mode.as_id())
}
/// Pass-through twin: no toolset in this build carries a plan-gated tool.
pub(super) fn filter_cursor_tools_by_plan_mode(
    defs: Vec<ToolDefinition>,
    _plan_active: bool,
) -> Vec<ToolDefinition> {
    defs
}
impl SessionActor {
    pub(super) fn apply_prompt_modes_to_snapshot(&self, snapshot: &mut TurnDeltaSnapshot) {
        snapshot.start_prompt_mode = Some(self.turn_start_prompt_mode.lock().to_string());
        snapshot.end_prompt_mode = Some(self.turn_prompt_mode.lock().to_string());
    }
    /// `false` twin: this template integration is not compiled into this
    /// build, so no session runs it. Keeps ungated call sites compiling in
    /// both configurations.
    pub(super) fn is_cursor_harness(&self) -> bool {
        false
    }
    pub(super) async fn handle_session_mode(&self, session_mode_id: acp::SessionModeId) {
        use xai_grok_tools::types::SessionMode;
        let prompt_mode = prompt_mode_from_session_mode_id(&session_mode_id);
        *self.current_prompt_mode.lock() = prompt_mode;
        let mode = SessionMode::from_id(session_mode_id.0.as_ref());
        if mode.is_plan() {
            let entered = self.plan_mode.lock().enter_pending();
            if entered {
                self.persist_plan_mode_state();
                self.enqueue_current_mode_update(acp::SessionModeId::new(
                    SessionMode::Plan.as_id(),
                ));
            }
            tracing::info!(
                session_id = %self.session_info.id.0,
                entered,
                "Plan mode toggled ON (Pending)"
            );
            let turn_in_flight = self.state.lock().await.running_task.is_some();
            if entered && turn_in_flight {
                self.activate_plan_mode_mid_turn().await;
            }
            xai_grok_telemetry::session_ctx::log_event(
                xai_grok_telemetry::events::PlanModeToggled {
                    enabled: true,
                    trigger: xai_grok_telemetry::events::PlanModeTrigger::User,
                    turn_in_flight,
                    was_previously_active: !entered,
                },
            );
            if entered {
                tracing::info_span!(
                    "session.permission_mode_changed",
                    from_mode =
                        super::telemetry::permission_mode_label(self.permissions.is_yolo_mode()),
                    to_mode = "plan",
                    trigger = "user",
                    enabled = true,
                )
                .in_scope(|| {});
            }
            return;
        }
        let was_plan = {
            let tracker = self.plan_mode.lock();
            tracker.state() != crate::session::plan_mode::PlanModeState::Inactive
        };
        if was_plan {
            let turn_in_flight = self.state.lock().await.running_task.is_some();
            self.plan_mode.lock().user_exit(turn_in_flight);
            self.persist_plan_mode_state();
            self.enqueue_current_mode_update(session_mode_id.clone());
            tracing::info!(
                session_id = %self.session_info.id.0,
                new_mode = %session_mode_id.0,
                turn_in_flight,
                "Plan mode toggled OFF"
            );
            xai_grok_telemetry::session_ctx::log_event(
                xai_grok_telemetry::events::PlanModeToggled {
                    enabled: false,
                    trigger: xai_grok_telemetry::events::PlanModeTrigger::User,
                    turn_in_flight,
                    was_previously_active: true,
                },
            );
            tracing::info_span!(
                "session.permission_mode_changed",
                from_mode = "plan",
                to_mode = %session_mode_id.0,
                trigger = "user",
                enabled = false,
            )
            .in_scope(|| {});
        }
        let agent_def = match session_mode_id.0.as_ref() {
            "browser_use" => Some(AgentDefinition::browser_use()),
            name => {
                let cwd = self.tool_context.cwd.as_path();
                xai_grok_agent::discovery::by_name_in_cwd(name, cwd)
            }
        };
        if let Some(ref def) = agent_def {
            tracing::info!(
                session_id = %self.session_info.id.0,
                agent_name = %def.name,
                agent_scope = %def.scope,
                prompt_mode = ?def.prompt_mode,
                has_completion_req = def.completion_requirement.is_some(),
                tool_configs = def.tool_config.tools.len(),
                "Resolved AgentDefinition for session mode"
            );
            self.agent
                .borrow()
                .update_policies_from_definition(def)
                .await;
            *self.active_agent_type.lock() = Some(def.name.clone());
        }
        if let Some(ref def) = agent_def {
            let new_prompt = self.agent.borrow().render_prompt_for_definition(def).await;
            let mut conversation = self.chat_state_handle.get_conversation().await;
            for item in conversation.iter_mut() {
                if let ConversationItem::System(sys) = item {
                    sys.content = std::sync::Arc::<str>::from(new_prompt);
                    break;
                }
            }
            self.chat_state_handle.replace_conversation(conversation);
        }
    }
    /// Settle the mode a turn runs in, applying the prompt's declaration when
    /// it made one.
    ///
    /// Only a real user turn declares a mode. A synthetic turn — a background
    /// task wake, a goal summary, a notification drain — is constructed
    /// internally with a placeholder `PromptMode::Agent` that reads as "the
    /// user asked for agent mode", so reconciling one ends plan mode just by
    /// waking the session: a background task finishing while you were planning
    /// was enough to do it. Those turns inherit the session's mode instead.
    ///
    /// Returns the resolved mode rather than echoing the argument, so a
    /// synthetic turn is also *recorded* under the mode it really ran in.
    pub(super) fn resolve_turn_prompt_mode(
        &self,
        origin: &crate::session::PromptOrigin,
        declared: PromptMode,
    ) -> PromptMode {
        if !origin.is_synthetic() {
            self.reconcile_plan_mode_with_prompt(declared);
        }
        *self.current_prompt_mode.lock()
    }
    /// Bring the plan-mode tracker into agreement with the prompt's mode.
    ///
    /// Mirrors `handle_session_mode` but driven from `_meta.mode` on the
    /// prompt — the only signal the client sends. Both transitions are
    /// idempotent, so `set_mode`-driven flows are unaffected.
    ///
    /// Like `handle_session_mode`, a real transition here emits a
    /// `CurrentModeUpdate`. Without it a client that carries its mode on the
    /// prompt could enter or leave plan mode with no signal at all — and since
    /// the same line is what lands in `updates.jsonl`, a later replay could not
    /// recover the mode either.
    pub(super) fn reconcile_plan_mode_with_prompt(&self, prompt_mode: PromptMode) {
        use crate::session::plan_mode::PlanModeState;
        *self.current_prompt_mode.lock() = prompt_mode;
        match prompt_mode {
            PromptMode::Plan => {
                let entered = self.plan_mode.lock().enter_pending();
                if entered {
                    self.persist_plan_mode_state();
                    self.enqueue_current_mode_update(session_mode_id_from_prompt_mode(prompt_mode));
                }
            }
            PromptMode::Agent | PromptMode::Ask => {
                let was_plan = {
                    let tracker = self.plan_mode.lock();
                    tracker.state() != PlanModeState::Inactive
                };
                if was_plan {
                    self.plan_mode.lock().user_exit(false);
                    self.persist_plan_mode_state();
                    self.enqueue_current_mode_update(session_mode_id_from_prompt_mode(prompt_mode));
                }
            }
        }
    }
    /// Inject plan mode system-reminders into the conversation.
    ///
    /// Called once per turn from `handle_prompt()`, before the user's actual
    /// message is pushed. Handles three mutually-ordered cases:
    ///
    /// 1. **Pending → Active**: First prompt after user toggled plan mode on.
    ///    Injects the full (or reentry) reminder and transitions to Active.
    /// 2. **Already Active**: Subsequent prompts while plan mode is on.
    ///    Injects an alternating full/sparse per-turn reminder.
    /// 3. **Exit reminder**: One-shot reminder after plan mode was exited.
    ///    Injected once, then the flag is cleared.
    ///
    /// All reminders are pushed as `<system-reminder>`-wrapped user messages
    /// so the model sees them in the same turn as the user's prompt.
    /// Tool names are resolved at render time via `TemplateRenderer`.
    pub(super) async fn inject_plan_mode_reminders(&self) {
        use crate::session::plan_mode::{
            PlanModeState, plan_mode_exit_reminder_template, plan_mode_reminder_full_template,
            plan_mode_reminder_sparse_template,
        };
        let use_cursor_reminders = self.is_cursor_harness();
        let push_reminder = |this: &Self, content: &str| {
            this.push_system_reminder_with_tag(content, this.reminder_wrapper_tag());
        };
        let mut injected_this_turn = false;
        let activation = {
            let tracker = self.plan_mode.lock();
            (tracker.state() == PlanModeState::Pending)
                .then(|| (tracker.is_reentry(), tracker.plan_file_path().to_path_buf()))
        };
        if let Some((is_reentry, plan_path)) = activation {
            self.plan_mode.lock().activate();
            self.persist_plan_mode_state();
            let plan_has_content =
                crate::session::plan_mode::plan_file_has_content(&plan_path).await;
            let template = self.plan_activation_template(is_reentry);
            if let Some(rendered) = self
                .render_plan_template(template, &plan_path, plan_has_content)
                .await
            {
                push_reminder(self, &rendered);
                injected_this_turn = true;
                self.plan_mode.lock().record_reminder_injected();
                self.persist_plan_mode_state();
                tracing::info!(
                    session_id = %self.session_info.id.0,
                    is_reentry,
                    uses_template_reminders = use_cursor_reminders,
                    "Plan mode activated: injected system-reminder"
                );
            }
        }
        if !injected_this_turn {
            let per_turn = {
                let tracker = self.plan_mode.lock();
                tracker.is_active().then(|| {
                    (
                        tracker.should_use_full_reminder(),
                        tracker.plan_file_path().to_path_buf(),
                    )
                })
            };
            if let Some((use_full, plan_path)) = per_turn {
                let plan_has_content =
                    crate::session::plan_mode::plan_file_has_content(&plan_path).await;
                let template = if use_full {
                    plan_mode_reminder_full_template()
                } else {
                    plan_mode_reminder_sparse_template()
                };
                if let Some(rendered) = self
                    .render_plan_template(template, &plan_path, plan_has_content)
                    .await
                {
                    push_reminder(self, &rendered);
                    self.plan_mode.lock().record_reminder_injected();
                    self.persist_plan_mode_state();
                }
            }
        }
        if self.plan_mode.lock().has_pending_exit_reminder() {
            let plan_path = self.plan_mode.lock().plan_file_path().to_path_buf();
            let template = plan_mode_exit_reminder_template();
            if let Some(rendered) = self.render_plan_template(template, &plan_path, false).await {
                push_reminder(self, &rendered);
            }
            self.plan_mode.lock().clear_pending_exit_reminder();
            self.persist_plan_mode_state();
        }
    }
    /// Activate plan mode for a turn that is already running.
    ///
    /// Mid-turn counterpart of `inject_plan_mode_reminders` case 1: the user
    /// toggled plan mode ON (Shift+Tab) while the model was thinking, so the
    /// tracker sits in `Pending` and the running turn would otherwise proceed
    /// without any plan-mode instruction. Activate immediately (so
    /// `is_active()` tool gating applies to subsequent calls) and buffer the
    /// activation reminder on the tracker; `flush_pending_skill_reminders`
    /// delivers it at the running turn's next safe point (loop top / after
    /// each tool batch) — or, if the turn ends first, the cancel/idle flush
    /// lands it for the next turn. Buffering (vs a direct conversation push)
    /// keeps the in-flight batch's tool_result blocks adjacent, and lets a
    /// toggle-off withdraw an undelivered reminder (`user_exit`).
    ///
    /// No-op unless the tracker is `Pending`: `enter_pending`'s
    /// `ExitPending → Active` re-entry needs no reminder (the model already
    /// has plan-mode context and no exit reminder was injected yet).
    ///
    /// A failed template render still activates (without a buffer), keeping
    /// gating in lockstep with the turn-start path.
    pub(super) async fn activate_plan_mode_mid_turn(&self) {
        use crate::session::plan_mode::PlanModeState;
        let activation = {
            let tracker = self.plan_mode.lock();
            (tracker.state() == PlanModeState::Pending)
                .then(|| (tracker.is_reentry(), tracker.plan_file_path().to_path_buf()))
        };
        let Some((is_reentry, plan_path)) = activation else {
            return;
        };
        let plan_has_content = crate::session::plan_mode::plan_file_has_content(&plan_path).await;
        let template = self.plan_activation_template(is_reentry);
        let rendered = self
            .render_plan_template(template, &plan_path, plan_has_content)
            .await;
        let tag = self.reminder_wrapper_tag();
        let buffered = rendered.is_some();
        let activated = match rendered {
            Some(rendered) => self
                .plan_mode
                .lock()
                .activate_mid_turn(format!("<{tag}>\n{rendered}\n</{tag}>")),
            None => {
                tracing::warn!(
                    session_id = %self.session_info.id.0,
                    "Mid-turn plan activation: reminder render failed; \
                     activating without a buffered reminder"
                );
                self.plan_mode.lock().activate()
            }
        };
        if !activated {
            return;
        }
        self.persist_plan_mode_state();
        tracing::info!(
            session_id = %self.session_info.id.0,
            is_reentry,
            buffered,
            "Plan mode activated mid-turn"
        );
    }
    /// The activation reminder template for the active template (no
    /// first-entry/reentry distinction), or grok's reentry/full variant.
    /// Shared by turn-start injection (`inject_plan_mode_reminders` case 1)
    /// and the mid-turn toggle (`activate_plan_mode_mid_turn`).
    fn plan_activation_template(&self, is_reentry: bool) -> &'static str {
        use crate::session::plan_mode::{
            plan_mode_reentry_reminder_template, plan_mode_reminder_full_template,
        };
        if is_reentry {
            plan_mode_reentry_reminder_template()
        } else {
            plan_mode_reminder_full_template()
        }
    }
    /// Render a plan mode template via the tool bridge's `TemplateRenderer`.
    ///
    /// Passes `plan_path` and `plan_has_content` as extra context alongside the
    /// registry's `tools.by_kind.*` mappings.
    pub(super) async fn render_plan_template(
        &self,
        template: &str,
        plan_path: &std::path::Path,
        plan_has_content: bool,
    ) -> Option<String> {
        let extra = serde_json::json!({
            "plan_path": plan_path.display().to_string(),
            "plan_has_content": plan_has_content,
        });
        self.agent
            .borrow()
            .tool_bridge()
            .render_prompt(template, &extra)
            .await
    }
    /// Persist the current plan mode state to disk.
    ///
    /// Called after every state transition so plan mode survives
    /// session reload/resume/reconnect.
    pub(super) fn persist_plan_mode_state(&self) {
        let snapshot = self.plan_mode.lock().snapshot();
        let _ = self
            .notifications
            .persistence_tx
            .send(PersistenceMsg::PlanModeState(snapshot));
    }
    /// Plan-mode missing-exit guard: decide what a just-completed turn
    /// earns. Called from `handle_completion` while the state lock is held
    /// (the queue-depth input), acted on by
    /// [`Self::deliver_plan_missing_exit_nudge`] after it is dropped.
    ///
    /// The guard exists for one failure shape: the model announces "the
    /// plan is ready, exiting plan mode" as PLAIN TEXT and ends the turn
    /// without calling the exit tool — the session then sits in plan mode
    /// with no signal to anyone. Fires only when ALL of these hold:
    ///
    /// - this handler owned the completion (a stale one finalizes nothing);
    /// - the turn finished plainly (`Completed` + `EndTurn`) — cancelled,
    ///   errored, token-capped, and stationarity-ended turns never nudge;
    /// - no further input is already queued (a waiting user prompt will
    ///   reconcile the mode itself);
    /// - the turn was a real user turn — or our own nudge retry. Other
    ///   synthetic turns (background wakes, goal summaries, …) run
    ///   read-only in plan mode and end with text by design; nudging them
    ///   would push the model out of plan mode on a background event;
    /// - the turn called neither `exit_plan_mode` nor `ask_user_question`
    ///   (the per-turn latch set in `prepare_tool_call`);
    /// - plan mode is still active.
    pub(super) fn plan_missing_exit_nudge_decision(
        &self,
        prompt_id: &str,
        result: &PromptTurnResult,
        owned_completion: bool,
        queued_inputs_empty: bool,
    ) -> Option<MissingExitNudge> {
        if !owned_completion || !queued_inputs_empty {
            return None;
        }
        let plain_end_turn = matches!(
            result,
            Ok(PromptTurnOk {
                stop_reason: acp::StopReason::EndTurn,
                completion_kind: PromptCompletionKind::Completed,
                ..
            })
        );
        if !plain_end_turn {
            return None;
        }
        let origin = crate::session::PromptOrigin::from_prompt_id(prompt_id);
        if origin.is_synthetic()
            && !matches!(origin, crate::session::PromptOrigin::PlanMissingExitNudge)
        {
            return None;
        }
        if self
            .turn_had_exit_or_ask
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return None;
        }
        let mut tracker = self.plan_mode.lock();
        if !tracker.is_active() {
            return None;
        }
        let nudge_count = tracker.record_missing_exit_nudge();
        if nudge_count > crate::session::plan_mode::MAX_MISSING_EXIT_NUDGES {
            Some(MissingExitNudge::CapReached)
        } else {
            Some(MissingExitNudge::Nudge {
                nudge_count,
                plan_path: tracker.plan_file_path().to_path_buf(),
            })
        }
    }
    /// Act on a [`MissingExitNudge`] decision: queue the synthetic nudge
    /// turn, or — once the per-activation budget is spent — post a visible
    /// notice and hand control back to the user.
    ///
    /// Only queues the input; starting it is left to the caller's
    /// scheduler kick (`handle_completion`'s completion arm runs
    /// `maybe_start_running_task` right after), so a direct
    /// `handle_completion` caller in tests can observe the queued nudge
    /// without a turn running.
    pub(super) async fn deliver_plan_missing_exit_nudge(&self, action: MissingExitNudge) {
        match action {
            MissingExitNudge::CapReached => {
                tracing::warn!(
                    session_id = %self.session_info.id.0,
                    "Plan-mode missing-exit nudge budget spent; handing control back"
                );
                xai_grok_telemetry::unified_log::warn(
                    "shell.plan_mode.missing_exit_nudge_cap",
                    Some(self.session_info.id.0.as_ref()),
                    None,
                );
                self.send_slash_command_output(
                    "Plan mode is still on, but the model keeps ending its turn with plain \
                     text instead of calling exit_plan_mode. I've stopped auto-reminding it \
                     for this round — reply to steer it, or press Shift+Tab to leave plan mode.",
                )
                .await;
            }
            MissingExitNudge::Nudge {
                nudge_count,
                plan_path,
            } => {
                let plan_has_content =
                    crate::session::plan_mode::plan_file_has_content(&plan_path).await;
                let rendered = self
                    .render_plan_template(
                        crate::session::plan_mode::plan_mode_missing_exit_tool_template(),
                        &plan_path,
                        plan_has_content,
                    )
                    .await;
                let Some(rendered) = rendered else {
                    tracing::warn!(
                        session_id = %self.session_info.id.0,
                        "Plan-mode missing-exit nudge: reminder render failed; skipping"
                    );
                    return;
                };
                let tag = self.reminder_wrapper_tag();
                let text = format!("<{tag}>\n{rendered}\n</{tag}>");
                let prompt_id = format!(
                    "plan-missing-exit-nudge-{}-{nudge_count}",
                    chrono::Utc::now().timestamp_millis()
                );
                tracing::info!(
                    session_id = %self.session_info.id.0,
                    nudge_count,
                    "Plan-mode turn ended without a closing tool; queueing nudge turn"
                );
                xai_grok_telemetry::unified_log::info(
                    "shell.plan_mode.missing_exit_nudge",
                    Some(self.session_info.id.0.as_ref()),
                    Some(serde_json::json!({ "nudge_count": nudge_count })),
                );
                let prompt_blocks = vec![acp::ContentBlock::Text(acp::TextContent::new(text))];
                let (respond_to, _rx) = oneshot::channel();
                let _ = self
                    .queue_input(QueueInputRequest::new(
                        prompt_blocks,
                        prompt_id,
                        PromptMode::Plan,
                        respond_to,
                    ))
                    .await;
            }
        }
    }
}
/// Outcome of [`SessionActor::plan_missing_exit_nudge_decision`].
pub(super) enum MissingExitNudge {
    /// Queue a synthetic nudge turn carrying the missing-exit reminder.
    Nudge {
        /// 1-based count of nudges this activation (for the prompt id and logs).
        nudge_count: u32,
        plan_path: std::path::PathBuf,
    },
    /// The per-activation nudge budget is spent — stop self-waking.
    CapReached,
}
