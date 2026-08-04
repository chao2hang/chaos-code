//! Sampler configuration types.
//!
//! [`SamplerConfig`] is the per-request configuration handed to the
//! sampler. It deliberately does **not** alias
//! `xai_grok_sampling_types::SamplingConfig` so that the sampler crate
//! avoids transitive dependencies on shell-specific types
//! (`xai-grok-tools`, etc.).

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use xai_grok_sampling_types::{
    ApiBackend, CompactionAtTokens, CompactionsRemaining, DoomLoopRecoveryPolicy, ReasoningEffort,
};

use crate::attribution::SharedAttributionCallback;
use crate::retry::{DEFAULT_MAX_RETRIES, RATE_LIMIT_RETRY_THRESHOLD};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthScheme {
    #[default]
    Bearer,
    XApiKey,
}

/// All knobs that control a single sampling request.
///
/// The session typically owns one `SamplerConfig` per active model
/// and passes it (or a per-request override) to the actor on every
/// submit.
///
/// # Construction in `xai-grok-shell`
///
/// `SamplerConfig` is the single source of truth for sampler
/// configuration. The shell builds it directly (see
/// `agent::config::resolve_model_to_sampling_config` and
/// `session::acp_session::SessionActor::reconstruct_full_config`) by
/// composing chat-state's `xai_grok_sampling_types::SamplingConfig`
/// with `Credentials` (api key, client version).
///
/// URL-derived request headers (e.g. `X-XAI-Token-Auth` for the
/// cli-chat-proxy) are
/// folded into [`Self::extra_headers`] by
/// `agent::config::inject_url_derived_headers` before the
/// `SamplerConfig` is handed to the actor. Auth is selected separately
/// via `auth_scheme`, while `api_backend` controls only the request/response
/// protocol shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplerConfig {
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: String,
    pub max_completion_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub api_backend: ApiBackend,
    #[serde(default)]
    pub auth_scheme: AuthScheme,
    /// Extra request headers applied verbatim. The sampler never inspects
    /// the URL to derive headers; callers (the session) inject proxy auth
    /// and other access headers here before constructing the config.
    pub extra_headers: IndexMap<String, String>,
    /// Query parameters folded into every request URL (percent-encoded).
    #[serde(default)]
    pub query_params: IndexMap<String, String>,
    /// Header name to environment variable, resolved into request headers at
    /// client build and never persisted.
    #[serde(default)]
    pub env_http_headers: IndexMap<String, String>,
    /// Total context window size in tokens. The sampler does not enforce
    /// it; it is informational metadata used by the session for compaction
    /// decisions.
    pub context_window: u64,
    pub force_http1: bool,
    pub max_retries: Option<u32>,
    pub stream_tool_calls: bool,
    /// When true, the chat-completions stream parser scans
    /// `delta.content` for inline `<think>...</think>` pseudo-XML tags
    /// (DeepSeek-R1, Qwen3-Thinking, GLM-Z1 and other Chinese reasoning
    /// models that emit reasoning inline in `content` instead of via a
    /// structured `reasoning_content` field) and routes the wrapped
    /// text through the reasoning channel. The TUI then renders it as
    /// a foldable thought block, the same as native `reasoning_content`.
    ///
    /// Partial-buffer safe: tags split across SSE chunks are
    /// re-assembled; an unclosed `<think>` at stream end is flushed as
    /// reasoning (covers `max_tokens` truncation). When false (the
    /// default), `delta.content` is passed through unchanged — zero
    /// overhead, zero behavior change.
    #[serde(default)]
    pub extract_inline_thinking: bool,
    pub idle_timeout_secs: Option<u64>,

    // Reasoning effort
    pub reasoning_effort: Option<ReasoningEffort>,

    // Client identity
    pub origin_client: Option<OriginClientInfo>,
    pub client_identifier: Option<String>,
    pub deployment_id: Option<String>,
    pub user_id: Option<String>,
    pub client_version: Option<String>,
    /// Verbatim `User-Agent` header override. When set, the client sends it
    /// as-is (spaces and all) instead of rendering one from `origin_client`.
    /// Useful for mimicking an existing client environment such as WorkBuddy.
    #[serde(default)]
    pub user_agent: Option<String>,

    /// Optional hook invoked at every UNAUTHORIZED (401) response
    /// site. The sampler passes the bearer that was actually sent on
    /// the wire to the callback; the implementation is free to do
    /// whatever it wants with it (typically: join it with a live
    /// credential source and emit an attribution event for diagnosis
    /// of stale-token vs. server-rejected-live-token 401s). `None`
    /// (default) is a no-op -- the 401 arm returns the same
    /// `SamplingError::Auth` it always did.
    ///
    /// `Arc<dyn Trait>` is not serializable, so the field is skipped
    /// in (de)serialization. Round-tripping a config through serde
    /// drops the callback; callers that deserialize a `SamplerConfig`
    /// from disk must re-attach the callback before passing it to
    /// [`crate::SamplingClient::new`] or 401 attribution will be
    /// silently disabled for the rebuilt client.
    #[serde(skip)]
    pub attribution_callback: Option<SharedAttributionCallback>,

    /// Live bearer resolve per request. `None` uses construction-time `api_key`.
    #[serde(skip)]
    pub bearer_resolver: Option<SharedBearerResolver>,

    #[serde(default)]
    pub supports_backend_search: bool,

    /// Per-model config for the `x-compactions-remaining` header; `None` disables it.
    #[serde(default)]
    pub compactions_remaining: Option<CompactionsRemaining>,

    /// Per-model config for the `x-compaction-at` header; `None` disables it.
    #[serde(default)]
    pub compaction_at_tokens: Option<CompactionAtTokens>,

    /// Server-side doom-loop check policy; `None` disables it. When set, the
    /// client itself sends the opt-in `x-grok-doom-loop-check` header on
    /// streaming Responses API requests and absorbs the reported trigger
    /// events (unlike the environment headers in [`Self::extra_headers`],
    /// this header gates the client's own decode behavior, so it lives with
    /// the decoder).
    #[serde(default)]
    pub doom_loop_recovery: Option<DoomLoopRecoveryPolicy>,

    /// Per-request header injector (e.g. OTel traceparent). Called in `post()`.
    #[serde(skip)]
    pub header_injector: Option<SharedHeaderInjector>,

    /// When true, no x-grok-* headers are added, and only the WorkBuddy
    /// headers from `extra_headers`/`env_http_headers` are sent.
    #[serde(default)]
    pub is_workbuddy: bool,

    /// Native CatPaw channel settings. When set (and `api_backend == CatPaw`),
    /// requests go through the encrypted CatPaw protocol. The account token
    /// is resolved per request via [`CatPawSamplerConfig::account_resolver`]
    /// and never persisted here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catpaw: Option<CatPawSamplerConfig>,

    /// CatPaw Remote Agent settings. When set (and `api_backend ==
    /// RemoteAgent`), requests go through the encrypted Remote Agent
    /// protocol with repository-scoped execution. Conversation ids
    /// are persisted across turns via
    /// [`RemoteAgentSamplerConfig::conversation_state`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_agent: Option<RemoteAgentSamplerConfig>,
}

