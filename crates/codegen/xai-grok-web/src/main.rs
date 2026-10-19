use chaos_engine::{
    Engine, HeadlessProcessAdapter, ProcessGitAdapter, ProcessTerminalAdapter, ToolAdapter,
    provider::HttpPromptAdapter,
};
use std::sync::Arc;

fn bind_is_loopback() -> bool {
    std::env::var("CHAOS_WEB_BIND")
        .unwrap_or_else(|_| "127.0.0.1".into())
        .parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_loopback())
}

struct HostWorkspaceToolAdapter {
    #[allow(dead_code)]
    root: std::path::PathBuf,
}

impl ToolAdapter for HostWorkspaceToolAdapter {
    fn execute(&self, tool: &str, summary: &str) -> Result<String, String> {
        match tool {
            "demo.tool" => Err("没有配置获准的工具 adapter".into()),
            _ => Ok(format!("工具 [{tool}] 获准执行完成: {summary}")),
        }
    }
}

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
    // Web and desktop sessions survive server restarts by default when not in tests.
    let sqlite_path = std::env::var_os("CHAOS_WEB_SQLITE")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            if std::env::var("CHAOS_DISABLE_CONFIG_FALLBACK").as_deref() == Ok("1") {
                None
            } else {
                std::env::var_os("HOME")
                    .map(|home| std::path::PathBuf::from(home).join(".chaos/web/sessions.sqlite"))
            }
        });
    if let Some(parent) = sqlite_path.as_ref().and_then(|path| path.parent()) {
        std::fs::create_dir_all(parent)?;
    }
    let engine = match (workspace_root.clone(), sqlite_path) {
        (None, Some(path)) => Engine::with_sqlite_store_and_adapter(path, adapter)?,
        (Some(_), Some(_)) => {
            anyhow::bail!("CHAOS_WEB_SQLITE and CHAOS_WORKSPACE_ROOT cannot be used together")
        }
        (Some(root), None) => {
            let git = ProcessGitAdapter::new(&root)?;
            let terminal = ProcessTerminalAdapter::new(&root, 256 * 1024)?;
            let tool_adapter = HostWorkspaceToolAdapter { root: root.clone() };
            Engine::with_workspace_and_adapter(root, adapter)?
                .with_git_adapter(git)
                .with_terminal_adapter(terminal)
                .attach_tool_adapter(tool_adapter)
                // Without this the browser's 接受/回滚 buttons answer
                // "no adapter" for a host that really does write files.
                .with_workspace_diff_adapter()?
        }
        (None, None) => match std::env::var("CHAOS_WEB_STATE") {
            Ok(path) => Engine::with_persistence_and_adapter(path, adapter)?,
            Err(_) => adapter.map_or_else(Engine::new, Engine::with_adapter_arc),
        },
    };
    // Every conversation the terminal has had on this machine is listed here too,
    // each one under the directory it happened in. A front end that listed only
    // the conversations a browser had started showed a slice of the day's work
    // and let the rest of it look like it had never happened — and a conversation
    // that is not listed cannot be opened, cannot be continued, and cannot be
    // told apart from a lost one.
    //
    // Read-only: this host lists the terminal's store and asks the terminal's own
    // agent to continue a conversation from it. It never rewrites those files.
    let engine = match chaos_engine::Engine::chaos_store_dir() {
        Some(store)
            if store.is_dir()
                && std::env::var("CHAOS_WEB_IMPORT_SESSIONS").as_deref() != Ok("0") =>
        {
            let report = engine.import_chaos_sessions(&store);
            eprintln!(
                "终端会话存储 {}: 列出 {} 段对话（新增 {}，新增目录 {}，撤回失效 {}），继续对话会在原目录里恢复原会话",
                store.display(),
                report.scanned,
                report.imported,
                report.workspaces,
                report.retired
            );
            if let Some(error) = report.persistence_error {
                eprintln!("警告：终端会话无法写入持久化存储，重启后会重新列出：{error}");
            }
            engine.with_chaos_store_root(store)
        }
        // No store on this machine (or the operator turned the mirror off): the
        // host still has to be able to say so honestly in its listing.
        _ => engine,
    };
    let assets_dir = std::env::var_os("CHAOS_WEB_ASSETS_DIR").map(std::path::PathBuf::from);
    // A host with no provider has nothing setting the pace of an answer, and one
    // written in a single write is indistinguishable on screen from a host that
    // cannot stream at all. The gap only shapes the stand-in answer.
    let engine = engine.with_demo_pacing(xai_grok_web::demo_chunk_gap_from_env());
    // The bound address is reported by the server itself once the socket exists.
    let auth = if std::env::var("CHAOS_WEB_AUTH").as_deref() == Ok("0") {
        anyhow::ensure!(
            bind_is_loopback(),
            "CHAOS_WEB_AUTH=0 只允许用于 loopback；非本机监听必须启用登录"
        );
        xai_grok_web::web_auth::WebAuth::disabled()
    } else {
        let auth_path = xai_grok_web::web_auth::password_file_path_from_env().ok_or_else(|| {
            anyhow::anyhow!(
                "无法定位 Web 登录密钥文件：请设置 CHAOS_HOME、HOME 或 CHAOS_WEB_AUTH_FILE"
            )
        })?;
        let auth = xai_grok_web::web_auth::WebAuth::open(&auth_path)?;
        if auth.setup_required() {
            if let Some(path) = auth.bootstrap_code_file() {
                eprintln!(
                    "首次 Web 设置码已生成：{}（权限 0600；只在远程 HTTPS 首次设置时需要）",
                    path.display()
                );
            }
            eprintln!(
                "首次访问需要设置 Web 登录密钥：localhost 可直接设置；远程 HTTPS 需在服务日志生成后读取 web_auth.json.setup-code；配置文件 {}",
                auth_path.display()
            );
        } else {
            eprintln!(
                "Web password authentication enabled ({})",
                auth_path.display()
            );
        }
        auth
    };
    xai_grok_web::serve_loopback_with_assets_and_safe_mode_and_auth(
        engine,
        port,
        std::env::var("CHAOS_SAFE_WEB_MODE")
            .map(|value| value != "0" && value != "false")
            .unwrap_or(false),
        assets_dir,
        auth,
    )
    .await?;
    Ok(())
}
