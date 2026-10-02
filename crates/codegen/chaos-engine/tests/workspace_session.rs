use chaos_engine::{ClientMessage, Engine, ServerMessage};
use tempfile::tempdir;
use uuid::Uuid;

#[test]
fn resume_rejects_a_session_from_another_workspace() {
    let engine = Engine::new();
    let first = engine.handle(ClientMessage::CreateSession {
        client_msg_id: "create-first".into(),
        workspace_id: None,
    });
    let (session_id, workspace_id) = match first.as_slice() {
        [
            ServerMessage::SessionCreated {
                session_id,
                workspace_id,
            },
        ] => (*session_id, *workspace_id),
        other => panic!("unexpected {other:?}"),
    };
    let wrong_workspace = Uuid::new_v4();
    let result = engine.handle(ClientMessage::Resume {
        client_msg_id: "resume-wrong".into(),
        session_id,
        workspace_id: Some(wrong_workspace),
    });
    assert!(matches!(
        result.as_slice(),
        [ServerMessage::Error { code, .. }] if code == "workspace_session_mismatch"
    ));
    let result = engine.handle(ClientMessage::Resume {
        client_msg_id: "resume-right".into(),
        session_id,
        workspace_id: Some(workspace_id),
    });
    assert!(matches!(
        result.as_slice(),
        [ServerMessage::SessionSnapshot { .. }]
    ));
}

#[test]
fn persisted_session_keeps_workspace_binding_after_reopen() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("state.json");
    let first = Engine::with_persistence(&path).unwrap();
    let created = first.handle(ClientMessage::CreateSession {
        client_msg_id: "create".into(),
        workspace_id: None,
    });
    let (session_id, workspace_id) = match created.as_slice() {
        [
            ServerMessage::SessionCreated {
                session_id,
                workspace_id,
            },
        ] => (*session_id, *workspace_id),
        other => panic!("unexpected {other:?}"),
    };
    drop(first);
    let second = Engine::with_persistence(&path).unwrap();
    let result = second.handle(ClientMessage::Snapshot {
        client_msg_id: "snapshot".into(),
        session_id,
        workspace_id: Some(workspace_id),
    });
    assert!(matches!(
        result.as_slice(),
        [ServerMessage::SessionSnapshot { .. }]
    ));
}

/// `Workspaces` reports `active_workspace_id` as the nil UUID when the host has no
/// active workspace. A client that echoes that placeholder back means "none
/// selected", which is what a reconnecting page sends after a host restart.
#[test]
fn placeholder_active_workspace_id_round_trips_as_no_workspace_selected() {
    let engine = Engine::new();
    let listed = engine.handle(ClientMessage::ListWorkspaces {
        client_msg_id: "list".into(),
    });
    let placeholder = match listed.as_slice() {
        [
            ServerMessage::Workspaces {
                active_workspace_id,
                ..
            },
        ] => *active_workspace_id,
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(placeholder, Uuid::nil());

    let created = engine.handle(ClientMessage::CreateSession {
        client_msg_id: "create-via-placeholder".into(),
        workspace_id: Some(placeholder),
    });
    let session_id = match created.as_slice() {
        [ServerMessage::SessionCreated { session_id, .. }] => *session_id,
        other => panic!("unexpected {other:?}"),
    };
    let resumed = engine.handle(ClientMessage::Resume {
        client_msg_id: "resume-via-placeholder".into(),
        session_id,
        workspace_id: Some(placeholder),
    });
    assert!(
        matches!(resumed.as_slice(), [ServerMessage::SessionSnapshot { .. }]),
        "{resumed:?}"
    );
}

/// Only the placeholder is read as "none"; an unknown id stays a refusal.
#[test]
fn unknown_workspace_id_is_still_refused() {
    let engine = Engine::new();
    let result = engine.handle(ClientMessage::CreateSession {
        client_msg_id: "create-unknown".into(),
        workspace_id: Some(Uuid::new_v4()),
    });
    assert!(
        matches!(
            result.as_slice(),
            [ServerMessage::Error { code, .. }] if code == "workspace_unavailable"
        ),
        "{result:?}"
    );
}
