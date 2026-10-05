use crate::util::shared_guard::ReadWriteOrRecover;
use agent_client_protocol as acp;

use crate::agent::config::ModelEntry;
use crate::auth::PreferredAuthMethod;

/// Shared, live handle to the agent's current ACP auth method id.
///
/// `Arc` so a clone can cross the per-session-thread boundary at spawn.
/// The `ArcSwapOption` interior lets the agent's `authenticate` handler publish a new method without re-spawning sessions.
/// Every running session's per-turn auth gate observes the new method on its next turn.
/// `None` until the first `authenticate`.
/// Auth is process-global (one user, one `AuthManager`), so all sessions sharing one cell is correct.
pub(crate) type SharedAuthMethodId = std::sync::Arc<arc_swap::ArcSwapOption<acp::AuthMethodId>>;

/// Construct a [`SharedAuthMethodId`]. `None` is the pre-`authenticate` state.
pub(crate) fn new_shared_auth_method_id(initial: Option<acp::AuthMethodId>) -> SharedAuthMethodId {
    std::sync::Arc::new(arc_swap::ArcSwapOption::new(
        initial.map(std::sync::Arc::new),
    ))
}

/// Env var that, when set, advertises `xai.api_key` as a viable auth method.
///
/// Kept as a constant so test code and the production check stay in sync.
pub const XAI_API_KEY_ENV_VAR: &str = "XAI_API_KEY";

/// Legacy env var name.
/// Checked as a fallback when `XAI_API_KEY` is not set, so existing deployments that use the old name keep working.
pub const LEGACY_XAI_API_KEY_ENV_VAR: &str = "GROK_CODE_XAI_API_KEY";

/// Runtime API-key state, deliberately kept out of the process environment.
///
/// Both runtime publishers used to call `std::env::set_var("XAI_API_KEY", ..)`:
/// `initialize()` republished the key read from `auth.json`, and the
/// `x.ai/setApiKey` extension rewrote it on every change. That wrote to the
/// process-global environment while unrelated threads kept calling
/// `std::env::var` — the exact race edition 2024 makes `set_var` `unsafe` for —
/// and it also leaked the secret into the environment block of every child
/// process spawned afterwards (shell tools, hooks, MCP servers).
#[derive(Clone)]
enum RuntimeApiKey {
    /// Nothing published a key at runtime, so the inherited environment wins.
    Unset,
    /// A key was published at runtime and wins over the inherited environment.
    Present(String),
    /// `clear_api_key` ran, so an inherited `XAI_API_KEY` must stop being
    /// honoured for the rest of the process.
    Cleared,
}

static RUNTIME_API_KEY: std::sync::RwLock<RuntimeApiKey> =
    std::sync::RwLock::new(RuntimeApiKey::Unset);

fn runtime_api_key() -> RuntimeApiKey {
    RUNTIME_API_KEY.read_or_recover().clone()
}

/// Publish an API key for the rest of the process without mutating the
/// inherited environment.
pub(crate) fn set_runtime_api_key(key: impl Into<String>) {
    let mut slot = RUNTIME_API_KEY.write_or_recover();
    *slot = RuntimeApiKey::Present(key.into());
}

/// Forget any runtime key and stop honouring an inherited `XAI_API_KEY`.
///
/// The legacy `GROK_CODE_XAI_API_KEY` is left alone, matching the previous
/// behaviour of removing only `XAI_API_KEY`.
pub(crate) fn clear_runtime_api_key() {
    let mut slot = RUNTIME_API_KEY.write_or_recover();
    *slot = RuntimeApiKey::Cleared;
}

/// Put the runtime key back to "environment wins", so a test that drove a
/// production publisher cannot leak into the next one.
#[cfg(test)]
pub(crate) fn reset_runtime_api_key_for_test() {
    let mut slot = RUNTIME_API_KEY.write_or_recover();
    *slot = RuntimeApiKey::Unset;
}

/// Read the API key from the environment.
///
/// Checks `XAI_API_KEY` first, then falls back to the legacy `GROK_CODE_XAI_API_KEY` for backward compatibility.
/// A key published through [`set_runtime_api_key`] / [`clear_runtime_api_key`] takes precedence over `XAI_API_KEY`.
pub(crate) fn read_xai_api_key_env() -> Result<String, std::env::VarError> {
    match runtime_api_key() {
        RuntimeApiKey::Present(key) => Ok(key),
        RuntimeApiKey::Cleared => std::env::var(LEGACY_XAI_API_KEY_ENV_VAR),
        RuntimeApiKey::Unset => std::env::var(XAI_API_KEY_ENV_VAR)
            .or_else(|_| std::env::var(LEGACY_XAI_API_KEY_ENV_VAR)),
    }
}

/// Returns `true` if either `XAI_API_KEY` or `GROK_CODE_XAI_API_KEY` is set.
pub fn has_xai_api_key_env() -> bool {
    read_xai_api_key_env().is_ok()
}

/// Whether credentials for `xai.api_key` exist right now (per-model BYOK or the first-party env key).
///
/// CHAOS BEHAVIOR: this no longer decides what gets advertised. Chaos is BYOK-only, so
/// [`build_auth_methods`] advertises exactly one method, `xai.api_key`, for every input, and its
/// return value cannot change the advertised list. Upstream grok-build consumed it here to choose
/// between `xai.api_key`, `cached_token` and an interactive login method; that branch is gone.
/// Live consumers are the `initialize()` diagnostics payload, the BYOK ordering `debug_assert!` in
/// `crates/codegen/xai-grok-shell/src/agent/mvp_agent/acp_agent.rs`, and the `chaos` CLI auth banner
/// in `crates/codegen/xai-grok-shell/src/cli_models.rs`, which still reports "model credentials"
/// versus "not configured" from this predicate.
///
/// Probes `std::env` at call time and consults each `ModelEntry` for a resolvable api_key/env_key.
/// Both inputs can change between calls, so the result is not cached. The runtime key cell
/// ([`set_runtime_api_key`] / [`clear_runtime_api_key`]) outranks `XAI_API_KEY`.
///
/// `disable_api_key_auth` (`[grok_com_config] disable_api_key_auth` / `GROK_DISABLE_API_KEY_AUTH`) is
/// the admin kill switch. It has two effects, and only one of them is here:
/// * this predicate returns false, so the banner and telemetry stop claiming a usable key;
/// * credential resolution refuses to hand out a static key, in `resolve_static_api_key` inside
///   `crates/codegen/xai-grok-shell/src/auth/manager.rs`, which is what actually blocks sampling.
///   That is where the kill switch is enforced, because the advertised list is constant.
///
/// Presence-only for the first-party env key (treats it as usable).
/// Paths that have run the validity probe should call [`should_advertise_xai_api_key_with_env_ok`]
/// with the probe result instead.
pub(crate) fn should_advertise_xai_api_key<'a, I>(disable_api_key_auth: bool, models: I) -> bool
where
    I: IntoIterator<Item = &'a ModelEntry>,
{
    should_advertise_xai_api_key_with_env_ok(disable_api_key_auth, models, true)
}

