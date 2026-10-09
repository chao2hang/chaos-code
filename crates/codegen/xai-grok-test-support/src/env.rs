//! Binary resolution, serial env guards, and git sandbox creation.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::sandbox::TestSandbox;

// First setter wins and later sets are ignored, so parallel tests never race the process-wide choice.
static GROK_BINARY_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

pub fn set_grok_binary_override(path: PathBuf) {
    let _ = GROK_BINARY_OVERRIDE.set(path);
}

pub fn resolved_grok_binary_override() -> Option<PathBuf> {
    resolved_override()
}

/// Parse env var `key` into `T`, falling back to `default` when it is unset or present-but-unparseable (warning in the latter case).
pub fn env_parse<T: std::str::FromStr>(key: &str, default: T) -> T {
    let Ok(raw) = std::env::var(key) else {
        return default;
    };
    match raw.parse() {
        Ok(value) => value,
        Err(_) => {
            eprintln!("[test-support] ignoring unparseable {key}={raw:?}; using default");
            default
        }
    }
}

/// Process-wide lock around every environment mutation performed through this
/// module.
///
/// `std::env::set_var`/`remove_var` are `unsafe` from edition 2024 because the
/// platform environment is a single process-global buffer that `getenv` reads
/// without synchronization. Serializing writers here removes writer/writer
/// races; a test that also needs its *reads* to be stable must wrap the whole
/// read-modify-write sequence in [`with_write_lock`].
static ENV_WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

