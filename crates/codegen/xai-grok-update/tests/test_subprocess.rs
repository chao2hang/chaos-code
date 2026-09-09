//! `auto_update::install_npm` and `version::fetch_npm_tag` spawn `npm` by bare name (`Command::new("npm")`).
//! To test them without touching the real npm registry, we install a fake `npm` shell script that logs its args and prints canned stdout.
//! The script lives in a tempdir prepended to `PATH` for the duration of the test.
//!
//! The same pattern covers `gh` for the `gh-release` installer paths.
//!
//! All tests in this file mutate `PATH` (global), so they're serialized with `#[serial]`.

#![cfg(unix)]

mod common;

use serial_test::serial;

use common::{FakeBinGuard, GhApiMockGuard};
use xai_grok_update::auto_update::install_npm_for_test;
use xai_grok_update::version::{
    fetch_gh_release_version, fetch_npm_tag_for_test, fetch_npm_version_for_test,
};

// ─────────────────────────────────────────────────────────────────────────────
// fetch_npm_tag — reads a single dist-tag from `npm view`.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
#[serial]
async fn fetch_npm_tag_returns_string_response() {
    let g = FakeBinGuard::install_npm();
    g.set_stdout("\"0.1.181\"\n");

    let v = fetch_npm_tag_for_test("latest", None).await.unwrap();
    assert_eq!(v, "0.1.181");
}

#[tokio::test]
#[serial]
async fn fetch_npm_tag_returns_array_response_picks_last() {
    // npm view sometimes returns an array of versions for ambiguous specs.
    // The implementation picks the LAST one (rev().find_map).
    let g = FakeBinGuard::install_npm();
    g.set_stdout(r#"["0.1.179", "0.1.180", "0.1.181"]"#);

    let v = fetch_npm_tag_for_test("latest", None).await.unwrap();
    assert_eq!(v, "0.1.181");
}

#[tokio::test]
#[serial]
async fn fetch_npm_tag_passes_pkg_and_tag_to_npm() {
    let g = FakeBinGuard::install_npm();
    g.set_stdout("\"0.1.181\"");

    let _ = fetch_npm_tag_for_test("latest", None).await.unwrap();
    let log = g.args_log();
    assert_eq!(log.len(), 1, "exactly one npm invocation");
    let args = &log[0];
    assert!(args.contains("view"), "args: {args}");
    // For "latest" tag, no `@latest` suffix is appended in pkg_spec.
    assert!(args.contains("chaos-code"), "args: {args}");
    assert!(!args.contains("@latest"), "args: {args}");
    assert!(args.contains("--json"), "args: {args}");
}

#[tokio::test]
#[serial]
async fn fetch_npm_tag_alpha_appends_at_alpha_suffix() {
    let g = FakeBinGuard::install_npm();
    g.set_alpha_stdout("\"0.1.181-alpha.1\"");

    let v = fetch_npm_tag_for_test("alpha", None).await.unwrap();
    assert_eq!(v, "0.1.181-alpha.1");

    let log = g.args_log();
    assert!(log[0].contains("chaos-code@alpha"), "args: {}", log[0]);
}

#[tokio::test]
#[serial]
async fn fetch_npm_tag_passes_registry_flag_when_set() {
    let g = FakeBinGuard::install_npm();
    g.set_stdout("\"0.1.181\"");

    let _ = fetch_npm_tag_for_test("latest", Some("https://npm.example.com"))
        .await
        .unwrap();
    let log = g.args_log();
    assert!(
        log[0].contains("--registry=https://npm.example.com"),
        "args: {}",
        log[0]
    );
}

#[tokio::test]
#[serial]
async fn fetch_npm_tag_no_registry_flag_when_unset() {
    let g = FakeBinGuard::install_npm();
    g.set_stdout("\"0.1.181\"");

    let _ = fetch_npm_tag_for_test("latest", None).await.unwrap();
    let log = g.args_log();
    assert!(!log[0].contains("--registry"), "args: {}", log[0]);
}

#[tokio::test]
#[serial]
async fn fetch_npm_tag_propagates_npm_failure() {
    let g = FakeBinGuard::install_npm();
    g.set_exit_code(1);

    let err = fetch_npm_tag_for_test("latest", None).await.unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("npm view"), "msg: {msg}");
    assert!(msg.contains("failed"), "msg: {msg}");
}

#[tokio::test]
#[serial]
async fn fetch_npm_tag_invalid_json_returns_err() {
    let g = FakeBinGuard::install_npm();
    g.set_stdout("not valid json {");

    let err = fetch_npm_tag_for_test("latest", None).await.unwrap_err();
    // serde_json should error on this.
    let msg = format!("{err:#}");
    assert!(!msg.is_empty());
}

