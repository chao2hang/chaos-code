use std::path::PathBuf;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("AES error: {0}")]
    Aes(String),
    #[error("base64 error: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("CatPaw authentication error: {0}")]
    Auth(String),
    #[error("CatPaw client configuration error: {0}")]
    Config(String),
    #[error("cryptography error: {0}")]
    Crypto(String),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("PKCS#7 padding error")]
    Pkcs7,
    #[error("RSA error: {0}")]
    Rsa(String),
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("account store file is not owner-only: {0}")]
    UnsafePermissions(PathBuf),
    #[error("upstream returned HTTP {status}: {body}")]
    Upstream { status: u16, body: String },
    #[error("UTF-8 error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
}

impl From<rsa::Error> for Error {
    fn from(error: rsa::Error) -> Self {
        Self::Rsa(error.to_string())
    }
}
