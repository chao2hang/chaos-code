use axum::{
    Json, Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use chaos_engine::{ClientMessage, Engine, PROTOCOL_VERSION, ServerMessage};
use sha2::{Digest, Sha256};
use std::{
    net::SocketAddr,
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tower_http::services::ServeDir;
use tower_http::{cors::CorsLayer, limit::RequestBodyLimitLayer};

pub mod chaos_bridge;
pub mod preview;
pub mod remote_relay;
pub mod web_auth;

use preview::PreviewConfig;

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
    pub auth: web_auth::WebAuth,
    pub safe_web_mode: bool,
    pub file_root: std::path::PathBuf,
    pub assets_dir: Option<std::path::PathBuf>,
    /// Dev-server ports an operator named at startup. Empty means `/preview`
    /// answers with a reason code and dials nothing.
    pub previews: PreviewConfig,
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
    router_with_previews(
        engine,
        token,
        safe_web_mode,
        assets_dir,
        PreviewConfig::disabled(),
    )
}

/// The full constructor. `previews` is a parameter rather than an environment
/// read so that a test can enable a port without touching process state.
pub fn router_with_previews(
    engine: Engine,
    token: impl Into<String>,
    safe_web_mode: bool,
    assets_dir: Option<std::path::PathBuf>,
    previews: PreviewConfig,
) -> Router {
    router_with_auth_and_previews(
        engine,
        token,
        safe_web_mode,
        assets_dir,
        previews,
        web_auth::WebAuth::disabled(),
    )
}

/// Production router with browser password authentication. The ordinary router
/// constructors stay unauthenticated for in-process protocol tests; the shipped
/// binary always calls this constructor with persistent auth enabled.
pub fn router_with_auth_and_previews(
    engine: Engine,
    token: impl Into<String>,
    safe_web_mode: bool,
    assets_dir: Option<std::path::PathBuf>,
    previews: PreviewConfig,
    auth: web_auth::WebAuth,
) -> Router {
    router_with_auth_and_previews_and_file_root(
        engine,
        token,
        safe_web_mode,
        assets_dir,
        previews,
        auth,
        None,
    )
}

pub fn router_with_auth_and_previews_and_file_root(
    engine: Engine,
    token: impl Into<String>,
    safe_web_mode: bool,
    assets_dir: Option<std::path::PathBuf>,
    previews: PreviewConfig,
    auth: web_auth::WebAuth,
    file_root: Option<PathBuf>,
) -> Router {
    let state = Arc::new(WebState {
        engine,
        token: Arc::new(token.into()),
        auth: auth.with_file_root(file_root.clone()),
        safe_web_mode,
        file_root: file_root.unwrap_or_else(chaos_bridge::file_root),
        assets_dir: assets_dir.clone(),
        previews,
    });
    let router = Router::new()
        .route("/health", get(health))
        .route("/api/auth/status", get(web_auth::status))
        .route("/api/auth/setup", post(web_auth::setup))
        .route("/api/auth/login", post(web_auth::login))
        .route("/api/auth/logout", post(web_auth::logout))
        .route("/api/handshake", get(handshake))
        .route("/api/server-info", get(server_info))
        .route("/api/sessions", post(create_session))
        .route("/", get(index_page))
        .route("/remote", get(remote_page))
        .route("/remote/v4", get(remote_spa_page))
        .route("/remote/v4/", get(remote_spa_page))
        .route("/ws", get(websocket))
        .route("/ws/zcode", get(zcode_websocket))
        .layer(
            CorsLayer::new().allow_origin(
                dev_origins()
                    .iter()
                    .filter_map(|origin| origin.parse::<HeaderValue>().ok())
                    .collect::<Vec<HeaderValue>>(),
            ),
        )
        .layer(RequestBodyLimitLayer::new(MAX_REQUEST_BYTES))
        // Merged after the layers above on purpose. `RequestBodyLimitLayer`
        // caps this server's own JSON API at 64 KiB; a previewed app POSTing a
        // form or uploading a file is not that API, and the preview module sets
        // its own documented bound.
        .merge(preview::routes())
        .fallback(spa_fallback)
        .with_state(state.clone());
    let router = if let Some(assets_dir) = assets_dir {
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
            // The official mobile SPA mirror: `<assets_dir>/remote-spa/latest/`
            // holds the captured `/remote/v4` client (page shell + assets), and
            // `remote-spa/latest/assets/index.js` is the one-line-patched copy
            // whose relay endpoint points at this same origin instead of the
            // cloud relay. Served at the exact paths the client's own router
            // matches (`/remote/v4`, `/remote/v4/latest/`, `…/assets/*`).
            .nest_service(
                "/remote/v4/latest/assets",
                tower::ServiceBuilder::new()
                    .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
                        header::CACHE_CONTROL,
                        HeaderValue::from_static("public, max-age=300"),
                    ))
                    .service(
                        ServeDir::new(assets_dir.join("remote-spa/latest/assets"))
                            .precompressed_gzip()
                            .precompressed_br(),
                    ),
            )
    } else {
        router
    };
    router.layer(axum::middleware::from_fn_with_state(state, web_auth::gate))
}