thread_local! {
    /// Nesting depth of [`with_write_lock`] on the current thread.
    static WRITE_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Run `f` while this thread holds the process-wide environment write lock.
///
/// Re-entrant on the acquiring thread — a nested [`set_var`] inside a
/// [`with_write_lock`] closure reuses the outer acquisition instead of
/// deadlocking on the non-reentrant platform mutex — but *not* across threads.
/// Use it when a test needs a multi-step read-modify-write to be atomic with
/// respect to other env-mutating tests.
///
/// Poisoning is tolerated: a panicking writer leaves the guarded environment
/// usable, and aborting every later test would hide the original failure.
pub fn with_write_lock<T>(f: impl FnOnce() -> T) -> T {
    struct Depth(usize, Option<std::sync::MutexGuard<'static, ()>>);
    impl Drop for Depth {
        fn drop(&mut self) {
            WRITE_DEPTH.with(|cell| cell.set(self.0));
        }
    }

    let depth = WRITE_DEPTH.with(std::cell::Cell::get);
    let guard = if depth == 0 {
        Some(
            ENV_WRITE_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    } else {
        None
    };
    WRITE_DEPTH.with(|cell| cell.set(depth + 1));
    let _depth = Depth(depth, guard);
    f()
}

/// Set an environment variable for the rest of the process.
///
/// Prefer [`EnvGuard`] when the value should not outlive the test.
pub fn set_var(key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) {
    with_write_lock(|| {
        // SAFETY: this thread holds the process-wide write lock, so no other
        // mutation through this module can overlap it. Concurrent *readers* are
        // still the caller's responsibility — keep env-mutating tests serial.
        unsafe { std::env::set_var(key, value) };
    });
}

/// Remove an environment variable for the rest of the process.
pub fn remove_var(key: impl AsRef<OsStr>) {
    with_write_lock(|| {
        // SAFETY: see [`set_var`].
        unsafe { std::env::remove_var(key) };
    });
}

/// Read an environment variable under the write lock, so a test can snapshot a
/// value it is about to restore without racing another writer.
pub fn var_os(key: impl AsRef<OsStr>) -> Option<OsString> {
    with_write_lock(|| std::env::var_os(key))
}

/// RAII guard for a single environment variable in `#[serial]` tests.
/// It snapshots the prior value, applies the change, and restores the prior value (or unsets it) on drop, even if an assertion panics.
/// Restoring rather than always unsetting avoids clobbering vars a parent process/harness set (e.g. `RUST_LOG`).
///
/// Each individual write goes through [`with_write_lock`]; the lock is
/// deliberately *not* held for the guard's lifetime, because tests routinely
/// stack several guards. Wrap the test body in [`with_write_lock`] (or use
/// `#[serial_test::serial]`) when the value must stay put across steps.
/// RAII guard for a single environment variable in `#[serial]` tests. Restoring rather than always unsetting avoids
/// clobbering vars a parent process/harness set (e.g. `RUST_LOG`). Callers MUST be `#[serial_test::serial]`. The `unsafe`
/// `set_var`/`remove_var` are sound only when no other thread accesses the environment concurrently.
pub struct EnvGuard {
    key: std::borrow::Cow<'static, str>,
    prior: Option<OsString>,
}

impl EnvGuard {
    /// Set `key` to `value` for the guard's lifetime.
    pub fn set(key: impl Into<std::borrow::Cow<'static, str>>, value: impl AsRef<OsStr>) -> Self {
        let key = key.into();
        let prior = var_os(key.as_ref());
        set_var(key.as_ref(), value);
        Self { key, prior }
    }

    /// Unset `key` for the guard's lifetime.
    pub fn unset(key: impl Into<std::borrow::Cow<'static, str>>) -> Self {
        let key = key.into();
        let prior = var_os(key.as_ref());
        remove_var(key.as_ref());
        Self { key, prior }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match self.prior.take() {
            Some(v) => set_var(&*self.key, v),
            None => remove_var(&*self.key),
        }
    }
}

/// Point every `GROK_*` path and telemetry switch at `home` and drop inherited
/// credentials/proxies, so a test cannot touch a developer's real config.
///
/// # Safety
/// No other thread may access the environment concurrently; call before any other thread exists.
pub unsafe fn isolate_grok_env(home: &Path) {
    // Every write still goes through the module's write lock, so this cannot
    // race another helper-based mutation; the caller's "single-threaded"
    // contract is what keeps the *reads* elsewhere in the process sound.
    set_var("GROK_HOME", home);
    set_var("GROK_TELEMETRY_ENABLED", "false");
    set_var("GROK_TELEMETRY_MIXPANEL_ENABLED", "false");
    set_var("GROK_TELEMETRY_MIXPANEL_TOKEN", "");
    set_var("GROK_TELEMETRY_EVENTS_URL", "");
    set_var("GROK_TELEMETRY_EVENTS_API_KEY", "");
    set_var("GROK_FEEDBACK_ENABLED", "false");
    set_var("GROK_TRACE_UPLOAD", "false");
    for var in [
        "GROK_AUTH",
        "GROK_AUTH_PATH",
        "GROK_DEPLOYMENT_KEY",
        "GROK_MANAGED_CONFIG",
        "GROK_CONFIG",
        "GROK_CONFIG_PATH",
        "GROK_CLI_CHAT_PROXY_BASE_URL",
        "GROK_MODELS_BASE_URL",
        "GROK_MODELS_LIST_URL",
        "XAI_API_KEY",
        "GROK_API_KEY",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        remove_var(var);
    }
}

fn workspace_root() -> PathBuf {
    // nth(3): crate is nested three levels below the cargo workspace root.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("workspace root")
        .to_path_buf()
}

fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"))
}

/// Resolve a workspace binary: the prebuilt `target/debug/<bin>`, else `cargo build -p <package>
/// --bin <bin>` with stdin closed, `pager_env()` applied, and the child detached from the TTY.
pub fn ensure_cargo_bin(package: &str, bin: &str) -> PathBuf {
    let binary = target_dir()
        .join("debug")
        .join(format!("{bin}{}", std::env::consts::EXE_SUFFIX));
    if binary.exists() {
        return binary;
    }

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut cmd = Command::new(&cargo);
    cmd.current_dir(workspace_root())
        .args(["build", "-p", package, "--bin", bin])
        .stdin(std::process::Stdio::null())
        .envs(xai_tty_utils::pager_env());
    xai_tty_utils::detach_std_command(&mut cmd);
    let output = cmd
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn {cargo} to build {bin}: {e}"));

    assert!(
        output.status.success(),
        "failed to build {bin} (exit {:?})\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        binary.exists(),
        "{bin} build completed but binary missing at {}",
        binary.display()
    );
    binary
}

// Reserve four cores for the pager children other test threads spawn, so a cold build does not starve them.
fn build_jobs() -> usize {
    let cores = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1);
    cores.saturating_sub(4).max(1)
}

pub fn ensure_cargo_bin_with_features(
    package: &str,
    bin: &str,
    features: &[&str],
    target_subdir: &str,
) -> PathBuf {
    let out_target = target_dir().join(target_subdir);
    let binary = out_target
        .join("debug")
        .join(format!("{bin}{}", std::env::consts::EXE_SUFFIX));
    if binary.exists() {
        return binary;
    }

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut cmd = Command::new(&cargo);
    cmd.current_dir(workspace_root())
        .args(["build", "-p", package, "--bin", bin])
        .args(["--features", &features.join(",")])
        .args(["--jobs", &build_jobs().to_string()])
        .env("CARGO_TARGET_DIR", &out_target)
        .stdin(std::process::Stdio::null())
        .envs(xai_tty_utils::pager_env());
    xai_tty_utils::detach_std_command(&mut cmd);
    let output = cmd
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn {cargo} to build {bin}: {e}"));

    assert!(
        output.status.success(),
        "failed to build {bin} with features {features:?} (exit {:?})\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        binary.exists(),
        "{bin} build completed but binary missing at {}",
        binary.display()
    );
    binary
}

pub fn grok_binary() -> PathBuf {
    if let Some(path) = resolved_override() {
        return path;
    }
    if let Some(path) = env_binary("GROK_BINARY") {
        return path;
    }

    // `cargo test` 在 Chaos 分支下设置的是 `CARGO_BIN_EXE_chaos`；保留旧名作为
    // Bazel/上游兼容回退。
    for key in ["CARGO_BIN_EXE_chaos", "CARGO_BIN_EXE_xai-grok-pager"] {
        if let Ok(path) = std::env::var(key) {
            let p = PathBuf::from(path);
            if p.exists() {
                return p;
            }
        }
    }

    // Chaos 分支把 `xai-grok-pager-bin` 的 bin target 改名为 `chaos`
    // （见该 crate 的 `[[bin]]`），这里必须跟着改，否则 lifecycle 测试
    // 永远找不到（也永远建不出）那个二进制。
    ensure_cargo_bin("xai-grok-pager-bin", "chaos")
}

fn resolved_override() -> Option<PathBuf> {
    GROK_BINARY_OVERRIDE.get().cloned()
}

pub fn env_binary(key: &str) -> Option<PathBuf> {
    let path = std::env::var(key).ok().filter(|path| !path.is_empty())?;
    let p = PathBuf::from(path);
    assert!(p.exists(), "{key} does not exist: {}", p.display());
    // Bazel leaves this path runfiles-relative, and the child cwd is not the runfiles root.
    Some(std::path::absolute(&p).unwrap_or(p))
}

/// Off `target/debug`, so this build cannot overwrite [`grok_binary`].
fn shipped_pager_target_dir() -> PathBuf {
    target_dir().join("shipped-pager")
}

fn feature_stamp(features: &[&str]) -> String {
    format!("no-default:{}", features.join(","))
}

fn stamp_matches(stamp: &Path, wanted: &str) -> bool {
    std::fs::read_to_string(stamp)
        .ok()
        .is_some_and(|have| have == wanted)
}

fn reuse_shipped_binary(binary: &Path, stamp: &Path, wanted: &str) -> bool {
    binary.exists() && stamp_matches(stamp, wanted)
}

/// The feature stamp rejects a mismatched binary; cargo still runs.
pub fn ensure_default_target_with_features(package: &str, bin: &str, features: &[&str]) -> PathBuf {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    build_shipped_pager(
        package,
        bin,
        features,
        &shipped_pager_target_dir(),
        &workspace_root(),
        &cargo,
    )
}

fn build_shipped_pager(
    package: &str,
    bin: &str,
    features: &[&str],
    out_target: &Path,
    workspace: &Path,
    cargo: &str,
) -> PathBuf {
    let binary = out_target
        .join("debug")
        .join(format!("{bin}{}", std::env::consts::EXE_SUFFIX));
    let stamp = binary.with_extension("features");
    let wanted = feature_stamp(features);
    if binary.exists() && !reuse_shipped_binary(&binary, &stamp, &wanted) {
        std::fs::remove_file(&binary).unwrap_or_else(|e| {
            panic!(
                "failed to reject {bin} with a mismatched feature stamp at {}: {e}",
                binary.display()
            )
        });
    }

    let mut cmd = Command::new(cargo);
    cmd.current_dir(workspace)
        .args(["build", "-p", package, "--bin", bin])
        .args(["--no-default-features", "--features"])
        .arg(features.join(","))
        .args(["--jobs", &build_jobs().to_string()])
        .env("CARGO_TARGET_DIR", out_target)
        .stdin(std::process::Stdio::null())
        .envs(xai_tty_utils::pager_env());
    xai_tty_utils::detach_std_command(&mut cmd);
    let output = cmd
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn {cargo} to build {bin}: {e}"));
    assert!(
        output.status.success(),
        "failed to build {bin} with features {features:?} (exit {:?})\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        binary.exists(),
        "{bin} build completed but binary missing at {}",
        binary.display()
    );
    std::fs::write(&stamp, &wanted).unwrap_or_else(|e| {
        panic!(
            "failed to record features for {bin} at {}: {e}",
            stamp.display()
        )
    });
    binary
}

pub fn git_workdir() -> TestSandbox {
    TestSandbox::builder().git().build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Each test needs its own key: `cargo test` runs these in parallel threads
    /// inside one process, so a shared key would make them race each other
    /// instead of the code under test.
    fn probe(who: &str) -> std::borrow::Cow<'static, str> {
        format!("XAI_TEST_SUPPORT_ENV_PROBE_{who}").into()
    }

    #[test]
    fn set_and_remove_var_reach_the_real_process_environment() {
        let key = probe("basic");
        remove_var(key.as_ref());
        assert_eq!(std::env::var_os(key.as_ref()), None);

        set_var(key.as_ref(), "written");
        // Read through std directly: the helper must not shadow or buffer the
        // value, it has to land in the process env that child processes see.
        assert_eq!(std::env::var(key.as_ref()).as_deref(), Ok("written"));

        remove_var(key.as_ref());
        assert_eq!(std::env::var_os(key.as_ref()), None);
    }

    #[test]
    fn env_guard_restores_a_previous_value_and_unsets_a_new_key() {
        let key = probe("guard");
        remove_var(key.as_ref());
        {
            let _fresh = EnvGuard::set(key.clone(), "scoped");
            assert_eq!(std::env::var(key.as_ref()).as_deref(), Ok("scoped"));
        }
        assert_eq!(
            std::env::var_os(key.as_ref()),
            None,
            "a key the guard created must be unset"
        );

        set_var(key.as_ref(), "pre-existing");
        {
            let _restore = EnvGuard::set(key.clone(), "scoped");
            assert_eq!(std::env::var(key.as_ref()).as_deref(), Ok("scoped"));
        }
        assert_eq!(
            std::env::var(key.as_ref()).as_deref(),
            Ok("pre-existing"),
            "a pre-existing value must survive the guard"
        );
        remove_var(key.as_ref());
    }

    #[test]
    fn env_guard_restores_even_when_the_test_panics() {
        let key = probe("panic");
        set_var(key.as_ref(), "pre-existing");
        let key_in_closure = key.clone();
        let result = std::panic::catch_unwind(move || {
            let _guard = EnvGuard::set(key_in_closure, "scoped");
            panic!("guard must unwind-clean");
        });
        assert!(result.is_err());
        assert_eq!(
            std::env::var(key.as_ref()).as_deref(),
            Ok("pre-existing"),
            "the guard has to restore across a panic"
        );
        remove_var(key.as_ref());
    }

    #[test]
    fn write_lock_is_reentrant_so_nested_helpers_do_not_deadlock() {
        let key = probe("reentrant");
        remove_var(key.as_ref());
        with_write_lock(|| {
            set_var(key.as_ref(), "batched");
            remove_var(key.as_ref());
            with_write_lock(|| set_var(key.as_ref(), "nested"));
            assert_eq!(std::env::var(key.as_ref()).as_deref(), Ok("nested"));
        });
        remove_var(key.as_ref());
    }

    #[test]
    fn concurrent_writers_do_not_lose_or_cross_contaminate_values() {
        // The point of the process-wide write lock: many threads may each write
        // and read back their own key without a torn or overwritten result.
        let (writers, rounds) = (8, 200);
        let keys: Vec<String> = (0..writers)
            .map(|i| probe(&format!("many{i}")).into_owned())
            .collect();
        for key in &keys {
            remove_var(key);
        }

        std::thread::scope(|scope| {
            for (index, key) in keys.iter().enumerate() {
                let key = key.clone();
                scope.spawn(move || {
                    for round in 0..rounds {
                        let value = format!("writer{index}-{round}");
                        set_var(&key, &value);
                        assert_eq!(var_os(&key).as_deref(), Some(std::ffi::OsStr::new(&value)));
                        remove_var(&key);
                        assert_eq!(var_os(&key), None);
                    }
                });
            }
        });

        for key in &keys {
            assert_eq!(var_os(key), None, "{key} leaked past the writers");
        }
    }

    #[test]
    fn batched_writes_are_atomic_against_a_reader() {
        // Two keys a test must flip together. A reader that takes the lock once
        // must never see one flipped and its partner still stale.
        let (a, b) = ("XAI_TEST_SUPPORT_ENV_PAIR_A", "XAI_TEST_SUPPORT_ENV_PAIR_B");
        remove_var(a);
        remove_var(b);
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed_torn = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let (reader_stop, reader_saw_torn) = (stop.clone(), observed_torn.clone());
        std::thread::scope(|scope| {
            scope.spawn(move || {
                while !reader_stop.load(std::sync::atomic::Ordering::Relaxed) {
                    // Both reads under a single acquisition; reading each under
                    // its own lock would let the writer slip in between.
                    let (va, vb) = with_write_lock(|| (var_os(a), var_os(b)));
                    if va != vb {
                        reader_saw_torn.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    std::thread::yield_now();
                }
            });
            for _ in 0..500 {
                with_write_lock(|| {
                    set_var(a, "same");
                    set_var(b, "same");
                });
                with_write_lock(|| {
                    remove_var(a);
                    remove_var(b);
                });
            }
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        });

        assert!(
            !observed_torn.load(std::sync::atomic::Ordering::Relaxed),
            "torn batch"
        );
        remove_var(a);
        remove_var(b);
    }

    use super::{
        build_shipped_pager, feature_stamp, reuse_shipped_binary, shipped_pager_target_dir,
        target_dir,
    };

    #[test]
    fn shipped_pager_binary_is_not_the_grok_binary_path() {
        let name = format!("xai-grok-pager{}", std::env::consts::EXE_SUFFIX);
        let grok = target_dir().join("debug").join(&name);
        let shipped = shipped_pager_target_dir().join("debug").join(&name);
        assert_ne!(grok, shipped);
    }

    #[test]
    fn feature_stamp_mismatch_rejects_the_shipped_binary() {
        let dir = std::env::temp_dir().join("shipped-pager-stamp-mismatch");
        let binary = dir.join(format!("xai-grok-pager{}", std::env::consts::EXE_SUFFIX));
        let stamp = binary.with_extension("features");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&binary, b"shipped").unwrap();
        let wanted = feature_stamp(&["jemalloc", "chat"]);
        std::fs::write(&stamp, feature_stamp(&["jemalloc"])).unwrap();
        assert!(
            !reuse_shipped_binary(&binary, &stamp, &wanted),
            "a mismatched feature stamp must not reuse the binary"
        );
        std::fs::write(&stamp, &wanted).unwrap();
        assert!(
            reuse_shipped_binary(&binary, &stamp, &wanted),
            "a matching feature stamp may reuse the binary"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn matching_feature_stamp_still_asks_cargo() {
        let dir = std::env::temp_dir().join(format!("shipped-pager-cargo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let out_target = dir.join("out");
        let debug = out_target.join("debug");
        std::fs::create_dir_all(&debug).unwrap();
        let binary = debug.join(format!("xai-grok-pager{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&binary, b"stale").unwrap();
        let features = ["jemalloc", "chat"];
        std::fs::write(binary.with_extension("features"), feature_stamp(&features)).unwrap();
        let marker = dir.join("invoked");
        let cargo = dir.join("cargo");
        std::fs::write(
            &cargo,
            format!("#!/bin/sh\nprintf yes > '{}'\n", marker.display()),
        )
        .unwrap();
        let mut perms = std::fs::metadata(&cargo).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(&cargo, perms).unwrap();
        let got = build_shipped_pager(
            "xai-grok-pager-bin",
            "xai-grok-pager",
            &features,
            &out_target,
            &dir,
            cargo.to_str().unwrap(),
        );
        assert_eq!(got, binary);
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap_or_default(),
            "yes",
            "matching feature stamp must not return the cached binary without cargo"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
