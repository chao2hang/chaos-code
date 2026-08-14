//! Ed25519 离线签名验证。
//!
//! 签名格式（原始 ed25519，无 minisign 头）：
//!
//! ```text
//! <base64 编码的 64 字节签名>
//! ```
//!
//! 公钥格式：
//!
//! ```text
//! <base64 编码的 32 字节公钥>
//! ```
//!
//! 设计原则：
//! - **只做验证，不做签名**：签名密钥仅在发布者本地/CI secret 中存在。
//!   本 crate 只负责"拿到 sig + 公钥 + 二进制，返回是否合法"。
//! - **单公钥、单签名**：不像 minisign 支持 trusted comment / key id，
//!   减少表面积，方便审计。
//! - **不和 auto_update.rs 紧耦合**：这个模块只关心字节，不关心网络、
//!   路径、文件名。auto_update 在下载完二进制和 .sig 文件后调
//!   [`verify_file`] 即可。
//!
//! 公钥来源：
//!
//! [`PUBLIC_KEY`] 是编译期常量，**默认值是占位符**（全 0 字节，表示
//! "未配置签名，验证永远失败"）。正式发版时通过
//! `CHAOS_SIGNING_PUBLIC_KEY` 环境变量注入：
//!
//! ```sh
//! CHAOS_SIGNING_PUBLIC_KEY=<base64> cargo build -p xai-grok-pager-bin --release
//! ```
//!
//! Gray-release switch:
//!
//! Set `CHAOS_REQUIRE_SIG=0` to temporarily skip signature verification
//! (for development or a misconfigured release). The default is controlled
//! by the `require-sig` Cargo feature: enabled when the feature is on
//! (intended for production release builds), disabled otherwise. The env
//! var always wins over the feature default.

use std::path::Path;

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use thiserror::Error;

/// Signature verification errors.
///
/// Every variant is intentionally vague about what went wrong on the wire
/// side — we don't echo back bytes that could be a secret or leak oracle
/// info. The reason is only printed to tracing at debug level.
#[derive(Debug, Error)]
pub enum SignatureError {
    #[error("signature verification failed")]
    VerificationFailed,

    #[error("public key not configured")]
    NoPublicKey,

    #[error("signature file missing or unreadable")]
    SignatureUnreadable,

    #[error("binary file missing or unreadable")]
    BinaryUnreadable,

    #[error("invalid public key format")]
    InvalidPublicKey,

    #[error("invalid signature format")]
    InvalidSignature,
}

/// Placeholder public key (all zeros). Means "signing not configured yet".
///
/// In production builds this gets overridden at compile time via
/// `option_env!("CHAOS_SIGNING_PUBLIC_KEY")`.
pub const PLACEHOLDER_PUBLIC_KEY_B64: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

/// Compile-time public key for signature verification.
///
/// Resolved at build time from the `CHAOS_SIGNING_PUBLIC_KEY` environment
/// variable (base64-encoded 32-byte ed25519 public key). If unset at build
/// time, falls back to [`PLACEHOLDER_PUBLIC_KEY_B64`], which is an invalid
/// key — signature verification will always fail with
/// [`SignatureError::NoPublicKey`].
///
/// This is the single source of truth for "what key do we trust". There is
/// no runtime override: if we let the user point verification at a different
/// key, a compromised update server could just ship a different key too.
pub fn public_key() -> Result<VerifyingKey, SignatureError> {
    let b64 = option_env!("CHAOS_SIGNING_PUBLIC_KEY").unwrap_or(PLACEHOLDER_PUBLIC_KEY_B64);
    parse_public_key_b64(b64)
}

/// Whether the compiled-in public key is the placeholder (all zeros),
/// meaning signing was not configured at build time.
///
/// Use this to gate startup: when `signature_required()` is `true` but
/// this returns `true`, the build is misconfigured and should refuse to
/// run auto-update rather than silently accepting unsigned binaries.
pub fn is_placeholder_key() -> bool {
    let b64 = option_env!("CHAOS_SIGNING_PUBLIC_KEY").unwrap_or(PLACEHOLDER_PUBLIC_KEY_B64);
    b64 == PLACEHOLDER_PUBLIC_KEY_B64
}

