use pretty_assertions::assert_eq;

fn track(
    event: super::ExtEvent,
    pending: &mut std::collections::HashSet<super::BackgroundWork>,
    state: &mut super::BackgroundLifecycleState,
) -> Option<super::ext_protocol::SubagentStreamEvent> {
    let event = match event {
        super::ExtEvent::SubagentSpawned { subagent_id, attempt_id, event_seq } => {
            super::ExtEvent::SubagentLifecycle(super::ext_protocol::SubagentStreamEvent {
                event: "spawned".into(),
                child_session_id: subagent_id.clone(),
                parent_session_id: "parent".into(),
                subagent_type: "explore".into(),
                description: "test subagent".into(),
                parent_tool_call_id: Some("tool-parent".into()),
                status: Some("running".into()),
                model: None,
                workflow_run_id: None,
                agent_address: None,
                run_in_background: Some(true),
                started_at_ms: Some(1),
                duration_ms: None,
                subagent_id,
                attempt_id,
                event_seq,
            })
        }
        super::ExtEvent::SubagentProgress { subagent_id, attempt_id, event_seq } => {
            super::ExtEvent::SubagentLifecycle(super::ext_protocol::SubagentStreamEvent {
                event: "progress".into(),
                child_session_id: subagent_id.clone(),
                parent_session_id: "parent".into(),
                subagent_type: "explore".into(),
                description: "test subagent".into(),
                parent_tool_call_id: Some("tool-parent".into()),
                status: Some("running".into()),
                model: None,
                workflow_run_id: None,
                agent_address: None,
                run_in_background: Some(true),
                started_at_ms: Some(1),
                duration_ms: None,
                subagent_id,
                attempt_id,
                event_seq,
            })
        }
        super::ExtEvent::SubagentFinished { subagent_id, attempt_id, event_seq } => {
            super::ExtEvent::SubagentLifecycle(super::ext_protocol::SubagentStreamEvent {
                event: "finished".into(),
                child_session_id: subagent_id.clone(),
                parent_session_id: "parent".into(),
                subagent_type: "explore".into(),
                description: "test subagent".into(),
                parent_tool_call_id: Some("tool-parent".into()),
                status: Some("completed".into()),
                model: None,
                workflow_run_id: None,
                agent_address: None,
                run_in_background: Some(true),
                started_at_ms: Some(1),
                duration_ms: None,
                subagent_id,
                attempt_id,
                event_seq,
            })
        }
        other => other,
    };
    super::track_background_lifecycle(event, pending, state)
}

#[test]
fn lifecycle_tracking_is_independent_of_wait_flag() {
    let mut pending = std::collections::HashSet::new();
    let mut completed = super::BackgroundLifecycleState::default();
    track(
        super::ExtEvent::TaskBackgrounded {
            task_id: "t1".into(),
            is_monitor: false,
        },
        &mut pending,
        &mut completed,
    );
    track(
        super::ExtEvent::SubagentSpawned {
            subagent_id: "s1".into(),
            attempt_id: Some("at1.one".into()),
            event_seq: Some(1),
        },
        &mut pending,
        &mut completed,
    );
    assert!(pending.contains(&super::BackgroundWork::Task("t1".into())));
    assert!(pending.contains(&super::BackgroundWork::Subagent("s1".into())));

    track(
        super::ExtEvent::TaskCompleted {
            task_id: "t1".into(),
        },
        &mut pending,
        &mut completed,
    );
    track(
        super::ExtEvent::SubagentFinished {
            subagent_id: "s1".into(),
            attempt_id: Some("at1.one".into()),
            event_seq: Some(2),
        },
        &mut pending,
        &mut completed,
    );
    assert!(pending.is_empty());
}

#[test]
fn completion_before_backgrounded_never_rearms_pending() {
    let mut pending = std::collections::HashSet::new();
    let mut completed = super::BackgroundLifecycleState::default();
    track(
        super::ExtEvent::TaskCompleted {
            task_id: "t1".into(),
        },
        &mut pending,
        &mut completed,
    );
    track(
        super::ExtEvent::TaskBackgrounded {
            task_id: "t1".into(),
            is_monitor: false,
        },
        &mut pending,
        &mut completed,
    );
    assert!(pending.is_empty());
}

#[test]
fn duplicate_backgrounded_after_completion_stays_dead() {
    let mut pending = std::collections::HashSet::new();
    let mut completed = super::BackgroundLifecycleState::default();
    let bg = || super::ExtEvent::TaskBackgrounded {
        task_id: "t1".into(),
        is_monitor: false,
    };
    track(bg(), &mut pending, &mut completed);
    assert!(pending.contains(&super::BackgroundWork::Task("t1".into())));
    track(
        super::ExtEvent::TaskCompleted {
            task_id: "t1".into(),
        },
        &mut pending,
        &mut completed,
    );
    assert!(pending.is_empty());
    track(bg(), &mut pending, &mut completed);
    assert!(
        pending.is_empty(),
        "a backgrounded for an already-completed id must not re-arm pending"
    );
}