async fn health() -> Response {
    secure_json(serde_json::json!({ "status": "ok" }))
}

/// The official mobile SPA's page shell, served from the mirror directory.
///
/// The shell is the official one (theme pre-paint bootstrap + one module
/// script). When the mirror is absent the route degrades to the bundled mobile
/// page instead of failing.
async fn remote_spa_page(State(state): State<Arc<WebState>>) -> Response {
    let path = state
        .assets_dir
        .as_ref()
        .map(|dir| dir.join("remote-spa/latest/index.html"))
        .or_else(|| assets_root().map(|dir| dir.join("remote-spa/latest/index.html")));
    let html = match path.as_deref().and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(html) => (html, "no-cache"),
        None => (
            include_str!("remote.html").to_string(),
            "no-store",
        ),
    };
    let (body, cache) = html;
    let mut response = Html(body).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    response
}

fn assets_root() -> Option<std::path::PathBuf> {
    std::env::var("CHAOS_WEB_ASSETS_DIR")
        .ok()
        .map(std::path::PathBuf::from)
}

pub fn has_official_mobile_client(assets_dir: &Option<std::path::PathBuf>) -> bool {
    let in_assets = assets_dir
        .as_ref()
        .map(|dir| dir.join("remote-spa/latest/index.html").is_file())
        .unwrap_or(false);
    let in_env = assets_root()
        .map(|dir| dir.join("remote-spa/latest/index.html").is_file())
        .unwrap_or(false);
    in_assets || in_env
}

pub fn mint_mobile_pairing_url() -> String {
    use base64::Engine;
    use ring::rand::SecureRandom;

    let uuid = uuid::Uuid::new_v4().simple().to_string();
    let sid = format!("d_{}", &uuid[..22]);
    let mut rng_bytes = [0u8; 32];
    let rng = ring::rand::SystemRandom::new();
    let _ = rng.fill(&mut rng_bytes);
    let hash = base64::engine::general_purpose::STANDARD.encode(rng_bytes);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let mid = uuid::Uuid::new_v4().to_string();
    format!("/remote/v4?sid={sid}&hash={hash}&t={t}&mid={mid}&name=phone&app_version=3.15.3")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiMode {
    Mobile,
    Desktop,
}

pub fn detect_ui_mode(
    uri: &axum::http::Uri,
    headers: &HeaderMap,
) -> (UiMode, Option<&'static str>) {
    // 1. Explicit query parameter ?ui=... or ?mode=...
    if let Some(query) = uri.query() {
        for param in query.split('&') {
            if let Some((k, v)) = param.split_once('=') {
                if k == "ui" || k == "mode" || k == "view" {
                    if v.eq_ignore_ascii_case("mobile") || v.eq_ignore_ascii_case("phone") {
                        return (
                            UiMode::Mobile,
                            Some("chaos_ui=mobile; Path=/; Max-Age=31536000; SameSite=Lax"),
                        );
                    }
                    if v.eq_ignore_ascii_case("desktop") || v.eq_ignore_ascii_case("pc") {
                        return (
                            UiMode::Desktop,
                            Some("chaos_ui=desktop; Path=/; Max-Age=31536000; SameSite=Lax"),
                        );
                    }
                    if v.eq_ignore_ascii_case("auto") {
                        return (
                            detect_ui_mode_from_device(headers),
                            Some("chaos_ui=; Path=/; Max-Age=0; SameSite=Lax"),
                        );
                    }
                }
            }
        }
    }

    // 2. Cookie preference: chaos_ui=mobile | desktop
    if let Some(cookie_header) = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()) {
        for cookie in cookie_header.split(';') {
            let cookie = cookie.trim();
            if let Some((k, v)) = cookie.split_once('=') {
                if k.trim() == "chaos_ui" {
                    let v = v.trim();
                    if v.eq_ignore_ascii_case("mobile") || v.eq_ignore_ascii_case("phone") {
                        return (UiMode::Mobile, None);
                    }
                    if v.eq_ignore_ascii_case("desktop") || v.eq_ignore_ascii_case("pc") {
                        return (UiMode::Desktop, None);
                    }
                }
            }
        }
    }

    // 3. Automatic device detection
    (detect_ui_mode_from_device(headers), None)
}

fn detect_ui_mode_from_device(headers: &HeaderMap) -> UiMode {
    if let Some(ch_mobile) = headers.get("sec-ch-ua-mobile").and_then(|v| v.to_str().ok()) {
        if ch_mobile.contains("?1") {
            return UiMode::Mobile;
        } else if ch_mobile.contains("?0") {
            return UiMode::Desktop;
        }
    }

    if let Some(ua) = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()) {
        let ua_lower = ua.to_ascii_lowercase();
        if ua_lower.contains("mobile")
            || ua_lower.contains("android")
            || ua_lower.contains("iphone")
            || ua_lower.contains("ipad")
            || ua_lower.contains("ipod")
            || ua_lower.contains("windows phone")
            || ua_lower.contains("iemobile")
            || ua_lower.contains("blackberry")
            || ua_lower.contains("opera mini")
            || ua_lower.contains("opera mobi")
            || ua_lower.contains("harmonyos")
        {
            return UiMode::Mobile;
        }
    }

    UiMode::Desktop
}

