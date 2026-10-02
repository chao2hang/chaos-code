//! Whether the bytes a host is being asked to run came from a key it trusts.
//!
//! The digest check in [`super::install`] answers "did the upload arrive whole".
//! It cannot answer "who produced these bytes", because the same caller that sends
//! the artifact also sends its digest — recomputing a sha256 over tampered bytes is
//! free. Provenance is a second, independent question: an ed25519 signature over the
//! artifact, checked against a key the *host* was told about, which the sender has
//! no way to influence unless it holds the signing key.
//!
//! What this buys, and what it does not:
//!
//! - It stops a leaked one-time credential, a tampered build, or somebody with
//!   write access to wherever the artifact was staged from making their bytes the
//!   server that starts next. That is the durable, self-perpetuating failure the
//!   install path is the gate for.
//! - It does not restrict what an authorised session may do with the `exec`
//!   capability. A session allowed to run an arbitrary program can already run an
//!   arbitrary program; that boundary is the capability grant and the `--allow`
//!   list, not this check.
//!
//! The verification primitives live in `xai-grok-signature` — the same ed25519
//! rules, the same sidecar format, the same placeholder-key meaning as `chaos
//! update` and the three installers, so one release key answers for all of them.

use std::path::Path;

use xai_grok_signature::{
    PLACEHOLDER_PUBLIC_KEY_B64, SignatureError, VerifyingKey, parse_public_key_b64, public_key,
    verify_bytes,
};

/// Set to `0`/`false`/`no`/`off` to let an unsigned artifact become current.
///
/// This is the only way the check is skipped, and it is a decision the operator
/// makes about a host — a session cannot ask for it.
pub const REQUIRE_SIGNATURE_ENV: &str = "CHAOS_REMOTE_REQUIRE_SIGNATURE";

/// Key source read at *runtime*, as a fallback for a host running a build whose
/// embedded key is not the one its artifacts are signed with.
pub const SIGNING_PUBLIC_KEY_ENV: &str = "CHAOS_SIGNING_PUBLIC_KEY";

/// The largest artifact the signature check will read into memory.
///
/// Ed25519 verification needs the whole message, so unlike the digest — which is
/// streamed — this one is paid for in RAM. The server already refuses an upload
/// over `max_transfer_bytes * 64` before it writes a byte; this is the ceiling on
/// the same file measured against memory instead of disk. The shipped server is
/// tens of megabytes, so the cap is an order of magnitude of headroom, and going
/// over it is a refusal rather than an out-of-memory kill of the running host.
pub const MAX_VERIFIED_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;

/// What a host requires of an artifact before it becomes `current`.
///
/// Two independent facts: whether a signature must be present, and which key it is
/// checked against. They are separate because the two failures need different fixes
/// — "sign it" versus "tell this host whom to trust".
#[derive(Clone, Debug)]
pub struct ProvenancePolicy {
    /// The key an artifact must be signed by. `None` means none was usable, which
    /// can only ever refuse.
    key: Option<VerifyingKey>,
    /// Where `key` came from, for the message that names the fix.
    key_source: Option<&'static str>,
    /// Why no key could be used, when one was offered and unusable. Kept separate
    /// from `key` so the refusal quotes the real problem instead of the generic
    /// "configure a key" hint.
    key_problem: Option<String>,
    required: bool,
}

impl ProvenancePolicy {
    /// The policy a host starts with: signatures required, key from the
    /// environment or from the build.
    ///
    /// Requiring by default is the point. A host that silently accepted unsigned
    /// artifacts until somebody remembered to configure a key would carry the same
    /// exposure as having no check at all, and the exposure would be invisible.
    pub fn from_env() -> Self {
        Self::from_values(
            &std::env::var(SIGNING_PUBLIC_KEY_ENV).unwrap_or_default(),
            &std::env::var(REQUIRE_SIGNATURE_ENV).unwrap_or_default(),
        )
    }

