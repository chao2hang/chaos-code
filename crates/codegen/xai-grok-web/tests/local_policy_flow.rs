use axum::serve;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chaos_engine::{ClientMessage, Engine, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use std::{fs, time::Duration};
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::router;

async fn start(
    engine: Engine,
) -> (
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    tokio::task::JoinHandle<()>,
) {
    start_with_shutdown(engine, Duration::from_millis(700)).await
}

/// The transfer tests below exchange a message per round trip, so they need a
/// shutdown window that outlasts the exchange instead of a fixed 700 ms.
async fn start_with_shutdown(
    engine: Engine,
    shutdown: Duration,
) -> (
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        serve(listener, router(engine, ""))
            .with_graceful_shutdown(async move { tokio::time::sleep(shutdown).await })
            .await
            .unwrap();
    });
    let (mut socket, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
    let _: ServerMessage =
        serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    (socket, task)
}

async fn send(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    message: ClientMessage,
) {
    socket
        .send(Message::Text(
            serde_json::to_string(&message).unwrap().into(),
        ))
        .await
        .unwrap();
}

async fn next(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> ServerMessage {
    serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap()
}

#[tokio::test]
async fn websocket_git_status_is_read_only_and_attachment_policy_is_enforced() {
    let directory = tempdir().unwrap();
    fs::create_dir(directory.path().join(".git")).unwrap();
    let engine = Engine::with_workspace(directory.path()).unwrap();
    let (mut socket, task) = start(engine).await;

    send(
        &mut socket,
        ClientMessage::GetGitStatus {
            client_msg_id: "git".into(),
        },
    )
    .await;
    let git = next(&mut socket).await;
    assert!(
        matches!(git, ServerMessage::GitStatus { .. })
            || matches!(git, ServerMessage::Error { code, .. } if code == "git_failed")
    );

    send(
        &mut socket,
        ClientMessage::ValidateAttachment {
            client_msg_id: "att-ok".into(),
            filename: "note.txt".into(),
            byte_len: 12,
            content_type: "text/plain".into(),
        },
    )
    .await;
    assert!(
        matches!(next(&mut socket).await, ServerMessage::AttachmentValidated { filename, .. } if filename == "note.txt")
    );
    send(
        &mut socket,
        ClientMessage::ValidateAttachment {
            client_msg_id: "att-bad".into(),
            filename: "../secret.exe".into(),
            byte_len: 12,
            content_type: "application/octet-stream".into(),
        },
    )
    .await;
    assert!(
        matches!(next(&mut socket).await, ServerMessage::Error { code, .. } if code == "attachment_rejected")
    );
    task.await.unwrap();
}

/// The same sequence the browser client runs: policy check, upload, slices that
/// each fit one frame, finalize, and the approval that puts the bytes in the
/// workspace. Chunking is not decoration here -- the frame the host refuses is
/// sent on purpose, and the upload survives it.
#[tokio::test]
async fn websocket_upload_lands_file_bytes_in_the_workspace_after_approval() {
    let directory = tempdir().unwrap();
    fs::create_dir(directory.path().join(".git")).unwrap();
    // 47 KiB slices mirror `UPLOAD_CHUNK_BYTES` in the browser client, so this is
    // a 100 KiB file travelling in three frames.
    const SLICE: usize = 47 * 1024;
    let payload: Vec<u8> = (0..100_000u32).map(|index| (index % 251) as u8).collect();
    let engine = Engine::with_workspace(directory.path()).unwrap();
    let (mut socket, task) = start_with_shutdown(engine, Duration::from_secs(20)).await;

    send(
        &mut socket,
        ClientMessage::CreateSession {
            client_msg_id: "session".into(),
            workspace_id: None,
        },
    )
    .await;
    let session_id = match next(&mut socket).await {
        ServerMessage::SessionCreated { session_id, .. } => session_id,
        other => panic!("expected a session, got {other:?}"),
    };

    send(
        &mut socket,
        ClientMessage::ValidateAttachment {
            client_msg_id: "validate".into(),
            filename: "note.txt".into(),
            byte_len: payload.len() as u64,
            content_type: "text/plain".into(),
        },
    )
    .await;
    assert!(
        matches!(next(&mut socket).await, ServerMessage::AttachmentValidated { byte_len, .. } if byte_len == payload.len() as u64)
    );

    send(
        &mut socket,
        ClientMessage::BeginAttachment {
            client_msg_id: "begin".into(),
            session_id,
            filename: "note.txt".into(),
            content_type: "text/plain".into(),
            byte_len: payload.len() as u64,
        },
    )
    .await;
    assert!(matches!(next(&mut socket).await, ServerMessage::Ack { .. }));
    let upload_id = match next(&mut socket).await {
        ServerMessage::AttachmentStarted {
            upload_id,
            filename,
            ..
        } => {
            assert_eq!(filename, "note.txt");
            upload_id
        }
        other => panic!("expected attachment_started, got {other:?}"),
    };

    // One frame over the host's 64 KiB bound. The host refuses the frame rather
    // than the upload, so the slices after it still land.
    let oversized = STANDARD.encode(vec![7u8; SLICE * 2 + 1]);
    assert!(oversized.len() > 64 * 1024);
    send(
        &mut socket,
        ClientMessage::AttachmentChunk {
            client_msg_id: "too-large".into(),
            upload_id,
            chunk: oversized,
        },
    )
    .await;
    assert!(
        matches!(next(&mut socket).await, ServerMessage::Error { code, .. } if code == "message_too_large")
    );

    let mut received = 0u64;
    for (index, chunk) in payload.chunks(SLICE).enumerate() {
        send(
            &mut socket,
            ClientMessage::AttachmentChunk {
                client_msg_id: format!("chunk-{index}"),
                upload_id,
                chunk: STANDARD.encode(chunk),
            },
        )
        .await;
        assert!(matches!(next(&mut socket).await, ServerMessage::Ack { .. }));
        match next(&mut socket).await {
            ServerMessage::AttachmentProgress {
                received: total, ..
            } => received = total,
            other => panic!("expected attachment_progress, got {other:?}"),
        }
    }
    assert_eq!(received, payload.len() as u64);

    send(
        &mut socket,
        ClientMessage::FinalizeAttachment {
            client_msg_id: "finalize".into(),
            upload_id,
            relative_path: "notes/note.txt".into(),
        },
    )
    .await;
    assert!(matches!(next(&mut socket).await, ServerMessage::Ack { .. }));
    let request_id = match next(&mut socket).await {
        ServerMessage::ToolApprovalRequested {
            tool, request_id, ..
        } => {
            assert_eq!(tool, "workspace.attach_attachment");
            request_id
        }
        other => panic!("expected an approval request, got {other:?}"),
    };

    fs::create_dir(directory.path().join("notes")).unwrap();
    send(
        &mut socket,
        ClientMessage::Approve {
            client_msg_id: "approve".into(),
            request_id,
        },
    )
    .await;
    let mut completed = None;
    for _ in 0..16 {
        match next(&mut socket).await {
            ServerMessage::AttachmentCompleted { path, bytes, .. } => {
                completed = Some((path, bytes));
                break;
            }
            ServerMessage::Error { code, message } => panic!("approval failed: {code}: {message}"),
            _ => {}
        }
    }
    let (path, bytes) = completed.expect("attachment_completed after the approval");
    assert_eq!(path, "notes/note.txt");
    assert_eq!(bytes, payload.len() as u64);
    assert_eq!(
        fs::read(directory.path().join("notes/note.txt")).unwrap(),
        payload
    );
    // The staged copy is renamed into place, not copied: nothing is left behind.
    let staging = directory.path().join(".chaos-staging");
    assert_eq!(
        fs::read_dir(&staging)
            .map(|entries| entries.count())
            .unwrap_or(0),
        0,
        "the staging directory still holds the upload"
    );
    task.await.unwrap();
}

/// Cancel is the other half of the transfer: an abandoned upload cannot be
/// finalized afterwards, so no bytes reach the workspace.
#[tokio::test]
async fn websocket_cancelled_upload_cannot_be_finalized() {
    let directory = tempdir().unwrap();
    fs::create_dir(directory.path().join(".git")).unwrap();
    let engine = Engine::with_workspace(directory.path()).unwrap();
    let (mut socket, task) = start_with_shutdown(engine, Duration::from_secs(20)).await;

    send(
        &mut socket,
        ClientMessage::CreateSession {
            client_msg_id: "session".into(),
            workspace_id: None,
        },
    )
    .await;
    let session_id = match next(&mut socket).await {
        ServerMessage::SessionCreated { session_id, .. } => session_id,
        other => panic!("expected a session, got {other:?}"),
    };
    send(
        &mut socket,
        ClientMessage::BeginAttachment {
            client_msg_id: "begin".into(),
            session_id,
            filename: "note.txt".into(),
            content_type: "text/plain".into(),
            byte_len: 8,
        },
    )
    .await;
    assert!(matches!(next(&mut socket).await, ServerMessage::Ack { .. }));
    let upload_id = match next(&mut socket).await {
        ServerMessage::AttachmentStarted { upload_id, .. } => upload_id,
        other => panic!("expected attachment_started, got {other:?}"),
    };

    send(
        &mut socket,
        ClientMessage::CancelAttachment {
            client_msg_id: "cancel".into(),
            upload_id,
        },
    )
    .await;
    assert!(matches!(next(&mut socket).await, ServerMessage::Ack { .. }));
    assert!(
        matches!(next(&mut socket).await, ServerMessage::AttachmentCancelled { upload_id: abandoned } if abandoned == upload_id)
    );

    send(
        &mut socket,
        ClientMessage::FinalizeAttachment {
            client_msg_id: "finalize".into(),
            upload_id,
            relative_path: "note.txt".into(),
        },
    )
    .await;
    assert!(
        matches!(next(&mut socket).await, ServerMessage::Error { code, .. } if code == "attachment_not_found")
    );
    assert!(!directory.path().join("note.txt").exists());
    task.await.unwrap();
}