fn s1(state: &super::BackgroundLifecycleState) -> &crate::app::subagent::SubagentLifecycleState {
    let Some(s) = state.subagents.get("s1") else {
        panic!("missing subagent s1: {:?}", state.subagents.keys());
    };
    s
}

fn spawn_event(attempt_id: &str, event_seq: u64) -> super::ExtEvent {
    super::ExtEvent::SubagentSpawned {
        subagent_id: "s1".into(),
        attempt_id: Some(attempt_id.into()),
        event_seq: Some(event_seq),
    }
}

fn legacy_spawn_event(event_seq: Option<u64>) -> super::ExtEvent {
    super::ExtEvent::SubagentSpawned {
        subagent_id: "s1".into(),
        attempt_id: None,
        event_seq,
    }
}

fn legacy_finish_event(event_seq: Option<u64>) -> super::ExtEvent {
    super::ExtEvent::SubagentFinished {
        subagent_id: "s1".into(),
        attempt_id: None,
        event_seq,
    }
}

#[test]
fn newer_subagent_spawn_after_finish_rearms_pending() {
    let mut pending = std::collections::HashSet::new();
    let mut completed = super::BackgroundLifecycleState::default();
    let work = super::BackgroundWork::Subagent("s1".into());
    track(spawn_event("at1.one", 1), &mut pending, &mut completed);
    track(
        super::ExtEvent::SubagentFinished {
            subagent_id: "s1".into(),
            attempt_id: Some("at1.one".into()),
            event_seq: Some(2),
        },
        &mut pending,
        &mut completed,
    );
    assert!(pending.is_empty());
    assert!(completed.subagents.contains_key("s1"));

    track(spawn_event("at1.one", 1), &mut pending, &mut completed);
    assert!(pending.is_empty());
    assert!(completed.subagents.contains_key("s1"));

    track(spawn_event("at1.two", 3), &mut pending, &mut completed);
    assert!(pending.contains(&work));
    assert_eq!(s1(&completed).current_attempt_id(), Some("at1.two"));
    track(
        super::ExtEvent::SubagentFinished {
            subagent_id: "s1".into(),
            attempt_id: Some("at1.one".into()),
            event_seq: Some(2),
        },
        &mut pending,
        &mut completed,
    );
    assert!(pending.contains(&work));
    track(
        super::ExtEvent::SubagentFinished {
            subagent_id: "s1".into(),
            attempt_id: Some("at1.two".into()),
            event_seq: Some(4),
        },
        &mut pending,
        &mut completed,
    );
    assert!(pending.is_empty());
    track(
        super::ExtEvent::SubagentFinished {
            subagent_id: "s1".into(),
            attempt_id: Some("at1.one".into()),
            event_seq: Some(2),
        },
        &mut pending,
        &mut completed,
    );
    track(spawn_event("at1.two", 3), &mut pending, &mut completed);
    assert!(pending.is_empty());
    assert_eq!(s1(&completed).last_event_seq(), Some(4));
}

#[test]
fn retained_prior_finish_records_without_clearing_current_pending() {
    let mut pending = std::collections::HashSet::new();
    let mut state = super::BackgroundLifecycleState::default();
    let work = super::BackgroundWork::Subagent("s1".into());
    track(spawn_event("at1.one", 1), &mut pending, &mut state);
    track(spawn_event("at1.two", 3), &mut pending, &mut state);

    track(
        super::ExtEvent::SubagentFinished {
            subagent_id: "s1".into(),
            attempt_id: Some("at1.one".into()),
            event_seq: Some(2),
        },
        &mut pending,
        &mut state,
    );

    assert!(s1(&state).is_attempt_finished("at1.one"));
    assert_eq!(s1(&state).current_attempt_id(), Some("at1.two"));
    assert!(!s1(&state).is_finished());
    assert!(pending.contains(&work));
}

