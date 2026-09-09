// Per-test-case module for the `pty_e2e` integration test crate.
#[allow(unused_imports)]
use super::common::*;

/// Single-line message; the drag enters it at the tail word.
const GAPDEEP_LINE: &str = "GAPDEEP alpha beta gamma delta epsilon";

const ENTRY_WORD: &str = "epsilon";


/// PTY: a mouse-down on the blank gap below the conversation (between the turn marker and the prompt box) starts an anchor-less drag.
/// Dead space is a valid drag start.
/// The anchor appears at the first drag position that lands on selectable text: here a word inside the last message.
/// The payload is the entry-to-release slice of that single line, not a snap to the text nearest the press, not a block copy.
///
/// `SSH_CONNECTION` forces the OSC 52 clipboard route for readback.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY e2e; run the owning pty_e2e_* Cargo test with --ignored (see Cargo.toml)"]
async fn drag_enters_content_from_gap_pty() {
    let content = ContentController::start().await.expect("start content");
    // Copy-on-release (OSC 52) only in flash. Pin it so sibling tests that seed hold/word_select cannot change the behavior if config leaks.
    seed_ui_config(&content, "keep_text_selection = \"flash\"");
    content.set_response(GAPDEEP_LINE.to_string());

    let binary = pager_binary().expect("resolve pager binary");
    let overrides: Vec<(String, String)> = vec![(
        "SSH_CONNECTION".into(),
        "scripted-test 1 127.0.0.1 2".into(),
    )];
    let env_refs: Vec<(&str, &str)> = overrides
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    let mut harness = PtyHarness::spawn_with_content_env_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &[],
        &env_refs,
        Some(content.home()),
    )
    .expect("spawn pager");

    harness
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome");
    harness
        .inject_keys(format!("{PROMPT}\r").as_bytes())
        .expect("submit prompt");
    harness
        .wait_for_text(ENTRY_WORD, Duration::from_secs(45))
        .expect("message rendered");
    harness
        .wait_for_text("耗时", Duration::from_secs(20))
        .expect("turn marker rendered");
    harness
        .wait_for_turn_idle(Duration::from_secs(20))
        .expect("turn idle before locating gap-drag coords");

    harness.inject_keys(b"\t").expect("focus scrollback");
    harness
        .wait_for_text("Space:prompt", Duration::from_secs(10))
        .expect("scrollback focused (Space:prompt hint) after Tab");

    let screen = harness.screen_contents();
    let (msg_row, _) = locate_screen_text(&screen, "GAPDEEP")
        .unwrap_or_else(|| panic!("could not locate GAPDEEP; screen:\n{screen}"));
    let (entry_row, entry_col) = locate_screen_text(&screen, ENTRY_WORD)
        .unwrap_or_else(|| panic!("could not locate {ENTRY_WORD:?}; screen:\n{screen}"));
    assert_eq!(entry_row, msg_row, "setup: single unwrapped message line");
    let (marker_row, _) = locate_screen_text(&screen, "耗时")
        .unwrap_or_else(|| panic!("could not locate the turn marker; screen:\n{screen}"));
    assert!(marker_row > msg_row, "setup: marker below the message");

    // PRESS in the gap, then drag up into the message
    // The motion samples jump the marker row deliberately (terminals coalesce motion)
    // The column clamp within a row makes the marker's line hittable at any column of its row, so a sample there would anchor the drag on the marker
    // That anchor would be correct (the first text entered wins), but it is not this test's subject
    // First sample on the message anchors at the word's first column; then extend to its last column and release
    let head_col = entry_col + ENTRY_WORD.len() as u16 - 1;
    let seen = decode_osc52_payloads(harness.raw_output()).len();
    let mut drag = String::new();
    drag.push_str(&sgr_mouse(0, gap_row, entry_col, 'M'));
    drag.push_str(&sgr_mouse(32, entry_row, entry_col, 'M'));
    drag.push_str(&sgr_mouse(32, entry_row, head_col, 'M'));
    drag.push_str(&sgr_mouse(0, entry_row, head_col, 'm'));
    harness
        .inject_keys(drag.as_bytes())
        .expect("press the gap, drag up into the message");

    let deadline = Instant::now() + Duration::from_secs(10);
    let payloads = loop {
        harness.update(Duration::from_millis(200));
        let all = decode_osc52_payloads(harness.raw_output());
        if all.len() > seen || Instant::now() >= deadline {
            break all.into_iter().skip(seen).collect::<Vec<_>>();
        }
    };
    assert!(
        !payloads.is_empty(),
        "expected an OSC 52 clipboard write after release; screen:\n{}",
        harness.screen_contents()
    );
    let joined = payloads.join("\n");
    assert_eq!(
        joined, ENTRY_WORD,
        "payload must be the entry-to-release slice of the entered line \
         (anchor at text entry, not at the press or a snap); payloads={payloads:?}"
    );
    assert!(
        !harness.contains_text("panicked"),
        "pager panicked\nscreen:\n{}",
        harness.screen_contents()
    );

    harness.quit().expect("clean quit");
}