pub fn remote_page_response() -> Response {
    let mut response = Html(include_str!("remote.html")).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'; form-action 'self'"));
    response
        .headers_mut()
        .insert(header::VARY, HeaderValue::from_static("User-Agent, Sec-CH-UA-Mobile, Cookie, Accept-Encoding"));
    response
}

pub fn desktop_page_response(assets_dir: &Option<std::path::PathBuf>) -> Response {
    let path = assets_dir.as_ref().map(|dir| dir.join("index.html"));
    if let Some(content) = path.and_then(|p| std::fs::read_to_string(p).ok()) {
        let mut response = Html(content).into_response();
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html"));
        response
            .headers_mut()
            .insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));
        response
    } else {
        remote_page_response()
    }
}

/// Dynamic index page handler: serves the mobile UI or desktop UI based on
/// detected device type, cookie preference, or ?ui= query parameter override.
/// Dynamic index page handler: serves the unified responsive workspace.
/// Mobile and desktop browsers both stay on `/` with no URL redirection.
/// The layout automatically adapts responsively via CSS and viewport breakpoints.
async fn index_page(
    State(state): State<Arc<WebState>>,
    uri: axum::http::Uri,
    headers: HeaderMap,
) -> Response {
    let (_mode, cookie_update) = detect_ui_mode(&uri, &headers);
    let mut resp = if uri.query().is_some_and(|q| q.contains("ui=legacy-remote")) {
        remote_page_response()
    } else {
        desktop_page_response(&state.assets_dir)
    };
    if let Some(cookie) = cookie_update {
        if let Ok(val) = HeaderValue::from_str(cookie) {
            resp.headers_mut().insert(header::SET_COOKIE, val);
        }
    }
    resp
}

/// The `/remote` route: preserved for backward compatibility and bookmarks.
async fn remote_page(
    State(_state): State<Arc<WebState>>,
    uri: axum::http::Uri,
    _headers: HeaderMap,
) -> Response {
    if let Some(query) = uri.query() {
        if query.contains("ui=desktop") || query.contains("ui=pc") {
            let mut resp = axum::response::Redirect::to("/?ui=desktop").into_response();
            let _ = HeaderValue::from_str("chaos_ui=desktop; Path=/; Max-Age=31536000; SameSite=Lax")
                .map(|val| resp.headers_mut().insert(header::SET_COOKIE, val));
            return resp;
        }
    }
    let mut resp = remote_page_response();
    if let Some(query) = uri.query() {
        if query.contains("ui=mobile") {
            let _ = HeaderValue::from_str("chaos_ui=mobile; Path=/; Max-Age=31536000; SameSite=Lax")
                .map(|val| resp.headers_mut().insert(header::SET_COOKIE, val));
        }
    }
    resp
}

/// Fallback for SPA routing: HTML navigations without static asset extensions
/// render through index_page, while missing static files 404.
async fn spa_fallback(
    State(state): State<Arc<WebState>>,
    uri: axum::http::Uri,
    headers: HeaderMap,
) -> Response {
    let path = uri.path();
    if let Some(last_seg) = path.split('/').last() {
        if let Some((_, ext)) = last_seg.split_once('.') {
            let ext_lower = ext.to_ascii_lowercase();
            if !matches!(ext_lower.as_str(), "html" | "htm") {
                return StatusCode::NOT_FOUND.into_response();
            }
        }
    }
    index_page(State(state), uri, headers).await
}

