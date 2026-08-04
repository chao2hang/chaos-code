//! Login and refresh wire types. CatPaw has returned both camelCase and
//! snake_case spellings over time; deserialization accepts both while the
//! canonical serialization remains camelCase.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenSet {
    #[serde(alias = "access_token")]
    pub access_token: String,
    #[serde(alias = "refresh_token")]
    pub refresh_token: String,
    #[serde(default, alias = "expires_at")]
    pub expires: i64,
    #[serde(default, alias = "refresh_expires")]
    pub refresh_expires: i64,
    #[serde(default, alias = "mis_id")]
    pub mis_id: Option<String>,
    #[serde(default, alias = "user_info")]
    pub user_info: Option<Value>,
}

impl std::fmt::Debug for TokenSet {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TokenSet")
            .field("access_token", &"[REDACTED]")
            .field("refresh_token", &"[REDACTED]")
            .field("expires", &self.expires)
            .field("refresh_expires", &self.refresh_expires)
            .field("mis_id", &self.mis_id)
            .field("user_info", &self.user_info)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RefreshTokenRequest {
    #[serde(alias = "refresh_token")]
    pub refresh_token: String,
}

impl RefreshTokenRequest {
    pub fn new(refresh_token: impl Into<String>) -> Self {
        Self {
            refresh_token: refresh_token.into(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RefreshTokenWireResponse {
    #[serde(default, alias = "access_token")]
    pub access_token: Option<String>,
    #[serde(default, alias = "refresh_token")]
    pub refresh_token: Option<String>,
    #[serde(default, alias = "expires_at")]
    pub expires: Option<i64>,
    #[serde(default, alias = "refresh_expires")]
    pub refresh_expires: Option<i64>,
    #[serde(default, alias = "mis_id")]
    pub mis_id: Option<String>,
}

impl RefreshTokenWireResponse {
    pub fn into_token_set(self, old_refresh_token: &str) -> Option<TokenSet> {
        Some(TokenSet {
            access_token: self.access_token?,
            refresh_token: self
                .refresh_token
                .unwrap_or_else(|| old_refresh_token.to_string()),
            expires: self.expires.unwrap_or_default(),
            refresh_expires: self.refresh_expires.unwrap_or_default(),
            mis_id: self.mis_id,
            user_info: None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LoginTokensWire {
    #[serde(alias = "access_token")]
    pub access_token: String,
    #[serde(alias = "refresh_token")]
    pub refresh_token: String,
    #[serde(default, alias = "expires_at")]
    pub expires: i64,
    #[serde(default, alias = "refresh_expires")]
    pub refresh_expires: i64,
    #[serde(default, alias = "mis_id")]
    pub mis_id: Option<String>,
    #[serde(default)]
    pub user_info: Option<Value>,
}

impl From<LoginTokensWire> for TokenSet {
    fn from(value: LoginTokensWire) -> Self {
        Self {
            access_token: value.access_token,
            refresh_token: value.refresh_token,
            expires: value.expires,
            refresh_expires: value.refresh_expires,
            mis_id: value.mis_id,
            user_info: value.user_info,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_wire_accepts_both_field_styles_and_rotates_or_reuses_refresh() {
        let request: RefreshTokenRequest =
            serde_json::from_str(r#"{"refresh_token":"old"}"#).unwrap();
        assert_eq!(request.refresh_token, "old");
        let response: RefreshTokenWireResponse = serde_json::from_value(serde_json::json!({
            "accessToken": "new-access",
            "expires": 123
        }))
        .unwrap();
        assert_eq!(response.into_token_set("old").unwrap().refresh_token, "old");
    }
}
