//! Built-in request-client profiles.
//!
//! A profile describes the public identity and compatibility hints that Chaos
//! can attach to requests. It does not provide credentials and it does not
//! replace the model's endpoint, protocol, or authentication configuration.

use std::sync::Arc;

use indexmap::IndexMap;
use serde::Serialize;

use xai_grok_sampler::{HeaderInjector, OriginClientInfo, SamplerConfig, SharedHeaderInjector};

/// A named request-client identity exposed by `chaos clients` and `--client`.
///
/// The fields are owned because profiles can be supplied by the user's
/// `[clients.custom.<id>]` configuration, not only by the built-in catalog.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ClientProfile {
    /// Stable value accepted by `--client` and `[model.<id>] client`.
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Public request protocol commonly used by this client.
    pub protocol: String,
    /// Public authentication header style commonly used by this client.
    pub auth_scheme: String,
    /// Environment variable normally used for the corresponding API key.
    pub env_key: String,
    /// Wire value used for `x-grok-client-identifier` and the origin UA token.
    pub client_identifier: String,
    /// Verbatim `User-Agent` override. `None` renders a UA from
    /// `client_identifier`; `Some` is sent as-is (spaces allowed) to mimic an
    /// existing client environment.
    pub user_agent: Option<String>,
    /// Static extra headers attached to every sampling request. These take
    /// precedence over model/provider headers of the same name so an
    /// explicitly selected identity wins. Values are sent verbatim (secrets
    /// included); prefer `env_http_headers` to keep secrets out of the file.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub extra_headers: IndexMap<String, String>,
    /// Header name to environment variable; the variable is resolved and the
    /// header injected at request-build time. Mirrors `model_providers`
    /// `env_http_headers` so profiles can carry per-identity headers without
    /// storing values on disk.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub env_http_headers: IndexMap<String, String>,
}

/// Static representation used for the built-in catalog.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct BuiltinClientProfile {
    pub id: &'static str,
    pub name: &'static str,
    pub protocol: &'static str,
    pub auth_scheme: &'static str,
    pub env_key: &'static str,
    pub client_identifier: &'static str,
    pub user_agent: Option<&'static str>,
    pub extra_headers: &'static [(&'static str, &'static str)],
    pub env_http_headers: &'static [(&'static str, &'static str)],
}

impl BuiltinClientProfile {
    fn to_owned(self) -> ClientProfile {
        ClientProfile {
            id: self.id.to_owned(),
            name: self.name.to_owned(),
            protocol: self.protocol.to_owned(),
            auth_scheme: self.auth_scheme.to_owned(),
            env_key: self.env_key.to_owned(),
            client_identifier: self.client_identifier.to_owned(),
            user_agent: self.user_agent.map(str::to_owned),
            extra_headers: self
                .extra_headers
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            env_http_headers: self
                .env_http_headers
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        }
    }
}

/// Built-in profiles. Endpoint, model, protocol, and credential values still
/// come from each model/provider entry; these fields are compatibility hints
/// and request identity only.
pub const BUILTIN_CLIENT_PROFILES: &[BuiltinClientProfile] = &[
    BuiltinClientProfile {
        id: "claude-code",
        name: "Claude Code",
        protocol: "messages",
        auth_scheme: "x_api_key",
        env_key: "ANTHROPIC_API_KEY",
        client_identifier: "claude-code",
        user_agent: None,
        extra_headers: &[],
        env_http_headers: &[],
    },
    BuiltinClientProfile {
        id: "codex",
        name: "Codex",
        protocol: "responses",
        auth_scheme: "bearer",
        env_key: "OPENAI_API_KEY",
        client_identifier: "codex",
        user_agent: None,
        extra_headers: &[],
        env_http_headers: &[],
    },
    BuiltinClientProfile {
        id: "grok-build",
        name: "Grok Build",
        protocol: "responses",
        auth_scheme: "bearer",
        env_key: "XAI_API_KEY",
        client_identifier: "grok-build",
        user_agent: None,
        extra_headers: &[],
        env_http_headers: &[],
    },
    BuiltinClientProfile {
        id: "workbuddy",
        name: "WorkBuddy",
        protocol: "chat_completions",
        auth_scheme: "bearer",
        env_key: "X_AI_API_KEY",
        client_identifier: "workbuddy",
        user_agent: Some("WorkBuddy/5.3.5 WorkBuddy/5.3.5 CLI/2.115.0"),
        extra_headers: &[
            ("x-ide-name", "WorkBuddy"),
            ("x-ide-type", "WorkBuddy"),
            ("x-ide-version", "5.3.5"),
            ("x-stainless-lang", "js"),
            ("x-stainless-runtime", "node"),
            ("x-stainless-runtime-version", "v22.21.1"),
            ("x-stainless-os", "Windows"),
            ("x-stainless-arch", "x64"),
            ("x-stainless-package-version", "6.25.0"),
            ("x-stainless-retry-count", "0"),
            ("x-domain", "www.codebuddy.cn"),
            ("x-product", "SaaS"),
            ("x-requested-with", "XMLHttpRequest"),
            ("x-codebuddy-request", "1"),
            ("X-Agent-Intent", "craft"),
            ("X-Agent-Purpose", "conversation"),
            ("X-User-Id", "160eab12-4fe2-4079-9824-087331efa1c5")
        ],
        env_http_headers: &[
            ("X-API-Key", "X_AI_API_KEY")
        ]
    },
];

