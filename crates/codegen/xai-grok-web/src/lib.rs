use axum::{Json, Router, extract::State, routing::get};
use chaos_engine::{Engine, PROTOCOL_VERSION, ServerMessage};
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;

#[derive(Clone)]
pub struct WebState {
    pub engine: Engine,
}

pub fn router(engine: Engine) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/handshake", get(handshake))
        .with_state(Arc::new(WebState { engine }))
}

async fn health() -> Json<Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn handshake(State(state): State<Arc<WebState>>) -> Json<ServerMessage> {
    let _ = &state.engine;
    Json(ServerMessage::Handshake {
        protocol_version: PROTOCOL_VERSION,
    })
}

pub async fn serve_loopback(engine: Engine, port: u16) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).await?;
    axum::serve(listener, router(engine)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        response::Response,
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_is_loopback_ready() {
        let response: Response<Body> = router(Engine::new())
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
