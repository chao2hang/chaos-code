//! Preview proxy: a developer's own dev server, reached through this host at
//! `/preview/<port>/…`.
//!
//! A dev server listening on `127.0.0.1:3000` is unreachable from a browser that
//! reaches this host by name, and the usual answer — "open port 3000 too" — puts
//! a server with no authentication on the network. Mapping it under a path
//! prefix instead keeps every guard this host already has (`Host`, `Origin`,
//! credential, loopback-only by default) in front of the previewed app.
//!
//! Serving a dev server under a prefix breaks four things, and rewriting them is
//! why this is a proxy rather than a redirect://!
//! * `Host` — Vite, webpack-dev-server and friends refuse a `Host` they do not
//!   recognise. The upstream is dialed directly, so it sees `127.0.0.1:<port>`.
//! * `Origin` / `Referer` — a dev server that checks `Origin` compares it with
//!   itself, so both are rewritten to the upstream authority. A plain navigation
//!   carries no `Origin` at all; it is given the upstream's own, which is what the
//!   dev server would have seen from a request made directly to it.
//! * Cookies — `Set-Cookie: Path=/` from a dev server would be stored for the
//!   whole host and sent to every other route. `Path` is scoped to the preview
//!   prefix and `Domain` is dropped.
//! * WebSocket — the HMR socket lives under the same prefix, so the upgrade is
//!   bridged; without it the page loads and then never updates.
//!
//! ## What stands in front of it
//!
//! A dev server usually has no authentication of its own, so the proxy is the
//! only thing between it and whoever can reach this host. Three checks run before
//! any dev server is dialed, each answered with a reason code:
//!
//! * `Host` — the same rule the rest of this server uses: loopback, or the one
//!   public name the operator declared via `CHAOS_WEB_PUBLIC_ORIGIN`.
//! * loopback — and this is the "not publicly exposed by default" part. Even with
//!   a declared public origin, a preview is refused when the request came in
//!   through that name (`preview_loopback_only`), because publishing this host and
//!   publishing a project's dev server are two different decisions. An operator
//!   who means the second one says so with `CHAOS_WEB_PREVIEW_ALLOW_PUBLIC=1`.
//!   Reaching a preview from elsewhere is what `chaos-remote forward` is for: it
//!   puts the remote host's preview behind this machine's loopback.
//! * `Origin` — a page on another site may not drive state-changing requests into
//!   someone's dev server through their own browser session.
//!
//! The host's own bearer token is dropped before forwarding: it authenticates
//! *this* server, and a previewed app has no claim on it. It is not required of
//! the previewed page either, on purpose — see [`guard`].
//!
//! Nothing is reachable unless an operator names the ports
//! (`CHAOS_WEB_PREVIEW_PORTS=3000,5173`), and `127.0.0.1:<named port>` is the
//! only address this module will ever connect to.
//!
//! ## What the previewed app has to do
//!
//! A proxy in front of a path prefix cannot rewrite the app's own absolute URLs:
//! a `<script src="/main.js">` in the app's HTML resolves against this host, not
//! against the preview. The app has to build its URLs under the prefix — Vite's
//! `base: '/preview/3000/'`, webpack's `publicPath` — and `X-Forwarded-Prefix` is
//! sent on every request so a server-rendered app can do the same. The HMR client
//! is the app's own code too: it decides the host and port its socket connects to,
//! so a dev server that hard-codes its own port produces a socket that bypasses
//! this host (fine over loopback, missing when the browser is somewhere else).

