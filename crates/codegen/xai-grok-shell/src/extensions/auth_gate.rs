use agent_client_protocol as acp;

use crate::auth::{AuthManager, GrokAuth};

/// Advice for a caller that has no xAI credential to offer.
///
/// Upstream told the user to run `grok login`. This fork does not sign in to xAI:
/// `chaos login` is a no-op kept for command-path compatibility (`pager-bin/src/main.rs`),
/// so that advice sent users to a command that prints "configure config.toml" and exits.
/// Duplicated call sites share this constant so the wording cannot drift again.
pub(crate) const NO_XAI_SIGNIN: &str = "Chaos does not sign in to xAI.";

// Some call sites pass their message as `non_xai_message` to `require_xai_auth`,
// which *replaces* that call's `missing_message` rather than appending to it. On
// those branches nothing else says what the user was trying to do, so the
// message has to keep its own lead-in clause. They live here, next to
// `NO_XAI_SIGNIN`, so the shared closing sentence stays reviewable in one place
// instead of being retyped at each site.
pub(crate) const NO_XAI_SIGNIN_BILLING: &str =
    "Billing data requires an xAI account credential. Chaos does not sign in to xAI.";
pub(crate) const NO_XAI_SIGNIN_AUTO_TOPUP: &str =
    "Auto top-up data requires an xAI account credential. Chaos does not sign in to xAI.";
pub(crate) const NO_XAI_SIGNIN_SHARE: &str =
    "Share session requires an xAI account credential. Chaos does not sign in to xAI.";

/// Require xAI auth from a sync context: with no `.await` to refresh, a token inside the client's early-invalidation buffer still counts.
pub(crate) fn require_xai_auth(
    auth_manager: &AuthManager,
    missing_message: &'static str,
    non_xai_message: &'static str,
) -> Result<GrokAuth, acp::Error> {
    let auth = auth_manager
        .current_or_expired()
        .ok_or_else(|| acp::Error::auth_required().data(missing_message))?;
    if !auth.is_xai_auth() {
        return Err(acp::Error::auth_required().data(non_xai_message));
    }
    Ok(auth)
}
