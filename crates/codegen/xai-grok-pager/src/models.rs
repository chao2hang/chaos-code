//! `chaos models` 子命令。

use anyhow::Result;
use tokio_util::sync::CancellationToken;
use xai_grok_shell::agent::config::Config as AgentConfig;
use xai_grok_shell::cli_models::{AuthStatus, list_models};

use crate::client_identity::{PAGER_CLIENT_TYPE, PAGER_CLIENT_VERSION};

pub async fn list_available_models(agent_config: &AgentConfig) -> Result<()> {
    match AuthStatus::resolve(agent_config) {
        AuthStatus::ApiKey => println!("当前使用 XAI_API_KEY。"),
        AuthStatus::LoggedIn(host) => println!("已登录到 {host}。"),
        AuthStatus::ModelCredentials(model) => {
            println!("模型 '{model}' 正在使用独立 API Key。");
        }
        AuthStatus::DeploymentKey => println!("当前使用部署密钥认证。"),
        AuthStatus::NotAuthenticated => println!("当前未认证。"),
    }
    println!();

    let cancel = CancellationToken::new();
    let spawned = crate::acp::spawn::spawn_grok_shell(agent_config.clone(), &cancel, None).await?;
    let _agent_guard =
        crate::acp::spawn::AgentShutdownGuard::new(cancel.clone(), Some(spawned.thread_handle));

    let state = list_models(&spawned.channel.tx, PAGER_CLIENT_TYPE, PAGER_CLIENT_VERSION).await?;

    println!("默认模型：{}", state.current_model_id.0);
    println!();
    println!("可用模型：");
    for m in state.available_models {
        if m.model_id == state.current_model_id {
            println!("  * {}（默认）", m.model_id.0);
        } else {
            println!("  - {}", m.model_id.0);
        }
    }

    Ok(())
}