#[tokio::test]
#[serial]
async fn fetch_npm_tag_unexpected_json_shape_returns_err() {
    // npm view can return null, an object, etc
    // The function expects string or array of strings; anything else is an error
    let g = FakeBinGuard::install_npm();
    g.set_stdout("42");

    let err = fetch_npm_tag_for_test("latest", None).await.unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("unexpected JSON"), "msg: {msg}");
}

#[tokio::test]
#[serial]
async fn fetch_npm_tag_empty_array_returns_err() {
    let g = FakeBinGuard::install_npm();
    g.set_stdout("[]");

    let err = fetch_npm_tag_for_test("latest", None).await.unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("empty"), "msg: {msg}");
}

// ─────────────────────────────────────────────────────────────────────────────
// fetch_npm_version — alpha channel calls both tags and returns the max.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
#[serial]
async fn fetch_npm_version_stable_calls_only_latest() {
    let g = FakeBinGuard::install_npm();
    g.set_stdout("\"0.1.181\"");

    let v = fetch_npm_version_for_test("stable", None).await.unwrap();
    assert_eq!(v, "0.1.181");
    assert_eq!(g.args_log().len(), 1, "stable should make one call");
}

#[tokio::test]
#[serial]
async fn fetch_npm_version_alpha_returns_max_of_alpha_and_latest_when_alpha_higher() {
    let g = FakeBinGuard::install_npm();
    g.set_stdout("\"0.1.181\""); // latest tag (stable)
    g.set_alpha_stdout("\"0.1.182-alpha.1\""); // alpha tag

    let v = fetch_npm_version_for_test("alpha", None).await.unwrap();
    assert_eq!(v, "0.1.182-alpha.1");
    assert_eq!(g.args_log().len(), 2, "alpha should make two calls");
}

#[tokio::test]
#[serial]
async fn fetch_npm_version_alpha_returns_stable_when_higher() {
    // Common case: stable shipped after a stale alpha tag; the updater must not strand alpha users on the older alpha
    let g = FakeBinGuard::install_npm();
    g.set_stdout("\"0.1.182\"");
    g.set_alpha_stdout("\"0.1.181-alpha.1\"");

    let v = fetch_npm_version_for_test("alpha", None).await.unwrap();
    assert_eq!(v, "0.1.182");
}

// ─────────────────────────────────────────────────────────────────────────────
// install_npm — spawns `npm i -g @pkg@version`.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
#[serial]
async fn install_npm_calls_npm_with_version_arg() {
    let g = FakeBinGuard::install_npm();
    // No stdout/exit setup, so the fake npm succeeds with empty stdout

    install_npm_for_test(Some("0.1.181"), "stable", None).unwrap();
    let log = g.args_log();
    assert_eq!(log.len(), 1, "exactly one npm invocation");
    let args = &log[0];
    assert!(args.contains("i -g"), "args: {args}");
    assert!(args.contains("chaos-code@0.1.181"), "args: {args}");
}

#[tokio::test]
#[serial]
async fn install_npm_falls_back_to_dist_tag_on_no_target() {
    let g = FakeBinGuard::install_npm();

    install_npm_for_test(None, "stable", None).unwrap();
    let log = g.args_log();
    assert!(
        log[0].contains("chaos-code@latest"),
        "stable channel uses @latest dist-tag: {}",
        log[0]
    );
}

#[tokio::test]
#[serial]
async fn install_npm_falls_back_to_alpha_dist_tag_on_alpha_channel() {
    let g = FakeBinGuard::install_npm();

    install_npm_for_test(None, "alpha", None).unwrap();
    let log = g.args_log();
    assert!(
        log[0].contains("chaos-code@alpha"),
        "alpha channel uses @alpha dist-tag: {}",
        log[0]
    );
}

#[tokio::test]
#[serial]
async fn install_npm_passes_registry_flag_when_set() {
    let g = FakeBinGuard::install_npm();

    install_npm_for_test(Some("0.1.181"), "stable", Some("https://npm.example.com")).unwrap();
    let log = g.args_log();
    assert!(
        log[0].contains("--registry=https://npm.example.com"),
        "args: {}",
        log[0]
    );
}

#[tokio::test]
#[serial]
async fn install_npm_no_registry_flag_when_unset() {
    let g = FakeBinGuard::install_npm();

    install_npm_for_test(Some("0.1.181"), "stable", None).unwrap();
    let log = g.args_log();
    assert!(!log[0].contains("--registry"), "args: {}", log[0]);
}

#[tokio::test]
#[serial]
async fn install_npm_returns_err_on_npm_failure() {
    let g = FakeBinGuard::install_npm();
    g.set_exit_code(1);

    let err = install_npm_for_test(Some("0.1.181"), "stable", None).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("npm install failed"), "msg: {msg}");
}

