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
            serde_json::to_string(&ClientMessage::CreateSession {
                workspace_id: None,
                client_msg_id: "session".into(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let session_id = match serde_json::from_str::<ServerMessage>(
        &socket.next().await.unwrap().unwrap().into_text().unwrap(),
    )
    .unwrap()
    {
        ServerMessage::SessionCreated { session_id, .. } => session_id,
        _ => panic!("expected session"),
    };
    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::ProposeFileWrite {
                client_msg_id: "write".into(),
                session_id,
                relative_path: "new.txt".into(),
                contents: "created".into(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let _: ServerMessage =
        serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    let approval = serde_json::from_str::<ServerMessage>(
        &socket.next().await.unwrap().unwrap().into_text().unwrap(),
    )
    .unwrap();
    let request_id = match approval {
        ServerMessage::ToolApprovalRequested { request_id, .. } => request_id,
        _ => panic!("expected approval"),
    };
    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::Approve {
                client_msg_id: "approve".into(),
                request_id,
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let mut wrote = false;
    for _ in 0..4 {
        let event = serde_json::from_str::<ServerMessage>(
            &socket.next().await.unwrap().unwrap().into_text().unwrap(),
        )
        .unwrap();
        wrote |= matches!(event, ServerMessage::FileWritten { path, .. } if path == "new.txt");
        if wrote {
            break;
        }
    }
    assert!(wrote);
    assert_eq!(
        std::fs::read_to_string(directory.path().join("new.txt")).unwrap(),
        "created"
    );
    let mut saw_audit = false;
    for _ in 0..3 {
        let event: ServerMessage =
            serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap())
                .unwrap();
        saw_audit |= matches!(event, ServerMessage::Audit { .. });
        if saw_audit {
            break;
        }
    }
    assert!(saw_audit);

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
        matches!(escaped, ServerMessage::Error { .. }),
        "unexpected escape result: {escaped:?}"
    );
    server.await.unwrap();
}