use axum::{
    body::Body,
    extract::{FromRequestParts, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use std::{str::FromStr, sync::Arc};
use tokio_tungstenite::{
    WebSocketStream, tungstenite::Message as UpstreamMessage,
    tungstenite::client::IntoClientRequest,
};

use crate::{WebState, secure_json};

/// `HeaderName`s the `http` crate does not expose as constants.
fn header_name(name: &'static str) -> HeaderName {
    HeaderName::from_static(name)
}

/// Ports an operator has agreed to expose. An empty list means the feature is
/// off; off is answered with a reason code rather than a 404 that reads as a
/// broken link.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PreviewConfig {
    ports: Vec<u16>,
    /// Whether a preview may also be reached through the declared public origin
    /// instead of loopback only. Off by default: a dev server has no
    /// authentication of its own, so pointing a public name at it publishes it.
    pub allow_public: bool,
}

/// Naming thousands of ports is a typo rather than a policy, and every name is
/// another loopback service this server will connect to.
pub const MAX_PREVIEW_PORTS: usize = 32;
/// A previewed app's request body is buffered before it is forwarded, so a
/// request bigger than this is refused by name instead of being streamed with
/// no bound.
pub const MAX_PREVIEW_BODY_BYTES: usize = 32 * 1024 * 1024;

/// Why a preview stopped before a dev server answered. The code goes in the body
/// because "the preview never loads" has many causes and an operator can only
/// fix the one they are told about.
pub const PREVIEW_DISABLED: &str = "preview_disabled";
pub const PREVIEW_PORT_NOT_ALLOWED: &str = "preview_port_not_allowed";
pub const PREVIEW_TARGET_INVALID: &str = "preview_target_invalid";
pub const PREVIEW_UPSTREAM_UNREACHABLE: &str = "preview_upstream_unreachable";
pub const PREVIEW_BODY_TOO_LARGE: &str = "preview_body_too_large";
pub const PREVIEW_UPSTREAM_FAILED: &str = "preview_upstream_failed";
pub const PREVIEW_UPGRADE_UNSUPPORTED: &str = "preview_upgrade_unsupported";
/// Reached through a name other machines can also be pointed at, while previews
/// are still loopback-only. The code is separate from `host_not_allowed` because
/// the fix is different: that one is answered by declaring the origin, this one
/// by deciding that a development server should face the network.
pub const PREVIEW_LOOPBACK_ONLY: &str = "preview_loopback_only";

/// What a request has to satisfy before any dev server is dialed.
///
/// The host's bearer token is deliberately *not* one of them: a previewed page
/// loads its scripts, styles and images as ordinary subresource requests, and a
/// browser attaches no `Authorization` header to any of them, so requiring one
/// here would refuse the page rather than protect it. What does protect it are
/// the three checks a browser cannot opt out of: which name the request came in
/// through, which page the `Origin` claims to be, and — by default — the fact
/// that only the machine itself may ask at all.
pub fn guard(headers: &HeaderMap, config: &PreviewConfig) -> Option<Response> {
    if !crate::host_allowed(headers) {
        return Some(refusal(
            StatusCode::UNAUTHORIZED,
            "host_not_allowed",
            "set CHAOS_WEB_PUBLIC_ORIGIN to the name this host is reached by",
        ));
    }
    if !config.allow_public && !loopback_request(headers) {
        return Some(refusal(
            StatusCode::UNAUTHORIZED,
            PREVIEW_LOOPBACK_ONLY,
            "previews are reachable through 127.0.0.1/localhost only; set \
             CHAOS_WEB_PREVIEW_ALLOW_PUBLIC=1 to also serve them through the \
             declared public origin",
        ));
    }
    if !crate::origin_allowed(headers) {
        let reason = crate::origin_refusal(headers);
        return Some(refusal(
            StatusCode::UNAUTHORIZED,
            reason,
            "the page that issued this request is not this host",
        ));
    }
    None
}

/// Whether the request arrived addressed to the machine itself rather than to a
/// name other machines can be pointed at.
pub fn loopback_request(headers: &HeaderMap) -> bool {
    headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(|host| host.parse::<axum::http::uri::Authority>().ok())
        .is_some_and(|authority| {
            !authority.as_str().contains('@') && crate::loopback_authority(&authority)
        })
}

fn refusal(status: StatusCode, reason: &str, detail: impl Into<String>) -> Response {
    let mut response = secure_json(serde_json::json!({
        "error": reason,
        "detail": detail.into(),
    }));
    *response.status_mut() = status;
    response
}

impl PreviewConfig {
    pub fn disabled() -> Self {
        Self::default()
    }

    /// `CHAOS_WEB_PREVIEW_PORTS=3000,5173`. Absent, blank or `0` means off.
    /// A malformed value is an error so the host refuses to start rather than
    /// run with fewer previews than the operator asked for.
    pub fn from_env() -> Result<Self, String> {
        let ports = Self::parse(&std::env::var("CHAOS_WEB_PREVIEW_PORTS").unwrap_or_default())?;
        let allow_public = match std::env::var("CHAOS_WEB_PREVIEW_ALLOW_PUBLIC")
            .unwrap_or_default()
            .trim()
        {
            "" | "0" | "false" | "no" => false,
            "1" | "true" | "yes" => true,
            other => {
                return Err(format!(
                    "CHAOS_WEB_PREVIEW_ALLOW_PUBLIC={other:?} is not 0 or 1"
                ));
            }
        };
        Ok(Self {
            ports,
            allow_public,
        })
    }

    pub fn from_ports(ports: Vec<u16>) -> Self {
        Self {
            ports,
            allow_public: false,
        }
    }

    pub fn public_enabled(mut self, allow_public: bool) -> Self {
        self.allow_public = allow_public;
        self
    }

    pub fn parse(raw: &str) -> Result<Vec<u16>, String> {
        let raw = raw.trim();
        if raw.is_empty() || raw == "0" {
            return Ok(Vec::new());
        }
        let mut ports = Vec::new();
        for entry in raw.split(',') {
            let entry = entry.trim();
            if entry.is_empty() {
                return Err("an empty entry (a trailing comma?)".to_string());
            }
            let port: u16 = entry
                .parse()
                .map_err(|_| format!("{entry:?} is not a port number"))?;
            if port == 0 {
                return Err("port 0 cannot be previewed".to_string());
            }
            if ports.contains(&port) {
                return Err(format!("port {port} is named twice"));
            }
            ports.push(port);
        }
        if ports.len() > MAX_PREVIEW_PORTS {
            return Err(format!(
                "{} ports is more than the {MAX_PREVIEW_PORTS} allowed",
                ports.len()
            ));
        }
        Ok(ports)
    }

    pub fn is_enabled(&self) -> bool {
        !self.ports.is_empty()
    }

    pub fn ports(&self) -> &[u16] {
        &self.ports
    }

    pub fn allows(&self, port: u16) -> bool {
        self.ports.contains(&port)
    }

    /// What the operator named, for the refusal messages and for the index at
    /// `/preview`, so a wrong URL says what is actually allowed.
    pub fn summary(&self) -> String {
        if self.ports.is_empty() {
            return "no ports are enabled (set CHAOS_WEB_PREVIEW_PORTS)".to_string();
        }
        self.ports
            .iter()
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// Where a preview URL points.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewTarget {
    pub port: u16,
    /// `/preview/3000` — the prefix the browser sees.
    pub prefix: String,
    /// `/src/main.tsx?v=1` — what the dev server sees; always starts with `/`.
    pub upstream_path: String,
    /// `http://127.0.0.1:3000` — the only authority this module dials.
    pub upstream_origin: String,
}

/// Split `/preview/<port>/…` into a target after checking that the port was
/// named. A path that is not shaped like a preview is reported, not guessed at.
pub fn resolve_target(
    path_and_query: &str,
    config: &PreviewConfig,
) -> Result<PreviewTarget, Response> {
    let rest = path_and_query.strip_prefix("/preview/").ok_or_else(|| {
        refusal(
            StatusCode::NOT_FOUND,
            PREVIEW_TARGET_INVALID,
            "expected /preview/<port>/<path>",
        )
    })?;
    let (port_part, tail) = rest.split_once('/').unwrap_or((rest, ""));
    let port: u16 = port_part.parse().map_err(|_| {
        refusal(
            StatusCode::NOT_FOUND,
            PREVIEW_TARGET_INVALID,
            format!("{port_part:?} is not a port number"),
        )
    })?;
    if !config.allows(port) {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            if config.is_enabled() {
                PREVIEW_PORT_NOT_ALLOWED
            } else {
                PREVIEW_DISABLED
            },
            format!("port {port}; enabled: {}", config.summary()),
        ));
    }
    let tail = match tail {
        "" => "/".to_string(),
        tail if tail.starts_with('/') => tail.to_string(),
        tail => format!("/{tail}"),
    };
    Ok(PreviewTarget {
        port,
        prefix: format!("/preview/{port}"),
        upstream_path: tail,
        upstream_origin: format!("http://127.0.0.1:{port}"),
    })
}

/// Hop-by-hop headers (RFC 9110 §7.6.1): they describe one connection rather
/// than the exchange, so forwarding them corrupts whichever side reads them.
/// `Upgrade` is one of them, and on the WebSocket path it would also collide with
/// the copy the client writes for itself.
///
/// `Keep-Alive` is hop-by-hop too, but the `http` crate exposes no constant for it.
const KEEP_ALIVE: HeaderName = HeaderName::from_static("keep-alive");

const HOP_BY_HOP: [HeaderName; 8] = [
    header::CONNECTION,
    KEEP_ALIVE,
    header::PROXY_AUTHENTICATE,
    header::PROXY_AUTHORIZATION,
    header::TE,
    header::TRAILER,
    header::TRANSFER_ENCODING,
    header::UPGRADE,
];

fn is_hop_by_hop(name: &HeaderName) -> bool {
    HOP_BY_HOP.contains(name)
}

