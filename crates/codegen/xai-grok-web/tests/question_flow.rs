use axum::serve;
use chaos_engine::{ClientMessage, Engine, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::router;

#[tokio::test]
async fn websocket_question_requires_an_explicit_answer() {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        serve(listener, router(Engine::new(), ""))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_millis(250)).await })
            .await
            .unwrap();
    });
    let (mut socket, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
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
    let session_id = match serde_json::from_str::<ServerMessage>(
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
                client_msg_id: "ask".into(),
                session_id,
                prompt: "/ask continue?".into(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let question_id = loop {
        let event = serde_json::from_str::<ServerMessage>(
            &socket.next().await.unwrap().unwrap().into_text().unwrap(),
        )
        .unwrap();
        if let ServerMessage::QuestionRequested { question_id, .. } = event {
            break question_id;
        }
    };
    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::RespondQuestion {
                client_msg_id: "answer".into(),
                question_id,
                answer: "是".into(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let mut resolved = false;
    for _ in 0..3 {
        let event = serde_json::from_str::<ServerMessage>(
            &socket.next().await.unwrap().unwrap().into_text().unwrap(),
        )
        .unwrap();
        resolved |=
            matches!(event, ServerMessage::QuestionResolved { answer, .. } if answer == "是");
        if resolved {
            break;
        }
    }
    assert!(resolved);
    server.await.unwrap();
}
