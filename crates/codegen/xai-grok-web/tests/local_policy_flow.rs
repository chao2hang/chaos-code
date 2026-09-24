use axum::serve;
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
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        serve(listener, router(engine, ""))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_millis(700)).await })
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
