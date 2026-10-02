use chaos_engine::{ClientMessage, Engine, PromptAdapter, ServerMessage};
use std::sync::Arc;
use tempfile::tempdir;

struct EchoAdapter;

impl PromptAdapter for EchoAdapter {
    fn run_prompt(&self, prompt: &str) -> Result<Vec<String>, String> {
        Ok(vec![format!("sqlite provider<{prompt}>")])
    }
}

#[test]
fn sqlite_engine_entry_restores_state_after_reopen() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("gui.db");
    let first = Engine::with_sqlite_store(&path).unwrap();
    let created = first.handle(ClientMessage::CreateSession {
        workspace_id: None,
        client_msg_id: "create".into(),
    });
    let session_id = match &created[0] {
        ServerMessage::SessionCreated { session_id, .. } => *session_id,
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
        workspace_id: None,
    });
    assert!(
        matches!(&snapshot[0], ServerMessage::SessionSnapshot { messages, .. } if messages.iter().any(|message| message.text == "persist sqlite"))
    );
}

/// `CHAOS_WEB_SQLITE` used to build the engine through `with_sqlite_store`,
/// which silently discarded the configured prompt adapter, so a host with both
/// a SQLite store and a provider answered every prompt with the demo responder.
#[test]
fn a_sqlite_backed_host_keeps_the_configured_prompt_adapter() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("provider.db");

    let engine = Engine::with_sqlite_store_and_adapter(&path, Some(Arc::new(EchoAdapter))).unwrap();
    let created = engine.handle(ClientMessage::CreateSession {
        workspace_id: None,
        client_msg_id: "create".into(),
    });
    let session_id = match &created[0] {
        ServerMessage::SessionCreated { session_id, .. } => *session_id,
        other => panic!("unexpected {other:?}"),
    };
    let streamed: String = engine
        .handle(ClientMessage::Submit {
            client_msg_id: "submit".into(),
            session_id,
            prompt: "hello".into(),
        })
        .into_iter()
        .filter_map(|event| match event {
            ServerMessage::TextDelta { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(streamed, "sqlite provider<hello>");
    drop(engine);

    // The transcript written through the provider survives the reopen.
    let reopened = Engine::with_sqlite_store_and_adapter(&path, None).unwrap();
    let snapshot = reopened.handle(ClientMessage::Resume {
        client_msg_id: "resume".into(),
        session_id,
        workspace_id: None,
    });
    assert!(
        matches!(&snapshot[0], ServerMessage::SessionSnapshot { messages, .. }
        if messages.iter().any(|message| message.text == "sqlite provider<hello>"))
    );
}
