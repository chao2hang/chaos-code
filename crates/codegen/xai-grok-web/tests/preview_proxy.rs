//! The preview proxy driven through the shipped `chaos-web` binary.
//!
//! Every assertion here is made against a real second web server acting as the
//! previewed app — real HTTP/1.1, a real WebSocket handshake — because the four
//! things this feature exists for (`Host`, `Origin`, `Cookie`, `Location`) are
//! things a stub would agree to whatever the proxy sent. The stand-in is also
//! deliberately hostile: it refuses a `Host` it does not recognise and an `Origin`
//! that is not its own, the way Vite and webpack-dev-server do. If a rewrite
//! regressed, the browser would read that refusal instead of its app, and these
//! tests say so.
//!
//! The rest of the suite is what stands in front of the preview: a port nobody
//! named, a dev server that is not running, an upgrade that is not WebSocket, a
//! page from another site, and — the default that matters most — a request that
//! arrived through the host's public name.

use axum::{
    Router,
    extract::{Request, State, ws::WebSocketUpgrade},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::any,
};
use futures_util::{SinkExt, StreamExt};
use std::{
    io::{BufRead, Read, Write},
    net::TcpListener,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message as WireMessage,
    tungstenite::client::IntoClientRequest, tungstenite::handshake::derive_accept_key,
};

/// A subprotocol the stand-in dev server offers, so the negotiation can be
/// watched from the browser's side.
const HMR_PROTOCOL: &str = "chaos-hmr";

const X_FORWARDED_PREFIX: HeaderName = HeaderName::from_static("x-forwarded-prefix");
const X_FORWARDED_HOST: HeaderName = HeaderName::from_static("x-forwarded-host");
const X_FORWARDED_PROTO: HeaderName = HeaderName::from_static("x-forwarded-proto");

/// Ports this test binary has already decided to use.
///
/// The kernel hands an ephemeral port to whoever asks for one, so a port a test
/// probed for and released can be handed to the next test as well. That is
/// harmless for a port a test is about to bind, but not for a port a test is
/// treating as dead or as "not in the allowlist": the second test would find the
/// first test's dev server behind it, and read a live preview where the point of
/// the assertion was that there is none.
static CLAIMED_PORTS: Mutex<Vec<u16>> = Mutex::new(Vec::new());

/// A port no other test in this binary will use.
///
/// A server the test itself owns should not call this — bind on port 0 and read
/// the port back, so the port being advertised is one already held.
fn free_port() -> u16 {
    loop {
        let probe = TcpListener::bind("127.0.0.1:0")
            .expect("free port")
            .local_addr()
            .expect("probe address")
            .port();
        let mut claimed = CLAIMED_PORTS.lock().unwrap();
        if !claimed.contains(&probe) {
            claimed.push(probe);
            return probe;
        }
    }
}

// ---------------------------------------------------------------------------
// The previewed app
// ---------------------------------------------------------------------------

/// What the dev server was actually asked. Assertions are made against this as
/// well as against what came back, because a proxy can echo a body it never
/// forwarded.
#[derive(Clone, Debug)]
struct Seen {
    method: String,
    path: String,
    host: String,
    origin: Option<String>,
    referer: Option<String>,
    authorization: Option<String>,
    cookie: Option<String>,
    forwarded_prefix: Option<String>,
    forwarded_host: Option<String>,
    forwarded_proto: Option<String>,
    websocket_keys: usize,
    body: Vec<u8>,
}

