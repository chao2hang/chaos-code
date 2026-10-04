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
use tempfile::tempdir;
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
        .env_remove("CHAOS_WEB_TOKEN")
        .env_remove("CHAOS_WEB_PUBLIC_ORIGIN")
        .env_remove("CHAOS_WEB_ASSETS_DIR")
        .env_remove("CHAOS_WEB_DEV_ORIGIN")
        .env_remove("CHAOS_WEB_ALLOW_DYNAMIC_DEV_ORIGIN")
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

/// One plain HTTP/1.1 request, with the headers a TLS terminator in front of
/// the loopback host would put on it. Returns the status and the response body.
fn http_request(port: u16, path: &str, headers: &[(&str, &str)]) -> (u16, String) {
    use std::io::{Read, Write};
    let mut stream =
        std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect to the built host");
    let mut raw = format!("GET {path} HTTP/1.1\r\n");
    for (name, value) in headers {
        raw.push_str(&format!("{name}: {value}\r\n"));
    }
    raw.push_str("Connection: close\r\n\r\n");
    stream.write_all(raw.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (head, body) = response
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("no header terminator in {response:?}"));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("no status line in {response:?}"));
    (status, body.to_owned())
}

fn http_get(port: u16, path: &str, headers: &[(&str, &str)]) -> u16 {
    http_request(port, path, headers).0
}

/// The `error` field of a refusal body. The reason code is operator-facing: an
/// operator reading a log line has to be able to tell a misconfigured proxy from
/// a missing credential without attaching a debugger to either.
fn refusal_code(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .unwrap_or_else(|e| panic!("refusal body is not JSON ({e}): {body:?}"))
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("refusal body has no error field: {body:?}"))
        .to_owned()
}

/// What a browser behind a TLS terminator actually sends: the public name as
/// `Host`, and an `https` `Origin` that can never match the loopback address
/// this server is dialed at.
const PUBLIC: [(&str, &str); 4] = [
    ("Host", "chaos.example.test:8443"),
    ("Origin", "https://chaos.example.test:8443"),
    ("X-Forwarded-Proto", "https"),
    ("Authorization", "Bearer rotatable-token"),
];

#[tokio::test]
async fn the_built_host_serves_a_declared_public_origin_behind_tls() {
    let mut host = start_host(&[
        ("CHAOS_WEB_TOKEN", "rotatable-token"),
        ("CHAOS_WEB_PUBLIC_ORIGIN", "https://chaos.example.test:8443"),
    ]);
    let logged = host
        .wait_for("accepting Host and Origin for chaos.example.test:8443")
        .await;
    assert!(
        logged.contains("X-Forwarded-Proto"),
        "the proxy's obligation must be stated at startup: {logged}"
    );

    assert_eq!(
        http_get(host.port, "/api/handshake", &PUBLIC),
        200,
        "the declared origin, presented over TLS by the proxy, is this deployment"
    );

    let without_tls_claim: Vec<(&str, &str)> = PUBLIC
        .iter()
        .filter(|(name, _)| *name != "X-Forwarded-Proto")
        .copied()
        .collect();
    let (status, body) = http_request(host.port, "/api/handshake", &without_tls_claim);
    assert_eq!(
        status, 401,
        "an https origin is not same-origin with anything unless the proxy says the client side was TLS"
    );
    assert_eq!(
        refusal_code(&body),
        "origin_requires_forwarded_proto",
        "the one fixable-by-the-proxy refusal has to name itself: {body:?}"
    );

    let other_page: Vec<(&str, &str)> = PUBLIC
        .iter()
        .map(|(name, value)| {
            if *name == "Origin" {
                (*name, "https://evil.example")
            } else {
                (*name, *value)
            }
        })
        .collect();
    let (status, body) = http_request(host.port, "/api/handshake", &other_page);
    assert_eq!(
        status, 401,
        "declaring one origin must not admit another page"
    );
    assert_eq!(refusal_code(&body), "origin_not_allowed");

    let no_credential: Vec<(&str, &str)> = PUBLIC
        .iter()
        .filter(|(name, _)| *name != "Authorization")
        .copied()
        .collect();
    let (status, body) = http_request(host.port, "/api/handshake", &no_credential);
    assert_eq!(
        status, 401,
        "a host reachable through a proxy is never anonymous"
    );
    assert_eq!(refusal_code(&body), "credential_required");
}

/// Loopback-only is still the default: nothing about the public-name handling
/// changes what the host answers when the operator did not ask for it.
#[tokio::test]
async fn an_undeclared_public_host_stays_refused() {
    let mut host = start_host(&[("CHAOS_WEB_TOKEN", "rotatable-token")]);
    host.wait_for("listening on").await;
    let (status, body) = http_request(host.port, "/api/handshake", &PUBLIC);
    assert_eq!(
        status, 401,
        "a public Host must be refused until the operator declares it"
    );
    assert_eq!(
        refusal_code(&body),
        "host_not_allowed",
        "the fix here is to set CHAOS_WEB_PUBLIC_ORIGIN, not to rotate a token: {body:?}"
    );
}

