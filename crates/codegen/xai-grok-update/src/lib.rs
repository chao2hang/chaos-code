pub mod auto_update;
pub mod version;
mod version_policy;

pub use auto_update::UpdateStatus;
pub use version::{UpdateConfig, channel_label, channel_name, write_version_cache};
pub use version_policy::enforce_version_policy_or_exit;
/// The verification primitives live in their own crate, because the remote host
/// installs artifacts too and cannot depend upward on this crate. Re-exported
/// under the path every caller (and `tests/test_signature_integration.rs`)
/// already uses.
pub use xai_grok_signature as signature;
pub use xai_grok_signature::{
    SignatureError, is_placeholder_key, public_key, require_configured_public_key,
    signature_required, verify_bytes, verify_file,
};
