//! Tolerant parser for upstream error bodies.

use serde_json::Value;

use crate::error::MAX_USER_ERROR_BODY_CHARS;

/// High-level kind of a provider error, derived from the parsed slug and
/// message text. Drives retry decisions: only transient kinds (rate-limit /
/// server / transport) are worth retrying at the call site; `Auth`/`Billing`/
/// `Context` are deterministic failures.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderErrorKind {
    /// Credential missing / wrong / expired (401/403 / `invalid_api_key` /
    /// `authentication_error` / `token expired` …).
    Auth,
    /// Quota / balance exhausted (402 / `insufficient_balance` /
    /// `billing_error` / Zhipu `4013` …). Retrying cannot help.
    Billing,
    /// Rate limited (429 / `rate_limit_error` / `throttling` …). Retry after
    /// `Retry-After`.
    RateLimit,
    /// Provider overloaded / 5xx / capacity. Transient, bounded retry.
    Server,
    /// Context-window / size overflow. Deterministic.
    Context,
    /// Transport-level or ambiguous transient (network, connection reset,
    /// empty-but-legit stream). Bounded retry.
    Transient,
    /// No signal to classify.
    #[default]
    Unknown,
}

impl ProviderErrorKind {
    /// Whether retrying this error is worth it at all. `Auth`, `Billing`
    /// and `Context` are permanent — do not burn retry budget on them.
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            ProviderErrorKind::RateLimit | ProviderErrorKind::Server | ProviderErrorKind::Transient
        )
    }

    /// ASCII, stable identifier used in machine-facing messages and tests.
    pub fn as_str(self) -> &'static str {
        match self {
            ProviderErrorKind::Auth => "auth",
            ProviderErrorKind::Billing => "billing",
            ProviderErrorKind::RateLimit => "rate_limit",
            ProviderErrorKind::Server => "server",
            ProviderErrorKind::Context => "context",
            ProviderErrorKind::Transient => "transient",
            ProviderErrorKind::Unknown => "unknown",
        }
    }
}

/// Everything salvageable from an upstream error body.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ProviderError {
    /// Human-readable reason. Never empty.
    pub message: String,
    pub kind: Option<String>,
    pub code: Option<String>,
    pub param: Option<String>,
    pub wke: Option<String>,
}

impl ProviderError {
    fn from_message(message: impl Into<String>) -> Option<Self> {
        let (message, wke) = split_wke(message.into());
        let message = message.trim().to_owned();
        if message.is_empty() {
            return None;
        }
        Some(Self {
            message,
            wke,
            ..Default::default()
        })
    }

