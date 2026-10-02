use axum::{
    Json, Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chaos_engine::{ClientMessage, Engine, PROTOCOL_VERSION, ServerMessage};
use sha2::{Digest, Sha256};
use std::{
    net::SocketAddr,
    path::{Component, Path, PathBuf},
    sync::Arc,
};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::{cors::CorsLayer, limit::RequestBodyLimitLayer};

const MAX_REQUEST_BYTES: usize = 64 * 1024;
const DEV_ORIGINS: [&str; 4] = [
    "http://127.0.0.1:5173",
    "http://localhost:5173",
    "http://127.0.0.1:5174",
    "http://localhost:5174",
];
const REQUEST_ID: &str = "client_msg_id";

#[derive(Clone)]
pub struct WebState {
    pub engine: Engine,
    pub token: Arc<String>,
    pub safe_web_mode: bool,
}
pub fn router(engine: Engine, token: impl Into<String>) -> Router {
    router_with_safe_mode(engine, token, false)
}

pub fn router_with_safe_mode(
    engine: Engine,
    token: impl Into<String>,
    safe_web_mode: bool,
) -> Router {
    router_with_assets_and_safe_mode(engine, token, safe_web_mode, None)
}

pub fn router_with_assets_and_safe_mode(
    engine: Engine,
    token: impl Into<String>,
    safe_web_mode: bool,
    assets_dir: Option<std::path::PathBuf>,
) -> Router {
    let state = Arc::new(WebState {
        engine,
        token: Arc::new(token.into()),
        safe_web_mode,
    });
    let router = Router::new()
        .route("/health", get(health))
        .route("/api/handshake", get(handshake))
        .route("/api/sessions", post(create_session))
        .route("/ws", get(websocket))
        .layer(
            CorsLayer::new().allow_origin(
                dev_origins()
                    .iter()
                    .filter_map(|origin| origin.parse::<HeaderValue>().ok())
                    .collect::<Vec<HeaderValue>>(),
            ),
        )
        .layer(RequestBodyLimitLayer::new(MAX_REQUEST_BYTES))
        .with_state(state);
    if let Some(assets_dir) = assets_dir {
        router
            .nest_service(
                "/api",
                Router::new().fallback(|| async { StatusCode::NOT_FOUND }),
            )
            .nest_service(
                "/assets",
                tower::ServiceBuilder::new()
                    .layer(axum::middleware::from_fn_with_state(
                        assets_dir.join("assets"),
                        add_static_etag,
                    ))
                    .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
                        header::CACHE_CONTROL,
                        HeaderValue::from_static("public, no-cache"),
                    ))
                    .layer(tower_http::set_header::SetResponseHeaderLayer::appending(
                        header::VARY,
                        HeaderValue::from_static("Accept-Encoding"),
                    ))
                    .service(
                        ServeDir::new(assets_dir.join("assets"))
                            .precompressed_gzip()
                            .precompressed_br(),
                    ),
            )
            .fallback_service(
                tower::ServiceBuilder::new()
                    .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
                        header::CACHE_CONTROL,
                        HeaderValue::from_static("no-cache"),
                    ))
                    .layer(tower_http::set_header::SetResponseHeaderLayer::appending(
                        header::VARY,
                        HeaderValue::from_static("Accept-Encoding"),
                    ))
                    .service(
                        ServeFile::new(assets_dir.join("index.html"))
                            .precompressed_gzip()
                            .precompressed_br(),
                    ),
            )
    } else {
        router
    }
}

async fn health() -> Response {
    secure_json(serde_json::json!({ "status": "ok" }))
}

fn constant_time_equal(left: &str, right: &str) -> bool {
    let mut diff = left.len() ^ right.len();
    for (a, b) in left.as_bytes().iter().zip(right.as_bytes()) {
        diff |= usize::from(a != b);
    }
    diff == 0
}

fn authorized(state: &WebState, headers: &HeaderMap) -> bool {
    if state.token.is_empty() {
        return true;
    }
    let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let Some(candidate) = value.strip_prefix("Bearer ") else {
        return false;
    };
    constant_time_equal(candidate, state.token.as_str())
}

fn dev_origins() -> Vec<String> {
    let mut origins = DEV_ORIGINS
        .iter()
        .map(|origin| (*origin).to_owned())
        .collect::<Vec<_>>();
    let dynamic = approved_dynamic_dev_origin(
        std::env::var("CHAOS_WEB_DEV_ORIGIN").ok().as_deref(),
        std::env::var("CHAOS_WEB_ALLOW_DYNAMIC_DEV_ORIGIN").as_deref() == Ok("1"),
        cfg!(debug_assertions),
    );
    if let Some(origin) = dynamic
        && !origins.contains(&origin)
    {
        origins.push(origin);
    }
    origins
}

fn approved_dynamic_dev_origin(
    origin: Option<&str>,
    explicitly_enabled: bool,
    debug_build: bool,
) -> Option<String> {
    if !explicitly_enabled || !debug_build {
        return None;
    }
    let origin = origin?;
    let Ok(uri) = origin.parse::<axum::http::Uri>() else {
        return None;
    };
    if !matches!(uri.scheme_str(), Some("http"))
        || uri.path() != "/"
        || uri.query().is_some()
        || origin.contains('#')
    {
        return None;
    }
    let authority = uri.authority()?;
    if authority.as_str().contains('@') || invalid_port(authority.as_str()) {
        return None;
    }
    let host = authority.host().to_ascii_lowercase();
    let loopback_host = matches!(host.as_str(), "127.0.0.1" | "localhost" | "[::1]" | "::1");
    let port = authority.port_u16()?;
    (loopback_host && port != 0).then(|| format!("http://{authority}"))
}