/// Gray-release switch for signature verification.
///
/// `CHAOS_REQUIRE_SIG=0` / `=false` / `=no` / `=off` → skip verification.
///
/// Default: **enabled** when the `require-sig` Cargo feature is on (the
/// intended state for production release builds), **disabled** otherwise.
/// The env var always wins over the feature default, so it can be used to
/// force-verify a dev build or to temporarily bypass a misconfigured
/// release. Once `require-sig` is the default in the release profile, the
/// env override is only a escape hatch for emergencies.
pub fn signature_required() -> bool {
    match std::env::var("CHAOS_REQUIRE_SIG") {
        Ok(val) => {
            let v = val.trim().to_ascii_lowercase();
            !(v == "0" || v == "false" || v == "no" || v == "off")
        }
        // Env not set: use the compile-time feature default.
        Err(_) => cfg!(feature = "require-sig"),
    }
}

/// Verify a detached ed25519 signature against a binary blob.
///
/// Returns `Ok(())` on valid, or a [`SignatureError`] otherwise. The error
/// message never contains the signature or key material.
pub fn verify_bytes(
    binary: &[u8],
    signature_b64: &str,
    public_key: &VerifyingKey,
) -> Result<(), SignatureError> {
    let sig_bytes = decode_signature_b64(signature_b64)?;
    let signature = Signature::from_bytes(&sig_bytes);
    public_key.verify(binary, &signature).map_err(|_| {
        tracing::debug!("ed25519 verify failed");
        SignatureError::VerificationFailed
    })
}

/// Verify a file on disk against its `.sig` sidecar.
///
/// Sidecar path is `<binary_path>.sig` if `sig_path` is `None`.
pub fn verify_file(
    binary_path: &Path,
    sig_path: Option<&Path>,
    public_key: &VerifyingKey,
) -> Result<(), SignatureError> {
    let binary = std::fs::read(binary_path).map_err(|e| {
        tracing::debug!(path = %binary_path.display(), error = %e, "binary read failed");
        SignatureError::BinaryUnreadable
    })?;
    let sig_path = sig_path.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        let mut p = binary_path.to_path_buf();
        let ext = p
            .extension()
            .map(|e| format!("{}.sig", e.to_string_lossy()))
            .unwrap_or_else(|| "sig".to_string());
        p.set_extension(ext);
        p
    });
    let sig_text = std::fs::read_to_string(&sig_path).map_err(|e| {
        tracing::debug!(path = %sig_path.display(), error = %e, "sig file read failed");
        SignatureError::SignatureUnreadable
    })?;
    let sig_b64 = extract_signature_body(&sig_text)?;
    verify_bytes(&binary, &sig_b64, public_key)
}

/// Parse a base64-encoded ed25519 public key.
fn parse_public_key_b64(b64: &str) -> Result<VerifyingKey, SignatureError> {
    let bytes = base64_decode(b64).ok_or(SignatureError::InvalidPublicKey)?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| SignatureError::InvalidPublicKey)?;
    // Detect the placeholder (all zeros) — treat it as "not configured"
    // rather than "invalid key", so the error message is clearer.
    if arr == [0u8; 32] {
        return Err(SignatureError::NoPublicKey);
    }
    VerifyingKey::from_bytes(&arr).map_err(|e| {
        tracing::debug!(error = %e, "invalid ed25519 public key");
        SignatureError::InvalidPublicKey
    })
}

/// Decode a signature from its on-disk text format.
///
/// Accepts either:
/// - a minisign-style file with `untrusted comment:` header + body line
/// - a bare base64 string (no header)
///
/// Returns the base64 signature body (still base64 — caller decodes).
fn extract_signature_body(text: &str) -> Result<String, SignatureError> {
    let lines: Vec<&str> = text.lines().collect();
    for line in lines {
        let line = line.trim();
        if line.is_empty() || line.starts_with("untrusted comment:") {
            continue;
        }
        // The first non-comment, non-empty line is the signature.
        // minisign has a trusted comment line after the sig body — we
        // ignore it (we don't do trusted comments).
        return Ok(line.to_string());
    }
    Err(SignatureError::InvalidSignature)
}

