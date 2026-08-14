pub mod auto_update;
pub mod signature;
pub mod version;
mod version_policy;

pub use auto_update::UpdateStatus;
pub use signature::{
    SignatureError, is_placeholder_key, public_key, signature_required, verify_bytes, verify_file,
};
pub use version::{UpdateConfig, channel_label, channel_name, write_version_cache};
pub use version_policy::enforce_version_policy_or_exit;

/// Ensure a signing public key is configured when signature verification
/// is required.
///
/// Call this early in startup (before auto-update runs). When
/// `signature_required()` returns `true` (env `CHAOS_REQUIRE_SIG` is not
/// `0`/`false`/`no`/`off`) but the build has no compiled-in public key
/// ([`is_placeholder_key`] is `true`), this returns an error explaining
/// how to fix it — preventing the updater from silently accepting
/// unverified binaries.
pub fn require_configured_public_key() -> std::result::Result<(), String> {
    if signature_required() && is_placeholder_key() {
        return Err(
            "signature verification required (CHAOS_REQUIRE_SIG is enabled) but \
             CHAOS_SIGNING_PUBLIC_KEY is not configured at build time. \
             Rebuild with CHAOS_SIGNING_PUBLIC_KEY=<base64-32-byte-key> \
             or set CHAOS_REQUIRE_SIG=0 to bypass during the transition."
                .to_string(),
        );
    }
    Ok(())
}