/// Native CatPaw channel settings carried on [`SamplerConfig`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatPawSamplerConfig {
    /// Provider label (for account-pool correlation).
    pub provider: String,
    /// `userModelTypeCode` from the CatPaw model catalog.
    pub model_type_code: i32,
    /// Per-request account resolution; never serialized (tokens live in the
    /// encrypted account store, resolved fresh for every request).
    #[serde(skip)]
    pub account_resolver: Option<SharedCatPawAccountResolver>,
}

/// Resolves a live CatPaw account credential per request. Returns
/// `(access_token, mis_id)`.
pub trait CatPawAccountResolver: Send + Sync + std::fmt::Debug {
    fn resolve(&self) -> Option<(String, String)>;
}

pub type SharedCatPawAccountResolver = std::sync::Arc<dyn CatPawAccountResolver>;

/// Default resolver that never produces a credential. Used when a CatPaw
/// config is built without a live account resolver; requests fail with a
/// clear "no account" error instead of sending an empty token.
#[derive(Debug)]
pub struct NoCatPawAccountResolver;

impl CatPawAccountResolver for NoCatPawAccountResolver {
    fn resolve(&self) -> Option<(String, String)> {
        None
    }
}

/// Native CatPaw Remote Agent settings carried on [`SamplerConfig`].
///
/// The Remote Agent protocol drives repository-scoped execution through
/// the encrypted CatPaw transport. Conversation state (the upstream
/// `conversationId` returned by `create_agent`) is shared across
/// turns via [`SharedRemoteAgentConversationState`] so subsequent turns
/// call `continue_agent` rather than spawning a fresh conversation
/// every request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteAgentSamplerConfig {
    /// Provider label (for account-pool correlation).
    pub provider: String,
    /// `userModelTypeCode` from the CatPaw model catalog.
    pub model_type_code: i32,
    /// Git repository URL (must be on the allowed internal host).
    pub git_repo_url: String,
    /// Base branch the agent diffs against.
    pub git_base_branch: String,
    /// Checkout branch the agent operates on.
    #[serde(default)]
    pub git_checkout_branch: String,
    /// Per-request account resolution; never serialized (tokens live in the
    /// encrypted account store, resolved fresh for every request).
    #[serde(skip)]
    pub account_resolver: Option<SharedRemoteAgentAccountResolver>,
    /// Per-session conversation id state; never serialized. Shared with
    /// the shell session store so turns within the same session reuse
    /// the upstream `conversationId`.
    #[serde(skip)]
    pub conversation_state: Option<SharedRemoteAgentConversationState>,
}

