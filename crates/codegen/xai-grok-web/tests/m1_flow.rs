use axum::serve;
use chaos_engine::{ClientMessage, DiffAdapter, Engine, ServerMessage, ToolAdapter};
use futures_util::{SinkExt, StreamExt};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::router;

#[derive(Clone, Default)]
struct ToolLog(Arc<Mutex<Vec<String>>>);
impl ToolAdapter for ToolLog {
    fn execute(&self, tool: &str, summary: &str) -> Result<String, String> {
        self.0.lock().unwrap().push(format!("{tool}:{summary}"));
        Ok("tool-result".into())
    }
}

#[derive(Clone, Default)]
struct DiffLog(Arc<Mutex<Vec<String>>>);
impl DiffAdapter for DiffLog {
    fn accept(&self, proposal_id: &str, summary: &str) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .push(format!("accept:{proposal_id}:{summary}"));
        Ok(())
    }
    fn rollback(&self, proposal_id: &str) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .push(format!("rollback:{proposal_id}"));
        Ok(())
    }
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(engine: Engine) -> (Socket, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        serve(listener, router(engine, ""))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_millis(900)).await })
            .await
            .unwrap();
    });
    let (mut socket, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
    let _: ServerMessage =
        serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    (socket, task)
}

async fn send(socket: &mut Socket, message: ClientMessage) {
    socket
        .send(Message::Text(
            serde_json::to_string(&message).unwrap().into(),
        ))
        .await
        .unwrap();
}

async fn next(socket: &mut Socket) -> ServerMessage {
    serde_json::from_str(&socket.next().await.unwrap().unwrap().into_text().unwrap()).unwrap()
}

#[tokio::test]
async fn websocket_tool_approval_emits_ordered_ack_resolution_audit() {
    let log = ToolLog::default();
    let engine = Engine::with_tool_adapter(log.clone());
    let (mut socket, task) = connect(engine).await;
    send(
        &mut socket,
        ClientMessage::CreateSession {
            workspace_id: None,
            client_msg_id: "create".into(),
        },
    )
    .await;
    let session_id = match next(&mut socket).await {
        ServerMessage::SessionCreated { session_id, .. } => session_id,
        other => panic!("{other:?}"),
    };
    send(
        &mut socket,
        ClientMessage::Submit {
            client_msg_id: "submit".into(),
            session_id,
            prompt: "/approve-tool write".into(),
        },
    )
    .await;
    assert!(matches!(next(&mut socket).await, ServerMessage::Ack { .. }));
    let request_id = match next(&mut socket).await {
        ServerMessage::ToolApprovalRequested { request_id, .. } => request_id,
        other => panic!("{other:?}"),
    };
    send(
        &mut socket,
        ClientMessage::Approve {
            client_msg_id: "approve".into(),
            request_id,
        },
    )
    .await;
    assert!(matches!(next(&mut socket).await, ServerMessage::Ack { .. }));
    assert!(matches!(
        next(&mut socket).await,
        ServerMessage::ToolStarted { .. }
    ));
    assert!(
        matches!(next(&mut socket).await, ServerMessage::ToolProgress { progress, .. } if progress == "completed")
    );
    assert!(
        matches!(next(&mut socket).await, ServerMessage::ToolResult { result, .. } if result == "tool-result")
    );
    assert!(matches!(
        next(&mut socket).await,
        ServerMessage::Usage { .. }
    ));
    assert!(matches!(
        next(&mut socket).await,
        ServerMessage::ApprovalResolved { approved: true, .. }
    ));
    assert!(
        matches!(next(&mut socket).await, ServerMessage::Audit { outcome, .. } if outcome == "executed")
    );
    assert_eq!(&*log.0.lock().unwrap(), &["demo.tool:write"]);
    task.await.unwrap();
}