/// Parse the origin the operator says this deployment is reached through.
///
/// Strict on purpose: the value decides which `Host` and `Origin` headers are
/// accepted from outside loopback, so a typo must fail loudly rather than
/// quietly widen what the server answers.
pub fn public_origin_authority(declared: &str) -> Result<String, String> {
    let trimmed = declared.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("empty".to_string());
    }
    let uri = trimmed
        .parse::<axum::http::Uri>()
        .map_err(|error| format!("not a valid URI: {error}"))?;
    // `https:` only: the point of this setting is a TLS terminator in front of
    // the loopback server, and an `http:` value would accept a plaintext origin
    // the frontend never sends (it picks `wss:` exactly when the page is https).
    if uri.scheme_str() != Some("https") {
        return Err("must be an https: origin".to_string());
    }
    let authority = uri.authority().ok_or("missing host")?;
    if authority.as_str().contains('@') {
        return Err("must not contain credentials".to_string());
    }
    if authority.host().is_empty() || invalid_port(authority.as_str()) {
        return Err("host or port is not valid".to_string());
    }
    if !matches!(uri.path(), "" | "/") || uri.query().is_some() || trimmed.contains('#') {
        return Err("must not contain a path, query, or fragment".to_string());
    }
    Ok(authority.as_str().to_ascii_lowercase())
}

/// The declared public origin's authority, or `None` when unset or unusable.
/// Read per request like [`dev_origins`], so the checks have no cached state.
fn configured_public_authority() -> Option<String> {
    let declared = std::env::var("CHAOS_WEB_PUBLIC_ORIGIN").ok()?;
    if declared.trim().is_empty() {
        return None;
    }
    match public_origin_authority(&declared) {
        Ok(authority) => Some(authority),
        Err(reason) => {
            eprintln!("CHAOS_WEB_PUBLIC_ORIGIN ignored: {reason}");
            None
        }
    }
}

/// Whether a header's authority is exactly the one the operator declared.
fn configured_public_authority_matches(
    declared: Option<&str>,
    seen: &axum::http::uri::Authority,
) -> bool {
    declared.is_some_and(|declared| seen.as_str().eq_ignore_ascii_case(declared))
}

/// Whether the proxy told us the browser's side of the connection was TLS.
///
/// This is an extra condition, not an authorization source: the backend binds
/// to loopback, so only the local proxy can set it, and the operator's declared
/// origin plus the bearer token still have to match.
fn forwarded_proto_https(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .next()
                .unwrap_or_default()
                .trim()
                .eq_ignore_ascii_case("https")
        })
}

fn origin_allowed(headers: &HeaderMap) -> bool {
    origin_allowed_against(headers, configured_public_authority().as_deref())
}

fn origin_allowed_against(headers: &HeaderMap, public_authority: Option<&str>) -> bool {
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return true;
    };
    if dev_origins().iter().any(|allowed| origin == allowed) {
        return true;
    }
    let Ok(origin_uri) = origin.parse::<axum::http::Uri>() else {
        return false;
    };
    // A page served over HTTPS sends an `https:` origin, which can never be
    // same-origin with the `http` request Host the backend actually sees. It is
    // accepted only when the operator named that exact origin and the proxy
    // reported TLS, so a random `https://elsewhere` page is still refused.
    if origin_uri.scheme_str() == Some("https") {
        let seen = origin_uri.authority();
        return seen.is_some_and(|seen| {
            !seen.as_str().contains('@')
                && !invalid_port(seen.as_str())
                && configured_public_authority_matches(public_authority, seen)
                && forwarded_proto_https(headers)
        });
    }
    let Some(request_host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    origin_uri.scheme_str() == Some("http")
        && matches!(origin_uri.path(), "" | "/")
        && origin_uri.query().is_none()
        && !origin.contains('#')
        && origin_uri.authority().is_some_and(|authority| {
            authority.as_str().eq_ignore_ascii_case(request_host)
                && !authority.as_str().contains('@')
                && !invalid_port(authority.as_str())
        })
}

fn invalid_port(authority: &str) -> bool {
    let port = if authority.starts_with('[') {
        authority.find(']').and_then(|close| {
            authority
                .get(close + 1..)
                .filter(|suffix| suffix.starts_with(':'))
                .map(|suffix| &suffix[1..])
        })
    } else {
        authority.rsplit_once(':').map(|(_, port)| port)
    };
    port.is_some_and(|port| {
        port.is_empty()
            || !port.bytes().all(|byte| byte.is_ascii_digit())
            || port.parse::<u16>().is_err()
    })
}

fn accepted_encoding_quality(headers: &HeaderMap, encodings: &[&str]) -> u16 {
    headers
        .get_all(header::ACCEPT_ENCODING)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|entry| {
            let mut parts = entry.trim().splitn(2, ';');
            let name = parts.next()?.trim();
            if !encodings
                .iter()
                .any(|encoding| name.eq_ignore_ascii_case(encoding))
            {
                return None;
            }
            let q = parts.next().map(str::trim).unwrap_or("q=1");
            let value = q.strip_prefix("q=").or_else(|| q.strip_prefix("Q="))?;
            parse_quality(value)
        })
        .max()
        .unwrap_or(0)
}

