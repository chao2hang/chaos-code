//! What a browser sees while an answer is still being produced.
//!
//! These run over the shipped [`router`] with a real WebSocket client, so the
//! transport under test is the one `apps/chaos-ui` talks to. The things they hold
//! the host to are the ones a `Vec<ServerMessage>` round trip cannot express: a
//! chunk that reaches the socket before the Producer has finished, a second tab that
//! opened the same conversation and follows its turn, a tab that never opened it and
//! hears nothing of it, and a `cancel` that arrives while the turn is still going.

use axum::serve;
use chaos_engine::{ClientMessage, Engine, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use uuid::Uuid;
use xai_grok_test_support::{MockInferenceServer, MockModelEntry};
use xai_grok_web::{MAX_IN_FLIGHT_PER_CONNECTION, router};

const MODEL: &str = "streaming-model";
const TOKEN: &str = "sk-streaming-token-0123456789";

/// How long the harness waits for a frame that is expected to arrive.
const WAIT: Duration = Duration::from_secs(30);

type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Host {
    address: SocketAddr,
    server: JoinHandle<()>,
}

impl Drop for Host {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn start_host(engine: Engine) -> Host {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let _ = serve(listener, router(engine, "")).await;
    });
    Host { address, server }
}

fn frame<T: serde::Serialize>(value: &T) -> Message {
    Message::Text(serde_json::to_string(value).unwrap().into())
}

fn new_id(kind: &str) -> String {
    format!("{kind}-{}", Uuid::new_v4())
}

async fn receive(socket: &mut Client) -> ServerMessage {
    let text = tokio::time::timeout(WAIT, socket.next())
        .await
        .expect("the host stopped sending frames")
        .expect("the host closed the socket")
        .expect("websocket frame")
        .into_text()
        .expect("utf-8 frame");
    serde_json::from_str(&text).expect("server message")
}

/// Opens a connection and reads the handshake, without creating anything.
async fn connect(address: SocketAddr) -> Client {
    let (mut socket, _) = connect_async(format!("ws://{address}/ws"))
        .await
        .expect("connect");
    let handshake = receive(&mut socket).await;
    assert!(
        matches!(handshake, ServerMessage::Handshake { .. }),
        "{handshake:?}"
    );
    socket
}

/// Opens a connection and its own session, returning both.
async fn connect_with_session(address: SocketAddr) -> (Client, Uuid) {
    let mut socket = connect(address).await;
    socket
        .send(frame(&ClientMessage::CreateSession {
            client_msg_id: new_id("create"),
            workspace_id: None,
        }))
        .await
        .unwrap();
    let session_id = loop {
        match receive(&mut socket).await {
            ServerMessage::SessionCreated { session_id, .. } => break session_id,
            ServerMessage::Ack { .. } => continue,
            other => panic!("expected SessionCreated, got {other:?}"),
        }
    };
    (socket, session_id)
}

async fn submit(socket: &mut Client, session_id: Uuid, prompt: &str) {
    socket
        .send(frame(&ClientMessage::Submit {
            client_msg_id: new_id("submit"),
            session_id,
            prompt: prompt.into(),
        }))
        .await
        .unwrap();
}

/// Opens a session this connection did not create, which is what makes it a
/// follower of that session's events -- the request a tab makes when the user
/// clicks into a conversation that is already there.
async fn follow(socket: &mut Client, session_id: Uuid) {
    socket
        .send(frame(&ClientMessage::Snapshot {
            client_msg_id: new_id("snapshot"),
            session_id,
            workspace_id: None,
        }))
        .await
        .unwrap();
    loop {
        match receive(socket).await {
            ServerMessage::SessionSnapshot {
                session_id: opened, ..
            } => {
                assert_eq!(opened, session_id, "the host opened another session");
                return;
            }
            ServerMessage::Ack { .. } => continue,
            other => panic!("expected SessionSnapshot, got {other:?}"),
        }
    }
}

fn adapter(base_url: &str) -> Arc<dyn chaos_engine::PromptAdapter> {
    use chaos_engine::provider::HttpPromptAdapter;
    Arc::new(
        HttpPromptAdapter::new(base_url, MODEL, Some(TOKEN.to_string()))
            .expect("loopback provider configuration must validate"),
    )
}

/// A Provider reply that arrives one word at a time, paced so the difference
/// between "streamed" and "collected then flushed" is a second of wall clock.
const PACED_TEXT: &str = "第一 个 分 块 之 后 还 有 很 多 要 说 的 话";
const CHUNK_GAP: Duration = Duration::from_millis(120);