/// Single advertise policy for `xai.api_key`: the kill switch, BYOK, and the first-party env key.
/// The env key is gated by `first_party_env_ok` (probe result, or `true` for presence-only); BYOK still advertises without a probe.
pub(crate) fn should_advertise_xai_api_key_with_env_ok<'a, I>(
    disable_api_key_auth: bool,
    models: I,
    first_party_env_ok: bool,
) -> bool
where
    I: IntoIterator<Item = &'a ModelEntry>,
{
    if disable_api_key_auth {
        return false;
    }
    let has_byok = models.into_iter().any(ModelEntry::has_own_credentials);
    has_byok || (has_xai_api_key_env() && first_party_env_ok)
}

/// Inputs to [`build_auth_methods`].
///
/// The caller (`MvpAgent::initialize()`) computes the booleans.
/// They depend on async side effects (token refresh) and shared mutable state (`AuthManager`).
/// The list-construction logic itself is pure so it can be unit-tested without any of that machinery.
///
/// CHAOS BEHAVIOR: the struct keeps upstream's field set so the ported call site stays readable, but
/// no field changes the output. Chaos is BYOK-only and every branch of [`build_auth_methods`] returns
/// the same one-element list. `chaos_auth_tests` asserts that over the full input cross-product.
pub struct AuthMethodsBuildInputs<'a> {
    /// Upstream: true if `xai.api_key` may be advertised. Computed via
    /// [`should_advertise_xai_api_key_with_env_ok`] after the validity probe.
    /// Chaos: consumed only by the `initialize()` diagnostics payload and the BYOK `debug_assert!`.
    pub has_external_api_key: bool,
    /// Upstream: true if a cached session token is available (present at startup, or recovered via silent refresh).
    /// Chaos: unused; cached xAI sessions are never advertised.
    pub has_cached_token: bool,
    /// Upstream: true if enterprise OIDC is configured, which replaces the `grok.com` method.
    /// Chaos: unused; browser login is never exposed.
    pub has_enterprise_oidc: bool,
    /// Upstream: required when `has_enterprise_oidc` is true; ignored otherwise. Chaos: unused.
    pub enterprise_oidc_issuer: Option<&'a str>,
    /// Upstream: display label for the login method (`grok.com` or `oidc`). Chaos: unused.
    pub login_label: Option<&'a str>,
    /// Upstream: true if `grok_com_config.auth_provider_command` is configured, which sets
    /// `meta.external_provider = true` on the `grok.com` method. Chaos: unused.
    pub has_auth_provider_command: bool,
    /// Upstream: config pin (`[auth] preferred_method`); `Some` was fail-closed to that method family.
    /// Chaos: unused; the pinned and unpinned builders all return `xai.api_key`. Config parsing still
    /// accepts the key, so an existing `config.toml` keeps loading unchanged.
    pub preferred_method: Option<PreferredAuthMethod>,
}

/// Output of [`build_auth_methods`].
pub struct BuiltAuthMethods {
    /// Auth methods in advertised order.
    /// ORDER IS THE CONTRACT: the pager's `startup_auth_metadata()` reads `methods.first()` to decide whether interactive login is needed.
    /// Chaos: always exactly one element, `xai.api_key`, so the pager never shows a login screen.
    pub methods: Vec<acp::AuthMethod>,
    /// The default `auth_method_id` to install on the agent.
    /// Chaos: always `Some(xai.api_key)`. The `Option` and the `None` cases are upstream's (an
    /// unavailable pin meant "fail auth"); no Chaos path returns `None`.
    pub default_auth_method_id: Option<acp::AuthMethodId>,
}

/// Build the `auth_methods` list and default `auth_method_id` from pre-computed inputs.
///
/// CHAOS BEHAVIOR: BYOK only. For every input, and for each of the three `preferred_method` arms,
/// the result is `methods == [xai.api_key]` and `default_auth_method_id == Some(xai.api_key)`.
/// There is no `cached_token` entry, no `grok.com` / `oidc` entry, no empty list and no `None`
/// default. Chaos never starts an interactive login and never reuses an xAI session; missing
/// credentials are a provider configuration error, not a reason to open a browser.
/// `chaos_auth_tests::every_configuration_advertises_only_api_key` locks all 96 input combinations.
///
/// Where the failures surface instead of in the advertised list:
/// * no usable key at all -> credential resolution fails with [`PREFERRED_API_KEY_UNAVAILABLE`];
///   see `resolve_static_api_key` in `crates/codegen/xai-grok-shell/src/auth/manager.rs`.
/// * `disable_api_key_auth` is set -> the same resolver returns no static key and sampling reports
///   `AuthError::ApiKeyAuthDisabled`, asserted live in
///   `crates/codegen/xai-grok-shell/src/auth/manager_tests.rs`.
///
/// Upstream's matrix (kept as history for readers diffing against grok-build): unpinned order was
/// `xai.api_key` if `has_external_api_key`, then `cached_token` if `has_cached_token`, then exactly
/// one of `oidc` (if `has_enterprise_oidc`) or `grok.com`; unpinned default was `cached_token`, else
/// `xai.api_key`, else `None`. Pinned to `ApiKey` it advertised only `xai.api_key` or failed closed
/// with an empty list and `None`; pinned to `Oidc` it offered `cached_token` plus interactive login
/// and never `xai.api_key`.
pub fn build_auth_methods(inputs: AuthMethodsBuildInputs<'_>) -> BuiltAuthMethods {
    let AuthMethodsBuildInputs {
        has_external_api_key,
        has_cached_token,
        has_enterprise_oidc,
        enterprise_oidc_issuer,
        login_label,
        has_auth_provider_command,
        preferred_method,
    } = inputs;

    match preferred_method {
        Some(PreferredAuthMethod::ApiKey) => build_pinned_api_key(has_external_api_key),
        Some(PreferredAuthMethod::Oidc) => build_pinned_oidc(
            has_cached_token,
            has_enterprise_oidc,
            enterprise_oidc_issuer,
            login_label,
            has_auth_provider_command,
        ),
        None => build_unpinned(
            has_external_api_key,
            has_cached_token,
            has_enterprise_oidc,
            enterprise_oidc_issuer,
            login_label,
            has_auth_provider_command,
        ),
    }
}

fn build_pinned_api_key(_has_external_api_key: bool) -> BuiltAuthMethods {
    BuiltAuthMethods {
        methods: vec![xai_api_key_auth_method()],
        default_auth_method_id: Some(acp::AuthMethodId::new(XAI_API_KEY_METHOD_ID)),
    }
}

