use chaos_engine::{ClientMessage, Engine, ServerMessage};
use std::fs;
use tempfile::tempdir;

#[test]
fn tui_import_reads_legacy_summary_without_modifying_source() {
    let root = tempdir().unwrap();
    let session = root.path().join("session-1");
    fs::create_dir_all(&session).unwrap();
    let summary = br#"{
        "info": {"id":"session-1","cwd":"/workspace/project"},
        "session_summary":"legacy title",
        "num_messages":3
    }"#;
    let updates = b"{\"method\":\"session/update\"}\n{\"method\":\"session/update\"}\n";
    fs::write(session.join("summary.json"), summary).unwrap();
    fs::write(session.join("updates.jsonl"), updates).unwrap();
    let before_summary = fs::read(session.join("summary.json")).unwrap();
    let before_updates = fs::read(session.join("updates.jsonl")).unwrap();

    let result = Engine::new()
        .with_tui_session_root(root.path())
        .unwrap()
        .handle(ClientMessage::ImportTuiSession {
            client_msg_id: "import".into(),
            root: root.path().display().to_string(),
            session_id: "session-1".into(),
        });
    assert!(matches!(
        result.as_slice(),
        [ServerMessage::TuiSessionImport { cwd, title, message_count, source_unchanged, .. }]
            if cwd == "/workspace/project"
                && title.as_deref() == Some("legacy title")
                && *message_count == 3
                && *source_unchanged
    ));
    assert_eq!(
        fs::read(session.join("summary.json")).unwrap(),
        before_summary
    );
    assert_eq!(
        fs::read(session.join("updates.jsonl")).unwrap(),
        before_updates
    );
}

#[test]
fn tui_import_rejects_unconfigured_root() {
    let root = tempdir().unwrap();
    let result = Engine::new().handle(ClientMessage::ImportTuiSession {
        client_msg_id: "unconfigured".into(),
        root: root.path().display().to_string(),
        session_id: "session-1".into(),
    });
    assert!(matches!(
        result.as_slice(),
        [ServerMessage::Error { code, .. }] if code == "tui_session_root_not_allowed"
    ));
}

#[test]
fn tui_import_rejects_missing_or_malformed_summary() {
    let root = tempdir().unwrap();
    let session = root.path().join("broken");
    fs::create_dir_all(&session).unwrap();
    fs::write(session.join("summary.json"), b"not-json").unwrap();
    let result = Engine::new()
        .with_tui_session_root(root.path())
        .unwrap()
        .handle(ClientMessage::ImportTuiSession {
            client_msg_id: "broken".into(),
            root: root.path().display().to_string(),
            session_id: "broken".into(),
        });
    assert!(matches!(
        result.as_slice(),
        [ServerMessage::Error { code, .. }] if code == "tui_session_invalid"
    ));
}