async fn paced_provider() -> MockInferenceServer {
    let provider =
        MockInferenceServer::start_with_required_auth(vec![MockModelEntry::new(MODEL)], TOKEN)
            .await
            .expect("mock provider");
    provider.set_response(PACED_TEXT);
    provider.set_chunk_delay(Some(CHUNK_GAP));
    provider
}

fn streamed(events: &[ServerMessage]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            ServerMessage::TextDelta { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

// --------------------------------------------------------------------------------
// Tests
// --------------------------------------------------------------------------------

#[tokio::test]
async fn deltas_reach_the_socket_before_the_producer_finishes() {
    let provider = paced_provider().await;
    let host = start_host(Engine::with_adapter_arc(adapter(&provider.url()))).await;
    let (mut socket, session_id) = connect_with_session(host.address).await;
    submit(&mut socket, session_id, "慢慢说").await;

    let started = Instant::now();
    let mut first: Option<Duration> = None;
    let mut events = Vec::new();
    loop {
        let event = receive(&mut socket).await;
        let terminal = matches!(
            event,
            ServerMessage::Completed { .. } | ServerMessage::Error { .. }
        );
        if matches!(event, ServerMessage::TextDelta { .. }) && first.is_none() {
            first = Some(started.elapsed());
        }
        events.push(event);
        if terminal {
            break;
        }
    }
    let total = started.elapsed();
    let chunks = streamed(&events);

    assert!(
        chunks.len() > 1,
        "the provider sent {} paced chunks; the socket saw {chunks:?}",
        PACED_TEXT.split(' ').count()
    );
    assert_eq!(chunks.concat(), PACED_TEXT, "the transcript lost a chunk");
    let first = first.expect("no TextDelta ever reached the socket");
    assert!(
        first * 2 < total,
        "the first chunk waited {first:?} of the turn's {total:?}, so the reply was \
         collected and flushed at the end"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ServerMessage::Completed { .. })),
        "{events:?}"
    );
}

#[tokio::test]
async fn a_second_tab_that_opened_the_session_follows_the_turn() {
    let provider = paced_provider().await;
    let host = start_host(Engine::with_adapter_arc(adapter(&provider.url()))).await;
    let (mut owner, session_id) = connect_with_session(host.address).await;
    // The observer follows before the turn starts, the way a second tab does when
    // the user opens the same conversation while an answer is still being written.
    // Opening it is what entitles it to see it; see
    // `a_tab_that_never_opened_the_session_never_sees_the_turn`.
    let mut observer = connect(host.address).await;
    follow(&mut observer, session_id).await;
    submit(&mut owner, session_id, "说给两个人看").await;

    let mut watched = Vec::new();
    loop {
        let event = receive(&mut observer).await;
        let terminal = matches!(
            event,
            ServerMessage::Completed { .. } | ServerMessage::Error { .. }
        );
        watched.push(event);
        if terminal {
            break;
        }
    }

    assert_eq!(
        streamed(&watched).concat(),
        PACED_TEXT,
        "the second connection did not watch the whole turn: {watched:?}"
    );
    assert!(
        watched
            .iter()
            .any(|event| matches!(event, ServerMessage::Completed { .. })),
        "{watched:?}"
    );
    assert!(
        !watched
            .iter()
            .any(|event| matches!(event, ServerMessage::SessionCreated { .. })),
        "one tab opening a session hijacked another tab's view: {watched:?}"
    );

    // The owner still sees its own turn end exactly once.
    let mut owned = Vec::new();
    loop {
        let event = receive(&mut owner).await;
        let done = matches!(event, ServerMessage::Completed { .. });
        owned.push(event);
        if done {
            break;
        }
    }
    assert!(
        owned
            .iter()
            .filter(|event| matches!(event, ServerMessage::Completed { .. }))
            .count()
            == 1,
        "the turn settled twice on the socket that started it: {owned:?}"
    );
}

#[tokio::test]
async fn a_tab_that_never_opened_the_session_never_sees_the_turn() {
    let provider = paced_provider().await;
    let host = start_host(Engine::with_adapter_arc(adapter(&provider.url()))).await;
    // Connected to the same host, past the same handshake, and never named a session.
    let mut bystander = connect(host.address).await;
    let (mut owner, session_id) = connect_with_session(host.address).await;
    submit(&mut owner, session_id, "只说给一个标签看").await;

    loop {
        let event = receive(&mut owner).await;
        assert!(!matches!(event, ServerMessage::Error { .. }), "{event:?}");
        if matches!(event, ServerMessage::Completed { .. }) {
            break;
        }
    }

    // The turn is over, so nothing is producing any more; had any of it been routed
    // here, the frame would already be queued on this socket.
    if let Ok(frame) = tokio::time::timeout(Duration::from_millis(500), bystander.next()).await {
        panic!("a session nobody opened here still reached it: {frame:?}");
    }
}