fn build_pinned_oidc(
    _has_cached_token: bool,
    has_enterprise_oidc: bool,
    enterprise_oidc_issuer: Option<&str>,
    login_label: Option<&str>,
    has_auth_provider_command: bool,
) -> BuiltAuthMethods {
    // Chaos is BYOK-only: never advertise Grok.com or OIDC browser login.
    let _ = (
        has_enterprise_oidc,
        enterprise_oidc_issuer,
        login_label,
        has_auth_provider_command,
    );

    BuiltAuthMethods {
        methods: vec![xai_api_key_auth_method()],
        default_auth_method_id: Some(acp::AuthMethodId::new(XAI_API_KEY_METHOD_ID)),
    }
}

fn build_unpinned(
    has_external_api_key: bool,
    _has_cached_token: bool,
    has_enterprise_oidc: bool,
    enterprise_oidc_issuer: Option<&str>,
    login_label: Option<&str>,
    has_auth_provider_command: bool,
) -> BuiltAuthMethods {
    // Always advertise the non-interactive method. Missing credentials are a
    // provider configuration error, never a reason to start browser login.
    let _ = has_external_api_key;
    let methods = vec![xai_api_key_auth_method()];
    let default_auth_method_id = Some(acp::AuthMethodId::new(XAI_API_KEY_METHOD_ID));

    // Ignore cached xAI sessions and never expose browser-based authentication.
    let _ = (
        has_enterprise_oidc,
        enterprise_oidc_issuer,
        login_label,
        has_auth_provider_command,
    );

    BuiltAuthMethods {
        methods,
        default_auth_method_id,
    }
}

/// Upstream's login-method selection, retained because the `grok.com` / `oidc` constructors it calls
/// are still needed for ACP wire compatibility and for [`AuthMethodKind`] classification.
///
/// No Chaos production path calls this: browser login is never advertised, so keeping it uncalled is
/// intentional rather than an oversight. `tests` exercises both arms so the retained constructors
/// cannot rot unnoticed.
fn push_interactive_login(
    methods: &mut Vec<acp::AuthMethod>,
    has_enterprise_oidc: bool,
    enterprise_oidc_issuer: Option<&str>,
    login_label: Option<&str>,
    has_auth_provider_command: bool,
) {
    if has_enterprise_oidc {
        // Caller invariant: `enterprise_oidc_issuer` MUST be `Some(...)` when `has_enterprise_oidc` is true
        // Production callers derive both from the same `cfg.grok_com_config.oidc` Option
        // The inconsistent `(true, None)` combination is a programmer error, so panic loudly
        let issuer = enterprise_oidc_issuer
            .expect("enterprise_oidc_issuer is required when has_enterprise_oidc is true");
        methods.push(oidc_auth_method(issuer, login_label));
    } else {
        methods.push(grok_com_auth_method(login_label, has_auth_provider_command));
    }
}

/// ACP session auth method. Use `is_session_based_method` for classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethodKind {
    XaiApiKey,
    CachedToken,
    GrokCom,
    Oidc,
    Unknown,
}

impl AuthMethodKind {
    pub fn from_id(id: &acp::AuthMethodId) -> Self {
        match id.0.as_ref() {
            XAI_API_KEY_METHOD_ID => Self::XaiApiKey,
            CACHED_TOKEN_AUTH_METHOD_ID => Self::CachedToken,
            GROK_COM_METHOD_ID => Self::GrokCom,
            OIDC_METHOD_ID => Self::Oidc,
            _ => Self::Unknown,
        }
    }

    /// API key auth: no auth.json, no refresh, no user interaction.
    pub fn is_api_key(self) -> bool {
        matches!(self, Self::XaiApiKey)
    }

    /// `true` for session-based methods (cached_token, grok.com, oidc).
    pub fn is_session_based(self) -> bool {
        matches!(self, Self::CachedToken | Self::GrokCom | Self::Oidc)
    }

    /// Requires user interaction (browser, OIDC redirect, or external auth command).
    pub fn needs_interactive_login(self) -> bool {
        matches!(self, Self::GrokCom | Self::Oidc)
    }

    pub fn auth_error_message(self) -> &'static str {
        if self.is_session_based() {
            AUTH_ERROR_SESSION_EXPIRED
        } else {
            AUTH_ERROR_API_KEY
        }
    }
}

/// `true` for session-based ACP methods (cached_token, grok.com, oidc).
pub fn is_session_based_method(method_id: &acp::AuthMethodId) -> bool {
    AuthMethodKind::from_id(method_id).is_session_based()
}

/// Per-model BYOK status: whether the selected model carries its own `[model.*]` `api_key`/`env_key`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelByok {
    /// Model has its own per-model key (not refreshable).
    Byok,
    /// Model has no per-model key (session auth governs).
    NotByok,
    /// Config couldn't be loaded/parsed; BYOK status indeterminate.
    Unknown,
}

impl ModelByok {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Byok => "byok",
            Self::NotByok => "not_byok",
            Self::Unknown => "unknown",
        }
    }
}

/// Whether this session and model combination uses a refreshable session token.
///
/// Gates on stable inputs, not `Credentials.auth_type`.
/// That field collapses to `ApiKey` when the session-token cache is momentarily empty and `XAI_API_KEY` is set.
/// The collapse demoted live OIDC sessions to non-refreshable api-key mode and 401'd every prompt until restart.
/// `model_byok` still excludes genuine per-model BYOK, whose keys are not refreshable.
///
/// `Unknown` means BYOK status is indeterminate: config currently unparseable, no sampling config yet, or the per-model memo was cleared.
/// It must **not** demote a live session to non-refreshable api-key mode.
/// That demotion re-sends the stale buffered token on every turn and 401s with `bad-credentials` until restart.
/// Instead, `Unknown` refreshes only when `endpoint_is_first_party`.
/// On a first-party host (cli-chat-proxy / first-party API) the session token cannot leak to a third-party BYOK endpoint.
/// A definite `NotByok` always refreshes (it only ever routes to the session endpoint); a definite `Byok` never does.
pub(crate) fn session_token_auth_gate(
    is_session_based_method: bool,
    model_byok: ModelByok,
    endpoint_is_first_party: bool,
) -> bool {
    is_session_based_method
        && match model_byok {
            ModelByok::NotByok => true,
            ModelByok::Byok => false,
            ModelByok::Unknown => endpoint_is_first_party,
        }
}

pub const AUTH_ERROR_SESSION_EXPIRED: &str = "模型认证已失效，请更新当前 Provider 的 API Key。";

pub const AUTH_ERROR_API_KEY: &str =
    "模型认证失败，请检查 api_key/env_key、base_url 和 auth_scheme 配置。";

/// Compatibility exit for callers holding a legacy session method.
/// Chaos never starts an interactive flow; every path returns API-key auth.
pub fn method_id_after_cached_token_unavailable(
    _has_external_api_key: bool,
    _preferred_method: Option<PreferredAuthMethod>,
) -> Option<&'static str> {
    Some(XAI_API_KEY_METHOD_ID)
}

