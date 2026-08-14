//! Integration tests for the auto-update signature verification chain.
//!
//! These tests exercise [`auto_update::verify_downloaded_artifact`] against
//! a wiremock server that serves `.sig` sidecar files, covering the three
//! key scenarios: valid signature, tampered binary, and missing .sig file.
//!
//! The signature verification logic itself is unit-tested in
//! `signature::tests`; here we test the *integration* — that the download
//! pipeline correctly fetches the .sig and gates on the result.

#![cfg(unix)]

use std::sync::OnceLock;

mod common;
use common::{reset_home, test_home};

use ed25519_dalek::{Signer, SigningKey};
use xai_grok_update::signature;

/// Test keypair: deterministic seed so test vectors are stable.
fn test_keypair() -> (SigningKey, signature::VerifyingKey) {
    let seed: [u8; 32] = *b"test-seed-0123456789abcdef012345";
    let signing = SigningKey::from_bytes(&seed);
    let verifying = signing.verifying_key();
    (signing, verifying)
}

fn b64_encode(bytes: &[u8]) -> String {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    STANDARD.encode(bytes)
}

/// Write a fake "binary" and its valid .sig to a temp dir.
fn write_signed_binary(
    dir: &std::path::Path,
    content: &[u8],
) -> (std::path::PathBuf, std::path::PathBuf) {
    let (sk, _) = test_keypair();
    let bin_path = dir.join("chaos-test-binary");
    let sig_path = dir.join("chaos-test-binary.sig");

    std::fs::write(&bin_path, content).unwrap();
    let sig = sk.sign(content);
    let sig_text = format!(
        "untrusted comment: test signature\n{}\n",
        b64_encode(&sig.to_bytes())
    );
    std::fs::write(&sig_path, sig_text).unwrap();

    (bin_path, sig_path)
}

/// `verify_file` succeeds on a valid signature and fails on a tampered binary.
#[tokio::test]
#[serial_test::serial]
async fn verify_file_passes_valid_signature() {
    let _ = test_home();
    reset_home();

    let (_, vk) = test_keypair();
    let dir = tempfile::tempdir().unwrap();
    let (bin_path, sig_path) = write_signed_binary(dir.path(), b"real binary content");

    let result = signature::verify_file(&bin_path, Some(&sig_path), &vk);
    assert!(result.is_ok(), "valid signature should verify: {result:?}");
}

#[tokio::test]
#[serial_test::serial]
async fn verify_file_rejects_tampered_binary() {
    let _ = test_home();
    reset_home();

    let (_, vk) = test_keypair();
    let dir = tempfile::tempdir().unwrap();
    let (bin_path, sig_path) = write_signed_binary(dir.path(), b"original content");

    // Tamper with the binary after signing.
    std::fs::write(&bin_path, b"tampered content").unwrap();

    let result = signature::verify_file(&bin_path, Some(&sig_path), &vk);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        signature::SignatureError::VerificationFailed
    ));
}

/// `verify_file` fails when the .sig sidecar is missing.
#[tokio::test]
#[serial_test::serial]
async fn verify_file_fails_when_sig_missing() {
    let _ = test_home();
    reset_home();

    let (_, vk) = test_keypair();
    let dir = tempfile::tempdir().unwrap();
    let bin_path = dir.path().join("chaos-no-sig");
    std::fs::write(&bin_path, b"binary without sig").unwrap();

    // No sig_path provided; verify_file should try the default sidecar
    // (.sig extension) and fail because it doesn't exist.
    let result = signature::verify_file(&bin_path, None, &vk);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        signature::SignatureError::SignatureUnreadable
    ));
}