async fn server_info(State(state): State<Arc<WebState>>) -> Response {
    // No `workspaceIdentity` here: the official frontend treats ANY workspace
    // entry carrying an identity as a REMOTE target (useWorkspaceServices.tsx
    // resolveWorkspaceServiceIsRemoteTarget), which leaves the local shell in
    // "remote-waiting" (rpcReady=false) and the conversation pane renders null.
    let mut paths = Vec::new();
    if let Ok(path) = std::env::current_dir() {
        paths.push(path.to_string_lossy().into_owned());
    }
    let file_root_str = state.file_root.to_string_lossy().into_owned();
    if !paths.contains(&file_root_str) {
        paths.push(file_root_str);
    }
    // Also include any user-persisted workspace paths from settings
    if let Some(home) = std::env::var_os("HOME") {
        let settings_path = std::path::PathBuf::from(home).join(".chaos/web/zcode_settings.json");
        if let Ok(content) = std::fs::read_to_string(&settings_path)
            && let Ok(val) = serde_json::from_str::<serde_json::Value>(&content)
        {
            if let Some(sessions) = val
                .get("lastWorkspaceSession")
                .and_then(serde_json::Value::as_array)
            {
                for s in sessions {
                    if let Some(p) = s.get("workspacePath").and_then(serde_json::Value::as_str)
                        && !paths.contains(&p.to_string())
                        && std::path::Path::new(p).exists()
                    {
                        paths.push(p.to_string());
                    }
                }
            }
        }
    }
    let workspaces: Vec<serde_json::Value> = paths
        .into_iter()
        .map(|p| {
            let label = std::path::Path::new(&p)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Chaos".to_string());
            serde_json::json!({
                "path": p,
                "label": label,
            })
        })
        .collect();
    secure_json(serde_json::json!({
        "serverId": "chaos-web",
        "version": env!("CARGO_PKG_VERSION"),
        "protocolVersion": 1,
        // 不是写死的 false。绑到 0.0.0.0 之后，客户端得先知道这里到底要不要凭据，
        // 才能把「没带 token」和「服务坏了」区分开——之前它恒说 false，而 /ws 其实在
        // 拿 CHAOS_WEB_TOKEN 挡人。
        "authRequired": !state.token.is_empty() || state.auth.enabled(),
        "workspaces": workspaces,
        "capabilities": { "websocketRpc": true }
    }))
}

fn constant_time_equal(left: &str, right: &str) -> bool {
    let mut diff = left.len() ^ right.len();
    for (a, b) in left.as_bytes().iter().zip(right.as_bytes()) {
        diff |= usize::from(a != b);
    }
    diff == 0
}

fn authorized(state: &WebState, headers: &HeaderMap) -> bool {
    if state.auth.enabled() {
        return state.auth.authenticated(headers);
    }
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
                && (configured_public_authority_matches(public_authority, seen)
                    || loopback_authority(seen))
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
    // valid only because the operator typed it. An explicit CHAOS_WEB_BIND IP is
    // the same kind of operator-declared name: binding 0.0.0.0 already signals
    // the intent to serve that address directly.
    loopback_authority(&authority)
        || configured_public_authority_matches(public_authority, &authority)
        || configured_bind_ip_matches(&authority)
}

/// The IP the operator explicitly bound via CHAOS_WEB_BIND, if any.
fn configured_bind_ip() -> Option<std::net::IpAddr> {
    let declared = std::env::var("CHAOS_WEB_BIND").ok()?;
    declared
        .trim()
        .parse::<std::net::IpAddr>()
        .ok()
        .filter(|ip| !ip.is_loopback())
}

fn configured_bind_ip_matches(authority: &axum::http::uri::Authority) -> bool {
    let Some(bind_ip) = configured_bind_ip() else {
        return false;
    };
    let seen = match authority.host().parse::<std::net::IpAddr>() {
        Ok(seen) => seen,
        Err(_) => return false,
    };
    // A wildcard bind (0.0.0.0 / ::) serves every local address, so any
    // numeric Host is the operator's own declaration.
    bind_ip.is_unspecified() || seen == bind_ip
}

/// The addresses this server considers "the machine itself", as opposed to a name
/// other machines can be pointed at.
pub(crate) fn loopback_authority(authority: &axum::http::uri::Authority) -> bool {
    matches!(
        authority.host().to_ascii_lowercase().as_str(),
        "127.0.0.1" | "localhost" | "[::1]" | "::1"
    )
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

async fn zcode_websocket(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if let Some(rejection) = reject(&state, &headers) {
        return rejection;
    }
    let engine = state.engine.clone();
    let file_root = state.file_root.clone();
    upgrade.on_upgrade(move |socket| chaos_bridge::chaos_direct_session(socket, engine, file_root))
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
    let file_root = state.file_root.clone();

    // If the request Referer is from the mobile remote client (/remote/v4),
    // directly upgrade to the relay session without waiting or sniffing.
    let is_remote_client = headers
        .get(header::REFERER)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|r| r.contains("/remote/v4"));
    if is_remote_client {
        return upgrade.on_upgrade(move |socket| {
            remote_relay::relay_session(socket, None, engine, file_root)
        });
    }

    upgrade.on_upgrade(move |socket| sniffer_session(socket, engine, safe_web_mode, file_root))
}

/// One upgraded socket, three protocols.
///
/// The official ZCode frontend speaks its binary `@zcode/rpc` on this same
/// `/ws` path; the Chaos JSON protocol opens with a server-sent text
/// `Handshake` — and a JSON client waits for that greeting before sending,
/// so the protocol cannot be picked by waiting for the client's first frame
/// alone. Both sides therefore race: the client's first frame is awaited for a
/// short grace window. A ZCode client (and a JSON client that sends first)
/// answer within it; a JSON client that waits gets its `Handshake` when the
/// window closes. Either way the JSON session sends the `Handshake` before any
/// frame it processes, so clients see the greeting they always did.
///
/// The third protocol is the mobile remote-control relay: the official phone
/// SPA (served at `/remote/v4`) dials the same `/ws` and opens with a JSON
/// text frame `{"type":"auth_init",…}` (captured wire shape), which neither
/// binary RPC nor a waiting JSON client ever produces.
const PROTOCOL_SNIFF_GRACE: Duration = Duration::from_millis(2000);