fn accepted_gzip_quality(headers: &HeaderMap) -> u16 {
    accepted_encoding_quality(headers, &["gzip", "x-gzip"])
}

fn parse_quality(value: &str) -> Option<u16> {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if fraction.len() > 3 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let whole = whole.parse::<u16>().ok()?;
    if whole > 1 || (whole == 1 && fraction.bytes().any(|byte| byte != b'0')) {
        return None;
    }
    let mut result = whole * 1000;
    for (index, byte) in fraction.bytes().enumerate() {
        let factor = match index {
            0 => 100,
            1 => 10,
            2 => 1,
            _ => return None,
        };
        result += u16::from(byte - b'0') * factor;
    }
    Some(result)
}

async fn add_static_etag(
    axum::extract::State(root): axum::extract::State<PathBuf>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if request.method() != axum::http::Method::GET && request.method() != axum::http::Method::HEAD {
        return next.run(request).await;
    }
    let relative = request
        .uri()
        .path()
        .trim_start_matches("/assets/")
        .trim_start_matches('/');
    let gzip_quality = accepted_gzip_quality(request.headers());
    let compressed_suffix = relative
        .strip_suffix(".gz")
        .map(|base| (base, "gzip"))
        .or_else(|| relative.strip_suffix(".br").map(|base| (base, "br")));
    let accepts_compressed_variant = compressed_suffix.as_ref().is_none_or(|(_, encoding)| {
        if *encoding == "gzip" {
            gzip_quality > 0
        } else {
            accepted_encoding_quality(request.headers(), &[encoding]) > 0
        }
    });
    let (lookup_name, representation_suffix) = compressed_suffix.unwrap_or((relative, ""));
    let relative_path = Path::new(lookup_name);
    let canonical_root = dunce::canonicalize(&root).ok();
    let path = if accepts_compressed_variant
        && relative_path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        canonical_root
            .as_ref()
            .and_then(|canonical_root| dunce::canonicalize(canonical_root.join(relative_path)).ok())
            .filter(|path| {
                canonical_root
                    .as_ref()
                    .is_some_and(|root| path.starts_with(root))
            })
    } else {
        None
    };
    let Some(path) = path else {
        return next.run(request).await;
    };
    let (path, representation_suffix) = if representation_suffix.is_empty() {
        let br_quality = accepted_encoding_quality(request.headers(), &["br"]);
        let gzip_quality = accepted_gzip_quality(request.headers());
        let br_path = root.join(format!("{relative}.br"));
        let gzip_path = root.join(format!("{relative}.gz"));
        let selected = if br_quality > 0 && br_quality >= gzip_quality && br_path.is_file() {
            (br_path, ".br")
        } else if gzip_quality > 0 && gzip_path.is_file() {
            (gzip_path, ".gz")
        } else if br_quality > 0 && br_path.is_file() {
            (br_path, ".br")
        } else {
            (path, "")
        };
        let Some(canonical_root) = canonical_root.as_ref() else {
            return next.run(request).await;
        };
        let Ok(canonical_path) = dunce::canonicalize(selected.0) else {
            return next.run(request).await;
        };
        if !canonical_path.starts_with(canonical_root) {
            return next.run(request).await;
        }
        (canonical_path, selected.1)
    } else {
        (path, representation_suffix)
    };
    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return next.run(request).await;
    };
    let mut digest = Sha256::new();
    let mut chunk = [0_u8; 16 * 1024];
    loop {
        match tokio::io::AsyncReadExt::read(&mut file, &mut chunk).await {
            Ok(0) => break,
            Ok(read) => digest.update(&chunk[..read]),
            Err(_) => return next.run(request).await,
        }
    }
    let etag = format!(
        "\"{}{}\"",
        digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        representation_suffix
    );
    if if_none_match(request.headers(), &etag) {
        let mut response = StatusCode::NOT_MODIFIED.into_response();
        response.headers_mut().insert(
            header::ETAG,
            HeaderValue::from_str(&etag).expect("SHA-256 ETag is a valid header"),
        );
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, no-cache"),
        );
        response
            .headers_mut()
            .insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));
        return response;
    }
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(&etag).expect("SHA-256 ETag is a valid header"),
    );
    response
}

fn if_none_match(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .any(|candidate| {
            candidate == "*" || candidate == etag || candidate.strip_prefix("W/") == Some(etag)
        })
}

fn host_allowed(headers: &HeaderMap) -> bool {
    host_allowed_against(headers, configured_public_authority().as_deref())
}

fn host_allowed_against(headers: &HeaderMap, public_authority: Option<&str>) -> bool {
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let Ok(authority) = host.parse::<axum::http::uri::Authority>() else {
        return false;
    };
    if authority.as_str().contains('@') || invalid_port(authority.as_str()) {
        return false;
    }
    // Loopback stays valid — the proxy dials it and may or may not rewrite Host,
    // and health checks still arrive that way. The public name is additionally
    // valid only because the operator typed it.
    matches!(
        authority.host().to_ascii_lowercase().as_str(),
        "127.0.0.1" | "localhost" | "[::1]" | "::1"
    ) || configured_public_authority_matches(public_authority, &authority)
}

