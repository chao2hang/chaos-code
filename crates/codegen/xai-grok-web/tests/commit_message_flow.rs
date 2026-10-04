//! End-to-end coverage for the commit-message suggestion over the shipped Web
//! host: browser WebSocket -> `Engine` -> real `ProcessGitAdapter` on a real
//! repository -> `HttpPromptAdapter` -> HTTP/SSE.
//!
//! The only test double is the inference endpoint, which speaks real HTTP on a
//! loopback port. Everything else, including the git repository the suggestion
//! is drawn from, is the thing that ships.

use axum::serve;
use chaos_engine::{
    ClientMessage, Engine, ProcessGitAdapter, ServerMessage, provider::HttpPromptAdapter,
};
use futures_util::{SinkExt, StreamExt};
use std::{
    net::{Ipv4Addr, SocketAddr},
    path::Path,
    process::Command,
    sync::Arc,
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use xai_grok_test_support::{MockInferenceServer, MockModelEntry};
use xai_grok_web::router;

const MODEL: &str = "commit-model";
const TOKEN: &str = "sk-commit-token-0123456789";

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

async fn receive(socket: &mut Client) -> ServerMessage {
    let text = tokio::time::timeout(Duration::from_secs(30), socket.next())
        .await
        .expect("the host stopped answering")
        .expect("host closed the socket")
        .expect("websocket frame")
        .into_text()
        .expect("utf-8 frame");
    serde_json::from_str(&text).expect("server message")
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// A repository with one commit and one staged edit, plus a second edit that was
/// never staged. A suggestion must describe the first and not the second.
fn staged_repository() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "Commit Flow"]);
    git(
        root,
        &["config", "user.email", "commit-flow@example.invalid"],
    );
    std::fs::write(root.join("note.txt"), "first version\n").unwrap();
    git(root, &["add", "--", "note.txt"]);
    git(root, &["commit", "-q", "-m", "base"]);
    std::fs::write(root.join("note.txt"), "second version\n").unwrap();
    git(root, &["add", "--", "note.txt"]);
    std::fs::write(root.join("note.txt"), "second version\nthird, unstaged\n").unwrap();
    directory
}

fn adapter(base_url: &str, api_key: Option<&str>) -> Arc<dyn chaos_engine::PromptAdapter> {
    Arc::new(
        HttpPromptAdapter::new(base_url, MODEL, api_key.map(str::to_string))
            .expect("loopback provider configuration must validate"),
    )
}

/// Opens a session, asks for a commit message and collects until the host has
/// given its answer or its reason for refusing.
async fn request_suggestion(address: SocketAddr) -> Vec<ServerMessage> {
    let (mut socket, _) = connect_async(format!("ws://{address}/ws"))
        .await
        .expect("websocket connect");
    let _: ServerMessage = receive(&mut socket).await;
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
        .send(frame(&ClientMessage::SuggestCommitMessage {
            client_msg_id: format!("suggest-{}", uuid::Uuid::new_v4()),
            session_id,
        }))
        .await
        .unwrap();
    let mut events = Vec::new();
    loop {
        let event = receive(&mut socket).await;
        let terminal = matches!(
            event,
            ServerMessage::CommitMessageSuggestion { .. } | ServerMessage::Error { .. }
        );
        events.push(event);
        if terminal {
            return events;
        }
    }
}

fn suggestion(events: &[ServerMessage]) -> (String, bool) {
    events
        .iter()
        .find_map(|event| match event {
            ServerMessage::CommitMessageSuggestion {
                message, truncated, ..
            } => Some((message.clone(), *truncated)),
            _ => None,
        })
        .expect("no commit message suggestion")
}

