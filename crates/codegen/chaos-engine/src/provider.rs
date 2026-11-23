//! Host-configured inference provider.
//!
//! The client protocol deliberately never carries an API key, so the credential
//! lives on the host side only. These types read it from the process
//! environment and talk to an OpenAI-compatible endpoint. Nothing here is
//! reachable from a client message: the base URL, model and key are fixed by
//! whoever started the process.

use crate::PromptAdapter;
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader},
    time::Duration,
};

/// Connect failures should surface fast; the whole turn gets a generous
/// budget because a long generation legitimately takes minutes.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
/// A model catalogue is a readout, not a turn. The client connection's frame
/// worker waits for it, so an endpoint that accepts the connection and then goes
/// quiet gets seconds rather than the five minutes a real generation may take.
const PROBE_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
/// Kept small because the body is echoed into a GUI error message.
const MAX_ERROR_BODY_BYTES: usize = 512;
/// A single turn larger than this is treated as a broken or hostile endpoint.
const MAX_STREAM_BYTES: u64 = 8 * 1024 * 1024;

/// Environment variables read by [`HttpPromptAdapter::from_env`].
pub const ENV_BASE_URL: &str = "CHAOS_PROVIDER_BASE_URL";
pub const ENV_MODEL: &str = "CHAOS_PROVIDER_MODEL";
pub const ENV_API_KEY: &str = "CHAOS_PROVIDER_API_KEY";

/// Result of asking a configured provider which models it serves.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderHealth {
    pub reachable: bool,
    pub model_ids: Vec<String>,
    pub configured_model_known: bool,
    /// Human-readable, credential-free reason when `reachable` is false.
    pub detail: Option<String>,
}

/// A prompt adapter that calls an OpenAI-compatible `/chat/completions`
/// endpoint and streams the reply back as SSE deltas.
///
/// The blocking HTTP client is built per call on purpose: `reqwest::blocking`
/// owns a tokio runtime, and the engine holding this adapter is cloned into
/// async tasks, so a long-lived client could end up dropped inside an async
/// context, which tokio refuses with a panic.
pub struct HttpPromptAdapter {
    base_url: String,
    chat_endpoint: String,
    models_endpoint: String,
    model: String,
    api_key: Option<String>,
}

impl std::fmt::Debug for HttpPromptAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpPromptAdapter")
            .field("chat_endpoint", &self.chat_endpoint)
            .field("model", &self.model)
            .field(
                "api_key",
                if self.api_key.is_some() {
                    &"[configured]"
                } else {
                    &"[none]"
                },
            )
            .finish()
    }
}

/// Rejects endpoints that could smuggle a credential or redirect the request.
///
/// `http://` is accepted only for loopback hosts, which is what a local model
/// server or a test fixture uses; anything else must be `https://`.
fn validated_endpoints(base_url: &str) -> Result<(String, String), String> {
    if base_url.trim().is_empty() {
        return Err(format!("{ENV_BASE_URL} 不能为空"));
    }
    let parsed = reqwest::Url::parse(base_url)
        .map_err(|error| format!("{ENV_BASE_URL} 不是合法 URL: {error}"))?;
    let scheme = parsed.scheme();
    let host = parsed
        .host_str()
        .ok_or_else(|| format!("{ENV_BASE_URL} 缺少主机名"))?;
    // `Url::host_str` keeps the brackets around an IPv6 literal.
    let hostname = host.trim_start_matches('[').trim_end_matches(']');
    let loopback = hostname.eq_ignore_ascii_case("localhost")
        || hostname
            .strip_suffix(".localhost")
            .is_some_and(|prefix| !prefix.is_empty())
        || hostname
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    if scheme == "http" && !loopback {
        return Err(format!("{ENV_BASE_URL} 使用明文 http，仅允许回环地址"));
    }
    if scheme != "http" && scheme != "https" {
        return Err(format!("{ENV_BASE_URL} 协议必须是 http 或 https"));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(format!("{ENV_BASE_URL} 不得内嵌凭据"));
    }
    if parsed.fragment().is_some() {
        return Err(format!("{ENV_BASE_URL} 不得包含 fragment"));
    }
    if parsed.query().is_some() {
        return Err(format!("{ENV_BASE_URL} 不得包含查询参数"));
    }
    let base = base_url.trim_end_matches('/');
    Ok((format!("{base}/chat/completions"), format!("{base}/models")))
}

/// Whether a base URL is shaped like an endpoint this host may talk to at all:
/// `https://` anywhere, `http://` only on loopback, never with embedded
/// credentials. The GUI's provider validation asks this instead of keeping its
/// own stricter copy, which would reject a locally-run model server the operator
/// configured and tell the user their own endpoint was invalid.
#[must_use]
pub fn endpoint_shape_allowed(base_url: &str) -> bool {
    validated_endpoints(base_url).is_ok()
}

