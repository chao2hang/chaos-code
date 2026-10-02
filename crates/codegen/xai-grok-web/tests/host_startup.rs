//! Starts the built `chaos-web` binary itself, so the environment handling in
//! `main.rs` is covered end to end: provider configuration, the startup health
//! check, and what a browser actually receives over the WebSocket.

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::{
    io::BufRead,
    net::TcpListener,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use xai_grok_test_support::{MockInferenceServer, MockModelEntry};

const MODEL: &str = "host-model";

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("free port")
        .local_addr()
        .unwrap()
        .port()
}

struct Host {
    child: Child,
    lines: Arc<Mutex<Vec<String>>>,
    port: u16,
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Host {
    /// Waits for `needle` on the host's stderr, which is how the binary reports
    /// its bind and its provider health check.
    async fn wait_for(&mut self, needle: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let logged = self.lines.lock().unwrap().join("\n");
            if logged.contains(needle) {
                return logged;
            }
            if let Some(status) = self.child.try_wait().expect("poll the host process") {
                panic!("host exited with {status} before logging {needle:?}:\n{logged}");
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {needle:?}:\n{logged}"
            );
            // The mock inference server lives on this test's runtime, so
            // waiting has to yield to it instead of parking the thread.
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

fn start_host(env: &[(&str, &str)]) -> Host {
    let port = free_port();
    let mut command = Command::new(env!("CARGO_BIN_EXE_chaos-web"));
    command
        .env("CHAOS_WEB_PORT", port.to_string())
        // A host inherits its operator's shell otherwise, and the suite would
        // pick up whatever provider the developer has configured locally.
        .env_remove("CHAOS_WEB_STATE")
        .env_remove("CHAOS_WORKSPACE_ROOT")
        .env_remove("CHAOS_WEB_SQLITE")
        .env_remove("CHAOS_AGENT_BINARY")
        .env_remove("CHAOS_PROVIDER_BASE_URL")
        .env_remove("CHAOS_PROVIDER_MODEL")
        .env_remove("CHAOS_PROVIDER_API_KEY")
        .env_remove("CHAOS_SAFE_WEB_MODE")
        .envs(env.iter().copied())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    // Test fixture. `Host::drop` kills and reaps the child, so it cannot
    // outlive the test. Enrollment is deliberately skipped: `ProcessScope`
    // reaps by process group, and this child inherits the test harness's own
    // group, so `kill_all` would signal the test process itself.
    #[allow(clippy::disallowed_methods)]
    let mut child = command.spawn().expect("start chaos-web");
    let stderr = child.stderr.take().expect("piped stderr");
    let lines = Arc::new(Mutex::new(Vec::new()));
    {
        let lines = Arc::clone(&lines);
        thread::spawn(move || {
            // Line by line: the assertions poll while the host is still running,
            // so buffering to EOF would never release them.
            for line in std::io::BufReader::new(stderr)
                .lines()
                .map_while(Result::ok)
            {
                lines.lock().unwrap().push(line);
            }
        });
    }
    Host { child, lines, port }
}

async fn receive(socket: &mut WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>) -> Value {
    let text = socket
        .next()
        .await
        .expect("host closed the socket")
        .expect("frame")
        .into_text()
        .expect("utf-8");
    serde_json::from_str(&text).expect("server message")
}

/// Submits one prompt over a real WebSocket and returns what the user reads.
async fn streamed_text(port: u16, prompt: &str) -> String {
    let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
        .await
        .expect("websocket connect");
    let _handshake = receive(&mut socket).await;
    socket
        .send(Message::Text(
            serde_json::json!({
                "type": "create_session",
                "client_msg_id": "host-create",
                "workspace_id": null,
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    let session_id = loop {
        let event = receive(&mut socket).await;
        if event["type"] == "session_created" {
            break event["session_id"].as_str().expect("session id").to_owned();
        }
    };
    socket
        .send(Message::Text(
            serde_json::json!({
                "type": "submit",
                "client_msg_id": "host-submit",
                "session_id": session_id,
                "prompt": prompt,
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    let mut text = String::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(60), receive(&mut socket))
            .await
            .expect("terminal event");
        match event["type"].as_str() {
            Some("text_delta") => text.push_str(event["text"].as_str().unwrap_or_default()),
            Some("completed") | Some("error") => return text,
            _ => {}
        }
    }
}

#[tokio::test]
async fn the_built_host_streams_the_configured_provider_reply() {
    let provider = MockInferenceServer::start_with_models(vec![MockModelEntry::new(MODEL)])
        .await
        .unwrap();
    provider.set_response("hello from the endpoint");
    let base_url = provider.url();
    let mut host = start_host(&[
        ("CHAOS_PROVIDER_BASE_URL", base_url.as_str()),
        ("CHAOS_PROVIDER_MODEL", MODEL),
        ("CHAOS_PROVIDER_API_KEY", "sk-host-token-0123456789"),
    ]);

    let logged = host.wait_for("listening on").await;
    assert!(
        logged.contains("/v1/chat/completions"),
        "the endpoint in use must be reported: {logged}"
    );
    assert!(
        logged.contains("ready") && logged.contains("configured model host-model is listed"),
        "the startup health check must report the endpoint: {logged}"
    );
    assert!(
        !logged.contains("sk-host-token-0123456789"),
        "the credential must never be printed: {logged}"
    );

    assert_eq!(
        streamed_text(host.port, "say hi").await,
        "hello from the endpoint"
    );

    let body = provider
        .request_bodies()
        .into_iter()
        .find(|body| body.get("messages").is_some())
        .expect("chat completion body");
    assert_eq!(body["model"], MODEL);
    assert_eq!(body["stream"], true);
}

#[tokio::test]
async fn the_built_host_reports_an_unreachable_provider_and_fails_the_turn() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let dead_port = listener.local_addr().unwrap().port();
    drop(listener);
    let base_url = format!("http://127.0.0.1:{dead_port}/v1");
    let mut host = start_host(&[
        ("CHAOS_PROVIDER_BASE_URL", base_url.as_str()),
        ("CHAOS_PROVIDER_MODEL", MODEL),
    ]);

    // The host still serves; it just says the endpoint is not reachable.
    let logged = host.wait_for("listening on").await;
    assert!(logged.contains("not reachable"), "{logged}");
    assert_eq!(
        streamed_text(host.port, "say hi").await,
        "",
        "a dead endpoint must produce an error event, not a demo reply"
    );
}

#[test]
fn an_invalid_provider_configuration_stops_the_host() {
    let port = free_port();
    let output = Command::new(env!("CARGO_BIN_EXE_chaos-web"))
        .env("CHAOS_WEB_PORT", port.to_string())
        .env_remove("CHAOS_AGENT_BINARY")
        .env_remove("CHAOS_PROVIDER_API_KEY")
        .env("CHAOS_PROVIDER_BASE_URL", "http://provider.example.com/v1")
        .env("CHAOS_PROVIDER_MODEL", MODEL)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .expect("run chaos-web");
    assert!(!output.status.success(), "a bad provider config must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Provider 配置无效"), "{stderr}");
    assert!(stderr.contains("明文"), "{stderr}");
    // Nothing is listening, because the process exited before binding.
    assert!(
        std::net::TcpStream::connect(("127.0.0.1", port)).is_err(),
        "the host must not serve with an invalid provider config"
    );
}

#[tokio::test]
async fn without_a_provider_the_host_still_answers_with_the_demo_responder() {
    let mut host = start_host(&[]);
    let logged = host.wait_for("listening on").await;
    assert!(
        !logged.contains("Provider"),
        "no provider is configured, so nothing should be claimed: {logged}"
    );
    let text = streamed_text(host.port, "ping").await;
    assert!(text.contains("演示响应"), "{text:?}");
}