impl Default for RemoteAgentSamplerConfig {
    fn default() -> Self {
        Self {
            provider: String::new(),
            model_type_code: 0,
            git_repo_url: String::new(),
            git_base_branch: "master".to_string(),
            git_checkout_branch: String::new(),
            account_resolver: None,
            conversation_state: None,
        }
    }
}

/// Resolves a live CatPaw account credential per request. Returns
/// `(access_token, mis_id)`. Reused by the Remote Agent path because
/// the same encrypted account store signs both protocols.
pub trait RemoteAgentAccountResolver: Send + Sync + std::fmt::Debug {
    fn resolve(&self) -> Option<(String, String)>;
}

pub type SharedRemoteAgentAccountResolver = std::sync::Arc<dyn RemoteAgentAccountResolver>;

/// Default resolver that never produces a credential.
#[derive(Debug)]
pub struct NoRemoteAgentAccountResolver;

impl RemoteAgentAccountResolver for NoRemoteAgentAccountResolver {
    fn resolve(&self) -> Option<(String, String)> {
        None
    }
}

/// Mutable, `Send + Sync` handle to the per-session CatPaw Remote Agent
/// conversation id. The shell session sets the id after the first
/// create_agent response; subsequent turns call `continue_agent` with
/// that id instead of opening a new conversation.
///
/// `None` means "no conversation yet" (first turn or after an explicit
/// reset); the sampler treats this as "call create_agent". A `Some(_)
/// id` means "call continue_agent with this id".
#[derive(Debug, Default)]
pub struct RemoteAgentConversationState {
    inner: std::sync::Mutex<Option<String>>,
}

impl RemoteAgentConversationState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Convenience wrapper that returns an `Arc<Self>` for direct use
    /// as a `SharedRemoteAgentConversationState` without the caller
    /// having to wrap manually.
    pub fn new_shared() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self::new())
    }

    /// Read the cached conversation id, if any.
    pub fn get(&self) -> Option<String> {
        self.inner.lock().ok().and_then(|guard| guard.clone())
    }

    /// Cache the conversation id returned by the upstream `create_agent`
    /// response so subsequent turns reuse it.
    pub fn set(&self, id: impl Into<String>) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = Some(id.into());
        }
    }

    /// Drop the cached conversation id (e.g. on auth failure or explicit
    /// session reset).
    pub fn clear(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = None;
        }
    }
}

pub type SharedRemoteAgentConversationState = std::sync::Arc<RemoteAgentConversationState>;

