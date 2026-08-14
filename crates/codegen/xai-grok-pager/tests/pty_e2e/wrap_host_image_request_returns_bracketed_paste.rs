// Per-test-case module for the `pty_e2e` integration test crate.
#[allow(unused_imports)]
use super::common::*;

/// Minimal sanity check: does the wrapped child receive ANY stdin input
/// after writing a host-image request OSC to stderr? We check with a
/// read-with-timeout via `read -t` to avoid hanging.
#[test]
#[ignore = "PTY e2e; run the owning pty_e2e_* Cargo test with --ignored (see Cargo.toml)"]
#[cfg(unix)]
fn wrap_host_image_request_returns_bracketed_paste() {
    // Bash: write request to stderr, read up to 512 bytes with 2s timeout,
    // print what we got then exit. Using `read -N 512 -t 2` isn't portable
    // everywhere, so we use `timeout 3 cat | head -c 512 | od -c` instead.
    let script = r#"
printf '\033]999;ChaosWrapClipboardImage?\007' >&2
# Give wrap time to inject, then dump whatever is on stdin
{ sleep 1; } &
timeout 3 cat 2>/dev/null | head -c 256 | od -c | head -10
echo "DONE"
"#;

    let (_code, raw) = run_wrap(&["bash", "-c", script], &[("SHELL", "/bin/bash")]);

    // Don't assert exit code — timeout may kill cat.
    assert!(raw.contains("DONE"), "must reach DONE marker\nraw:\n{raw}");
    assert!(
        raw.contains("GROK_WRAP_"),
        "wrap must inject GROK_WRAP_* bracketed paste after host-image request\nraw:\n{raw}"
    );
}
