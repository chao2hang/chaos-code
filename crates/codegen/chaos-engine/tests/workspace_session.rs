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
