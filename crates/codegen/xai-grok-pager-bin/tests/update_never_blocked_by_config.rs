//! `grok update` is a recovery command: a config failure must not block it.
//!
//! A local server answers every lookup the updater makes — channel pointer and
//! release tag alike — with the binary's own version, so a healthy run exits 0
//! ("already up to date") without reaching `api.github.com`.
//! A run with a corrupt config must exit 0 too; reintroducing a config `?` fails exactly that run.
//! The pointer must equal the current version: the installer converges in both directions, so an older pointer triggers a downgrade attempt.

use std::io::{Read, Write};
use std::process::Command;
use std::sync::{Arc, Mutex};

/// Resolve the pager binary like the PTY harness: `PAGER_BINARY` under Bazel (runfiles-relative), else cargo's compile-time constant.
fn pager_binary() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("PAGER_BINARY") {
        return std::path::absolute(&p)
            .unwrap_or_else(|e| panic!("failed to absolutize PAGER_BINARY {p}: {e}"));
    }
    // `chaos` 是本分支的二进制名（bin target 已改名），旧名保留为上游兼容回退。
    option_env!("CARGO_BIN_EXE_chaos")
        .or(option_env!("CARGO_BIN_EXE_xai-grok-pager"))
        .map(std::path::PathBuf::from)
        .expect("PAGER_BINARY is unset and this build is not `cargo test`")
}

/// Local stand-in for the update endpoints, plus a log of what the binary asked for.
struct UpdateEndpoints {
    /// Owning the listener is what keeps the accept loop alive.
    _listener: std::net::TcpListener,
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl UpdateEndpoints {
    /// Request paths in arrival order.
    fn paths(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

/// Answer both lookups the updater makes: the channel pointer (`{base}/{channel}`,
/// plain text) and the GitHub release API (`/repos/{repo}/releases[?…]`, JSON).
/// Without the API half `grok update` falls through to `api.github.com`, which
/// makes the outcome depend on whether the repository has a published release —
/// a fork with none sees HTTP 403 and the run fails for an unrelated reason.
fn spawn_update_endpoints(body: Arc<Mutex<String>>) -> UpdateEndpoints {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let serving = listener.try_clone().unwrap();
    let requests: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let served = requests.clone();
    std::thread::spawn(move || {
        for stream in serving.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut buf = [0u8; 1024];
            let read = stream.read(&mut buf).unwrap_or(0);
            let request_line = String::from_utf8_lossy(&buf[..read])
                .lines()
                .next()
                .unwrap_or_default()
                .to_owned();
            let path = request_line
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_owned();
            served
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(path.clone());
            let version = body.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let payload = if path.contains("/releases/latest") {
                format!(r#"{{"tag_name":"v{version}"}}"#)
            } else if path.contains("/releases") {
                format!(r#"[{{"tag_name":"v{version}","draft":false}}]"#)
            } else {
                version
            };
            let _ = stream.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    payload.len(),
                    payload
                )
                .as_bytes(),
            );
        }
    });
    UpdateEndpoints {
        _listener: listener,
        base,
        requests,
    }
}

/// Run `grok update` in an isolated home against the local endpoints.
fn run_update(base: &str, config_toml: &str, extra_args: &[&str]) -> std::process::Output {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("config.toml"), config_toml).unwrap();
    Command::new(pager_binary())
        .arg("update")
        .args(extra_args)
        .env_clear()
        .env("HOME", home.path())
        .env("GROK_HOME", home.path())
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .envs(platform_essentials())
        .env("GROK_CLI_BASE_URL", base)
        .env("CHAOS_GH_API_BASE", base)
        .env("CHAOS_GH_DOWNLOAD_BASE", base)
        .output()
        .expect("spawn grok update")
}

/// Environment a Windows child needs in order to open a socket at all.
///
/// `env_clear` is the isolation this file is about -- no inherited credentials, no real
/// home -- but Winsock refuses to initialise without the system directory in the
/// environment, and the failure surfaces as a connect error on the *loopback* endpoint:
/// `tcp open error: The requested service provider could not be loaded or initialized.
/// (os error 10106)`, which reads like a broken test server rather than a stripped
/// environment. These are the same keys
/// `xai-grok-test-support::sandbox::platform_allowlist` keeps for spawned children. On
/// unix the list is empty, so the isolation here is unchanged on the host that can verify
/// it.
#[cfg(windows)]
fn platform_essentials() -> Vec<(&'static str, std::ffi::OsString)> {
    ["PATHEXT", "SystemRoot", "WINDIR", "ComSpec", "TEMP", "TMP"]
        .into_iter()
        .filter_map(|key| std::env::var_os(key).map(|value| (key, value)))
        .collect()
}

#[cfg(not(windows))]
fn platform_essentials() -> Vec<(&'static str, std::ffi::OsString)> {
    Vec::new()
}

/// The valid run proves the environment resolves to success, so a nonzero corrupt run can only mean a config failure aborted the update.
#[test]
fn corrupt_config_never_changes_update_outcome() {
    let body = Arc::new(Mutex::new("0.0.1".to_owned()));
    let endpoints = spawn_update_endpoints(body.clone());
    let base = endpoints.base.clone();

    // Probe the binary's own version so the pointer matches it exactly.
    let check = run_update(&base, "[cli]\n", &["--check", "--json"]);
    let status: serde_json::Value = serde_json::from_slice(&check.stdout)
        .unwrap_or_else(|e| panic!("update --check --json must emit JSON: {e}"));
    let current = status["currentVersion"]
        .as_str()
        .expect("currentVersion in update --check --json")
        .to_owned();
    *body.lock().unwrap_or_else(|e| e.into_inner()) = current;

    let valid = run_update(&base, "[cli]\n", &[]);
    assert!(
        valid.status.success(),
        "healthy grok update against the local base must exit 0\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&valid.stdout),
        String::from_utf8_lossy(&valid.stderr)
    );

    // The release tag must have come from this server: if the updater resolved
    // it anywhere else, the green exit above says nothing about this repo.
    let paths = endpoints.paths();
    assert!(
        paths.iter().any(|p| p.contains("/releases")),
        "the updater must resolve the release tag from the local base; requests: {paths:?}"
    );

    let corrupt = run_update(&base, "this is not toml {{{[[[", &[]);
    assert!(
        corrupt.status.success(),
        "a corrupt config.toml must not block grok update\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&corrupt.stdout),
        String::from_utf8_lossy(&corrupt.stderr)
    );
}