async fn sniffer_session(
    socket: WebSocket,
    engine: Engine,
    safe_web_mode: bool,
    file_root: PathBuf,
) {
    let mut socket = socket;
    let first = match tokio::time::timeout(PROTOCOL_SNIFF_GRACE, socket.recv()).await {
        Ok(Some(Ok(first))) => Some(first),
        Ok(Some(Err(_))) | Ok(None) => return,
        Err(_grace_elapsed) => None,
    };
    if let Some(Message::Text(text)) = &first {
        if serde_json::from_str::<serde_json::Value>(text)
            .ok()
            .filter(|v| v.get("type").and_then(serde_json::Value::as_str) == Some("auth_init"))
            .is_some()
        {
            remote_relay::relay_session(socket, first, engine, file_root).await;
            return;
        }
    }
    match first {
        Some(first) if chaos_bridge::sniffs_as_chaos_rpc(&first) => {
            chaos_bridge::chaos_session(socket, first, file_root).await;
        }
        first => {
            json_protocol_session(socket, engine, safe_web_mode, first, file_root).await;
        }
    }
}

/// Whether a Safe Web Mode client may send this message.
///
/// The policy lives beside the protocol enum in `chaos_engine`, next to the list
/// the settings panel renders, because a host that enforced a policy without also
/// reporting it produced a panel that promised nothing while the socket refused
/// messages the panel had never listed. That is `safe_mode_refusals` in the
/// engine, and `the_refusal_list_matches_what_the_socket_actually_refuses` in
/// `tests/host_info_flow.rs` is what keeps the two from drifting.
fn safe_mode_allows(message: &ClientMessage) -> bool {
    chaos_engine::safe_mode_allows(message)
}

/// The gap a host without a provider should leave between the chunks of its
/// stand-in answer, in `CHAOS_DEMO_CHUNK_GAP_MS`.
///
/// A real producer sets its own pace. The stand-in has none, and an answer written
/// out in one go is indistinguishable on screen from a host that cannot stream at
/// all, so a host that is meant to be looked at can ask for one. The value is
/// capped because the wait is repeated once per chunk in front of a reader.
pub fn demo_chunk_gap_from_env() -> Duration {
    let Some(raw) = std::env::var("CHAOS_DEMO_CHUNK_GAP_MS").ok() else {
        return Duration::ZERO;
    };
    match raw.trim().parse::<u64>() {
        Ok(ms) => Duration::from_millis(ms.min(MAX_DEMO_CHUNK_GAP_MS)),
        Err(_) => {
            eprintln!("CHAOS_DEMO_CHUNK_GAP_MS={raw:?} 不是毫秒数，演示应答将不自定节奏");
            Duration::ZERO
        }
    }
}

/// See [`demo_chunk_gap_from_env`].
const MAX_DEMO_CHUNK_GAP_MS: u64 = 1_000;

/// What the socket answers to a message Safe Web Mode withholds.
///
/// The words are what the settings panel promises, and
/// `the_refusal_list_matches_what_the_socket_actually_refuses` in
/// `tests/host_info_flow.rs` fails if the two stop matching.
const SAFE_MODE_REFUSAL: &str = "Safe Web Mode 禁止此操作";

/// How many client messages one connection may have waiting or being processed.
///
/// A prompt hands its answer to the engine's own run thread and is answered at
/// once, but an approved command or a Provider round trip still occupies the
/// connection until it returns. Without a bound a socket that sent frames as fast
/// as it could would queue them without limit, so the refusal is named rather than
/// paid for. The shipped browser never reaches it: it uploads a file in windows of
/// four slices (`UPLOAD_WINDOW` in `apps/chaos-ui/src/attachments.ts`) rather than
/// putting a whole 10 MiB attachment in front of the connection at once.
/// `apps/chaos-ui/src/attachments.test.ts` compares that window with this number,
/// and `tests/streaming_flow.rs` sizes the burst that provokes the refusal from it.
pub const MAX_IN_FLIGHT_PER_CONNECTION: usize = 16;

/// What a connection is told when the thread handling its frames died.
const CONNECTION_LOST: &str = "Agent 处理线程已终止";