    /// The best available machine-readable tag, preferring the most specific.
    pub fn slug(&self) -> Option<&str> {
        [
            self.wke.as_deref(),
            self.code.as_deref(),
            self.kind.as_deref(),
        ]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|s| is_slug_shaped(s))
    }

    /// Whether the message is markup rather than prose.
    pub fn message_is_markup(&self) -> bool {
        let m = self.message.trim();
        let lower = m.to_ascii_lowercase();
        m.starts_with('<')
            || lower.contains("<html")
            || lower.contains("<!doctype")
            || lower.contains("</")
    }

    /// One-line display text.
    pub fn display_message(&self) -> String {
        match self.slug() {
            Some(slug)
                if !message_already_says(&self.message, slug)
                    && !slug.chars().all(|c| c.is_ascii_digit()) =>
            {
                truncate_provider_message(&format!("{slug}: {}", self.message))
            }
            _ => truncate_provider_message(&self.message),
        }
    }

    /// Classify this error into a retry-relevant [`ProviderErrorKind`].
    ///
    /// Order matters: the most specific signal (slug / code / kind tags
    /// first, then message keywords) wins. `Code`-only integer statuses are
    /// not trusted here — real 401/429/5xx already surface as
    /// `SamplingError::Api { status }`, so a bare `code: 429` in the body is
    /// ambiguous and only classified by the message text.
    pub fn classify(&self) -> ProviderErrorKind {
        let msg = self.message.to_ascii_lowercase();
        // Combined tag signal from code + kind + slug. We lower-case and also
        // strip common separators so `InvalidParameter` == `invalid_parameter`
        // == `invalidparameter` for matching, and so generic but informative
        // tags (e.g. `Throttling`) are classifiable even when they don't pass
        // `is_slug_shaped` for display.
        let mut tags: Vec<String> = [self.code.as_deref(), self.kind.as_deref()]
            .into_iter()
            .flatten()
            .filter_map(normalize_tag)
            .collect();
        if let Some(n) = self.slug().and_then(normalize_tag) {
            tags.push(n);
        }
        let joined = tags.join(" ");
        let codes_only_integer = tags
            .iter()
            .all(|t| t.is_empty() || t.chars().all(|c| c.is_ascii_digit()));

        // --- code/kind-based claims (machine signals, most reliable) ---
        if !codes_only_integer && !joined.is_empty() {
            // Auth: api_key / auth / unauthorized / forbidden / access_denied.
            if joined.contains("api_key")
                || joined.contains("token_expired")
                || joined.contains("authentication")
                || joined.contains("unauthorized")
                || joined.contains("forbidden")
                || joined.contains("access_denied")
                || joined.contains("denied")
            {
                return ProviderErrorKind::Auth;
            }
            // Balance / quota. We check the raw tag set (not `joined`) so a
            // numeric code like `4013` still matches even when accompanied by
            // other tags (e.g. Zhipu pairs `err_code: 4013` with
            // `type: insufficient_balance_error` → joined would be
            // "4013 insufficientbalanceerror" and `== "4013"` would miss).
            if joined.contains("balance")
                || joined.contains("insufficient")
                || joined.contains("billing")
                || joined.contains("quota")
                || joined == "no_quota"
                || tags.iter().any(|t| t.as_str() == "402")
                || tags.iter().any(|t| t.as_str() == "4013")
            // Zhipu balance code
            {
                return ProviderErrorKind::Billing;
            }
            // Rate-limit.
            if joined.contains("throttl")
                || joined.contains("rate_limit")
                || joined.contains("many_requests")
                || tags.iter().any(|t| t.as_str() == "429")
                || tags.iter().any(|t| t.as_str() == "4022")
            // Zhipu throttling code
            {
                return ProviderErrorKind::RateLimit;
            }
            // Context / length.
            if joined.contains("context")
                || joined.contains("length")
                || joined.contains("token_limit")
            {
                return ProviderErrorKind::Context;
            }
            // Capacity / server.
            if joined.contains("overload")
                || joined.contains("service_unavailable")
                || joined.contains("capacity")
                || joined.contains("timeout")
                || tags.iter().any(|t| t.as_str() == "529")
            {
                return ProviderErrorKind::Server;
            }
        }

        // --- message-text keywords (heuristic, when tags gave no signal) ---
        if msg.contains("insufficient balance")
            || msg.contains("credit balance")
            || msg.contains("no quota")
            || msg.contains("out of credits")
            || msg.contains("余额不足")
            || msg.contains("charge")
        {
            ProviderErrorKind::Billing
        } else if msg.contains("rate limit")
            || msg.contains("too many requests")
            || msg.contains("throttl")
            || msg.contains("flow control")
            || msg.contains("频率")
            || msg.contains("过于频繁")
            || msg.contains("请求过多")
        {
            ProviderErrorKind::RateLimit
        } else if msg.contains("authentication")
            || msg.contains("invalid api key")
            || msg.contains("wrong api key")
            || msg.contains("no such user")
            || msg.contains("token expired")
            || msg.contains("invalid token")
            || msg.contains("unauthorized")
            || msg.contains("not authorized")
            || msg.contains("access denied")
            || msg.contains("没有权限")
            || msg.contains("密钥")
            || msg.contains("鉴权")
        {
            ProviderErrorKind::Auth
        } else if msg.contains("context length")
            || msg.contains("context is too long")
            || msg.contains("maximum context")
            || (msg.contains("exceed") && msg.contains("context"))
            || msg.contains("上下文")
            || msg.contains("超长")
        {
            ProviderErrorKind::Context
        } else if msg.contains("overloaded")
            || msg.contains("service unavailable")
            || msg.contains("server is busy")
            || msg.contains("暂时无法处理")
            || msg.contains("服务不可用")
            || msg.contains("过大")
        {
            ProviderErrorKind::Server
        } else {
            ProviderErrorKind::Transient
        }
    }
}

