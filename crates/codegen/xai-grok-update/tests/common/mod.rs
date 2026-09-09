//! Shared helpers for integration tests.
//!
//! Each `tests/*.rs` integration test is its own binary, so each binary has its own `OnceLock<GROK_HOME>`.
//! The helpers below ensure the per-binary initialization is identical: same env-var set, same isolation guarantees, same reset between tests.
//!
//! Mirrors the GROK_HOME isolation pattern used in other integration tests.
//!
//! ## Usage
//!
//! ```ignore
//! mod common;
//! use common::{test_home, reset_home};
//!
//! #[tokio::test]
//! #[serial_test::serial]
//! async fn my_test() {
//!     let _ = test_home();   // initializes GROK_HOME once per binary
//!     reset_home();          // wipes state between tests
//!     // ...
//! }
//! ```

#![allow(dead_code)] // each test binary uses a different subset

#[cfg(unix)]
pub mod artifact_server;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

// ─────────────────────────────────────────────────────────────────────────────
// GROK_HOME isolation
// ─────────────────────────────────────────────────────────────────────────────

/// Returns a process-wide test `GROK_HOME`, initialized exactly once per test binary.
/// Once initialized, `xai_grok_config::grok_home()` will resolve to this directory for the lifetime of the process.
///
/// Also clears env vars that the auto-update code consults so a parent shell's values can't pollute the baseline.
/// For example, running tests from `npm run` would otherwise inherit `npm_config_user_agent` and `NPM_TOKEN`.
pub fn test_home() -> &'static PathBuf {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.keep();
        // SAFETY: called once at OnceLock init, before any other thread touches
        // these env vars. Tests using this helper must be `#[serial]`.
        unsafe {
            std::env::set_var("GROK_HOME", &path);
            std::env::remove_var("GROK_TEST_VERSION");
            std::env::remove_var("NPM_TOKEN");
            std::env::remove_var("GROK_INSTALLER");
            std::env::remove_var("GROK_MANAGED_BY_NPM");
            std::env::remove_var("GROK_MANAGED_BY_INTERNAL");
        }
        path
    })
}

/// Wipe state in `GROK_HOME` between tests so each test sees a clean home.
/// Removes the well-known files and subdirectories the update path writes, and clears env vars that individual tests may set.
pub fn reset_home() {
    let home = test_home();
    let _ = std::fs::remove_file(home.join("config.toml"));
    let _ = std::fs::remove_file(home.join("version.json"));
    let _ = std::fs::remove_file(home.join("version.json.tmp"));
    let _ = std::fs::remove_dir_all(home.join("bin"));
    let _ = std::fs::remove_dir_all(home.join("downloads"));
    // SAFETY: tests using this helper must be `#[serial]`.
    unsafe {
        std::env::remove_var("GROK_TEST_VERSION");
        std::env::remove_var("NPM_TOKEN");
        std::env::remove_var("GROK_INSTALLER");
    }
}

/// Override the version reported by `get_installed_grok_version()` for the
/// duration of the test (until [`reset_home`] or process exit).
pub fn set_test_version(v: &str) {
    // SAFETY: tests using this helper must be `#[serial]`.
    unsafe { std::env::set_var("GROK_TEST_VERSION", v) };
}

// ─────────────────────────────────────────────────────────────────────────────
// Install-test fixtures (shared by the blitz + convergence suites)
// ─────────────────────────────────────────────────────────────────────────────

/// Host `{os}-{arch}` string matching the versioned binary naming scheme (`grok-{version}-{platform}`).
pub fn host_platform() -> String {
    let os = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        panic!("unsupported test platform");
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        panic!("unsupported test arch");
    };
    format!("{os}-{arch}")
}

