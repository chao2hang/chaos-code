// Per-test-case module for the `pty_e2e` integration test crate.
#[allow(unused_imports)]
use super::common::*;

/// 1b. **Welcome screen renders Chaos block-shade logo correctly.**
///
/// The welcome logo is Unicode shade/half-block art spelling "CHAOS". This
/// test asserts distinctive fragments from the full logo appear intact in the
/// PTY screen buffer (regression guard for encoding / layout regressions).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn welcome_screen_braille_logo_renders_correctly() {
    let content = ContentController::start().await.expect("start content");

    let binary = pager_binary().expect("resolve pager binary");
    // Use a tall terminal so pick_logo() selects the full logo.
    let mut harness =
        PtyHarness::spawn_with_content(&binary, DEFAULT_ROWS, DEFAULT_COLS, &content, &[])
            .expect("spawn pager");

    harness
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome text");

    let screen = harness.screen_contents();

    // Distinctive fragments from logo07.txt (full CHAOS block-shade art).
    assert!(
        screen.contains("▄████▄"),
        "logo fragment `▄████▄` not found in screen — \
         logo may be missing or truncated.\n\
         Screen contents:\n{screen}"
    );
    assert!(
        screen.contains("▒█████"),
        "logo fragment `▒█████` not found in screen — \
         logo may be missing or truncated.\n\
         Screen contents:\n{screen}"
    );

    harness.quit().expect("clean quit");
}
