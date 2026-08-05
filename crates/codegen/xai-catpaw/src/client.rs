//! Minimal async HTTP client for CatPaw's native protocols.

use std::time::Duration;

use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::{Response, StatusCode, Url, header::HeaderValue};
use rqrr::BitGrid;
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

const MAX_QR_IMAGE_BYTES: usize = 2 * 1024 * 1024;

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
        let mut qr = QrStart::from_value(decode_response(response).await?)?;
        // The upstream returns a WeChat `showqrcode` image URL. Download and
        // decode the image here so terminal clients can render the real QR
        // modules; encoding the URL or poll code would produce a different QR.
        if !qr.qr_code_image_url.is_empty() {
            match self.download_qr_modules(&qr.qr_code_image_url).await {
                Ok(modules) => qr.qr_modules = modules,
                Err(error) => {
                    tracing::warn!(%error, "CatPaw QR image unavailable; showing URL fallback")
                }
            }
        }
        Ok(qr)
    }

    async fn download_qr_modules(&self, image_url: &str) -> Result<Vec<Vec<bool>>> {
        let image_url = Url::parse(image_url)
            .map_err(|error| Error::Auth(format!("invalid CatPaw QR image URL: {error}")))?;
        if image_url.scheme() != "https" || image_url.host_str() != Some("mp.weixin.qq.com") {
            return Err(Error::Auth(
                "CatPaw QR image URL must use the official WeChat HTTPS host".into(),
            ));
        }
        let response = self.login_http.get(image_url).send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(Error::Upstream {
                status: status.as_u16(),
                body: format!("QR image download failed with HTTP {status}"),
            });
        }
        let final_url = response.url();
        if final_url.scheme() != "https" || final_url.host_str() != Some("mp.weixin.qq.com") {
            return Err(Error::Auth(
                "CatPaw QR image redirected away from the official WeChat HTTPS host".into(),
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_QR_IMAGE_BYTES as u64)
        {
            return Err(Error::Auth("CatPaw QR image is unexpectedly large".into()));
        }
        let mut bytes = Vec::new();
        let mut response = response;
        while let Some(chunk) = response.chunk().await? {
            if bytes.len().saturating_add(chunk.len()) > MAX_QR_IMAGE_BYTES {
                return Err(Error::Auth("CatPaw QR image is unexpectedly large".into()));
            }
            bytes.extend_from_slice(&chunk);
        }
        qr_modules_from_image(&bytes)
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

    /// Query the current CatPaw account's per-model quota snapshot.
    pub async fn quota(&self, headers: &UpstreamHeaders) -> Result<crate::quota::QuotaInfo> {
        let response = self
            .http
            .get(self.endpoint(ApiPaths::CHAT_MODEL_USAGE)?)
            .headers(headers.build()?)
            .send()
            .await?;
        Ok(crate::quota::QuotaInfo::from_value(
            decode_response(response).await?,
        ))
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

fn qr_modules_from_image(bytes: &[u8]) -> Result<Vec<Vec<bool>>> {
    let image = image::load_from_memory(bytes)
        .map_err(|error| Error::Auth(format!("CatPaw QR image decode failed: {error}")))?
        .to_luma8();
    let mut prepared = rqrr::PreparedImage::prepare(image);
    let grid = prepared
        .detect_grids()
        .into_iter()
        .next()
        .ok_or_else(|| Error::Auth("CatPaw QR image contains no detectable code".into()))?;
    let size = grid.grid.size();
    Ok((0..size)
        .map(|y| (0..size).map(|x| grid.grid.bit(y, x)).collect())
        .collect())
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
    fn qr_image_is_recovered_as_a_decodable_module_grid() {
        use image::{DynamicImage, ImageFormat, Luma};
        use qrcode::QrCode;
        use std::io::Cursor;

        let payload = "https://weixin.qq.com/x/catpaw-terminal-test";
        let image = QrCode::new(payload.as_bytes())
            .unwrap()
            .render::<Luma<u8>>()
            .min_dimensions(430, 430)
            .build();
        let mut encoded = Cursor::new(Vec::new());
        DynamicImage::ImageLuma8(image)
            .write_to(&mut encoded, ImageFormat::Jpeg)
            .unwrap();

        let modules = qr_modules_from_image(encoded.get_ref()).unwrap();
        let size = modules.len();
        assert!(size >= 21 && modules.iter().all(|row| row.len() == size));

        let grid = rqrr::SimpleGrid::from_func(size, |x, y| modules[y][x]);
        let (_, decoded) = rqrr::Grid::new(grid).decode().unwrap();
        assert_eq!(decoded, payload);
    }

    #[tokio::test]
    #[ignore = "live CatPaw/WeChat smoke test"]
    async fn live_qr_image_yields_a_terminal_sized_grid() {
        let qr = Client::new().unwrap().start_qr_login().await.unwrap();
        let size = qr.qr_modules.len();
        println!("live CatPaw QR grid: {size}x{size}");
        assert!(!qr.qr_code_image_url.is_empty());
        assert!((21..=41).contains(&size));
        assert!(qr.qr_modules.iter().all(|row| row.len() == size));
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
