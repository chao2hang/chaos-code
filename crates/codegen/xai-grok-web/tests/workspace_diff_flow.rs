use axum::serve;
use chaos_engine::{ClientMessage, DiffAdapter, DiffPreview, Engine, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use std::{fs, path::PathBuf, sync::Arc, time::Duration};
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::router;

struct FileDiff {
    root: Arc<PathBuf>,
}

impl DiffAdapter for FileDiff {
    fn accept(&self, proposal_id: &str, _summary: &str) -> Result<(), String> {
        if proposal_id != "p1" {
            return Err("unknown proposal".into());
        }
        fs::write(self.root.join("hello.txt"), "after").map_err(|e| e.to_string())
    }

    fn rollback(&self, proposal_id: &str) -> Result<(), String> {
        if proposal_id != "p1" {
            return Err("unknown proposal".into());
        }
        fs::write(self.root.join("hello.txt"), "before").map_err(|e| e.to_string())
    }

    fn preview(&self, proposal_id: &str) -> Result<DiffPreview, String> {
        if proposal_id != "p1" {
            return Err("unknown proposal".into());
        }
        Ok(DiffPreview {
            proposal_id: proposal_id.into(),
            path: "hello.txt".into(),
            before: Some("before".into()),
            after: "after".into(),
        })
    }
}

async fn next_message(
    socket: &mut tokio_tungstenite::WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
) -> ServerMessage {
    serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap()
}

#[tokio::test]
async fn websocket_drives_workspace_write_and_diff_lifecycle() {
    let directory = tempdir().unwrap();
    fs::write(directory.path().join("hello.txt"), "before").unwrap();
    let root = Arc::new(directory.path().to_path_buf());
    let engine =
        Engine::with_workspace_and_diff_adapter(directory.path(), FileDiff { root: root.clone() })
            .unwrap();
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        serve(listener, router(engine, ""))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_secs(8)).await })
            .await
            .unwrap();
    });
    let (mut socket, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
    assert!(matches!(
        next_message(&mut socket).await,
        ServerMessage::Handshake { .. }
    ));

    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::CreateSession {
                client_msg_id: "create".into(),
                workspace_id: None,
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let session_id = match next_message(&mut socket).await {
        ServerMessage::SessionCreated { session_id, .. } => session_id,
        other => panic!("unexpected {other:?}"),
    };

    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::ProposeFileWrite {
                client_msg_id: "propose-reject".into(),
                session_id,
                relative_path: "hello.txt".into(),
                contents: "must-not-write".into(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let _ack = next_message(&mut socket).await;
    let request_id = match next_message(&mut socket).await {
        ServerMessage::ToolApprovalRequested { request_id, .. } => request_id,
        other => panic!("unexpected {other:?}"),
    };
    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::Reject {
                client_msg_id: "reject".into(),
                request_id,
                reason: "test".into(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    for _ in 0..3 {
        let _ = next_message(&mut socket).await;
    }
    assert_eq!(
        fs::read_to_string(directory.path().join("hello.txt")).unwrap(),
        "before"
    );

    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::ProposeFileWrite {
                client_msg_id: "propose-approve".into(),
                session_id,
                relative_path: "hello.txt".into(),
                contents: "after".into(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let _ack = next_message(&mut socket).await;
    let request_id = match next_message(&mut socket).await {
        ServerMessage::ToolApprovalRequested { request_id, .. } => request_id,
        other => panic!("unexpected {other:?}"),
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
    for _ in 0..6 {
        let _ = next_message(&mut socket).await;
    }
    assert_eq!(
        fs::read_to_string(directory.path().join("hello.txt")).unwrap(),
        "after"
    );

    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::PreviewDiff {
                client_msg_id: "preview".into(),
                session_id,
                proposal_id: "p1".into(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let _ = next_message(&mut socket).await;
    assert!(
        matches!(next_message(&mut socket).await, ServerMessage::DiffPreview { preview, .. } if preview.before.as_deref() == Some("before") && preview.after == "after")
    );

    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::AcceptDiff {
                client_msg_id: "accept".into(),
                session_id,
                proposal_id: "p1".into(),
                summary: "write hello".into(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    for _ in 0..3 {
        let _ = next_message(&mut socket).await;
    }
    assert_eq!(
        fs::read_to_string(directory.path().join("hello.txt")).unwrap(),
        "after"
    );

    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::RollbackDiff {
                client_msg_id: "rollback".into(),
                session_id,
                proposal_id: "p1".into(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    for _ in 0..3 {
        let _ = next_message(&mut socket).await;
    }
    assert_eq!(
        fs::read_to_string(directory.path().join("hello.txt")).unwrap(),
        "before"
    );
    drop(socket);
    server.await.unwrap();
}