/// Decode a base64 signature string into 64 raw bytes.
fn decode_signature_b64(b64: &str) -> Result<[u8; 64], SignatureError> {
    let bytes = base64_decode(b64).ok_or(SignatureError::InvalidSignature)?;
    bytes
        .try_into()
        .map_err(|_| SignatureError::InvalidSignature)
}

/// Thin base64 decode wrapper. Returns `None` on any error — we don't
/// distinguish invalid chars from wrong length; both are "bad signature".
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    STANDARD.decode(s).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Signer;
    use ed25519_dalek::SigningKey;

    // Helper: build a SigningKey from a fixed seed so test vectors are
    // deterministic and don't need to be regenerated on every run.
    fn test_keypair() -> (SigningKey, VerifyingKey) {
        let seed: [u8; 32] = *b"test-seed-0123456789abcdef012345";
        let signing = SigningKey::from_bytes(&seed);
        let verifying = signing.verifying_key();
        (signing, verifying)
    }

    fn b64_encode(bytes: &[u8]) -> String {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        STANDARD.encode(bytes)
    }

    #[test]
    fn valid_signature_verifies() {
        let (sk, vk) = test_keypair();
        let message = b"hello chaos";
        let sig = sk.sign(message);
        let sig_b64 = b64_encode(&sig.to_bytes());
        assert!(verify_bytes(message, &sig_b64, &vk).is_ok());
    }

    #[test]
    fn tampered_binary_fails() {
        let (sk, vk) = test_keypair();
        let message = b"hello chaos";
        let sig = sk.sign(message);
        let sig_b64 = b64_encode(&sig.to_bytes());
        let mut tampered = *message;
        tampered[0] = b'H';
        let err = verify_bytes(&tampered, &sig_b64, &vk).expect_err("should fail");
        assert!(matches!(err, SignatureError::VerificationFailed));
    }

    #[test]
    fn wrong_public_key_fails() {
        let (sk, _) = test_keypair();
        let (_, wrong_vk) = {
            let seed: [u8; 32] = *b"other-seed-0123456789abcdef01234";
            let s = SigningKey::from_bytes(&seed);
            let vk = s.verifying_key();
            (s, vk)
        };
        let message = b"hello";
        let sig = sk.sign(message);
        let sig_b64 = b64_encode(&sig.to_bytes());
        let err = verify_bytes(message, &sig_b64, &wrong_vk).expect_err("should fail");
        assert!(matches!(err, SignatureError::VerificationFailed));
    }

    #[test]
    fn malformed_signature_base64_fails() {
        let (_, vk) = test_keypair();
        let err = verify_bytes(b"hello", "not-valid-base64!!!", &vk).expect_err("should fail");
        assert!(matches!(err, SignatureError::InvalidSignature));
    }

    #[test]
    fn wrong_length_signature_fails() {
        let (_, vk) = test_keypair();
        // 32 bytes of base64 (44 chars) — half of what ed25519 needs
        let short = b64_encode(&[0u8; 32]);
        let err = verify_bytes(b"hello", &short, &vk).expect_err("should fail");
        assert!(matches!(err, SignatureError::InvalidSignature));
    }

    #[test]
    fn placeholder_public_key_rejected() {
        let err = public_key().expect_err("placeholder should fail");
        assert!(matches!(err, SignatureError::NoPublicKey));
    }

    #[test]
    fn minisign_style_header_extracts_body() {
        let text = "untrusted comment: test signature\nABCDEFGH==\ntrusted comment: ignore\n";
        let body = extract_signature_body(text).unwrap();
        assert_eq!(body, "ABCDEFGH==");
    }

    #[test]
    fn bare_b64_signature_extracts_body() {
        let text = "ABCDEFGH==\n";
        let body = extract_signature_body(text).unwrap();
        assert_eq!(body, "ABCDEFGH==");
    }

    #[test]
    fn empty_signature_file_fails() {
        let text = "untrusted comment: only header\n";
        let err = extract_signature_body(text).expect_err("should fail");
        assert!(matches!(err, SignatureError::InvalidSignature));
    }

    #[test]
    fn signature_required_env_parsing() {
        // Test the parsing logic independently of the env, since test
        // harnesses may inherit env vars and the default also depends on
        // the `require-sig` Cargo feature (compile-time).
        assert!(signature_required_env_value("1"));
        assert!(signature_required_env_value("true"));
        assert!(signature_required_env_value("yes"));
        assert!(signature_required_env_value("on"));
        assert!(signature_required_env_value("")); // empty string = still on
        assert!(!signature_required_env_value("0"));
        assert!(!signature_required_env_value("false"));
        assert!(!signature_required_env_value("no"));
        assert!(!signature_required_env_value("off"));
        assert!(!signature_required_env_value("  0  "));
    }

    // Duplicate the parsing logic in test form so it's testable without
    // environmental flakiness. The production function reads from env.
    fn signature_required_env_value(val: &str) -> bool {
        let v = val.trim().to_ascii_lowercase();
        !(v == "0" || v == "false" || v == "no" || v == "off")
    }

    #[test]
    fn verify_file_roundtrip() {
        let (sk, vk) = test_keypair();
        let dir = tempfile::tempdir().unwrap();
        let bin_path = dir.path().join("chaos-test");
        let sig_path = dir.path().join("chaos-test.sig");

        let content = b"fake binary content";
        std::fs::write(&bin_path, content).unwrap();
        let sig = sk.sign(content);
        let sig_text = format!(
            "untrusted comment: signature from test\n{}\n",
            b64_encode(&sig.to_bytes())
        );
        std::fs::write(&sig_path, sig_text).unwrap();

        assert!(verify_file(&bin_path, Some(&sig_path), &vk).is_ok());

        // Tamper with binary
        std::fs::write(&bin_path, b"tampered").unwrap();
        let err = verify_file(&bin_path, Some(&sig_path), &vk).expect_err("should fail");
        assert!(matches!(err, SignatureError::VerificationFailed));
    }

    #[test]
    fn verify_file_missing_sig_fails() {
        let (_, vk) = test_keypair();
        let dir = tempfile::tempdir().unwrap();
        let bin_path = dir.path().join("chaos-test");
        std::fs::write(&bin_path, b"x").unwrap();
        let err = verify_file(&bin_path, None, &vk).expect_err("should fail");
        assert!(matches!(err, SignatureError::SignatureUnreadable));
    }

    #[test]
    fn is_placeholder_key_true_when_unset() {
        // When CHAOS_SIGNING_PUBLIC_KEY is not set at build time, the key
        // is the placeholder (all zeros).
        assert!(is_placeholder_key());
    }

    #[test]
    fn require_configured_public_key_ok_when_not_required() {
        // When signature_required() is false (default without the feature,
        // and env not set to force it), require_configured_public_key
        // should succeed even with a placeholder key.
        // This test assumes CHAOS_REQUIRE_SIG is not set in the test env;
        // if it is, the behavior depends on the env value.
        if !signature_required() {
            assert!(crate::require_configured_public_key().is_ok());
        }
    }

    #[test]
    fn minisign_style_with_trusted_comment_extracts_body() {
        // minisign .sig files may include a trusted comment line after the
        // signature body. We ignore it (we don't do trusted comments), but
        // the extraction must still find the body line.
        let (sk, vk) = test_keypair();
        let message = b"with trusted comment";
        let sig = sk.sign(message);
        let sig_text = format!(
            "untrusted comment: minisign sig\n{}\ntrusted comment: timestamp:now\n",
            b64_encode(&sig.to_bytes())
        );
        let body = extract_signature_body(&sig_text).unwrap();
        assert_eq!(body, b64_encode(&sig.to_bytes()));
        assert!(verify_bytes(message, &body, &vk).is_ok());
    }

    #[test]
    fn bare_b64_with_whitespace_extracts_body() {
        // A bare base64 signature (no minisign header) with surrounding
        // whitespace should still be extracted correctly.
        let (sk, vk) = test_keypair();
        let message = b"bare b64";
        let sig = sk.sign(message);
        let sig_b64 = b64_encode(&sig.to_bytes());
        let text = format!("  \n{sig_b64}\n  \n");
        let body = extract_signature_body(&text).unwrap();
        assert_eq!(body, sig_b64);
        assert!(verify_bytes(message, &body, &vk).is_ok());
    }
}
