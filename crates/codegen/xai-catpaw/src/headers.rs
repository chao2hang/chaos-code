//! Header fingerprint used by the CatPaw desktop client.

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use crate::{Error, Result};

pub const API_HOST: &str = "catpaw.meituan.com";
pub const API_BASE: &str = "https://catpaw.meituan.com";
pub const CLIENT_ID: &str = "1d47d6ff96";
pub const SSOID_PREFIX: &str = "f32a546874";
pub const TENANT: &str = "5282fa6645";
pub const IDE_VERSION: &str = "2026.4.7";
pub const AGENT_UI_VERSION: &str = "0.2.5";
pub const PLUGIN_ID: &str = "mt-idekit.mt-idekit-code";
pub const DEFAULT_MIS_ID: &str = "19218289559";
pub const CLIENT_TYPE: &str = "CatPaw IDE";
pub const CLIENT_ENV: &str = "LOCAL_IDE";

#[derive(Clone)]
pub struct UpstreamHeaders {
    pub access_token: String,
    pub mis_id: String,
}

impl std::fmt::Debug for UpstreamHeaders {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UpstreamHeaders")
            .field("access_token", &"[REDACTED]")
            .field("mis_id", &self.mis_id)
            .finish()
    }
}

impl UpstreamHeaders {
    pub fn new(access_token: impl Into<String>, mis_id: impl Into<String>) -> Self {
        Self {
            access_token: access_token.into(),
            mis_id: mis_id.into(),
        }
    }

    pub fn build(&self) -> Result<HeaderMap> {
        let cookie = format!(
            "{CLIENT_ID}_passportid={token}; {SSOID_PREFIX}_ssoid={token}",
            token = self.access_token
        );
        header_map([
            ("cookie", cookie),
            ("catpaw-auth", self.access_token.clone()),
            ("ide-type", CLIENT_TYPE.into()),
            ("client-type", CLIENT_TYPE.into()),
            ("ide-version", IDE_VERSION.into()),
            ("plugin-id", PLUGIN_ID.into()),
            ("plugin-version", IDE_VERSION.into()),
            ("client-env", CLIENT_ENV.into()),
            ("user-mis-id", self.mis_id.clone()),
            ("user-uid", self.mis_id.clone()),
            ("mis-id", self.mis_id.clone()),
            ("content-type", "application/json".into()),
            ("accept", "application/json".into()),
            ("user-agent", default_user_agent()),
            ("platform-info", current_platform().into()),
            ("tenant", TENANT.into()),
        ])
    }

    pub fn build_agent_stream(&self) -> Result<HeaderMap> {
        let mut headers = self.build()?;
        let cookie = headers
            .get("cookie")
            .cloned()
            .ok_or_else(|| Error::Config("CatPaw Cookie header was not built".into()))?;
        headers.insert("accept", HeaderValue::from_static("text/event-stream"));
        headers.insert("cache-control", HeaderValue::from_static("no-cache"));
        headers.insert("connection", HeaderValue::from_static("keep-alive"));
        headers.insert("ui-version", HeaderValue::from_static(AGENT_UI_VERSION));
        headers.insert("catpaw-cookie", cookie);
        Ok(headers)
    }

    pub fn build_anonymous() -> Result<HeaderMap> {
        Self::anonymous()
    }

    pub fn anonymous() -> Result<HeaderMap> {
        header_map([
            ("client-type", CLIENT_TYPE.into()),
            ("ide-version", IDE_VERSION.into()),
            ("tenant", TENANT.into()),
            ("platform", current_platform().into()),
            ("content-type", "application/json".into()),
            ("accept", "application/json".into()),
            ("user-agent", default_user_agent()),
        ])
    }

    pub fn build_minimal(access_token: &str) -> Result<HeaderMap> {
        Self::minimal(access_token)
    }

    pub fn minimal(access_token: &str) -> Result<HeaderMap> {
        header_map([
            ("catpaw-auth", access_token.to_string()),
            ("tenant", TENANT.into()),
            ("content-type", "application/json".into()),
        ])
    }
}

fn header_map<const N: usize>(pairs: [(&str, String); N]) -> Result<HeaderMap> {
    let mut headers = HeaderMap::with_capacity(N);
    for (name, value) in pairs {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|error| Error::Config(format!("invalid header name {name}: {error}")))?;
        let value = HeaderValue::from_str(&value)
            .map_err(|error| Error::Config(format!("invalid value for {name}: {error}")))?;
        headers.insert(name, value);
    }
    Ok(headers)
}

pub fn default_user_agent() -> String {
    // Match the working desktop/relay fingerprint. The apparent Windows token
    // is intentional even when this library runs on another platform; the
    // actual target is reported separately in `platform(-info)`.
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) CatPawRelay/0.1.0".to_string()
}

pub fn current_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "win32-x64"
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        "darwin-arm64"
    } else if cfg!(target_os = "macos") {
        "darwin-x64"
    } else if cfg!(target_arch = "aarch64") {
        "linux-arm64"
    } else {
        "linux-x64"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_headers_include_passport_fingerprint() {
        let headers = UpstreamHeaders::new("access-token", "mis-user")
            .build()
            .unwrap();
        assert_eq!(headers["catpaw-auth"], "access-token");
        assert!(headers["cookie"].to_str().unwrap().contains("access-token"));
        assert_eq!(headers["tenant"], TENANT);
        assert_eq!(headers["platform-info"], current_platform());
    }
}
