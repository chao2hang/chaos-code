use axum::serve;
use chaos_engine::{ClientMessage, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::router;

#[tokio::test]
async fn websocket_drives_real_session_create_submit_and_cancel_flow() {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        serve(listener, router(chaos_engine::Engine::new(), ""))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_millis(250)).await })
            .await
            .unwrap();
    });

    let (mut socket, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
    let handshake: ServerMessage = serde_json::from_str(
        socket
            .next()
            .await
            .unwrap()
            .unwrap()
            .into_text()
            .unwrap()
            .as_str(),
    )
    .unwrap();
    assert!(matches!(
        handshake,
        ServerMessage::Handshake {
            protocol_version: 1
        }
    ));

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
    let created: ServerMessage =
        serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    let session_id = match created {
        ServerMessage::SessionCreated { session_id, .. } => session_id,
        other => panic!("unexpected {other:?}"),
    };

    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::Submit {
                client_msg_id: "submit".into(),
                session_id,
                prompt: "真实 WebSocket".into(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let mut saw_delta = false;
    let mut saw_completed = false;
    for _ in 0..10 {
        let event: ServerMessage =
            serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap())
                .unwrap();
        saw_delta |= matches!(event, ServerMessage::TextDelta { .. });
        saw_completed |= matches!(event, ServerMessage::Completed { .. });
        if saw_completed {
            break;
        }
    }
    assert!(saw_delta && saw_completed);

    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::Cancel {
                client_msg_id: "cancel".into(),
                session_id,
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let mut saw_cancel = false;
    for _ in 0..3 {
        let event: ServerMessage =
            serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap())
                .unwrap();
        saw_cancel |= matches!(event, ServerMessage::Cancelled { .. });
        if saw_cancel {
            break;
        }
    }
    assert!(saw_cancel);
    server.await.unwrap();
}