impl Default for SamplerConfig {
    /// Empty defaults so callers can use `..Default::default()` and
    /// new fields don't ripple through every literal site.
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: String::new(),
            model: String::new(),
            max_completion_tokens: None,
            temperature: None,
            top_p: None,
            api_backend: ApiBackend::default(),
            auth_scheme: AuthScheme::default(),
            extra_headers: IndexMap::new(),
            query_params: IndexMap::new(),
            env_http_headers: IndexMap::new(),
            context_window: 0,
            force_http1: false,
            max_retries: None,
            stream_tool_calls: false,
            extract_inline_thinking: false,
            idle_timeout_secs: None,
            reasoning_effort: None,
            origin_client: None,
            client_identifier: None,
            deployment_id: None,
            user_id: None,
            client_version: None,
            user_agent: None,
            attribution_callback: None,
            bearer_resolver: None,
            supports_backend_search: false,
            compactions_remaining: None,
            compaction_at_tokens: None,
            doom_loop_recovery: None,
            header_injector: None,
            is_workbuddy: false,
            catpaw: None,
            remote_agent: None,
        }
    }
}

/// Cheap sync read of the current bearer for [`SamplerConfig::bearer_resolver`].
pub trait BearerResolver: Send + Sync + std::fmt::Debug {
    fn current_bearer(&self) -> Option<String>;
}

pub type SharedBearerResolver = std::sync::Arc<dyn BearerResolver>;

/// Per-request header injection (e.g. OTel `traceparent`).
pub trait HeaderInjector: Send + Sync + std::fmt::Debug {
    fn inject(&self, headers: &mut reqwest::header::HeaderMap);
}

pub type SharedHeaderInjector = std::sync::Arc<dyn HeaderInjector>;

/// Retry knobs for the sampler's internal transport-error retry loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of retries before giving up.
    pub max_retries: u32,
    /// After this many rate-limit (429) retries, escalate to the caller.
    /// Lower than `max_retries` because rate-limit waits can be long.
    pub rate_limit_retry_threshold: u32,
    #[serde(default)]
    pub retry_only_before_output: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: DEFAULT_MAX_RETRIES,
            rate_limit_retry_threshold: RATE_LIMIT_RETRY_THRESHOLD,
            retry_only_before_output: false,
        }
    }
}

/// Identity of the client that originated the request, used for
/// User-Agent rendering. The shell layer composes this with platform
/// info into a final UA string.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OriginClientInfo {
    pub product: String,
    pub version: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_policy_defaults() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_retries, DEFAULT_MAX_RETRIES);
        assert_eq!(
            policy.rate_limit_retry_threshold,
            RATE_LIMIT_RETRY_THRESHOLD
        );
    }

    #[test]
    fn inline_thinking_is_default_off_for_old_and_default_configs() {
        assert!(!SamplerConfig::default().extract_inline_thinking);

        let mut serialized = serde_json::to_value(SamplerConfig::default()).unwrap();
        serialized
            .as_object_mut()
            .unwrap()
            .remove("extract_inline_thinking");
        let config: SamplerConfig = serde_json::from_value(serialized).unwrap();
        assert!(!config.extract_inline_thinking);
    }

    /// Configs serialized before the field existed must keep deserializing.
    #[test]
    fn config_without_doom_loop_recovery_deserializes_to_none() {
        let mut stripped = serde_json::to_value(SamplerConfig::default()).unwrap();
        stripped
            .as_object_mut()
            .unwrap()
            .remove("doom_loop_recovery");
        let config: SamplerConfig = serde_json::from_value(stripped).unwrap();
        assert!(config.doom_loop_recovery.is_none());

        let with_policy = SamplerConfig {
            doom_loop_recovery: Some(DoomLoopRecoveryPolicy {
                max_threshold: 8,
                max_retries: 2,
            }),
            ..Default::default()
        };
        let round_tripped: SamplerConfig =
            serde_json::from_value(serde_json::to_value(&with_policy).unwrap()).unwrap();
        assert_eq!(
            round_tripped.doom_loop_recovery,
            with_policy.doom_loop_recovery
        );
    }
}