fn header_text(headers: &HeaderMap, name: HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

impl Seen {
    fn of(headers: &HeaderMap, method: &str, path: &str, body: Vec<u8>) -> Self {
        Self {
            method: method.to_string(),
            path: path.to_string(),
            host: header_text(headers, header::HOST).unwrap_or_default(),
            origin: header_text(headers, header::ORIGIN),
            referer: header_text(headers, header::REFERER),
            authorization: header_text(headers, header::AUTHORIZATION),
            cookie: header_text(headers, header::COOKIE),
            forwarded_prefix: header_text(headers, X_FORWARDED_PREFIX),
            forwarded_host: header_text(headers, X_FORWARDED_HOST),
            forwarded_proto: header_text(headers, X_FORWARDED_PROTO),
            websocket_keys: headers.get_all(header::SEC_WEBSOCKET_KEY).iter().count(),
            body,
        }
    }
}

#[derive(Clone)]
struct DevState {
    port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
}

/// A stand-in for `npm run dev`: it serves, sets cookies, redirects, echoes a
/// body, speaks WebSocket, and — like the real thing — rejects a request that
/// claims to be addressed to somebody else.
fn dev_server(port: u16) -> (Router, Arc<Mutex<Vec<Seen>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let router = Router::new()
        .route("/ws", any(ws_route))
        .fallback(any(http_route))
        .with_state(DevState {
            port,
            seen: Arc::clone(&seen),
        });
    (router, seen)
}

/// The dev server's own `allowedHosts` and origin checks. This is what makes the
/// `Host` and `Origin` rewrites observable from outside: without them the browser
/// would read this refusal instead of its application.
fn refuses_foreign_claim(state: &DevState, seen: &Seen) -> Option<Response> {
    let host_is_own = seen
        .host
        .split(':')
        .next()
        .is_some_and(|host| matches!(host, "127.0.0.1" | "localhost" | "[::1]"));
    if !host_is_own {
        return Some(
            (
                StatusCode::FORBIDDEN,
                format!("blocked host: {} is not allowed here", seen.host),
            )
                .into_response(),
        );
    }
    if let Some(origin) = &seen.origin
        && *origin != format!("http://127.0.0.1:{}", state.port)
    {
        return Some(
            (
                StatusCode::FORBIDDEN,
                format!("blocked origin: {origin} is not this dev server"),
            )
                .into_response(),
        );
    }
    None
}

async fn http_route(State(state): State<DevState>, request: Request) -> Response {
    let headers = request.headers().clone();
    let method = request.method().as_str().to_string();
    let path = request.uri().path().to_string();
    let body = axum::body::to_bytes(request.into_body(), 8 * 1024 * 1024)
        .await
        .unwrap_or_default()
        .to_vec();
    let seen = Seen::of(&headers, &method, &path, body);
    state.seen.lock().unwrap().push(seen.clone());
    if let Some(refusal) = refuses_foreign_claim(&state, &seen) {
        return refusal;
    }

    match path.as_str() {
        "/set-cookie" => {
            // Appended into a HeaderMap on purpose: an array of header pairs goes
            // through `insert`, which would collapse the two cookies into one and
            // quietly remove the thing this test is about.
            let mut headers = HeaderMap::new();
            headers.append(
                header::SET_COOKIE,
                HeaderValue::from_static(
                    "sid=abc; Path=/; Domain=chaos.example.test; HttpOnly; SameSite=Lax",
                ),
            );
            headers.append(header::SET_COOKIE, HeaderValue::from_static("theme=dark"));
            headers.append(header::LOCATION, HeaderValue::from_static("/"));
            (StatusCode::SEE_OTHER, headers).into_response()
        }
        "/redirect-root" => (
            StatusCode::FOUND,
            [(header::LOCATION, HeaderValue::from_static("/login"))],
        )
            .into_response(),
        "/redirect-abs" => (
            StatusCode::FOUND,
            [(
                header::LOCATION,
                HeaderValue::from_str(&format!(
                    "http://127.0.0.1:{}/login?next=%2Fa",
                    state.port
                ))
                .expect("location"),
            )],
        )
            .into_response(),
        "/redirect-away" => (
            StatusCode::FOUND,
            [(
                header::LOCATION,
                HeaderValue::from_static("https://example.com/docs"),
            )],
        )
            .into_response(),
        "/slow" => {
            // Long enough that the assertion on the far side cannot win by luck.
            tokio::time::sleep(Duration::from_millis(300)).await;
            "eventually".into_response()
        }
        _ => (
            StatusCode::OK,
            format!(
                "host={}|origin={}|referer={}|auth={}|cookie={}|prefix={}|fwd_host={}|fwd_proto={}|body={}",
                seen.host,
                seen.origin.unwrap_or_default(),
                seen.referer.unwrap_or_default(),
                seen.authorization.unwrap_or_default(),
                seen.cookie.unwrap_or_default(),
                seen.forwarded_prefix.unwrap_or_default(),
                seen.forwarded_host.unwrap_or_default(),
                seen.forwarded_proto.unwrap_or_default(),
                String::from_utf8_lossy(&seen.body),
            ),
        )
            .into_response(),
    }
}

async fn ws_route(
    State(state): State<DevState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let seen = Seen::of(&headers, "GET", "/ws", Vec::new());
    state.seen.lock().unwrap().push(seen.clone());
    if let Some(refusal) = refuses_foreign_claim(&state, &seen) {
        return refusal;
    }
    let (saw_host, saw_origin, saw_referer, saw_keys) = (
        seen.host.clone(),
        seen.origin.clone().unwrap_or_default(),
        seen.referer.clone().unwrap_or_default(),
        seen.websocket_keys,
    );
    upgrade
        .protocols([HMR_PROTOCOL.to_string()])
        .on_upgrade(move |socket| async move {
            let mut socket = socket;
            // Only after the upgrade is the negotiated subprotocol known.
            let greeting = format!(
                "hello host={saw_host} origin={saw_origin} referer={saw_referer} keys={saw_keys} proto={}",
                socket
                    .protocol()
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("(none)"),
            );
            if socket
                .send(axum::extract::ws::Message::Text(greeting.into()))
                .await
                .is_err()
            {
                return;
            }
            while let Some(message) = socket.recv().await {
                match message {
                    Ok(axum::extract::ws::Message::Text(text)) => {
                        let reply = format!("echo {text}");
                        if socket
                            .send(axum::extract::ws::Message::Text(reply.into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Ok(axum::extract::ws::Message::Close(_)) | Err(_) => break,
                    Ok(_) => {}
                }
            }
        })
}

// ---------------------------------------------------------------------------
// The host under test
// ---------------------------------------------------------------------------

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
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

fn start_host(env: &[(&str, &str)]) -> Host {
    let port = free_port();
    let mut command = Command::new(env!("CARGO_BIN_EXE_chaos-web"));
    command
        .env("CHAOS_WEB_PORT", port.to_string())
        // A host inherits its operator's shell otherwise, and these tests would
        // preview whatever a developer has enabled locally.
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
        .env_remove("CHAOS_WEB_PREVIEW_PORTS")
        .env_remove("CHAOS_WEB_PREVIEW_ALLOW_PUBLIC")
        .envs(env.iter().copied())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    // Test fixture. `Host::drop` kills and reaps the child, so it cannot outlive
    // the test; enrolling it in the process scope would put the test harness's own
    // process group in the kill set.
    #[allow(clippy::disallowed_methods)]
    let mut child = command.spawn().expect("start chaos-web");
    let stderr = child.stderr.take().expect("piped stderr");
    let lines = Arc::new(Mutex::new(Vec::new()));
    {
        let lines = Arc::clone(&lines);
        thread::spawn(move || {
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

// ---------------------------------------------------------------------------
// A raw HTTP/1.1 client
//
// The headers under test are the ones a browser chooses, not the ones a client
// library derives from the URL it was handed, so the request is written out.
// ---------------------------------------------------------------------------

/// status, response headers (names lower-cased, duplicates kept), body
type Raw = (u16, Vec<(String, String)>, String);

fn raw_request(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Raw {
    let mut stream =
        std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect to the built host");
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .expect("read timeout");
    let mut raw = format!("{method} {path} HTTP/1.1\r\n");
    for (name, value) in headers {
        raw.push_str(&format!("{name}: {value}\r\n"));
    }
    if !body.is_empty() {
        raw.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    raw.push_str("Connection: close\r\n\r\n");
    stream.write_all(raw.as_bytes()).expect("send request");
    stream.write_all(body).expect("send body");
    stream.flush().expect("flush");
    let mut response = Vec::new();
    stream.read_to_end(&mut response).expect("read response");
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap_or_else(|| {
            panic!(
                "no header terminator in {:?}",
                String::from_utf8_lossy(&response)
            )
        });
    let head = String::from_utf8_lossy(&response[..split]).into_owned();
    let mut body_bytes = response[split + 4..].to_vec();

    let mut lines = head.split("\r\n");
    let status_line = lines.next().expect("status line");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .unwrap_or_else(|| panic!("no status line in {status_line:?}"))
        .parse()
        .expect("numeric status");
    let mut response_headers = Vec::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            response_headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    if response_headers
        .iter()
        .any(|(name, value)| name == "transfer-encoding" && value.contains("chunked"))
    {
        body_bytes = decode_chunks(&body_bytes);
    }
    (
        status,
        response_headers,
        String::from_utf8_lossy(&body_bytes).into_owned(),
    )
}

fn decode_chunks(mut wire: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let end = wire
            .windows(2)
            .position(|pair| pair == b"\r\n")
            .expect("chunk length line");
        let size = usize::from_str_radix(String::from_utf8_lossy(&wire[..end]).trim(), 16)
            .expect("chunk length");
        wire = &wire[end + 2..];
        if size == 0 {
            return out;
        }
        out.extend_from_slice(&wire[..size]);
        wire = &wire[size + 2..];
    }
}

fn header_of(response: &Raw, name: &str) -> Option<String> {
    response
        .1
        .iter()
        .find(|(header_name, _)| header_name == name)
        .map(|(_, value)| value.clone())
}

fn all_headers<'a>(response: &'a Raw, name: &str) -> Vec<&'a str> {
    response
        .1
        .iter()
        .filter(|(header_name, _)| header_name == name)
        .map(|(_, value)| value.as_str())
        .collect()
}

fn refusal_code(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .unwrap_or_else(|error| panic!("refusal body is not JSON ({error}): {body:?}"))
        .get("error")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("refusal body has no error field: {body:?}"))
        .to_owned()
}

fn borrowed(pairs: &[(String, String)]) -> Vec<(&str, &str)> {
    pairs
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect()
}

/// The headers a browser sends when the page it is on was served by this host.
fn browser_headers(port: u16) -> Vec<(String, String)> {
    vec![
        ("Host".to_string(), format!("127.0.0.1:{port}")),
        ("Origin".to_string(), format!("http://127.0.0.1:{port}")),
    ]
}

/// One named request, so the closures below stay readable.
struct Call {
    port: u16,
    method: &'static str,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Call {
    fn get(port: u16, path: String, headers: Vec<(String, String)>) -> Self {
        Self {
            port,
            method: "GET",
            path,
            headers,
            body: Vec::new(),
        }
    }

    fn run(&self) -> Raw {
        raw_request(
            self.port,
            self.method,
            &self.path,
            &borrowed(&self.headers),
            &self.body,
        )
    }
}

fn preview_path(dev_port: u16, tail: &str) -> String {
    format!("/preview/{dev_port}/{tail}")
}

fn host_header(port: u16) -> (String, String) {
    ("Host".to_string(), format!("127.0.0.1:{port}"))
}

struct Fixture {
    host: Host,
    /// The port the stand-in dev server listens on, and the preview named for it.
    dev_port: u16,
    /// A second named port with nothing behind it, for the dead-upstream case.
    dead_port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
}

/// Starts the stand-in dev server and the host that previews it.
async fn fixture(env: &[(&str, &str)]) -> Fixture {
    // Bound before anything is told the port. A probed-and-released port can be
    // taken in the window before this server binds it, which shows up as a bind
    // failure in whichever test lost the race.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("the dev server binds");
    let dev_port = listener
        .local_addr()
        .expect("the dev server's own address")
        .port();
    let dead_port = free_port();
    let (router, seen) = dev_server(dev_port);
    // The dev server has to be served from this runtime: the raw client below
    // blocks a thread of its own, so a current-thread runtime would never get to
    // answer it. Hence the multi-thread flavors on the tests.
    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("the dev server serves");
    });
    let ports = format!("{dev_port},{dead_port}");
    let mut host_env = vec![("CHAOS_WEB_PREVIEW_PORTS", ports.as_str())];
    host_env.extend_from_slice(env);
    Fixture {
        host: start_host(&host_env),
        dev_port,
        dead_port,
        seen,
    }
}

fn seen_count(seen: &Arc<Mutex<Vec<Seen>>>) -> usize {
    seen.lock().unwrap().len()
}

fn last_seen(seen: &Arc<Mutex<Vec<Seen>>>) -> Seen {
    seen.lock()
        .unwrap()
        .last()
        .cloned()
        .expect("the dev server saw a request")
}

/// Opens the previewed app's HMR socket through the host, as a browser would: the
/// `Origin` of the page it was reading, and the subprotocols it can speak.
async fn connect_preview(
    web_port: u16,
    dev_port: u16,
    origin: &str,
    protocols: &[&str],
) -> (
    WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    u16,
    Vec<(String, String)>,
) {
    connect_preview_with(web_port, dev_port, origin, protocols, &[]).await
}

/// `connect_preview`, plus headers a particular test wants on the wire — a
/// credential, say, to watch whether it survives the proxy.
async fn connect_preview_with(
    web_port: u16,
    dev_port: u16,
    origin: &str,
    protocols: &[&str],
    extra: &[(&str, &str)],
) -> (
    WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    u16,
    Vec<(String, String)>,
) {
    let url = format!("ws://127.0.0.1:{web_port}/preview/{dev_port}/ws");
    let mut request = url.as_str().into_client_request().expect("ws request");
    request.headers_mut().insert(
        header::ORIGIN,
        HeaderValue::from_str(origin).expect("origin"),
    );
    if !protocols.is_empty() {
        request.headers_mut().insert(
            header::SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_str(&protocols.join(", ")).expect("protocols"),
        );
    }
    for (name, value) in extra {
        request.headers_mut().insert(
            HeaderName::from_bytes(name.as_bytes()).expect("header name"),
            HeaderValue::from_str(value).expect("header value"),
        );
    }
    let (socket, handshake) = connect_async(request).await.expect("ws handshake");
    let headers = handshake
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_ascii_lowercase(),
                value.to_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    (socket, handshake.status().as_u16(), headers)
}

/// The next text frame from a bridged socket.
async fn next_text(socket: &mut WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>) -> String {
    socket
        .next()
        .await
        .expect("stream")
        .expect("frame")
        .to_text()
        .expect("utf-8")
        .to_string()
}

// ---------------------------------------------------------------------------
// The rewrites
// ---------------------------------------------------------------------------

/// The reason this module exists: the app's own `Host` and `Origin` checks have to
/// pass, and this host's credential must not travel with the request.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_previewed_app_sees_its_own_authority_and_not_the_hosts_credential() {
    let Fixture {
        mut host,
        dev_port,
        seen,
        ..
    } = fixture(&[("CHAOS_WEB_TOKEN", "rotatable-token")]).await;
    let logged = host
        .wait_for("previewing dev servers on loopback only")
        .await;
    assert!(
        logged.contains(&format!(
            "http://127.0.0.1:{dev_port} -> /preview/{dev_port}/"
        )),
        "startup has to say which dev server sits behind which prefix: {logged}"
    );
    let web_port = host.port;

    let mut headers = browser_headers(web_port);
    headers.push((
        "Authorization".to_string(),
        "Bearer rotatable-token".to_string(),
    ));
    headers.push((
        "Referer".to_string(),
        format!("http://127.0.0.1:{web_port}/preview/{dev_port}/app"),
    ));
    let call = Call {
        port: web_port,
        method: "POST",
        path: preview_path(dev_port, "submit"),
        headers,
        body: b"draft=true".to_vec(),
    };
    let (status, _, body) = tokio::task::spawn_blocking(move || call.run())
        .await
        .expect("join");

    assert_eq!(status, 200, "the dev server refused: {body}");
    assert!(
        body.contains(&format!("host=127.0.0.1:{dev_port}")),
        "the dev server must see its own Host: {body}"
    );
    assert!(
        body.contains(&format!("origin=http://127.0.0.1:{dev_port}")),
        "and its own Origin: {body}"
    );
    assert!(
        body.contains(&format!("referer=http://127.0.0.1:{dev_port}/app")),
        "the prefix is this host's business, not the app's: {body}"
    );
    assert!(
        body.contains("auth=|"),
        "the bearer token authenticates this host, not the app: {body}"
    );
    assert!(
        body.contains(&format!("prefix=/preview/{dev_port}")),
        "so the app can still build a URL that comes back here: {body}"
    );
    assert!(
        body.contains("fwd_host=127.0.0.1:") && body.contains("fwd_proto=http"),
        "the forwarding headers say who the request was really for: {body}"
    );
    assert!(
        body.ends_with("body=draft=true"),
        "and the form itself arrived: {body}"
    );

    let observed = last_seen(&seen);
    assert_eq!(observed.method, "POST");
    assert_eq!(observed.path, "/submit");
    assert_eq!(observed.authorization, None, "dropped, not merely hidden");
    assert_eq!(
        observed.forwarded_prefix.as_deref(),
        Some(format!("/preview/{dev_port}").as_str())
    );
    assert_eq!(observed.body, b"draft=true".to_vec());
}

/// A dev server's `Set-Cookie: Path=/` would otherwise be stored against the whole
/// host and replayed on every other route this server has.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cookie_from_the_previewed_app_cannot_escape_its_prefix() {
    let Fixture {
        mut host, dev_port, ..
    } = fixture(&[]).await;
    host.wait_for("previewing dev servers").await;
    let web_port = host.port;
    let call = Call::get(
        web_port,
        preview_path(dev_port, "set-cookie"),
        browser_headers(web_port),
    );
    let response = tokio::task::spawn_blocking(move || call.run())
        .await
        .expect("join");

    assert_eq!(response.0, 303, "{response:?}");
    let cookies = all_headers(&response, "set-cookie");
    assert_eq!(cookies.len(), 2, "both cookies survive: {cookies:?}");
    assert_eq!(
        cookies[0],
        format!("sid=abc; Path=/preview/{dev_port}/; HttpOnly; SameSite=Lax"),
        "Path is scoped to the preview and Domain is dropped"
    );
    assert_eq!(
        cookies[1],
        format!("theme=dark; Path=/preview/{dev_port}/"),
        "a cookie that named no Path still cannot escape the prefix"
    );
    assert_eq!(
        header_of(&response, "location").as_deref(),
        Some(format!("/preview/{dev_port}/").as_str()),
        "a redirect to the app's own root stays inside the preview, not this host"
    );
}

/// The two ways a dev server writes a redirect both have to land back inside the
/// prefix, and somebody else's URL has to be left alone.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_redirect_from_the_previewed_app_stays_under_the_prefix() {
    let Fixture {
        mut host, dev_port, ..
    } = fixture(&[]).await;
    host.wait_for("previewing dev servers").await;
    let web_port = host.port;
    let calls: Vec<Call> = ["redirect-root", "redirect-abs", "redirect-away"]
        .iter()
        .map(|tail| {
            Call::get(
                web_port,
                preview_path(dev_port, tail),
                vec![host_header(web_port)],
            )
        })
        .collect();
    let responses =
        tokio::task::spawn_blocking(move || calls.iter().map(Call::run).collect::<Vec<_>>())
            .await
            .expect("join");

    assert_eq!(responses[0].0, 302, "{:?}", responses[0]);
    assert_eq!(
        header_of(&responses[0], "location").as_deref(),
        Some(format!("/preview/{dev_port}/login").as_str()),
        "a root-relative redirect is relative to the app, not to this host"
    );
    assert_eq!(responses[1].0, 302, "{:?}", responses[1]);
    assert_eq!(
        header_of(&responses[1], "location").as_deref(),
        Some(format!("/preview/{dev_port}/login?next=%2Fa").as_str()),
        "and so is one written as an absolute URL to the dev server itself"
    );
    assert_eq!(responses[2].0, 302, "{:?}", responses[2]);
    assert_eq!(
        header_of(&responses[2], "location").as_deref(),
        Some("https://example.com/docs"),
        "a host the app is genuinely leaving must not be dragged along"
    );
}

/// A previewed app POSTs forms and uploads files; that traffic is not this
/// server's 64 KiB JSON API.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_previewed_post_is_not_bounded_by_the_api_request_cap() {
    let Fixture {
        mut host,
        dev_port,
        seen,
        ..
    } = fixture(&[]).await;
    host.wait_for("previewing dev servers").await;
    let web_port = host.port;
    let payload = vec![b'x'; 200 * 1024];
    let call = Call {
        port: web_port,
        method: "POST",
        path: preview_path(dev_port, "upload"),
        headers: browser_headers(web_port),
        body: payload,
    };
    let (status, _, body) = tokio::task::spawn_blocking(move || call.run())
        .await
        .expect("join");

    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body.rsplit("body=").next().expect("body marker").len(),
        200 * 1024,
        "the upload arrived whole"
    );
    assert_eq!(last_seen(&seen).method, "POST");
}

