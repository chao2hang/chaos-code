use chaos_engine::{Engine, HeadlessProcessAdapter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let port = std::env::var("CHAOS_WEB_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8787);
    let adapter = std::env::var_os("CHAOS_AGENT_BINARY").map(|binary| {
        HeadlessProcessAdapter::new(
            binary,
            std::env::var_os("CHAOS_AGENT_CWD").unwrap_or_else(|| ".".into()),
        )
    });
    let engine = match std::env::var("CHAOS_WEB_STATE") {
        Ok(path) => Engine::with_persistence_and_adapter(
            path,
            adapter.map(|value| {
                std::sync::Arc::new(value) as std::sync::Arc<dyn chaos_engine::PromptAdapter>
            }),
        )?,
        Err(_) => adapter.map_or_else(Engine::new, Engine::with_adapter),
    };
    eprintln!("Chaos Web listening on http://127.0.0.1:{port}");
    xai_grok_web::serve_loopback(engine, port).await
}
