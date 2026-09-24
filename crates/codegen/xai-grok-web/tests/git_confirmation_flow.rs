use axum::serve;
use chaos_engine::{ClientMessage, Engine, ProcessGitAdapter, ServerMessage};
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
async fn destructive_git_commit_requires_two_approvals_before_head_changes() {
    let directory = tempdir().unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .args(["-C", directory.path().to_str().unwrap()])
            .args(args)
            .output()
            .unwrap()
    };
    assert!(run(&["init", "-q"]).status.success());
    assert!(run(&["config", "user.name", "Test User"]).status.success());
    assert!(
        run(&["config", "user.email", "test@example.invalid"])
            .status
            .success()
    );
    std::fs::write(directory.path().join("note.txt"), "before").unwrap();
    assert!(run(&["add", "--", "note.txt"]).status.success());
    assert!(run(&["commit", "-m", "initial"]).status.success());
    let old_head = String::from_utf8(run(&["rev-parse", "HEAD"]).stdout).unwrap();
    std::fs::write(directory.path().join("note.txt"), "after").unwrap();
    assert!(run(&["add", "--", "note.txt"]).status.success());

    let engine = Engine::new().with_git_adapter(ProcessGitAdapter::new(directory.path()).unwrap());
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
    let session_id = match next(&mut socket).await {
        ServerMessage::SessionCreated { session_id, .. } => session_id,
        other => panic!("{other:?}"),
    };
    send(
        &mut socket,
        ClientMessage::ProposeGitMutation {
            client_msg_id: "propose-commit".into(),
            session_id,
            operation: "commit".into(),
            argument: "approved change".into(),
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
        ClientMessage::Approve {
            client_msg_id: "first-approval".into(),
            request_id,
        },
    )
    .await;
    let _ = next(&mut socket).await;
    let second_confirmation = match next(&mut socket).await {
        ServerMessage::ToolApprovalRequested {
            request_id: same_id,
            summary,
            ..
        } => {
            assert_eq!(same_id, request_id);
            assert!(summary.contains("再次确认"));
            same_id
        }
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(
        String::from_utf8(run(&["rev-parse", "HEAD"]).stdout).unwrap(),
        old_head
    );
    send(
        &mut socket,
        ClientMessage::Approve {
            client_msg_id: "second-approval".into(),
            request_id: second_confirmation,
        },
    )
    .await;
    let mut resolved = false;
    for _ in 0..8 {
        if matches!(next(&mut socket).await, ServerMessage::ApprovalResolved { approved: true, request_id: resolved_id, .. } if resolved_id == request_id)
        {
            resolved = true;
            break;
        }
    }
    assert!(resolved);
    let new_head = String::from_utf8(run(&["rev-parse", "HEAD"]).stdout).unwrap();
    assert_ne!(new_head, old_head);
    assert_eq!(
        String::from_utf8(run(&["log", "-1", "--pretty=%s"]).stdout)
            .unwrap()
            .trim(),
        "approved change"
    );
    drop(socket);
    server.await.unwrap();
}
