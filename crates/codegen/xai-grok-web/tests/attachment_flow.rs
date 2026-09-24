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
    let _ = next(&mut socket).await;
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
    server.await.unwrap();
}
