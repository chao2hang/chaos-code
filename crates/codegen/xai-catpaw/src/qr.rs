//! QR login response normalization.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QrStart {
    pub code: String,
    #[serde(default)]
    pub expire_time: i64,
    #[serde(default)]
    pub qr_code_image_url: String,
    /// QR modules extracted from `qr_code_image_url` for terminal rendering.
    /// `true` means a dark module. This is intentionally not serialized onto
    /// the wire; it is populated by `Client::start_qr_login` after fetching the
    /// upstream image.
    #[serde(skip)]
    pub qr_modules: Vec<Vec<bool>>,
}

impl QrStart {
    pub fn from_value(value: Value) -> Result<Self> {
        let object = value
            .as_object()
            .ok_or_else(|| Error::Auth("QR start response is not an object".into()))?;
        let code = string_field(object, &["code", "qrCode", "qr_code"])
            .ok_or_else(|| Error::Auth("QR start response has no code".into()))?;
        Ok(Self {
            code,
            expire_time: integer_field(object, &["expireTime", "expire_time", "expiresIn"])
                .unwrap_or_default(),
            qr_code_image_url: string_field(
                object,
                &["qrCodeImageUrl", "qr_code_image_url", "imageUrl", "url"],
            )
            .unwrap_or_default(),
            qr_modules: Vec::new(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QrPoll {
    pub status: QrStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mobile_bound: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_expires: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mis_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum QrStatus {
    Pending,
    Scanned,
    Ok,
    Expired,
}

impl QrPoll {
    pub fn from_value(value: Value) -> Result<Self> {
        let object = value
            .as_object()
            .ok_or_else(|| Error::Auth("QR poll response is not an object".into()))?;
        let access_token = string_field(object, &["accessToken", "access_token"]);
        let refresh_token = string_field(object, &["refreshToken", "refresh_token"]);
        let scanned = bool_field(object, &["scanned", "isScanned"]).unwrap_or(false);
        let expired = bool_field(object, &["expired", "isExpired"]).unwrap_or(false)
            || string_field(object, &["status"])
                .is_some_and(|status| status.eq_ignore_ascii_case("expired"));
        let status = if access_token.is_some() {
            QrStatus::Ok
        } else if expired {
            QrStatus::Expired
        } else if scanned {
            QrStatus::Scanned
        } else {
            QrStatus::Pending
        };
        Ok(Self {
            status,
            mobile_bound: bool_field(object, &["mobileBound", "mobile_bound"]),
            access_token,
            refresh_token,
            expires: integer_field(object, &["expires", "expiresAt", "expires_at"]),
            refresh_expires: integer_field(
                object,
                &["refreshExpires", "refreshExpiresAt", "refresh_expires"],
            ),
            mis_id: string_field(object, &["misId", "mis_id"]),
        })
    }
}

fn string_field(object: &serde_json::Map<String, Value>, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        object
            .get(*name)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn integer_field(object: &serde_json::Map<String, Value>, names: &[&str]) -> Option<i64> {
    names
        .iter()
        .find_map(|name| object.get(*name).and_then(Value::as_i64))
}

fn bool_field(object: &serde_json::Map<String, Value>, names: &[&str]) -> Option<bool> {
    names
        .iter()
        .find_map(|name| object.get(*name).and_then(Value::as_bool))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_start_and_poll_normalize_camel_snake_and_states() {
        let start = QrStart::from_value(serde_json::json!({
            "code": "qr-code",
            "expire_time": 60,
            "qr_code_image_url": "data:image/png;base64,x"
        }))
        .unwrap();
        assert_eq!(start.code, "qr-code");
        assert_eq!(start.expire_time, 60);
        assert_eq!(
            QrPoll::from_value(serde_json::json!({"scanned": false}))
                .unwrap()
                .status,
            QrStatus::Pending
        );
        assert_eq!(
            QrPoll::from_value(serde_json::json!({"scanned": true, "mobileBound": false}))
                .unwrap()
                .status,
            QrStatus::Scanned
        );
        assert_eq!(
            QrPoll::from_value(serde_json::json!({
                "access_token": "a",
                "refreshToken": "r",
                "expiresAt": 10
            }))
            .unwrap()
            .status,
            QrStatus::Ok
        );
        assert_eq!(
            QrPoll::from_value(serde_json::json!({"status": "expired"}))
                .unwrap()
                .status,
            QrStatus::Expired
        );
    }
}