// ---------------------------------------------------------------------------
// The HMR socket
// ---------------------------------------------------------------------------

/// A page loads and then never updates: that is the HMR socket failing, so the
/// bridge is the part of this feature a developer feels first.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_hmr_socket_is_bridged_with_its_own_handshake() {
    let Fixture {
        mut host,
        dev_port,
        seen,
        ..
    } = fixture(&[]).await;
    host.wait_for("previewing dev servers").await;
    let web_port = host.port;
    let origin = format!("http://127.0.0.1:{web_port}");
    let (mut socket, status, handshake) =
        connect_preview(web_port, dev_port, &origin, &[HMR_PROTOCOL]).await;

    assert_eq!(status, 101, "handshake headers: {handshake:?}");
    assert_eq!(
        handshake
            .iter()
            .find(|(name, _)| name == "sec-websocket-protocol")
            .map(|(_, value)| value.as_str()),
        Some(HMR_PROTOCOL),
        "the dev server's choice is reported back to the browser"
    );

    let greeting = socket
        .next()
        .await
        .expect("stream")
        .expect("frame")
        .to_text()
        .expect("utf-8")
        .to_string();
    assert!(
        greeting.contains(&format!("host=127.0.0.1:{dev_port}")),
        "the handshake upstream carried the app's own Host: {greeting}"
    );
    assert!(
        greeting.contains(&format!("origin=http://127.0.0.1:{dev_port}")),
        "and its own Origin, or the app would have refused the socket: {greeting}"
    );
    assert!(
        greeting.contains("keys=1"),
        "exactly one Sec-WebSocket-Key upstream, or the accept check is a coin toss: {greeting}"
    );
    assert!(
        greeting.contains("proto=chaos-hmr"),
        "the negotiated subprotocol is what the app expects: {greeting}"
    );

    socket
        .send(WireMessage::Text("hmr-ping".into()))
        .await
        .expect("send");
    let reply = socket.next().await.expect("stream").expect("frame");
    assert_eq!(reply.to_text().expect("utf-8"), "echo hmr-ping");
    socket.send(WireMessage::Close(None)).await.expect("close");

    let observed = last_seen(&seen);
    assert_eq!(observed.path, "/ws");
    assert_eq!(
        observed.websocket_keys, 1,
        "the browser's key stays with the browser; the proxy handshakes for itself"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_subprotocol_nobody_offered_is_not_echoed_back() {
    let Fixture {
        mut host, dev_port, ..
    } = fixture(&[]).await;
    host.wait_for("previewing dev servers").await;
    let web_port = host.port;
    let origin = format!("http://127.0.0.1:{web_port}");
    let (mut socket, status, handshake) = connect_preview(web_port, dev_port, &origin, &[]).await;

    assert_eq!(status, 101, "handshake headers: {handshake:?}");
    assert!(
        !handshake
            .iter()
            .any(|(name, _)| name == "sec-websocket-protocol"),
        "the browser offered nothing, so selecting something is a protocol violation: {handshake:?}"
    );
    let greeting = socket
        .next()
        .await
        .expect("stream")
        .expect("frame")
        .to_text()
        .expect("utf-8")
        .to_string();
    assert!(
        greeting.contains("proto=(none)"),
        "the app sees no subprotocol either: {greeting}"
    );
}

// ---------------------------------------------------------------------------
// The upgrade request as the dev server receives it, byte for byte
// ---------------------------------------------------------------------------

/// A text frame as a server sends one: unmasked, short enough not to need an
/// extended length.
fn unmasked_text_frame(text: &str) -> Vec<u8> {
    let payload = text.as_bytes();
    let mut out = vec![0x81];
    if payload.len() < 126 {
        out.push(payload.len() as u8);
    } else {
        out.push(126);
        out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    }
    out.extend_from_slice(payload);
    out
}

fn read_exact_or_eof(socket: &mut std::net::TcpStream, buf: &mut [u8]) -> bool {
    let mut filled = 0;
    while filled < buf.len() {
        match socket.read(&mut buf[filled..]) {
            Ok(0) => return false,
            Ok(n) => filled += n,
            Err(error) => panic!("reading from the bridge: {error}"),
        }
    }
    true
}

/// One frame off the wire: its opcode, whether the client bit said it was masked,
/// and its payload.
fn read_frame(socket: &mut std::net::TcpStream) -> Option<(u8, bool, String)> {
    let mut header = [0u8; 2];
    if !read_exact_or_eof(socket, &mut header) {
        return None;
    }
    let masked = header[1] & 0x80 != 0;
    let length = match header[1] & 0x7f {
        126 => {
            let mut len = [0u8; 2];
            if !read_exact_or_eof(socket, &mut len) {
                return None;
            }
            u16::from_be_bytes(len) as usize
        }
        127 => {
            let mut len = [0u8; 8];
            if !read_exact_or_eof(socket, &mut len) {
                return None;
            }
            u64::from_be_bytes(len) as usize
        }
        short => short as usize,
    };
    let mut mask = [0u8; 4];
    if masked && !read_exact_or_eof(socket, &mut mask) {
        return None;
    }
    let mut payload = vec![0u8; length];
    if !read_exact_or_eof(socket, &mut payload) {
        return None;
    }
    if masked {
        for (index, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[index % 4];
        }
    }
    Some((
        header[0] & 0x0f,
        masked,
        String::from_utf8_lossy(&payload).into_owned(),
    ))
}

/// The value of a header in a raw request block, and how many times it appeared.
fn raw_header(request: &str, name: &str) -> (usize, Option<String>) {
    let mut seen = 0;
    let mut first = None;
    for line in request.lines().skip(1) {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case(name) {
            seen += 1;
            if first.is_none() {
                first = Some(value.trim().to_string());
            }
        }
    }
    (seen, first)
}

/// A dev server that completes the upgrade by hand and keeps the raw request.
///
/// The axum stand-in above asserts what the app was able to read; this one asserts
/// what was actually sent, which is what a browser and a dev server negotiate over.
/// It also answers with the accept key derived from whatever key it was handed, so
/// the handshake only completes if the bridge validates against the key it put on
/// the wire itself — reusing the browser's key fails here rather than passing
/// quietly.
fn raw_upstream(
    listener: TcpListener,
    request: Arc<Mutex<String>>,
    frames: Arc<Mutex<Vec<(u8, bool, String)>>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("the bridge connects");
        socket
            .set_read_timeout(Some(Duration::from_secs(30)))
            .expect("read timeout on the raw dev server");
        let mut bytes = Vec::new();
        let mut byte = [0u8; 1];
        while !bytes.ends_with(b"\r\n\r\n") {
            let read = socket.read(&mut byte).expect("read the upgrade request");
            assert_ne!(read, 0, "the bridge closed before finishing its handshake");
            bytes.push(byte[0]);
            assert!(
                bytes.len() < 8 * 1024,
                "a handshake that never ends: {bytes:?}"
            );
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let (_, key) = raw_header(&text, "sec-websocket-key");
        let key = key.expect("the bridge sent a Sec-WebSocket-Key");
        let offered = raw_header(&text, "sec-websocket-protocol").1;
        let mut response = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\
             Connection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n",
            derive_accept_key(key.as_bytes()),
        );
        if offered.is_some_and(|offered| {
            offered
                .split(',')
                .any(|offer| offer.trim().eq_ignore_ascii_case(HMR_PROTOCOL))
        }) {
            response.push_str(&format!("Sec-WebSocket-Protocol: {HMR_PROTOCOL}\r\n"));
        }
        response.push_str("\r\n");
        socket.write_all(response.as_bytes()).expect("101");
        socket.flush().expect("flush the 101");
        *request.lock().unwrap() = text;
        // First a frame the bridge has to carry downstream, then one round trip so
        // a bridge that stops pumping in either direction is visible from the
        // browser's side of the connection.
        socket
            .write_all(&unmasked_text_frame("from-upstream"))
            .expect("push a frame downstream");
        socket.flush().expect("flush the frame");
        loop {
            let Some(frame) = read_frame(&mut socket) else {
                return;
            };
            let echo = match frame.0 {
                1 => Some(format!("echo:{}", frame.2)),
                _ => None,
            };
            frames.lock().unwrap().push(frame);
            if let Some(reply) = echo {
                if socket.write_all(&unmasked_text_frame(&reply)).is_err() {
                    return;
                }
                socket.flush().expect("flush the echo");
            }
        }
    })
}

