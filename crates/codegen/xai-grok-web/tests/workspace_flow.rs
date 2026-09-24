use axum::serve;
use chaos_engine::{ClientMessage, Engine, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::router;

#[tokio::test]
async fn websocket_workspace_requests_are_confined_to_root() {
    let directory = tempdir().unwrap();
    std::fs::write(directory.path().join("note.txt"), "workspace needle").unwrap();
    let engine = Engine::with_workspace(directory.path()).unwrap();
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        serve(listener, router(engine, ""))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_millis(500)).await })
            .await
            .unwrap();
    });

    let (mut socket, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
    let _: ServerMessage =
        serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::ListFiles {
                client_msg_id: "list".into(),
                relative_path: ".".into(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let listed: ServerMessage =
        serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    assert!(
        matches!(listed, ServerMessage::FilesListed { entries, .. } if entries.iter().any(|entry| entry == "note.txt"))
    );

    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::WriteFile {
                client_msg_id: "write".into(),
                relative_path: "new.txt".into(),
                contents: "created".into(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let written: ServerMessage =
        serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    assert!(matches!(written, ServerMessage::FileWritten { path, .. } if path == "new.txt"));
    assert_eq!(
        std::fs::read_to_string(directory.path().join("new.txt")).unwrap(),
        "created"
    );

    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::ReadFile {
                client_msg_id: "read".into(),
                relative_path: "../outside".into(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let escaped: ServerMessage =
        serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    assert!(
        matches!(escaped, ServerMessage::Error { code, .. } if code == "path_invalid" || code == "path_escape")
    );
    server.await.unwrap();
}
