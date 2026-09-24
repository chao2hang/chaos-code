use axum::serve;
use chaos_engine::{ClientMessage, Engine, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::router;

async fn start(engine: Engine) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        serve(listener, router(engine, ""))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_millis(400)).await })
            .await
            .unwrap();
    });
    (format!("ws://{address}/ws"), task)
}

async fn create_and_submit(url: &str) -> uuid::Uuid {
    let (mut socket, _) = connect_async(url).await.unwrap();
    let _: ServerMessage =
        serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::CreateSession {
                client_msg_id: "create".into(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let id = match serde_json::from_str::<ServerMessage>(
        &socket.next().await.unwrap().unwrap().into_text().unwrap(),
    )
    .unwrap()
    {
        ServerMessage::SessionCreated { session_id } => session_id,
        other => panic!("unexpected {other:?}"),
    };
    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::Submit {
                client_msg_id: "submit".into(),
                session_id: id,
                prompt: "persisted".into(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    for _ in 0..8 {
        let event = serde_json::from_str::<ServerMessage>(
            &socket.next().await.unwrap().unwrap().into_text().unwrap(),
        )
        .unwrap();
        if matches!(event, ServerMessage::Completed { .. }) {
            break;
        }
    }
    id
}

#[tokio::test]
async fn websocket_resume_reads_snapshot_after_engine_restart() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("sessions.json");
    let first = Engine::with_persistence(&path).unwrap();
    let (url, task) = start(first).await;
    let session_id = create_and_submit(&url).await;
    task.await.unwrap();

    let second = Engine::with_persistence(&path).unwrap();
    let (url, task) = start(second).await;
    let (mut socket, _) = connect_async(url).await.unwrap();
    let _: ServerMessage =
        serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::Resume {
                client_msg_id: "resume".into(),
                session_id,
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let snapshot: ServerMessage =
        serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    assert!(
        matches!(snapshot, ServerMessage::SessionSnapshot { messages, .. } if messages.iter().any(|message| message.text == "persisted"))
    );
    task.await.unwrap();
}