#[tokio::test]
async fn the_staged_diff_reaches_the_endpoint_and_the_answer_comes_back() {
    let repository = staged_repository();
    let head = git(repository.path(), &["rev-parse", "HEAD"]);
    let provider =
        MockInferenceServer::start_with_required_auth(vec![MockModelEntry::new(MODEL)], TOKEN)
            .await
            .unwrap();
    // Wrapped the way a chatty model wraps an answer: what the browser is handed
    // has to be the message, so the wrapping is proof the host stripped it.
    provider.set_response("```text\ndocs: 改写 note.txt 的说明\n\n旧措辞描述的是上一个版本。\n```");
    let engine = Engine::with_workspace_and_adapter(
        repository.path(),
        Some(adapter(&provider.url(), Some(TOKEN))),
    )
    .unwrap()
    .with_git_adapter(ProcessGitAdapter::new(repository.path()).unwrap());
    let host = start_host(engine).await;

    let events = request_suggestion(host.address).await;
    let (message, truncated) = suggestion(&events);

    assert_eq!(
        message,
        "docs: 改写 note.txt 的说明\n\n旧措辞描述的是上一个版本。"
    );
    assert!(
        !truncated,
        "a two-line diff cannot be truncated: {events:?}"
    );

    let calls: Vec<_> = provider
        .requests()
        .into_iter()
        .filter(|entry| entry.path.ends_with("/chat/completions"))
        .collect();
    assert_eq!(calls.len(), 1, "exactly one inference call per suggestion");
    assert_eq!(
        calls[0].authorization.as_deref(),
        Some("Bearer sk-commit-token-0123456789"),
        "the host-side credential has to reach the endpoint"
    );
    let body = calls[0].body.clone().expect("chat completion body");
    assert_eq!(body["model"], MODEL);
    let prompt = body["messages"][0]["content"]
        .as_str()
        .expect("prompt text");
    assert!(
        prompt.contains("+second version"),
        "the staged hunk never reached the Provider: {prompt}"
    );
    assert!(
        !prompt.contains("third, unstaged"),
        "content that was never staged leaked into the prompt: {prompt}"
    );
    assert!(prompt.contains("---START STAGED DIFF---"));

    // Read-only: HEAD did not move and the staged area still holds the edit.
    assert_eq!(git(repository.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(
        git(repository.path(), &["diff", "--cached", "--name-only"]),
        "note.txt"
    );
    assert_eq!(
        git(repository.path(), &["log", "-1", "--format=%s"]),
        "base",
        "the suggestion must not have committed anything"
    );
}

#[tokio::test]
async fn a_host_without_a_provider_says_so_instead_of_inventing_a_message() {
    let repository = staged_repository();
    let engine = Engine::with_workspace(repository.path())
        .unwrap()
        .with_git_adapter(ProcessGitAdapter::new(repository.path()).unwrap());
    let host = start_host(engine).await;

    let events = request_suggestion(host.address).await;

    assert!(
        events
            .iter()
            .any(|event| matches!(event, ServerMessage::Error { code, .. } if code == "commit_suggestion_unavailable")),
        "{events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ServerMessage::CommitMessageSuggestion { .. })),
        "a refusal must not also deliver wording: {events:?}"
    );
}

#[tokio::test]
async fn a_failing_endpoint_is_reported_rather_than_answered_with_nothing() {
    let repository = staged_repository();
    let provider =
        MockInferenceServer::start_with_required_auth(vec![MockModelEntry::new(MODEL)], TOKEN)
            .await
            .unwrap();
    provider.set_response("this must never be delivered");
    let engine = Engine::with_workspace_and_adapter(
        repository.path(),
        Some(adapter(&provider.url(), Some("sk-wrong-credential"))),
    )
    .unwrap()
    .with_git_adapter(ProcessGitAdapter::new(repository.path()).unwrap());
    let host = start_host(engine).await;

    let events = request_suggestion(host.address).await;

    let (code, detail) = events
        .iter()
        .find_map(|event| match event {
            ServerMessage::Error { code, message } => Some((code.clone(), message.clone())),
            _ => None,
        })
        .expect("a rejected credential must produce an error");
    assert_eq!(code, "agent_failed");
    assert!(detail.contains("401"), "{detail}");
    assert_eq!(provider.request_count_for("/v1/chat/completions"), 1);
}