/// The handshake the bridge sends upstream has to be a handshake a dev server can
/// answer: one of each single-value header, the app's own authority, the browser's
/// subprotocol offer, the host's credential left behind, and frames masked the way
/// a client must mask them.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_upgrade_request_the_bridge_sends_is_one_the_app_can_answer() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("the raw dev server binds");
    let dev_port = listener
        .local_addr()
        .expect("the raw dev server's own address")
        .port();
    let raw = Arc::new(Mutex::new(String::new()));
    let frames = Arc::new(Mutex::new(Vec::new()));
    let upstream = raw_upstream(listener, Arc::clone(&raw), Arc::clone(&frames));

    let ports = dev_port.to_string();
    let mut host = start_host(&[
        ("CHAOS_WEB_PREVIEW_PORTS", ports.as_str()),
        ("CHAOS_WEB_TOKEN", "rotatable-token"),
    ]);
    host.wait_for("previewing dev servers").await;
    let web_port = host.port;
    let origin = format!("http://127.0.0.1:{web_port}");
    let (mut socket, status, _) = connect_preview_with(
        web_port,
        dev_port,
        &origin,
        &[HMR_PROTOCOL],
        &[("Authorization", "Bearer rotatable-token")],
    )
    .await;
    assert_eq!(status, 101);
    assert_eq!(next_text(&mut socket).await, "from-upstream");
    socket
        .send(WireMessage::Text("hmr-ping".into()))
        .await
        .expect("send");
    assert_eq!(next_text(&mut socket).await, "echo:hmr-ping");

    let handshake = raw.lock().unwrap().clone();
    let count = |name: &str| raw_header(&handshake, name).0;
    let value = |name: &str| raw_header(&handshake, name).1;
    assert_eq!(
        handshake.lines().next().unwrap_or_default(),
        "GET /ws HTTP/1.1",
        "the prefix is the browser's business, not the app's: {handshake}"
    );
    assert_eq!(
        count("upgrade"),
        1,
        "a doubled header is a malformed request: {handshake}"
    );
    assert_eq!(
        value("upgrade").unwrap_or_default().to_ascii_lowercase(),
        "websocket"
    );
    assert_eq!(
        count("connection"),
        1,
        "one Connection, whatever tokens it holds: {handshake}"
    );
    assert_eq!(
        value("connection").unwrap_or_default().to_ascii_lowercase(),
        "upgrade",
        "one token, not the browser's copy joined to the bridge's: {handshake}"
    );
    assert_eq!(
        value("host").as_deref(),
        Some(format!("127.0.0.1:{dev_port}").as_str()),
        "one Host, and it is the app's own"
    );
    assert_eq!(
        value("origin").as_deref(),
        Some(format!("http://127.0.0.1:{dev_port}").as_str()),
        "one Origin, or a dev server with a check like Vite's refuses the socket"
    );
    assert_eq!(
        count("sec-websocket-key"),
        1,
        "the accept could only have validated against the key the bridge itself sent"
    );
    assert_eq!(
        (
            count("sec-websocket-version"),
            value("sec-websocket-version")
        ),
        (1, Some("13".to_string())),
    );
    assert_eq!(
        (
            count("sec-websocket-protocol"),
            value("sec-websocket-protocol")
        ),
        (1, Some(HMR_PROTOCOL.to_string())),
        "the browser's offer travels, or the app refuses the socket it wanted"
    );
    assert_eq!(
        count("authorization"),
        0,
        "the host's credential stays on this side of the proxy: {handshake}"
    );
    assert_eq!(
        count("sec-websocket-extensions"),
        0,
        "permessage-deflate is not bridged, so it is not offered either"
    );
    assert_eq!(
        frames
            .lock()
            .unwrap()
            .iter()
            .filter(|(opcode, _, _)| *opcode == 1)
            .map(|(_, masked, payload)| (*masked, payload.clone()))
            .collect::<Vec<_>>(),
        vec![(true, "hmr-ping".to_string())],
        "upstream the bridge is a client, so its frames are masked"
    );

    drop(socket);
    upstream.join().expect("the raw dev server finished");
}