#[tokio::test]
#[serial]
async fn install_npm_with_token_passes_userconfig() {
    // SAFETY: serial_test ensures no other thread touches NPM_TOKEN.
    unsafe { std::env::set_var("NPM_TOKEN", "secrettoken") };
    let g = FakeBinGuard::install_npm();

    install_npm_for_test(Some("0.1.181"), "stable", None).unwrap();
    let log = g.args_log();
    assert!(
        log[0].contains("--userconfig="),
        "with NPM_TOKEN, must pass --userconfig: {}",
        log[0]
    );
    // The userconfig path should be cleaned up afterwards.
    let userconfig_arg = log[0]
        .split_whitespace()
        .find(|a| a.starts_with("--userconfig="))
        .unwrap()
        .trim_start_matches("--userconfig=");
    assert!(
        !std::path::Path::new(userconfig_arg).exists(),
        "userconfig file should be cleaned up: {userconfig_arg}"
    );
    unsafe { std::env::remove_var("NPM_TOKEN") };
}

#[tokio::test]
#[serial]
async fn install_npm_no_token_no_userconfig() {
    unsafe { std::env::remove_var("NPM_TOKEN") };
    let g = FakeBinGuard::install_npm();

    install_npm_for_test(Some("0.1.181"), "stable", None).unwrap();
    let log = g.args_log();
    assert!(!log[0].contains("--userconfig"), "args: {}", log[0]);
}

// ─────────────────────────────────────────────────────────────────────────────
// fetch_gh_release_version — exercises the `gh release list` shell-out.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
#[serial]
async fn fetch_gh_release_stable_returns_tag_stripped() {
    // Stable channel hits `/releases/latest` once; the `v` prefix is stripped.
    let g = GhApiMockGuard::start().await;
    g.stub_latest("v0.1.181", false, false).await;

    let v = fetch_gh_release_version("stable").await.unwrap();
    assert_eq!(v, "0.1.181");

    assert_eq!(g.received_latest_count().await, 1);
}

#[tokio::test]
#[serial]
async fn fetch_gh_release_stable_handles_tag_without_v_prefix() {
    let g = GhApiMockGuard::start().await;
    g.stub_latest("0.1.181", false, false).await;

    let v = fetch_gh_release_version("stable").await.unwrap();
    assert_eq!(v, "0.1.181");
}

#[tokio::test]
#[serial]
async fn fetch_gh_release_alpha_returns_max_of_pre_and_stable() {

    let v = fetch_gh_release_version("alpha").await.unwrap();
    assert_eq!(v, "0.1.182-alpha.1");
}

#[tokio::test]
#[serial]
async fn fetch_gh_release_alpha_returns_stable_when_higher() {
    let g = GhApiMockGuard::start().await;
    g.stub_latest("v0.1.181", false, false).await;
    g.stub_list(&[
        ("v0.1.180-alpha.5", true, false),
        ("v0.1.181", false, false),
    ])
    .await;

    let v = fetch_gh_release_version("alpha").await.unwrap();
    assert_eq!(v, "0.1.181");
}

#[tokio::test]
#[serial]
async fn fetch_gh_release_propagates_http_failure() {
    let g = GhApiMockGuard::start().await;
    g.stub_latest_status(500).await;

    let err = fetch_gh_release_version("stable").await.unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("500"), "msg: {msg}");
    assert!(
        msg.contains("releases/latest"),
        "msg should mention the endpoint: {msg}"
    );
}

#[tokio::test]
#[serial]
async fn fetch_gh_release_targets_correct_repo() {
    // The request path must include the chaos-code repo slug so a refactor
    // doesn't accidentally point at the wrong repository.
    let g = GhApiMockGuard::start().await;
    g.stub_latest("v0.1.181", false, false).await;

    let _ = fetch_gh_release_version("stable").await.unwrap();
    let requests = g.server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let path = requests[0].url.path();
    assert!(
        path.contains("/repos/chao2hang/chaos-code/releases"),
        "unexpected path: {path}"
    );
}

#[tokio::test]
#[serial]
async fn fetch_gh_release_skips_draft_releases() {
    // Draft releases must be excluded from the semver-max calculation even
    // when they appear in the list response (API may return them).
    let g = GhApiMockGuard::start().await;
    g.stub_latest("v0.1.181", false, false).await;
    g.stub_list(&[
        ("v0.2.0", true, true),     // draft + prerelease — must be skipped
        ("v0.1.181", false, false), // stable release
    ])
    .await;

    let v = fetch_gh_release_version("alpha").await.unwrap();
    assert_eq!(v, "0.1.181");
}
