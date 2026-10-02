//! End-to-end coverage for the GUI prompt path with a real inference
//! endpoint: browser WebSocket -> `Engine` -> `HttpPromptAdapter` -> HTTP/SSE.
//!
//! Every assertion here runs through the shipped `router()` and the shipped
//! adapter. The only test double is the inference server itself, which speaks
//! real HTTP on a loopback port.

use axum::serve;
use chaos_engine::{ClientMessage, Engine, ServerMessage, provider::HttpPromptAdapter};
use futures_util::{SinkExt, StreamExt};
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use xai_grok_test_support::{MockInferenceServer, MockModelEntry, ScriptedResponse};
use xai_grok_web::router;

const MODEL: &str = "gui-model";
const TOKEN: &str = "sk-provider-token-0123456789";

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

/// Runs `work` on a thread with no ambient runtime, which is what the shipped
/// WebSocket loop does for every prompt. `reqwest::blocking` refuses to start
/// or stop its private runtime on a thread that already belongs to one.
async fn off_runtime<T, F>(work: F) -> T
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    rx.await.expect("the adapter thread panicked")
}

async fn receive(socket: &mut Client) -> ServerMessage {
    let text = socket
        .next()
        .await
        .expect("host closed the socket")
        .expect("websocket frame")
        .into_text()
        .expect("utf-8 frame");
    serde_json::from_str(&text).expect("server message")
}

/// Opens a session, submits one prompt and collects events through the terminal
/// one, so a test asserts on the whole transcript rather than one message.
async fn submit(address: SocketAddr, prompt: &str) -> Vec<ServerMessage> {
    let (mut socket, _) = connect_async(format!("ws://{address}/ws"))
        .await
        .expect("websocket connect");
    let _: ServerMessage = receive(&mut socket).await;
    // Submission ids are deduplicated per engine, so two connections must not
    // reuse the same ones.
    socket
        .send(frame(&ClientMessage::CreateSession {
            client_msg_id: format!("create-{}", uuid::Uuid::new_v4()),
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
    socket
        .send(frame(&ClientMessage::Submit {
            client_msg_id: format!("submit-{}", uuid::Uuid::new_v4()),
            session_id,
            prompt: prompt.into(),
        }))
        .await
        .unwrap();
    let mut events = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(30), receive(&mut socket))
            .await
            .expect("the host never produced a terminal event");
        let terminal = matches!(
            event,
            ServerMessage::Completed { .. } | ServerMessage::Error { .. }
        );
        events.push(event);
        if terminal {
            return events;
        }
    }
}

fn adapter(base_url: &str, api_key: Option<&str>) -> Arc<dyn chaos_engine::PromptAdapter> {
    Arc::new(
        HttpPromptAdapter::new(base_url, MODEL, api_key.map(str::to_string))
            .expect("loopback provider configuration must validate"),
    )
}

/// Text the user would actually read, in arrival order.
fn streamed_text(events: &[ServerMessage]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            ServerMessage::TextDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn error_event(events: &[ServerMessage]) -> Option<(String, String)> {
    events.iter().find_map(|event| match event {
        ServerMessage::Error { code, message } => Some((code.clone(), message.clone())),
        _ => None,
    })
}

#[tokio::test]
async fn a_provider_reply_streams_over_the_websocket() {
    let provider =
        MockInferenceServer::start_with_required_auth(vec![MockModelEntry::new(MODEL)], TOKEN)
            .await
            .unwrap();
    provider.set_response("provider says hi");
    let host = start_host(Engine::with_adapter_arc(adapter(
        &provider.url(),
        Some(TOKEN),
    )))
    .await;

    let events = submit(host.address, "summarise this").await;

    assert_eq!(streamed_text(&events), "provider says hi");
    assert!(
        !streamed_text(&events).contains("演示响应"),
        "the demo responder must not answer once a provider is configured: {:?}",
        streamed_text(&events)
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ServerMessage::Completed { .. })),
        "{events:?}"
    );

    let calls: Vec<_> = provider
        .requests()
        .into_iter()
        .filter(|entry| entry.path.ends_with("/chat/completions"))
        .collect();
    assert_eq!(calls.len(), 1, "exactly one inference call per prompt");
    assert_eq!(
        calls[0].authorization.as_deref(),
        Some("Bearer sk-provider-token-0123456789"),
        "the host-side credential has to reach the endpoint"
    );
    assert_eq!(
        calls[0].header("content-type"),
        Some("application/json"),
        "the request body has to be typed as JSON"
    );
    let body = calls[0].body.clone().expect("chat completion body");
    assert_eq!(body["model"], MODEL);
    assert_eq!(body["stream"], true);
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["messages"][0]["content"], "summarise this");
}