/// Minimal [`xai_grok_update::UpdateConfig`] for install tests.
pub fn make_update_config(channel: &str) -> xai_grok_update::UpdateConfig {
    xai_grok_update::UpdateConfig {
        proxy_base_url: "http://test.invalid/v1".to_string(),
        auth_scope: "test".to_string(),
        deployment_key: None,
        alpha_test_key: None,
        channel: channel.to_string(),
        npm_registry: None,
    }
}

/// True if shell-script artifacts can execute in this environment.
/// False in restricted sandboxes (e.g. hermetic remote execution) that lack /bin/sh.
#[cfg(unix)]
pub fn can_exec_shell_scripts() -> bool {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("probe");
    std::fs::write(&p, b"#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::process::Command::new(&p)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// A small real executable: exits 0 for `--version`, so the smoke-test passes.
pub fn small_good_artifact() -> Vec<u8> {
    b"#!/bin/sh\nexit 0\n".to_vec()
}

/// Backdate every file in `GROK_HOME/downloads` by ~2 hours.
///
/// `cleanup_old_downloads` deliberately never deletes a freshly-written binary or temp file (it may belong to a concurrent in-flight install).
/// Tests asserting the retention policy must therefore age their fixtures to look like real leftovers from previous releases.
pub fn backdate_downloads() {
    let downloads = test_home().join("downloads");
    let Ok(entries) = std::fs::read_dir(&downloads) else {
        return;
    };
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 60 * 60);
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_file()
            && let Ok(f) = std::fs::File::options().write(true).open(&p)
        {
            let _ = f.set_times(std::fs::FileTimes::new().set_modified(old));
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// PATH-override fake binary
// ─────────────────────────────────────────────────────────────────────────────

/// RAII guard that places a sh-script with name `name` at the head of `PATH`.
/// Restores `PATH` on drop.
///
/// All tests using this MUST be `#[serial]` because `PATH` is process-global.
pub struct FakeBinGuard {
    pub tmp: tempfile::TempDir,
    pub name: String,
    prev_path: OsString,
}

impl FakeBinGuard {
    /// Install a fake binary at `<tmp>/<name>` whose body is produced by `script_body(<tmp>)`, and prepend `<tmp>` to `PATH`.
    pub fn install<F>(name: &str, script_body: F) -> Self
    where
        F: FnOnce(&Path) -> String,
    {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let body = script_body(&dir);

        let script_path = dir.join(name);
        std::fs::write(&script_path, body).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let prev_path = std::env::var_os("PATH").unwrap_or_default();
        let mut new_path = OsString::from(&dir);
        new_path.push(":");
        new_path.push(&prev_path);
        // SAFETY: serial_test ensures no other thread races on PATH.
        unsafe { std::env::set_var("PATH", &new_path) };

        Self {
            tmp,
            name: name.to_string(),
            prev_path,
        }
    }

    /// Install a fake `npm` using the standard [`fake_npm_script`] template.
    pub fn install_npm() -> Self {
        Self::install("npm", fake_npm_script)
    }

    /// Install a fake `gh` using the standard [`fake_gh_script`] template.
    pub fn install_gh() -> Self {
        Self::install("gh", fake_gh_script)
    }

    /// The tempdir backing this guard (where canned stdout/stderr/exit files can be written by tests, and where `<name>-args.log` is appended).
    pub fn dir(&self) -> PathBuf {
        self.tmp.path().to_path_buf()
    }

    /// Argv lines logged by the fake script, one line per invocation.
    pub fn args_log(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir().join(format!("{}-args.log", self.name)))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }

    pub fn set_stdout(&self, content: &str) {
        std::fs::write(self.dir().join(format!("{}-stdout", self.name)), content).unwrap();
    }

    pub fn set_stderr(&self, content: &str) {
        std::fs::write(self.dir().join(format!("{}-stderr", self.name)), content).unwrap();
    }

    pub fn set_alpha_stdout(&self, content: &str) {
        std::fs::write(
            self.dir().join(format!("{}-alpha-stdout", self.name)),
            content,
        )
        .unwrap();
    }

    pub fn set_stable_only_stdout(&self, content: &str) {
        std::fs::write(
            self.dir().join(format!("{}-stable-only-stdout", self.name)),
            content,
        )
        .unwrap();
    }

    pub fn set_with_pre_stdout(&self, content: &str) {
        std::fs::write(
            self.dir().join(format!("{}-with-pre-stdout", self.name)),
            content,
        )
        .unwrap();
    }

    pub fn set_exit_code(&self, code: i32) {
        std::fs::write(
            self.dir().join(format!("{}-exit", self.name)),
            code.to_string(),
        )
        .unwrap();
    }
}

impl Drop for FakeBinGuard {
    fn drop(&mut self) {
        // SAFETY: serial_test ensures no other thread races on PATH.
        unsafe { std::env::set_var("PATH", &self.prev_path) };
    }
}

/// Single-quote a path for safe substitution into a sh script.
fn single_quote_for_sh(p: &Path) -> String {
    let s = p.to_string_lossy();
    // Escape any embedded single quotes (paranoid: tempdir paths shouldn't contain them, but defensively quote)
    let escaped = s.replace('\'', "'\\''");
    format!("'{escaped}'")
}

/// sh script body for a fake `npm`.
/// Logs argv to `<dir>/npm-args.log` and dispatches stdout based on the first matching argv pattern:
///
/// - argv contains `@alpha`     → cat `<dir>/npm-alpha-stdout`
/// - else                       → cat `<dir>/npm-stdout`
///
/// Always cats `<dir>/npm-stderr` to stderr (if exists).
/// Exits with the integer in `<dir>/npm-exit` (default 0).
pub fn fake_npm_script(dir: &Path) -> String {
    let dq = single_quote_for_sh(dir);
    format!(
        r#"#!/bin/sh
echo "$@" >> {dq}/npm-args.log
if echo "$@" | grep -q '@alpha'; then
  if [ -f {dq}/npm-alpha-stdout ]; then cat {dq}/npm-alpha-stdout; fi
elif [ -f {dq}/npm-stdout ]; then
  cat {dq}/npm-stdout
fi
if [ -f {dq}/npm-stderr ]; then cat {dq}/npm-stderr >&2; fi
exit_code=0
if [ -f {dq}/npm-exit ]; then exit_code=$(cat {dq}/npm-exit); fi
exit "$exit_code"
"#
    )
}

/// sh script body for a fake `gh`.
/// Logs argv to `<dir>/gh-args.log` and dispatches stdout based on `release list` argv:
///
/// - argv contains `release list --exclude-pre-releases` → `<dir>/gh-stable-only-stdout`
/// - argv contains `release list` (no exclude flag)      → `<dir>/gh-with-pre-stdout`
/// - else                                                 → `<dir>/gh-stdout`
///
/// Exits with `<dir>/gh-exit` (default 0).
pub fn fake_gh_script(dir: &Path) -> String {
    let dq = single_quote_for_sh(dir);
    format!(
        r#"#!/bin/sh
echo "$@" >> {dq}/gh-args.log
if echo "$@" | grep -q 'release list'; then
  if echo "$@" | grep -q '\-\-exclude-pre-releases'; then
    if [ -f {dq}/gh-stable-only-stdout ]; then cat {dq}/gh-stable-only-stdout; fi
  else
    if [ -f {dq}/gh-with-pre-stdout ]; then cat {dq}/gh-with-pre-stdout; fi
  fi
elif [ -f {dq}/gh-stdout ]; then
  cat {dq}/gh-stdout
fi
exit_code=0
if [ -f {dq}/gh-exit ]; then exit_code=$(cat {dq}/gh-exit); fi
exit "$exit_code"
"#
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// wiremock GitHub API fixtures
// ─────────────────────────────────────────────────────────────────────────────

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// RAII guard that starts a wiremock server, sets `CHAOS_GH_API_BASE` (and
/// optionally `CHAOS_GH_DOWNLOAD_BASE`) to point at it, and restores the
/// original env values on drop.
///
/// All tests using this MUST be `#[serial]` because env vars are
/// process-global.
pub struct GhApiMockGuard {
    pub server: MockServer,
    prev: Option<String>,
    prev_download: Option<String>,
}

impl GhApiMockGuard {
    /// Start a wiremock server and point `CHAOS_GH_API_BASE` at it.
    pub async fn start() -> Self {
        let server = MockServer::start().await;
        let prev = std::env::var("CHAOS_GH_API_BASE").ok();
        // SAFETY: tests using this helper must be `#[serial]`.
        unsafe { std::env::set_var("CHAOS_GH_API_BASE", server.uri()) };
        Self {
            server,
            prev,
            prev_download: None,
        }
    }

    /// Also point `CHAOS_GH_DOWNLOAD_BASE` at the same server, so download
    /// paths (asset URLs) hit the wiremock instead of GitHub. Required for
    /// tests that exercise `ensure_latest_on_disk` / `install_gh_release`.
    pub fn with_download_base(mut self) -> Self {
        self.prev_download = std::env::var("CHAOS_GH_DOWNLOAD_BASE").ok();
        // SAFETY: tests using this helper must be `#[serial]`.
        unsafe { std::env::set_var("CHAOS_GH_DOWNLOAD_BASE", self.server.uri()) };
        self
    }

    /// Mount `/repos/{repo}/releases/latest` to return a single release with
    /// the given `tag_name` and (optionally) `prerelease`/`draft` flags.
    pub async fn stub_latest(&self, tag: &str, prerelease: bool, draft: bool) {
        let body = serde_json::json!({
            "tag_name": tag,
            "prerelease": prerelease,
            "draft": draft,
            "name": tag,
            "id": 1,
        });
        Mock::given(method("GET"))
            .and(path("/repos/chao2hang/chaos-code/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&self.server)
            .await;
    }

    /// Mount `/repos/{repo}/releases?per_page=10` to return a list of releases.
    /// Each tuple is `(tag_name, prerelease, draft)`.
    pub async fn stub_list(&self, releases: &[(&str, bool, bool)]) {
        let items: Vec<serde_json::Value> = releases
            .iter()
            .enumerate()
            .map(|(i, (tag, pre, draft))| {
                serde_json::json!({
                    "tag_name": tag,
                    "prerelease": pre,
                    "draft": draft,
                    "name": tag,
                    "id": i as u32 + 1,
                })
            })
            .collect();
        Mock::given(method("GET"))
            .and(path("/repos/chao2hang/chaos-code/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_json(items))
            .mount(&self.server)
            .await;
    }

    /// Mount `/repos/{repo}/releases/latest` to return the given status code
    /// with an empty body.
    pub async fn stub_latest_status(&self, status: u16) {
        Mock::given(method("GET"))
            .and(path("/repos/chao2hang/chaos-code/releases/latest"))
            .respond_with(ResponseTemplate::new(status))
            .mount(&self.server)
            .await;
    }

    /// Return the number of requests received so far on `/releases/latest`.
    pub async fn received_latest_count(&self) -> usize {
        self.server
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|r| r.url.path() == "/repos/chao2hang/chaos-code/releases/latest")
            .count()
    }
}

impl Drop for GhApiMockGuard {
    fn drop(&mut self) {
        // SAFETY: tests using this helper must be `#[serial]`.
        unsafe {
            match &self.prev {
                Some(v) => std::env::set_var("CHAOS_GH_API_BASE", v),
                None => std::env::remove_var("CHAOS_GH_API_BASE"),
            }
            match &self.prev_download {
                Some(v) => std::env::set_var("CHAOS_GH_DOWNLOAD_BASE", v),
                None => std::env::remove_var("CHAOS_GH_DOWNLOAD_BASE"),
            }
        }
    }
}