/// `true` for a WebSocket upgrade request.
pub fn is_websocket_upgrade(headers: &HeaderMap) -> bool {
    let upgrading = headers
        .get(header::CONNECTION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
        });
    let websocket = headers
        .get(header::UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("websocket"));
    upgrading && websocket
}

/// `true` for any upgrade request. Upgrades other than WebSocket are refused by
/// name instead of being answered with a plain HTTP response, which a browser
/// would report as a failed upgrade with no hint why.
pub fn is_any_upgrade(headers: &HeaderMap) -> bool {
    headers
        .get(header::UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| !value.trim().is_empty())
        && headers
            .get(header::CONNECTION)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .split(',')
                    .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
            })
}

/// Turn the browser's headers into what the dev server should receive.
///
/// Dropped on the way out: the host's `Authorization` (a credential for this
/// server, not for the app), and the `Host`/`Origin`/`Referer` the browser meant
/// for this host — each is re-added below with the upstream's own authority.
pub fn rewrite_request_headers(incoming: &HeaderMap, target: &PreviewTarget) -> HeaderMap {
    let mut out = HeaderMap::new();
    for (name, value) in incoming {
        if is_hop_by_hop(name)
            || matches!(
                name,
                &header::HOST
                    | &header::ORIGIN
                    | &header::REFERER
                    | &header::AUTHORIZATION
                    | &header::SEC_WEBSOCKET_KEY
                    | &header::SEC_WEBSOCKET_VERSION
                    | &header::SEC_WEBSOCKET_EXTENSIONS
            )
        {
            continue;
        }
        out.append(name.clone(), value.clone());
    }
    if let Ok(value) = HeaderValue::from_str(&format!("127.0.0.1:{}", target.port)) {
        out.insert(header::HOST, value);
    }
    if let Ok(value) = HeaderValue::from_str(&target.upstream_origin) {
        out.insert(header::ORIGIN, value);
    }
    if let Some(referer) = incoming
        .get(header::REFERER)
        .and_then(|value| value.to_str().ok())
        .and_then(|referer| rewrite_referer(referer, target))
        && let Ok(value) = HeaderValue::from_str(&referer)
    {
        out.insert(header::REFERER, value);
    }
    // The forwarding headers are what makes a misbehaving dev server
    // debuggable, and what lets an app build its own URLs correctly.
    if let Ok(value) = HeaderValue::from_str(&target.prefix) {
        out.insert(HeaderName::from_static("x-forwarded-prefix"), value);
    }
    if let Some(host) = incoming
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        && let Ok(value) = HeaderValue::from_str(host)
    {
        out.insert(header_name("x-forwarded-host"), value);
    }
    let proto = if crate::forwarded_proto_https(incoming) {
        "https"
    } else {
        "http"
    };
    out.insert(
        HeaderName::from_static("x-forwarded-proto"),
        HeaderValue::from_static(proto),
    );
    out
}

/// Rewrite a `Referer` the browser sent to this host into the same URL on the
/// dev server, so a dev server that checks referrers still recognises its page.
pub fn rewrite_referer(referer: &str, target: &PreviewTarget) -> Option<String> {
    let after_authority = referer.split_once("://").map(|(_, rest)| rest)?;
    // Keep the leading slash: it is what makes the remainder a path.
    let path_and_query = match after_authority.find('/') {
        Some(slash) => &after_authority[slash..],
        None => "/",
    };
    let tail = path_and_query
        .strip_prefix(&target.prefix)
        .unwrap_or(path_and_query);
    let tail = if tail.is_empty() { "/" } else { tail };
    Some(format!("{}{}", target.upstream_origin, tail))
}

/// `/preview/3000` + `/src/app.js` → `/preview/3000/src/app.js`.
fn join_path(prefix: &str, path: &str) -> String {
    if path == "/" {
        return format!("{prefix}/");
    }
    if path.starts_with('/') {
        format!("{prefix}{path}")
    } else {
        format!("{prefix}/{path}")
    }
}

/// Scope a dev server's `Set-Cookie` to its own prefix.
///
/// `Domain` is dropped: naming a domain widens the cookie to every subdomain of
/// this host, which is a reach the previewed app never earned. A cookie with no
/// `Path` gets the prefix, so it is sent back to this preview and nowhere else.
pub fn rewrite_set_cookie(value: &str, target: &PreviewTarget) -> String {
    let mut out = String::with_capacity(value.len() + target.prefix.len() + 8);
    let mut saw_path = false;
    for (index, part) in value.split(';').enumerate() {
        let part = part.trim();
        if index == 0 {
            out.push_str(part);
            continue;
        }
        let name = part.split('=').next().unwrap_or(part).trim();
        if name.eq_ignore_ascii_case("domain") {
            continue;
        }
        if name.eq_ignore_ascii_case("path") {
            saw_path = true;
            let upstream_path = part
                .split_once('=')
                .map(|(_, path)| path.trim())
                .filter(|path| !path.is_empty())
                .unwrap_or("/");
            out.push_str("; Path=");
            out.push_str(&join_path(&target.prefix, upstream_path));
            continue;
        }
        out.push_str("; ");
        out.push_str(part);
    }
    if !saw_path {
        out.push_str("; Path=");
        out.push_str(&target.prefix);
        out.push('/');
    }
    out
}

/// Pull a dev server's redirect back under the prefix, whether it was written as
/// `/login` or as an absolute `http://127.0.0.1:3000/login`.
pub fn rewrite_location(location: &str, target: &PreviewTarget) -> String {
    if let Some(after) = location.strip_prefix(&target.upstream_origin) {
        let (path, query) = after.split_once('?').unwrap_or((after, ""));
        let joined = join_path(&target.prefix, if path.is_empty() { "/" } else { path });
        return if query.is_empty() {
            joined
        } else {
            format!("{joined}?{query}")
        };
    }
    if location.starts_with('/') && !location.starts_with(&target.prefix) {
        return join_path(&target.prefix, location);
    }
    location.to_string()
}

/// Response headers to replay, with `Set-Cookie` and `Location` rewritten and
/// hop-by-hop headers removed. Multi-value headers keep every value.
pub fn rewrite_response_headers(
    headers: &HeaderMap,
    target: &PreviewTarget,
) -> Vec<(HeaderName, String)> {
    let mut out = Vec::with_capacity(headers.len());
    for (name, value) in headers {
        if is_hop_by_hop(name) {
            continue;
        }
        match value.to_str() {
            Ok(text) => match *name {
                header::SET_COOKIE => out.push((name.clone(), rewrite_set_cookie(text, target))),
                header::LOCATION => out.push((name.clone(), rewrite_location(text, target))),
                _ => out.push((name.clone(), text.to_string())),
            },
            // A non-UTF-8 header value cannot be rewritten; replay it unchanged
            // rather than dropping a header the app needs.
            Err(_) => {
                if let Ok(copy) = HeaderValue::from_bytes(value.as_bytes()) {
                    out.push((
                        name.clone(),
                        String::from_utf8_lossy(copy.as_bytes()).into_owned(),
                    ));
                }
            }
        }
    }
    out
}