#[test]
fn exact_pending_finish_discards_legacy_before_next_spawn() {
    let mut pending = std::collections::HashSet::new();
    let mut state = super::BackgroundLifecycleState::default();
    let work = super::BackgroundWork::Subagent("s1".into());
    track(legacy_finish_event(Some(2)), &mut pending, &mut state);
    track(
        super::ExtEvent::SubagentFinished {
            subagent_id: "s1".into(),
            attempt_id: Some("at1.one".into()),
            event_seq: Some(3),
        },
        &mut pending,
        &mut state,
    );
    track(spawn_event("at1.one", 1), &mut pending, &mut state);

    assert!(s1(&state).is_attempt_finished("at1.one"));
    assert!(!s1(&state).retains_attempt(&crate::app::subagent::SubagentAttemptKey::Legacy));
    track(spawn_event("at1.two", 4), &mut pending, &mut state);
    assert_eq!(s1(&state).current_attempt_id(), Some("at1.two"));
    assert!(!s1(&state).is_finished());
    assert!(pending.contains(&work));
}

#[test]
fn legacy_finish_before_typed_spawn_does_not_leave_pending_work() {
    let mut pending = std::collections::HashSet::new();
    let mut state = super::BackgroundLifecycleState::default();
    track(legacy_finish_event(Some(2)), &mut pending, &mut state);

    track(spawn_event("at1.one", 1), &mut pending, &mut state);

    assert_eq!(s1(&state).current_attempt_id(), Some("at1.one"));
    assert!(s1(&state).is_finished());
    assert!(pending.is_empty());
}

#[test]
fn legacy_finish_before_two_typed_spawns_rearms_only_the_second() {
    let mut pending = std::collections::HashSet::new();
    let mut state = super::BackgroundLifecycleState::default();
    let work = super::BackgroundWork::Subagent("s1".into());
    track(legacy_finish_event(Some(2)), &mut pending, &mut state);
    track(spawn_event("at1.one", 1), &mut pending, &mut state);

    track(spawn_event("at1.two", 3), &mut pending, &mut state);

    assert!(s1(&state).is_attempt_finished("at1.one"));
    assert_eq!(s1(&state).current_attempt_id(), Some("at1.two"));
    assert!(!s1(&state).is_finished());
    assert!(pending.contains(&work));
}

#[test]
fn legacy_finish_closes_the_only_typed_running_attempt() {
    let mut pending = std::collections::HashSet::new();
    let mut state = super::BackgroundLifecycleState::default();
    let work = super::BackgroundWork::Subagent("s1".into());
    track(spawn_event("at1.one", 1), &mut pending, &mut state);
    assert!(pending.contains(&work));

    track(legacy_finish_event(Some(2)), &mut pending, &mut state);

    assert!(s1(&state).is_finished());
    assert!(pending.is_empty());
}

#[test]
fn legacy_finish_closes_current_typed_attempt_after_wake() {
    let mut pending = std::collections::HashSet::new();
    let mut state = super::BackgroundLifecycleState::default();
    let work = super::BackgroundWork::Subagent("s1".into());
    track(spawn_event("at1.one", 1), &mut pending, &mut state);
    track(
        super::ExtEvent::SubagentFinished {
            subagent_id: "s1".into(),
            attempt_id: Some("at1.one".into()),
            event_seq: Some(2),
        },
        &mut pending,
        &mut state,
    );
    track(spawn_event("at1.two", 3), &mut pending, &mut state);
    assert!(pending.contains(&work));

    track(legacy_finish_event(Some(4)), &mut pending, &mut state);

    assert_eq!(s1(&state).current_attempt_id(), Some("at1.two"));
    assert!(s1(&state).is_finished());
    assert!(pending.is_empty());
}

#[test]
fn legacy_finish_after_typed_completion_is_dropped() {
    let mut pending = std::collections::HashSet::new();
    let mut state = super::BackgroundLifecycleState::default();
    track(spawn_event("at1.one", 1), &mut pending, &mut state);
    track(
        super::ExtEvent::SubagentFinished {
            subagent_id: "s1".into(),
            attempt_id: Some("at1.one".into()),
            event_seq: Some(2),
        },
        &mut pending,
        &mut state,
    );

    track(legacy_finish_event(Some(3)), &mut pending, &mut state);

    assert_eq!(s1(&state).current_attempt_id(), Some("at1.one"));
    assert!(s1(&state).is_finished());
    assert_eq!(s1(&state).last_event_seq(), Some(2));
    assert!(!s1(&state).retains_attempt(&crate::app::subagent::SubagentAttemptKey::Legacy));
    assert!(pending.is_empty());
}

#[test]
fn finished_legacy_duplicate_spawn_does_not_rearm_pending() {
    let mut pending = std::collections::HashSet::new();
    let mut state = super::BackgroundLifecycleState::default();
    track(legacy_spawn_event(None), &mut pending, &mut state);
    track(legacy_finish_event(None), &mut pending, &mut state);
    track(legacy_spawn_event(None), &mut pending, &mut state);

    assert_eq!(s1(&state).current_attempt_id(), None);
    assert!(s1(&state).is_finished());
    assert!(pending.is_empty());
}
