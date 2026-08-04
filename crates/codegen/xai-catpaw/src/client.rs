//! Minimal async HTTP client for CatPaw's native protocols.

use std::time::Duration;

use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::{Response, StatusCode, Url, header::HeaderValue};
use serde_json::Value;

use crate::agent::{
    AgentConnectRequest, AgentContinueRequest, AgentCreateRequest, AgentCreateResponse,
};
use crate::chat::{ChatAccumulator, ChatRequest};
use crate::crypto::{decrypt_response_bytes, encrypt_request_envelope};
use crate::endpoints::ApiPaths;
use crate::headers::{API_BASE, UpstreamHeaders, default_user_agent};
use crate::models::ModelMap;
use crate::qr::{QrPoll, QrStart};
use crate::tokens::{RefreshTokenRequest, RefreshTokenWireResponse, TokenSet};
use crate::{Error, Result};

#[derive(Clone)]
pub struct Client {
    base_url: Url,
    http: reqwest::Client,
    login_http: reqwest::Client,
    agent_http: reqwest::Client,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Client")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

impl Client {
    pub fn new() -> Result<Self> {
        Self::with_base_url(API_BASE)
    }

    /// Build a client against a custom origin. This is primarily useful for
    /// deterministic tests and private reverse proxies; endpoint paths remain
    /// the native CatPaw paths.
    pub fn with_base_url(base_url: impl AsRef<str>) -> Result<Self> {
        let mut base_url = Url::parse(base_url.as_ref())
            .map_err(|error| Error::Config(format!("invalid CatPaw base URL: {error}")))?;
        if !base_url.path().ends_with('/') {
            base_url.set_path(&format!("{}/", base_url.path()));
        }
        let user_agent = default_user_agent();
        let http = reqwest::Client::builder()
            .user_agent(&user_agent)
            .timeout(Duration::from_secs(60))
            .build()?;
        let login_http = reqwest::Client::builder()
            .user_agent(&user_agent)
            .timeout(Duration::from_secs(60))
            .cookie_store(true)
            .build()?;
        // Remote Agent runs can legitimately exceed a whole-request timeout.
        let agent_http = reqwest::Client::builder().user_agent(user_agent).build()?;
        Ok(Self {
            base_url,
            http,
            login_http,
            agent_http,
        })
    }

    pub async fn start_qr_login(&self) -> Result<QrStart> {
        let response = self
            .login_http
            .get(self.endpoint(ApiPaths::LOGIN_QRCODE)?)
            .headers(UpstreamHeaders::anonymous()?)
            .send()
            .await?;
        QrStart::from_value(decode_response(response).await?)
    }

    pub async fn poll_qr_login(&self, code: &str) -> Result<QrPoll> {
        let response = self
            .login_http
            .post(self.endpoint(ApiPaths::LOGIN_ACCESS_TOKEN)?)
            .headers(UpstreamHeaders::anonymous()?)
            .json(&serde_json::json!({"code": code}))
            .send()
            .await?;
        QrPoll::from_value(decode_response(response).await?)
    }

    pub async fn refresh_token(
        &self,
        current_access_token: &str,
        refresh_token: &str,
    ) -> Result<TokenSet> {
        let response = self
            .http
            .post(self.endpoint(ApiPaths::LOGIN_REFRESH)?)
            .headers(UpstreamHeaders::minimal(current_access_token)?)
            .json(&RefreshTokenRequest::new(refresh_token))
            .send()
            .await?;
        let value = decode_response(response).await?;
        let wire: RefreshTokenWireResponse = serde_json::from_value(value)?;
        wire.into_token_set(refresh_token)
            .ok_or_else(|| Error::Auth("refresh response has no accessToken".into()))
    }

    pub async fn models(&self, headers: &UpstreamHeaders) -> Result<ModelMap> {
        let response = self
            .http
            .get(self.endpoint(ApiPaths::GPT_MODEL_LIST)?)
            .headers(headers.build()?)
            .send()
            .await?;
        let value = decode_response(response).await?;
        let mut models = ModelMap::seeded();
        models.merge_from_payload(&value);
        Ok(models)
    }

    /// Send an encrypted Chat request and return its raw plaintext SSE
    /// response. Use [`Self::collect_chat`] for non-streaming consumers.
    pub async fn chat_stream(
        &self,
        request: &ChatRequest,
        headers: &UpstreamHeaders,
    ) -> Result<Response> {
        let encrypted = encrypt_request_envelope(request)?;
        let mut request_headers = headers.build()?;
        request_headers.insert(
            "encrypted-key",
            HeaderValue::from_str(&encrypted.encrypted_key)
                .map_err(|error| Error::Config(format!("invalid encrypted key header: {error}")))?,
        );
        request_headers.insert("accept", HeaderValue::from_static("text/event-stream"));
        let response = self
            .http
            .post(self.endpoint(ApiPaths::GPT_OPENAI_STREAM)?)
            .headers(request_headers)
            .body(encrypted.body)
            .send()
            .await?;
        ensure_success(response).await
    }