/// A page on another site must not be able to drive requests into somebody's dev
/// server through their own browser session.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_page_from_another_site_cannot_reach_a_preview() {
    let Fixture {
        mut host,
        dev_port,
        seen,
        ..
    } = fixture(&[]).await;
    host.wait_for("previewing dev servers").await;
    let web_port = host.port;
    let before = seen_count(&seen);
    let mut headers = vec![host_header(web_port)];
    headers.push(("Origin".to_string(), "http://evil.example".to_string()));
    let call = Call {
        port: web_port,
        method: "POST",
        path: preview_path(dev_port, "submit"),
        headers,
        body: Vec::new(),
    };
    let response = tokio::task::spawn_blocking(move || call.run())
        .await
        .expect("join");

    assert_eq!(response.0, 401, "{response:?}");
    assert_eq!(refusal_code(&response.2), "origin_not_allowed");
    assert_eq!(
        seen_count(&seen),
        before,
        "a refused request must not reach the app at all"
    );
}

// ---------------------------------------------------------------------------
// What stands in front of the preview
// ---------------------------------------------------------------------------

/// The default the row asks for: nothing is previewed until a port is named.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_preview_that_was_never_enabled_says_so() {
    let dev_port = free_port();
    let mut host = start_host(&[]);
    let web_port = host.port;
    host.wait_for("listening on").await;
    let (index, page) = tokio::task::spawn_blocking(move || {
        (
            Call::get(
                web_port,
                "/preview".to_string(),
                vec![host_header(web_port)],
            )
            .run(),
            Call::get(
                web_port,
                preview_path(dev_port, ""),
                vec![host_header(web_port)],
            )
            .run(),
        )
    })
    .await
    .expect("join");

    assert_eq!(index.0, 404, "the index is not served either: {index:?}");
    assert_eq!(refusal_code(&index.2), "preview_disabled");
    assert_eq!(page.0, 403, "{page:?}");
    assert_eq!(refusal_code(&page.2), "preview_disabled");
    assert!(
        page.2.contains("CHAOS_WEB_PREVIEW_PORTS"),
        "and it names the knob to turn: {page:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unnamed_port_and_a_dead_dev_server_are_told_apart() {
    let Fixture {
        mut host,
        dev_port,
        dead_port,
        ..
    } = fixture(&[]).await;
    host.wait_for("previewing dev servers").await;
    let web_port = host.port;
    let other = free_port();
    let (unnamed, dead, index) = tokio::task::spawn_blocking(move || {
        (
            Call::get(
                web_port,
                preview_path(other, ""),
                vec![host_header(web_port)],
            )
            .run(),
            Call::get(
                web_port,
                preview_path(dead_port, ""),
                vec![host_header(web_port)],
            )
            .run(),
            Call::get(
                web_port,
                "/preview".to_string(),
                vec![host_header(web_port)],
            )
            .run(),
        )
    })
    .await
    .expect("join");

    assert_eq!(unnamed.0, 403, "{unnamed:?}");
    assert_eq!(refusal_code(&unnamed.2), "preview_port_not_allowed");
    assert!(
        unnamed
            .2
            .contains(&format!("enabled: {dev_port},{dead_port}")),
        "a wrong port has to say which ports are on: {unnamed:?}"
    );
    // The dead one is the operator's own typo or a server that has not started
    // yet; it must not read as a permission problem.
    assert_eq!(dead.0, 502, "{dead:?}");
    assert_eq!(refusal_code(&dead.2), "preview_upstream_unreachable");
    assert_eq!(index.0, 200, "{index:?}");
    let listed: serde_json::Value = serde_json::from_str(&index.2).expect("index json");
    let ports: Vec<u16> = listed["previews"]
        .as_array()
        .expect("previews array")
        .iter()
        .map(|entry| entry["port"].as_u64().expect("port") as u16)
        .collect();
    assert_eq!(
        ports,
        vec![dev_port, dead_port],
        "the index lists what is on"
    );
    assert_eq!(listed["upstream"], "127.0.0.1");
    assert_eq!(
        listed["previews"][0]["url"].as_str(),
        Some(format!("/preview/{dev_port}/").as_str())
    );
}