    /// The same decision given the two environment values, with no process state
    /// involved — which is what makes it testable without racing other tests.
    ///
    /// A key that is present but unparseable does not silently fall back to the
    /// compiled-in one: an operator who exported a broken key means to override it,
    /// and quietly using a different key than the one they typed is worse than
    /// refusing.
    pub fn from_values(key_value: &str, require_value: &str) -> Self {
        let required = require_signature_from_value(require_value);
        let key_value = key_value.trim();
        if key_value.is_empty() {
            return Self::from_compiled_key(required);
        }
        match Self::trusting_key_b64(key_value) {
            Ok(mut policy) => {
                policy.required = required;
                policy
            }
            Err(problem) => Self {
                key: None,
                key_source: None,
                key_problem: Some(problem),
                required,
            },
        }
    }

    /// A policy that checks against `key_b64`, the bare base64 of the 32-byte key
    /// (the same text as a release's `CHAOS_SIGNING_PUBLIC_KEY`), and requires a
    /// signature.
    ///
    /// An all-zeros key — the placeholder a build without `CHAOS_SIGNING_PUBLIC_KEY`
    /// embeds — is reported as "not configured" rather than accepted as a key, so a
    /// host cannot be talked into trusting it.
    pub fn trusting_key_b64(key_b64: &str) -> Result<Self, String> {
        let key_b64 = key_b64.trim();
        if key_b64 == PLACEHOLDER_PUBLIC_KEY_B64 {
            return Err(format!(
                "the signing public key is the all-zeros placeholder, which means \
                 \"no key\": export {SIGNING_PUBLIC_KEY_ENV} or pass \
                 --trust-signing-key with the base64 of the real 32-byte key"
            ));
        }
        match parse_public_key_b64(key_b64) {
            Ok(key) => Ok(Self {
                key: Some(key),
                key_source: Some(SIGNING_PUBLIC_KEY_ENV),
                key_problem: None,
                required: true,
            }),
            Err(e) => Err(format!(
                "the signing public key is not usable: {e}; it must be the base64 of \
                 the 32 raw key bytes, which is `openssl pkey -pubout -outform DER \
                 | tail -c 32 | base64` — not the base64 of the DER encoding"
            )),
        }
    }

    /// The key baked into this build by `option_env!`, if any.
    pub fn from_compiled_key(required: bool) -> Self {
        match public_key() {
            Ok(key) => Self {
                key: Some(key),
                key_source: Some("the key this build was compiled with"),
                key_problem: None,
                required,
            },
            // A build with no embedded key: the host still requires a signature,
            // and says why it cannot check one.
            Err(_) => Self {
                key: None,
                key_source: None,
                key_problem: None,
                required,
            },
        }
    }

    /// A host with no key at all. With `required` set it can only refuse, which is
    /// the honest reading of "verify, but I never told you against what".
    pub fn without_key(required: bool) -> Self {
        Self {
            key: None,
            key_source: None,
            key_problem: None,
            required,
        }
    }

    /// Signatures are not required, but one that is offered is still checked.
    ///
    /// Turning the requirement off is a statement about what the operator accepts
    /// in the absence of a signature, not a statement that a bad signature is
    /// acceptable.
    pub fn unsigned_allowed() -> Self {
        Self::without_key(false)
    }

    /// Same, with a key, for a host that installs unsigned builds sometimes and
    /// checks the signatures that do arrive.
    pub fn unsigned_allowed_with_key_b64(key_b64: &str) -> Result<Self, String> {
        Self::trusting_key_b64(key_b64).map(|mut policy| {
            policy.required = false;
            policy
        })
    }

    pub fn requires_signature(&self) -> bool {
        self.required
    }

