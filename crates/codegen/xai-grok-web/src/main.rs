use chaos_engine::Engine;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let port = std::env::var("CHAOS_WEB_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8787);
    eprintln!("Chaos Web listening on http://127.0.0.1:{port}");
    xai_grok_web::serve_loopback(Engine::new(), port).await
}