/// Runs the host with a public origin and requires it to refuse to start.
///
/// The wait is bounded: `output()` would block forever on a regression where the
/// guard is gone and the host simply starts serving, turning a failing test into
/// a hung CI job with no report.
fn expect_startup_refusal(public_origin: &str, token: Option<&str>, needle: &str) {
    use wait_timeout::ChildExt;
    let port = free_port();
    let mut command = Command::new(env!("CARGO_BIN_EXE_chaos-web"));
    command
        .env("CHAOS_WEB_PORT", port.to_string())
        .env("CHAOS_WEB_PUBLIC_ORIGIN", public_origin)
        .env_remove("CHAOS_WEB_TOKEN")
        .env_remove("CHAOS_AGENT_BINARY")
        .env_remove("CHAOS_PROVIDER_BASE_URL")
        .env_remove("CHAOS_WEB_STATE")
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(token) = token {
        command.env("CHAOS_WEB_TOKEN", token);
    }
    // Test fixture, and the same reasoning as `start_host`: the child is reaped
    // here (or killed by the bounded wait below), and enrolling it would put the
    // test harness's own process group in the kill set.
    #[allow(clippy::disallowed_methods)]
    let mut child = command.spawn().expect("run chaos-web");
    let stderr = child.stderr.take().expect("piped stderr");
    let reader = thread::spawn(move || {
        let mut captured = Vec::new();
        let _ = std::io::Read::read_to_end(&mut std::io::BufReader::new(stderr), &mut captured);
        captured
    });
    let status = child.wait_timeout(Duration::from_secs(30)).expect("wait");
    let status = match status {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            panic!(
                "chaos-web was still running after 30s with CHAOS_WEB_PUBLIC_ORIGIN={public_origin:?}; \
                 it must refuse to start"
            );
        }
    };
    let captured = reader.join().expect("read stderr");
    assert!(!status.success(), "startup must fail for {public_origin:?}");
    let stderr = String::from_utf8_lossy(&captured).into_owned();
    assert!(stderr.contains(needle), "{stderr}");
    assert!(
        std::net::TcpStream::connect(("127.0.0.1", port)).is_err(),
        "the host must not serve: {stderr}"
    );
}

#[test]
fn a_public_origin_without_a_credential_stops_the_host() {
    expect_startup_refusal(
        "https://chaos.example.test:8443",
        None,
        "必须同时设置 CHAOS_WEB_TOKEN",
    );
}

#[test]
fn a_malformed_public_origin_stops_the_host() {
    // A path, not an https origin, and a credential in the authority: each is a
    // typo that would silently widen or narrow the accept list instead.
    expect_startup_refusal(
        "https://chaos.example.test/app",
        Some("rotatable-token"),
        "CHAOS_WEB_PUBLIC_ORIGIN 无效",
    );
    expect_startup_refusal(
        "http://chaos.example.test",
        Some("rotatable-token"),
        "https",
    );
}

/// A browser's worth of session against the built host: one WebSocket, one
/// session, and the calls the 差异 tab actually makes.
struct Session {
    socket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    session_id: String,
}