/// Error when `preferred_method=api_key` but no key/BYOK credentials exist.
pub const PREFERRED_API_KEY_UNAVAILABLE: &str =
    "未配置 Provider API Key；请设置模型或 model_providers 的 api_key/env_key。";

/// Error when `preferred_method=oidc` but the session path cannot proceed.
pub const PREFERRED_OIDC_UNAVAILABLE: &str = "Chaos 不支持 OIDC 登录；请改用 Provider API Key。";

pub const XAI_API_KEY_METHOD_ID: &str = "xai.api_key";
pub fn xai_api_key_auth_method() -> acp::AuthMethod {
    acp::AuthMethod::Agent(
        acp::AuthMethodAgent::new(
            acp::AuthMethodId::new(XAI_API_KEY_METHOD_ID),
            "xai.api_key".to_string(),
        )
        .description(Some(
            "config.toml 中 model_providers 的 api_key/env_key（或环境变量）".to_string(),
        )),
    )
}

pub const CACHED_TOKEN_AUTH_METHOD_ID: &str = "cached_token";
pub fn cached_token_auth_method() -> acp::AuthMethod {
    acp::AuthMethod::Agent(
        acp::AuthMethodAgent::new(
            acp::AuthMethodId::new(CACHED_TOKEN_AUTH_METHOD_ID),
            "cached_token".to_string(),
        )
        .description(Some(format!(
            "来自 {} 的缓存令牌",
            xai_grok_config::display_home_path("auth.json")
        ))),
    )
}

pub const GROK_COM_METHOD_ID: &str = "grok.com";

/// xAI OAuth2/OIDC auth. Method id `"grok.com"` kept for ACP wire compatibility.
pub(crate) fn grok_com_auth_method(
    label: Option<&str>,
    has_auth_provider_command: bool,
) -> acp::AuthMethod {
    let name = label.unwrap_or("Grok");
    let meta = if has_auth_provider_command {
        let mut m = acp::Meta::new();
        m.insert("external_provider".to_owned(), serde_json::json!(true));
        Some(m)
    } else {
        None
    };
    acp::AuthMethod::Agent(
        acp::AuthMethodAgent::new(acp::AuthMethodId::new(GROK_COM_METHOD_ID), name.to_string())
            .description(Some(format!("Sign in with {name}")))
            .meta(meta),
    )
}

pub const OIDC_METHOD_ID: &str = "oidc";
pub fn oidc_auth_method(issuer: &str, label: Option<&str>) -> acp::AuthMethod {
    let name = label
        .map(|l| l.to_string())
        .unwrap_or_else(|| format!("Single sign-on ({})", issuer));
    acp::AuthMethod::Agent(
        acp::AuthMethodAgent::new(acp::AuthMethodId::new(OIDC_METHOD_ID), name.clone())
            .description(Some(format!("Sign in with {name}"))),
    )
}

#[cfg(test)]
mod runtime_api_key_tests {
    //! The runtime key replaced `std::env::set_var("XAI_API_KEY", ..)`, so these
    //! drive the real reader and assert the environment is never touched.
    use super::*;
    use crate::util::shared_guard::poison_rwlock_through_a_panicking_writer;
    use serial_test::serial;

    /// Restores "environment wins" so a test cannot leak the cell.
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            reset_runtime_api_key_for_test();
        }
    }

    #[test]
    #[serial]
    fn unset_cell_leaves_the_environment_authoritative() {
        let _reset = Reset;
        reset_runtime_api_key_for_test();
        xai_grok_test_support::env::with_write_lock(|| {
            xai_grok_test_support::env::set_var(XAI_API_KEY_ENV_VAR, "from-environment");
            xai_grok_test_support::env::remove_var(LEGACY_XAI_API_KEY_ENV_VAR);
            assert_eq!(read_xai_api_key_env().as_deref(), Ok("from-environment"));

            xai_grok_test_support::env::remove_var(XAI_API_KEY_ENV_VAR);
            assert_eq!(
                read_xai_api_key_env(),
                Err(std::env::VarError::NotPresent),
                "no runtime key and no env var must read as absent"
            );
        });
    }

    #[test]
    #[serial]
    fn runtime_key_overrides_the_inherited_environment() {
        let _reset = Reset;
        reset_runtime_api_key_for_test();
        xai_grok_test_support::env::with_write_lock(|| {
            xai_grok_test_support::env::set_var(XAI_API_KEY_ENV_VAR, "stale-env-key");
            set_runtime_api_key("key-from-auth-json");
            assert_eq!(read_xai_api_key_env().as_deref(), Ok("key-from-auth-json"));
            // The whole point of the change: the secret never lands in the env
            // block that every child process would inherit.
            assert_eq!(
                std::env::var(XAI_API_KEY_ENV_VAR).as_deref(),
                Ok("stale-env-key"),
                "publishing a runtime key must not rewrite the environment"
            );
            xai_grok_test_support::env::remove_var(XAI_API_KEY_ENV_VAR);
        });
    }

    #[test]
    #[serial]
    fn clearing_the_runtime_key_masks_an_inherited_key_but_keeps_legacy() {
        let _reset = Reset;
        reset_runtime_api_key_for_test();
        xai_grok_test_support::env::with_write_lock(|| {
            xai_grok_test_support::env::set_var(XAI_API_KEY_ENV_VAR, "inherited-key");
            xai_grok_test_support::env::set_var(LEGACY_XAI_API_KEY_ENV_VAR, "legacy-key");
            clear_runtime_api_key();
            assert_eq!(
                read_xai_api_key_env().as_deref(),
                Ok("legacy-key"),
                "clearing masks XAI_API_KEY only, matching the old remove_var scope"
            );

            xai_grok_test_support::env::remove_var(LEGACY_XAI_API_KEY_ENV_VAR);
            assert_eq!(
                read_xai_api_key_env(),
                Err(std::env::VarError::NotPresent),
                "after clearing, an inherited XAI_API_KEY must not be honoured"
            );
            xai_grok_test_support::env::remove_var(XAI_API_KEY_ENV_VAR);
        });
    }

    #[test]
    #[serial]
    fn concurrent_reads_and_writes_of_the_cell_never_tear() {
        let _reset = Reset;
        reset_runtime_api_key_for_test();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                for i in 0..2_000 {
                    set_runtime_api_key(format!("key-{i}"));
                    clear_runtime_api_key();
                }
            });
            for _ in 0..2_000 {
                // `read_xai_api_key_env` must stay callable while another thread
                // publishes; a plain `static mut` env write is what this replaced.
                let _ = read_xai_api_key_env();
            }
        });
        reset_runtime_api_key_for_test();
    }

    /// The cell is read on every auth resolution, and the value behind it is a plain
    /// enum. A poisoning must cost at most the key the dead writer was storing, never
    /// the resolution that only wanted to read it.
    #[test]
    #[serial]
    fn a_poisoned_runtime_key_cell_still_answers_and_still_accepts_a_write() {
        let _reset = Reset;
        reset_runtime_api_key_for_test();
        set_runtime_api_key("stored-by-the-dead-writer");
        poison_rwlock_through_a_panicking_writer(&RUNTIME_API_KEY);
        assert!(
            matches!(runtime_api_key(), RuntimeApiKey::Present(_)),
            "the key the dead writer stored must still be handed out"
        );
        clear_runtime_api_key();
        assert!(
            matches!(runtime_api_key(), RuntimeApiKey::Cleared),
            "and a later clear must still land behind the poisoning"
        );
        set_runtime_api_key("published-after-the-poisoning");
        assert!(
            matches!(
                runtime_api_key(),
                RuntimeApiKey::Present(key) if key == "published-after-the-poisoning"
            ),
            "and so must a later publish, which is what the auth flow calls"
        );
        assert!(
            RUNTIME_API_KEY.is_poisoned(),
            "the poisoning itself must stay visible to anyone who asks"
        );
        RUNTIME_API_KEY.clear_poison();
    }
}