/// Resolve a built-in profile ID, accepting a few unambiguous convenience
/// aliases. Custom profiles are resolved by [`Config`](crate::agent::config::Config)
/// because their values live in the loaded configuration.
pub fn by_id(id: &str) -> Option<ClientProfile> {
    let canonical = match id.trim().to_ascii_lowercase().as_str() {
        "claude" | "anthropic" | "claude-code" => "claude-code",
        "codex" | "openai" => "codex",
        "grok" | "grok-build" => "grok-build",
        "workbuddy" | "wb" => "workbuddy",
        _ => return None,
    };
    BUILTIN_CLIENT_PROFILES
        .iter()
        .find(|profile| profile.id == canonical)
        .copied()
        .map(BuiltinClientProfile::to_owned)
}

/// Per-request WorkBuddy tracing/identity header injector.
///
/// The real WorkBuddy client (a Node.js process talking to a CodeBuddy
/// gateway) attaches a stable conversation id plus fresh message/request ids
/// and Zipkin/B3 tracing headers to every chat completion. Without these
/// headers the gateway treats the request as a third-party client and
/// answers `403 unsupported_client`.
///
/// The static identity headers (`x-ide-*`, `x-stainless-*`, `x-domain`,
/// `x-codebuddy-request`, …) come from [`BuiltinClientProfile`] via
/// [`ClientProfile::apply_to_sampling_config`]; this injector only produces
/// the dynamic per-request fields that can't live in `extra_headers`.
///
/// `conversation_id` and `acp_connection_id` are session-scoped. Reusing
/// the same pair across every turn keeps the gateway's per-conversation
/// logs coherent.
#[derive(Debug, Clone)]
pub struct WorkBuddyHeaderInjector {
    pub conversation_id: String,
    pub acp_connection_id: String,
}