impl Session {
    async fn open(port: u16) -> Self {
        let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{port}/ws"))
            .await
            .expect("websocket connect");
        let _handshake = receive(&mut socket).await;
        socket
            .send(Message::Text(
                serde_json::json!({
                    "type": "create_session",
                    "client_msg_id": format!("create-{}", uuid::Uuid::new_v4()),
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
        Self { socket, session_id }
    }

    async fn send(&mut self, body: Value) {
        self.socket
            .send(Message::Text(body.to_string().into()))
            .await
            .unwrap();
    }

    /// Asks the host to write a file and waits for the approval prompt, which is
    /// the only way a browser gets bytes into a workspace.
    async fn propose_and_approve(&mut self, relative_path: &str, contents: &str) {
        self.send(serde_json::json!({
            "type": "propose_file_write",
            "client_msg_id": format!("propose-{}", uuid::Uuid::new_v4()),
            "session_id": self.session_id,
            "relative_path": relative_path,
            "contents": contents,
        }))
        .await;
        let request_id = loop {
            let event = receive(&mut self.socket).await;
            match event["type"].as_str() {
                Some("tool_approval_requested") => {
                    break event["request_id"].as_str().expect("request id").to_owned();
                }
                Some("error") => panic!("the proposal was refused: {event}"),
                _ => {}
            }
        };
        self.send(serde_json::json!({
            "type": "approve",
            "client_msg_id": format!("approve-{}", uuid::Uuid::new_v4()),
            "request_id": request_id,
        }))
        .await;
    }

    /// Waits for the write to be answered, returning the undo point it offered.
    /// A `file_written` with no `diff_preview` behind it is the regression this
    /// test file exists for: the tab would have nothing to show.
    async fn landed_write(&mut self, relative_path: &str) -> Value {
        loop {
            let event = tokio::time::timeout(Duration::from_secs(30), receive(&mut self.socket))
                .await
                .expect("the host answers the approved write");
            match event["type"].as_str() {
                Some("diff_preview") if event["preview"]["path"] == relative_path => {
                    return event["preview"].clone();
                }
                Some("error") => panic!("the approved write failed: {event}"),
                _ => {}
            }
        }
    }

    /// Resolves a proposal and reports what came back, failing on a refusal.
    async fn resolve(&mut self, kind: &str, proposal_id: &str) {
        self.send(serde_json::json!({
            "type": kind,
            "client_msg_id": format!("{kind}-{}", uuid::Uuid::new_v4()),
            "session_id": self.session_id,
            "proposal_id": proposal_id,
            "summary": "确认保留",
        }))
        .await;
        loop {
            let event = receive(&mut self.socket).await;
            match event["type"].as_str() {
                Some("diff_resolved") => return,
                Some("error") => panic!("{kind} was refused: {event}"),
                _ => {}
            }
        }
    }

    /// Sends a resolve call and returns the error the host reported, failing if
    /// it reported a resolution instead.
    async fn refused_resolve(&mut self, kind: &str, proposal_id: &str) -> Value {
        self.send(serde_json::json!({
            "type": kind,
            "client_msg_id": format!("{kind}-{}", uuid::Uuid::new_v4()),
            "session_id": self.session_id,
            "proposal_id": proposal_id,
            "summary": "确认保留",
        }))
        .await;
        loop {
            let event = receive(&mut self.socket).await;
            match event["type"].as_str() {
                Some("error") => return event,
                Some("diff_resolved") => {
                    panic!("{kind} should have been refused, got {event}");
                }
                _ => {}
            }
        }
    }
}

/// The whole point of the 差异 tab, run against the binary an operator starts:
/// a write lands, the browser is told what it replaced, and 回滚变更 puts those
/// bytes back.
#[tokio::test]
async fn the_built_host_offers_an_undo_point_for_a_write_that_landed() {
    let workspace = tempdir().unwrap();
    std::fs::write(
        workspace.path().join("note.txt"),
        "before the model typed\n",
    )
    .unwrap();
    let root = workspace.path().to_str().unwrap().to_owned();
    let mut host = start_host(&[("CHAOS_WORKSPACE_ROOT", root.as_str())]);
    host.wait_for("listening on").await;

    let mut session = Session::open(host.port).await;
    session
        .propose_and_approve("note.txt", "after the model typed\n")
        .await;
    let preview = session.landed_write("note.txt").await;
    assert_eq!(
        preview["before"], "before the model typed\n",
        "the old side must be what was really on disk: {preview}"
    );
    assert_eq!(preview["after"], "after the model typed\n");
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("note.txt")).unwrap(),
        "after the model typed\n"
    );

    let proposal_id = preview["proposal_id"].as_str().unwrap();
    session.resolve("rollback_diff", proposal_id).await;
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("note.txt")).unwrap(),
        "before the model typed\n"
    );
}

/// The other button: 接受变更 says the change stays, spends the undo point, and
/// must not rewrite the file it is confirming.
#[tokio::test]
async fn the_built_host_keeps_an_accepted_write_and_spends_the_undo_point() {
    let workspace = tempdir().unwrap();
    std::fs::write(
        workspace.path().join("note.txt"),
        "before the model typed\n",
    )
    .unwrap();
    let root = workspace.path().to_str().unwrap().to_owned();
    let mut host = start_host(&[("CHAOS_WORKSPACE_ROOT", root.as_str())]);
    host.wait_for("listening on").await;

    let mut session = Session::open(host.port).await;
    session
        .propose_and_approve("note.txt", "after the model typed\n")
        .await;
    let preview = session.landed_write("note.txt").await;
    let proposal_id = preview["proposal_id"].as_str().unwrap().to_owned();
    session.resolve("accept_diff", &proposal_id).await;
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("note.txt")).unwrap(),
        "after the model typed\n"
    );

    let refusal = session.refused_resolve("rollback_diff", &proposal_id).await;
    assert_eq!(refusal["code"], "diff_failed", "{refusal}");
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("note.txt")).unwrap(),
        "after the model typed\n",
        "a refused rollback must not touch the file"
    );
}

/// What the guard is for: the user (or git) edited the file after the model's
/// write, so 回滚变更 refuses, leaves those edits in place, and does not clear
/// the preview the browser is showing.
#[tokio::test]
async fn the_built_host_refuses_to_roll_back_over_an_edit_it_did_not_make() {
    let workspace = tempdir().unwrap();
    std::fs::write(workspace.path().join("note.txt"), "committed\n").unwrap();
    let root = workspace.path().to_str().unwrap().to_owned();
    let mut host = start_host(&[("CHAOS_WORKSPACE_ROOT", root.as_str())]);
    host.wait_for("listening on").await;

    let mut session = Session::open(host.port).await;
    session
        .propose_and_approve("note.txt", "written by the model\n")
        .await;
    let preview = session.landed_write("note.txt").await;
    std::fs::write(workspace.path().join("note.txt"), "edited by hand\n").unwrap();

    let refusal = session
        .refused_resolve("rollback_diff", preview["proposal_id"].as_str().unwrap())
        .await;
    assert_eq!(refusal["code"], "diff_failed", "{refusal}");
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("note.txt")).unwrap(),
        "edited by hand\n"
    );
}