#[cfg(test)]
mod tests {
    //! Coverage for the fork's auth-method surface.
    //!
    //! Upstream grok-build kept this module live. Chaos pinned the fork to BYOK-only and parked
    //! the whole module behind `#[cfg(any())]` instead of curating it -- three times, on
    //! 2026-07-26, 2026-08-07 and 2026-09-04, with two upstream syncs restoring `#[cfg(test)]` in
    //! between and each re-park also swapping out upstream's newer version of this module. The
    //! last stretch ran 31 days, and during it `cargo test` compiled none of it: the module
    //! appeared in no test report and no `#[ignore]` ledger, and
    //! `scripts/ci/panic-site-census.py` counted the `.unwrap()` calls inside it as production
    //! panic sites. `scripts/ci/check-dead-cfg.py` is the gate that keeps that from recurring.
    //!
    //! The module is live again. What survived is the part that still describes shipped behavior:
    //! the `AuthMethodKind` classifier matrix, the credential predicate, the env-var precedence
    //! rules, the `AuthManager` legacy-token load path, and the retained login-method constructors.
    //!
    //! These 11 upstream tests were deleted rather than revived. Each one asserts an ordering that
    //! BYOK-only removed, so keeping them would mean weakening their assertions into something that
    //! no longer catches the upstream regression it was written for:
    //! * `after_cached_token_unavailable_falls_to_grok_com_without_api_key` -- expects fallthrough to
    //!   `grok.com`; Chaos returns `xai.api_key` for every input.
    //! * `after_cached_token_unavailable_fails_closed_when_pinned` -- expects `None` on a pin; Chaos
    //!   never returns `None`.
    //! * `byok_with_cached_token_keeps_xai_api_key_first` -- its distinguishing claim is that
    //!   `default_auth_method_id` stays `cached_token`; no Chaos path yields `cached_token`.
    //! * `session_only_user_first_method_is_cached_token` -- session auth is never advertised.
    //! * `fresh_user_only_advertises_grok_com_and_requires_login` -- Chaos never requires a login.
    //! * `enterprise_oidc_replaces_grok_com_but_xai_api_key_still_first` -- no `oidc` method exists.
    //! * `auth_provider_command_sets_external_provider_meta` -- moved to
    //!   `retained_login_methods_still_build`, which asserts the same metadata on the constructor
    //!   that is still reachable instead of on the advertised list, which is not.
    //! * `pin_api_key_with_key_only_advertises_api_key`, `pin_api_key_without_key_fails_closed_even_with_session`,
    //!   `pin_oidc_with_session_hides_api_key`, `pin_oidc_without_session_is_interactive_only` -- the
    //!   `preferred_method` pin is parsed but no longer changes the list; the cross-product test in
    //!   `chaos_auth_tests` covers all three pins instead.
    //!
    //! Three others came back under new names, because the claim they check changed shape rather
    //! than disappearing: `disable_api_key_auth_suppresses_xai_api_key_method` became
    //! `disable_api_key_auth_decides_the_predicate_not_the_advertised_list`,
    //! `grok_login_legacy_token_does_not_require_login` became
    //! `legacy_grok_auth_env_is_loaded_but_never_advertised`, and
    //! `no_legacy_token_means_no_cached_token_advertised` became
    //! `no_legacy_token_means_no_current_credential`. One was folded rather than renamed:
    //! `after_cached_token_unavailable_prefers_api_key_when_advertiseable` now runs as
    //! `chaos_auth_tests::legacy_session_fallthrough_is_api_key_only`, which loops every
    //! cached-token input instead of the two combinations it spelled out.

    use super::*;
    use crate::agent::config::{Config, resolve_model_list};
    use serial_test::serial;
    use xai_grok_test_support::EnvGuard;

    /// The runtime key cell outranks `std::env` in [`read_xai_api_key_env`], so a leaked runtime key
    /// makes every env-var assertion vacuous. Reset on entry and on exit (including panic).
    struct EnvOnlyKeys;

    impl EnvOnlyKeys {
        fn new() -> Self {
            reset_runtime_api_key_for_test();
            EnvOnlyKeys
        }
    }

    impl Drop for EnvOnlyKeys {
        fn drop(&mut self) {
            reset_runtime_api_key_for_test();
        }
    }

