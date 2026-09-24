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
async fn pending_question_prompt_survives_disconnect_and_resume() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("state.json");
    let engine = Engine::with_persistence(&path).unwrap();
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        serve(listener, router(engine, ""))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_secs(5)).await })
            .await
            .unwrap();
    });
    let url = format!("ws://{address}/ws");
    let (mut first, _) = connect_async(&url).await.unwrap();
    let _ = next(&mut first).await;
    send(
        &mut first,
        ClientMessage::CreateSession {
            client_msg_id: "create".into(),
            workspace_id: None,
        },
    )
    .await;
    let session_id = match next(&mut first).await {
        ServerMessage::SessionCreated { session_id, .. } => session_id,
        other => panic!("{other:?}"),
    };
    send(
        &mut first,
        ClientMessage::Submit {
            client_msg_id: "ask".into(),
            session_id,
            prompt: "/ask choose a color".into(),
        },
    )
    .await;
    let _ = next(&mut first).await;
    let question_id = match next(&mut first).await {
        ServerMessage::QuestionRequested { question_id, .. } => question_id,
        other => panic!("{other:?}"),
    };
    drop(first);

    let (mut second, _) = connect_async(&url).await.unwrap();
    let _ = next(&mut second).await;
    send(
        &mut second,
        ClientMessage::Resume {
            client_msg_id: "resume-question".into(),
            session_id,
            workspace_id: None,
        },
    )
    .await;
    assert!(
        matches!(next(&mut second).await, ServerMessage::SessionSnapshot { pending_question: Some(question), .. } if question.question_id == question_id && question.prompt == "choose a color")
    );
    send(
        &mut second,
        ClientMessage::RespondQuestion {
            client_msg_id: "answer".into(),
            question_id,
            answer: "blue".into(),
        },
    )
    .await;
    let mut resolved = false;
    for _ in 0..4 {
        if matches!(next(&mut second).await, ServerMessage::QuestionResolved { question_id: resolved_id, answer, .. } if resolved_id == question_id && answer == "blue")
        {
            resolved = true;
            break;
        }
    }
    assert!(resolved);
    drop(second);
    server.await.unwrap();
}