    /// Set whether a signature must be present, keeping the key.
    ///
    /// A caller that resolved its key from a flag still wants the environment's
    /// answer to "must there be one" — that switch is the operator's, not the
    /// command line's.
    pub fn with_requirement(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// The key this host will check against, if it has one.
    pub fn key(&self) -> Option<&VerifyingKey> {
        self.key.as_ref()
    }

    /// One line for the startup banner: what this host will accept, and what it
    /// will check against. Printed, because a host whose policy nobody can see is
    /// a host somebody will misdiagnose at 3 a.m.
    pub fn describe(&self) -> String {
        match (self.required, &self.key_source, &self.key_problem) {
            (true, Some(source), _) => format!("a signature from {source} is required"),
            (false, Some(source), _) => {
                format!("unsigned artifacts are allowed; a signature is checked against {source}")
            }
            (_, _, Some(problem)) => format!(
                "no usable signing key: {problem} — installs that need checking \
                 will be refused"
            ),
            (true, None, None) => format!(
                "a signature is required but no key is configured, so every install \
                 will be refused (set {SIGNING_PUBLIC_KEY_ENV} or pass \
                 --trust-signing-key)"
            ),
            (false, None, None) => {
                "unsigned artifacts are allowed and no signature can be checked".to_string()
            }
        }
    }

    /// Read `path` and check it. [`Self::check`] is the same decision on bytes
    /// already in hand.
    pub fn check_file(&self, path: &Path, signature: Option<&str>) -> Result<(), String> {
        if signature.is_none() && !self.required {
            // Nothing to verify and nothing demanded: no reason to read a
            // hundred-megabyte file.
            return Ok(());
        }
        let len = std::fs::metadata(path)
            .map_err(|e| format!("stat {}: {e}", path.display()))?
            .len();
        if len > MAX_VERIFIED_ARTIFACT_BYTES {
            return Err(format!(
                "artifact_too_large: a {len}-byte artifact is over the \
                 {MAX_VERIFIED_ARTIFACT_BYTES}-byte limit for signature checking, \
                 which holds the whole artifact in memory"
            ));
        }
        let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        self.check(&bytes, signature)
    }

    /// The decision itself, on bytes already read.
    pub fn check(&self, artifact: &[u8], signature: Option<&str>) -> Result<(), String> {
        let Some(signature) = signature else {
            if self.required {
                return Err(format!(
                    "signature_missing: the artifact carries no signature, and this \
                     host requires one; produce the sidecar with the release key \
                     (`{}`) and pass it with --signature, or start this host with \
                     --allow-unsigned-artifact if it installs unsigned builds",
                    sidecar_hint()
                ));
            }
            return Ok(());
        };
        let Some(key) = &self.key else {
            return Err(match &self.key_problem {
                Some(problem) => format!("no_trusted_key: {problem}"),
                None => format!(
                    "no_trusted_key: a signature was offered but this host has no \
                     signing public key to check it against; pass \
                     --trust-signing-key or set {SIGNING_PUBLIC_KEY_ENV}"
                ),
            });
        };
        match verify_bytes(artifact, signature, key) {
            Ok(()) => Ok(()),
            Err(SignatureError::InvalidSignature) => Err(
                "signature_malformed: the offered signature is not the base64 of a \
                 64-byte ed25519 signature"
                    .to_string(),
            ),
            Err(_) => Err(format!(
                "signature_invalid: the signature does not match this artifact under \
                 {}",
                self.key_source.unwrap_or("the configured key")
            )),
        }
    }
}

/// Whether the environment asks for signatures, with the same vocabulary as
/// `CHAOS_REQUIRE_SIG` so one muscle memory covers both switches. An unset or
/// empty value means required.
fn require_signature_from_value(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    !(value == "0" || value == "false" || value == "no" || value == "off")
}

/// [`require_signature_from_value`] read from the process environment.
///
/// Public for a caller that gets its key from somewhere the policy constructors do
/// not look at — `chaos-remote-server --trust-signing-key` — and still has to honour
/// the one switch that turns the requirement off.
pub fn requirement_requested_by_env() -> bool {
    require_signature_from_value(&std::env::var(REQUIRE_SIGNATURE_ENV).unwrap_or_default())
}

/// The command that produces a sidecar, quoted in the refusal.
fn sidecar_hint() -> &'static str {
    "openssl pkeyutl -rawin -in <artifact> -inkey <key.pem> -sign | base64"
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use ed25519_dalek::{Signer as _, SigningKey};

    fn keypair(seed: u8) -> (SigningKey, String) {
        let signing = SigningKey::from_bytes(&[seed; 32]);
        let b64 = base64::engine::general_purpose::STANDARD.encode(signing.verifying_key());
        (signing, b64)
    }

    fn signature_for(signing: &SigningKey, bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(signing.sign(bytes).to_bytes())
    }

    #[test]
    fn a_matching_signature_is_accepted() {
        let (signing, public) = keypair(7);
        let policy = ProvenancePolicy::trusting_key_b64(&public).expect("a valid key");
        let artifact = b"#!/bin/sh\necho the server\n";
        policy
            .check(artifact, Some(&signature_for(&signing, artifact)))
            .expect("the signature matches the bytes");
        assert!(policy.requires_signature());
    }

