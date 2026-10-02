//! End-to-end coverage of the shipped MCP client against real peers.
//!
//! Both peers here are genuine protocol endpoints rather than in-process mocks:
//! - stdio: a second OS process (`src/bin/mock-mcp-server.rs`) speaking
//!   newline-delimited JSON-RPC over its own stdin/stdout.
//! - streamable HTTP: an axum server bound to an ephemeral loopback port.
//!
//! Everything on the client side is the production path: `start_mcp_server` →
//! `McpClient::ensure_initialized` → `McpClient::get_tool_registrations` →
//! `McpClient::call_tool`. Before these tests, neither `get_tool_registrations`
//! nor `call_tool` was exercised anywhere in the repository, so tool discovery
//! name-qualification and the tool-call round trip had zero coverage.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use agent_client_protocol as acp;
use axum::body::Body;
use axum::http::{HeaderName, StatusCode, header};
use axum::response::IntoResponse;
use serde_json::{Value, json};
use serial_test::serial;
use xai_grok_mcp::rmcp;
use xai_grok_mcp::servers::{
    MCP_TOOL_NAME_DELIMITER, McpSpawnCtx, McpState, OauthInteractivity, start_mcp_server,
};

/// Path to the fixture binary, built by Cargo as part of this package.
const FIXTURE_BIN: &str = env!("CARGO_BIN_EXE_mock-mcp-server");

/// The credential the HTTP fake accepts.
const TOKEN: &str = "at-e2e";

fn session_ctx(event_writer: &xai_grok_session_events::EventWriter) -> McpSpawnCtx<'_> {
    xai_grok_mcp::isolate_grok_home_for_tests();
    McpSpawnCtx::for_session(
        "sess-e2e",
        event_writer,
        OauthInteractivity::NonInteractive,
        None,
    )
}

