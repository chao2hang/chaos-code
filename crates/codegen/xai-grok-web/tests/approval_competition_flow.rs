use axum::serve;
use chaos_engine::{ClientMessage, Engine, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::router;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn next(socket: &mut Socket) -> ServerMessage {
    let message = tokio::time::timeout(Duration::from_secs(3), socket.next())
        .await
        .expect("timed out waiting for WebSocket event")
        .unwrap()
        .unwrap();
    serde_json::from_str(&message.into_text().unwrap()).unwrap()
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
async fn two_websocket_clients_can_resolve_one_approval_only_once() {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        serve(listener, router(Engine::new(), ""))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_secs(4)).await })
            .await
            .unwrap();
    });
    let (mut first, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
    let (mut second, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
    assert!(matches!(
        next(&mut first).await,
        ServerMessage::Handshake { .. }
    ));
    assert!(matches!(
        next(&mut second).await,
        ServerMessage::Handshake { .. }
    ));
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
        other => panic!("unexpected {other:?}"),
    };
    send(
        &mut first,
        ClientMessage::Submit {
            client_msg_id: "submit".into(),
            session_id,
            prompt: "/approve-tool test".into(),
        },
    )
    .await;
    let _ = next(&mut first).await;
    let request_id = match next(&mut first).await {
        ServerMessage::ToolApprovalRequested { request_id, .. } => request_id,
        other => panic!("unexpected {other:?}"),
    };
    send(
        &mut first,
        ClientMessage::Approve {
            client_msg_id: "approve-first".into(),
            request_id,
        },
    )
    .await;
    let first_ack = next(&mut first).await;
    assert!(matches!(first_ack, ServerMessage::Ack { .. }));
    send(
        &mut second,
        ClientMessage::Approve {
            client_msg_id: "approve-second".into(),
            request_id,
        },
    )
    .await;
    let second_result = next(&mut second).await;
    assert!(
        matches!(second_result, ServerMessage::Error { code, .. } if code == "approval_not_found")
    );
    assert!(matches!(
        next(&mut first).await,
        ServerMessage::ToolStarted { .. }
    ));
    assert!(
        matches!(next(&mut first).await, ServerMessage::Error { code, .. } if code == "tool_unavailable")
    );
    let resolution = next(&mut first).await;
    match resolution {
        ServerMessage::ApprovalResolved {
            approved: false, ..
        } => {
            assert!(
                matches!(next(&mut first).await, ServerMessage::Audit { outcome, .. } if outcome == "unavailable")
            );
        }
        ServerMessage::Audit { outcome, .. } => assert_eq!(outcome, "unavailable"),
        other => panic!("unexpected {other:?}"),
    }
    drop(first);
    drop(second);
    server.await.unwrap();
}