/// Only WebSocket is bridged. Anything else is answered by name instead of with a
/// plain response to an upgrade request, which a browser reports as a failure with
/// no hint why.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_upgrade_that_is_not_websocket_is_refused_by_name() {
    let Fixture {
        mut host, dev_port, ..
    } = fixture(&[]).await;
    host.wait_for("previewing dev servers").await;
    let web_port = host.port;
    let mut headers = vec![host_header(web_port)];
    headers.push(("Connection".to_string(), "Upgrade".to_string()));
    headers.push(("Upgrade".to_string(), "h2c".to_string()));
    let call = Call::get(web_port, preview_path(dev_port, ""), headers);
    let response = tokio::task::spawn_blocking(move || call.run())
        .await
        .expect("join");

    assert_eq!(response.0, 400, "{response:?}");
    assert_eq!(refusal_code(&response.2), "preview_upgrade_unsupported");
}

/// Publishing this host and publishing a project's dev server are two different
/// decisions, and the second one has to be made out loud.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_preview_is_not_reachable_through_the_public_name() {
    let public: [(&str, &str); 4] = [
        ("Host", "chaos.example.test:8443"),
        ("Origin", "https://chaos.example.test:8443"),
        ("X-Forwarded-Proto", "https"),
        ("Authorization", "Bearer rotatable-token"),
    ];
    let declared: [(&str, &str); 2] = [
        ("CHAOS_WEB_TOKEN", "rotatable-token"),
        ("CHAOS_WEB_PUBLIC_ORIGIN", "https://chaos.example.test:8443"),
    ];

    let Fixture {
        mut host,
        dev_port,
        seen,
        ..
    } = fixture(&declared).await;
    host.wait_for("previewing dev servers on loopback only")
        .await;
    let web_port = host.port;
    let refused = tokio::task::spawn_blocking(move || {
        raw_request(web_port, "GET", &preview_path(dev_port, ""), &public, b"")
    })
    .await
    .expect("join");

    assert_eq!(refused.0, 401, "{refused:?}");
    assert_eq!(refusal_code(&refused.2), "preview_loopback_only");
    assert!(
        refused.2.contains("CHAOS_WEB_PREVIEW_ALLOW_PUBLIC"),
        "the only way past this has to be named: {refused:?}"
    );
    assert_eq!(
        seen_count(&seen),
        0,
        "and nothing was dialed on the way to that refusal"
    );

    // The same host and the same public name, with the second decision made
    // explicitly, does serve — and the dev server still sees only loopback.
    let mut env: Vec<(&str, &str)> = declared.to_vec();
    env.push(("CHAOS_WEB_PREVIEW_ALLOW_PUBLIC", "1"));
    let Fixture {
        mut host, dev_port, ..
    } = fixture(&env).await;
    let logged = host
        .wait_for("previewing dev servers through the declared public origin as well")
        .await;
    let web_port = host.port;
    let served = tokio::task::spawn_blocking(move || {
        raw_request(web_port, "GET", &preview_path(dev_port, ""), &public, b"")
    })
    .await
    .expect("join");

    assert_eq!(served.0, 200, "{served:?}");
    assert!(
        served.2.contains(&format!("host=127.0.0.1:{dev_port}")),
        "the public name belongs to the browser's side only: {}",
        served.2
    );
    assert!(
        logged.contains(&format!("/preview/{dev_port}/")),
        "startup still says what is behind what: {logged}"
    );
}

