use std::sync::OnceLock;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

use crate::{Error, Result};

pub const XOR_KEY: &str = "ThisIsMyXorKey";

const PUBLIC_KEY_XOR: &str = include_str!("../../assets/key1.b64");
const PRIVATE_KEY_XOR: &str = include_str!("../../assets/key2.b64");

pub fn xor_decipher(data_b64: &str, key: &str) -> Result<String> {
    if key.is_empty() {
        return Err(Error::Crypto("XOR key must not be empty".into()));
    }
    let raw = BASE64.decode(data_b64.trim())?;
    let key = key.as_bytes();
    let plaintext = raw
        .iter()
        .enumerate()
        .map(|(index, byte)| byte ^ key[index % key.len()])
        .collect();
    Ok(String::from_utf8(plaintext)?)
}

pub fn pub_key_pem() -> Result<&'static str> {
    static KEY: OnceLock<Result<String>> = OnceLock::new();
    KEY.get_or_init(|| xor_decipher(PUBLIC_KEY_XOR, XOR_KEY))
        .as_deref()
        .map_err(|error| Error::Crypto(error.to_string()))
}

pub fn priv_key_pem() -> Result<&'static str> {
    static KEY: OnceLock<Result<String>> = OnceLock::new();
    KEY.get_or_init(|| xor_decipher(PRIVATE_KEY_XOR, XOR_KEY))
        .as_deref()
        .map_err(|error| Error::Crypto(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_assets_decode_to_pem() {
        assert!(
            pub_key_pem()
                .unwrap()
                .starts_with("-----BEGIN PUBLIC KEY-----")
        );
        assert!(
            priv_key_pem()
                .unwrap()
                .starts_with("-----BEGIN PRIVATE KEY-----")
        );
    }
}
