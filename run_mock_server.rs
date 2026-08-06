use std::net::SocketAddr;

use axum::{
    extract::Request,
    routing::post,
    Router,
};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let app = Router::new().route("/v1/chat/completions", post(capture_headers));

    let addr = SocketAddr::from(([127, 0, 0, 1], 8787));
    let listener = TcpListener::bind(addr).await?;

    println!("Listening on http://{}", addr);
    axum::serve(listener, app).await
}

async fn capture_headers(request: Request) -> &'static str {
    println!("=== CAPTURED FULL REQUEST ===");
    println!("\nHEADERS:");
    for (name, value) in request.headers() {
        if let Ok(v) = value.to_str() {
            // 对于可能敏感的字段，我们可以打码处理
            let display_value = if name.as_str().eq_ignore_ascii_case("x-api-key") 
                || name.as_str().eq_ignore_ascii_case("authorization")
            {
                format!("{}...", &v[..v.len().min(8)])
            } else {
                v.to_string()
            };
            println!("{}: {}", name, display_value);
        }
    }

    let body = request.into_body();
    let bytes = axum::body::to_bytes(body, 1024 * 1024).await.unwrap();
    let body_str = String::from_utf8_lossy(&bytes);

    println!("\nBODY:");
    println!("{}", body_str);
    println!("=== END CAPTURED REQUEST ===\n");

    // 返回一个简单的响应
    r#"{
        "id": "chatcmpl-123",
        "object": "chat.completion.chunk",
        "created": 1722660856,
        "model": "gpt-5.6-luna",
        "choices": [
            {
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "content": "Hello, world! This is a mock response."
                },
                "finish_reason": null
            }
        ]
    }"#
}