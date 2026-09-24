use axum::serve;
use chaos_engine::{ClientMessage, Engine, ProcessGitAdapter, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::router;

#[tokio::test]
async fn websocket_uses_real_git_adapter_only_after_approval() {
    let directory = tempdir().unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .args(["-C", directory.path().to_str().unwrap()])
            .args(args)
            .output()
            .unwrap()
    };
    assert!(run(&["init", "-q"]).status.success());
    std::fs::write(directory.path().join("note.txt"), "hello").unwrap();
    let engine = Engine::new().with_git_adapter(ProcessGitAdapter::new(directory.path()).unwrap());
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
            serde_json::to_string(&ClientMessage::CreateSession {
                workspace_id: None,
                client_msg_id: "create".into(),
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
        other => panic!("{other:?}"),
    };
    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::ProposeGitMutation {
                client_msg_id: "stage".into(),
                session_id,
                operation: "stage".into(),
                argument: "note.txt".into(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let _: ServerMessage =
        serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    let request_id = match serde_json::from_str::<ServerMessage>(
        &socket.next().await.unwrap().unwrap().into_text().unwrap(),
    )
    .unwrap()
    {
        ServerMessage::ToolApprovalRequested { request_id, .. } => request_id,
        other => panic!("{other:?}"),
    };
    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::Approve {
                client_msg_id: "approve-stage".into(),
                request_id,
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    for _ in 0..6 {
        let event: ServerMessage =
            serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap())
                .unwrap();
        if matches!(event, ServerMessage::ApprovalResolved { .. }) {
            break;
        }
    }
    let staged = run(&["diff", "--cached", "--name-only"]);
    assert!(
        String::from_utf8_lossy(&staged.stdout)
            .lines()
            .any(|line| line == "note.txt")
    );
    server.await.unwrap();
}