/// Loopback still means loopback: a public name that merely begins with the
/// loopback address is not it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_forged_loopback_hostname_is_not_loopback() {
    let Fixture {
        mut host, dev_port, ..
    } = fixture(&[
        ("CHAOS_WEB_TOKEN", "rotatable-token"),
        ("CHAOS_WEB_PUBLIC_ORIGIN", "https://127.0.0.1.evil.example"),
    ])
    .await;
    host.wait_for("previewing dev servers").await;
    let web_port = host.port;
    let mut headers = vec![("Host".to_string(), "127.0.0.1.evil.example".to_string())];
    headers.push((
        "Authorization".to_string(),
        "Bearer rotatable-token".to_string(),
    ));
    let call = Call::get(web_port, preview_path(dev_port, ""), headers);
    let response = tokio::task::spawn_blocking(move || call.run())
        .await
        .expect("join");

    assert_eq!(response.0, 401, "{response:?}");
    assert_eq!(refusal_code(&response.2), "preview_loopback_only");
}

/// Merging the preview routes after the layers must not disturb what was already
/// being served.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn preview_routes_leave_the_rest_of_the_host_alone() {
    let Fixture {
        mut host, dev_port, ..
    } = fixture(&[("CHAOS_WEB_TOKEN", "rotatable-token")]).await;
    host.wait_for("previewing dev servers").await;
    let web_port = host.port;
    let with_token = |call: &mut Call| {
        call.headers.push((
            "Authorization".to_string(),
            "Bearer rotatable-token".to_string(),
        ));
    };
    let (health, handshake, anonymous, oversized, slow) = tokio::task::spawn_blocking(move || {
        let mut handshake = Call::get(
            web_port,
            "/api/handshake".to_string(),
            vec![host_header(web_port)],
        );
        with_token(&mut handshake);
        let mut oversized = Call {
            port: web_port,
            method: "POST",
            path: "/api/sessions".to_string(),
            headers: vec![
                host_header(web_port),
                ("Content-Type".to_string(), "application/json".to_string()),
            ],
            body: vec![b'x'; 70 * 1024],
        };
        with_token(&mut oversized);
        (
            Call::get(web_port, "/health".to_string(), vec![host_header(web_port)]).run(),
            handshake.run(),
            Call::get(
                web_port,
                "/api/handshake".to_string(),
                vec![host_header(web_port)],
            )
            .run(),
            oversized.run(),
            Call::get(
                web_port,
                preview_path(dev_port, "slow"),
                vec![host_header(web_port)],
            )
            .run(),
        )
    })
    .await
    .expect("join");

    assert_eq!(health.0, 200, "{health:?}");
    assert_eq!(handshake.0, 200, "{handshake:?}");
    assert_eq!(anonymous.0, 401, "{anonymous:?}");
    assert_eq!(refusal_code(&anonymous.2), "credential_required");
    assert_eq!(
        oversized.0, 413,
        "the API keeps its own 64 KiB cap: {oversized:?}"
    );
    assert_eq!(slow.0, 200, "{slow:?}");
    assert_eq!(slow.2, "eventually");
}