/// Why a request is not allowed, or `None` when it is.
///
/// The code goes into the 401 body because these rules fail identically from
/// outside: a proxy forwarding a name nobody declared, a proxy that stays quiet
/// about TLS, and a missing credential all look like "the page never connects",
/// and only one of them is fixed by editing the proxy.
fn refusal_reason(state: &WebState, headers: &HeaderMap) -> Option<&'static str> {
    if !host_allowed(headers) {
        return Some("host_not_allowed");
    }
    if !origin_allowed(headers) {
        return Some(origin_refusal(headers));
    }
    if !authorized(state, headers) {
        return Some("credential_required");
    }
    None
}

/// Split the one origin refusal an operator can act on from the rest.
fn origin_refusal(headers: &HeaderMap) -> &'static str {
    let names_the_declared_host = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .and_then(|origin| origin.parse::<axum::http::Uri>().ok())
        .filter(|uri| uri.scheme_str() == Some("https"))
        .is_some_and(|uri| {
            uri.authority().is_some_and(|seen| {
                configured_public_authority_matches(configured_public_authority().as_deref(), seen)
            })
        });
    if names_the_declared_host && !forwarded_proto_https(headers) {
        "origin_requires_forwarded_proto"
    } else {
        "origin_not_allowed"
    }
}

fn unauthorized(reason: &'static str) -> Response {
    let mut response = secure_json(serde_json::json!({ "error": reason }));
    *response.status_mut() = StatusCode::UNAUTHORIZED;
    response
}

fn reject(state: &WebState, headers: &HeaderMap) -> Option<Response> {
    refusal_reason(state, headers).map(unauthorized)
}

fn client_request_id(headers: &HeaderMap) -> String {
    headers
        .get(REQUEST_ID)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= 128)
        .map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

fn secure_json<T: serde::Serialize>(value: T) -> Response {
    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            ),
            (
                header::CONTENT_SECURITY_POLICY,
                HeaderValue::from_static("default-src 'self'; frame-ancestors 'none'"),
            ),
            (
                header::X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            ),
        ],
        Json(value),
    )
        .into_response()
}

async fn handshake(State(state): State<Arc<WebState>>, headers: HeaderMap) -> Response {
    if let Some(rejection) = reject(&state, &headers) {
        return rejection;
    }
    secure_json(ServerMessage::Handshake {
        protocol_version: PROTOCOL_VERSION,
    })
}

async fn create_session(State(state): State<Arc<WebState>>, headers: HeaderMap) -> Response {
    if let Some(rejection) = reject(&state, &headers) {
        return rejection;
    }
    let event = state.engine.handle(ClientMessage::CreateSession {
        client_msg_id: client_request_id(&headers),
        workspace_id: None,
    });
    secure_json(event)
}

async fn websocket(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if let Some(rejection) = reject(&state, &headers) {
        return rejection;
    }
    let engine = state.engine.clone();
    let safe_web_mode = state.safe_web_mode;
    upgrade.on_upgrade(move |socket| websocket_session(socket, engine, safe_web_mode))
}

fn safe_mode_allows(message: &ClientMessage) -> bool {
    matches!(
        message,
        ClientMessage::CreateSession { .. }
            | ClientMessage::Resume { .. }
            | ClientMessage::Snapshot { .. }
            | ClientMessage::Submit { .. }
            | ClientMessage::Cancel { .. }
            | ClientMessage::RespondQuestion { .. }
            | ClientMessage::GetSettings { .. }
            | ClientMessage::ListFiles { .. }
            | ClientMessage::ReadFile { .. }
            | ClientMessage::SearchFiles { .. }
            | ClientMessage::ScanMarketplace { .. }
            | ClientMessage::PreviewDiff { .. }
            | ClientMessage::FinalizeAttachment { .. }
            | ClientMessage::ImportTuiSession { .. }
    )
}

async fn websocket_session(mut socket: WebSocket, engine: Engine, safe_web_mode: bool) {
    let _ = socket
        .send(Message::Text(
            serde_json::to_string(&ServerMessage::Handshake {
                protocol_version: PROTOCOL_VERSION,
            })
            .unwrap()
            .into(),
        ))
        .await;
    while let Some(Ok(message)) = socket.recv().await {
        let Message::Text(text) = message else {
            continue;
        };
        if text.len() > MAX_REQUEST_BYTES {
            let _ = socket
                .send(Message::Text(
                    serde_json::to_string(&ServerMessage::Error {
                        code: "message_too_large".into(),
                        message: "消息超过 64 KiB 限制".into(),
                    })
                    .unwrap()
                    .into(),
                ))
                .await;
            continue;
        }
        match serde_json::from_str::<ClientMessage>(&text) {
            Ok(message) => {
                if safe_web_mode && !safe_mode_allows(&message) {
                    let event = ServerMessage::Error {
                        code: "safe_web_mode_blocked".into(),
                        message: "Safe Web Mode 禁止此操作".into(),
                    };
                    let _ = socket
                        .send(Message::Text(serde_json::to_string(&event).unwrap().into()))
                        .await;
                    continue;
                }
                // Prompt adapters block on a subprocess or an HTTPS round trip,
                // so they run on their own OS thread: a dedicated thread keeps
                // the shared async worker free and, unlike a blocking-pool
                // thread, carries no ambient runtime for the adapter to trip
                // over. Ordering within a connection is preserved by awaiting.
                let blocking_engine = engine.clone();
                let (tx, rx) = tokio::sync::oneshot::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(blocking_engine.handle(message));
                });
                let events = rx.await.unwrap_or_else(|_| {
                    vec![ServerMessage::Error {
                        code: "agent_failed".into(),
                        message: "Agent 处理线程已终止".into(),
                    }]
                });
                for event in events {
                    let _ = socket
                        .send(Message::Text(serde_json::to_string(&event).unwrap().into()))
                        .await;
                }
            }
            Err(error) => {
                let event = ServerMessage::Error {
                    code: "invalid_message".into(),
                    message: error.to_string(),
                };
                let _ = socket
                    .send(Message::Text(serde_json::to_string(&event).unwrap().into()))
                    .await;
            }
        }
    }
}