/// The client that dials dev servers. Redirects are never followed: a `302` is
/// rewritten and handed back so the browser stays under the prefix. Proxies are
/// never used either — the upstream is this machine's own loopback.
fn upstream_client() -> reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    // The upstream is this machine's own loopback over plain HTTP: no TLS is ever
    // negotiated, so the signed-root TLS policy has nothing to apply to here.
    #[allow(clippy::disallowed_methods)]
    let built = CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(std::time::Duration::from_secs(5))
            // A dev server can sit inside a long rebuild; a hung request is
            // worse to debug than a slow one.
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .unwrap_or_default()
    });
    built.clone()
}

/// `GET /preview` and `/preview/` answer with what is enabled instead of a bare
/// 404, because the entry point is typed by hand.
async fn preview_index(State(state): State<Arc<WebState>>, headers: HeaderMap) -> Response {
    if let Some(refusal) = guard(&headers, &state.previews) {
        return refusal;
    }
    if !state.previews.is_enabled() {
        return refusal(
            StatusCode::NOT_FOUND,
            PREVIEW_DISABLED,
            state.previews.summary(),
        );
    }
    secure_json(serde_json::json!({
        "previews": state.previews.ports().iter().map(|port| serde_json::json!({
            "port": port,
            "url": format!("/preview/{port}/"),
        })).collect::<Vec<_>>(),
        "upstream": "127.0.0.1",
    }))
}

async fn preview_route(State(state): State<Arc<WebState>>, request: Request) -> Response {
    let (mut parts, body) = request.into_parts();
    if let Some(refusal) = guard(&parts.headers, &state.previews) {
        return refusal;
    }
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|value| value.as_str().to_string())
        .unwrap_or_else(|| parts.uri.path().to_string());
    let target = match resolve_target(&path_and_query, &state.previews) {
        Ok(target) => target,
        Err(response) => return response,
    };
    if is_any_upgrade(&parts.headers) {
        if !is_websocket_upgrade(&parts.headers) {
            return refusal(
                StatusCode::BAD_REQUEST,
                PREVIEW_UPGRADE_UNSUPPORTED,
                format!(
                    "only WebSocket upgrades are bridged, not {:?}",
                    parts.uri.path()
                ),
            );
        }
        let upgrade =
            match axum::extract::ws::WebSocketUpgrade::from_request_parts(&mut parts, &state).await
            {
                Ok(upgrade) => upgrade,
                Err(rejection) => {
                    return refusal(
                        StatusCode::BAD_REQUEST,
                        PREVIEW_UPGRADE_UNSUPPORTED,
                        format!("the upgrade could not be accepted: {rejection}"),
                    );
                }
            };
        drop(body);
        return bridge_websocket(upgrade, &target, &parts.headers).await;
    }
    let bytes = match axum::body::to_bytes(body, MAX_PREVIEW_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return refusal(
                StatusCode::PAYLOAD_TOO_LARGE,
                PREVIEW_BODY_TOO_LARGE,
                format!(
                    "a previewed request body is buffered up to {MAX_PREVIEW_BODY_BYTES} bytes"
                ),
            );
        }
    };
    let url = format!("{}{}", target.upstream_origin, target.upstream_path);
    let method = match reqwest::Method::from_str(parts.method.as_str()) {
        Ok(method) => method,
        Err(_) => {
            return refusal(
                StatusCode::METHOD_NOT_ALLOWED,
                PREVIEW_TARGET_INVALID,
                format!("{:?} cannot be forwarded", parts.method),
            );
        }
    };
    let mut builder = upstream_client()
        .request(method, &url)
        .headers(rewrite_request_headers(&parts.headers, &target));
    if !bytes.is_empty() {
        builder = builder.body(bytes);
    }
    let response = match builder.send().await {
        Ok(response) => response,
        Err(error) => {
            return refusal(
                StatusCode::BAD_GATEWAY,
                if error.is_connect() {
                    PREVIEW_UPSTREAM_UNREACHABLE
                } else {
                    PREVIEW_UPSTREAM_FAILED
                },
                format!("{url}: {error}"),
            );
        }
    };
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let headers = response.headers().clone();
    let mut builder = Response::builder().status(status);
    for (name, value) in rewrite_response_headers(&headers, &target) {
        if let Ok(value) = HeaderValue::from_str(&value) {
            builder = builder.header(name, value);
        }
    }
    builder
        .body(Body::from_stream(response.bytes_stream()))
        .unwrap_or_else(|_| {
            refusal(
                StatusCode::BAD_GATEWAY,
                PREVIEW_UPSTREAM_FAILED,
                "the upstream response could not be replayed",
            )
        })
}

/// The browser's handshake headers as the dev server should receive them.
///
/// `Sec-WebSocket-Key` and `-Version` are dropped rather than copied: this proxy
/// completes a handshake of its own upstream, so the browser's key is not the key
/// whose `Sec-WebSocket-Accept` will come back, and the client already supplies
/// the version. Copying them would put two of each on the upstream request.
/// `Sec-WebSocket-Extensions` is dropped so neither hop negotiates an extension
/// that the other side's framing does not know about. The browser's
/// `Sec-WebSocket-Protocol` offer *is* copied, because the dev server has to be
/// able to choose from it.
pub fn rewrite_upgrade_headers(
    incoming: &HeaderMap,
    target: &PreviewTarget,
) -> Vec<(HeaderName, HeaderValue)> {
    let mut out = Vec::new();
    for (name, value) in incoming {
        if is_hop_by_hop(name)
            || matches!(
                name,
                &header::HOST
                    | &header::ORIGIN
                    | &header::AUTHORIZATION
                    | &header::REFERER
                    | &header::SEC_WEBSOCKET_KEY
                    | &header::SEC_WEBSOCKET_VERSION
                    | &header::SEC_WEBSOCKET_EXTENSIONS
            )
        {
            continue;
        }
        out.push((name.clone(), value.clone()));
    }
    if let Ok(value) = HeaderValue::from_str(&format!("127.0.0.1:{}", target.port)) {
        out.push((header::HOST, value));
    }
    if let Ok(value) = HeaderValue::from_str(&target.upstream_origin) {
        out.push((header::ORIGIN, value));
    }
    if let Some(referer) = incoming
        .get(header::REFERER)
        .and_then(|value| value.to_str().ok())
        .and_then(|referer| rewrite_referer(referer, target))
        .and_then(|referer| HeaderValue::from_str(&referer).ok())
    {
        out.push((header::REFERER, referer));
    }
    out
}

