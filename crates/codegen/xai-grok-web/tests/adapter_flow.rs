use axum::serve;
use chaos_engine::{ClientMessage, Engine, PromptAdapter, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::router;

struct TestAgent;

impl PromptAdapter for TestAgent {
    fn run_prompt(&self, prompt: &str) -> Result<Vec<String>, String> {
        Ok(vec![format!("agent:{prompt}")])
    }
}

#[tokio::test]
async fn websocket_uses_configured_agent_adapter_for_submit() {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        serve(listener, router(Engine::with_adapter(TestAgent), ""))
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
        other => panic!("unexpected {other:?}"),
    };
    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::Submit {
                client_msg_id: "submit".into(),
                session_id,
                prompt: "hello".into(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let mut saw_agent_text = false;
    let mut saw_completed = false;
    for _ in 0..8 {
        let event = serde_json::from_str::<ServerMessage>(
            &socket.next().await.unwrap().unwrap().into_text().unwrap(),
        )
        .unwrap();
        saw_agent_text |=
            matches!(&event, ServerMessage::TextDelta { text, .. } if text == "agent:hello");
        saw_completed |= matches!(event, ServerMessage::Completed { .. });
        if saw_agent_text && saw_completed {
            break;
        }
    }
    assert!(saw_agent_text && saw_completed);
    server.await.unwrap();
}
