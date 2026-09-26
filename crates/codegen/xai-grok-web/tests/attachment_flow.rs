use axum::serve;
use base64::Engine as Base64Engine;
use chaos_engine::{ClientMessage, Engine, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::router;

#[tokio::test]
async fn websocket_upload_requires_approval_before_staging_into_workspace() {
    let directory = tempdir().unwrap();
    let engine = Engine::with_workspace(directory.path()).unwrap();
    std::fs::create_dir(directory.path().join("nested")).unwrap();
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        serve(listener, router(engine, ""))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_secs(5)).await })
            .await
            .unwrap();
    });
    let (mut socket, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
    let _: ServerMessage =
        serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
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
    send(
        &mut socket,
        ClientMessage::CreateSession {
            client_msg_id: "create".into(),
            workspace_id: None,
        },
    )
    .await;
    let session_id = match next(&mut socket).await {
        ServerMessage::SessionCreated { session_id, .. } => session_id,
        other => panic!("unexpected {other:?}"),
    };
    send(
        &mut socket,
        ClientMessage::BeginAttachment {
            client_msg_id: "begin".into(),
            session_id,
            filename: "note.txt".into(),
            content_type: "text/plain".into(),
            byte_len: 5,
        },
    )
    .await;
    let _ = next(&mut socket).await;
    let upload_id = match next(&mut socket).await {
        ServerMessage::AttachmentStarted { upload_id, .. } => upload_id,
        other => panic!("unexpected {other:?}"),
    };
    assert!(directory.path().join(".chaos-staging").exists());
    assert!(
        std::fs::read_dir(directory.path().join(".chaos-staging"))
            .unwrap()
            .next()
            .is_none()
    );
    send(
        &mut socket,
        ClientMessage::AttachmentChunk {
            client_msg_id: "chunk".into(),
            upload_id,
            chunk: Base64Engine::encode(&base64::engine::general_purpose::STANDARD, b"hello"),
        },
    )
    .await;
    let _ = next(&mut socket).await;
    let progress = next(&mut socket).await;
    assert!(matches!(
        progress,
        ServerMessage::AttachmentProgress { received, .. } if received == 5
    ));
    assert!(
        std::fs::read_dir(directory.path().join(".chaos-staging"))
            .unwrap()
            .next()
            .is_none()
    );
    send(
        &mut socket,
        ClientMessage::ReadFile {
            client_msg_id: "read-staging-over-websocket".into(),
            relative_path: ".chaos-staging/upload-secret.part".into(),
        },
    )
    .await;
    assert!(matches!(
        next(&mut socket).await,
        ServerMessage::Error { code, .. } if code == "path_escape"
    ));
    send(
        &mut socket,
        ClientMessage::ListFiles {
            client_msg_id: "list-staging-over-websocket".into(),
            relative_path: ".chaos-staging".into(),
        },
    )
    .await;
    assert!(matches!(
        next(&mut socket).await,
        ServerMessage::Error { code, .. } if code == "path_escape"
    ));
    send(
        &mut socket,
        ClientMessage::SearchFiles {
            client_msg_id: "search-staging-over-websocket".into(),
            query: "hello".into(),
        },
    )
    .await;
    assert!(matches!(
        next(&mut socket).await,
        ServerMessage::SearchResults { matches, .. } if matches.is_empty()
    ));
    send(
        &mut socket,
        ClientMessage::FinalizeAttachment {
            client_msg_id: "finalize".into(),
            upload_id,
            relative_path: "note.txt".into(),
        },
    )
    .await;
    let _ = next(&mut socket).await;
    let request_id = match next(&mut socket).await {
        ServerMessage::ToolApprovalRequested { request_id, .. } => request_id,
        other => panic!("unexpected {other:?}"),
    };
    assert!(!directory.path().join("note.txt").exists());
    send(
        &mut socket,
        ClientMessage::Reject {
            client_msg_id: "reject".into(),
            request_id,
            reason: "test".into(),
        },
    )
    .await;
    for _ in 0..3 {
        let _ = next(&mut socket).await;
    }
    assert!(!directory.path().join("note.txt").exists());

    send(
        &mut socket,
        ClientMessage::FinalizeAttachment {
            client_msg_id: "finalize-again".into(),
            upload_id,
            relative_path: "note.txt".into(),
        },
    )
    .await;
    let _ = next(&mut socket).await;
    let request_id = match next(&mut socket).await {
        ServerMessage::ToolApprovalRequested { request_id, .. } => request_id,
        other => panic!("unexpected {other:?}"),
    };
    send(
        &mut socket,
        ClientMessage::Approve {
            client_msg_id: "approve-attachment".into(),
            request_id,
        },
    )
    .await;
    for _ in 0..4 {
        let _ = next(&mut socket).await;
    }
    assert_eq!(
        std::fs::read_to_string(directory.path().join("note.txt")).unwrap(),
        "hello"
    );

    send(
        &mut socket,
        ClientMessage::BeginAttachment {
            client_msg_id: "begin-cancelled-upload".into(),
            session_id,
            filename: "cancelled.txt".into(),
            content_type: "text/plain".into(),
            byte_len: 5,
        },
    )
    .await;
    let _ = next(&mut socket).await;
    let _ = next(&mut socket).await;
    let cancelled_upload_id = match next(&mut socket).await {
        ServerMessage::AttachmentStarted { upload_id, .. } => upload_id,
        other => panic!("unexpected {other:?}"),
    };
    send(
        &mut socket,
        ClientMessage::AttachmentChunk {
            client_msg_id: "chunk-cancelled-upload".into(),
            upload_id: cancelled_upload_id,
            chunk: Base64Engine::encode(&base64::engine::general_purpose::STANDARD, b"abort"),
        },
    )
    .await;
    let _ = next(&mut socket).await;
    assert!(matches!(
        next(&mut socket).await,
        ServerMessage::AttachmentProgress { received: 5, .. }
    ));
    send(
        &mut socket,
        ClientMessage::CancelAttachment {
            client_msg_id: "cancel-upload".into(),
            upload_id: cancelled_upload_id,
        },
    )
    .await;
    let _ = next(&mut socket).await;
    assert!(matches!(
        next(&mut socket).await,
        ServerMessage::AttachmentCancelled { upload_id } if upload_id == cancelled_upload_id
    ));
    send(
        &mut socket,
        ClientMessage::FinalizeAttachment {
            client_msg_id: "finalize-cancelled-upload".into(),
            upload_id: cancelled_upload_id,
            relative_path: "cancelled.txt".into(),
        },
    )
    .await;
    assert!(matches!(
        next(&mut socket).await,
        ServerMessage::Error { code, .. } if code == "attachment_not_found"
    ));
    assert!(!directory.path().join("cancelled.txt").exists());
    server.await.unwrap();
}
