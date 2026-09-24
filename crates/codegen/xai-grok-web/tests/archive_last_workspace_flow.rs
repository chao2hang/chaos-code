use axum::serve;
use chaos_engine::{ClientMessage, Engine, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::router;

#[tokio::test]
async fn archiving_last_workspace_creates_a_new_default_workspace_and_session() {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        serve(listener, router(Engine::new(), ""))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_secs(3)).await })
            .await
            .unwrap();
    });
    let (mut socket, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
    let _ = socket.next().await.unwrap();
    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::CreateSession {
                client_msg_id: "create-first".into(),
                workspace_id: None,
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let (old_session, old_workspace) = match serde_json::from_str::<ServerMessage>(
        &socket.next().await.unwrap().unwrap().into_text().unwrap(),
    )
    .unwrap()
    {
        ServerMessage::SessionCreated {
            session_id,
            workspace_id,
        } => (session_id, workspace_id),
        other => panic!("{other:?}"),
    };
    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::ArchiveWorkspace {
                client_msg_id: "archive-last".into(),
                workspace_id: old_workspace,
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let mut archived = false;
    for _ in 0..4 {
        let message = serde_json::from_str::<ServerMessage>(
            &socket.next().await.unwrap().unwrap().into_text().unwrap(),
        )
        .unwrap();
        if matches!(message, ServerMessage::WorkspaceArchived { workspace_id } if workspace_id == old_workspace)
        {
            archived = true;
            break;
        }
    }
    assert!(archived);
    let switched = serde_json::from_str::<ServerMessage>(
        &socket.next().await.unwrap().unwrap().into_text().unwrap(),
    )
    .unwrap();
    let default_workspace = match switched {
        ServerMessage::WorkspaceSwitched { workspace_id } => workspace_id,
        other => panic!("{other:?}"),
    };
    assert_ne!(default_workspace, old_workspace);
    let created = serde_json::from_str::<ServerMessage>(
        &socket.next().await.unwrap().unwrap().into_text().unwrap(),
    )
    .unwrap();
    let new_session = match created {
        ServerMessage::SessionSnapshot {
            session_id,
            workspace_id: Some(workspace_id),
            ..
        } => {
            assert_eq!(workspace_id, default_workspace);
            session_id
        }
        other => panic!("{other:?}"),
    };
    assert_ne!(new_session, old_session);
    let registry = serde_json::from_str::<ServerMessage>(
        &socket.next().await.unwrap().unwrap().into_text().unwrap(),
    )
    .unwrap();
    assert!(
        matches!(registry, ServerMessage::Workspaces { active_workspace_id, ref workspaces } if active_workspace_id == default_workspace && workspaces.iter().any(|workspace| workspace.id == old_workspace && workspace.archived))
    );
    drop(socket);
    server.await.unwrap();
}
