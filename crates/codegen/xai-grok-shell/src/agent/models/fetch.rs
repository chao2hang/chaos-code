use super::*;

// ── Fetch ───────────────────────────────────────────────────────────────────

/// Build the prefetched model map from a flat list of entries.
pub(crate) fn build_prefetched_map(
    models: Vec<config::ModelEntryConfig>,
    api_base_url_override: Option<String>,
) -> IndexMap<String, ModelEntry> {
    let mut map: IndexMap<String, ModelEntry> = IndexMap::with_capacity(models.len());
    for m in models {
        let key = m.id.clone().unwrap_or_else(|| m.model.clone());
        let info = config::ModelInfo::from_config(&m);
        let entry = ModelEntry {
            info,
            api_key: None,
            env_key: None,
            auth_provider: None,
            api_base_url: m.api_base_url.clone().or(api_base_url_override.clone()),
        };
        map.insert(key, entry);
    }
    map
}

/// Fetch remote models. Checks disk cache first; persists after fetch.
pub(crate) fn prefetch_models_blocking(
    endpoints: &config::EndpointsConfig,
    auth: Option<&GrokAuth>,
    fetch_auth: ModelFetchAuth,
) -> Option<IndexMap<String, ModelEntry>> {
    prefetch_models_blocking_gated(
        endpoints,
        auth,
        fetch_auth,
        crate::util::config::resolve_remote_fetch_enabled(),
    )
}

/// Blocking models + `/v1/settings` prefetch pair, shared by the early
pub(crate) fn prefetch_models_and_settings_blocking(
    endpoints: &config::EndpointsConfig,
    auth: Option<&GrokAuth>,
    fetch_auth: ModelFetchAuth,
) -> (
    Option<IndexMap<String, ModelEntry>>,
    Option<crate::util::config::RemoteSettings>,
) {
    let remote_fetch_enabled = crate::util::config::resolve_remote_fetch_enabled();
    let models = prefetch_models_blocking_gated(endpoints, auth, fetch_auth, remote_fetch_enabled);
    let settings = match auth {
        Some(auth) if remote_fetch_enabled => {
            let _timer = crate::instrumentation_timer!("startup.early_settings_fetch");
            // Chaos-fork: `fetch_settings_blocking` returns `Option<RemoteSettings>`
            // directly here; upstream wraps in a `SettingsFetch` enum with an
            // `.into_option()` accessor. Keep the plain-Option shape and skip it.
            crate::remote::fetch_settings_blocking(
                &endpoints.proxy_url(),
                auth,
                endpoints.alpha_test_key.as_deref(),
            )
        }
        _ => None,
    };
    (models, settings)
}

/// `remote_fetch_enabled` is a parameter so the pair helper above resolves the knob once for both halves.
fn prefetch_models_blocking_gated(
    endpoints: &config::EndpointsConfig,
    auth: Option<&GrokAuth>,
    fetch_auth: ModelFetchAuth,
    remote_fetch_enabled: bool,
) -> Option<IndexMap<String, ModelEntry>> {
    let cache_auth = fetch_auth.cache_auth_method();
    let cache_origin = crate::remote::models_list_url(endpoints, fetch_auth);
    let cache = ModelsCacheManager::new();
    if let Some(cached) = cache.load_fresh(&cache_auth, &cache_origin) {
        return Some(cached.models);
    }

    if !remote_fetch_enabled {
        tracing::info!("models fetch skipped: remote_fetch disabled");
        return None;
    }

    let _timer = crate::instrumentation_timer!("startup.fetch_models_blocking");
    match fetch_models_blocking(endpoints, auth, fetch_auth) {
        Ok(FetchModelsResult { models, etag }) if !models.is_empty() => {
            let api_base_url_override = match fetch_auth {
                ModelFetchAuth::ApiKey => Some(endpoints.xai_api_base_url.clone()),
                _ => None,
            };
            let map = build_prefetched_map(models, api_base_url_override);

            tracing::info!(count = map.len(), etag = ?etag, "Prefetched models");
            cache.persist(&map, etag.as_deref(), cache_auth, &cache_origin);
            Some(map)
        }
        Ok(FetchModelsResult { .. }) => {
            tracing::warn!("Models endpoint returned empty list");
            None
        }
        Err(e) => {
            tracing::warn!("Failed to fetch models: {:?}", e);
            None
        }
    }
}

