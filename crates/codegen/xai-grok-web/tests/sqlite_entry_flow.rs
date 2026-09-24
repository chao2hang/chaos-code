use chaos_engine::{ClientMessage, Engine, ServerMessage};
use tempfile::tempdir;

#[test]
fn sqlite_engine_entry_restores_state_after_reopen() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("gui.db");
    let first = Engine::with_sqlite_store(&path).unwrap();
    let created = first.handle(ClientMessage::CreateSession {
        client_msg_id: "create".into(),
    });
    let session_id = match &created[0] {
        ServerMessage::SessionCreated { session_id } => *session_id,
        other => panic!("unexpected {other:?}"),
    };
    first.handle(ClientMessage::Submit {
        client_msg_id: "submit".into(),
        session_id,
        prompt: "persist sqlite".into(),
    });
    drop(first);
    let second = Engine::with_sqlite_store(&path).unwrap();
    let snapshot = second.handle(ClientMessage::Resume {
        client_msg_id: "resume".into(),
        session_id,
    });
    assert!(
        matches!(&snapshot[0], ServerMessage::SessionSnapshot { messages, .. } if messages.iter().any(|message| message.text == "persist sqlite"))
    );
}
