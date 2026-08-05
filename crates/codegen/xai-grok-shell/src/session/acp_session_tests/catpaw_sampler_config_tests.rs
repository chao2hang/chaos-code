use super::support::*;
use super::*;
use std::sync::Arc;
use tokio::sync::mpsc;

/// CatPaw channel metadata must survive the model-switch → chat-state →
/// reconstruct path.
///
/// Regression: `reconstruct_full_config` rebuilt the per-turn sampler config
/// from the chat-state `xai_grok_sampling_types::SamplingConfig`, which did
/// not carry the CatPaw channel data — every live CatPaw-backed turn failed
/// with `Internal error: "invalid client configuration: CatPaw backend
/// requires SamplerConfig.catpaw"`.
#[tokio::test(flavor = "current_thread")]
async fn catpaw_channel_survives_model_switch_and_reconstruct() {
    use xai_grok_sampler::config::CatPawSamplerConfig;

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, _) = mpsc::unbounded_channel();
            let (persistence_tx, _persistence_rx) = mpsc::unbounded_channel();
            let actor = Arc::new(
                create_test_actor(50_000, 100_000, 85, gateway_tx, persistence_tx).await,
            );

            let sampler_cfg = xai_grok_sampler::SamplerConfig {
                api_key: None,
                base_url: "https://catpaw.meituan.com".to_string(),
                model: "glm-5.2".to_string(),
                max_completion_tokens: None,
                temperature: None,
                top_p: None,
                api_backend: crate::sampling::ApiBackend::CatPaw,
                auth_scheme: Default::default(),
                extra_headers: Default::default(),
                query_params: Default::default(),
                env_http_headers: Default::default(),
                context_window: 256_000,
                client_version: None,
                force_http1: false,
                max_retries: None,
                stream_tool_calls: false,
                extract_inline_thinking: false,
                idle_timeout_secs: None,
                client_identifier: None,
                reasoning_effort: None,
                deployment_id: None,
                user_id: None,
                origin_client: None,
                attribution_callback: None,
                bearer_resolver: None,
                supports_backend_search: false,
                compactions_remaining: None,
                compaction_at_tokens: None,
                doom_loop_recovery: None,
                header_injector: None,
                user_agent: None,
                is_workbuddy: false,
                catpaw: Some(CatPawSamplerConfig {
                    provider: "catpaw".to_string(),
                    model_type_code: 75,
                    account_resolver: None,
                }),
                remote_agent: None,
            };

            let _ = actor
                .handle_set_session_model(sampler_cfg, false, false, true, 85)
                .await;

            // 1) chat-state 保留了 CatPaw 通道元数据。
            let stored = actor
                .chat_state_handle
                .get_sampling_config()
                .await
                .expect("sampling config stored after model switch");
            assert_eq!(stored.api_backend, crate::sampling::ApiBackend::CatPaw);
            let channel = stored.catpaw.expect("catpaw channel persisted in chat state");
            assert_eq!(channel.provider, "catpaw");
            assert_eq!(channel.model_type_code, 75);

            // 2) reconstruct 重建出带 account resolver 的 sampler 配置。
            let rebuilt = actor.reconstruct_full_config().await;
            assert_eq!(rebuilt.api_backend, crate::sampling::ApiBackend::CatPaw);
            let cp = rebuilt.catpaw.expect("rebuilt catpaw config must be present");
            assert_eq!(cp.model_type_code, 75);
            assert!(
                cp.account_resolver.is_some(),
                "reconstruct must re-attach the account resolver"
            );
        })
        .await;
}