pub async fn serve_loopback(engine: Engine, port: u16) -> anyhow::Result<()> {
    serve_loopback_with_safe_mode(engine, port, false).await
}

pub async fn serve_loopback_with_safe_mode(
    engine: Engine,
    port: u16,
    safe_web_mode: bool,
) -> anyhow::Result<()> {
    serve_loopback_with_assets_and_safe_mode(engine, port, safe_web_mode, None).await
}

pub async fn serve_loopback_with_assets_and_safe_mode(
    engine: Engine,
    port: u16,
    safe_web_mode: bool,
    assets_dir: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    let token = std::env::var("CHAOS_WEB_TOKEN").unwrap_or_default();
    // Naming a public origin means the server is expected to answer requests
    // addressed to that name through a proxy, so it must not be anonymous and
    // the value must be well-formed rather than silently ignored.
    if let Some(declared) = std::env::var("CHAOS_WEB_PUBLIC_ORIGIN")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        let authority = public_origin_authority(&declared)
            .map_err(|reason| anyhow::anyhow!("CHAOS_WEB_PUBLIC_ORIGIN 无效：{reason}"))?;
        if token.is_empty() {
            anyhow::bail!(
                "CHAOS_WEB_PUBLIC_ORIGIN={authority} 让服务对 loopback 之外的地址生效，必须同时设置 CHAOS_WEB_TOKEN"
            );
        }
        eprintln!(
            "accepting Host and Origin for {authority}; the proxy in front of this server must set X-Forwarded-Proto: https"
        );
    }
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).await?;
    axum::serve(
        listener,
        router_with_assets_and_safe_mode(engine, token, safe_web_mode, assets_dir),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use std::{fs, io::Write};
    use tower::ServiceExt;
    #[tokio::test]
    async fn protected_handshake_rejects_without_bearer() {
        let response = router(Engine::new(), "secret")
            .oneshot(Request::get("/api/handshake").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    #[tokio::test]
    async fn protected_handshake_accepts_bearer_and_secure_headers() {
        let response = router(Engine::new(), "secret")
            .oneshot(
                Request::get("/api/handshake")
                    .header(header::HOST, "127.0.0.1")
                    .header("authorization", "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response
                .headers()
                .contains_key(header::CONTENT_SECURITY_POLICY)
        );
    }
    /// Every refusal names the rule that fired, because the three of them look
    /// identical to a user ("the page never connects") and only one of them is
    /// fixed by editing the proxy.
    async fn refusal_code(app: axum::Router, request: axum::extract::Request) -> String {
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json"),
            "the reason has to be machine-readable"
        );
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        serde_json::from_slice::<serde_json::Value>(&body)
            .expect("refusal body")
            .get("error")
            .and_then(serde_json::Value::as_str)
            .expect("error field")
            .to_owned()
    }

    #[tokio::test]
    async fn refusals_name_the_rule_that_blocked_the_request() {
        // These four cases are about an operator who declared nothing. Rather
        // than silently passing under an environment that did declare a public
        // origin, say which assumption is broken.
        assert_eq!(
            configured_public_authority(),
            None,
            "CHAOS_WEB_PUBLIC_ORIGIN is set in this environment; unset it to run this test"
        );

        let undeclared_host = refusal_code(
            router(Engine::new(), "secret"),
            Request::get("/api/handshake")
                .header(header::HOST, "chaos.example.test:8443")
                .header("authorization", "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(undeclared_host, "host_not_allowed");

        let no_credential = refusal_code(
            router(Engine::new(), "secret"),
            Request::get("/api/handshake")
                .header(header::HOST, "127.0.0.1")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(no_credential, "credential_required");

        let foreign_page = refusal_code(
            router(Engine::new(), "secret"),
            Request::get("/api/handshake")
                .header(header::HOST, "127.0.0.1")
                .header(header::ORIGIN, "http://evil.example")
                .header("authorization", "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(foreign_page, "origin_not_allowed");

        // The order matters for diagnosis: a request that fails the Host rule is
        // not also reported as lacking a credential, or the operator fixes the
        // wrong thing.
        let both = refusal_code(
            router(Engine::new(), "secret"),
            Request::post("/api/sessions")
                .header(header::HOST, "elsewhere.test")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(both, "host_not_allowed");
    }

    #[test]
    fn dynamic_development_origin_is_debug_only_explicit_loopback_and_origin_only() {
        assert_eq!(
            approved_dynamic_dev_origin(Some("http://127.0.0.1:43987"), true, true),
            Some("http://127.0.0.1:43987".into())
        );
        assert_eq!(
            approved_dynamic_dev_origin(Some("http://127.0.0.1"), true, true),
            None,
            "a browser-default port without an explicit bound test port is not approved"
        );
        assert_eq!(
            approved_dynamic_dev_origin(Some("http://127.0.0.1:0"), true, true),
            None,
            "port zero does not identify the running listener"
        );
        for origin in [
            "https://127.0.0.1:43987",
            "http://evil.example:43987",
            "http://127.0.0.1:43987/path",
            "http://user@127.0.0.1:43987",
            "http://127.0.0.1:99999",
        ] {
            assert_eq!(
                approved_dynamic_dev_origin(Some(origin), true, true),
                None,
                "accepted {origin}"
            );
        }
        assert_eq!(
            approved_dynamic_dev_origin(Some("http://127.0.0.1:43987"), false, true),
            None
        );
        assert_eq!(
            approved_dynamic_dev_origin(Some("http://127.0.0.1:43987"), true, false),
            None
        );
    }

    #[tokio::test]
    async fn origin_is_checked() {
        let response = router(Engine::new(), "")
            .oneshot(
                Request::get("/api/handshake")
                    .header(header::HOST, "127.0.0.1")
                    .header("origin", "https://evil.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    #[tokio::test]
    async fn host_is_checked() {
        let response = router(Engine::new(), "")
            .oneshot(
                Request::get("/api/handshake")
                    .header(header::HOST, "evil.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn host_parser_rejects_malformed_or_credentialed_authorities() {
        for host in [
            "localhost:invalid",
            "user@localhost",
            "127.0.0.1:99999",
            "localhost:",
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(header::HOST, HeaderValue::from_str(host).unwrap());
            assert!(!host_allowed(&headers), "accepted Host header: {host}");
        }
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("LOCALHOST:3000"));
        assert!(host_allowed(&headers));
    }

    /// A proxy that forwards the public `Host` is only believed when the
    /// operator named that host; nothing changes when the setting is absent.
    #[test]
    fn a_declared_public_host_is_accepted_and_everything_else_still_is_not() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::HOST,
            HeaderValue::from_static("chaos.example.com:8443"),
        );
        assert!(
            !host_allowed_against(&headers, None),
            "an undeclared public Host must stay refused"
        );
        assert!(host_allowed_against(
            &headers,
            Some("chaos.example.com:8443")
        ));
        assert!(
            !host_allowed_against(&headers, Some("other.example.com")),
            "declaring one host must not accept another"
        );
        // Loopback keeps working, because the proxy dials it and health checks
        // arrive there directly.
        headers.insert(header::HOST, HeaderValue::from_static("127.0.0.1:8787"));
        assert!(host_allowed_against(
            &headers,
            Some("chaos.example.com:8443")
        ));
    }

    /// The browser's `Origin` on an HTTPS page can never match the `http` Host
    /// the backend sees, so the declared origin is what makes it same-origin.
    #[test]
    fn an_https_origin_is_accepted_only_for_the_declared_host_over_tls() {
        let request = |origin: &str, proto: Option<&str>| {
            let mut headers = HeaderMap::new();
            headers.insert(header::ORIGIN, HeaderValue::from_str(origin).unwrap());
            headers.insert(header::HOST, HeaderValue::from_static("127.0.0.1:8787"));
            if let Some(proto) = proto {
                headers.insert("x-forwarded-proto", HeaderValue::from_str(proto).unwrap());
            }
            headers
        };
        let declared = Some("chaos.example.com:8443");

        assert!(
            !origin_allowed_against(&request("https://chaos.example.com:8443", None), declared),
            "an https origin without a TLS claim from the proxy must be refused"
        );
        assert!(origin_allowed_against(
            &request("https://chaos.example.com:8443", Some("https")),
            declared
        ));
        assert!(origin_allowed_against(
            &request("HTTPS://Chaos.Example.com:8443", Some("HTTPS, http")),
            declared
        ));
        assert!(
            !origin_allowed_against(&request("https://evil.example", Some("https")), declared),
            "any other https page must stay refused"
        );
        assert!(
            !origin_allowed_against(
                &request("https://chaos.example.com:8443", Some("http")),
                declared
            ),
            "the proxy must report TLS"
        );
        assert!(
            !origin_allowed_against(
                &request("https://chaos.example.com:8443", Some("https")),
                None,
            ),
            "with nothing declared, an https origin has no matching deployment"
        );
        // The plaintext same-origin rule is untouched.
        let mut http = request("http://127.0.0.1:8787", None);
        http.insert(header::HOST, HeaderValue::from_static("127.0.0.1:8787"));
        assert!(origin_allowed_against(&http, None));
    }

    /// A typo in the one setting that widens the accept list has to be loud.
    #[test]
    fn a_public_origin_is_only_accepted_as_a_bare_https_origin() {
        assert_eq!(
            public_origin_authority("https://chaos.example.com:8443/").as_deref(),
            Ok("chaos.example.com:8443")
        );
        for rejected in [
            "http://chaos.example.com",
            "chaos.example.com",
            "https://user@chaos.example.com",
            "https://chaos.example.com/app",
            "https://chaos.example.com?token=abc",
            "https://chaos.example.com#frag",
            "https://chaos.example.com:99999",
            "https://chaos.example.com:",
            "https://",
            "",
        ] {
            assert!(
                public_origin_authority(rejected).is_err(),
                "accepted public origin: {rejected:?}"
            );
        }
    }

    #[tokio::test]
    async fn protected_routes_reject_missing_host() {
        let response = router(Engine::new(), "")
            .oneshot(Request::get("/api/handshake").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let response = router(Engine::new(), "")
            .oneshot(Request::post("/api/sessions").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn session_creation_is_idempotent_for_a_valid_request_id() {
        let engine = Engine::new();
        let app = router(engine.clone(), "");
        let make_request = || {
            Request::post("/api/sessions")
                .header(header::HOST, "127.0.0.1")
                .header(REQUEST_ID, "retry-1")
                .body(Body::empty())
                .unwrap()
        };

        let first = app.clone().oneshot(make_request()).await.unwrap();
        let second = app.oneshot(make_request()).await.unwrap();
        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(second.status(), StatusCode::OK);
        let first: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(first.into_body(), MAX_REQUEST_BYTES)
                .await
                .unwrap(),
        )
        .unwrap();
        let second: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(second.into_body(), MAX_REQUEST_BYTES)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(matches!(
            first.as_array().and_then(|events| events.first()),
            Some(serde_json::Value::Object(event))
                if event.get("type").and_then(serde_json::Value::as_str) == Some("session_created")
        ));
        assert!(matches!(
            second.as_array().and_then(|events| events.first()),
            Some(serde_json::Value::Object(event))
                if event.get("type").and_then(serde_json::Value::as_str) == Some("ack")
        ));
        let listed = engine.handle(ClientMessage::ListWorkspaces {
            client_msg_id: "list-workspaces".into(),
        });
        assert!(matches!(
            &listed[0],
            ServerMessage::Workspaces { workspaces, .. } if workspaces.len() == 1
        ));
    }

    #[tokio::test]
    async fn invalid_session_request_id_falls_back_to_a_fresh_id() {
        let engine = Engine::new();
        let app = router(engine.clone(), "");
        for request_id in ["", &"x".repeat(129)] {
            let response = app
                .clone()
                .oneshot(
                    Request::post("/api/sessions")
                        .header(header::HOST, "127.0.0.1")
                        .header(REQUEST_ID, request_id)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
        let listed = engine.handle(ClientMessage::ListWorkspaces {
            client_msg_id: "list-workspaces".into(),
        });
        assert!(matches!(
            &listed[0],
            ServerMessage::Workspaces { workspaces, .. } if workspaces.len() == 1
        ));
    }

    #[test]
    fn parses_accept_encoding_quality_values_and_case() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT_ENCODING,
            HeaderValue::from_static("gzip;q=0.8, br;Q=0.4"),
        );
        assert_eq!(accepted_gzip_quality(&headers), 800);
        assert_eq!(accepted_encoding_quality(&headers, &["br"]), 400);
        assert_eq!(accepted_encoding_quality(&headers, &["deflate"]), 0);
        headers.insert(
            header::ACCEPT_ENCODING,
            HeaderValue::from_static("br;q=0, gzip;q=0.125"),
        );
        assert_eq!(accepted_encoding_quality(&headers, &["br"]), 0);
        assert_eq!(accepted_gzip_quality(&headers), 125);
        headers.insert(
            header::ACCEPT_ENCODING,
            HeaderValue::from_static("x-gzip;q=0.7"),
        );
        assert_eq!(accepted_gzip_quality(&headers), 700);
    }

    #[tokio::test]
    async fn static_assets_fallback_to_spa_without_shadowing_api_or_health_routes() {
        let assets = tempfile::tempdir().unwrap();
        fs::write(
            assets.path().join("index.html"),
            "<main>Chaos built UI</main>",
        )
        .unwrap();
        fs::create_dir(assets.path().join("assets")).unwrap();
        fs::write(
            assets.path().join("assets/app.js"),
            "globalThis.chaosReady=true;",
        )
        .unwrap();
        let mut gzip = flate2::write::GzEncoder::new(
            fs::File::create(assets.path().join("assets/app.js.gz")).unwrap(),
            flate2::Compression::default(),
        );
        gzip.write_all(b"globalThis.chaosReady=true;").unwrap();
        gzip.finish().unwrap();
        let app =
            router_with_assets_and_safe_mode(Engine::new(), "", false, Some(assets.path().into()));

        for (path, expected_type, expected_body) in [
            ("/", "text/html", "<main>Chaos built UI</main>"),
            (
                "/assets/app.js",
                "text/javascript",
                "globalThis.chaosReady=true;",
            ),
            (
                "/conversations/session-1",
                "text/html",
                "<main>Chaos built UI</main>",
            ),
        ] {
            let response = app
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            if path.starts_with("/assets/") {
                assert_eq!(
                    response.headers().get(header::CACHE_CONTROL).unwrap(),
                    "public, no-cache"
                );
            } else {
                assert_eq!(
                    response.headers().get(header::CACHE_CONTROL).unwrap(),
                    "no-cache"
                );
            }
            assert_eq!(
                response
                    .headers()
                    .get(header::CONTENT_TYPE)
                    .unwrap()
                    .to_str()
                    .unwrap(),
                expected_type,
                "{path}"
            );
            let body = axum::body::to_bytes(response.into_body(), 1024)
                .await
                .unwrap();
            assert_eq!(body.as_ref(), expected_body.as_bytes(), "{path}");
        }

        let initial_asset = app
            .clone()
            .oneshot(Request::get("/assets/app.js").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let asset_etag = initial_asset.headers().get(header::ETAG).unwrap().clone();
        let initial_body = axum::body::to_bytes(initial_asset.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(initial_body.as_ref(), b"globalThis.chaosReady=true;");

        let head_response = app
            .clone()
            .oneshot(Request::head("/assets/app.js").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(head_response.status(), StatusCode::OK);
        assert_eq!(
            head_response.headers().get(header::ETAG).unwrap(),
            &asset_etag
        );

        let non_read_response = app
            .clone()
            .oneshot(
                Request::post("/assets/app.js")
                    .header(header::IF_NONE_MATCH, &asset_etag)
                    .body(Body::from("unsafe mutation"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(non_read_response.status(), StatusCode::METHOD_NOT_ALLOWED);

        let not_modified = app
            .clone()
            .oneshot(
                Request::get("/assets/app.js")
                    .header(header::IF_NONE_MATCH, &asset_etag)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(not_modified.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(
            not_modified.headers().get(header::ETAG).unwrap(),
            &asset_etag
        );
        assert_eq!(
            axum::body::to_bytes(not_modified.into_body(), 1024)
                .await
                .unwrap()
                .len(),
            0
        );

        fs::write(
            assets.path().join("assets/app.js"),
            "globalThis.chaosReady=false;",
        )
        .unwrap();
        let changed_asset = app
            .clone()
            .oneshot(
                Request::get("/assets/app.js")
                    .header(header::IF_NONE_MATCH, &asset_etag)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(changed_asset.status(), StatusCode::OK);
        assert_ne!(
            changed_asset.headers().get(header::ETAG).unwrap(),
            &asset_etag
        );
        assert_eq!(
            axum::body::to_bytes(changed_asset.into_body(), 1024)
                .await
                .unwrap()
                .as_ref(),
            b"globalThis.chaosReady=false;"
        );

        let gzip_asset_bytes = fs::read(assets.path().join("assets/app.js.gz")).unwrap();
        let gzip_asset_etag = format!(
            "\"{}.gz\"",
            Sha256::digest(&gzip_asset_bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        let x_gzip_response = app
            .clone()
            .oneshot(
                Request::get("/assets/app.js")
                    .header(header::ACCEPT_ENCODING, "x-gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(x_gzip_response.status(), StatusCode::OK);
        assert_eq!(
            x_gzip_response
                .headers()
                .get(header::CONTENT_ENCODING)
                .unwrap(),
            "gzip"
        );
        assert_eq!(
            x_gzip_response.headers().get(header::ETAG).unwrap(),
            &gzip_asset_etag
        );

        let compressed_etag = app
            .clone()
            .oneshot(
                Request::get("/assets/app.js")
                    .header(header::ACCEPT_ENCODING, "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .headers()
            .get(header::ETAG)
            .unwrap()
            .clone();
        let compressed_conditional = app
            .clone()
            .oneshot(
                Request::get("/assets/app.js")
                    .header(header::ACCEPT_ENCODING, "gzip")
                    .header(header::IF_NONE_MATCH, &compressed_etag)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(compressed_conditional.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(
            compressed_conditional.headers().get(header::ETAG).unwrap(),
            &compressed_etag
        );

        let compressed = app
            .clone()
            .oneshot(
                Request::get("/assets/app.js")
                    .header(header::ACCEPT_ENCODING, "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(compressed.status(), StatusCode::OK);
        assert_eq!(
            compressed.headers().get(header::CONTENT_ENCODING).unwrap(),
            "gzip"
        );
        assert_eq!(
            compressed.headers().get(header::VARY).unwrap(),
            "Accept-Encoding"
        );
        let compressed_body = axum::body::to_bytes(compressed.into_body(), 1024)
            .await
            .unwrap();
        let mut decoder = flate2::read::GzDecoder::new(compressed_body.as_ref());
        let mut decoded = String::new();
        std::io::Read::read_to_string(&mut decoder, &mut decoded).unwrap();
        assert_eq!(decoded, "globalThis.chaosReady=true;");

        let index = app
            .clone()
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(index.status(), StatusCode::OK);
        assert!(index.headers().get(header::ETAG).is_none());
        assert_eq!(
            index.headers().get(header::VARY).unwrap(),
            "Accept-Encoding"
        );
        assert_eq!(
            index.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-cache"
        );

        let missing_asset = app
            .clone()
            .oneshot(
                Request::get("/assets/missing.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_asset.status(), StatusCode::NOT_FOUND);
        let api = app
            .clone()
            .oneshot(
                Request::get("/api/handshake")
                    .header(header::HOST, "127.0.0.1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(api.status(), StatusCode::OK);
        let missing_api = app
            .clone()
            .oneshot(
                Request::get("/api/not-a-route")
                    .header(header::HOST, "127.0.0.1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_api.status(), StatusCode::NOT_FOUND);
        let health = app
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);
        let health_body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(health.into_body(), 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(health_body["status"], "ok");
    }

    #[tokio::test]
    async fn health_is_public() {
        let response = router(Engine::new(), "secret")
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_SECURITY_POLICY)
                .unwrap(),
            "default-src 'self'; frame-ancestors 'none'"
        );
        assert_eq!(
            response
                .headers()
                .get(header::X_CONTENT_TYPE_OPTIONS)
                .unwrap(),
            "nosniff"
        );
    }
}
