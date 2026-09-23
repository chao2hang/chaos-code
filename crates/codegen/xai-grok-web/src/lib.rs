use axum::{
    Json, Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chaos_engine::{ClientMessage, Engine, PROTOCOL_VERSION, ServerMessage};
use serde::Deserialize;
use std::{net::SocketAddr, sync::Arc};
use tower_http::cors::CorsLayer;

#[derive(Clone)]
pub struct WebState {
    pub engine: Engine,
    pub token: Arc<String>,
}
#[derive(Deserialize)]
pub struct TokenQuery {
    token: Option<String>,
}

pub fn router(engine: Engine, token: impl Into<String>) -> Router {
    let state = Arc::new(WebState {
        engine,
        token: Arc::new(token.into()),
    });
    Router::new()
        .route("/health", get(health))
        .route("/api/handshake", get(handshake))
        .route("/api/sessions", post(create_session))
        .route("/ws", get(websocket))
        .layer(CorsLayer::very_permissive())
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status":"ok"}))
}
fn authorized(state: &WebState, token: Option<&str>) -> bool {
    state.token.is_empty() || token == Some(state.token.as_str())
}
async fn handshake(
    State(state): State<Arc<WebState>>,
    query: axum::extract::Query<TokenQuery>,
) -> Response {
    if !authorized(&state, query.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    (
        [(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )],
        Json(ServerMessage::Handshake {
            protocol_version: PROTOCOL_VERSION,
        }),
    )
        .into_response()
}
async fn create_session(
    State(state): State<Arc<WebState>>,
    query: axum::extract::Query<TokenQuery>,
) -> Response {
    if !authorized(&state, query.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let event = state.engine.handle(ClientMessage::CreateSession {
        client_msg_id: uuid::Uuid::new_v4().to_string(),
    });
    Json(event).into_response()
}
async fn websocket(
    State(state): State<Arc<WebState>>,
    query: axum::extract::Query<TokenQuery>,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !authorized(&state, query.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let engine = state.engine.clone();
    upgrade.on_upgrade(move |socket| websocket_session(socket, engine))
}
async fn websocket_session(mut socket: WebSocket, engine: Engine) {
    let _ = socket
        .send(Message::Text(
            serde_json::to_string(&ServerMessage::Handshake {
                protocol_version: PROTOCOL_VERSION,
            })
            .unwrap()
            .into(),
        ))
        .await;
    while let Some(Ok(Message::Text(text))) = socket.recv().await {
        match serde_json::from_str::<ClientMessage>(&text) {
            Ok(message) => {
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
    let token = std::env::var("CHAOS_WEB_TOKEN").unwrap_or_default();
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).await?;
    axum::serve(listener, router(engine, token)).await?;
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
    async fn protected_handshake_rejects_without_token() {
        let response = router(Engine::new(), "secret")
            .oneshot(Request::get("/api/handshake").body(Body::empty()).unwrap())
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
