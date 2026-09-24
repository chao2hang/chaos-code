use axum::serve;
use chaos_engine::{ClientMessage, Engine, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use std::{fs, time::Duration};
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
async fn archiving_active_workspace_switches_to_fallback_session_and_updates_registry() {
    let directory = tempdir().unwrap();
    let state_path = directory.path().join("state.json");
    let server_dir = tempdir().unwrap();
    let state_file = state_path.clone();
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let engine = Engine::with_persistence(state_file).unwrap();
        serve(listener, router(engine, ""))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_secs(5)).await })
            .await
            .unwrap();
    });
    let (mut socket, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
    let _ = next(&mut socket).await;
    send(
        &mut socket,
        ClientMessage::CreateSession {
            client_msg_id: "create-default".into(),
            workspace_id: None,
        },
    )
    .await;
    let default_id = match next(&mut socket).await {
        ServerMessage::SessionCreated {
            session_id,
            workspace_id,
        } => (session_id, workspace_id),
        other => panic!("{other:?}"),
    };
    send(
        &mut socket,
        ClientMessage::CreateWorkspace {
            client_msg_id: "create-new".into(),
            name: "temporary".into(),
        },
    )
    .await;
    assert!(matches!(next(&mut socket).await, ServerMessage::Ack { .. }));
    assert!(matches!(
        next(&mut socket).await,
        ServerMessage::WorkspaceSwitched { .. }
    ));
    let created_session = next(&mut socket).await;
    let temporary_session = match created_session {
        ServerMessage::SessionCreated {
            session_id,
            workspace_id,
        } => (session_id, workspace_id),
        other => panic!("{other:?}"),
    };
    let registry = next(&mut socket).await;
    let temporary = match registry {
        ServerMessage::Workspaces {
            active_workspace_id,
            workspaces,
        } => {
            assert!(workspaces.iter().any(|item| item.id == default_id.1));
            active_workspace_id
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(temporary_session.1, temporary);
    send(
        &mut socket,
        ClientMessage::Submit {
            client_msg_id: "write-temp".into(),
            session_id: temporary_session.0,
            prompt: "temporary transcript".into(),
        },
    )
    .await;
    let mut completed = false;
    for _ in 0..12 {
        if matches!(next(&mut socket).await, ServerMessage::Completed { session_id, .. } if session_id == temporary_session.0)
        {
            completed = true;
            break;
        }
    }
    assert!(completed);

    send(
        &mut socket,
        ClientMessage::ArchiveWorkspace {
            client_msg_id: "archive".into(),
            workspace_id: temporary,
        },
    )
    .await;
    let archived = next(&mut socket).await;
    assert!(
        matches!(archived, ServerMessage::WorkspaceArchived { workspace_id } if workspace_id == temporary),
        "unexpected {archived:?}"
    );
    assert!(
        matches!(next(&mut socket).await, ServerMessage::WorkspaceSwitched { workspace_id } if workspace_id == default_id.1)
    );
    assert!(
        matches!(next(&mut socket).await, ServerMessage::SessionSnapshot { session_id, .. } if session_id == default_id.0)
    );
    assert!(
        matches!(next(&mut socket).await, ServerMessage::Workspaces { active_workspace_id, workspaces } if active_workspace_id == default_id.1 && workspaces.iter().find(|item| item.id == temporary).is_some_and(|item| item.archived))
    );
    send(
        &mut socket,
        ClientMessage::Submit {
            client_msg_id: "submit-archived".into(),
            session_id: temporary_session.0,
            prompt: "should fail".into(),
        },
    )
    .await;
    assert!(
        matches!(next(&mut socket).await, ServerMessage::Error { code, .. } if code == "workspace_unavailable")
    );
    let _ = fs::remove_file(server_dir.path().join("unused"));
    drop(socket);
    server.await.unwrap();
}