#[tokio::test]
async fn websocket_repeated_approval_and_question_resolution_fail_closed() {
    let log = ToolLog::default();
    let (mut socket, task) = connect(Engine::with_tool_adapter(log)).await;
    send(
        &mut socket,
        ClientMessage::CreateSession {
            workspace_id: None,
            client_msg_id: "create".into(),
        },
    )
    .await;
    let session_id = match next(&mut socket).await {
        ServerMessage::SessionCreated { session_id, .. } => session_id,
        other => panic!("{other:?}"),
    };
    send(
        &mut socket,
        ClientMessage::Submit {
            client_msg_id: "ask".into(),
            session_id,
            prompt: "/ask continue?".into(),
        },
    )
    .await;
    assert!(matches!(next(&mut socket).await, ServerMessage::Ack { .. }));
    let question_id = match next(&mut socket).await {
        ServerMessage::QuestionRequested { question_id, .. } => question_id,
        other => panic!("{other:?}"),
    };
    send(
        &mut socket,
        ClientMessage::RespondQuestion {
            client_msg_id: "answer".into(),
            question_id,
            answer: "yes".into(),
        },
    )
    .await;
    assert!(matches!(next(&mut socket).await, ServerMessage::Ack { .. }));
    assert!(matches!(
        next(&mut socket).await,
        ServerMessage::QuestionResolved { .. }
    ));
    assert!(matches!(
        next(&mut socket).await,
        ServerMessage::Audit { .. }
    ));
    send(
        &mut socket,
        ClientMessage::RespondQuestion {
            client_msg_id: "answer-again".into(),
            question_id,
            answer: "no".into(),
        },
    )
    .await;
    assert!(
        matches!(next(&mut socket).await, ServerMessage::Error { code, .. } if code == "question_not_found")
    );
    task.await.unwrap();
}

#[tokio::test]
async fn websocket_diff_without_adapter_reports_structured_failure() {
    let (mut socket, task) = connect(Engine::new()).await;
    send(
        &mut socket,
        ClientMessage::CreateSession {
            workspace_id: None,
            client_msg_id: "create".into(),
        },
    )
    .await;
    let session_id = match next(&mut socket).await {
        ServerMessage::SessionCreated { session_id, .. } => session_id,
        other => panic!("{other:?}"),
    };
    send(
        &mut socket,
        ClientMessage::AcceptDiff {
            client_msg_id: "accept".into(),
            session_id,
            proposal_id: "p1".into(),
            summary: "safe".into(),
        },
    )
    .await;
    assert!(matches!(next(&mut socket).await, ServerMessage::Ack { .. }));
    assert!(
        matches!(next(&mut socket).await, ServerMessage::Error { code, .. } if code == "diff_failed")
    );
    assert!(
        matches!(next(&mut socket).await, ServerMessage::DiffResolved { action, .. } if action == "accept_diff")
    );
    task.await.unwrap();
}

#[tokio::test]
async fn websocket_diff_accept_and_rollback_use_the_bound_adapter() {
    let log = DiffLog::default();
    let engine = Engine::with_diff_adapter(log.clone());
    let (mut socket, task) = connect(engine).await;
    send(
        &mut socket,
        ClientMessage::CreateSession {
            workspace_id: None,
            client_msg_id: "create".into(),
        },
    )
    .await;
    let session_id = match next(&mut socket).await {
        ServerMessage::SessionCreated { session_id, .. } => session_id,
        other => panic!("{other:?}"),
    };
    send(
        &mut socket,
        ClientMessage::AcceptDiff {
            client_msg_id: "accept".into(),
            session_id,
            proposal_id: "p1".into(),
            summary: "safe".into(),
        },
    )
    .await;
    assert!(matches!(next(&mut socket).await, ServerMessage::Ack { .. }));
    assert!(
        matches!(next(&mut socket).await, ServerMessage::DiffResolved { action, .. } if action == "accept_diff")
    );
    assert!(
        matches!(next(&mut socket).await, ServerMessage::Audit { outcome, .. } if outcome == "applied")
    );
    send(
        &mut socket,
        ClientMessage::RollbackDiff {
            client_msg_id: "rollback".into(),
            session_id,
            proposal_id: "p1".into(),
        },
    )
    .await;
    assert!(matches!(next(&mut socket).await, ServerMessage::Ack { .. }));
    assert!(
        matches!(next(&mut socket).await, ServerMessage::DiffResolved { action, .. } if action == "rollback_diff")
    );
    assert_eq!(&*log.0.lock().unwrap(), &["accept:p1:safe", "rollback:p1"]);
    task.await.unwrap();
}