/// Run one Chaos-protocol frame through the normal request path: size limit,
/// JSON parse, Safe Web Mode policy, in-flight bound, worker handoff.
///
/// Returns `false` when the session must end (the worker died under this
/// frame). Shared by the select loop and the protocol sniffer, which consumed
/// the session's first frame to pick a protocol and must not lose it.
async fn process_json_frame(
    socket: &mut WebSocket,
    _engine: &Engine,
    safe_web_mode: bool,
    permits: &Arc<tokio::sync::Semaphore>,
    frames_tx: &std::sync::mpsc::Sender<(ClientMessage, tokio::sync::OwnedSemaphorePermit)>,
    frame: Message,
) -> bool {
    match frame {
        Message::Text(text) => {
            if text.len() > MAX_REQUEST_BYTES {
                return refuse(socket, "message_too_large", "消息超过 64 KiB 限制")
                    .await
                    .is_ok();
            }
            let client = match serde_json::from_str::<ClientMessage>(&text) {
                Ok(client) => client,
                Err(error) => {
                    return refuse(socket, "invalid_message", &error.to_string())
                        .await
                        .is_ok();
                }
            };
            if safe_web_mode && !safe_mode_allows(&client) {
                return refuse(socket, "safe_web_mode_blocked", SAFE_MODE_REFUSAL)
                    .await
                    .is_ok();
            }
            let Ok(permit) = permits.clone().try_acquire_owned() else {
                return refuse(
                    socket,
                    "too_many_in_flight",
                    "此连接待处理请求已达上限，请等待已有请求结束",
                )
                .await
                .is_ok();
            };
            if let Err(std::sync::mpsc::SendError((_, permit))) = frames_tx.send((client, permit)) {
                // The worker is gone, so this frame has nobody left to run it.
                drop(permit);
                return refuse(socket, "agent_failed", CONNECTION_LOST)
                    .await
                    .is_ok();
            }
            true
        }
        _ => true,
    }
}

async fn json_protocol_session(
    mut socket: WebSocket,
    engine: Engine,
    safe_web_mode: bool,
    first_client_frame: Option<Message>,
    file_root: PathBuf,
) {
    let _ = socket
        .send(Message::Text(
            serde_json::to_string(&ServerMessage::Handshake {
                protocol_version: PROTOCOL_VERSION,
            })
            .unwrap()
            .into(),
        ))
        .await;
    // Everything the engine produces goes out through this socket, including what
    // another tab or another client caused. That is what makes a second tab follow
    // a turn instead of showing a transcript that quietly stopped updating. The
    // socket's own requests come back over the same channel, which is why the
    // worker below is given no return path: one event, one send.
    let mut events = engine.subscribe();
    let sink = events.sink();
    let permits = Arc::new(tokio::sync::Semaphore::new(MAX_IN_FLIGHT_PER_CONNECTION));
    // Frames from one connection are handled one at a time, in the order they
    // arrived. An attachment whose slices are written out of order is a file whose
    // bytes are a permutation of what was uploaded and nothing on either side can
    // see it, and an approval is only meaningful after the request it approves.
    // Serialising the requests costs nothing a stream needs: the answer goes out
    // over the subscription above, so waiting for one request cannot hold back the
    // events of the request before it.
    let (frames_tx, frames_rx) =
        std::sync::mpsc::channel::<(ClientMessage, tokio::sync::OwnedSemaphorePermit)>();
    // The sniffer already read the first client frame to pick a protocol; feed
    // it through the normal path now that the request plumbing exists, so no
    // byte is dropped. `None` never happens on the sniffer's JSON path.
    if let Some(first) = first_client_frame {
        if let Message::Text(ref text) = first {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
                if v.get("type").and_then(serde_json::Value::as_str) == Some("auth_init") {
                    remote_relay::relay_session(socket, Some(first), engine, file_root).await;
                    return;
                }
            }
        }
        if !process_json_frame(
            &mut socket,
            &engine,
            safe_web_mode,
            &permits,
            &frames_tx,
            first,
        )
        .await
        {
            return;
        }
    }
    // Only the async side can write to the socket, so a frame whose handler died
    // comes back here to be reported rather than left unacknowledged.
    let (notices_tx, mut notices) = tokio::sync::mpsc::unbounded_channel::<ServerMessage>();
    {
        let worker_engine = engine.clone();
        let worker_sink = sink.clone();
        let worker_notices = notices_tx.clone();
        std::thread::spawn(move || {
            while let Ok((client, permit)) = frames_rx.recv() {
                // A poisoned engine lock or a handler that panics would otherwise
                // take this worker quietly with it and leave the connection waiting
                // for an answer that is never coming.
                let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    worker_engine.dispatch_to(client, Some(&worker_sink), &mut |_event| {});
                }));
                if handled.is_err() {
                    let _ = worker_notices.send(ServerMessage::Error {
                        code: "agent_failed".into(),
                        message: CONNECTION_LOST.into(),
                    });
                }
                // The slot is held for the request, not for the worker's start-up.
                drop(permit);
            }
        });
    }
    enum Input {
        Event(ServerMessage),
        Frame(Message),
        Closed,
    }
    loop {
        // Whichever side has something ready first is handled first, and the engine
        // side wins a tie: a chunk that is ready to leave should not queue behind a
        // request that just arrived on the same socket.
        let input = {
            let produced = events.recv();
            let received = socket.recv();
            let notice = notices.recv();
            tokio::select! {
                biased;
                maybe_event = produced => match maybe_event {
                    Some(event) => Input::Event(event),
                    // The engine stopped delivering here: either it is gone or this
                    // connection could not keep up. A transcript with a hole in it is
                    // worse than a closed socket, because the hole is invisible, so the
                    // socket closes and the client resumes from a fresh snapshot.
                    None => Input::Closed,
                },
                maybe_notice = notice => match maybe_notice {
                    // Nothing will ever answer a frame on this connection again,
                    // which is a worse version of the hole above.
                    None => Input::Closed,
                    Some(message) => Input::Event(message),
                },
                maybe_frame = received => match maybe_frame {
                    Some(Ok(frame)) => Input::Frame(frame),
                    _ => Input::Closed,
                },
            }
        };
        match input {
            Input::Closed => break,
            Input::Event(event) => {
                let text = serde_json::to_string(&event).unwrap().into();
                if socket.send(Message::Text(text)).await.is_err() {
                    break;
                }
            }
            Input::Frame(frame @ Message::Text(_)) => {
                if let Message::Text(ref text) = frame {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
                        if v.get("type").and_then(serde_json::Value::as_str) == Some("auth_init") {
                            remote_relay::relay_session(socket, Some(frame), engine, file_root).await;
                            break;
                        }
                    }
                }
                if !process_json_frame(
                    &mut socket,
                    &engine,
                    safe_web_mode,
                    &permits,
                    &frames_tx,
                    frame,
                )
                .await
                {
                    break;
                }
            }
            Input::Frame(_) => {}
        }
    }
}

