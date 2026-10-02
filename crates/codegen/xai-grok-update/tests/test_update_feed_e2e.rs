//! End-to-end tests for `chaos update` against a self-hosted release feed.
//!
//! Everything here drives [`xai_grok_update::auto_update::run_update`], the
//! function the `update` subcommand calls, so a change that breaks the shipped
//! command fails here rather than only in a unit test of one of its helpers.
//!
//! The feed is two loopback servers:
//!
//! - wiremock answers the GitHub Releases API (`CHAOS_GH_API_BASE`)
//! - [`ArtifactServer`] serves the asset bytes (`CHAOS_GH_DOWNLOAD_BASE`)
//!
//! The asset bytes need their own server because the interesting cases are
//! transport failures (a body that stops early), and wiremock buffers a whole
//! response before sending it, so it cannot cut one off mid-body.
//!
//! The scenarios are the ones a self-update has to get right on a real
//! machine: the normal upgrade keeps the user's own state, a feed that cannot
//! be verified is refused *before* its bytes are run, an interrupted download
//! or an artifact that cannot start leaves the previous build serving, and the
//! post-install sweep keeps exactly the rollback build it needs while clearing
//! older and previously-branded leftovers.

#![cfg(unix)]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serial_test::serial;

use common::artifact_server::{ArtifactServer, Mode};
use common::{
    GhApiMockGuard, can_exec_shell_scripts, host_platform, make_update_config, reset_home,
    set_test_version, test_home,
};
use xai_grok_update::auto_update::{CliUpdateTrigger, run_update};
use xai_grok_update::version::installed_on_disk_version;

const RUNNING: &str = "0.2.5";
const TARGET: &str = "0.2.7";