#[tokio::test]
async fn cancel_reaches_a_turn_that_is_still_streaming() {
    let provider = paced_provider().await;
    let host = start_host(Engine::with_adapter_arc(adapter(&provider.url()))).await;
    let (mut socket, session_id) = connect_with_session(host.address).await;
    submit(&mut socket, session_id, "说到一半停下").await;

    // Two chunks in, the turn is demonstrably still being produced: at this pacing
    // the reply runs for over a second.
    let mut chunks = 0;
    while chunks < 2 {
        match receive(&mut socket).await {
            ServerMessage::TextDelta { .. } => chunks += 1,
            ServerMessage::Completed { .. } => {
                panic!("the turn finished before the cancel was even sent")
            }
            other => assert!(!matches!(other, ServerMessage::Error { .. }), "{other:?}"),
        }
    }
    socket
        .send(frame(&ClientMessage::Cancel {
            client_msg_id: new_id("cancel"),
            session_id,
        }))
        .await
        .unwrap();

    let mut saw_interrupted = false;
    let mut terminal = None;
    while terminal.is_none() {
        match receive(&mut socket).await {
            ServerMessage::Audit {
                action, outcome, ..
            } => {
                assert_eq!(action, "cancel");
                saw_interrupted |= outcome == "interrupted";
            }
            event @ (ServerMessage::Completed { .. } | ServerMessage::Cancelled { .. }) => {
                terminal = Some(event)
            }
            ServerMessage::TextDelta { .. } => chunks += 1,
            other => assert!(!matches!(other, ServerMessage::Error { .. }), "{other:?}"),
        }
    }

    assert!(
        saw_interrupted,
        "the audit did not say a live run was interrupted"
    );
    assert!(
        matches!(terminal, Some(ServerMessage::Cancelled { .. })),
        "an interrupted turn reported {terminal:?}"
    );
    assert!(
        chunks < PACED_TEXT.split(' ').count(),
        "every chunk arrived anyway ({chunks} of {})",
        PACED_TEXT.split(' ').count()
    );

    // The session is usable again afterwards, which is what clearing the
    // registration is for.
    submit(&mut socket, session_id, "那再来一轮").await;
    let mut restarted = Vec::new();
    loop {
        let event = tokio::time::timeout(WAIT, receive(&mut socket))
            .await
            .expect("the session stayed busy after the cancel");
        let done = matches!(
            event,
            ServerMessage::Completed { .. } | ServerMessage::Error { .. }
        );
        restarted.push(event);
        if done {
            break;
        }
    }
    assert!(
        !restarted.iter().any(
            |event| matches!(event, ServerMessage::Error { code, .. } if code == "session_busy")
        ),
        "the cancelled run still owned the session: {restarted:?}"
    );
}

