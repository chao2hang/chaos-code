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
use std::{net::SocketAddr, sync::Arc};
use tower_http::{cors::CorsLayer, limit::RequestBodyLimitLayer};

const MAX_REQUEST_BYTES: usize = 64 * 1024;
const DEV_ORIGINS: [&str; 2] = ["http://127.0.0.1:5173", "http://localhost:5173"];

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
    let state = Arc::new(WebState {
        engine,
        token: Arc::new(token.into()),
        safe_web_mode,
    });
    Router::new()
        .route("/health", get(health))
        .route("/api/handshake", get(handshake))
        .route("/api/sessions", post(create_session))
        .route("/ws", get(websocket))
        .layer(CorsLayer::new().allow_origin(DEV_ORIGINS.map(|origin| origin.parse().unwrap())))
        .layer(RequestBodyLimitLayer::new(MAX_REQUEST_BYTES))
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
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

fn origin_allowed(headers: &HeaderMap) -> bool {
    headers
        .get(header::ORIGIN)
        .map(|origin| DEV_ORIGINS.iter().any(|allowed| origin == *allowed))
        .unwrap_or(true)
}

fn host_allowed(headers: &HeaderMap) -> bool {
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return true;
    };
    let hostname = host.rsplit_once(':').map_or(host, |(name, _)| name);
    matches!(hostname, "127.0.0.1" | "localhost" | "[::1]" | "::1")
}

fn request_allowed(state: &WebState, headers: &HeaderMap) -> bool {
    host_allowed(headers) && origin_allowed(headers) && authorized(state, headers)
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
    if !request_allowed(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    secure_json(ServerMessage::Handshake {
        protocol_version: PROTOCOL_VERSION,
    })
}

async fn create_session(State(state): State<Arc<WebState>>, headers: HeaderMap) -> Response {
    if !request_allowed(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let event = state.engine.handle(ClientMessage::CreateSession {
        client_msg_id: uuid::Uuid::new_v4().to_string(),
    });
    secure_json(event)
}

async fn websocket(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !request_allowed(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
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
            | ClientMessage::UpdateSettings { .. }
            | ClientMessage::ListFiles { .. }
            | ClientMessage::ReadFile { .. }
            | ClientMessage::SearchFiles { .. }
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
                for event in engine.handle(message) {
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
    let token = std::env::var("CHAOS_WEB_TOKEN").unwrap_or_default();
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).await?;
    axum::serve(
        listener,
        router_with_safe_mode(engine, token, safe_web_mode),
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
    #[tokio::test]
    async fn origin_is_checked() {
        let response = router(Engine::new(), "")
            .oneshot(
                Request::get("/api/handshake")
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
                    .header("host", "evil.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn health_is_public() {
        let response = router(Engine::new(), "secret")
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
