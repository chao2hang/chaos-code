//! Verify a downloaded release artifact against its `.sig` sidecar with the same
//! code the auto-updater runs at install time.
//!
//! `signature::public_key()` is fixed at compile time from
//! `CHAOS_SIGNING_PUBLIC_KEY`, so a normal `cargo build` carries the placeholder
//! and can only ever refuse. That is the production rule, not a bug to work
//! around here — pass the key the release actually used at build time:
//!
//! ```text
//! CHAOS_SIGNING_PUBLIC_KEY="$(gh variable get CHAOS_SIGNING_PUBLIC_KEY -R <repo>)" \
//!   cargo build --release --example verify_release_artifact -p xai-grok-update
//! ```
//!
//! Then point it at an artifact and its sidecar:
//!
//! ```text
//! ./target/release/examples/verify_release_artifact chaos-linux-x64 chaos-linux-x64.sig
//! ./target/release/examples/verify_release_artifact chaos-linux-x64 chaos-linux-x64.sig --tamper
//! ```
//!
//! `--tamper` re-checks a single-byte-corrupted copy of the same artifact. Ask for
//! it: a check that reports `verify=ok` but would also accept corrupted bytes is
//! worth nothing, and this is the only way to tell the two apart from outside.
//!
//! Exit status: `0` verified, `1` refused, `2` no public key compiled in.

use std::path::{Path, PathBuf};

fn usage() -> ! {
    eprintln!(
        "usage: verify_release_artifact <binary> <signature.sig> [--tamper]\n\
         \n\
         Verifies <binary> against <signature.sig> using xai_grok_update::signature,\n\
         the same verification the auto-updater performs before it activates a build.\n\
         --tamper also verifies a one-byte-corrupted copy, which must be refused."
    );
    std::process::exit(2);
}

/// Flip one byte of `binary` into a temp copy and return its path.
fn tampered_copy(binary: &Path) -> Result<PathBuf, String> {
    let bytes = std::fs::read(binary).map_err(|e| format!("reading {}: {e}", binary.display()))?;
    if bytes.is_empty() {
        return Err("artifact is empty, nothing to corrupt".into());
    }
    let mut corrupted = bytes;
    let mid = corrupted.len() / 2;
    corrupted[mid] ^= 0x01;
    let name = binary
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("artifact");
    let path = std::env::temp_dir().join(format!("chaos-tampered-{name}-{}", std::process::id()));
    std::fs::write(&path, &corrupted).map_err(|e| format!("writing temp copy: {e}"))?;
    Ok(path)
}

fn verify(label: &str, binary: &Path, sig: &Path, key: &ed25519_dalek::VerifyingKey) -> bool {
    match xai_grok_update::signature::verify_file(binary, Some(sig), key) {
        Ok(()) => {
            println!("verify[{label}]=ok");
            true
        }
        Err(e) => {
            println!("verify[{label}]=refused: {e}");
            false
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut binary: Option<PathBuf> = None;
    let mut sig: Option<PathBuf> = None;
    let mut tamper = false;

    for arg in &mut args {
        match arg.as_str() {
            "--tamper" => tamper = true,
            "-h" | "--help" => usage(),
            _ if arg.starts_with('-') => {
                eprintln!("unknown option: {arg}");
                usage();
            }
            _ if binary.is_none() => binary = Some(PathBuf::from(arg)),
            _ if sig.is_none() => sig = Some(PathBuf::from(arg)),
            _ => {
                eprintln!("unexpected argument: {arg}");
                usage();
            }
        }
    }

    let (Some(binary), Some(sig)) = (binary, sig) else {
        usage()
    };

    // Reported separately from the verify result: a refusal here means the build
    // has no key at all, which is a different failure than a bad signature.
    let Ok(key) = xai_grok_update::signature::public_key() else {
        println!("public_key=absent");
        eprintln!(
            "this build has no CHAOS_SIGNING_PUBLIC_KEY compiled in, so it can never \
             accept a signature; rebuild with the release's public key"
        );
        std::process::exit(2);
    };
    println!("public_key=configured");

    let mut ok = verify("artifact", &binary, &sig, &key);

    if tamper {
        match tampered_copy(&binary) {
            Ok(path) => {
                // The corrupt copy must be refused; if it verifies, the check above
                // proved nothing.
                let accepted = verify("tampered", &path, &sig, &key);
                let _ = std::fs::remove_file(&path);
                ok &= !accepted;
            }
            Err(e) => {
                eprintln!("tamper check could not run: {e}");
                ok = false;
            }
        }
    }

    std::process::exit(if ok { 0 } else { 1 });
}
