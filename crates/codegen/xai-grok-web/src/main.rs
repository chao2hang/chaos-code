use chaos_engine::Engine;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let port = std::env::var("CHAOS_WEB_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8787);
    let engine = match std::env::var("CHAOS_WEB_STATE") {
        Ok(path) => Engine::with_persistence(path)?,
        Err(_) => Engine::new(),
    };
    eprintln!("Chaos Web listening on http://127.0.0.1:{port}");
    xai_grok_web::serve_loopback(engine, port).await
}
