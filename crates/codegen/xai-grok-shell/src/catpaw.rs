//! Native CatPaw channel integration for the shell.
//!
//! The wire protocol, crypto, and encrypted account store live in the
//! `xai-catpaw` core crate. This module owns the *location* of the account
//! store (`$CHAOS_HOME/catpaw/`) and exposes thin account-pool helpers for
//! the TUI and sampling layers.
//!
//! Security contract: CatPaw credentials are never written to the config
//! file (`[model_providers]`), logs, traces, or session archives. They live
//! only in the AES-256-GCM encrypted SQLite store opened by
//! [`open_account_store`].

use std::path::PathBuf;

use xai_catpaw::Result;
use xai_catpaw::store::{Account, AccountStore};

pub use xai_catpaw::Client as CatPawClient;
/// Re-export the CatPaw client and QR wire types so the TUI/sampling layers
/// can drive login without depending on the core crate directly.
pub use xai_catpaw::models::{ModelInfo as CatPawModelInfo, ModelMap as CatPawModelMap};
pub use xai_catpaw::qr::{QrPoll, QrStart, QrStatus};
pub use xai_catpaw::quota::QuotaInfo as CatPawQuotaInfo;
pub use xai_catpaw::tokens::TokenSet;

/// Directory holding CatPaw account state: `<grok_home>/catpaw/`.
pub fn catpaw_home() -> PathBuf {
    xai_grok_shell_base::util::grok_home::grok_home().join("catpaw")
}

/// A fresh CatPaw client for QR login / refresh / model / chat calls.
pub fn login_client() -> Result<CatPawClient> {
    CatPawClient::new()
}

/// Open (creating on first use) the encrypted account store and its
/// independent key file. Both files must either already exist together or
/// be created together; the store fails closed on any permission or
/// symlink violation.
pub fn open_account_store() -> Result<AccountStore> {
    let root = catpaw_home();
    AccountStore::open(root.join("accounts.sqlite"), root.join("accounts.key"))
}

/// List all accounts with their metadata (tokens are redacted by the
/// store's `Debug`).
pub fn list_accounts() -> Result<Vec<Account>> {
    open_account_store()?.list()
}

/// Persist a freshly logged-in account's tokens under the given label.
pub fn insert_account(label: &str, mobile: Option<&str>, tokens: &TokenSet) -> Result<i64> {
    open_account_store()?.insert(label, mobile, tokens)
}

/// Atomically select and touch the least-recently-used healthy account.
pub fn select_lru_account() -> Result<Option<Account>> {
    open_account_store()?.select_lru()
}

/// Fetch the native CatPaw model catalog using a credential from the encrypted
/// account pool. The provider label is kept for API symmetry and diagnostics;
/// account selection currently remains global LRU because stored accounts are
/// CatPaw-native rather than tied to one OpenAI-compatible endpoint.
pub async fn fetch_models(_provider: &str) -> Result<CatPawModelMap> {
    let account = select_lru_account()?
        .ok_or_else(|| xai_catpaw::Error::Auth("CatPaw 渠道未登录：请先扫码登录".into()))?;
    let headers = xai_catpaw::headers::UpstreamHeaders::new(
        &account.tokens.access_token,
        account.tokens.mis_id.as_deref().unwrap_or_default(),
    );
    login_client()?.models(&headers).await
}

/// Fetch the current CatPaw account's model quota using the same encrypted
/// credential pool as inference and model discovery.
pub async fn fetch_quota(_provider: &str) -> Result<CatPawQuotaInfo> {
    let account = select_lru_account()?
        .ok_or_else(|| xai_catpaw::Error::Auth("CatPaw 渠道未登录：请先扫码登录".into()))?;
    let headers = xai_catpaw::headers::UpstreamHeaders::new(
        &account.tokens.access_token,
        account.tokens.mis_id.as_deref().unwrap_or_default(),
    );
    login_client()?.quota(&headers).await
}

/// Rotate a stored account's tokens after a refresh.
pub fn update_account_tokens(id: i64, tokens: &TokenSet) -> Result<()> {
    open_account_store()?.update_tokens(id, tokens)
}

/// Remove an account. Returns `true` if it existed.
pub fn delete_account(id: i64) -> Result<bool> {
    open_account_store()?.delete(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catpaw_home_lives_under_grok_home() {
        let home = catpaw_home();
        assert_eq!(home.file_name().and_then(|s| s.to_str()), Some("catpaw"));
        assert!(home.starts_with(xai_grok_shell_base::util::grok_home::grok_home()));
    }
}