/// Restores an env var on drop. `reset_home` clears the vars the shared
/// fixtures know about; these are set per-scenario and are process-global.
struct EnvGuard {
    key: &'static str,
    prev: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let prev = std::env::var(key).ok();
        // SAFETY: every test using this is `#[serial]`.
        unsafe { std::env::set_var(key, value) };
        Self { key, prev }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: `#[serial]` keeps other tests off the environment here.
        unsafe {
            match &self.prev {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

fn sh_quote(p: &Path) -> String {
    format!("'{}'", p.to_string_lossy().replace('\'', "'\\''"))
}

/// An artifact that reports every execution by appending to `marker` and then
/// exits with `exit_code`.
///
/// This is what separates "the bytes were downloaded" from "the bytes were
/// run": a refused artifact must leave `marker` absent, while an artifact that
/// reached the smoke test must leave it present. The marker also makes each
/// build byte-distinct, so "the old build is still the active one" is a content
/// check rather than a name check.
fn tripwire_artifact(marker: &Path, exit_code: i32) -> Vec<u8> {
    format!(
        "#!/bin/sh\necho ran >> {}\nexit {exit_code}\n",
        sh_quote(marker)
    )
    .into_bytes()
}

/// A whole artifact plus the length of a prefix that is, on its own, a working
/// script.
///
/// Serving only that prefix is the interruption case worth testing: the bytes
/// that did arrive would run and exit 0, so nothing but the byte count can tell
/// the update is incomplete. Truncating somewhere that leaves a broken script
/// would be caught by the smoke test alone, and the test would keep passing
/// after the download checks were removed.
fn artifact_with_runnable_prefix(marker: &Path) -> (Vec<u8>, usize) {
    let head = tripwire_artifact(marker, 0);
    let mut artifact = head.clone();
    artifact.extend_from_slice(&b"#".repeat(8 * 1024));
    (artifact, head.len())
}

/// How many times the artifact at `marker` reported an execution.
fn exec_count(marker: &Path) -> usize {
    std::fs::read_to_string(marker)
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

/// A marker path, with any file a previous test in this binary left there
/// removed. `test_home` is shared across the whole binary and `reset_home`
/// only clears the files the update path itself writes.
fn fresh_marker(name: &str) -> PathBuf {
    let path = test_home().join(name);
    let _ = std::fs::remove_file(&path);
    path
}

fn versioned_name(version: &str) -> String {
    format!("chaos-{version}-{}", host_platform())
}

fn staged_artifact(version: &str) -> PathBuf {
    test_home().join("downloads").join(versioned_name(version))
}

/// Lay down the build the updater is upgrading *from*: a versioned binary in
/// `downloads/` with `bin/chaos` pointing at it, which is the layout
/// `install_gh_release` leaves behind.
fn seed_active_build(version: &str, content: &[u8]) {
    let home = test_home().clone();
    let downloads = home.join("downloads");
    let bin = home.join("bin");
    std::fs::create_dir_all(&downloads).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    let path = downloads.join(versioned_name(version));
    std::fs::write(&path, content).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::os::unix::fs::symlink(
        std::path::Path::new("../downloads").join(versioned_name(version)),
        bin.join("chaos"),
    )
    .unwrap();
}

/// The version `bin/chaos` currently activates plus the bytes it resolves to.
///
/// Reading the bytes is the point: a symlink can point at a plausible name and
/// still hold a half-written file.
fn active_build() -> Option<(String, Vec<u8>)> {
    let link = test_home().join("bin").join("chaos");
    if !link.is_symlink() {
        return None;
    }
    let resolved = dunce::canonicalize(&link).ok()?;
    let name = resolved.file_name()?.to_string_lossy().to_string();
    let version = name
        .strip_prefix("chaos-")?
        .strip_suffix(&format!("-{}", host_platform()))?
        .to_string();
    Some((version, std::fs::read(&resolved).ok()?))
}

/// The build that was serving before the update is still the one `chaos`
/// resolves to, byte for byte.
fn assert_previous_build_still_serves(marker: &Path) {
    let (version, bytes) = active_build().expect("a build must stay active");
    assert_eq!(
        version, RUNNING,
        "a refused update must leave the running build active"
    );
    assert_eq!(
        bytes,
        tripwire_artifact(marker, 0),
        "the active build must still be the exact bytes it had before"
    );
    assert_eq!(
        installed_on_disk_version().as_deref(),
        Some(RUNNING),
        "the disk probe must keep reporting the old build, or the next check \
         would treat a refused update as installed"
    );
}

/// A release feed the shipped updater can be pointed at, held open for as long
/// as the test needs it.
///
/// Both servers are owned here rather than returned separately: the API guard
/// sets `CHAOS_GH_API_BASE` and clears it again on drop, so a helper that
/// dropped it mid-test would silently send the updater to the real GitHub
/// release feed — where it would download a real binary and "succeed" for the
/// wrong reason.
struct Feed {
    _api: GhApiMockGuard,
    artifacts: ArtifactServer,
}

impl Feed {
    fn set_mode(&self, mode: Mode) {
        self.artifacts.set_mode(mode);
    }

    /// Body-serving GETs so far; HEAD probes are not counted.
    fn request_count(&self) -> usize {
        self.artifacts.request_count()
    }
}

/// Fails loudly if the updater is not pointed at loopback, which is the
/// precondition every test here assumes.
fn assert_feed_is_loopback() {
    for key in ["CHAOS_GH_API_BASE", "CHAOS_GH_DOWNLOAD_BASE"] {
        let base = std::env::var(key)
            .unwrap_or_else(|_| panic!("{key} is unset: the updater would reach the real feed"));
        let host = base
            .split("://")
            .nth(1)
            .unwrap_or(&base)
            .split('/')
            .next()
            .unwrap_or_default()
            .to_string();
        let host = host.rsplit('@').next().unwrap_or(&host).replace(":80", "");
        assert!(
            host.starts_with("127.0.0.1") || host.starts_with("[::1]") || host == "localhost",
            "{key} points at {base}, not a loopback feed"
        );
    }
}

/// Points the shipped updater at a loopback feed offering `TARGET`, whose asset
/// is `artifact`, and reports the running version as `RUNNING`.
async fn serve_release(artifact: Vec<u8>) -> Feed {
    let _ = test_home();
    reset_home();
    set_test_version(RUNNING);
    // SAFETY: `#[serial]` tests only; `reset_home` clears this between tests.
    unsafe { std::env::set_var("GROK_INSTALLER", "gh-release") };

    let artifacts = ArtifactServer::start(artifact);
    let api = GhApiMockGuard::start()
        .await
        .with_download_base_at(&artifacts.uri());
    api.stub_latest(&format!("v{TARGET}"), false, false).await;
    assert_feed_is_loopback();
    Feed {
        _api: api,
        artifacts,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 正常升级: the feed artifact becomes the active build and the user's own state
// (config, MCP server definitions, session data, the previous build) survives it
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn an_upgrade_activates_the_feed_artifact_and_keeps_user_state() {
    if !can_exec_shell_scripts() {
        eprintln!("skipping: shell scripts cannot execute in this sandbox");
        return;
    }
    let home = test_home().clone();
    let new_build = fresh_marker("new-build.exec");
    let prev_build = fresh_marker("prev-build.exec");
    let artifact = tripwire_artifact(&new_build, 0);
    let feed = serve_release(artifact.clone()).await;
    seed_active_build(RUNNING, &tripwire_artifact(&prev_build, 0));

    // State the updater has no business touching. `[mcp_servers.*]` is not a
    // section the updater's config write merges, so it survives only if the
    // write really is a read-modify-write of the user's file.
    std::fs::write(
        home.join("config.toml"),
        "[cli]\nshow_tips = false\n\n[mcp_servers.docs]\ncommand = \"docs-mcp --stdio\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(home.join("sessions")).unwrap();
    let notes = home.join("sessions").join("notes.json");
    std::fs::write(&notes, b"[{\"id\":1}]").unwrap();

    let mut cfg = make_update_config("stable");
    let result = run_update(false, None, None, &mut cfg, CliUpdateTrigger::UserCommand)
        .await
        .expect("the update must succeed");
    assert_eq!(
        result.as_deref(),
        Some(TARGET),
        "the update must report the version it activated"
    );

    assert_eq!(feed.request_count(), 1, "one upgrade is one asset download");
    let (version, bytes) = active_build().expect("a build must be active");
    assert_eq!(version, TARGET, "the feed artifact must be the active one");
    assert_eq!(bytes, artifact, "the active bytes must be the served ones");
    assert!(
        exec_count(&new_build) >= 1,
        "the smoke test must have run the artifact it installed"
    );
    assert_eq!(
        exec_count(&prev_build),
        0,
        "an upgrade must not execute the build it is replacing"
    );

    let config = std::fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(
        config.contains("show_tips = false"),
        "a setting the updater does not own must survive: {config}"
    );
    assert!(
        config.contains("[mcp_servers.docs]"),
        "the user's MCP server definitions must survive: {config}"
    );
    assert!(
        config.contains("gh-release"),
        "the installer is persisted so later runs pick the same path: {config}"
    );
    assert_eq!(
        std::fs::read_to_string(&notes).unwrap(),
        "[{\"id\":1}]",
        "session data must survive the update byte-for-byte"
    );

    let cache: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(home.join("version.json")).unwrap())
            .expect("version.json must be readable JSON");
    assert_eq!(
        cache["version"].as_str(),
        Some(TARGET),
        "the version cache must name the version that is now active"
    );
    assert_eq!(installed_on_disk_version().as_deref(), Some(TARGET));
    assert!(
        staged_artifact(RUNNING).exists(),
        "the previous build stays on disk as the rollback target"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 签名失败: a feed whose artifact cannot be verified is refused before its bytes
// are exec'd, and the previous build keeps serving
//
// `CHAOS_SIGNING_PUBLIC_KEY` is baked in at compile time, so a test build
// carries the all-zero placeholder and the refusal reason is "key not
// configured" rather than "signature mismatch". Both are the same gate firing
// in `verify_downloaded_artifact` — with a key configured the same call refuses
// on a missing or mismatched `.sig` — and the mismatch arithmetic is covered in
// `signature::tests`. What this test pins is the ordering that matters on a
// real machine: refuse first, never exec, previous build untouched.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn an_unverifiable_feed_is_refused_before_its_artifact_runs() {
    if !can_exec_shell_scripts() {
        eprintln!("skipping: shell scripts cannot execute in this sandbox");
        return;
    }
    let _require_sig = EnvGuard::set("CHAOS_REQUIRE_SIG", "1");
    let home = test_home().clone();
    let new_build = fresh_marker("new-build.exec");
    let prev_build = fresh_marker("prev-build.exec");
    // Exits 0: were the gate skipped, this update would have "succeeded".
    let artifact = tripwire_artifact(&new_build, 0);
    let feed = serve_release(artifact.clone()).await;
    seed_active_build(RUNNING, &tripwire_artifact(&prev_build, 0));

    let mut cfg = make_update_config("stable");
    let err = run_update(false, None, None, &mut cfg, CliUpdateTrigger::UserCommand)
        .await
        .expect_err("an artifact that cannot be verified must be refused");
    let msg = err.to_string();
    assert!(
        msg.contains("CHAOS_SIGNING_PUBLIC_KEY"),
        "the refusal must name the verification gate, not fail silently: {msg}"
    );

    assert!(
        !new_build.exists(),
        "the refused artifact was exec'd even though verification failed: {:?}",
        std::fs::read_dir(home.join("downloads"))
    );
    assert_eq!(exec_count(&prev_build), 0);
    assert_previous_build_still_serves(&prev_build);
    assert!(
        feed.request_count() >= 1,
        "verification happens after the download, so the bytes were fetched \
         and still never ran"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 下载中断: a body that stops early is never published to the versioned path
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn an_interrupted_download_never_becomes_the_active_build() {
    if !can_exec_shell_scripts() {
        eprintln!("skipping: shell scripts cannot execute in this sandbox");
        return;
    }
    let new_build = fresh_marker("new-build.exec");
    let prev_build = fresh_marker("prev-build.exec");
    let (artifact, runnable_prefix) = artifact_with_runnable_prefix(&new_build);
    let feed = serve_release(artifact.clone()).await;
    seed_active_build(RUNNING, &tripwire_artifact(&prev_build, 0));
    // Cut at a point where what arrived is already a runnable script, so the
    // only thing that can reject this is the byte count.
    feed.set_mode(Mode::Truncate(runnable_prefix));

    let mut cfg = make_update_config("stable");
    run_update(false, None, None, &mut cfg, CliUpdateTrigger::UserCommand)
        .await
        .expect_err("a truncated artifact must fail the update");

    // The failure wording comes from the HTTP stack and varies with how the
    // socket died, so the assertion is on the on-disk outcome instead.
    assert!(
        feed.request_count() >= 1,
        "the download must actually have been attempted"
    );
    assert!(
        !staged_artifact(TARGET).exists(),
        "a partial body must never be published to the versioned path"
    );
    assert!(!new_build.exists(), "a partial body must never be exec'd");
    assert_previous_build_still_serves(&prev_build);
}

// ─────────────────────────────────────────────────────────────────────────────
// 回滚: an artifact that cannot start is deleted and the previous build stays
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn an_artifact_that_cannot_start_is_discarded_and_the_old_build_stays() {
    if !can_exec_shell_scripts() {
        eprintln!("skipping: shell scripts cannot execute in this sandbox");
        return;
    }
    let new_build = fresh_marker("new-build.exec");
    let prev_build = fresh_marker("prev-build.exec");
    // Complete bytes that exit non-zero on `--version`.
    let artifact = tripwire_artifact(&new_build, 1);
    let feed = serve_release(artifact.clone()).await;
    seed_active_build(RUNNING, &tripwire_artifact(&prev_build, 0));

    let mut cfg = make_update_config("stable");
    let err = run_update(false, None, None, &mut cfg, CliUpdateTrigger::UserCommand)
        .await
        .expect_err("an artifact that fails --version must not be activated");
    let msg = err.to_string();
    assert!(
        msg.contains("downloaded binary failed to run"),
        "the error must say the smoke test is what rejected it: {msg}"
    );
    assert!(
        msg.contains("Your current version is unchanged"),
        "and must tell the user their install is intact: {msg}"
    );

    assert!(
        exec_count(&new_build) >= 1,
        "the smoke test ran the artifact, which is how it caught it"
    );
    assert!(
        !staged_artifact(TARGET).exists(),
        "the rejected artifact must be deleted, not left for the next run to \
         mistake for an install"
    );
    assert_previous_build_still_serves(&prev_build);
    assert_eq!(feed.request_count(), 1, "one attempt, one download");
}

/// A retried update after the feed recovers needs neither `--force` nor a
/// cleaned-up home: the failed attempt left nothing behind that blocks it.
#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn a_retried_update_succeeds_once_the_feed_recovers() {
    if !can_exec_shell_scripts() {
        eprintln!("skipping: shell scripts cannot execute in this sandbox");
        return;
    }
    let new_build = fresh_marker("new-build.exec");
    let prev_build = fresh_marker("prev-build.exec");
    let (artifact, runnable_prefix) = artifact_with_runnable_prefix(&new_build);
    let feed = serve_release(artifact.clone()).await;
    seed_active_build(RUNNING, &tripwire_artifact(&prev_build, 0));
    let mut cfg = make_update_config("stable");

    feed.set_mode(Mode::Truncate(runnable_prefix));
    run_update(false, None, None, &mut cfg, CliUpdateTrigger::UserCommand)
        .await
        .expect_err("the broken feed must fail the update");
    assert_eq!(active_build().expect("an active build").0, RUNNING);

    // Nothing was reset and `--force` was not passed: the next check sees the
    // same target and this time the bytes arrive whole.
    feed.set_mode(Mode::Full);
    let result = run_update(false, None, None, &mut cfg, CliUpdateTrigger::UserCommand)
        .await
        .expect("the retry must succeed");
    assert_eq!(
        result.as_deref(),
        Some(TARGET),
        "the retry must complete the upgrade"
    );
    let (version, bytes) = active_build().expect("an active build");
    assert_eq!(version, TARGET);
    assert_eq!(bytes, artifact);
    assert!(
        staged_artifact(RUNNING).exists(),
        "the build that kept serving is still there to roll back to"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 旧数据: the post-install sweep keeps the rollback build and clears older
// binaries — including ones from before the rename — plus stale temp files,
// without touching a temp file another updater may still be writing
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn the_post_install_sweep_keeps_rollback_and_clears_old_and_legacy_files() {
    if !can_exec_shell_scripts() {
        eprintln!("skipping: shell scripts cannot execute in this sandbox");
        return;
    }
    let home = test_home().clone();
    let new_build = fresh_marker("new-build.exec");
    let prev_build = fresh_marker("prev-build.exec");
    let feed = serve_release(tripwire_artifact(&new_build, 0)).await;
    seed_active_build(RUNNING, &tripwire_artifact(&prev_build, 0));

    let downloads = home.join("downloads");
    let platform = host_platform();
    let leftovers = [
        // Two releases back: past the retention window, must go.
        format!("chaos-0.2.3-{platform}"),
        // Binaries under the pre-rename brand name: the newest of these is
        // kept as the previous version, the older one goes.
        format!("grok-0.2.4-{platform}"),
        format!("grok-0.2.2-{platform}"),
        // A temp file left by an update that died a while ago.
        "chaos-0.1.9-x.tmp".to_string(),
    ];
    for name in &leftovers {
        std::fs::write(downloads.join(name), b"#!/bin/sh\nexit 0\n").unwrap();
    }
    // The sweep only deletes what looks old, so age the fixtures; a binary
    // written moments ago may belong to a concurrent updater.
    common::backdate_downloads();
    // …and this one is deliberately fresh: it must survive.
    let in_flight = downloads.join(format!("chaos-{TARGET}-other.tmp"));
    std::fs::write(&in_flight, b"#!/bin/sh\nexit 0\n").unwrap();

    let mut cfg = make_update_config("stable");
    let result = run_update(false, None, None, &mut cfg, CliUpdateTrigger::UserCommand)
        .await
        .expect("the update must succeed");
    assert_eq!(result.as_deref(), Some(TARGET));

    assert!(
        staged_artifact(RUNNING).exists(),
        "the previous build must stay for a rollback"
    );
    assert!(
        !downloads.join(format!("chaos-0.2.3-{platform}")).exists(),
        "a binary two releases back must be swept"
    );
    assert!(
        downloads.join(format!("grok-0.2.4-{platform}")).exists(),
        "the newest pre-rename binary is the previous version and stays"
    );
    assert!(
        !downloads.join(format!("grok-0.2.2-{platform}")).exists(),
        "older pre-rename binaries must be swept"
    );
    assert!(
        !downloads.join("chaos-0.1.9-x.tmp").exists(),
        "a stale temp file must be swept"
    );
    assert!(
        in_flight.exists(),
        "a temp file written moments ago may be another updater's in-flight \
         download and must survive"
    );
    let (version, _) = active_build().expect("an active build");
    assert_eq!(version, TARGET);
    assert_eq!(
        feed.request_count(),
        1,
        "the sweep runs on the way to an install, not by re-downloading"
    );
}