/// Normalize a code/kind/slug tag to a lowercase, separator-stripped form so
/// `InvalidParameter` / `invalid_parameter` / `invalidparameter` all match.
fn normalize_tag(tag: &str) -> Option<String> {
    let mut s = tag.trim().to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }
    // Strip underscores / dashes to a flat token (keep multi-part joined).
    s = s.replace(['_', '-', ' '], "");
    Some(s)
}

fn is_slug_shaped(s: &str) -> bool {
    if s.is_empty() || s.contains(char::is_whitespace) {
        return false;
    }
    !matches!(
        s.to_ascii_lowercase().as_str(),
        "unknown"
            | "unknown_error"
            | "error"
            | "errors"
            | "server_error"
            | "api_error"
            | "internal"
            | "internal_error"
            | "none"
            | "null"
            // Domestic providers (DeepSeek / Zhipu / Qwen / Volcengine) reuse a
            // small set of generic type tags that add no signal for a slug —
            // they are classified by text/keywords below instead.
            | "authentication_error"
            | "rate_limit_error"
            | "insufficient_balance_error"
            | "insufficient_quota"
            | "invalid_parameter"
            | "invalidparameter"     // Qwen uses CamelCase without underscore
            | "invalid_argument"
            | "inputvalidation"
            | "throttling"
            | "access_denied"
    )
}

fn message_already_says(message: &str, slug: &str) -> bool {
    message
        .to_ascii_lowercase()
        .contains(&slug.to_ascii_lowercase())
}

fn split_wke(message: String) -> (String, Option<String>) {
    const PREFIX: &str = "[WKE=";
    let Some(start) = message.find(PREFIX) else {
        return (message, None);
    };
    let rest = &message[start + PREFIX.len()..];
    let Some(end) = rest.find(']') else {
        return (message, None);
    };
    let code = rest[..end].trim().to_owned();
    if code.is_empty() {
        return (message, None);
    }
    let cleaned = format!("{}{}", &message[..start], &rest[end + 1..]);
    let cleaned = cleaned.trim().trim_end_matches('.').trim().to_owned();
    let cleaned = if cleaned.is_empty() {
        code.clone()
    } else {
        cleaned
    };
    (cleaned, Some(code))
}

fn stringify_code(value: Option<&Value>) -> Option<String> {
    let s = match value? {
        Value::String(s) => s.trim().to_owned(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => return None,
    };
    (!s.is_empty() && s != "null").then_some(s)
}

fn nonempty_str(value: Option<&Value>) -> Option<String> {
    let s = value?.as_str()?.trim();
    (!s.is_empty()).then(|| s.to_owned())
}

fn first_str<'a>(obj: &Value, keys: impl IntoIterator<Item = &'a str>) -> Option<String> {
    keys.into_iter().find_map(|k| nonempty_str(obj.get(k)))
}

/// Parse an upstream error body into a [`ProviderError`]. Returns `None` when
/// the body carries nothing a human could read.
pub fn parse_provider_error(bytes: &[u8]) -> Option<ProviderError> {
    let text = std::str::from_utf8(bytes).ok()?.trim();
    parse_provider_error_str(text)
}