/// Answers a connection with the one message saying its frame was not accepted.
async fn refuse(socket: &mut WebSocket, code: &str, message: &str) -> Result<(), axum::Error> {
    socket
        .send(Message::Text(
            serde_json::to_string(&ServerMessage::Error {
                code: code.into(),
                message: message.into(),
            })
            .unwrap()
            .into(),
        ))
        .await
}

pub async fn serve_loopback(engine: Engine, port: u16) -> anyhow::Result<std::net::SocketAddr> {
    serve_loopback_with_safe_mode(engine, port, false).await
}

pub async fn serve_loopback_with_safe_mode(
    engine: Engine,
    port: u16,
    safe_web_mode: bool,
) -> anyhow::Result<std::net::SocketAddr> {
    serve_loopback_with_assets_and_safe_mode(engine, port, safe_web_mode, None).await
}

/// Serves on loopback until the process is stopped, and returns the address it
/// actually bound.
///
/// The address comes back rather than being reconstructed from `port`, because
/// `port` 0 is a legitimate request for any free port and a settings panel that
/// reported `127.0.0.1:0` would be reporting a place nobody can reach.
pub async fn serve_loopback_with_assets_and_safe_mode(
    engine: Engine,
    port: u16,
    safe_web_mode: bool,
    assets_dir: Option<std::path::PathBuf>,
) -> anyhow::Result<std::net::SocketAddr> {
    serve_loopback_with_assets_and_safe_mode_and_auth(
        engine,
        port,
        safe_web_mode,
        assets_dir,
        web_auth::WebAuth::disabled(),
    )
    .await
}