    /// Inputs with everything turned off; individual tests override what they care about.
    /// Every field is inert in Chaos -- see the field docs on [`AuthMethodsBuildInputs`].
    fn default_inputs() -> AuthMethodsBuildInputs<'static> {
        AuthMethodsBuildInputs {
            has_external_api_key: false,
            has_cached_token: false,
            has_enterprise_oidc: false,
            enterprise_oidc_issuer: None,
            login_label: None,
            has_auth_provider_command: false,
            preferred_method: None,
        }
    }

    fn method_ids(methods: &[acp::AuthMethod]) -> Vec<&str> {
        methods.iter().map(|m| m.id().0.as_ref()).collect()
    }

    fn default_id(built: &BuiltAuthMethods) -> Option<&str> {
        built
            .default_auth_method_id
            .as_ref()
            .map(|id| id.0.as_ref())
    }

    fn first_kind(methods: &[acp::AuthMethod]) -> Option<AuthMethodKind> {
        methods.first().map(|m| AuthMethodKind::from_id(m.id()))
    }

    /// TOML shape shared by the BYOK tests: one model carrying its own `env_key`.
    fn byok_config(test_env_var: &str) -> toml::Value {
        let dm = crate::models::default_model();
        toml::from_str(&format!(
            r#"
            [model."{dm}"]
            model = "{dm}"
            base_url = "https://inference.example.com/v1"
            context_window = 200000
            env_key = "{test_env_var}"
            "#,
        ))
        .expect("BYOK test config should parse")
    }

    // -- AuthMethodKind classification ------------------------------------
    //
    // Still fully live: `is_session_based_method` / `AuthMethodKind::from_id` gate refresh and
    // telemetry in sampler_turn.rs, session_setup.rs, subagent/mod.rs, agent_ops.rs and acp_agent.rs,
    // and must keep classifying the ids that upstream clients can still put on the wire.

    #[test]
    fn auth_method_kind_classifier_matrix() {
        let session_methods = [
            CACHED_TOKEN_AUTH_METHOD_ID,
            GROK_COM_METHOD_ID,
            OIDC_METHOD_ID,
        ];
        for method_id in session_methods {
            let id = acp::AuthMethodId::new(method_id);
            let kind = AuthMethodKind::from_id(&id);
            assert!(
                kind.is_session_based(),
                "{method_id}: kind must be session-based"
            );
            assert!(
                is_session_based_method(&id),
                "{method_id}: wrapper must agree"
            );
            assert!(
                kind.needs_interactive_login() == (method_id != CACHED_TOKEN_AUTH_METHOD_ID),
                "{method_id}: only the browser methods need interactive login",
            );
        }
        let api_id = acp::AuthMethodId::new(XAI_API_KEY_METHOD_ID);
        let api_kind = AuthMethodKind::from_id(&api_id);
        assert!(!api_kind.is_session_based());
        assert!(api_kind.is_api_key());
        assert!(!api_kind.needs_interactive_login());
        assert!(!is_session_based_method(&acp::AuthMethodId::new(
            "unknown-method"
        )));
        assert_eq!(api_kind.auth_error_message(), AUTH_ERROR_API_KEY);
        assert_eq!(
            AuthMethodKind::from_id(&acp::AuthMethodId::new(GROK_COM_METHOD_ID))
                .auth_error_message(),
            AUTH_ERROR_SESSION_EXPIRED,
        );
    }

    // -- advertised list: shipped behavior ---------------------------------

    /// A BYOK user gets `xai.api_key` first, which is what keeps the pager off the login screen.
    /// The list has exactly one element in Chaos, so "first" and "only" coincide here.
    #[test]
    fn enterprise_byok_first_method_is_xai_api_key() {
        let built = build_auth_methods(AuthMethodsBuildInputs {
            has_external_api_key: true,
            ..default_inputs()
        });

        assert_eq!(
            first_kind(&built.methods),
            Some(AuthMethodKind::XaiApiKey),
            "BYOK: auth_methods.first() MUST be xai.api_key \
             (deferred-to-last ordering sends users to the login screen)",
        );
        assert_eq!(default_id(&built), Some(XAI_API_KEY_METHOD_ID));
        // The pager-side predicate: the first method must not require interactive login.
        assert!(
            !AuthMethodKind::from_id(built.methods[0].id()).needs_interactive_login(),
            "first method MUST NOT need interactive login when xai.api_key is available",
        );
    }

    /// `XAI_API_KEY` alone (no per-model credentials) also satisfies the predicate.
    #[test]
    #[serial]
    fn global_external_api_key_advertises_xai_api_key_first() {
        let _runtime = EnvOnlyKeys::new();
        let _set = EnvGuard::set(XAI_API_KEY_ENV_VAR, "xai-external-key");
        let cfg = Config::default();
        let models = resolve_model_list(&cfg, None);
        let has_external_api_key = should_advertise_xai_api_key(false, models.values());
        assert!(has_external_api_key);
        let built = build_auth_methods(AuthMethodsBuildInputs {
            has_external_api_key,
            ..default_inputs()
        });
        assert_eq!(first_kind(&built.methods), Some(AuthMethodKind::XaiApiKey));
        assert_eq!(method_ids(&built.methods), vec![XAI_API_KEY_METHOD_ID]);
    }

    // -- credential predicate ---------------------------------------------
    //
    // The predicate no longer feeds the advertised list (see its doc comment), but it still drives
    // the `chaos` CLI auth banner in cli_models.rs and the `debug_assert!` in acp_agent.rs.

    /// End-to-end from a real `config.toml`: the predicate follows `env_key` resolution.
    ///
    /// The unset branch is the non-vacuity control for the set branch. It asserts only the
    /// predicate, because in Chaos the advertised list is the same either way.
    #[test]
    #[serial]
    fn enterprise_byok_config_does_not_require_login() {
        const TEST_ENV_VAR: &str = "TEST_ENTERPRISE_REGRESSION_AUTH_TOKEN";

        let _runtime = EnvOnlyKeys::new();
        // Held to end-of-scope so a panic still restores the environment.
        let _global = EnvGuard::unset(XAI_API_KEY_ENV_VAR);
        let _legacy = EnvGuard::unset(LEGACY_XAI_API_KEY_ENV_VAR);

        let dm = crate::models::default_model();
        let toml = byok_config(TEST_ENV_VAR);
        let cfg = Config::new_from_toml_cfg(&toml).expect("config should parse");
        let models = resolve_model_list(&cfg, None);
        let model = models.get(dm).expect("enterprise-style model should exist");
        assert_eq!(
            model.env_key.as_ref().map(|k| k.names()),
            Some(vec![TEST_ENV_VAR])
        );

        {
            let _unset = EnvGuard::unset(TEST_ENV_VAR);
            let has_external_api_key = should_advertise_xai_api_key(false, models.values());
            assert!(
                !has_external_api_key,
                "an unresolved env_key must NOT count as a credential",
            );
        }

        {
            let _set = EnvGuard::set(TEST_ENV_VAR, "enterprise-secret-token");
            let has_external_api_key = should_advertise_xai_api_key(false, models.values());
            assert!(has_external_api_key, "a resolved env_key IS a credential");
            let built = build_auth_methods(AuthMethodsBuildInputs {
                has_external_api_key,
                ..default_inputs()
            });
            assert_eq!(
                first_kind(&built.methods),
                Some(AuthMethodKind::XaiApiKey),
                "BYOK: xai.api_key must be auth_methods.first(); deferred-to-last \
                 ordering sends enterprise users to the login screen",
            );
            assert!(
                !AuthMethodKind::from_id(built.methods[0].id()).needs_interactive_login(),
                "auth_methods.first() MUST NOT need interactive login -- this \
                 is the exact predicate the pager's startup_auth_metadata() uses \
                 to decide whether to show the login screen",
            );
        }
    }

    /// Admin kill switch (`disable_api_key_auth`): it flips the predicate, not the advertised list.
    ///
    /// Upstream expected the method to disappear from the list. Chaos always advertises
    /// `xai.api_key`, so enforcement lives in credential resolution instead --
    /// `resolve_static_api_key` in `crates/codegen/xai-grok-shell/src/auth/manager.rs`, whose test
    /// `cached_api_key_session_rejected_when_api_key_auth_disabled` covers the enforcement side.
    #[test]
    #[serial]
    fn disable_api_key_auth_decides_the_predicate_not_the_advertised_list() {
        let _runtime = EnvOnlyKeys::new();
        let _set = EnvGuard::set(XAI_API_KEY_ENV_VAR, "xai-external-key");
        let cfg = Config::default();
        let models = resolve_model_list(&cfg, None);

        // Flag off: the key is usable.
        assert!(should_advertise_xai_api_key(false, models.values()));

        // Flag on: the predicate says no, even with a key present everywhere.
        assert!(
            !should_advertise_xai_api_key(true, models.values()),
            "the kill switch must win over an available credential",
        );
        let built = build_auth_methods(AuthMethodsBuildInputs {
            has_external_api_key: false,
            ..default_inputs()
        });
        assert_eq!(
            method_ids(&built.methods),
            vec![XAI_API_KEY_METHOD_ID],
            "the advertised list is constant; the kill switch is enforced in credential resolution",
        );
        assert_eq!(default_id(&built), Some(XAI_API_KEY_METHOD_ID));
    }

    #[test]
    #[serial]
    fn env_key_probe_ok_still_advertises() {
        let _runtime = EnvOnlyKeys::new();
        let _set = EnvGuard::set(XAI_API_KEY_ENV_VAR, "xai-live-key");
        let cfg = Config::default();
        let models = resolve_model_list(&cfg, None);
        assert!(should_advertise_xai_api_key_with_env_ok(
            false,
            models.values(),
            true
        ));
    }

    #[test]
    #[serial]
    fn byok_advertises_even_when_env_probe_unusable() {
        const TEST_ENV_VAR: &str = "TEST_BYOK_PROBE_INDEPENDENT_TOKEN";
        let _runtime = EnvOnlyKeys::new();
        let _unset = EnvGuard::unset(XAI_API_KEY_ENV_VAR);
        let _legacy = EnvGuard::unset(LEGACY_XAI_API_KEY_ENV_VAR);
        let _byok = EnvGuard::set(TEST_ENV_VAR, "enterprise-secret-token");

        let toml = byok_config(TEST_ENV_VAR);
        let cfg = Config::new_from_toml_cfg(&toml).expect("config should parse");
        let models = resolve_model_list(&cfg, None);
        assert!(
            should_advertise_xai_api_key_with_env_ok(false, models.values(), false),
            "BYOK must not depend on the first-party env probe"
        );
        assert!(
            !should_advertise_xai_api_key_with_env_ok(true, models.values(), false),
            "the kill switch must still win over BYOK",
        );
    }

    /// Legacy `GROK_CODE_XAI_API_KEY` is accepted as a fallback when `XAI_API_KEY` is not set, so
    /// existing deployments keep working.
    #[test]
    #[serial]
    fn legacy_env_var_fallback_advertises_xai_api_key() {
        let _runtime = EnvOnlyKeys::new();
        let _unset_new = EnvGuard::unset(XAI_API_KEY_ENV_VAR);
        let _set_legacy = EnvGuard::set(LEGACY_XAI_API_KEY_ENV_VAR, "xai-legacy-key");
        assert!(has_xai_api_key_env());
        assert_eq!(read_xai_api_key_env().unwrap(), "xai-legacy-key");

        let cfg = Config::default();
        let models = resolve_model_list(&cfg, None);
        assert!(should_advertise_xai_api_key(false, models.values()));
    }

    /// When both names are set, the new one wins.
    #[test]
    #[serial]
    fn new_env_var_takes_precedence_over_legacy() {
        let _runtime = EnvOnlyKeys::new();
        let _new = EnvGuard::set(XAI_API_KEY_ENV_VAR, "new-key");
        let _legacy = EnvGuard::set(LEGACY_XAI_API_KEY_ENV_VAR, "old-key");
        assert_eq!(read_xai_api_key_env().unwrap(), "new-key");
    }

    // -- legacy `grok login --legacy` token --------------------------------
    //
    // Upstream used the loaded token to put `cached_token` first. Chaos still loads it (an existing
    // `GROK_AUTH` value must not break `AuthManager`), but never advertises a session method.

    /// `AuthManager` still loads a legacy token (WebLogin, no `expires_at`) from the `GROK_AUTH` env
    /// var. The advertised list stays `xai.api_key`, which is why a legacy user is not sent to a
    /// login screen either.
    #[test]
    #[serial]
    fn legacy_grok_auth_env_is_loaded_but_never_advertised() {
        use crate::auth::{AuthManager, AuthMode, GrokAuth, GrokComConfig};

        let _runtime = EnvOnlyKeys::new();
        let _g1 = EnvGuard::unset("GROK_AUTH_PATH");
        let _g2 = EnvGuard::unset(XAI_API_KEY_ENV_VAR);
        let _g3 = EnvGuard::unset(LEGACY_XAI_API_KEY_ENV_VAR);

        // Exactly what `grok login --legacy` produces: WebLogin mode, no OIDC fields, no
        // refresh_token, no expires_at (is_expired falls back to the 30-day age check).
        let legacy_token = GrokAuth {
            key: "legacy-relay-token".into(),
            auth_mode: AuthMode::WebLogin,
            create_time: chrono::Utc::now(),
            user_id: "legacy-user".into(),
            email: Some("legacy@example.com".into()),
            oidc_issuer: None,
            oidc_client_id: None,
            refresh_token: None,
            expires_at: None,
            ..GrokAuth::test_default()
        };
        let legacy_json = serde_json::to_string(&legacy_token).expect("serialize legacy token");
        let _g = EnvGuard::set("GROK_AUTH", &legacy_json);

        let dir = tempfile::tempdir().expect("tempdir");
        let mgr = AuthManager::new(dir.path(), GrokComConfig::default());
        let current = mgr.current();
        assert!(
            current.is_some(),
            "a legacy token in GROK_AUTH must still load -- dropping it would break \
             every existing deployment that authenticated through grok login --legacy",
        );
        assert_eq!(current.as_ref().expect("loaded").key, "legacy-relay-token");

        let built = build_auth_methods(AuthMethodsBuildInputs {
            has_cached_token: mgr.current().is_some(),
            ..default_inputs()
        });
        assert_eq!(
            method_ids(&built.methods),
            vec![XAI_API_KEY_METHOD_ID],
            "a cached xAI session is never advertised by a BYOK-only build",
        );
        assert_eq!(default_id(&built), Some(XAI_API_KEY_METHOD_ID));
    }

    /// Negative control for the test above: with no `GROK_AUTH` and an empty auth.json,
    /// `AuthManager::current()` really is `None`. Without this, the assertion above would pass even
    /// if the loader returned `None` unconditionally.
    #[test]
    #[serial]
    fn no_legacy_token_means_no_current_credential() {
        use crate::auth::{AuthManager, GrokComConfig};

        let _runtime = EnvOnlyKeys::new();
        let _g1 = EnvGuard::unset("GROK_AUTH");
        let _g2 = EnvGuard::unset("GROK_AUTH_PATH");

        let dir = tempfile::tempdir().expect("tempdir");
        let mgr = AuthManager::new(dir.path(), GrokComConfig::default());
        assert!(mgr.current().is_none());

        let built = build_auth_methods(AuthMethodsBuildInputs {
            has_cached_token: mgr.current().is_some(),
            ..default_inputs()
        });
        assert_eq!(method_ids(&built.methods), vec![XAI_API_KEY_METHOD_ID]);
    }

    // -- retained login methods --------------------------------------------
    //
    // `grok.com` / `oidc` / `cached_token` are not advertised, but their ids still arrive on the ACP
    // wire from upstream-compatible clients and still drive AuthMethodKind classification, so the
    // constructors and the login-method selection helper must keep working.

    #[test]
    fn retained_login_methods_still_build() {
        let cached = cached_token_auth_method();
        assert_eq!(cached.id().0.as_ref(), CACHED_TOKEN_AUTH_METHOD_ID);

        let plain = grok_com_auth_method(None, false);
        assert_eq!(plain.id().0.as_ref(), GROK_COM_METHOD_ID);
        assert_eq!(plain.name(), "Grok");
        assert!(
            plain.meta().is_none(),
            "without auth_provider_command no external_provider meta must be set",
        );

        let external = grok_com_auth_method(Some("Corp SSO"), true);
        assert_eq!(external.name(), "Corp SSO");
        let meta = external.meta().expect("external_provider meta");
        assert_eq!(
            meta.get("external_provider"),
            Some(&serde_json::json!(true))
        );

        let oidc = oidc_auth_method("https://idp.example.com", None);
        assert_eq!(oidc.id().0.as_ref(), OIDC_METHOD_ID);
        assert!(
            oidc.name().contains("https://idp.example.com"),
            "an unlabelled OIDC method must name its issuer, got {:?}",
            oidc.name(),
        );
        assert_eq!(oidc_auth_method("https://x", Some("Staff")).name(), "Staff");
    }

    #[test]
    fn push_interactive_login_selects_the_configured_method() {
        let mut methods = Vec::new();
        push_interactive_login(&mut methods, false, None, None, false);
        assert_eq!(method_ids(&methods), vec![GROK_COM_METHOD_ID]);

        let mut methods = Vec::new();
        push_interactive_login(
            &mut methods,
            true,
            Some("https://idp.example.com"),
            Some("Staff"),
            true,
        );
        assert_eq!(method_ids(&methods), vec![OIDC_METHOD_ID]);
        assert!(AuthMethodKind::from_id(methods[0].id()).needs_interactive_login());
    }

    /// The `(true, None)` combination is a programmer error: production callers derive both from the
    /// same `Option`, so it must panic loudly rather than advertise a login method with no issuer.
    #[test]
    #[should_panic(
        expected = "enterprise_oidc_issuer is required when has_enterprise_oidc is true"
    )]
    fn push_interactive_login_requires_an_issuer_for_oidc() {
        let mut methods = Vec::new();
        push_interactive_login(&mut methods, true, None, None, false);
    }
}