/// Startup prefetch result: models + remote settings.
pub struct EarlyPrefetchResult {
    pub models: Option<IndexMap<String, ModelEntry>>,
    pub settings: Option<crate::util::config::RemoteSettings>,
}

/// Handle for a startup prefetch thread.
pub type EarlyPrefetchHandle = std::thread::JoinHandle<EarlyPrefetchResult>;

pub(crate) struct PrefetchEnv {
    pub(crate) auth: Option<GrokAuth>,
    pub(crate) endpoints: config::EndpointsConfig,
    pub(crate) model_fetch_auth: ModelFetchAuth,
}

/// Effective startup endpoints, resolved config-aware (not env-only) so the prefetch can't leak the bearer to api.x.ai.
fn resolve_startup_endpoints() -> config::EndpointsConfig {
    let mut endpoints = config::EndpointsConfig::from_effective_config();
    if endpoints.deployment_key.is_none() {
        endpoints.deployment_key = crate::managed_config::resolve_deployment_key();
    }
    endpoints
}

/// Decision core of the startup prefetch gate, split from the config loading
pub(crate) fn resolve_prefetch_env_from_parts(
    auth: Option<GrokAuth>,
    endpoints: config::EndpointsConfig,
    remote_fetch_enabled: bool,
) -> Option<PrefetchEnv> {
    if !remote_fetch_enabled {
        tracing::info!("startup model/settings prefetch skipped: remote_fetch disabled");
        return None;
    }

    let model_fetch_auth = ModelFetchAuth::resolve(&endpoints, auth.is_some());

    if auth.is_none()
        && !endpoints.has_custom_endpoint()
        && model_fetch_auth == ModelFetchAuth::Session
    {
        return None;
    }

    Some(PrefetchEnv {
        auth,
        endpoints,
        model_fetch_auth,
    })
}

/// Start model + settings prefetch on a background thread using pre-resolved auth.
///
/// Chaos-fork: the upstream `sync_managed` gate and the sibling
/// `start_early_prefetch_settings_only` API are dropped here — Chaos does not
/// run the managed-config kill-switch path, so there is no on-disk policy to
/// heal and nothing to fail closed against on cold start.
pub fn start_early_prefetch_with_auth(auth: Option<GrokAuth>) -> Option<EarlyPrefetchHandle> {
    let _timer = crate::instrumentation_timer!("startup.early_prefetch_launch");
    let endpoints = resolve_startup_endpoints();
    let env = resolve_prefetch_env_from_parts(
        auth,
        endpoints,
        crate::util::config::resolve_remote_fetch_enabled(),
    )?;
    Some(spawn_prefetch_thread(env))
}

/// Start model + settings prefetch on a background thread.
pub fn start_early_prefetch(grok_com_config: Option<GrokComConfig>) -> Option<EarlyPrefetchHandle> {
    let grok_home = crate::util::grok_home::grok_home();
    let auth = AuthManager::new(&grok_home, grok_com_config.unwrap_or_default()).current();
    start_early_prefetch_with_auth(auth)
}

fn spawn_prefetch_thread(env: PrefetchEnv) -> EarlyPrefetchHandle {
    std::thread::spawn(move || {
        let mut timer = crate::instrumentation_timer!("startup.early_prefetch");
        let proxy_endpoint = env.endpoints.proxy_url();
        timer.with_field("endpoint", proxy_endpoint.as_str());
        let (models, settings) = prefetch_models_and_settings_blocking(
            &env.endpoints,
            env.auth.as_ref(),
            env.model_fetch_auth,
        );
        EarlyPrefetchResult { models, settings }
    })
}
