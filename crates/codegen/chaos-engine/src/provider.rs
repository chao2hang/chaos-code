//! Host-configured inference provider.
//!
//! The GUI protocol deliberately never carries an API key, so the credential
//! lives on the host side only. These types read it from the process
//! environment and talk to an OpenAI-compatible endpoint. Nothing here is
//! reachable from a browser message: the base URL, model and key are fixed by
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
            chat_endpoint,
            models_endpoint,
            model,
            api_key,
        })
    }

    fn client(&self) -> Result<reqwest::blocking::Client, String> {
        // The shared TLS policy loads the OS store, the Mozilla bundle and
        // `GROK_EXTRA_CA_BUNDLE`, so a self-hosted provider behind an
        // enterprise proxy or a private CA still verifies.
        xai_grok_extra_ca::build_blocking_reqwest_client(|builder| {
            builder
                .connect_timeout(CONNECT_TIMEOUT)
                .timeout(REQUEST_TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
        })
        .map_err(|error| format!("无法初始化 HTTP 客户端: {error}"))
    }

    /// Builds an adapter from the process environment. Returns `Ok(None)` when
    /// no provider is configured, so a host can fall back to its default.
    pub fn from_env() -> Result<Option<Self>, String> {
        Self::from_parts(
            std::env::var(ENV_BASE_URL).ok().as_deref(),
            &std::env::var(ENV_MODEL).unwrap_or_default(),
            std::env::var(ENV_API_KEY).ok().as_deref(),
        )
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

    fn send_and_read(&self, body: serde_json::Value) -> Result<Vec<String>, String> {
        let client = self.client()?;
        let request = self.authorize(client.post(&self.chat_endpoint).json(&body));
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
        let mut chunks: Vec<String> = Vec::new();
        let mut provider_error: Option<String> = None;
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
            match Self::delta_of(&value) {
                Some(text) if !text.is_empty() => chunks.push(text),
                _ => {}
            }
        }
        if let Some(message) = provider_error {
            return Err(format!("Provider 报告错误：{message}"));
        }
        if chunks.is_empty() {
            return Err("Provider 未返回任何文本".to_string());
        }
        Ok(chunks)
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
    pub fn probe(&self) -> ProviderHealth {
        let request = match self.client() {
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
                        let configured_model_known = model_ids.iter().any(|id| id == &self.model);
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
}

impl PromptAdapter for HttpPromptAdapter {
    fn run_prompt(&self, prompt: &str) -> Result<Vec<String>, String> {
        let body = serde_json::json!({
            "model": self.model,
            "messages": [{ "role": "user", "content": prompt }],
            "stream": true,
        });
        self.send_and_read(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(delta: &str) -> serde_json::Value {
        serde_json::json!({ "choices": [{ "delta": { "content": delta } }] })
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
}
