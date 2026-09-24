use axum::serve;
use chaos_engine::{ClientMessage, Engine, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::router_with_safe_mode;

#[tokio::test]
async fn safe_web_mode_blocks_direct_mutation_protocol_calls() {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        serve(listener, router_with_safe_mode(Engine::new(), "", true))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_millis(300)).await })
            .await
            .unwrap();
    });
    let (mut socket, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
    let _: ServerMessage =
        serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    socket
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::ProposeTerminal {
                client_msg_id: "terminal".into(),
                session_id: uuid::Uuid::new_v4(),
                command: "printf blocked".into(),
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let result: ServerMessage =
        serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    assert!(matches!(result, ServerMessage::Error { code, .. } if code == "safe_web_mode_blocked"));
    server.await.unwrap();
}