#[tokio::test]
async fn a_rejected_credential_surfaces_as_agent_failed() {
    let provider =
        MockInferenceServer::start_with_required_auth(vec![MockModelEntry::new(MODEL)], TOKEN)
            .await
            .unwrap();
    provider.set_response("this must never be delivered");
    let host = start_host(Engine::with_adapter_arc(adapter(
        &provider.url(),
        Some("sk-wrong-credential"),
    )))
    .await;

    let events = submit(host.address, "hello").await;

    let (code, message) = error_event(&events).expect("a rejected key must produce an error");
    assert_eq!(code, "agent_failed");
    assert!(message.contains("401"), "{message}");
    assert!(message.contains("API Key"), "{message}");
    assert_eq!(streamed_text(&events), "");
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ServerMessage::Completed { .. })),
        "a failed turn must not look finished: {events:?}"
    );
}

#[tokio::test]
async fn an_upstream_failure_is_reported_with_its_status() {
    let provider = MockInferenceServer::start().await.unwrap();
    provider.enqueue_response(
        "/v1/chat/completions",
        ScriptedResponse::json(
            500,
            serde_json::json!({ "error": { "message": "gpu exploded" } }),
        ),
    );
    let host = start_host(Engine::with_adapter_arc(adapter(&provider.url(), None))).await;

    let events = submit(host.address, "hello").await;

    let (code, message) = error_event(&events).expect("a 500 must produce an error");
    assert_eq!(code, "agent_failed");
    assert!(message.contains("500"), "{message}");
    assert!(message.contains("服务端"), "{message}");
    assert!(message.contains("gpu exploded"), "{message}");
    assert_eq!(streamed_text(&events), "");
}

#[tokio::test]
async fn a_mid_stream_provider_error_is_not_delivered_as_text() {
    let provider = MockInferenceServer::start().await.unwrap();
    provider.enqueue_response(
        "/v1/chat/completions",
        ScriptedResponse::text(
            200,
            concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"usable prefix\"}}]}\n\n",
                "data: {\"error\":{\"message\":\"context overflow\"}}\n\n",
            ),
        ),
    );
    let host = start_host(Engine::with_adapter_arc(adapter(&provider.url(), None))).await;

    let events = submit(host.address, "hello").await;

    let (code, message) = error_event(&events).expect("an error frame must produce an error");
    assert_eq!(code, "agent_failed");
    assert!(message.contains("context overflow"), "{message}");
    assert!(!streamed_text(&events).contains("usable prefix"));
}

#[tokio::test]
async fn an_unreachable_endpoint_reports_a_connection_error() {
    // Bind then release, so the port is very likely unused.
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let host = start_host(Engine::with_adapter_arc(adapter(
        &format!("http://127.0.0.1:{port}/v1"),
        None,
    )))
    .await;

    let events = submit(host.address, "hello").await;

    let (code, message) = error_event(&events).expect("a dead port must produce an error");
    assert_eq!(code, "agent_failed");
    assert!(message.contains("无法建立连接"), "{message}");
    assert_eq!(streamed_text(&events), "");
}

#[tokio::test]
async fn an_echoed_credential_never_reaches_the_browser() {
    const SECRET: &str = "sk-leaked-abcdef0123456789";
    let provider = MockInferenceServer::start().await.unwrap();
    provider.enqueue_response(
        "/v1/chat/completions",
        ScriptedResponse::text(401, format!("invalid api key {SECRET}")),
    );
    let host = start_host(Engine::with_adapter_arc(adapter(
        &provider.url(),
        Some(SECRET),
    )))
    .await;

    let events = submit(host.address, "hello").await;

    let (_code, message) = error_event(&events).expect("a 401 must produce an error");
    assert!(message.contains("[redacted]"), "{message}");
    assert!(!message.contains(SECRET), "{message}");
    let rendered = format!("{events:?}");
    assert!(!rendered.contains(SECRET), "transcript leaked the key");
}

#[tokio::test]
async fn the_status_probe_lists_models_and_checks_the_configured_slug() {
    let provider = MockInferenceServer::start_with_models(vec![
        MockModelEntry::new("other-model"),
        MockModelEntry::new(MODEL),
    ])
    .await
    .unwrap();
    let url = provider.url();
    let health = off_runtime(move || {
        HttpPromptAdapter::new(&url, MODEL, None::<String>)
            .unwrap()
            .probe()
    })
    .await;

    assert!(health.reachable, "{health:?}");
    assert_eq!(health.model_ids, vec!["gui-model", "other-model"]);
    assert!(health.configured_model_known);
    assert_eq!(health.detail, None);

    let url = provider.url();
    let unknown = off_runtime(move || {
        HttpPromptAdapter::new(&url, "not-listed", None::<String>)
            .unwrap()
            .probe()
    })
    .await;
    assert!(unknown.reachable && !unknown.configured_model_known);

    // A closed port has to degrade into structured detail, not a panic.
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let dead_port = listener.local_addr().unwrap().port();
    drop(listener);
    let dead = off_runtime(move || {
        HttpPromptAdapter::new(
            &format!("http://127.0.0.1:{dead_port}/v1"),
            MODEL,
            None::<String>,
        )
        .unwrap()
        .probe()
    })
    .await;
    assert!(!dead.reachable);
    let detail = dead.detail.clone().expect("a dead port reports why");
    assert!(detail.contains("无法连接"), "{detail}");
}

