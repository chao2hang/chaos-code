use base64::Engine as _;
use chaos_engine::{ClientMessage, Engine, ServerMessage};
use uuid::Uuid;

#[test]
fn attachment_protocol_accepts_chunks_reports_progress_and_cancels() {
    let engine = Engine::new();
    let session_id = Uuid::new_v4();
    let begin = engine.handle(ClientMessage::BeginAttachment {
        client_msg_id: "begin".into(),
        session_id,
        filename: "note.txt".into(),
        content_type: "text/plain".into(),
        byte_len: 3,
    });
    let upload_id = match &begin[1] {
        ServerMessage::AttachmentStarted { upload_id, .. } => *upload_id,
        other => panic!("{other:?}"),
    };
    let chunk = base64::engine::general_purpose::STANDARD.encode(b"abc");
    let progress = engine.handle(ClientMessage::AttachmentChunk {
        client_msg_id: "chunk".into(),
        upload_id,
        chunk,
    });
    assert!(
        progress
            .iter()
            .any(|event| matches!(event, ServerMessage::AttachmentProgress { received: 3, .. }))
    );
    let cancelled = engine.handle(ClientMessage::CancelAttachment {
        client_msg_id: "cancel".into(),
        upload_id,
    });
    assert!(
        cancelled
            .iter()
            .any(|event| matches!(event, ServerMessage::AttachmentCancelled { .. }))
    );
}

#[test]
fn attachment_protocol_rejects_over_quota_and_unknown_upload() {
    let engine = Engine::new();
    let session_id = Uuid::new_v4();
    let begin = engine.handle(ClientMessage::BeginAttachment {
        client_msg_id: "begin".into(),
        session_id,
        filename: "note.txt".into(),
        content_type: "text/plain".into(),
        byte_len: 2,
    });
    let upload_id = match &begin[1] {
        ServerMessage::AttachmentStarted { upload_id, .. } => *upload_id,
        _ => panic!(),
    };
    let chunk = base64::engine::general_purpose::STANDARD.encode(b"abc");
    let rejected = engine.handle(ClientMessage::AttachmentChunk {
        client_msg_id: "chunk".into(),
        upload_id,
        chunk,
    });
    assert!(
        matches!(&rejected[0], ServerMessage::Error { code, .. } if code == "attachment_quota_exceeded")
    );
    let unknown = engine.handle(ClientMessage::CancelAttachment {
        client_msg_id: "unknown".into(),
        upload_id,
    });
    assert!(
        matches!(&unknown[0], ServerMessage::Error { code, .. } if code == "attachment_not_found")
    );
}