/// 从一帧 SSE 数据里读 Provider 回报的 token 用量。
///
/// 认得三种口径，字段缺了就是 0：
/// - OpenAI Chat Completions：`usage.prompt_tokens` / `completion_tokens`，
///   细节在 `prompt_tokens_details.cached_tokens` 与
///   `completion_tokens_details.reasoning_tokens`；
/// - Anthropic 兼容网关：`usage.input_tokens` / `output_tokens` /
///   `cache_creation_input_tokens` / `cache_read_input_tokens`；
/// - 只有 `total_tokens` 的极简网关：input/output 记 0，总量进 output，
///   让"有账"至少可见。
///
/// 整帧没有 `usage`（或 `usage` 为 null）时返回 `None`——不估算。
fn usage_of(payload: &serde_json::Value) -> Option<crate::ProviderUsage> {
    let usage = payload.get("usage")?;
    if !usage.is_object() {
        return None;
    }
    let count = |key: &str| -> u64 {
        usage
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    };
    let nested_count = |parent: &str, key: &str| -> u64 {
        usage
            .get(parent)
            .and_then(|details| details.get(key))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    };
    let input = count("prompt_tokens").max(count("input_tokens"));
    let output = count("completion_tokens").max(count("output_tokens"));
    let cached = nested_count("prompt_tokens_details", "cached_tokens")
        .max(count("cache_read_input_tokens"));
    let cache_creation = count("cache_creation_input_tokens");
    if input == 0 && output == 0 && cached == 0 && cache_creation == 0 {
        // 没有可读的明细就不硬造账目；total_tokens 网关单独兜底。
        let total = count("total_tokens");
        if total == 0 {
            return None;
        }
        return Some(crate::ProviderUsage {
            output_tokens: total,
            ..crate::ProviderUsage::default()
        });
    }
    // Chat Completions and Responses APIs report input as the full prompt, with
    // cached tokens as a subset. Anthropic-compatible APIs report uncached input
    // separately from cache reads/creation, so add those components only there.
    let anthropic_cache_fields = usage.get("cache_read_input_tokens").is_some()
        || usage.get("cache_creation_input_tokens").is_some();
    let cache_total = usage
        .get("cache_total_tokens")
        .and_then(serde_json::Value::as_u64)
        .or_else(|| {
            usage
                .get("cacheTotalTokens")
                .and_then(serde_json::Value::as_u64)
        })
        .unwrap_or_else(|| {
            if anthropic_cache_fields {
                input.saturating_add(cached).saturating_add(cache_creation)
            } else {
                input
            }
        });
    Some(crate::ProviderUsage {
        input_tokens: input,
        output_tokens: output,
        reasoning_tokens: nested_count("completion_tokens_details", "reasoning_tokens"),
        cached_tokens: cached,
        cache_creation_tokens: cache_creation,
        cache_total_tokens: cache_total,
    })
}