/// The blocking adapter must stay usable from a thread with no runtime, which
/// is what `websocket_session` relies on after moving work off the async task.
#[tokio::test]
async fn concurrent_sessions_do_not_serialize_on_the_provider() {
    let provider = MockInferenceServer::start().await.unwrap();
    provider.set_response("same answer for everyone");
    let host = Arc::new(start_host(Engine::with_adapter_arc(adapter(&provider.url(), None))).await);

    let mut tasks = Vec::new();
    for index in 0..4 {
        let host = Arc::clone(&host);
        tasks.push(tokio::spawn(async move {
            let events = submit(host.address, &format!("prompt {index}")).await;
            streamed_text(&events)
        }));
    }
    for task in tasks {
        assert_eq!(task.await.unwrap(), "same answer for everyone");
    }
    assert_eq!(
        provider
            .requests()
            .iter()
            .filter(|entry| entry.path.ends_with("/chat/completions"))
            .count(),
        4
    );
}

/// An operator who set the base URL but forgot the key has to be told the
/// endpoint refused the credentials, not shown an empty answer.
#[tokio::test]
async fn a_missing_credential_is_reported_as_an_authentication_failure() {
    let provider =
        MockInferenceServer::start_with_required_auth(vec![MockModelEntry::new(MODEL)], TOKEN)
            .await
            .unwrap();
    provider.set_response("this must never be delivered");
    let host = start_host(Engine::with_adapter_arc(adapter(&provider.url(), None))).await;

    let events = submit(host.address, "hello").await;

    let (code, message) = error_event(&events).expect("a missing key must produce an error");
    assert_eq!(code, "agent_failed");
    assert!(message.contains("401"), "{message}");
    assert_eq!(streamed_text(&events), "");
    assert_eq!(
        provider
            .requests()
            .iter()
            .filter(|entry| entry.authorization.is_some())
            .count(),
        0,
        "an unconfigured key must not send an Authorization header"
    );
}

#[tokio::test]
async fn a_rate_limited_endpoint_tells_the_user_to_retry() {
    let provider = MockInferenceServer::start().await.unwrap();
    provider.enqueue_response(
        "/v1/chat/completions",
        ScriptedResponse::text(429, "rate limit exceeded"),
    );
    let host = start_host(Engine::with_adapter_arc(adapter(&provider.url(), None))).await;

    let events = submit(host.address, "hello").await;

    let (code, message) = error_event(&events).expect("a 429 must produce an error");
    assert_eq!(code, "agent_failed");
    assert!(message.contains("429"), "{message}");
    assert!(message.contains("限流"), "{message}");
    assert_eq!(streamed_text(&events), "");
}

/// A single bad frame is a provider-side bug, and swallowing it would show a
/// silently truncated answer as if it were complete.
#[tokio::test]
async fn a_malformed_stream_frame_fails_the_turn() {
    let provider = MockInferenceServer::start().await.unwrap();
    provider.enqueue_response(
        "/v1/chat/completions",
        ScriptedResponse::text(200, "data: this is not json\n\n"),
    );
    let host = start_host(Engine::with_adapter_arc(adapter(&provider.url(), None))).await;

    let events = submit(host.address, "hello").await;

    let (code, message) = error_event(&events).expect("a bad frame must produce an error");
    assert_eq!(code, "agent_failed");
    assert!(message.contains("无法解析"), "{message}");
    assert_eq!(streamed_text(&events), "");
}

#[tokio::test]
async fn a_completion_without_text_is_reported_instead_of_looking_finished() {
    let provider = MockInferenceServer::start().await.unwrap();
    provider.enqueue_response(
        "/v1/chat/completions",
        ScriptedResponse::text(
            200,
            "data: {\"choices\":[{\"delta\":{\"content\":\"\"}}]}\n\ndata: [DONE]\n\n",
        ),
    );
    let host = start_host(Engine::with_adapter_arc(adapter(&provider.url(), None))).await;

    let events = submit(host.address, "hello").await;

    let (code, message) = error_event(&events).expect("an empty completion must produce an error");
    assert_eq!(code, "agent_failed");
    assert!(message.contains("未返回任何文本"), "{message}");
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ServerMessage::Completed { .. })),
        "{events:?}"
    );
}

/// An endpoint that lists nothing is a configuration problem the operator can
/// fix; the probe has to say so rather than claim a healthy provider.
#[tokio::test]
async fn an_empty_model_catalog_is_reported_as_such() {
    let provider = MockInferenceServer::start_with_models(Vec::new())
        .await
        .unwrap();
    let url = provider.url();
    let health = off_runtime(move || {
        HttpPromptAdapter::new(&url, MODEL, None::<String>)
            .unwrap()
            .probe()
    })
    .await;

    assert!(health.reachable, "{health:?}");
    assert!(health.model_ids.is_empty(), "{health:?}");
    assert!(
        !health.configured_model_known,
        "a catalog with no entries cannot contain the configured model"
    );
}
