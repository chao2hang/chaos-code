use chaos_engine::{Engine, HeadlessProcessAdapter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let port = std::env::var("CHAOS_WEB_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8787);
    let workspace_root = std::env::var_os("CHAOS_WORKSPACE_ROOT").map(std::path::PathBuf::from);
    let adapter = std::env::var_os("CHAOS_AGENT_BINARY").map(|binary| {
        HeadlessProcessAdapter::new(
            binary,
            std::env::var_os("CHAOS_AGENT_CWD").unwrap_or_else(|| ".".into()),
        )
    });
    let sqlite_path = std::env::var_os("CHAOS_WEB_SQLITE").map(std::path::PathBuf::from);
    let engine = match (workspace_root, sqlite_path) {
        (None, Some(path)) => Engine::with_sqlite_store(path)?,
        (Some(_), Some(_)) => {
            anyhow::bail!("CHAOS_WEB_SQLITE and CHAOS_WORKSPACE_ROOT cannot be used together")
        }
        (Some(root), None) => Engine::with_workspace_and_adapter(
            root,
            adapter.map(|value| {
                std::sync::Arc::new(value) as std::sync::Arc<dyn chaos_engine::PromptAdapter>
            }),
        )?,
        (None, None) => match std::env::var("CHAOS_WEB_STATE") {
            Ok(path) => Engine::with_persistence_and_adapter(
                path,
                adapter.map(|value| {
                    std::sync::Arc::new(value) as std::sync::Arc<dyn chaos_engine::PromptAdapter>
                }),
            )?,
            Err(_) => adapter.map_or_else(Engine::new, Engine::with_adapter),
        },
    };
    eprintln!("Chaos Web listening on http://127.0.0.1:{port}");
    xai_grok_web::serve_loopback_with_safe_mode(
        engine,
        port,
        std::env::var("CHAOS_SAFE_WEB_MODE")
            .map(|value| value != "0" && value != "false")
            .unwrap_or(false),
    )
    .await
}