#[cfg(test)]
mod chaos_auth_tests {
    use super::*;

    fn inputs(preferred_method: Option<PreferredAuthMethod>) -> AuthMethodsBuildInputs<'static> {
        AuthMethodsBuildInputs {
            has_external_api_key: false,
            has_cached_token: true,
            has_enterprise_oidc: true,
            enterprise_oidc_issuer: Some("https://issuer.invalid"),
            login_label: Some("legacy login"),
            has_auth_provider_command: true,
            preferred_method,
        }
    }

    /// The fork-level invariant: no combination of inputs can produce anything but `xai.api_key`.
    ///
    /// 3 pins x 2 `has_external_api_key` x 2 `has_cached_token` x 2 `has_enterprise_oidc`
    /// x 2 `login_label` x 2 `has_auth_provider_command` = 96 combinations. All the upstream
    /// "advertise the session/login method instead" branches lived in this input space, so the
    /// cross-product is what proves none of them can come back.
    #[test]
    fn every_configuration_advertises_only_api_key() {
        let flags = [false, true];
        let preferred_methods = [
            None,
            Some(PreferredAuthMethod::ApiKey),
            Some(PreferredAuthMethod::Oidc),
        ];
        let labels = [None, Some("legacy login")];

        let mut seen = 0_usize;
        for preferred_method in preferred_methods {
            for has_external_api_key in flags {
                for has_cached_token in flags {
                    for has_enterprise_oidc in flags {
                        for login_label in labels {
                            for has_auth_provider_command in flags {
                                seen += 1;
                                // inputs() starts from every flag on, so a field that
                                // upstream used to branch on cannot be silently ignored.
                                let mut build_inputs = inputs(preferred_method);
                                build_inputs.has_external_api_key = has_external_api_key;
                                build_inputs.has_cached_token = has_cached_token;
                                build_inputs.has_enterprise_oidc = has_enterprise_oidc;
                                build_inputs.enterprise_oidc_issuer = if has_enterprise_oidc {
                                    Some("https://issuer.invalid")
                                } else {
                                    None
                                };
                                build_inputs.login_label = login_label;
                                build_inputs.has_auth_provider_command = has_auth_provider_command;
                                let built = build_auth_methods(build_inputs);
                                let ids: Vec<&str> =
                                    built.methods.iter().map(|m| m.id().0.as_ref()).collect();
                                assert_eq!(
                                    ids,
                                    vec![XAI_API_KEY_METHOD_ID],
                                    "preferred_method={preferred_method:?} \
                                     external={has_external_api_key} cached={has_cached_token} \
                                     oidc={has_enterprise_oidc} label={login_label:?} \
                                     external_provider={has_auth_provider_command}",
                                );
                                assert_eq!(
                                    built
                                        .default_auth_method_id
                                        .as_ref()
                                        .map(|id| id.0.as_ref()),
                                    Some(XAI_API_KEY_METHOD_ID),
                                    "the default id must never be None: a BYOK-only build has \
                                     no interactive fallback to fall back to",
                                );
                                assert!(
                                    !AuthMethodKind::from_id(built.methods[0].id())
                                        .needs_interactive_login(),
                                    "no configuration may require interactive login",
                                );
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(seen, 96, "the cross-product must cover every input");
    }

    /// Every legacy session fallthrough resolves to the API-key method, for each input.
    #[test]
    fn legacy_session_fallthrough_is_api_key_only() {
        for has_external_api_key in [false, true] {
            for preferred_method in [
                None,
                Some(PreferredAuthMethod::ApiKey),
                Some(PreferredAuthMethod::Oidc),
            ] {
                assert_eq!(
                    method_id_after_cached_token_unavailable(
                        has_external_api_key,
                        preferred_method,
                    ),
                    Some(XAI_API_KEY_METHOD_ID),
                    "has_external_api_key={has_external_api_key} preferred={preferred_method:?}",
                );
            }
        }
    }
}