/// [`parse_provider_error`] over a string slice.
pub fn parse_provider_error_str(text: &str) -> Option<ProviderError> {
    let text = text.trim();
    if text.is_empty() || text.starts_with('<') {
        return None;
    }
    let value: Value = serde_json::from_str(text).ok()?;
    let parsed = walk(&value)?;
    Some(unwrap_double_encoding(parsed))
}

fn unwrap_double_encoding(outer: ProviderError) -> ProviderError {
    let Some(inner) = parse_provider_error_str(&outer.message) else {
        return outer;
    };
    if inner.message == outer.message {
        return outer;
    }
    ProviderError {
        message: inner.message,
        kind: inner.kind.or(outer.kind),
        code: inner.code.or(outer.code),
        param: inner.param.or(outer.param),
        wke: inner.wke.or(outer.wke),
    }
}

fn walk(value: &Value) -> Option<ProviderError> {
    match value {
        Value::String(s) => ProviderError::from_message(s.clone()),
        Value::Array(items) => items.iter().find_map(walk),
        Value::Object(_) => walk_object(value),
        _ => None,
    }
}

fn walk_object(obj: &Value) -> Option<ProviderError> {
    match obj.get("error") {
        Some(inner @ Value::Object(_)) => {
            let message = first_str(inner, ["message", "error", "detail", "description"])
                .or_else(|| nonempty_str(inner.get("type")))?;
            let (message, wke) = split_wke(message);
            Some(ProviderError {
                message,
                // `Type` (capitalized) is a Volcengine quirk.
                kind: nonempty_str(inner.get("type"))
                    .or_else(|| nonempty_str(inner.get("Type")))
                    .or_else(|| nonempty_str(obj.get("type"))),
                code: stringify_code(inner.get("code"))
                    .or_else(|| stringify_code(inner.get("error_code")))
                    // Zhipu uses `err_code` (can be a negative int like -1).
                    .or_else(|| stringify_code(inner.get("err_code")))
                    .or_else(|| stringify_code(obj.get("code"))),
                param: nonempty_str(inner.get("param"))
                    .or_else(|| nonempty_str(inner.get("field"))),
                wke,
            })
        }

        Some(Value::String(s)) => {
            let (message, wke) = split_wke(s.clone());
            let message = message.trim().to_owned();
            if message.is_empty() {
                return None;
            }
            Some(ProviderError {
                message,
                kind: nonempty_str(obj.get("type")),
                code: stringify_code(obj.get("code"))
                    .or_else(|| stringify_code(obj.get("error_code")))
                    .or_else(|| stringify_code(obj.get("err_code"))),
                param: nonempty_str(obj.get("param")),
                wke,
            })
        }

        Some(other @ Value::Array(_)) => walk(other),

        _ => {
            let message = first_str(obj, ["message", "detail", "msg", "description"])?;
            let (message, wke) = split_wke(message);
            Some(ProviderError {
                message,
                kind: nonempty_str(obj.get("type")),
                code: stringify_code(obj.get("code"))
                    .or_else(|| stringify_code(obj.get("error_code")))
                    .or_else(|| stringify_code(obj.get("err_code"))),
                param: nonempty_str(obj.get("param")),
                wke,
            })
        }
    }
}