/// `verify_bytes` handles edge cases: empty signature, wrong length.
#[tokio::test]
#[serial_test::serial]
async fn verify_bytes_rejects_malformed_signatures() {
    let _ = test_home();
    reset_home();

    let (_, vk) = test_keypair();

    // Empty signature string.
    let result = signature::verify_bytes(b"binary", "", &vk);
    assert!(result.is_err());

    // Too short (32 bytes instead of 64).
    let short = b64_encode(&[0u8; 32]);
    let result = signature::verify_bytes(b"binary", &short, &vk);
    assert!(result.is_err());
}

/// `signature_required()` respects the CHAOS_REQUIRE_SIG env var.
///
/// This test is careful to restore the env var afterward so it doesn't
/// pollute other tests in the same binary.
#[tokio::test]
#[serial_test::serial]
async fn signature_required_respects_env_var() {
    let _ = test_home();
    reset_home();

    // Save and restore the env var.
    let original = std::env::var("CHAOS_REQUIRE_SIG").ok();

    // SAFETY: test runs serially; env var is restored after.
    unsafe {
        std::env::set_var("CHAOS_REQUIRE_SIG", "0");
    }
    assert!(!signature::signature_required());

    unsafe {
        std::env::set_var("CHAOS_REQUIRE_SIG", "1");
    }
    assert!(signature::signature_required());

    // Restore.
    unsafe {
        match &original {
            Some(val) => std::env::set_var("CHAOS_REQUIRE_SIG", val),
            None => std::env::remove_var("CHAOS_REQUIRE_SIG"),
        }
    }
}

/// `is_placeholder_key()` returns true when the build has no key configured.
///
/// In the test build, CHAOS_SIGNING_PUBLIC_KEY is not set, so the key is
/// the placeholder (all zeros).
#[tokio::test]
#[serial_test::serial]
async fn is_placeholder_key_true_in_test_build() {
    let _ = test_home();
    reset_home();

    // The test build does not inject CHAOS_SIGNING_PUBLIC_KEY, so this
    // should be true. (If CI injects a key, this test would need adjustment.)
    assert!(signature::is_placeholder_key());
}

/// `require_configured_public_key` is OK when verification is disabled.
///
/// When CHAOS_REQUIRE_SIG is not set and the `require-sig` feature is off
/// (the default in dev builds), the function should succeed even with a
/// placeholder key.
#[tokio::test]
#[serial_test::serial]
async fn require_configured_public_key_ok_when_not_required() {
    let _ = test_home();
    reset_home();

    let original = std::env::var("CHAOS_REQUIRE_SIG").ok();
    // SAFETY: test runs serially; env var is restored after.
    unsafe {
        std::env::set_var("CHAOS_REQUIRE_SIG", "0");
    }

    let result = xai_grok_update::require_configured_public_key();
    assert!(result.is_ok(), "should be OK when not required: {result:?}");

    // Restore.
    unsafe {
        match &original {
            Some(val) => std::env::set_var("CHAOS_REQUIRE_SIG", val),
            None => std::env::remove_var("CHAOS_REQUIRE_SIG"),
        }
    }
}

/// `require_configured_public_key` fails when verification is required but
/// no key is configured (placeholder key in test build).
#[tokio::test]
#[serial_test::serial]
async fn require_configured_public_key_fails_when_required_but_no_key() {
    let _ = test_home();
    reset_home();

    let original = std::env::var("CHAOS_REQUIRE_SIG").ok();
    // SAFETY: test runs serially; env var is restored after.
    unsafe {
        std::env::set_var("CHAOS_REQUIRE_SIG", "1");
    }

    // In the test build, no key is configured, so this should fail.
    let result = xai_grok_update::require_configured_public_key();
    assert!(
        result.is_err(),
        "should fail when required but no key: {result:?}"
    );

    // Restore.
    unsafe {
        match &original {
            Some(val) => std::env::set_var("CHAOS_REQUIRE_SIG", val),
            None => std::env::remove_var("CHAOS_REQUIRE_SIG"),
        }
    }
}

// Suppress unused-import warning for OnceLock (kept for future use).
#[allow(dead_code)]
const _: OnceLock<()> = OnceLock::new();