/// Bridge the browser's HMR socket to the dev server's.
///
/// This is a message-level bridge rather than a byte tunnel: this server
/// completes the handshake with the browser using the browser's own key, and
/// opens a separate handshake upstream with `Host` and `Origin` rewritten so the
/// dev server accepts it. Each hop then gets its own correct `Sec-WebSocket-Accept`.
async fn bridge_websocket(
    upgrade: axum::extract::ws::WebSocketUpgrade,
    target: &PreviewTarget,
    headers: &HeaderMap,
) -> Response {
    let url = format!("ws://127.0.0.1:{}{}", target.port, target.upstream_path);
    let request = match url.as_str().into_client_request() {
        Ok(request) => request,
        Err(error) => {
            return refusal(
                StatusCode::BAD_GATEWAY,
                PREVIEW_TARGET_INVALID,
                format!("{url}: {error}"),
            );
        }
    };
    let mut request = request;
    // The WebSocket client writes these five itself: it takes the first value under
    // each name, discards any others, and rejects a request where a copy survives.
    // Inserting is what makes the value chosen here the value that arrives. For
    // `Host` it also overrides the one `into_client_request()` seeded from the URI we
    // are dialing — the same authority, since both come from `target`, so the two
    // ways of getting it right cannot currently be told apart from the outside.
    const SINGLE_VALUE: [HeaderName; 5] = [
        header::HOST,
        header::CONNECTION,
        header::UPGRADE,
        header::SEC_WEBSOCKET_VERSION,
        header::SEC_WEBSOCKET_KEY,
    ];
    for (name, value) in rewrite_upgrade_headers(headers, target) {
        if SINGLE_VALUE.contains(&name) {
            request.headers_mut().insert(name, value);
        } else {
            request.headers_mut().append(name, value);
        }
    }
    let authority = format!("127.0.0.1:{}", target.port);
    let socket = match tokio::net::TcpStream::connect(&authority).await {
        Ok(socket) => socket,
        Err(error) => {
            return refusal(
                StatusCode::BAD_GATEWAY,
                PREVIEW_UPSTREAM_UNREACHABLE,
                format!("{authority}: {error}"),
            );
        }
    };
    let (upstream, handshake) = match tokio_tungstenite::client_async(request, socket).await {
        Ok(pair) => pair,
        Err(error) => {
            return refusal(
                StatusCode::BAD_GATEWAY,
                PREVIEW_UPSTREAM_UNREACHABLE,
                format!("{url}: {error}"),
            );
        }
    };
    // Whatever the dev server selected has to be what the browser is told was
    // selected, and only if the browser offered it — echoing a protocol nobody
    // asked for is its own bug, and dropping one the app needs breaks the socket.
    let upgrade = match selected_subprotocol(handshake.headers(), headers) {
        Some(selected) => upgrade.protocols([selected]),
        None => upgrade,
    };
    upgrade.on_upgrade(move |browser| pump(browser, upstream))
}

/// The subprotocol the dev server chose, but only when the browser offered it.
/// `axum`'s own negotiation picks from a list this proxy does not have — the
/// acceptable set is whatever the upstream agrees to — so the offered list is
/// read from the browser's request headers directly.
fn selected_subprotocol(handshake: &HeaderMap, offered: &HeaderMap) -> Option<String> {
    let chosen = handshake
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    offered
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok())
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .find(|candidate| candidate.eq_ignore_ascii_case(chosen))
        .map(str::to_string)
}

/// Copy frames both ways until either side closes. Ping/pong is answered by each
/// hop's own state machine, which is what stops a sleeping laptop from leaving a
/// half-open socket.
async fn pump(
    mut browser: axum::extract::ws::WebSocket,
    mut upstream: WebSocketStream<tokio::net::TcpStream>,
) {
    loop {
        tokio::select! {
            from_browser = browser.recv() => match from_browser {
                Some(Ok(message)) => {
                    if upstream.send(to_upstream(message)).await.is_err() {
                        break;
                    }
                }
                Some(Err(_)) | None => break,
            },
            from_upstream = upstream.next() => match from_upstream {
                Some(Ok(message)) => {
                    if browser.send(to_browser(message)).await.is_err() {
                        break;
                    }
                }
                Some(Err(_)) | None => break,
            },
        }
    }
    let _ = browser.send(axum::extract::ws::Message::Close(None)).await;
    let _ = upstream.close(None).await;
}

fn to_upstream(message: axum::extract::ws::Message) -> UpstreamMessage {
    use axum::extract::ws::Message as Browser;
    match message {
        Browser::Text(text) => UpstreamMessage::Text(text.as_str().to_string().into()),
        Browser::Binary(bytes) => UpstreamMessage::Binary(bytes),
        Browser::Ping(bytes) => UpstreamMessage::Ping(bytes),
        Browser::Pong(bytes) => UpstreamMessage::Pong(bytes),
        Browser::Close(_) => UpstreamMessage::Close(None),
    }
}

fn to_browser(message: UpstreamMessage) -> axum::extract::ws::Message {
    use axum::extract::ws::Message as Browser;
    match message {
        UpstreamMessage::Text(text) => Browser::Text(text.as_str().to_string().into()),
        UpstreamMessage::Binary(bytes) => Browser::Binary(bytes),
        UpstreamMessage::Ping(bytes) => Browser::Ping(bytes),
        UpstreamMessage::Pong(bytes) => Browser::Pong(bytes),
        UpstreamMessage::Close(_) => Browser::Close(None),
        UpstreamMessage::Frame(_) => Browser::Ping(Vec::new().into()),
    }
}