fn truncate_provider_message(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() <= MAX_USER_ERROR_BODY_CHARS {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(MAX_USER_ERROR_BODY_CHARS).collect();
    out.push('\u{2026}');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CORPUS: &[(&str, &str, &str, Option<&str>)] = &[
        (
            "openai_chat",
            r#"{"error":{"message":"Invalid value for 'temperature'","type":"invalid_request_error","param":"temperature","code":null}}"#,
            "Invalid value for 'temperature'",
            Some("invalid_request_error"),
        ),
        (
            "openai_string_code",
            r#"{"error":{"message":"Incorrect API key provided","type":"invalid_request_error","code":"invalid_api_key"}}"#,
            "Incorrect API key provided",
            Some("invalid_api_key"),
        ),
        (
            "type_tagged",
            r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
            "Overloaded",
            Some("overloaded_error"),
        ),
        (
            "type_tagged_billing",
            r#"{"type":"error","error":{"type":"billing_error","message":"Your credit balance is too low"}}"#,
            "Your credit balance is too low",
            Some("billing_error"),
        ),
        (
            "openrouter_integer_code",
            r#"{"error":{"message":"Provider returned error","code":429}}"#,
            "Provider returned error",
            Some("429"),
        ),
        (
            "google_legacy_integer_code",
            r#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT"}}"#,
            "API key not valid. Please pass a valid API key.",
            Some("400"),
        ),
        (
            "azure_stringified_number_code",
            r#"{"error":{"code":"429","message":"Requests to the ChatCompletions Operation have exceeded the rate limit"}}"#,
            "Requests to the ChatCompletions Operation have exceeded the rate limit",
            Some("429"),
        ),
        (
            "vertex_array_wrapped",
            r#"[{"error":{"code":429,"message":"Quota exceeded for aiplatform.googleapis.com","status":"RESOURCE_EXHAUSTED"}}]"#,
            "Quota exceeded for aiplatform.googleapis.com",
            Some("429"),
        ),
        (
            "bedrock_capitalized_message",
            r#"{"Message":"Too many requests"}"#,
            "",
            None,
        ),
        (
            "bedrock_lowercase_message",
            r#"{"message":"The model is not ready for inference"}"#,
            "The model is not ready for inference",
            None,
        ),
        (
            "xai_flat_sentence_code",
            r#"{"code":"The service is currently unavailable","error":"Service temporarily unavailable."}"#,
            "Service temporarily unavailable.",
            None,
        ),
        (
            "json_error_numeric_code",
            r#"{"code":500,"error":"internal server error"}"#,
            "internal server error",
            Some("500"),
        ),
        (
            "bare_string_body",
            r#""A request may either be streaming or deferred, but not both.""#,
            "A request may either be streaming or deferred, but not both.",
            None,
        ),
        (
            "responses_sse_frame",
            r#"{"type":"error","message":"The model produced an invalid tool call","code":null,"param":null}"#,
            "The model produced an invalid tool call",
            None,
        ),
        (
            "wke_marker",
            r#"{"code":429,"error":"You ran out of credits. [WKE=personal-team-blocked:spending-limit]"}"#,
            "You ran out of credits",
            Some("personal-team-blocked:spending-limit"),
        ),
        (
            "detail_only",
            r#"{"detail":"Not authenticated"}"#,
            "Not authenticated",
            None,
        ),
        (
            "axum_deserialize_failure",
            r#"{"error":"Failed to deserialize the JSON body into the target type: messages[3].content: invalid type: null"}"#,
            "Failed to deserialize the JSON body into the target type: messages[3].content: invalid type: null",
            None,
        ),
        // ---- Domestic providers (DeepSeek / Qwen / Zhipu / Moonshot / Volcengine) ----
        (
            "deepseek_401_invalid_key",
            r#"{"error":{"message":"Authentication Fails (no such user)","type":"authentication_error"}}"#,
            "Authentication Fails (no such user)",
            None,
        ),
        (
            "deepseek_402_insufficient_balance",
            r#"{"error":{"message":"Insufficient Balance","type":"insufficient_balance_error"}}"#,
            "Insufficient Balance",
            None,
        ),
        (
            "deepseek_429_rate_limit",
            r#"{"error":{"message":"Rate limit reached for requests","type":"rate_limit_error","code":"rate_limit_exceeded"}}"#,
            "Rate limit reached for requests",
            Some("rate_limit_exceeded"),
        ),
        (
            "deepseek_400_context_too_long",
            r#"{"error":{"message":"Context length exceeded: 131072 > 65536","type":"invalid_request_error","param":"messages"}}"#,
            "Context length exceeded: 131072 > 65536",
            Some("invalid_request_error"),
        ),
        (
            "deepseek_504_overloaded",
            r#"{"error":{"message":"The model is overloaded, please try again later","type":"overloaded_error"}}"#,
            "The model is overloaded, please try again later",
            Some("overloaded_error"),
        ),
        (
            "qwen_400_invalid_parameter_flat",
            r#"{"code":"InvalidParameter","message":"The qwen-max model does not exist, please check your invoice.","request_id":"000000000000000000000000"}"#,
            "The qwen-max model does not exist, please check your invoice.",
            None,
        ),
        (
            "qwen_429_throttling_flat",
            r#"{"code":"Throttling","message":"Flow control triggered, please slow down","request_id":"aaa-bbb"}"#,
            "Flow control triggered, please slow down",
            None,
        ),
        (
            "zhipu_401_token_expired_errcode",
            r#"{"error":{"err_code":4011,"message":"token expired or invalid","type":"authentication_error"}}"#,
            "token expired or invalid",
            Some("4011"),
        ),
        (
            "zhipu_429_throttling_errcode",
            r#"{"error":{"err_code":4022,"message":"当前模型承载的并发较高，为提升您服务的稳定性，当前触发限流，请您稍后重试","type":"throttling"}}"#,
            "当前模型承载的并发较高，为提升您服务的稳定性，当前触发限流，请您稍后重试",
            Some("4022"),
        ),
        (
            "zhipu_402_balance_errcode",
            r#"{"error":{"err_code":4013,"message":"余额不足","type":"insufficient_balance_error"}}"#,
            "余额不足",
            Some("4013"),
        ),
        (
            "zhipu_internal_errcode_negative",
            r#"{"error":{"err_code":-1,"message":"内部服务异常","type":"internal_error"}}"#,
            "内部服务异常",
            Some("-1"),
        ),
        (
            "moonshot_401_invalid_api_key",
            r#"{"error":{"type":"invalid_request_error","message":"Invalid API key","param":null}}"#,
            "Invalid API key",
            Some("invalid_request_error"),
        ),
        (
            "volcengine_400_access_denied_capital_type",
            r#"{"error":{"Type":"AccessDenied","message":"You are not authorized to perform the operation."}}"#,
            "You are not authorized to perform the operation.",
            Some("AccessDenied"),
        ),
    ];

    #[test]
    fn corpus_parses_every_known_shape() {
        for (name, body, expected_message, expected_slug) in CORPUS {
            let parsed = parse_provider_error(body.as_bytes());
            if expected_message.is_empty() {
                assert!(
                    parsed.is_none(),
                    "{name}: expected no parse, got {parsed:?}"
                );
                continue;
            }
            let parsed = parsed.unwrap_or_else(|| panic!("{name}: failed to parse {body}"));
            assert_eq!(&parsed.message, expected_message, "{name}: message");
            assert_eq!(parsed.slug(), *expected_slug, "{name}: slug");
        }
    }

    #[test]
    fn double_encoded_body_is_unwrapped() {
        let body = r#"{"error":"{\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"max_tokens: 200000 > 64000, which is the maximum allowed\"}}"}"#;
        let parsed = parse_provider_error(body.as_bytes()).expect("parses");
        assert_eq!(
            parsed.message,
            "max_tokens: 200000 > 64000, which is the maximum allowed"
        );
        assert_eq!(parsed.slug(), Some("invalid_request_error"));
    }

    #[test]
    fn double_encoded_html_is_not_surfaced() {
        let body = serde_json::json!({
            "error": "<html><body>502 Bad Gateway</body></html>",
        })
        .to_string();
        let parsed = parse_provider_error(body.as_bytes()).expect("parses outer");
        assert!(parsed.slug().is_none());
        assert!(parsed.message_is_markup());
    }

    #[test]
    fn html_and_empty_bodies_are_rejected() {
        assert!(parse_provider_error(b"").is_none());
        assert!(parse_provider_error(b"   ").is_none());
        assert!(parse_provider_error(b"<!DOCTYPE html><html></html>").is_none());
        assert!(parse_provider_error(b"not json at all").is_none());
    }

    #[test]
    fn successful_payloads_are_not_mistaken_for_errors() {
        let chunk = r#"{"id":"abc","object":"chat.completion.chunk","created":0,"model":"grok","choices":[]}"#;
        assert!(parse_provider_error(chunk.as_bytes()).is_none());
    }

    #[test]
    fn sentence_shaped_codes_never_become_prefixes() {
        let parsed = parse_provider_error(
            br#"{"code":"Client specified an invalid argument","error":"model 'nope' does not exist"}"#,
        )
        .expect("parses");
        assert_eq!(
            parsed.code.as_deref(),
            Some("Client specified an invalid argument")
        );
        assert_eq!(parsed.slug(), None);
        assert_eq!(parsed.display_message(), "model 'nope' does not exist");
    }

    #[test]
    fn content_free_type_tags_are_not_prefixes() {
        for tag in ["unknown", "server_error", "error", "api_error", "internal"] {
            let body = format!(r#"{{"error":{{"message":"boom","type":"{tag}"}}}}"#);
            let parsed = parse_provider_error(body.as_bytes()).expect("parses");
            assert_eq!(parsed.display_message(), "boom", "tag {tag}");
        }
    }

    #[test]
    fn slug_is_not_repeated_when_the_message_already_says_it() {
        let parsed = parse_provider_error(
            br#"{"error":{"message":"rate_limit_error: slow down","type":"rate_limit_error"}}"#,
        )
        .expect("parses");
        assert_eq!(parsed.display_message(), "rate_limit_error: slow down");
    }

    #[test]
    fn display_message_is_length_capped_on_a_char_boundary() {
        let long = "é".repeat(MAX_USER_ERROR_BODY_CHARS + 50);
        let body = serde_json::json!({ "error": { "message": long } }).to_string();
        let parsed = parse_provider_error(body.as_bytes()).expect("parses");
        let shown = parsed.display_message();
        assert_eq!(shown.chars().count(), MAX_USER_ERROR_BODY_CHARS + 1);
        assert!(shown.ends_with('\u{2026}'));
    }

    #[test]
    fn wke_marker_is_lifted_out_of_the_message() {
        let (msg, wke) = split_wke("User exceeds storage [WKE=file:storage-exhausted]".into());
        assert_eq!(msg, "User exceeds storage");
        assert_eq!(wke.as_deref(), Some("file:storage-exhausted"));

        let (msg, wke) = split_wke("err [WKE=file:too-large".into());
        assert_eq!(msg, "err [WKE=file:too-large");
        assert_eq!(wke, None);

        let (msg, wke) = split_wke("[WKE=foo:bar]".into());
        assert_eq!(msg, "foo:bar");
        assert_eq!(wke.as_deref(), Some("foo:bar"));
    }

    #[test]
    fn classify_domestic_provider_bodies_maps_to_retry_kind() {
        use ProviderErrorKind::*;
        // (body, expected kind, is_retryable)
        let cases: &[(&str, ProviderErrorKind, bool)] = &[
            (
                r#"{"error":{"message":"Authentication Fails (no such user)","type":"authentication_error"}}"#,
                Auth,
                false,
            ),
            (
                r#"{"error":{"message":"Insufficient Balance","type":"insufficient_balance_error"}}"#,
                Billing,
                false,
            ),
            (
                r#"{"error":{"message":"Rate limit reached","type":"rate_limit_error","code":"rate_limit_exceeded"}}"#,
                RateLimit,
                true,
            ),
            (
                r#"{"error":{"message":"Context length exceeded: 131072 > 65536","type":"invalid_request_error"}}"#,
                Context,
                false,
            ),
            (
                r#"{"error":{"message":"The model is overloaded, please try again later","type":"overloaded_error"}}"#,
                Server,
                true,
            ),
            (
                r#"{"code":"Throttling","message":"Flow control triggered, please slow down","request_id":"x"}"#,
                RateLimit,
                true,
            ),
            (
                r#"{"error":{"err_code":4011,"message":"token expired or invalid","type":"authentication_error"}}"#,
                Auth,
                false,
            ),
            (
                r#"{"error":{"err_code":4013,"message":"余额不足","type":"insufficient_balance_error"}}"#,
                Billing,
                false,
            ),
            (
                r#"{"error":{"err_code":4022,"message":"当前模型承载的并发较高，请您稍后重试","type":"throttling"}}"#,
                RateLimit,
                true,
            ),
            (
                r#"{"error":{"err_code":-1,"message":"内部服务异常","type":"internal_error"}}"#,
                Transient,
                true,
            ),
            (
                r#"{"error":{"Type":"AccessDenied","message":"You are not authorized to perform the operation."}}"#,
                Auth,
                false,
            ),
            // Regression: Zhipu `err_code: 4013` paired with a `type` that
            // carries no balance keyword and a message without billing text.
            // Before the `tags.iter().any(...)` fix, `joined == "4013"` missed
            // because joined was "4013 invalidrequesterror" and the case fell
            // through to Transient.
            (
                r#"{"error":{"err_code":4013,"message":"操作失败","type":"invalid_request_error"}}"#,
                Billing,
                false,
            ),
            // Regression: Zhipu `err_code: 4022` without a throttling keyword in
            // type or message. Same `joined == "4022"` gap as above.
            (
                r#"{"error":{"err_code":4022,"message":"服务繁忙","type":"internal_error"}}"#,
                RateLimit,
                true,
            ),
            // Regression: `exceed` + `context` co-occurring without the literal
            // "context length" / "context is too long" substrings. Before the
            // fix, `msg.contains("exceed.*context")` matched nothing (str::contains
            // is literal, not regex) and this fell through to Transient.
            (
                r#"{"error":{"message":"token count exceed context window","type":"invalid_request_error"}}"#,
                Context,
                false,
            ),
        ];
        for (body, expected_kind, expected_retry) in cases {
            let parsed = parse_provider_error(body.as_bytes())
                .unwrap_or_else(|| panic!("failed to parse classify fixture: {body}"));
            assert_eq!(
                parsed.classify(),
                *expected_kind,
                "classify mismatch for {body}"
            );
            assert_eq!(
                parsed.classify().is_retryable(),
                *expected_retry,
                "is_retryable mismatch for {body}"
            );
        }
    }

    #[test]
    fn err_code_negative_and_string_codes_are_parsed() {
        let parsed = parse_provider_error(
            r#"{"error":{"err_code":-1,"message":"内部服务异常"}}"#.as_bytes(),
        )
        .expect("parses");
        assert_eq!(parsed.code.as_deref(), Some("-1"));

        let parsed =
            parse_provider_error(r#"{"error":{"err_code":4022,"message":"限流"}}"#.as_bytes())
                .expect("parses");
        assert_eq!(parsed.code.as_deref(), Some("4022"));
    }

    #[test]
    fn classifier_never_overrides_a_known_retry_status_with_context() {
        // A rate-limit body that also mentions "too long" must not be
        // misclassified as Context (slug/status signal wins).
        let parsed = parse_provider_error(
            r#"{"error":{"message":"rate_limit_error: too many requests (see docs)","type":"rate_limit_error"}}"#.as_bytes(),
        )
        .expect("parses");
        assert_eq!(parsed.classify(), ProviderErrorKind::RateLimit);
    }
}
