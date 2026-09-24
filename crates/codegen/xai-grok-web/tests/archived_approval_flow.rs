use axum::serve;
use chaos_engine::{ClientMessage, Engine, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::router;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
async fn next(socket: &mut Socket) -> ServerMessage {
    serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap()
}
async fn send(socket: &mut Socket, message: ClientMessage) {
    socket
        .send(Message::Text(
            serde_json::to_string(&message).unwrap().into(),
        ))
        .await
        .unwrap();
}

#[tokio::test]
async fn archived_workspace_approval_cannot_write_after_fallback_switch() {
    let root = tempdir().unwrap();
    let engine = Engine::with_workspace(root.path()).unwrap();
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        serve(listener, router(engine, ""))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_secs(4)).await })
            .await
            .unwrap();
    });
    let (mut socket, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
    let _ = next(&mut socket).await;
    send(
        &mut socket,
        ClientMessage::CreateSession {
            client_msg_id: "create".into(),
            workspace_id: None,
        },
    )
    .await;
    let (session_id, workspace_id) = match next(&mut socket).await {
        ServerMessage::SessionCreated {
            session_id,
            workspace_id,
        } => (session_id, workspace_id),
        other => panic!("{other:?}"),
    };
    send(
        &mut socket,
        ClientMessage::ProposeFileWrite {
            client_msg_id: "propose".into(),
            session_id,
            relative_path: "unsafe.txt".into(),
            contents: "must not land".into(),
        },
    )
    .await;
    let _ = next(&mut socket).await;
    let request_id = match next(&mut socket).await {
        ServerMessage::ToolApprovalRequested { request_id, .. } => request_id,
        other => panic!("{other:?}"),
    };
    send(
        &mut socket,
        ClientMessage::ArchiveWorkspace {
            client_msg_id: "archive".into(),
            workspace_id,
        },
    )
    .await;
    let mut archived = false;
    for _ in 0..4 {
        archived |= matches!(
            next(&mut socket).await,
            ServerMessage::WorkspaceArchived { .. }
        );
        if archived {
            break;
        }
    }
    assert!(archived);
    let mut registry_seen = false;
    for _ in 0..4 {
        registry_seen |= matches!(next(&mut socket).await, ServerMessage::Workspaces { .. });
        if registry_seen {
            break;
        }
    }
    assert!(registry_seen);
    send(
        &mut socket,
        ClientMessage::Approve {
            client_msg_id: "approve-stale".into(),
            request_id,
        },
    )
    .await;
    assert!(
        matches!(next(&mut socket).await, ServerMessage::Error { code, .. } if code == "workspace_unavailable")
    );
    assert!(!root.path().join("unsafe.txt").exists());
    drop(socket);
    server.await.unwrap();
}