/// Routes for the preview proxy. Registered unconditionally so a disabled
/// feature answers with `preview_disabled` instead of looking like a broken
/// link; the guard is enforced per request from [`PreviewConfig`].
pub fn routes() -> axum::Router<Arc<WebState>> {
    axum::Router::new()
        .route("/preview", axum::routing::get(preview_index))
        .route("/preview/{*rest}", axum::routing::any(preview_route))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(
                HeaderName::from_str(name).expect("header name"),
                HeaderValue::from_str(value).expect("header value"),
            );
        }
        map
    }

    fn target(port: u16) -> PreviewTarget {
        PreviewTarget {
            port,
            prefix: format!("/preview/{port}"),
            upstream_path: "/".to_string(),
            upstream_origin: format!("http://127.0.0.1:{port}"),
        }
    }

    /// The reason code of a refusal produced by one of these functions.
    fn code(response: Response) -> String {
        refusal_json(response)["error"]
            .as_str()
            .expect("error field")
            .to_string()
    }

    /// What the refusal tells the operator to do. A code alone is only useful if
    /// the body points at the thing to change.
    fn detail(response: Response) -> String {
        refusal_json(response)["detail"]
            .as_str()
            .expect("detail field")
            .to_string()
    }

    fn refusal_json(response: Response) -> serde_json::Value {
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/json",
            "a refusal is JSON an operator can parse"
        );
        let body = tokio_block_on(axum::body::to_bytes(response.into_body(), 1 << 20))
            .expect("refusal body");
        serde_json::from_slice(&body).expect("refusal body is JSON")
    }

    /// A refusal's body is a stream, and reading a stream needs a runtime.
    /// Constructing one per assertion is cheap next to what the assertion checks.
    fn tokio_block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime")
            .block_on(future)
    }

    #[test]
    fn the_allowlist_parser_accepts_only_a_port_list() {
        assert_eq!(PreviewConfig::parse("").unwrap(), Vec::<u16>::new());
        assert_eq!(PreviewConfig::parse("  ").unwrap(), Vec::<u16>::new());
        assert_eq!(PreviewConfig::parse("0").unwrap(), Vec::<u16>::new());
        assert_eq!(PreviewConfig::parse("3000").unwrap(), vec![3000]);
        assert_eq!(
            PreviewConfig::parse(" 3000 , 5173 ").unwrap(),
            vec![3000, 5173]
        );

        for (rejected, needle) in [
            ("3000,", "empty entry"),
            ("http://127.0.0.1:3000", "not a port number"),
            ("3000,0", "port 0"),
            ("3000,3000", "named twice"),
            ("99999", "not a port number"),
        ] {
            let error = PreviewConfig::parse(rejected)
                .expect_err(&format!("{rejected:?} must be refused"))
                .to_string();
            assert!(error.contains(needle), "{rejected:?} → {error}");
        }

        let many = (1000u16..1000 + MAX_PREVIEW_PORTS as u16 + 1)
            .map(|port| port.to_string())
            .collect::<Vec<_>>()
            .join(",");
        assert!(
            PreviewConfig::parse(&many)
                .expect_err("an oversized allowlist must be refused")
                .contains("more than"),
            "the cap has to say so"
        );
    }

    #[test]
    fn an_environment_value_names_the_variable_it_failed_on() {
        // Both variables are read by `from_env`, and an operator with one bad
        // value among two has to be told which one to edit.
        //
        // Edition 2024 makes these unsafe because another thread may be reading
        // the environment at the same time; this test does not start such a thread.
        unsafe {
            std::env::remove_var("CHAOS_WEB_PREVIEW_PORTS");
            std::env::set_var("CHAOS_WEB_PREVIEW_ALLOW_PUBLIC", "maybe");
        }
        let error = PreviewConfig::from_env().expect_err("a non-boolean must be refused");
        assert!(
            error.contains("CHAOS_WEB_PREVIEW_ALLOW_PUBLIC"),
            "the message must name the variable: {error}"
        );
        unsafe {
            std::env::remove_var("CHAOS_WEB_PREVIEW_ALLOW_PUBLIC");
        }
    }

    #[test]
    fn a_preview_url_resolves_only_to_a_named_port() {
        let config = PreviewConfig::from_ports(vec![3000]);

        let resolved = resolve_target("/preview/3000/src/app.js?v=2", &config).expect("allowed");
        assert_eq!(resolved.port, 3000);
        assert_eq!(resolved.prefix, "/preview/3000");
        assert_eq!(resolved.upstream_path, "/src/app.js?v=2");
        assert_eq!(resolved.upstream_origin, "http://127.0.0.1:3000");

        assert_eq!(
            resolve_target("/preview/3000", &config)
                .expect("bare prefix")
                .upstream_path,
            "/"
        );
        assert_eq!(
            resolve_target("/preview/3000/", &config)
                .expect("trailing slash")
                .upstream_path,
            "/"
        );

        assert_eq!(
            code(resolve_target("/preview/5173/", &config).expect_err("unnamed port")),
            PREVIEW_PORT_NOT_ALLOWED
        );
        assert_eq!(
            code(resolve_target("/preview/app/", &config).expect_err("not a port")),
            PREVIEW_TARGET_INVALID
        );
        // With nothing enabled the answer is the reason it is off, not a 404 that
        // reads as a broken link.
        assert_eq!(
            code(
                resolve_target("/preview/3000/", &PreviewConfig::disabled())
                    .expect_err("everything is unnamed")
            ),
            PREVIEW_DISABLED
        );
    }

    #[test]
    fn the_summary_lists_what_is_enabled() {
        assert_eq!(
            PreviewConfig::disabled().summary(),
            "no ports are enabled (set CHAOS_WEB_PREVIEW_PORTS)"
        );
        assert_eq!(
            PreviewConfig::from_ports(vec![3000, 5173]).summary(),
            "3000,5173"
        );
        assert!(
            detail(
                resolve_target("/preview/3000/", &PreviewConfig::from_ports(vec![5173]))
                    .expect_err("unnamed")
            )
            .contains("enabled: 5173"),
            "a wrong URL has to say what is actually allowed"
        );
    }

    #[test]
    fn request_headers_are_rewritten_to_the_upstreams_own_authority() {
        let config = PreviewConfig::from_ports(vec![3000]);
        let target = resolve_target("/preview/3000/", &config).expect("allowed");
        let outgoing = rewrite_request_headers(
            &headers(&[
                ("Host", "chaos.example.test:8443"),
                ("Origin", "https://chaos.example.test:8443"),
                ("X-Forwarded-Proto", "https"),
                (
                    "Referer",
                    "https://chaos.example.test:8443/preview/3000/app",
                ),
                ("Authorization", "Bearer the-host-token"),
                ("Cookie", "sid=1"),
                ("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ=="),
                ("Sec-WebSocket-Version", "13"),
                ("Connection", "keep-alive"),
                ("Keep-Alive", "timeout=5"),
                ("Transfer-Encoding", "chunked"),
                ("Accept", "text/html"),
            ]),
            &target,
        );

        assert_eq!(outgoing[header::HOST], "127.0.0.1:3000");
        assert_eq!(outgoing[header::ORIGIN], "http://127.0.0.1:3000");
        assert_eq!(
            outgoing[header::REFERER],
            "http://127.0.0.1:3000/app",
            "the prefix is this host's business, not the dev server's"
        );
        assert_eq!(outgoing[header::COOKIE], "sid=1");
        assert_eq!(outgoing[header::ACCEPT], "text/html");
        assert_eq!(outgoing["x-forwarded-prefix"], "/preview/3000");
        assert_eq!(outgoing["x-forwarded-host"], "chaos.example.test:8443");
        assert_eq!(outgoing["x-forwarded-proto"], "https");

        // The credential authenticates this server; a previewed app has no claim
        // on it. The per-connection headers describe a hop, not the exchange.
        for dropped in [
            header::AUTHORIZATION,
            header::SEC_WEBSOCKET_KEY,
            header::SEC_WEBSOCKET_VERSION,
            header::CONNECTION,
            HeaderName::from_static("keep-alive"),
            header::TRANSFER_ENCODING,
        ] {
            assert!(
                !outgoing.contains_key(&dropped),
                "{dropped} must not be forwarded"
            );
        }
    }

    #[test]
    fn an_upgrade_gets_its_own_handshake_instead_of_a_copy() {
        let config = PreviewConfig::from_ports(vec![3000]);
        let target = resolve_target("/preview/3000/", &config).expect("allowed");
        let outgoing = rewrite_upgrade_headers(
            &headers(&[
                ("Host", "127.0.0.1:8080"),
                ("Origin", "http://127.0.0.1:8080"),
                ("Referer", "http://127.0.0.1:8080/preview/3000/"),
                ("Authorization", "Bearer the-host-token"),
                ("Connection", "Upgrade"),
                ("Upgrade", "websocket"),
                ("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ=="),
                ("Sec-WebSocket-Version", "13"),
                ("Sec-WebSocket-Extensions", "permessage-deflate"),
                ("Sec-WebSocket-Protocol", "chaos-hmr, graphql-ws"),
                ("Cookie", "sid=1"),
            ]),
            &target,
        );
        let map = {
            let mut map = HeaderMap::new();
            for (name, value) in &outgoing {
                map.append(name.clone(), value.clone());
            }
            map
        };

        assert_eq!(map[header::HOST], "127.0.0.1:3000");
        assert_eq!(map[header::ORIGIN], "http://127.0.0.1:3000");
        assert_eq!(map[header::REFERER], "http://127.0.0.1:3000/");
        assert_eq!(map[header::COOKIE], "sid=1");
        // The offer survives; the choice is the dev server's to make.
        assert_eq!(map[header::SEC_WEBSOCKET_PROTOCOL], "chaos-hmr, graphql-ws");
        // Tungstenite supplies these itself; forwarding the browser's copies would
        // put values belonging to the other connection into this handshake.
        for dropped in [
            header::SEC_WEBSOCKET_KEY,
            header::SEC_WEBSOCKET_VERSION,
            header::SEC_WEBSOCKET_EXTENSIONS,
            header::CONNECTION,
            header::UPGRADE,
            header::AUTHORIZATION,
        ] {
            assert_eq!(
                outgoing.iter().filter(|(name, _)| name == dropped).count(),
                0,
                "{dropped} must be re-generated upstream, not copied"
            );
        }

        assert_eq!(
            outgoing
                .iter()
                .filter(|(name, _)| *name == header::HOST)
                .count(),
            1,
            "one Host, and it is the app's own"
        );
    }
    /// The bridge's choice of `insert` over `append` for the five headers the
    /// WebSocket client owns rests on what that client does when a name carries two
    /// values. That is a property of a dependency, so it is pinned here: if a future
    /// version joins or reorders them instead, this is where the change shows up.
    ///
    /// The `Host` assertion below is doing more work than the others. Two things could
    /// decide the authority the dev server reads: the request URI the bridge dials, or
    /// the header map the rewrite writes. They normally hold the same string, because
    /// `into_client_request()` seeds the map with the URI's authority, and that is why
    /// dropping the rewrite's `Host` entirely is invisible to the integration tests.
    /// Here the two are made to disagree — the URI is port 3000, the map says 9999 —
    /// so this says which one the client actually sends, and therefore that the
    /// rewrite is the thing the app sees.
    #[test]
    fn the_websocket_client_puts_one_of_its_own_headers_on_the_wire() {
        use tokio_tungstenite::tungstenite::{
            client::IntoClientRequest, handshake::client::generate_request,
        };

        let mut request = "ws://127.0.0.1:3000/ws"
            .into_client_request()
            .expect("a client request for the app");
        request
            .headers_mut()
            .insert(header::HOST, HeaderValue::from_static("127.0.0.1:9999"));
        for name in [
            header::HOST,
            header::CONNECTION,
            header::UPGRADE,
            header::SEC_WEBSOCKET_VERSION,
            header::SEC_WEBSOCKET_KEY,
        ] {
            request
                .headers_mut()
                .append(name, HeaderValue::from_static("a-second-copy"));
        }

        let (written, key) = generate_request(request).expect("the client builds its request");
        let written = String::from_utf8(written).expect("an ASCII request line");
        let lowered = written.to_ascii_lowercase();

        for name in [
            "host:",
            "connection:",
            "upgrade:",
            "sec-websocket-version:",
            "sec-websocket-key:",
        ] {
            assert_eq!(
                lowered.matches(name).count(),
                1,
                "{name} appears more than once in what the client sends:\n{written}"
            );
        }
        assert!(
            lowered.contains("host: 127.0.0.1:9999\r\n"),
            "the authority comes from the header map the rewrite writes, not from the \
             URI the client dials:\n{written}"
        );
        assert!(
            !lowered.contains("a-second-copy"),
            "the second value under each of those names never arrives:\n{written}"
        );
        assert!(
            written.contains(&key),
            "the key it verifies the answer against is the key it sent:\n{written}"
        );
    }

    /// A plain navigation carries no `Origin` header at all, and a dev server with
    /// an origin check reads whatever it is handed. It gets the upstream's own
    /// origin — the value it would have seen from a request made to it directly.
    #[test]
    fn a_request_that_carried_no_origin_is_given_the_upstreams_own() {
        let config = PreviewConfig::from_ports(vec![3000]);
        let target = resolve_target("/preview/3000/", &config).expect("allowed");
        let outgoing = rewrite_request_headers(&headers(&[("Host", "127.0.0.1:8080")]), &target);

        assert_eq!(outgoing[header::ORIGIN], "http://127.0.0.1:3000");
        assert_eq!(outgoing[header::HOST], "127.0.0.1:3000");
    }

    #[test]
    fn a_referer_loses_the_prefix_and_keeps_the_query() {
        let target = target(3000);
        assert_eq!(
            rewrite_referer("http://127.0.0.1:8080/preview/3000/src/app.js?v=1", &target)
                .as_deref(),
            Some("http://127.0.0.1:3000/src/app.js?v=1")
        );
        assert_eq!(
            rewrite_referer("http://127.0.0.1:8080/preview/3000", &target).as_deref(),
            Some("http://127.0.0.1:3000/")
        );
        // A referer from somewhere else is not this preview's page; guessing would
        // hand the dev server a referrer it never issued.
        assert_eq!(
            rewrite_referer("not a url", &target).as_deref(),
            None,
            "no authority to split"
        );
    }

    #[test]
    fn a_cookie_is_scoped_to_its_own_preview() {
        let target = target(3000);
        assert_eq!(
            rewrite_set_cookie("sid=abc; HttpOnly; SameSite=Lax", &target),
            "sid=abc; HttpOnly; SameSite=Lax; Path=/preview/3000/"
        );
        assert_eq!(
            rewrite_set_cookie(
                "sid=abc; Path=/admin; Domain=chaos.example.test; Secure",
                &target
            ),
            "sid=abc; Path=/preview/3000/admin; Secure",
            "Domain widens the cookie to every subdomain this host has"
        );
        assert_eq!(
            rewrite_set_cookie("sid=abc; Path=/", &target),
            "sid=abc; Path=/preview/3000/"
        );
        assert_eq!(
            rewrite_set_cookie("sid=abc; Path=", &target),
            "sid=abc; Path=/preview/3000/",
            "an empty Path is the same claim as no Path"
        );
    }

    #[test]
    fn a_redirect_stays_under_the_prefix() {
        let target = target(3000);
        assert_eq!(rewrite_location("/login", &target), "/preview/3000/login");
        assert_eq!(
            rewrite_location("http://127.0.0.1:3000/login?next=%2Fa", &target),
            "/preview/3000/login?next=%2Fa"
        );
        assert_eq!(
            rewrite_location("http://127.0.0.1:3000", &target),
            "/preview/3000/"
        );
        // Already under the prefix, or somebody else's server: leave it alone.
        assert_eq!(
            rewrite_location("/preview/3000/keep", &target),
            "/preview/3000/keep"
        );
        assert_eq!(
            rewrite_location("https://example.com/docs", &target),
            "https://example.com/docs"
        );
    }

    #[test]
    fn response_headers_keep_every_value_and_rewrite_the_two_that_matter() {
        let target = target(3000);
        let rewritten = rewrite_response_headers(
            &headers(&[
                ("set-cookie", "a=1; Path=/"),
                ("set-cookie", "b=2"),
                ("location", "/login"),
                ("content-type", "text/html"),
                ("connection", "keep-alive"),
                ("content-encoding", "br"),
            ]),
            &target,
        );
        let cookies: Vec<&str> = rewritten
            .iter()
            .filter(|(name, _)| name == header::SET_COOKIE)
            .map(|(_, value)| value.as_str())
            .collect();
        assert_eq!(
            cookies,
            vec!["a=1; Path=/preview/3000/", "b=2; Path=/preview/3000/"],
            "dropping the second cookie would silently lose a session"
        );
        assert!(
            rewritten
                .iter()
                .any(|(name, value)| name == header::LOCATION && value == "/preview/3000/login")
        );
        assert!(
            rewritten
                .iter()
                .any(|(name, value)| name == header::CONTENT_ENCODING && value == "br"),
            "the body is replayed byte for byte, so its encoding has to travel with it"
        );
        assert!(
            !rewritten.iter().any(|(name, _)| name == header::CONNECTION),
            "a hop-by-hop header describes the wrong connection"
        );
    }

    #[test]
    fn an_upgrade_is_recognised_by_its_two_tokens_not_by_a_substring() {
        assert!(is_websocket_upgrade(&headers(&[
            ("connection", "keep-alive, Upgrade"),
            ("upgrade", "WebSocket"),
        ])));
        assert!(!is_websocket_upgrade(&headers(&[
            ("connection", "upgrade"),
            ("upgrade", "h2c"),
        ])));
        assert!(!is_websocket_upgrade(&headers(&[("upgrade", "websocket")])));
        // `Connection: close` mentioning neither token must not look like an
        // upgrade, and a bare `Upgrade` with no Connection token is refused too.
        assert!(!is_any_upgrade(&headers(&[("connection", "close")])));
        assert!(!is_any_upgrade(&headers(&[("upgrade", "websocket")])));
        assert!(is_any_upgrade(&headers(&[
            ("connection", "UPGRADE"),
            ("upgrade", "h2c"),
        ])));
    }

    #[test]
    fn only_the_machine_itself_may_ask_for_a_preview() {
        let config = PreviewConfig::from_ports(vec![3000]);
        assert!(
            guard(
                &headers(&[
                    ("Host", "127.0.0.1:8080"),
                    ("Origin", "http://127.0.0.1:8080")
                ]),
                &config
            )
            .is_none(),
            "the ordinary browser request to a loopback host goes through"
        );
        assert!(
            guard(&headers(&[("Host", "localhost:8080")]), &config).is_none(),
            "no Origin is a plain navigation, which is the point of a preview"
        );
        // Nobody declared a public origin in this process, so a public Host is
        // refused by the host-wide rule before the preview rule even applies.
        assert_eq!(
            code(
                guard(&headers(&[("Host", "chaos.example.test:8443")]), &config)
                    .expect("a public Host is refused")
            ),
            "host_not_allowed"
        );
        assert_eq!(
            code(
                guard(
                    &headers(&[
                        ("Host", "127.0.0.1:8080"),
                        ("Origin", "http://evil.example"),
                    ]),
                    &config,
                )
                .expect("a foreign page is refused")
            ),
            "origin_not_allowed"
        );
        // A Host that is not even an authority cannot be shown to be loopback.
        assert!(
            guard(&headers(&[("Host", "not an authority")]), &config).is_some(),
            "an unparseable Host is not proof of loopback"
        );
        assert!(!loopback_request(&headers(&[(
            "Host",
            "chaos.example.test"
        )])));
        assert!(loopback_request(&headers(&[("Host", "[::1]:8080")])));
    }

    #[test]
    fn a_subprotocol_is_only_echoed_back_if_the_browser_asked_for_it() {
        let offered = headers(&[("sec-websocket-protocol", "chaos-hmr, graphql-ws")]);
        assert_eq!(
            selected_subprotocol(
                &headers(&[("sec-websocket-protocol", "chaos-hmr")]),
                &offered
            )
            .as_deref(),
            Some("chaos-hmr")
        );
        assert_eq!(
            selected_subprotocol(
                &headers(&[("sec-websocket-protocol", "CHAOS-HMR")]),
                &offered
            )
            .as_deref(),
            Some("chaos-hmr"),
            "the comparison is case-insensitive per RFC 6455 §1.9, and what the
             browser offered is what gets echoed back"
        );
        assert_eq!(
            selected_subprotocol(&headers(&[("sec-websocket-protocol", "chat")]), &offered),
            None,
            "echoing a protocol nobody offered is a protocol violation"
        );
        assert_eq!(
            selected_subprotocol(&headers(&[]), &offered),
            None,
            "the dev server chose nothing"
        );
        assert_eq!(
            selected_subprotocol(&headers(&[("sec-websocket-protocol", "x")]), &headers(&[])),
            None,
            "the browser offered none"
        );
    }
}