    pub async fn collect_chat(response: Response) -> Result<ChatAccumulator> {
        let mut events = response.bytes_stream().eventsource();
        let mut accumulator = ChatAccumulator::new();
        while let Some(event) = events.next().await {
            let event = event.map_err(|error| Error::Upstream {
                status: 502,
                body: format!("Chat SSE decode error: {error}"),
            })?;
            for line in event.data.lines().filter(|line| !line.trim().is_empty()) {
                if line.trim() == "[DONE]" {
                    return Ok(accumulator);
                }
                let value: Value = serde_json::from_str(line)?;
                if accumulator.ingest(&value).done {
                    return Ok(accumulator);
                }
            }
        }
        Ok(accumulator)
    }

    pub async fn create_agent(
        &self,
        request: &AgentCreateRequest,
        headers: &UpstreamHeaders,
    ) -> Result<AgentCreateResponse> {
        let response = self
            .http
            .post(self.endpoint(ApiPaths::AGENT_CONVERSATION_CREATE)?)
            .headers(headers.build()?)
            .json(request)
            .send()
            .await?;
        Ok(serde_json::from_value(decode_response(response).await?)?)
    }

    pub async fn continue_agent(
        &self,
        request: &AgentContinueRequest,
        headers: &UpstreamHeaders,
    ) -> Result<Value> {
        let response = self
            .http
            .post(self.endpoint(ApiPaths::AGENT_CONVERSATION_CONTINUE)?)
            .headers(headers.build()?)
            .json(request)
            .send()
            .await?;
        decode_response(response).await
    }

    pub async fn connect_agent(
        &self,
        request: &AgentConnectRequest,
        headers: &UpstreamHeaders,
    ) -> Result<Response> {
        let response = self
            .agent_http
            .post(self.endpoint(ApiPaths::AGENT_STREAM_CONNECT)?)
            .headers(headers.build_agent_stream()?)
            .json(request)
            .send()
            .await?;
        ensure_success(response).await
    }

    fn endpoint(&self, path: &str) -> Result<Url> {
        self.base_url
            .join(path.trim_start_matches('/'))
            .map_err(|error| Error::Config(format!("invalid CatPaw endpoint {path}: {error}")))
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::new().expect("the static CatPaw client configuration is valid")
    }
}

async fn ensure_success(response: Response) -> Result<Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    Err(Error::Upstream { status, body })
}

async fn decode_response(response: Response) -> Result<Value> {
    let status = response.status();
    let encrypted_key = response
        .headers()
        .get("encrypted-key")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = response.text().await?;
    let value = match serde_json::from_str::<Value>(&body) {
        Ok(value) => value,
        Err(_) if encrypted_key.is_some() => {
            let decrypted = decrypt_response_bytes(&body, encrypted_key.as_deref().unwrap())?;
            serde_json::from_slice(&decrypted)?
        }
        Err(_) if !status.is_success() => {
            return Err(Error::Upstream {
                status: status.as_u16(),
                body,
            });
        }
        Err(error) => return Err(Error::Json(error)),
    };
    normalize_response(value, encrypted_key.as_deref(), status)
}

fn normalize_response(
    value: Value,
    encrypted_key: Option<&str>,
    status: StatusCode,
) -> Result<Value> {
    if let (Some(ciphertext), Some(encrypted_key)) = (value.as_str(), encrypted_key) {
        let decrypted = decrypt_response_bytes(ciphertext, encrypted_key)?;
        return normalize_response(serde_json::from_slice(&decrypted)?, None, status);
    }
    if let Some(code) = value.get("code").and_then(Value::as_i64) {
        if code != 0 && code != 200 {
            let message = value.get("msg").and_then(Value::as_str).unwrap_or_default();
            return Err(Error::Auth(format!("upstream code={code}: {message}")));
        }
        if let Some(data) = value.get("data") {
            if let (Some(ciphertext), Some(encrypted_key)) = (data.as_str(), encrypted_key) {
                let decrypted = decrypt_response_bytes(ciphertext, encrypted_key)?;
                return normalize_response(serde_json::from_slice(&decrypted)?, None, status);
            }
            return Ok(data.clone());
        }
    }
    if let Some(passport_status) = value.get("status").and_then(Value::as_i64) {
        if passport_status != 200 {
            let message = value
                .pointer("/data/message")
                .or_else(|| value.get("message"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            return Err(Error::Auth(format!(
                "upstream status={passport_status}: {message}"
            )));
        }
        return Ok(value.get("data").cloned().unwrap_or(value));
    }
    if !status.is_success() {
        return Err(Error::Upstream {
            status: status.as_u16(),
            body: value.to_string(),
        });
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_base_url_keeps_native_paths() {
        let client = Client::with_base_url("http://127.0.0.1:1234/root").unwrap();
        assert_eq!(
            client.endpoint(ApiPaths::LOGIN_QRCODE).unwrap().as_str(),
            "http://127.0.0.1:1234/root/api/login/qrcode"
        );
    }

    #[test]
    fn response_envelopes_are_normalized() {
        assert_eq!(
            normalize_response(
                serde_json::json!({"code": 0, "data": {"value": 1}}),
                None,
                StatusCode::OK,
            )
            .unwrap(),
            serde_json::json!({"value": 1})
        );
        assert!(
            normalize_response(
                serde_json::json!({"code": 401, "msg": "expired"}),
                None,
                StatusCode::OK,
            )
            .is_err()
        );
    }
}