/// Concatenated text of a tool result.
fn text_of(result: &rmcp::model::CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| match block {
            rmcp::model::ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect()
}

// --------------------------------------------------------------------------------------
// stdio: the client talks to a real child process over a real pipe
// --------------------------------------------------------------------------------------

fn stdio_fixture(name: &str) -> acp::McpServer {
    acp::McpServer::Stdio(acp::McpServerStdio::new(name, FIXTURE_BIN))
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn stdio_server_completes_handshake_and_exposes_qualified_tools() {
    let event_writer = xai_grok_session_events::EventWriter::noop();
    let client = start_mcp_server(
        stdio_fixture("fixture"),
        None,
        None,
        None,
        &session_ctx(&event_writer),
    )
    .await
    .expect("fixture must spawn");

    let service = client
        .ensure_initialized()
        .await
        .expect("handshake with the fixture must complete");
    let peer = service
        .peer_info()
        .expect("initialize result must be retained after the handshake");
    let server_info = peer
        .server_info
        .as_ref()
        .expect("the fixture sends a serverInfo in its initialize result");
    assert_eq!(
        server_info.name, "grok-test-fixture",
        "the handshake must have completed against the fixture process"
    );

    let mcp_state = Arc::new(tokio::sync::Mutex::new(McpState::new(vec![stdio_fixture(
        "fixture",
    )])));
    let registrations = client
        .get_tool_registrations(Arc::clone(&mcp_state))
        .await
        .expect("tools/list over the stdio pipe must succeed");

    let mut names: Vec<&str> = registrations.iter().map(|r| r.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec![
            format!("fixture{MCP_TOOL_NAME_DELIMITER}always_fails").as_str(),
            format!("fixture{MCP_TOOL_NAME_DELIMITER}echo").as_str(),
            format!("fixture{MCP_TOOL_NAME_DELIMITER}server_pid").as_str(),
        ],
        "discovered tools must be qualified with the server name, which is what \
         the session's tool dispatch and the per-server unregistration prefix rely on"
    );

    let echo = registrations
        .iter()
        .find(|r| r.name.ends_with("echo"))
        .expect("echo must be registered");
    assert_eq!(
        echo.description, "Echoes its text argument back.",
        "tools/description must survive translation into a registration"
    );
    assert_eq!(
        echo.input_schema["type"], "object",
        "a schema missing `type` is normalized to `object`; this one already has it"
    );
    assert_eq!(
        echo.input_schema["properties"]["text"]["type"], "string",
        "the fixture's schema must reach the model-facing registration intact"
    );
    assert!(
        echo.model_visible,
        "a valid tool is model-visible by default"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn stdio_tool_call_round_trips_and_reports_tool_errors() {
    let event_writer = xai_grok_session_events::EventWriter::noop();
    let client = start_mcp_server(
        stdio_fixture("caller"),
        None,
        None,
        None,
        &session_ctx(&event_writer),
    )
    .await
    .expect("fixture must spawn");

    let ok = client
        .call_tool("echo", json!({ "text": "round trip" }))
        .await
        .expect("tools/call must succeed");
    assert_eq!(text_of(&ok), "round trip");
    assert_eq!(
        ok.is_error,
        Some(false),
        "the fixture marks success explicitly, and the client must not invent one"
    );

    // A tool-level failure is an OK JSON-RPC response with isError set: it must
    // surface as a result the model can read, not as a transport error.
    let failed = client
        .call_tool("always_fails", json!({}))
        .await
        .expect("an isError result is still a successful call");
    assert_eq!(failed.is_error, Some(true));
    assert!(
        text_of(&failed).contains("tool failure"),
        "the server's own error text must be preserved: {:?}",
        text_of(&failed)
    );

    // An unknown tool is a JSON-RPC protocol error (-32602), which the client
    // must classify as an error rather than returning empty content.
    let unknown = client.call_tool("no_such_tool", json!({})).await;
    assert!(
        unknown.is_err(),
        "an unknown tool must not look like a success"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn dropping_the_client_kills_the_stdio_child() {
    let event_writer = xai_grok_session_events::EventWriter::noop();
    let client = start_mcp_server(
        stdio_fixture("leak-check"),
        None,
        None,
        None,
        &session_ctx(&event_writer),
    )
    .await
    .expect("fixture must spawn");

    // Read the child's PID back through the tool-call path, so the value under
    // test comes from the process the production spawn code actually launched.
    let pid: u32 = text_of(
        &client
            .call_tool("server_pid", json!({}))
            .await
            .expect("server_pid must answer"),
    )
    .trim()
    .parse()
    .expect("the fixture reports its own PID");
    assert!(client.is_healthy().await);

    let Some(alive) = process_alive(pid) else {
        // No portable liveness probe on this platform; the assertions above
        // still ran against a real spawned child.
        eprintln!("process liveness is not observable on this platform; skipping");
        return;
    };
    assert!(alive, "the fixture is running: pid {pid}");

    drop(client);
    // `SafeTokioChildProcess::drop` signals the process group and reaps the
    // child on the current runtime; that is asynchronous, so wait for it.
    for _ in 0..100 {
        if !process_alive(pid).unwrap_or(false) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("MCP stdio child pid {pid} survived the client being dropped");
}

// --------------------------------------------------------------------------------------
// streamable HTTP: the client talks to a credentialed endpoint we own
// --------------------------------------------------------------------------------------

/// The fake's shared state: an accept-list switch plus request counters.
#[derive(Default)]
struct FakeEndpoint {
    /// When false, every JSON-RPC POST is rejected with 401.
    accepting: AtomicBool,
    authorized: AtomicUsize,
    unauthorized: AtomicUsize,
    /// Requests that carried back the session id handed out by `initialize`.
    with_session_id: AtomicUsize,
}

/// Serves one streamable-HTTP MCP endpoint on an ephemeral loopback port.
///
/// Returns the MCP URL and the shared counters. The endpoint is started
/// accepting; tests flip [`FakeEndpoint::accepting`] to model a revoked token.
async fn spawn_http_endpoint(state: Arc<FakeEndpoint>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake endpoint");
    let addr = listener.local_addr().expect("fake endpoint addr");
    let app = axum::Router::new().fallback(move |req: axum::extract::Request| {
        let state = Arc::clone(&state);
        async move {
            let authorized = req
                .headers()
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                == Some(&format!("Bearer {TOKEN}")[..]);
            if !state.accepting.load(Ordering::SeqCst) || !authorized {
                state.unauthorized.fetch_add(1, Ordering::SeqCst);
                return StatusCode::UNAUTHORIZED.into_response();
            }
            state.authorized.fetch_add(1, Ordering::SeqCst);
            if req
                .headers()
                .get(HeaderName::from_static("mcp-session-id"))
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v == "e2e-session")
            {
                state.with_session_id.fetch_add(1, Ordering::SeqCst);
            }

            // The GET side-channel stays open forever, as a real server's does.
            if req.method() == axum::http::Method::GET {
                return (
                    [(header::CONTENT_TYPE, "text/event-stream")],
                    Body::from_stream(futures::stream::pending::<Result<String, std::io::Error>>()),
                )
                    .into_response();
            }

            let bytes = axum::body::to_bytes(req.into_body(), 1 << 20)
                .await
                .unwrap_or_default();
            let msg: Value = serde_json::from_slice(&bytes).unwrap_or_default();
            let reply = |result: Value| {
                axum::Json(json!({ "jsonrpc": "2.0", "id": msg["id"], "result": result }))
                    .into_response()
            };
            match msg["method"].as_str() {
                Some("initialize") => {
                    let response = reply(json!({
                        "protocolVersion": msg["params"]["protocolVersion"],
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": "fake-http", "version": "0.0.0" },
                    }));
                    (
                        [(HeaderName::from_static("mcp-session-id"), "e2e-session")],
                        response,
                    )
                        .into_response()
                }
                Some("notifications/initialized") => StatusCode::ACCEPTED.into_response(),
                Some("ping") => reply(json!({})),
                Some("tools/list") => reply(json!({
                    "tools": [{
                        "name": "http_echo",
                        "description": "Echoes its text argument back.",
                        "inputSchema": {
                            "type": "object",
                            "properties": { "text": { "type": "string" } },
                            "required": ["text"],
                        }
                    }]
                })),
                Some("tools/call") => {
                    let text = msg["params"]["arguments"]["text"]
                        .as_str()
                        .unwrap_or_default();
                    reply(json!({
                        "content": [{ "type": "text", "text": text }],
                        "isError": false,
                    }))
                }
                Some(other) => (
                    StatusCode::BAD_REQUEST,
                    format!("fake endpoint: unexpected method {other}"),
                )
                    .into_response(),
                None => StatusCode::ACCEPTED.into_response(),
            }
        }
    });
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}/mcp")
}

fn http_server_with_token(name: &str, url: &str) -> acp::McpServer {
    acp::McpServer::Http(
        acp::McpServerHttp::new(name, url).headers(vec![acp::HttpHeader::new(
            "Authorization",
            format!("Bearer {TOKEN}"),
        )]),
    )
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn http_server_with_config_token_lists_and_calls_tools() {
    let endpoint = Arc::new(FakeEndpoint {
        accepting: AtomicBool::new(true),
        ..Default::default()
    });
    let url = spawn_http_endpoint(Arc::clone(&endpoint)).await;
    let event_writer = xai_grok_session_events::EventWriter::noop();

    let client = start_mcp_server(
        http_server_with_token("http-fixture", &url),
        None,
        None,
        None,
        &session_ctx(&event_writer),
    )
    .await
    .expect("a config-supplied Authorization header must yield a plain HTTP client");
    assert!(
        !client.has_auth(),
        "a config header means the server is not OAuth-managed"
    );

    let mcp_state = Arc::new(tokio::sync::Mutex::new(McpState::new(vec![
        http_server_with_token("http-fixture", &url),
    ])));
    let registrations = client
        .get_tool_registrations(Arc::clone(&mcp_state))
        .await
        .expect("tools/list over streamable HTTP must succeed");
    assert_eq!(
        registrations
            .iter()
            .map(|r| r.name.as_str())
            .collect::<Vec<_>>(),
        // Literal on purpose: this is the wire-level contract the session's
        // tool dispatch and the per-server unregister path are written against.
        vec!["http-fixture__http_echo"],
    );

    let result = client
        .call_tool("http_echo", json!({ "text": "via http" }))
        .await
        .expect("tools/call over streamable HTTP must succeed");
    assert_eq!(text_of(&result), "via http");

    assert_eq!(
        endpoint.unauthorized.load(Ordering::SeqCst),
        0,
        "every request must carry the configured credential"
    );
    assert!(
        endpoint.authorized.load(Ordering::SeqCst) >= 3,
        "initialize, tools/list and tools/call all happened"
    );
    assert!(
        endpoint.with_session_id.load(Ordering::SeqCst) >= 2,
        "the session id handed out by initialize must be sent on later requests"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn revoked_credential_stops_producing_tool_output() {
    let endpoint = Arc::new(FakeEndpoint {
        accepting: AtomicBool::new(true),
        ..Default::default()
    });
    let url = spawn_http_endpoint(Arc::clone(&endpoint)).await;
    let event_writer = xai_grok_session_events::EventWriter::noop();

    let client = start_mcp_server(
        http_server_with_token("revoked", &url),
        None,
        None,
        None,
        &session_ctx(&event_writer),
    )
    .await
    .expect("credential accepted before revocation");
    let before = text_of(
        &client
            .call_tool("http_echo", json!({ "text": "before" }))
            .await
            .expect("call before revocation"),
    );
    assert_eq!(before, "before");

    endpoint.accepting.store(false, Ordering::SeqCst);

    let after = tokio::time::timeout(Duration::from_secs(60), async {
        client
            .call_tool("http_echo", json!({ "text": "after" }))
            .await
    })
    .await
    .expect("a rejected call must fail rather than hang");
    assert!(
        after.is_err(),
        "once the endpoint rejects the credential the caller must get an error, \
         never the tool's output: {after:?}"
    );
    assert!(
        endpoint.unauthorized.load(Ordering::SeqCst) > 0,
        "the revoked call must actually have reached the endpoint and been rejected"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn missing_credential_never_produces_tool_output() {
    let endpoint = Arc::new(FakeEndpoint {
        accepting: AtomicBool::new(true),
        ..Default::default()
    });
    let url = spawn_http_endpoint(Arc::clone(&endpoint)).await;
    let event_writer = xai_grok_session_events::EventWriter::noop();

    // Same endpoint, no configured credential: OAuth discovery is attempted
    // against a server that does not implement it, and the tool call must fail
    // closed either way (spawn error, handshake error, or call error).
    let outcome = tokio::time::timeout(Duration::from_secs(60), async {
        let server = acp::McpServer::Http(acp::McpServerHttp::new("anonymous", &url));
        match start_mcp_server(server, None, None, None, &session_ctx(&event_writer)).await {
            Ok(client) => {
                client
                    .call_tool("http_echo", json!({ "text": "secret" }))
                    .await
            }
            Err(err) => Err(err),
        }
    })
    .await
    .expect("an unauthenticated call must fail rather than hang");

    assert!(
        outcome.is_err(),
        "an uncredentialed client must never receive tool output: {outcome:?}"
    );
    assert_eq!(
        endpoint.authorized.load(Ordering::SeqCst),
        0,
        "no request should have been accepted without the credential"
    );
}

// --------------------------------------------------------------------------------------
// helpers
// --------------------------------------------------------------------------------------

/// Removing a server from the config must take its qualified tools with it,
/// while a server that is still configured keeps both its tools and its
/// per-server `disabled_tools` list (that list is config, not runtime state).
#[test]
fn config_diff_clears_tools_of_the_removed_server() {
    let dropped = stdio_fixture("dropped");
    let kept = stdio_fixture("kept");
    let qualified = |server: &str, tool: &str| format!("{server}{MCP_TOOL_NAME_DELIMITER}{tool}");

    let mut state = McpState::new(vec![dropped.clone(), kept.clone()]);
    let generation_before = state.generation();
    state
        .mcp_tool_meta
        .insert(qualified("dropped", "secret_tool"), json!({}));
    state
        .mcp_tool_meta
        .insert(qualified("kept", "safe_tool"), json!({}));
    state
        .disabled_tools
        .insert("dropped".to_string(), ["off_tool".to_string()].into());

    let diff = state
        .update_configs_diff(vec![kept.clone()])
        .expect("a changed config list must produce a diff");
    assert_eq!(diff.removed, vec!["dropped".to_string()]);
    assert_eq!(diff.retained, vec!["kept".to_string()]);
    assert!(diff.added.is_empty());
    assert!(
        !state
            .mcp_tool_meta
            .contains_key(&qualified("dropped", "secret_tool")),
        "a removed server must not leave tools behind: {:?}",
        state.mcp_tool_meta.keys().collect::<Vec<_>>()
    );
    assert!(
        state
            .mcp_tool_meta
            .contains_key(&qualified("kept", "safe_tool")),
        "an unchanged server keeps its tools"
    );
    assert!(
        state.is_tool_disabled("dropped", "off_tool"),
        "disabled_tools is persisted config and survives the server being removed"
    );
    assert_eq!(state.generation(), generation_before + 1);

    assert!(
        state.update_configs_diff(vec![kept]).is_none(),
        "an identical config list must be a no-op, so healthy servers are not torn down"
    );
    assert_eq!(state.generation(), generation_before + 1);
}

/// Whether `pid` exists as a running (or sleeping, or stopped) process.
/// `None` means this platform offers no cheap probe for this test.
#[cfg(all(unix, not(target_os = "linux")))]
fn process_alive(pid: u32) -> Option<bool> {
    // SAFETY: kill(pid, 0) performs only permission/existence checking.
    Some(unsafe { libc::kill(pid as libc::pid_t, 0) == 0 })
}

/// Linux variant: `kill(pid, 0)` also succeeds for a zombie, and an MCP child
/// is reaped asynchronously by the drop path, so the process state is what
/// actually answers "is it still running".
#[cfg(target_os = "linux")]
fn process_alive(pid: u32) -> Option<bool> {
    let Ok(stat) = std::fs::read(format!("/proc/{pid}/stat")) else {
        return Some(false);
    };
    let stat = String::from_utf8_lossy(&stat);
    // `pid (comm) state ...`; `comm` may itself contain spaces and parentheses.
    Some(match stat.rfind(')') {
        Some(end) => !stat[end + 1..].trim_start().starts_with('Z'),
        None => true,
    })
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> Option<bool> {
    // Probing this would need OpenProcess from windows-sys, which this crate
    // does not depend on.
    None
}