    /// The attack the digest cannot see: the bytes were changed and the sha256 was
    /// recomputed over the result.
    #[test]
    fn a_signature_over_other_bytes_is_refused_by_name() {
        let (signing, public) = keypair(8);
        let policy = ProvenancePolicy::trusting_key_b64(&public).expect("a valid key");
        let error = policy
            .check(
                b"the published artifact",
                Some(&signature_for(&signing, b"swapped in")),
            )
            .expect_err("a signature over other bytes must not pass");
        assert!(error.starts_with("signature_invalid"), "{error}");
    }

    #[test]
    fn a_signature_from_another_key_is_refused_by_name() {
        let (signing, _) = keypair(9);
        let (_, other_public) = keypair(10);
        let policy = ProvenancePolicy::trusting_key_b64(&other_public).expect("a valid key");
        let artifact = b"the published artifact";
        let error = policy
            .check(artifact, Some(&signature_for(&signing, artifact)))
            .expect_err("another key's signature must not pass");
        assert!(error.starts_with("signature_invalid"), "{error}");
    }

    #[test]
    fn a_missing_signature_is_refused_by_name() {
        let (_, public) = keypair(11);
        let policy = ProvenancePolicy::trusting_key_b64(&public).expect("a valid key");
        let error = policy
            .check(b"whatever", None)
            .expect_err("a required signature cannot be absent");
        assert!(error.starts_with("signature_missing"), "{error}");
        assert!(
            error.contains("--allow-unsigned-artifact"),
            "the refusal has to name the way out: {error}"
        );
    }

    #[test]
    fn a_malformed_signature_is_refused_by_name() {
        let (_, public) = keypair(12);
        let policy = ProvenancePolicy::trusting_key_b64(&public).expect("a valid key");
        let error = policy
            .check(b"whatever", Some("not base64 at all !!!"))
            .expect_err("a signature that cannot be parsed is not a signature");
        assert!(error.starts_with("signature_malformed"), "{error}");
    }

    /// Opting out of the requirement is not opting out of checking a signature
    /// that happens to arrive.
    #[test]
    fn a_bad_signature_is_still_refused_when_signatures_are_not_required() {
        let (signing, public) = keypair(13);
        let policy = ProvenancePolicy::unsigned_allowed_with_key_b64(&public).expect("a valid key");
        assert!(!policy.requires_signature());
        policy
            .check(b"unsigned is fine", None)
            .expect("absent is allowed when not required");
        let error = policy
            .check(
                b"the real bytes",
                Some(&signature_for(&signing, b"other bytes")),
            )
            .expect_err("present and wrong is still wrong");
        assert!(error.starts_with("signature_invalid"), "{error}");
    }

    #[test]
    fn a_host_without_a_key_refuses_a_signature_it_cannot_check() {
        let (signing, _) = keypair(14);
        let artifact = b"the published artifact";
        let policy = ProvenancePolicy::unsigned_allowed();
        let error = policy
            .check(artifact, Some(&signature_for(&signing, artifact)))
            .expect_err("an unchecked signature is not a verified one");
        assert!(error.starts_with("no_trusted_key"), "{error}");
    }

    /// Fail closed means the *absence* of a key is not a licence to install: the
    /// requirement stands and the refusal says what to configure.
    #[test]
    fn requiring_without_a_key_refuses_and_names_the_fix() {
        let policy = ProvenancePolicy::without_key(true);
        let absent = policy
            .check(b"whatever", None)
            .expect_err("no key means nothing can be verified");
        assert!(absent.starts_with("signature_missing"), "{absent}");
        let offered = policy
            .check(b"whatever", Some(&"A".repeat(88)))
            .expect_err("still no key to check against");
        assert!(offered.starts_with("no_trusted_key"), "{offered}");
        assert!(
            policy.describe().contains("no key is configured"),
            "the banner has to say so too: {}",
            policy.describe()
        );
    }

    #[test]
    fn the_placeholder_key_is_not_treated_as_a_key() {
        let error = ProvenancePolicy::trusting_key_b64(PLACEHOLDER_PUBLIC_KEY_B64)
            .expect_err("all zeros means nobody configured a key");
        assert!(error.contains("placeholder"), "{error}");
    }