impl HttpPromptAdapter {
    /// Builds an adapter from an explicit base URL, model slug and optional
    /// bearer token. `base_url` is validated by [`validated_endpoints`].
    pub fn new(
        base_url: &str,
        model: impl Into<String>,
        api_key: Option<impl Into<String>>,
    ) -> Result<Self, String> {
        let (chat_endpoint, models_endpoint) = validated_endpoints(base_url)?;
        let model = model.into();
        if model.trim().is_empty() {
            return Err(format!("{ENV_MODEL} 不能为空"));
        }
        let api_key = api_key
            .map(Into::into)
            .filter(|key: &String| !key.is_empty());
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            chat_endpoint,
            models_endpoint,
            model,
            api_key,
        })
    }

    fn client(&self) -> Result<reqwest::blocking::Client, String> {
        self.client_with(CONNECT_TIMEOUT, REQUEST_TIMEOUT)
    }

    /// The catalogue read gets a short budget: an endpoint that accepts the
    /// connection and then stops answering must not hold the client connection's
    /// frame worker for the five minutes a real generation is allowed to take.
    fn probe_client(&self) -> Result<reqwest::blocking::Client, String> {
        self.client_with(PROBE_CONNECT_TIMEOUT, PROBE_TIMEOUT)
    }

    fn client_with(
        &self,
        connect: Duration,
        total: Duration,
    ) -> Result<reqwest::blocking::Client, String> {
        // The shared TLS policy loads the OS store, the Mozilla bundle and
        // `GROK_EXTRA_CA_BUNDLE`, so a self-hosted provider behind an
        // enterprise proxy or a private CA still verifies.
        xai_grok_extra_ca::build_blocking_reqwest_client(move |builder| {
            builder
                .connect_timeout(connect)
                .timeout(total)
                .redirect(reqwest::redirect::Policy::none())
        })
        .map_err(|error| format!("无法初始化 HTTP 客户端: {error}"))
    }

    /// Builds an adapter from the process environment. Returns `Ok(None)` when
    /// no provider is configured, so a host can fall back to its default.
    pub fn from_env() -> Result<Option<Self>, String> {
        // If CHAOS_DISABLE_CONFIG_FALLBACK is set, skip reading ~/.chaos/config.toml (used by integration tests).
        let disable_config = std::env::var("CHAOS_DISABLE_CONFIG_FALLBACK")
            .map(|v| v == "1" || v == "true")
            .unwrap_or(false);

        // Explicit process configuration is an operator override (and is useful
        // to tests); otherwise use Chaos CLI's own config.toml.
        let env_base = std::env::var(ENV_BASE_URL)
            .ok()
            .filter(|s| !s.trim().is_empty());
        if let Some(base_url) = env_base {
            return Self::from_parts(
                Some(&base_url),
                &std::env::var(ENV_MODEL).unwrap_or_default(),
                std::env::var(ENV_API_KEY).ok().as_deref(),
            );
        }
        if !disable_config && let Some((base_url, model, api_key)) = Self::from_chaos_config() {
            return Self::from_parts(Some(&base_url), &model, Some(&api_key));
        }
        Ok(None)
    }

    /// Read `~/.chaos/config.toml`: the `[model_providers.*]` entry referenced
    /// by the default model supplies base_url and api_key, and
    /// `[models].default` picks the model. `None` when the file is missing or
    /// does not describe a usable provider.
    fn from_chaos_config() -> Option<(String, String, String)> {
        let home = std::env::var("HOME").ok()?;
        let text =
            std::fs::read_to_string(std::path::Path::new(&home).join(".chaos/config.toml")).ok()?;
        let unquote = |v: &str| -> String {
            let t = v.trim();
            t.strip_prefix('"')
                .and_then(|r| r.strip_suffix('"'))
                .unwrap_or(t)
                .to_owned()
        };
        let mut default_model = None;
        // provider id → (base_url, api_key)
        let mut providers: Vec<(String, Option<String>, Option<String>)> = Vec::new();
        let mut current_provider: Option<String> = None;
        // config key → (API model id, provider id)
        let mut model_entries: Vec<(String, String, String)> = Vec::new();
        let mut in_models = false;
        let mut current_model: Option<(String, Option<String>, Option<String>)> = None;
        for raw in text.lines() {
            let line = raw.trim();
            if line.starts_with('[') && line.ends_with(']') {
                in_models = false;
                if let Some((alias, Some(model_id), Some(provider_id))) = current_model.take() {
                    model_entries.push((alias, model_id, provider_id));
                } else {
                    current_model = None;
                }
                current_provider = None;
                let inner = &line[1..line.len() - 1];
                if inner == "models" {
                    in_models = true;
                } else if let Some(rest) = inner.strip_prefix("model.") {
                    current_model = Some((rest.trim().trim_matches('"').to_owned(), None, None));
                } else if let Some(rest) = inner.strip_prefix("model_providers.") {
                    current_provider = Some(rest.trim().to_owned());
                    providers.push((rest.trim().to_owned(), None, None));
                }
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = unquote(value);
            if in_models && key == "default" {
                default_model = Some(value);
            } else if let Some(id) = current_provider.as_ref() {
                let entry = providers
                    .iter_mut()
                    .find(|(pid, _, _)| pid == id)
                    .map(|(_, b, k)| (b, k))?;
                if key == "base_url" {
                    *entry.0 = Some(value);
                } else if key == "api_key" {
                    *entry.1 = Some(value);
                }
            } else if let Some((_, model_id, provider_id)) = current_model.as_mut() {
                match key {
                    "model" => *model_id = Some(value),
                    "model_provider" => *provider_id = Some(value),
                    _ => {}
                }
            }
        }
        if let Some((alias, Some(model_id), Some(provider_id))) = current_model.take() {
            model_entries.push((alias, model_id, provider_id));
        }
        let default_model = default_model?;
        let (_, model_id, provider_id) = model_entries
            .into_iter()
            .find(|(alias, _, _)| alias == &default_model)?;
        let (_, base_url, api_key) = providers
            .into_iter()
            .find(|(pid, _, _)| pid == &provider_id)?;
        Some((base_url?, model_id, api_key?))
    }

    /// [`Self::from_env`] without touching the process environment, so the
    /// "not configured" branch is testable without racing other tests.
    pub fn from_parts(
        base_url: Option<&str>,
        model: &str,
        api_key: Option<&str>,
    ) -> Result<Option<Self>, String> {
        let Some(base_url) = base_url.filter(|value| !value.trim().is_empty()) else {
            return Ok(None);
        };
        Ok(Some(Self::new(
            base_url,
            model,
            api_key.map(str::to_string),
        )?))
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn chat_endpoint(&self) -> &str {
        &self.chat_endpoint
    }

    fn authorize(
        &self,
        request: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
        match &self.api_key {
            Some(key) => request.bearer_auth(key),
            None => request,
        }
    }

    /// Keeps the credential out of anything shown to a user. Providers echo
    /// parts of the request back in their error bodies.
    fn redact(&self, text: &str) -> String {
        match &self.api_key {
            Some(key) if key.len() >= 4 => text.replace(key.as_str(), "[redacted]"),
            _ => text.to_string(),
        }
    }

    /// Truncates on a UTF-8 boundary: error bodies are arbitrary provider bytes.
    fn clip(text: &str) -> String {
        if text.len() <= MAX_ERROR_BODY_BYTES {
            return text.to_string();
        }
        let mut end = MAX_ERROR_BODY_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &text[..end])
    }

    fn status_hint(status: reqwest::StatusCode) -> &'static str {
        match status.as_u16() {
            401 => "凭据被拒绝，请检查服务端的 API Key 配置",
            403 => "凭据无权访问该模型",
            404 => "端点不存在，请检查 Base URL 是否包含 /v1",
            429 => "已被限流，请稍后重试",
            500..=599 => "Provider 服务端错误，请稍后重试",
            _ => "Provider 返回错误",
        }
    }

    /// Extracts the assistant text of one SSE data frame.
    fn delta_of(payload: &serde_json::Value) -> Option<String> {
        let choice = payload.get("choices")?.as_array()?.first()?;
        if let Some(text) = choice
            .get("delta")
            .and_then(|delta| delta.get("content"))
            .and_then(serde_json::Value::as_str)
        {
            return Some(text.to_string());
        }
        // Servers that ignore `stream` still answer with a full message.
        choice
            .get("message")
            .and_then(|message| message.get("content"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    }

    /// Reads the SSE response and hands each delta to `emit` as its frame arrives,
    /// which is what makes the answer appear while the Provider is still writing
    /// it. `emit` returns `false` when the caller has stopped listening.
    ///
    /// Returns the token usage the Provider reported (usually on the last frame):
    /// `None` means the endpoint never said, which the usage stats honestly
    /// record as zero rather than estimate.
    fn send_and_stream(
        &self,
        body: serde_json::Value,
        emit: &mut dyn FnMut(&str) -> bool,
    ) -> Result<Option<crate::ProviderUsage>, String> {
        match self.send_and_stream_once(&body, emit) {
            Ok(usage) => Ok(usage),
            Err(error) => {
                // The engine's own sampler corrects `reasoning_effort` and
                // retries when a provider 400s on the parameter
                // (xai-grok-sampler/retry.rs `is_reasoning_effort_error`).
                // This minimal client mirrors that policy: strip the
                // parameter when the provider rejects it outright, or pin it
                // to explicit `"none"` when the provider rejects function
                // tools unless reasoning is off. Without it, one strict
                // gateway would turn the picker into a turn-killer even
                // though the provider answers fine.
                if !Self::error_rejects_reasoning_effort(&error) {
                    return Err(error);
                }
                let wants_none = Self::error_wants_reasoning_effort_none(&error);
                let mut corrected = body.clone();
                let corrected_body = corrected
                    .as_object_mut()
                    .expect("request body is an object");
                if wants_none {
                    if corrected_body
                        .get("reasoning_effort")
                        .and_then(|v| v.as_str())
                        == Some("none")
                    {
                        return Err(error);
                    }
                    corrected_body.insert(
                        "reasoning_effort".to_string(),
                        serde_json::Value::String("none".to_string()),
                    );
                } else if corrected_body.remove("reasoning_effort").is_none() {
                    return Err(error);
                }
                tracing::info!(
                    "provider rejected reasoning_effort; corrected it before one retry: {error}"
                );
                self.send_and_stream_once(&corrected, emit)
            }
        }
    }

    /// Whether an HTTP-level failure is the known "this provider does not take
    /// reasoning_effort" shape (400 whose body names the parameter), same
    /// wording family as `SamplingError::is_reasoning_effort_error`.
    fn error_rejects_reasoning_effort(error: &str) -> bool {
        error.contains("HTTP 400")
            && (error.contains("reasoning_effort")
                || error.contains("reasoning.effort")
                || error.contains("unsupported parameter: reasoning")
                || error.contains("unknown parameter: reasoning"))
    }

    /// Whether the provider demands an explicit `"none"` rather than omitting
    /// the parameter (same wording as `SamplingError::wants_reasoning_effort_none`).
    fn error_wants_reasoning_effort_none(error: &str) -> bool {
        // This error still contains the raw JSON body, so normalize escaped
        // double quotes before matching the same instruction as the sampler.
        // Merely naming `none` can mean the provider rejects it, not demands it.
        let error = error.replace("\\\"", "\"").to_ascii_lowercase();
        error.contains("set reasoning_effort to 'none'")
            || error.contains("set reasoning_effort to \"none\"")
    }

    fn send_and_stream_once(
        &self,
        body: &serde_json::Value,
        emit: &mut dyn FnMut(&str) -> bool,
    ) -> Result<Option<crate::ProviderUsage>, String> {
        let client = self.client()?;
        let request = self.authorize(client.post(&self.chat_endpoint).json(body));
        let response = request
            .send()
            .map_err(|error| format!("无法连接 Provider：{}", Self::transport_reason(&error)))?;
        let status = response.status();
        if !status.is_success() {
            let detail = response.text().unwrap_or_default();
            let detail = detail.trim();
            let clipped = Self::clip(detail);
            let hint = Self::status_hint(status);
            return Err(self.redact(&if clipped.is_empty() {
                format!("Provider 返回 HTTP {}：{hint}", status.as_u16())
            } else {
                format!(
                    "Provider 返回 HTTP {}：{hint}；响应：{}",
                    status.as_u16(),
                    clipped
                )
            }));
        }
        let mut reader = BufReader::new(std::io::Read::take(response, MAX_STREAM_BYTES)).lines();
        let mut produced = false;
        let mut provider_error: Option<String> = None;
        let mut usage: Option<crate::ProviderUsage> = None;
        while let Some(line) = reader
            .next()
            .transpose()
            .map_err(|error| format!("读取 Provider 响应失败：{error}"))?
        {
            let Some(payload) = line.trim_end().strip_prefix("data:").map(str::trim) else {
                continue;
            };
            if payload.is_empty() {
                continue;
            }
            if payload == "[DONE]" {
                break;
            }
            let value: serde_json::Value = serde_json::from_str(payload)
                .map_err(|error| format!("Provider 返回无法解析的数据帧：{error}"))?;
            if let Some(error) = value.get("error") {
                provider_error = Some(
                    error
                        .get("message")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("未提供原因")
                        .to_string(),
                );
                continue;
            }
            // 用量通常在最后一个数据帧上；每帧都看，最后一帧说了算。
            if let Some(reported) = usage_of(&value) {
                usage = Some(reported);
            }
            match Self::delta_of(&value) {
                Some(text) if !text.is_empty() => {
                    produced = true;
                    if !emit(&text) {
                        // The run was stopped. Returning here drops the response, and
                        // with it the connection, rather than draining a reply that
                        // nobody is going to show.
                        return Ok(usage);
                    }
                }
                _ => {}
            }
        }
        if let Some(message) = provider_error {
            return Err(format!("Provider 报告错误：{message}"));
        }
        if !produced {
            return Err("Provider 未返回任何文本".to_string());
        }
        Ok(usage)
    }

    /// Strips the URL from transport errors: a proxy could place credentials
    /// in it and the message ends up in the transcript.
    fn transport_reason(error: &reqwest::Error) -> String {
        if error.is_timeout() {
            "连接或读取超时".to_string()
        } else if error.is_connect() {
            "无法建立连接".to_string()
        } else {
            error
                .to_string()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        }
    }

    /// Asks the endpoint which models it serves. Never raises: the caller is a
    /// status readout, so a failure becomes structured detail instead.
    ///
    /// `selected` is the model whose availability is being asked about -- the
    /// GUI's current pick, not necessarily the slug this adapter was built with.
    /// Reporting against the wrong one would tell a user who just chose a model
    /// that their choice was unknown.
    pub fn probe_with(&self, selected: Option<&str>) -> ProviderHealth {
        let request = match self.probe_client() {
            Err(error) => {
                return ProviderHealth {
                    reachable: false,
                    detail: Some(error),
                    ..Default::default()
                };
            }
            Ok(client) => self.authorize(client.get(&self.models_endpoint)),
        };
        match request.send() {
            Err(error) => ProviderHealth {
                reachable: false,
                detail: Some(format!(
                    "无法连接 Provider：{}",
                    Self::transport_reason(&error)
                )),
                ..Default::default()
            },
            Ok(response) => {
                let status = response.status();
                if !status.is_success() {
                    return ProviderHealth {
                        reachable: false,
                        detail: Some(format!(
                            "Provider 返回 HTTP {}：{}",
                            status.as_u16(),
                            Self::status_hint(status)
                        )),
                        ..Default::default()
                    };
                }
                let body = response.text().unwrap_or_default();
                match serde_json::from_str::<serde_json::Value>(&body) {
                    Err(error) => ProviderHealth {
                        reachable: false,
                        detail: Some(self.redact(&format!("模型列表无法解析：{error}"))),
                        ..Default::default()
                    },
                    Ok(value) => {
                        let mut model_ids = value
                            .get("data")
                            .and_then(serde_json::Value::as_array)
                            .map(|items| {
                                items
                                    .iter()
                                    .filter_map(|item| {
                                        item.get("id").and_then(serde_json::Value::as_str)
                                    })
                                    .map(str::to_string)
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        model_ids.sort();
                        let wanted = selected.unwrap_or(&self.model);
                        let configured_model_known = model_ids.iter().any(|id| id == wanted);
                        ProviderHealth {
                            reachable: true,
                            model_ids,
                            configured_model_known,
                            detail: None,
                        }
                    }
                }
            }
        }
    }
    /// [`Self::probe_with`] for the model this adapter was built with.
    pub fn probe(&self) -> ProviderHealth {
        self.probe_with(None)
    }
}

impl PromptAdapter for HttpPromptAdapter {
    fn run_prompt(&self, prompt: &str, options: &crate::RunOptions) -> Result<Vec<String>, String> {
        let mut chunks = Vec::new();
        self.run_prompt_stream(prompt, options, &mut |chunk| {
            chunks.push(chunk.to_string());
            true
        })?;
        Ok(chunks)
    }

    fn run_prompt_stream(
        &self,
        prompt: &str,
        options: &crate::RunOptions,
        emit: &mut dyn FnMut(&str) -> bool,
    ) -> Result<(), String> {
        self.run_prompt_turn(prompt, options, emit).map(|_| ())
    }

    fn run_prompt_turn(
        &self,
        prompt: &str,
        options: &crate::RunOptions,
        emit: &mut dyn FnMut(&str) -> bool,
    ) -> Result<crate::TurnReport, String> {
        // A provider answers what it is sent. Sent one turn, it answers that turn
        // as though nothing came before — which is how a conversation the user
        // resumed ended up replying to its own middle. The conversation rides
        // along when the engine has one to give; when the CLI itself is
        // resuming the session it holds the context, and the engine sends
        // nothing extra.
        let mut messages: Vec<serde_json::Value> = options
            .history
            .iter()
            .filter(|message| message.role == "user" || message.role == "assistant")
            .map(|message| serde_json::json!({ "role": message.role, "content": message.text }))
            .collect();
        messages.push(serde_json::json!({ "role": "user", "content": prompt }));
        let mut body = serde_json::json!({
            "model": options.model_or(&self.model),
            "messages": messages,
            "stream": true,
        });
        // The thinking level the user picked reaches the wire here. `none` is a
        // real wire value (same convention as the engine's own request builder),
        // and a provider that rejects the parameter gets exactly one stripped
        // retry instead of failing the whole turn — see `send_and_stream`.
        if let Some(effort) = options
            .thought_level
            .as_deref()
            .map(str::trim)
            .filter(|level| !level.is_empty())
        {
            body["reasoning_effort"] = serde_json::Value::String(effort.to_string());
        }
        self.send_and_stream(body, emit)
            .map(|usage| crate::TurnReport { usage })
    }

    fn list_models(&self, selected: Option<&str>) -> crate::provider::ProviderHealth {
        self.probe_with(selected)
    }

    fn default_model(&self) -> Option<&str> {
        Some(&self.model)
    }

    fn base_url(&self) -> Option<&str> {
        Some(&self.base_url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(delta: &str) -> serde_json::Value {
        serde_json::json!({ "choices": [{ "delta": { "content": delta } }] })
    }

    #[test]
    fn cache_hit_denominator_respects_openai_and_anthropic_usage_shapes() {
        let openai = usage_of(&serde_json::json!({
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 20,
                "prompt_tokens_details": { "cached_tokens": 40 }
            }
        }))
        .unwrap();
        assert_eq!(openai.input_tokens, 100);
        assert_eq!(openai.cached_tokens, 40);
        assert_eq!(openai.cache_total_tokens, 100);

        let anthropic = usage_of(&serde_json::json!({
            "usage": {
                "input_tokens": 60,
                "output_tokens": 20,
                "cache_read_input_tokens": 30,
                "cache_creation_input_tokens": 10
            }
        }))
        .unwrap();
        assert_eq!(anthropic.input_tokens, 60);
        assert_eq!(anthropic.cached_tokens, 30);
        assert_eq!(anthropic.cache_creation_tokens, 10);
        assert_eq!(anthropic.cache_total_tokens, 100);
    }

    #[test]
    fn endpoints_are_derived_from_the_base_url() {
        let (chat, models) = validated_endpoints("http://127.0.0.1:4500/v1").unwrap();
        assert_eq!(chat, "http://127.0.0.1:4500/v1/chat/completions");
        assert_eq!(models, "http://127.0.0.1:4500/v1/models");
        // A trailing slash must not double up.
        let (chat, _) = validated_endpoints("http://localhost:8080/").unwrap();
        assert_eq!(chat, "http://localhost:8080/chat/completions");
    }

    #[test]
    fn plaintext_is_limited_to_loopback() {
        for rejected in [
            "http://example.com/v1",
            "http://10.0.0.5:11434/v1",
            "http://127.0.0.1.v1.invalid/v1",
        ] {
            let error = validated_endpoints(rejected).expect_err("must reject");
            assert!(error.contains("明文"), "{rejected} -> {error}");
        }
        for accepted in [
            "http://127.0.0.1:11434/v1",
            "http://localhost:11434/v1",
            "http://[::1]:11434/v1",
            "http://dev.localhost:11434/v1",
        ] {
            assert!(
                validated_endpoints(accepted).is_ok(),
                "{accepted} should be accepted"
            );
        }
        assert!(validated_endpoints("https://api.example.com/v1").is_ok());
    }

    #[test]
    fn credentials_and_fragments_in_the_url_are_rejected() {
        for rejected in [
            "https://user:pw@api.example.com/v1",
            "https://api.example.com/v1#frag",
            "https://api.example.com/v1?key=sk-secret",
            "ftp://127.0.0.1/v1",
            "   ",
            "not a url",
        ] {
            assert!(
                validated_endpoints(rejected).is_err(),
                "{rejected} should be rejected"
            );
        }
    }

    #[test]
    fn an_empty_model_slug_is_rejected() {
        let error = HttpPromptAdapter::new("http://127.0.0.1:1/v1", "  ", None::<String>)
            .expect_err("empty model must fail");
        assert!(error.contains(ENV_MODEL), "{error}");
    }

    #[test]
    fn debug_output_never_contains_the_key() {
        let adapter =
            HttpPromptAdapter::new("http://127.0.0.1:1/v1", "m", Some("sk-super-secret")).unwrap();
        let rendered = format!("{adapter:?}");
        assert!(!rendered.contains("sk-super-secret"), "{rendered}");
        assert!(rendered.contains("[configured]"), "{rendered}");
    }

    #[test]
    fn error_bodies_are_clipped_on_char_boundaries() {
        assert_eq!(HttpPromptAdapter::clip("short"), "short");
        let text = "错误".repeat(400);
        let clipped = HttpPromptAdapter::clip(&text);
        assert!(clipped.ends_with('…'));
        assert!(clipped.len() <= MAX_ERROR_BODY_BYTES + "…".len());
        assert!(clipped.starts_with("错误"));
    }

    #[test]
    fn the_key_is_redacted_from_messages() {
        let adapter =
            HttpPromptAdapter::new("http://127.0.0.1:1/v1", "m", Some("sk-abcdef")).unwrap();
        let message = adapter.redact("upstream echoed sk-abcdef back to us");
        assert_eq!(message, "upstream echoed [redacted] back to us");
    }

    #[test]
    fn status_codes_carry_actionable_hints() {
        for (code, needle) in [
            (401u16, "API Key"),
            (404, "/v1"),
            (429, "限流"),
            (503, "服务端"),
        ] {
            let hint = HttpPromptAdapter::status_hint(reqwest::StatusCode::from_u16(code).unwrap());
            assert!(hint.contains(needle), "{code} -> {hint}");
        }
    }

    #[test]
    fn deltas_come_from_streaming_or_non_streaming_frames() {
        assert_eq!(
            HttpPromptAdapter::delta_of(&frame("hel")).as_deref(),
            Some("hel")
        );
        // An empty delta is a real frame (tool calls, role-only openers).
        let role_only = serde_json::json!({ "choices": [{ "delta": { "role": "assistant" } }] });
        assert_eq!(HttpPromptAdapter::delta_of(&role_only), None);
        let whole = serde_json::json!({ "choices": [{ "message": { "content": "all of it" } }] });
        assert_eq!(
            HttpPromptAdapter::delta_of(&whole).as_deref(),
            Some("all of it")
        );
        assert_eq!(HttpPromptAdapter::delta_of(&serde_json::json!({})), None);
        assert_eq!(
            HttpPromptAdapter::delta_of(&serde_json::json!({ "choices": [] })),
            None
        );
    }

    #[test]
    fn an_unconfigured_or_misconfigured_provider_is_reported_not_panicked() {
        assert!(
            HttpPromptAdapter::from_parts(None, "m", None)
                .unwrap()
                .is_none()
        );
        assert!(
            HttpPromptAdapter::from_parts(Some("   "), "m", None)
                .unwrap()
                .is_none()
        );
        // Configured but unusable: this is a startup error, not a silent echo.
        let error = HttpPromptAdapter::from_parts(Some("http://example.com/v1"), "m", None)
            .expect_err("plaintext remote host must fail");
        assert!(error.contains("明文"), "{error}");
        assert!(
            HttpPromptAdapter::from_parts(Some("http://127.0.0.1:9/v1"), "m", None)
                .unwrap()
                .is_some()
        );
    }

    /// 一次一连接的最小对话端点形状（各测试内联自己的 listener 以便逐请求断言）。
    fn sse_ok(delta: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {}\n\ndata: [DONE]\n\n",
            serde_json::json!({ "choices": [{ "delta": { "content": delta } }] })
        )
    }

    /// 选了档位：请求体必须带 `reasoning_effort`。这是「web 端选思考强度」这条
    /// 路在 HTTP provider 上的落点——不落字段，上游永远按它自己的默认档跑。
    #[test]
    fn picked_thought_level_rides_the_chat_completions_body() {
        let bodies = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let recorder = bodies.clone();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let Ok((mut socket, _)) = listener.accept() else {
                return;
            };
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let Ok(read) = std::io::Read::read(&mut socket, &mut chunk) else {
                    return;
                };
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read]);
                let Some(header_end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") else {
                    continue;
                };
                let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
                let length = head
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length:")?
                            .trim()
                            .parse::<usize>()
                            .ok()
                    })
                    .unwrap_or(0);
                let body_end = header_end + 4 + length;
                if buffer.len() < body_end {
                    continue;
                }
                recorder
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buffer[header_end + 4..body_end]).into_owned());
                let _ = std::io::Write::write_all(&mut socket, sse_ok("done").as_bytes());
                let _ = socket.shutdown(std::net::Shutdown::Both);
                return;
            }
        });
        let adapter =
            HttpPromptAdapter::new(&format!("http://{address}/v1"), "m", None::<String>).unwrap();
        adapter
            .run_prompt(
                "hello",
                &crate::RunOptions {
                    thought_level: Some("high".into()),
                    ..Default::default()
                },
            )
            .expect("带档位的一轮应当成功");
        let seen = bodies.lock().unwrap();
        assert_eq!(seen.len(), 1, "恰好一条请求：{seen:?}");
        let body: serde_json::Value = serde_json::from_str(seen[0].as_str()).unwrap();
        assert_eq!(
            body["reasoning_effort"], "high",
            "选中的档位必须出现在请求体里：{body}"
        );
    }

    /// 没选档位：请求体里不得出现 `reasoning_effort`——空手不替用户做主张，
    /// provider 保持它自己的默认行为，和改动前完全一致。
    #[test]
    fn unpicked_thought_level_leaves_the_body_untouched() {
        let bodies = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let recorder = bodies.clone();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let Ok((mut socket, _)) = listener.accept() else {
                return;
            };
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let Ok(read) = std::io::Read::read(&mut socket, &mut chunk) else {
                    return;
                };
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read]);
                let Some(header_end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") else {
                    continue;
                };
                let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
                let length = head
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length:")?
                            .trim()
                            .parse::<usize>()
                            .ok()
                    })
                    .unwrap_or(0);
                let body_end = header_end + 4 + length;
                if buffer.len() < body_end {
                    continue;
                }
                recorder
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buffer[header_end + 4..body_end]).into_owned());
                let _ = std::io::Write::write_all(&mut socket, sse_ok("done").as_bytes());
                let _ = socket.shutdown(std::net::Shutdown::Both);
                return;
            }
        });
        let adapter =
            HttpPromptAdapter::new(&format!("http://{address}/v1"), "m", None::<String>).unwrap();
        adapter
            .run_prompt("hello", &crate::RunOptions::default())
            .expect("不带档位的一轮应当成功");
        let seen = bodies.lock().unwrap();
        assert_eq!(seen.len(), 1, "{seen:?}");
        let body: serde_json::Value = serde_json::from_str(seen[0].as_str()).unwrap();
        assert!(
            body.get("reasoning_effort").is_none(),
            "没人选档位时不得凭空带一个：{body}"
        );
    }

    /// 严格网关对 `reasoning_effort` 回 400：剥掉参数重试一次，而不是让整轮
    /// 失败。与引擎采样器 `is_reasoning_effort_error` → strip-retry 的既有口径一致。
    #[test]
    fn a_strict_gateway_400_on_reasoning_effort_gets_one_stripped_retry() {
        let bodies = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let recorder = bodies.clone();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            // 400 → 剥档位 → 重试：客户端会开第二条连接，两条都要服务。
            for attempt in 0..2usize {
                let Ok((mut socket, _)) = listener.accept() else {
                    return;
                };
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let Ok(read) = std::io::Read::read(&mut socket, &mut chunk) else {
                        return;
                    };
                    if read == 0 {
                        break;
                    }
                    buffer.extend_from_slice(&chunk[..read]);
                    let Some(header_end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") else {
                        continue;
                    };
                    let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
                    let length = head
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("content-length:")?
                                .trim()
                                .parse::<usize>()
                                .ok()
                        })
                        .unwrap_or(0);
                    let body_end = header_end + 4 + length;
                    if buffer.len() < body_end {
                        continue;
                    }
                    let body =
                        String::from_utf8_lossy(&buffer[header_end + 4..body_end]).into_owned();
                    recorder.lock().unwrap().push(body.clone());
                    let response = if attempt == 0 {
                        "HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n{\"error\":{\"message\":\"Unknown parameter: reasoning_effort is not supported with this model.\"}}".to_string()
                    } else {
                        sse_ok("retried")
                    };
                    let _ = std::io::Write::write_all(&mut socket, response.as_bytes());
                    let _ = socket.shutdown(std::net::Shutdown::Both);
                    break;
                }
            }
        });
        let adapter =
            HttpPromptAdapter::new(&format!("http://{address}/v1"), "m", None::<String>).unwrap();
        let reply = adapter
            .run_prompt(
                "hello",
                &crate::RunOptions {
                    thought_level: Some("max".into()),
                    ..Default::default()
                },
            )
            .expect("剥档位后的重试应当成功");
        assert_eq!(reply.concat(), "retried");
        let seen = bodies.lock().unwrap();
        assert_eq!(seen.len(), 2, "第一次 400，剥掉后重试恰好一次：{seen:?}");
        let first: serde_json::Value = serde_json::from_str(seen[0].as_str()).unwrap();
        assert_eq!(first["reasoning_effort"], "max");
        let second: serde_json::Value = serde_json::from_str(seen[1].as_str()).unwrap();
        assert!(
            second.get("reasoning_effort").is_none(),
            "重试不得再带 reasoning_effort：{second}"
        );
    }

    /// 拒绝理由要求显式 `reasoning_effort: "none"`（function tools 与推理不可
    /// 同开）时，不能只是剥掉参数：必须显式置为 `none` 再重试一次，否则网关
    /// 会以同样的 400 拒绝。
    #[test]
    fn a_gateway_demanding_explicit_none_gets_a_none_retry() {
        let bodies = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let recorder = bodies.clone();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for attempt in 0..2usize {
                let Ok((mut socket, _)) = listener.accept() else {
                    return;
                };
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let Ok(read) = std::io::Read::read(&mut socket, &mut chunk) else {
                        return;
                    };
                    if read == 0 {
                        break;
                    }
                    buffer.extend_from_slice(&chunk[..read]);
                    let Some(header_end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") else {
                        continue;
                    };
                    let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
                    let length = head
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("content-length:")?
                                .trim()
                                .parse::<usize>()
                                .ok()
                        })
                        .unwrap_or(0);
                    let body_end = header_end + 4 + length;
                    if buffer.len() < body_end {
                        continue;
                    }
                    let body =
                        String::from_utf8_lossy(&buffer[header_end + 4..body_end]).into_owned();
                    recorder.lock().unwrap().push(body.clone());
                    let response = if attempt == 0 {
                        "HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n{\"error\":{\"message\":\"Function tools with reasoning_effort are not supported for gpt-6-luna in /v1/chat/completions. To use function tools, use /v1/responses or set reasoning_effort to 'none'.\"}}".to_string()
                    } else {
                        sse_ok("retried")
                    };
                    let _ = std::io::Write::write_all(&mut socket, response.as_bytes());
                    let _ = socket.shutdown(std::net::Shutdown::Both);
                    break;
                }
            }
        });
        let adapter =
            HttpPromptAdapter::new(&format!("http://{address}/v1"), "m", None::<String>).unwrap();
        let reply = adapter
            .run_prompt("hello", &crate::RunOptions::default())
            .expect("显式 none 后的重试应当成功");
        assert_eq!(reply.concat(), "retried");
        let seen = bodies.lock().unwrap();
        assert_eq!(seen.len(), 2, "第一次 400，纠正后重试恰好一次：{seen:?}");
        let first: serde_json::Value = serde_json::from_str(seen[0].as_str()).unwrap();
        assert!(first.get("reasoning_effort").is_none());
        let second: serde_json::Value = serde_json::from_str(seen[1].as_str()).unwrap();
        assert_eq!(
            second["reasoning_effort"], "none",
            "重试必须显式带 none：{second}"
        );
    }

    #[test]
    fn explicit_none_correction_requires_an_instruction_not_a_rejected_value() {
        for message in [
            "To use function tools, set reasoning_effort to 'none'.",
            "To use function tools, set reasoning_effort to \"none\".",
        ] {
            // The adapter sees the raw JSON, including escaped double quotes.
            let error = format!(
                "Provider 返回 HTTP 400：{}",
                serde_json::json!({ "error": { "message": message } })
            );
            assert!(HttpPromptAdapter::error_rejects_reasoning_effort(&error));
            assert!(HttpPromptAdapter::error_wants_reasoning_effort_none(&error));
        }
        for message in [
            "Invalid value for 'reasoning_effort': 'none' is not supported with this model.",
            "Invalid value for reasoning_effort: \"none\" is not supported with this model.",
            "Unknown parameter: reasoning_effort; received value 'none'.",
            "Invalid reasoning_effort. Supported values are 'none', 'low', and 'high'.",
        ] {
            let error = format!(
                "Provider 返回 HTTP 400：{}",
                serde_json::json!({ "error": { "message": message } })
            );
            assert!(HttpPromptAdapter::error_rejects_reasoning_effort(&error));
            assert!(!HttpPromptAdapter::error_wants_reasoning_effort_none(&error));
        }
    }

    /// Rejecting the selected `none` value is the opposite of demanding it:
    /// the second HTTP request must omit effort and let the model use its default.
    #[test]
    fn a_gateway_rejecting_none_gets_one_stripped_retry() {
        use std::io::{BufRead, Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut bodies = Vec::<serde_json::Value>::new();
            for attempt in 0..2 {
                let (socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut reader = std::io::BufReader::new(socket);
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    assert!(reader.read_line(&mut line).unwrap() > 0);
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse::<usize>().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
                let response = if attempt == 0 {
                    assert_eq!(body["reasoning_effort"], "none");
                    let rejection = serde_json::json!({
                        "error": {
                            "message": "Invalid value for 'reasoning_effort': 'none' is not supported with this model."
                        }
                    });
                    format!("HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n{rejection}")
                } else {
                    assert!(body.get("reasoning_effort").is_none(), "{body}");
                    sse_ok("retried")
                };
                bodies.push(body);
                reader.get_mut().write_all(response.as_bytes()).unwrap();
            }
            bodies
        });
        let adapter =
            HttpPromptAdapter::new(&format!("http://{address}/v1"), "m", None::<String>).unwrap();
        let reply = adapter
            .run_prompt(
                "hello",
                &crate::RunOptions {
                    thought_level: Some("none".into()),
                    ..Default::default()
                },
            )
            .expect("rejecting none must strip the parameter and retry");
        assert_eq!(reply.concat(), "retried");
        let bodies = server.join().unwrap();
        assert_eq!(bodies.len(), 2, "exactly one corrected retry: {bodies:?}");
        assert_eq!(bodies[0]["reasoning_effort"], "none");
        assert!(bodies[1].get("reasoning_effort").is_none());
    }

    /// 拒绝理由没点名 `reasoning_effort` 的 400 不是这个口径的事：原样报错，
    /// 不做无依据的重试。
    #[test]
    fn an_unrelated_400_is_not_retried() {
        let bodies = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let recorder = bodies.clone();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let Ok((mut socket, _)) = listener.accept() else {
                return;
            };
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let Ok(read) = std::io::Read::read(&mut socket, &mut chunk) else {
                    return;
                };
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read]);
                let Some(header_end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") else {
                    continue;
                };
                let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
                let length = head
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length:")?
                            .trim()
                            .parse::<usize>()
                            .ok()
                    })
                    .unwrap_or(0);
                let body_end = header_end + 4 + length;
                if buffer.len() < body_end {
                    continue;
                }
                recorder
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buffer[header_end + 4..body_end]).into_owned());
                let _ = std::io::Write::write_all(
                    &mut socket,
                    b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n{\"error\":{\"message\":\"bad model name\"}}",
                );
                let _ = socket.shutdown(std::net::Shutdown::Both);
                return;
            }
        });
        let adapter =
            HttpPromptAdapter::new(&format!("http://{address}/v1"), "m", None::<String>).unwrap();
        let error = adapter
            .run_prompt(
                "hello",
                &crate::RunOptions {
                    thought_level: Some("high".into()),
                    ..Default::default()
                },
            )
            .expect_err("与档位无关的 400 必须原样失败");
        assert!(error.contains("bad model name"), "{error}");
        assert_eq!(bodies.lock().unwrap().len(), 1, "不重试");
    }
}