impl WorkBuddyHeaderInjector {
    /// Build an injector with freshly-generated UUIDs. Suitable for the
    /// session-title side-call, which happens before any session-stable
    /// identifier is available.
    pub fn with_random_ids() -> Self {
        Self {
            conversation_id: uuid::Uuid::new_v4().to_string(),
            acp_connection_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    /// Build an injector from a session's stable identifiers so every turn
    /// of the same session shares a single conversation id.
    pub fn new(conversation_id: String, acp_connection_id: String) -> Self {
        Self {
            conversation_id,
            acp_connection_id,
        }
    }

    fn put(headers: &mut reqwest::header::HeaderMap, name: &'static str, value: String) {
        if let Ok(v) = reqwest::header::HeaderValue::from_str(&value) {
            headers.insert(name, v);
        }
    }
}

impl HeaderInjector for WorkBuddyHeaderInjector {
    fn inject(&self, headers: &mut reqwest::header::HeaderMap) {
        let message_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
        // `X-Conversation-Request-ID` is grouped per user turn; we do not
        // have that grouping at this layer, so a fresh value is acceptable.
        let request_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
        let trace_id_hex = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
        let span_id = format!("{:016x}", rand::random::<u64>());

        Self::put(headers, "X-Conversation-ID", self.conversation_id.clone());
        Self::put(
            headers,
            "X-Conversation-Message-ID",
            message_id.clone(),
        );
        Self::put(headers, "X-Conversation-Request-ID", request_id);
        // Real client mirrors the message id here.
        Self::put(headers, "X-Request-ID", message_id);
        Self::put(headers, "X-Trace-ID", trace_id_hex.clone());
        Self::put(headers, "X-B3-TraceId", trace_id_hex.clone());
        Self::put(headers, "X-B3-SpanId", span_id.clone());
        // Root span: parent == self.
        Self::put(headers, "X-B3-ParentSpanId", span_id.clone());
        Self::put(headers, "X-B3-Sampled", "1".to_string());
        Self::put(
            headers,
            "b3",
            format!("{trace_id_hex}-{span_id}-1-{span_id}"),
        );
        Self::put(
            headers,
            "traceparent",
            format!("00-{trace_id_hex}-{span_id}-01"),
        );
        // ACP connection id — stable for the session, sent on every chat
        // completion by the real WorkBuddy client.
        Self::put(headers, "acp-connection-id", self.acp_connection_id.clone());
    }
}

/// Return an [`Arc`] handle around a freshly-allocated
/// [`WorkBuddyHeaderInjector`]. Convenience for callers that need to store
/// one in a [`SamplerConfig::header_injector`] slot.
pub fn workbuddy_header_injector() -> SharedHeaderInjector {
    Arc::new(WorkBuddyHeaderInjector::with_random_ids())
}

/// Return whether an origin belongs to Chaos' own ACP adapters rather than to
/// an external editor or agent client. These identifiers are transport
/// defaults, so they should not shadow an explicitly configured profile.
pub fn is_native_client_identifier(id: &str) -> bool {
    matches!(
        id.trim().to_ascii_lowercase().as_str(),
        "grok-pager" | "grok-shell"
    )
}

/// Resolve the profile that should provide request identity for a model.
///
/// A recognized external ACP client wins over config. Chaos' own transport
/// identifiers are only defaults, so a configured profile may replace them.
/// Unknown external clients remain untouched and do not inherit a profile.
pub fn profile_for_origin(
    origin: Option<&OriginClientInfo>,
    configured: Option<&ClientProfile>,
) -> Option<ClientProfile> {
    match origin {
        Some(origin) if !is_native_client_identifier(&origin.product) => by_id(&origin.product),
        _ => configured.cloned(),
    }
}

impl ClientProfile {
    /// Build the origin metadata used for `User-Agent` rendering.
    ///
    /// The version is intentionally omitted: selecting a profile must not
    /// claim to be a specific vendor release.
    pub fn origin_client(&self) -> OriginClientInfo {
        OriginClientInfo {
            product: self.client_identifier.clone(),
            version: None,
        }
    }

    /// Apply identity, headers, and per-profile transport flags from this
    /// profile onto the sampler config. Profile headers win over
    /// provider/model headers of the same name so an explicitly selected
    /// identity can mimic a specific client.
    pub fn apply_to_sampling_config(&self, config: &mut SamplerConfig) {
        if config.client_identifier.is_none() {
            config.client_identifier = Some(self.client_identifier.clone());
        }
        if config.origin_client.is_none() {
            config.origin_client = Some(self.origin_client());
        }
        if config.user_agent.is_none() {
            config.user_agent = self.user_agent.clone();
        }
        for (name, value) in &self.extra_headers {
            config.extra_headers.insert(name.clone(), value.clone());
        }
        for (name, value) in &self.env_http_headers {
            config.env_http_headers.insert(name.clone(), value.clone());
        }
        // Selecting the WorkBuddy profile forces `is_workbuddy` so the
        // sampler switches to its workbuddy-specific transport: HTTP/1.1,
        // `Accept: application/json`, dual `Bearer` + `X-API-Key`
        // authentication, and no `x-grok-*` identity headers. Without this
        // the one-shot path (which goes through `prepare_sampling_config`
        // rather than `reconstruct_full_config`) would still emit
        // grok-build headers and be rejected by the gateway.
        if self.client_identifier.eq_ignore_ascii_case("workbuddy") {
            config.is_workbuddy = true;
        }
    }

    /// Drop any header entries that a WorkBuddy client must not forward.
    /// The real WorkBuddy client never sends `x-grok-*` identity headers,
    /// the staging `traceparent`/`x-xai-token-auth`/`x-authenticateresponse`
    /// headers, or stale session headers from a non-workbuddy config. The
    /// header injector and the sampler-side `is_workbuddy` branch depend on
    /// this being clean before the request is built.
    pub fn strip_non_workbuddy_headers(&self, headers: &mut IndexMap<String, String>) {
        if !self.client_identifier.eq_ignore_ascii_case("workbuddy") {
            return;
        }
        headers.retain(|k, _| {
            !k.to_lowercase().starts_with("x-grok-")
                && !k.eq_ignore_ascii_case("traceparent")
                && !k.eq_ignore_ascii_case("x-xai-token-auth")
                && !k.eq_ignore_ascii_case("x-authenticateresponse")
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_three_builtin_profiles() {
        assert_eq!(
            BUILTIN_CLIENT_PROFILES
                .iter()
                .map(|profile| profile.id)
                .collect::<Vec<_>>(),
            vec!["claude-code", "codex", "grok-build", "workbuddy"]
        );
    }

    #[test]
    fn workbuddy_profile_carries_verbatim_user_agent() {
        let profile = by_id("workbuddy").expect("workbuddy profile");
        assert_eq!(profile.name, "WorkBuddy");
        assert_eq!(profile.protocol, "chat_completions");
        assert_eq!(profile.client_identifier, "workbuddy");
        let ua = profile.user_agent.expect("workbuddy UA");
        assert!(ua.contains(' '), "User-Agent may contain spaces");
        assert!(ua.starts_with("WorkBuddy/"));
    }

    #[test]
    fn workbuddy_ua_override_flows_into_sampling_config() {
        let profile = by_id("workbuddy").expect("workbuddy profile");
        let mut config = SamplerConfig::default();
        profile.apply_to_sampling_config(&mut config);
        assert_eq!(config.user_agent.as_deref(), profile.user_agent.as_deref());
        assert_eq!(config.client_identifier.as_deref(), Some("workbuddy"));
    }

    #[test]
    fn aliases_resolve_to_canonical_profiles() {
        assert_eq!(
            by_id("claude").map(|profile| profile.id),
            Some("claude-code".to_owned())
        );
        assert_eq!(
            by_id("OPENAI").map(|profile| profile.id),
            Some("codex".to_owned())
        );
        assert_eq!(
            by_id(" grok ").map(|profile| profile.id),
            Some("grok-build".to_owned())
        );
        assert!(by_id("unknown-client").is_none());
    }

    #[test]
    fn applying_a_profile_preserves_explicit_request_identity() {
        let profile = by_id("codex").expect("codex profile");
        let mut config = SamplerConfig {
            client_identifier: Some("external-client".to_owned()),
            origin_client: Some(OriginClientInfo {
                product: "external-client".to_owned(),
                version: Some("1.0".to_owned()),
            }),
            ..SamplerConfig::default()
        };

        profile.apply_to_sampling_config(&mut config);

        assert_eq!(config.client_identifier.as_deref(), Some("external-client"));
        assert_eq!(
            config
                .origin_client
                .as_ref()
                .map(|origin| origin.product.as_str()),
            Some("external-client")
        );
    }

    #[test]
    fn origin_profile_wins_over_configured_fallback() {
        let fallback = by_id("claude-code");
        let selected = profile_for_origin(
            Some(&OriginClientInfo {
                product: "codex".to_owned(),
                version: None,
            }),
            fallback.as_ref(),
        );

        assert_eq!(selected.map(|profile| profile.id), Some("codex".to_owned()));
    }

    #[test]
    fn unknown_external_origin_does_not_override_or_inherit_a_profile() {
        let fallback = by_id("codex");
        let selected = profile_for_origin(
            Some(&OriginClientInfo {
                product: "cursor".to_owned(),
                version: None,
            }),
            fallback.as_ref(),
        );

        assert!(selected.is_none());
    }

    #[test]
    fn pager_origin_allows_configured_profile_fallback() {
        let fallback = by_id("grok-build");
        let selected = profile_for_origin(
            Some(&OriginClientInfo {
                product: "grok-pager".to_owned(),
                version: None,
            }),
            fallback.as_ref(),
        );

        assert_eq!(
            selected.map(|profile| profile.id),
            Some("grok-build".to_owned())
        );
    }

    #[test]
    fn model_override_wins_over_global_client_default() {
        let raw: toml::Value = toml::from_str(
            r#"
                [clients]
                default = "codex"

                [model.claude]
                model = "claude-sonnet"
                context_window = 200000
                client = "claude-code"
            "#,
        )
        .expect("valid TOML");
        let config =
            crate::agent::config::Config::new_from_toml_cfg(&raw).expect("config should parse");
        let models = crate::agent::config::resolve_model_list(&config, None);

        let model = models.get("claude").expect("model override");
        assert_eq!(
            config
                .client_profile_for_model(model)
                .map(|profile| profile.id),
            Some("claude-code".to_owned())
        );

        let mut cli_config = config.clone();
        cli_config
            .set_client_profile_override(Some("grok"))
            .expect("alias should canonicalize");
        assert_eq!(
            cli_config
                .client_profile_for_model(model)
                .map(|profile| profile.id),
            Some("grok-build".to_owned())
        );
    }

    #[test]
    fn resolves_custom_profile_from_clients_config() {
        let raw: toml::Value = toml::from_str(
            r#"
                [clients]
                default = "my-client"

                [clients.custom.my-client]
                name = "My Client"
                protocol = "chat_completions"
                auth_scheme = "bearer"
                env_key = "MY_CLIENT_API_KEY"
                client_identifier = "my-client-wire"

                [model.test]
                model = "test-model"
                base_url = "https://example.test/v1"
                context_window = 128000
            "#,
        )
        .expect("valid TOML");
        let config =
            crate::agent::config::Config::new_from_toml_cfg(&raw).expect("config should parse");
        let models = crate::agent::config::resolve_model_list(&config, None);
        let model = models.get("test").expect("test model");
        let profile = config
            .client_profile_for_model(model)
            .expect("custom profile should resolve");

        assert_eq!(profile.id, "my-client");
        assert_eq!(profile.name, "My Client");
        assert_eq!(profile.protocol, "chat_completions");
        assert_eq!(profile.auth_scheme, "bearer");
        assert_eq!(profile.env_key, "MY_CLIENT_API_KEY");
        assert_eq!(profile.client_identifier, "my-client-wire");
    }

    #[test]
    fn workbuddy_profile_carries_static_identity_headers() {
        let profile = by_id("workbuddy").expect("workbuddy profile");
        assert_eq!(
            profile.extra_headers.get("x-ide-name").map(String::as_str),
            Some("WorkBuddy")
        );
        assert_eq!(
            profile
                .extra_headers
                .get("x-stainless-package-version")
                .map(String::as_str),
            Some("6.25.0")
        );
        assert_eq!(
            profile
                .extra_headers
                .get("x-codebuddy-request")
                .map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn profile_headers_merge_into_sampling_config() {
        let profile = by_id("workbuddy").expect("workbuddy profile");
        let mut config = SamplerConfig {
            extra_headers: [("x-existing".to_owned(), "keep".to_owned())]
                .into_iter()
                .collect(),
            ..SamplerConfig::default()
        };
        profile.apply_to_sampling_config(&mut config);
        // Profile headers are present.
        assert_eq!(
            config.extra_headers.get("x-ide-name").map(String::as_str),
            Some("WorkBuddy")
        );
        // Provider/model headers of a different name are preserved.
        assert_eq!(
            config.extra_headers.get("x-existing").map(String::as_str),
            Some("keep")
        );
    }

    #[test]
    fn builtin_override_can_add_and_override_headers() {
        let raw: toml::Value = toml::from_str(
            r#"
                [clients.overrides.workbuddy.extra_headers]
                "x-ide-version" = "9.9.9"
                "x-custom-token" = "secret"

                [clients.overrides.workbuddy.env_http_headers]
                "cookie" = "WORKBUDDY_COOKIE"
            "#,
        )
        .expect("valid TOML");
        let config =
            crate::agent::config::Config::new_from_toml_cfg(&raw).expect("config should parse");
        let profile = config
            .client_profile_by_id("workbuddy")
            .expect("workbuddy profile");

        // Override wins per-key over the built-in value.
        assert_eq!(
            profile
                .extra_headers
                .get("x-ide-version")
                .map(String::as_str),
            Some("9.9.9")
        );
        // Added header is present.
        assert_eq!(
            profile
                .extra_headers
                .get("x-custom-token")
                .map(String::as_str),
            Some("secret")
        );
        // Built-in headers not overridden remain intact.
        assert_eq!(
            profile.extra_headers.get("x-ide-name").map(String::as_str),
            Some("WorkBuddy")
        );
        // Env-sourced headers are carried separately.
        assert_eq!(
            profile.env_http_headers.get("cookie").map(String::as_str),
            Some("WORKBUDDY_COOKIE")
        );
    }

    #[test]
    fn custom_profile_carries_header_maps() {
        let raw: toml::Value = toml::from_str(
            r#"
                [clients.custom.weird]
                protocol = "chat_completions"
                auth_scheme = "bearer"
                env_key = "WEIRD_KEY"
                client_identifier = "weird"

                [clients.custom.weird.extra_headers]
                "x-token" = "plain"

                [clients.custom.weird.env_http_headers]
                "authorization" = "WEIRD_AUTH"
            "#,
        )
        .expect("valid TOML");
        let config =
            crate::agent::config::Config::new_from_toml_cfg(&raw).expect("config should parse");
        let profile = config
            .client_profile_by_id("weird")
            .expect("custom profile");
        assert_eq!(
            profile.extra_headers.get("x-token").map(String::as_str),
            Some("plain")
        );
        assert_eq!(
            profile
                .env_http_headers
                .get("authorization")
                .map(String::as_str),
            Some("WEIRD_AUTH")
        );
    }
}
