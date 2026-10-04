use chaos_engine::{
    Engine, HeadlessProcessAdapter, ProcessGitAdapter, ProcessTerminalAdapter,
    provider::HttpPromptAdapter,
};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let port = std::env::var("CHAOS_WEB_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8787);
    let workspace_root = std::env::var_os("CHAOS_WORKSPACE_ROOT").map(std::path::PathBuf::from);
    // A real Agent binary wins over the HTTP provider; a bad provider
    // configuration aborts startup instead of quietly serving the demo echo,
    // because the operator asked for real inference.
    let adapter: Option<Arc<dyn chaos_engine::PromptAdapter>> =
        match std::env::var_os("CHAOS_AGENT_BINARY") {
            Some(binary) => Some(Arc::new(HeadlessProcessAdapter::new(
                binary,
                std::env::var_os("CHAOS_AGENT_CWD").unwrap_or_else(|| ".".into()),
            ))),
            None => match HttpPromptAdapter::from_env() {
                Ok(Some(adapter)) => {
                    let adapter = Arc::new(adapter);
                    // The health check blocks, and `main` runs inside the
                    // server runtime, so it goes on its own thread.
                    let prober = Arc::clone(&adapter);
                    let health = std::thread::spawn(move || prober.probe()).join();
                    match health {
                        Ok(health) if health.reachable => eprintln!(
                            "Provider {} ready: {} model(s) listed, configured model {} is {}",
                            adapter.chat_endpoint(),
                            health.model_ids.len(),
                            adapter.model(),
                            if health.configured_model_known {
                                "listed"
                            } else {
                                "NOT listed"
                            },
                        ),
                        Ok(health) => eprintln!(
                            "Provider {} not reachable: {}",
                            adapter.chat_endpoint(),
                            health.detail.unwrap_or_else(|| "no detail".into()),
                        ),
                        Err(_) => eprintln!(
                            "Provider {} health check thread failed",
                            adapter.chat_endpoint()
                        ),
                    }
                    Some(adapter)
                }
                Ok(None) => None,
                Err(error) => anyhow::bail!("Provider 配置无效：{error}"),
            },
        };
    let sqlite_path = std::env::var_os("CHAOS_WEB_SQLITE").map(std::path::PathBuf::from);
    let engine = match (workspace_root.clone(), sqlite_path) {
        (None, Some(path)) => Engine::with_sqlite_store_and_adapter(path, adapter)?,
        (Some(_), Some(_)) => {
            anyhow::bail!("CHAOS_WEB_SQLITE and CHAOS_WORKSPACE_ROOT cannot be used together")
        }
        (Some(root), None) => {
            let git = ProcessGitAdapter::new(&root)?;
            let terminal = ProcessTerminalAdapter::new(&root, 256 * 1024)?;
            Engine::with_workspace_and_adapter(root, adapter)?
                .with_git_adapter(git)
                .with_terminal_adapter(terminal)
                // Without this the browser's 接受/回滚 buttons answer
                // "no adapter" for a host that really does write files.
                .with_workspace_diff_adapter()?
        }
        (None, None) => match std::env::var("CHAOS_WEB_STATE") {
            Ok(path) => Engine::with_persistence_and_adapter(path, adapter)?,
            Err(_) => adapter.map_or_else(Engine::new, Engine::with_adapter_arc),
        },
    };
    let assets_dir = std::env::var_os("CHAOS_WEB_ASSETS_DIR").map(std::path::PathBuf::from);
    // The bound address is reported by the server itself once the socket exists.
    xai_grok_web::serve_loopback_with_assets_and_safe_mode(
        engine,
        port,
        std::env::var("CHAOS_SAFE_WEB_MODE")
            .map(|value| value != "0" && value != "false")
            .unwrap_or(false),
        assets_dir,
    )
    .await?;
    Ok(())
}
