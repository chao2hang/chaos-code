//! Built-in request-client profiles.
//!
//! A profile describes the public identity and compatibility hints that Chaos
//! can attach to requests. It does not provide credentials and it does not
//! replace the model's endpoint, protocol, or authentication configuration.

use serde::Serialize;

use xai_grok_sampler::{OriginClientInfo, SamplerConfig};

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
    },
    BuiltinClientProfile {
        id: "codex",
        name: "Codex",
        protocol: "responses",
        auth_scheme: "bearer",
        env_key: "OPENAI_API_KEY",
        client_identifier: "codex",
    },
    BuiltinClientProfile {
        id: "grok-build",
        name: "Grok Build",
        protocol: "responses",
        auth_scheme: "bearer",
        env_key: "XAI_API_KEY",
        client_identifier: "grok-build",
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
        _ => return None,
    };
    BUILTIN_CLIENT_PROFILES
        .iter()
        .find(|profile| profile.id == canonical)
        .copied()
        .map(BuiltinClientProfile::to_owned)
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
        Some(origin) if !is_native_client_identifier(&origin.product) => {
            by_id(&origin.product)
        }
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

    /// Apply only request identity fields that are not already present.
    pub fn apply_to_sampling_config(&self, config: &mut SamplerConfig) {
        if config.client_identifier.is_none() {
            config.client_identifier = Some(self.client_identifier.clone());
        }
        if config.origin_client.is_none() {
            config.origin_client = Some(self.origin_client());
        }
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
            vec!["claude-code", "codex", "grok-build"]
        );
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
            config.origin_client.as_ref().map(|origin| origin.product.as_str()),
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
        let config = crate::agent::config::Config::new_from_toml_cfg(&raw)
            .expect("config should parse");
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
        let config = crate::agent::config::Config::new_from_toml_cfg(&raw)
            .expect("config should parse");
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
}