    #[test]
    fn a_key_that_is_not_32_bytes_is_rejected_when_configured() {
        let short = base64::engine::general_purpose::STANDARD.encode([0u8; 31]);
        let error = ProvenancePolicy::trusting_key_b64(&short).expect_err("wrong length");
        // The refusal has to say what shape was wanted, because the usual cause is
        // pasting the base64 of the whole DER key instead of the raw 32 bytes.
        assert!(error.contains("32 raw"), "{error}");
        assert!(error.contains("tail -c 32"), "{error}");
    }

    #[test]
    fn a_key_may_arrive_with_the_whitespace_a_shell_added() {
        let (_, public) = keypair(15);
        let padded = format!("  {public}\n");
        let policy = ProvenancePolicy::trusting_key_b64(&padded).expect("trimmed key");
        assert!(policy.key().is_some());
    }

    #[test]
    fn the_requirement_switch_reads_the_published_values() {
        for value in ["0", "false", "FALSE", " no ", "Off"] {
            assert!(
                !require_signature_from_value(value),
                "{value:?} must turn the requirement off"
            );
        }
        for value in ["", "1", "true", "yes", "on", "always"] {
            assert!(
                require_signature_from_value(value),
                "{value:?} must leave the requirement on"
            );
        }
    }

    /// What the two switches do when read together, which is the shape the server
    /// actually starts from.
    #[test]
    fn the_environment_pairs_a_key_with_a_requirement() {
        let (signing, public) = keypair(18);
        let artifact = b"the published artifact";
        let sig = signature_for(&signing, artifact);

        let both = ProvenancePolicy::from_values(&public, "1");
        assert!(both.requires_signature());
        assert!(both.key().is_some());
        both.check(artifact, Some(&sig))
            .expect("signed and required");

        let off = ProvenancePolicy::from_values(&public, "0");
        assert!(!off.requires_signature());
        off.check(artifact, None)
            .expect("the hatch lets it through");
        off.check(artifact, Some("not base64 !!!"))
            .expect_err("the hatch does not mean a broken signature passes");

        let no_key = ProvenancePolicy::from_values("", "1");
        assert!(
            no_key.requires_signature(),
            "an empty key value is still a required host"
        );

        // A key that was typed wrong is reported, not quietly ignored.
        let broken = ProvenancePolicy::from_values("not-a-key!!!", "1");
        assert!(broken.key().is_none());
        let error = broken
            .check(artifact, Some(&sig))
            .expect_err("an unusable key verifies nothing");
        assert!(error.starts_with("no_trusted_key"), "{error}");
        assert!(
            broken.describe().contains("not usable"),
            "the banner names the broken key: {}",
            broken.describe()
        );
    }

    #[test]
    fn check_file_reads_the_artifact_it_is_pointed_at() {
        let (signing, public) = keypair(16);
        let policy = ProvenancePolicy::trusting_key_b64(&public).expect("a valid key");
        let dir = tempfile::tempdir().expect("a scratch directory");
        let path = dir.path().join("chaos-remote-server");
        std::fs::write(&path, b"the published artifact").expect("write the artifact");
        policy
            .check_file(
                &path,
                Some(&signature_for(&signing, b"the published artifact")),
            )
            .expect("the file's own bytes verify");
        let error = policy
            .check_file(&path, Some(&signature_for(&signing, b"not those bytes")))
            .expect_err("the file, not the caller, decides");
        assert!(error.starts_with("signature_invalid"), "{error}");
    }

    /// The cap exists so that exceeding it costs a rejected request rather than the
    /// host's process; it is checked before the file is read.
    #[test]
    fn an_artifact_too_large_to_hold_in_memory_is_refused_without_reading_it() {
        let (signing, public) = keypair(17);
        let policy = ProvenancePolicy::trusting_key_b64(&public).expect("a valid key");
        let dir = tempfile::tempdir().expect("a scratch directory");
        let path = dir.path().join("huge");
        let file = std::fs::File::create(&path).expect("create the file");
        file.set_len(MAX_VERIFIED_ARTIFACT_BYTES + 1)
            .expect("extend a sparse file");
        let error = policy
            .check_file(&path, Some(&signature_for(&signing, b"whatever")))
            .expect_err("over the cap is over the cap");
        assert!(error.starts_with("artifact_too_large"), "{error}");
    }
}