#[tokio::test]
async fn a_connection_holding_too_many_requests_is_told_so_by_name() {
    // An approved command occupies the connection until the process ends, which is
    // the honest way to fill the queue: a prompt hands its answer to a run thread and
    // is answered at once, so it holds nothing.
    let directory = tempfile::tempdir().unwrap();
    let engine = Engine::with_workspace(directory.path())
        .unwrap()
        .with_terminal_adapter(
            chaos_engine::ProcessTerminalAdapter::new(directory.path(), 4096).unwrap(),
        );
    let host = start_host(engine).await;
    let (mut socket, session_id) = connect_with_session(host.address).await;

    socket
        .send(frame(&ClientMessage::ProposeTerminal {
            client_msg_id: new_id("propose"),
            session_id,
            command: "sleep 2".into(),
        }))
        .await
        .unwrap();
    let request_id = loop {
        match receive(&mut socket).await {
            ServerMessage::ToolApprovalRequested { request_id, .. } => break request_id,
            ServerMessage::Ack { .. } => continue,
            other => panic!("expected an approval request, got {other:?}"),
        }
    };
    socket
        .send(frame(&ClientMessage::Approve {
            client_msg_id: new_id("approve"),
            request_id,
        }))
        .await
        .unwrap();

    // The approval is being executed; everything below is what arrives while it is.
    // Sized from the host's own bound rather than a number that happens to work:
    // one permit belongs to the command, and a burst of the limit plus one cannot
    // be admitted whether that one has been handed back yet or not.
    const FURTHER: usize = MAX_IN_FLIGHT_PER_CONNECTION + 1;
    for _ in 0..FURTHER {
        socket
            .send(frame(&ClientMessage::GetHostInfo {
                client_msg_id: new_id("info"),
            }))
            .await
            .unwrap();
    }

    let mut refused = 0;
    let mut answered = 0;
    let mut command_result = false;
    // The refusals come straight back; what was accepted is answered once the
    // command ends. The poll stops on a quiet socket rather than on a count, because
    // an accepted frame is an Ack plus its answer.
    let quiet = Duration::from_secs(5);
    let deadline = Instant::now() + WAIT;
    while Instant::now() < deadline {
        let Ok(Some(event)) = tokio::time::timeout(quiet, socket.next()).await else {
            break;
        };
        match serde_json::from_str::<ServerMessage>(
            &event
                .expect("websocket frame")
                .into_text()
                .expect("utf-8 frame"),
        )
        .expect("server message")
        {
            ServerMessage::Error { code, message } => {
                assert_eq!(code, "too_many_in_flight", "{code}: {message}");
                assert!(!message.is_empty());
                refused += 1;
            }
            ServerMessage::TerminalResult { .. } => command_result = true,
            ServerMessage::HostInfo { .. } | ServerMessage::Ack { .. } => answered += 1,
            other => assert!(
                !matches!(other, ServerMessage::Error { .. }),
                "unexpected {other:?}"
            ),
        }
    }

    assert!(
        command_result,
        "the approved command never reported, so the queue was not what held it"
    );
    assert!(answered > 0, "nothing was accepted at all");
    assert!(
        refused > 0,
        "one socket queued {} further requests on top of a running command with no refusal",
        FURTHER
    );
}

/// A connection that started a turn keeps answering everything else on it.
///
/// The stop button, the file panel and the settings panel all share the socket with
/// the prompt, so a host that stayed inside the request until the Provider had
/// finished could not serve any of them -- which is how the answer looked frozen.
#[tokio::test]
async fn the_connection_that_started_a_turn_still_answers_everything_else() {
    let provider = paced_provider().await;
    let host = start_host(Engine::with_adapter_arc(adapter(&provider.url()))).await;
    let (mut socket, session_id) = connect_with_session(host.address).await;
    submit(&mut socket, session_id, "一边说一边查").await;

    let mut chunks = 0;
    while chunks < 2 {
        match receive(&mut socket).await {
            ServerMessage::TextDelta { .. } => chunks += 1,
            ServerMessage::Completed { .. } => {
                panic!("the turn finished before anything else was asked of it")
            }
            ServerMessage::Ack { .. } => continue,
            other => assert!(!matches!(other, ServerMessage::Error { .. }), "{other:?}"),
        }
    }

    let asked = Instant::now();
    socket
        .send(frame(&ClientMessage::GetHostInfo {
            client_msg_id: new_id("info"),
        }))
        .await
        .unwrap();
    loop {
        let event = receive(&mut socket).await;
        if matches!(event, ServerMessage::HostInfo { .. }) {
            break;
        }
        assert!(
            !matches!(event, ServerMessage::Completed { .. }),
            "the turn ended before the socket answered a request made during it"
        );
    }
    let waited = asked.elapsed();
    let chunks_at = PACED_TEXT.split(' ').count();
    assert!(
        waited < CHUNK_GAP * (chunks_at as u32 - 1) / 2,
        "a request made mid-turn waited {waited:?}, so the socket waited for its own turn"
    );
}

/// The engine refuses an unanswered question before it ever reaches a Provider;
/// this keeps that ordering true through the socket, where the refusal is the only
/// thing a client gets to go on.
#[tokio::test]
async fn a_prompt_while_a_question_is_pending_is_refused_over_the_socket() {
    let provider = paced_provider().await;
    let host = start_host(Engine::with_adapter_arc(adapter(&provider.url()))).await;
    let (mut socket, session_id) = connect_with_session(host.address).await;

    submit(&mut socket, session_id, "/ask 先回答我").await;
    loop {
        match receive(&mut socket).await {
            ServerMessage::QuestionRequested { .. } => break,
            ServerMessage::Ack { .. } => continue,
            other => panic!("expected QuestionRequested, got {other:?}"),
        }
    }

    submit(&mut socket, session_id, "那我换个问题").await;
    let mut code = None;
    while code.is_none() {
        match receive(&mut socket).await {
            ServerMessage::Error { code: reason, .. } => code = Some(reason),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
    assert_eq!(code.expect("no refusal"), "question_pending");
}