pub async fn serve_loopback_with_assets_and_safe_mode_and_auth(
    engine: Engine,
    port: u16,
    safe_web_mode: bool,
    assets_dir: Option<std::path::PathBuf>,
    auth: web_auth::WebAuth,
) -> anyhow::Result<std::net::SocketAddr> {
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
        anyhow::ensure!(
            auth.enabled() || !token.is_empty(),
            "CHAOS_WEB_PUBLIC_ORIGIN requires browser password auth or CHAOS_WEB_TOKEN"
        );
        eprintln!(
            "accepting Host and Origin for {authority}; the proxy in front of this server must set X-Forwarded-Proto: https"
        );
    }
    // Default loopback; operators expose the LAN via CHAOS_WEB_BIND (e.g. 0.0.0.0).
    // A non-loopback bind without CHAOS_WEB_PUBLIC_ORIGIN still works for direct
    // IP access, but pairing it with a token is strongly recommended.
    let bind_host = std::env::var("CHAOS_WEB_BIND").unwrap_or_else(|_| "127.0.0.1".to_owned());
    let bind_addr: SocketAddr = if let Ok(ip) = bind_host.parse() {
        SocketAddr::new(ip, port)
    } else {
        anyhow::bail!("CHAOS_WEB_BIND 无效：{bind_host}（需要 IP 地址，例如 0.0.0.0）");
    };
    if !bind_addr.ip().is_loopback() && !auth.enabled() && token.is_empty() {
        anyhow::bail!("非 loopback 监听必须启用浏览器登录或设置 CHAOS_WEB_TOKEN");
    }
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    let bound = listener.local_addr()?;
    // Reported here rather than by the caller, because only this function knows
    // what the socket actually took: `port` 0 means "any", and an operator
    // told `http://127.0.0.1:0` cannot open it.
    eprintln!("Chaos Web listening on http://{bound}");
    // 浏览器能借 `file.*` 读到哪儿，得在启动这一行就说清：绑到 0.0.0.0 之后它决定的
    // 是局域网能读到什么，等操作者在报错里看到就太晚了。
    let file_root = chaos_bridge::file_root();
    eprintln!(
        "file.* is confined to {}; widen with CHAOS_WEB_FILE_ROOT",
        file_root.display()
    );
    // A preview allowlist that does not parse must stop startup. Half a list
    // would mean the app a user expected to reach through this host is not
    // reachable, and the symptom is a preview that never loads.
    let previews = PreviewConfig::from_env().map_err(|reason| {
        anyhow::anyhow!(
            "预览代理配置无效：{reason}（CHAOS_WEB_PREVIEW_PORTS 为逗号分隔的端口，例如 3000,5173，不设置即关闭预览代理）"
        )
    })?;
    if previews.is_enabled() {
        eprintln!(
            "previewing dev servers{}: {}",
            if previews.allow_public {
                " through the declared public origin as well"
            } else {
                " on loopback only"
            },
            previews
                .ports()
                .iter()
                .map(|port| format!("http://127.0.0.1:{port} -> /preview/{port}/"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    // The settings panel reports what this process decided above, so the facts
    // are collected here rather than re-read from the environment later: a panel
    // that re-read `std::env::var` could disagree with the middleware that was
    // built from the value captured at startup.
    let token_required = !token.is_empty() || auth.enabled();
    let declared_origin = std::env::var("CHAOS_WEB_PUBLIC_ORIGIN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let preview_state = if !previews.is_enabled() {
        chaos_engine::PreviewProxyState::Disabled
    } else if previews.allow_public {
        chaos_engine::PreviewProxyState::AnyOrigin
    } else {
        chaos_engine::PreviewProxyState::NamedOnly
    };
    let preview_ports = previews.ports().to_vec();
    let engine = engine.with_host_info(|info| {
        info.host_version = host_version().to_string();
        info.bind_addr = bound.to_string();
        info.safe_web_mode = safe_web_mode;
        info.token_required = token_required;
        info.public_origin = declared_origin;
        info.preview_proxy = preview_state;
        info.preview_ports = preview_ports;
    });
    axum::serve(
        listener,
        router_with_auth_and_previews_and_file_root(
            engine,
            token,
            safe_web_mode,
            assets_dir,
            previews,
            auth,
            Some(file_root),
        )
        .into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(bound)
}

/// The version the browser is told it is talking to.
///
/// This crate's own, not the engine's: the panel labels the field "the process
/// serving the browser", and that process is `chaos-web`.
#[must_use]
pub fn host_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
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

    #[test]
    fn the_stand_in_pace_comes_from_the_environment_and_is_capped() {
        // Edition 2024 makes these unsafe because another thread may be reading the
        // environment at the same time; this test does not start such a thread.
        unsafe { std::env::remove_var("CHAOS_DEMO_CHUNK_GAP_MS") };
        assert_eq!(
            demo_chunk_gap_from_env(),
            Duration::ZERO,
            "no value, no pacing"
        );
        unsafe { std::env::set_var("CHAOS_DEMO_CHUNK_GAP_MS", "40") };
        assert_eq!(demo_chunk_gap_from_env(), Duration::from_millis(40));
        unsafe { std::env::set_var("CHAOS_DEMO_CHUNK_GAP_MS", "900000") };
        assert_eq!(
            demo_chunk_gap_from_env(),
            Duration::from_millis(MAX_DEMO_CHUNK_GAP_MS),
            "the gap is paid once per chunk by whoever is reading the answer"
        );
        unsafe { std::env::set_var("CHAOS_DEMO_CHUNK_GAP_MS", "很快") };
        assert_eq!(
            demo_chunk_gap_from_env(),
            Duration::ZERO,
            "a bad value must not stop the host"
        );
        unsafe { std::env::remove_var("CHAOS_DEMO_CHUNK_GAP_MS") };
    }

    #[tokio::test]
    async fn protected_handshake_rejects_without_bearer() {
        let response = router(Engine::new(), "secret")
            .oneshot(Request::get("/api/handshake").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    /// 绑到 loopback 之外时，客户端就靠这个字段判断「401 是我没带凭据」还是
    /// 「服务坏了」。它以前恒是 false，而 `CHAOS_WEB_TOKEN` 正在挡着 `/ws`。
    #[tokio::test]
    async fn server_info_reports_whether_a_credential_is_required() {
        async fn auth_required(token: &str) -> serde_json::Value {
            let response = router(Engine::new(), token)
                .oneshot(
                    Request::get("/api/server-info")
                        .header(header::HOST, "127.0.0.1")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
                .await
                .expect("server-info 应当有身体");
            serde_json::from_slice(&bytes).expect("server-info 应当是 JSON")
        }

        assert_eq!(auth_required("secret").await["authRequired"], true);
        assert_eq!(auth_required("").await["authRequired"], false);
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
