// Per-test-case module for the `pty_e2e` integration test crate.
#[allow(unused_imports)]
use crate::common::*;

/// Minimal mode surfaces the Shift+Tab session mode in the one-line info bar
/// directly under the prompt. The mode cycle itself is shared with the full TUI,
/// but minimal had no persistent indicator, so pressing Shift+Tab "did nothing
/// visible" (dogfood nit). The first press (Normal → Plan, gate-independent)
/// must light a Chinese `计划` flag in the info bar that
/// `crate::minimal::live::render_prompt_info` draws below the prompt.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn minimal_shift_tab_shows_mode_in_info_bar() {
    let content = ContentController::start().await.expect("start content");
    let mut harness = spawn_minimal(&content);
    wait_minimal_ready(&mut harness);

    // Baseline: nothing on the idle screen says "计划" — the welcome card hint
    // is "/help for commands …", the status line "minimal · /help", and the
    // empty-prompt placeholder. The mode flag under test is the only source of
    // the Chinese plan label. (The transient mode-switch banner is
    // "已切换到模式：计划", which also contains "计划", so wait after inject.)
    assert!(
        !harness.contains_text("计划"),
        "precondition: idle minimal screen must not already show '计划'\nscreen:\n{}",
        harness.screen_contents()
    );

    // Shift+Tab → BackTab (CSI Z). First press cycles Normal → Plan.
    harness.inject_keys(b"\x1b[Z").expect("inject BackTab");
    harness
        .wait_for_text("计划", Duration::from_secs(10))
        .expect("plan flag in the info bar under the prompt after Shift+Tab");

    assert!(
        !harness.contains_text("panicked"),
        "pager panicked\nscreen:\n{}",
        harness.screen_contents()
    );

    quit_minimal(&mut harness);
}
