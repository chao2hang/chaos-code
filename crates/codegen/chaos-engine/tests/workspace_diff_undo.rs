//! The undo point behind the GUI's 差异 tab, driven through the real entry point.
//!
//! Every case here goes through `Engine::handle` with `WorkspaceDiffAdapter`
//! attached the way a host attaches it, and checks the bytes on disk afterwards.
//! A proposal the browser can see but cannot put back, or a rollback that erases
//! someone else's later edit, is the failure these tests exist to catch.

use chaos_engine::{ClientMessage, DIFF_PROPOSAL_LIMIT, DiffAdapter, Engine, ServerMessage};
use tempfile::tempdir;
use uuid::Uuid;

/// The engine answers a `client_msg_id` it has already seen with a bare Ack, so
/// every message in these tests carries a fresh one.
fn msg_id(label: &str) -> String {
    format!("{label}-{}", Uuid::new_v4())
}

fn create_session(engine: &Engine) -> Uuid {
    let created = engine.handle(ClientMessage::CreateSession {
        client_msg_id: msg_id("create"),
        workspace_id: None,
    });
    match created.as_slice() {
        [ServerMessage::SessionCreated { session_id, .. }] => *session_id,
        other => panic!("unexpected {other:?}"),
    }
}

/// Propose a write and approve it, which is the only way bytes reach the
/// workspace, returning everything the host answered.
fn approved_write(
    engine: &Engine,
    session_id: Uuid,
    relative_path: &str,
    contents: &str,
) -> Vec<ServerMessage> {
    let proposed = engine.handle(ClientMessage::ProposeFileWrite {
        client_msg_id: msg_id(&format!("propose-{relative_path}")),
        session_id,
        relative_path: relative_path.to_string(),
        contents: contents.to_string(),
    });
    let request_id = match proposed.as_slice() {
        [
            ServerMessage::Ack { .. },
            ServerMessage::ToolApprovalRequested { request_id, .. },
        ] => *request_id,
        other => panic!("unexpected proposal answer {other:?}"),
    };
    let mut events = engine.handle(ClientMessage::Approve {
        client_msg_id: msg_id(&format!("approve-{relative_path}")),
        request_id,
    });
    // The proposal round's own events are not part of the approval answer, but a
    // refusal there is what these tests are usually about, so fold it in.
    events.splice(0..0, proposed.into_iter().skip(2));
    events
}

fn undo_point(events: &[ServerMessage], relative_path: &str) -> String {
    events
        .iter()
        .find_map(|event| match event {
            ServerMessage::DiffPreview { preview, .. } if preview.path == relative_path => {
                Some(preview.proposal_id.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no undo point offered for {relative_path}: {events:?}"))
}

fn error_code(events: &[ServerMessage]) -> Option<String> {
    events.iter().find_map(|event| match event {
        ServerMessage::Error { code, .. } => Some(code.clone()),
        _ => None,
    })
}

fn rollback(engine: &Engine, session_id: Uuid, proposal_id: &str) -> Vec<ServerMessage> {
    engine.handle(ClientMessage::RollbackDiff {
        client_msg_id: msg_id("rollback"),
        session_id,
        proposal_id: proposal_id.to_string(),
    })
}

fn accept(engine: &Engine, session_id: Uuid, proposal_id: &str) -> Vec<ServerMessage> {
    engine.handle(ClientMessage::AcceptDiff {
        client_msg_id: msg_id("accept"),
        session_id,
        proposal_id: proposal_id.to_string(),
        summary: "确认保留".into(),
    })
}

fn read_bytes(root: &std::path::Path, relative_path: &str) -> Option<Vec<u8>> {
    std::fs::read(root.join(relative_path)).ok()
}

#[test]
fn landed_write_offers_a_preview_naming_the_bytes_it_replaced() {
    let directory = tempdir().unwrap();
    std::fs::write(directory.path().join("note.txt"), b"line one\n").unwrap();
    let engine = Engine::with_workspace(directory.path())
        .unwrap()
        .with_workspace_diff_adapter()
        .unwrap();
    let session_id = create_session(&engine);

    let events = approved_write(&engine, session_id, "note.txt", "line two\n");
    let preview = match events
        .iter()
        .find(|event| matches!(event, ServerMessage::DiffPreview { .. }))
    {
        Some(ServerMessage::DiffPreview { preview, .. }) => preview,
        other => panic!("expected a preview, got {other:?}"),
    };
    assert_eq!(preview.path, "note.txt");
    assert_eq!(preview.before.as_deref(), Some("line one\n"));
    assert_eq!(preview.after, "line two\n");
    // The write itself is unchanged by the undo seam: the bytes landed.
    assert_eq!(
        read_bytes(directory.path(), "note.txt").as_deref(),
        Some("line two\n".as_bytes())
    );
    assert!(
        events.iter().any(|event| matches!(
            event,
            ServerMessage::FileWritten { path, .. } if path == "note.txt"
        )),
        "{events:?}"
    );
}

/// A write that created the path has no prior bytes, and the browser shows that
/// as a newly created file rather than an empty old version.
#[test]
fn created_file_previews_with_no_before_side() {
    let directory = tempdir().unwrap();
    let engine = Engine::with_workspace(directory.path())
        .unwrap()
        .with_workspace_diff_adapter()
        .unwrap();
    let session_id = create_session(&engine);

    let events = approved_write(&engine, session_id, "new.txt", "created\n");
    let preview = match events
        .iter()
        .find(|event| matches!(event, ServerMessage::DiffPreview { .. }))
    {
        Some(ServerMessage::DiffPreview { preview, .. }) => preview,
        other => panic!("expected a preview, got {other:?}"),
    };
    assert_eq!(preview.before, None);
    assert_eq!(preview.after, "created\n");
}

#[test]
fn rollback_puts_the_prior_bytes_back_and_reports_it_resolved() {
    let directory = tempdir().unwrap();
    std::fs::write(directory.path().join("note.txt"), b"line one\n").unwrap();
    let engine = Engine::with_workspace(directory.path())
        .unwrap()
        .with_workspace_diff_adapter()
        .unwrap();
    let session_id = create_session(&engine);
    let proposal_id = undo_point(
        &approved_write(&engine, session_id, "note.txt", "line two\n"),
        "note.txt",
    );

    let events = rollback(&engine, session_id, &proposal_id);
    assert_eq!(error_code(&events), None, "{events:?}");
    assert!(
        events.iter().any(|event| matches!(
            event,
            ServerMessage::DiffResolved { action, .. } if action == "rollback_diff"
        )),
        "{events:?}"
    );
    assert_eq!(
        read_bytes(directory.path(), "note.txt").as_deref(),
        Some("line one\n".as_bytes())
    );

    // The undo is spent: a second rollback must not be able to write again.
    std::fs::write(directory.path().join("note.txt"), b"edited elsewhere\n").unwrap();
    let again = rollback(&engine, session_id, &proposal_id);
    assert!(
        matches!(error_code(&again).as_deref(), Some("diff_failed")),
        "{again:?}"
    );
    assert_eq!(
        read_bytes(directory.path(), "note.txt").as_deref(),
        Some("edited elsewhere\n".as_bytes())
    );
}

/// Rolling back a write that created a path takes the path away. An empty file
/// left behind would be a file the workspace never had.
#[test]
fn rollback_of_a_created_file_takes_the_path_away() {
    let directory = tempdir().unwrap();
    let engine = Engine::with_workspace(directory.path())
        .unwrap()
        .with_workspace_diff_adapter()
        .unwrap();
    let session_id = create_session(&engine);
    let proposal_id = undo_point(
        &approved_write(&engine, session_id, "new.txt", "created\n"),
        "new.txt",
    );

    let events = rollback(&engine, session_id, &proposal_id);
    assert_eq!(error_code(&events), None, "{events:?}");
    assert_eq!(read_bytes(directory.path(), "new.txt"), None);
}

/// Accepting says the change stays, which is also what takes the undo away. The
/// bytes are already on disk, so accepting must not write anything.
#[test]
fn accept_keeps_the_bytes_and_spends_the_undo() {
    let directory = tempdir().unwrap();
    std::fs::write(directory.path().join("note.txt"), b"line one\n").unwrap();
    let engine = Engine::with_workspace(directory.path())
        .unwrap()
        .with_workspace_diff_adapter()
        .unwrap();
    let session_id = create_session(&engine);
    let proposal_id = undo_point(
        &approved_write(&engine, session_id, "note.txt", "line two\n"),
        "note.txt",
    );

    let events = accept(&engine, session_id, &proposal_id);
    assert_eq!(error_code(&events), None, "{events:?}");
    assert!(
        events.iter().any(|event| matches!(
            event,
            ServerMessage::DiffResolved { action, .. } if action == "accept_diff"
        )),
        "{events:?}"
    );
    assert_eq!(
        read_bytes(directory.path(), "note.txt").as_deref(),
        Some("line two\n".as_bytes())
    );

    let too_late = rollback(&engine, session_id, &proposal_id);
    assert!(
        matches!(error_code(&too_late).as_deref(), Some("diff_failed")),
        "{too_late:?}"
    );
    assert_eq!(
        read_bytes(directory.path(), "note.txt").as_deref(),
        Some("line two\n".as_bytes())
    );
}

/// The guard the whole design turns on: an edit made after the write belongs to
/// the user or to git, not to this proposal. The refusal has to leave the file
/// alone and keep the preview on screen -- `diff_resolved` clears it, so
/// emitting one here would hide the refusal behind a settled-looking tab.
#[test]
fn rollback_refuses_an_edit_it_did_not_make_and_leaves_the_file_alone() {
    let directory = tempdir().unwrap();
    std::fs::write(directory.path().join("note.txt"), b"line one\n").unwrap();
    let engine = Engine::with_workspace(directory.path())
        .unwrap()
        .with_workspace_diff_adapter()
        .unwrap();
    let session_id = create_session(&engine);
    let proposal_id = undo_point(
        &approved_write(&engine, session_id, "note.txt", "line two\n"),
        "note.txt",
    );
    std::fs::write(directory.path().join("note.txt"), b"git checkout\n").unwrap();

    let events = rollback(&engine, session_id, &proposal_id);
    assert!(
        matches!(error_code(&events).as_deref(), Some("diff_failed")),
        "{events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ServerMessage::DiffResolved { .. })),
        "a refusal must not clear the browser preview: {events:?}"
    );
    assert!(
        events.iter().any(|event| matches!(
            event,
            ServerMessage::Audit { outcome, .. } if outcome == "rejected"
        )),
        "{events:?}"
    );
    assert_eq!(
        read_bytes(directory.path(), "note.txt").as_deref(),
        Some("git checkout\n".as_bytes())
    );
}

/// The refusal is retryable rather than spent: once the file is back to what the
/// proposal wrote, the rollback goes through.
#[test]
fn a_refused_rollback_can_be_retried_once_the_file_matches_again() {
    let directory = tempdir().unwrap();
    std::fs::write(directory.path().join("note.txt"), b"line one\n").unwrap();
    let engine = Engine::with_workspace(directory.path())
        .unwrap()
        .with_workspace_diff_adapter()
        .unwrap();
    let session_id = create_session(&engine);
    let proposal_id = undo_point(
        &approved_write(&engine, session_id, "note.txt", "line two\n"),
        "note.txt",
    );
    std::fs::write(directory.path().join("note.txt"), b"oops\n").unwrap();
    assert!(matches!(
        error_code(&rollback(&engine, session_id, &proposal_id)).as_deref(),
        Some("diff_failed")
    ));

    std::fs::write(directory.path().join("note.txt"), b"line two\n").unwrap();
    let retry = rollback(&engine, session_id, &proposal_id);
    assert_eq!(error_code(&retry), None, "{retry:?}");
    assert_eq!(
        read_bytes(directory.path(), "note.txt").as_deref(),
        Some("line one\n".as_bytes())
    );
}

/// A path the workspace would not write to is refused by name, with nothing on
/// disk changed. The same rules govern the write and the undo, so a proposal
/// cannot outlive the confinement of the write that made it.
#[test]
fn a_proposal_can_never_write_outside_the_workspace() {
    let directory = tempdir().unwrap();
    let outside = tempdir().unwrap();
    std::fs::write(outside.path().join("victim.txt"), b"keep me\n").unwrap();
    let engine = Engine::with_workspace(directory.path())
        .unwrap()
        .with_workspace_diff_adapter()
        .unwrap();
    let session_id = create_session(&engine);

    let escape = format!("{}/victim.txt", outside.path().display());
    let events = approved_write(&engine, session_id, &escape, "overwritten\n");
    assert!(
        matches!(error_code(&events).as_deref(), Some("path_escape")),
        "{events:?}"
    );
    assert_eq!(
        read_bytes(outside.path(), "victim.txt").as_deref(),
        Some("keep me\n".as_bytes())
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ServerMessage::DiffPreview { .. })),
        "{events:?}"
    );
}

/// A symlinked directory cannot carry a write to a path that does not exist yet.
///
/// The file itself is not there to be inspected, so nothing but the check on where
/// its parent resolves stands between this write and the directory outside. This
/// is also the case that would let a rollback restore through a directory the
/// write was never allowed in, since both go through the same resolution.
#[cfg(unix)]
#[test]
fn an_approved_write_through_a_symlinked_directory_stays_inside_the_workspace() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().unwrap();
    let outside = tempdir().unwrap();
    symlink(outside.path(), directory.path().join("portal")).unwrap();
    let engine = Engine::with_workspace(directory.path())
        .unwrap()
        .with_workspace_diff_adapter()
        .unwrap();
    let session_id = create_session(&engine);

    let events = approved_write(&engine, session_id, "portal/new.txt", "escaped\n");
    assert!(
        matches!(error_code(&events).as_deref(), Some("path_escape")),
        "{events:?}"
    );
    assert!(!outside.path().join("new.txt").exists());
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ServerMessage::DiffPreview { .. })),
        "{events:?}"
    );
}

/// A symlink parked at the target after the write cannot point the restore
/// somewhere the write itself was never allowed to reach.
///
/// The file behind the link holds exactly the bytes this proposal recorded as its
/// new side, so nothing but root confinement stands between the restore and it:
/// with the confinement check removed, the restore writes straight through the
/// link and the file outside the workspace changes.
#[cfg(unix)]
#[test]
fn rollback_cannot_be_redirected_by_a_symlink_swapped_in_after_the_write() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let victim = outside.path().join("victim.txt");
    std::fs::write(&victim, b"created\n").unwrap();
    std::fs::write(directory.path().join("note.txt"), b"orig\n").unwrap();
    let engine = Engine::with_workspace(directory.path())
        .unwrap()
        .with_workspace_diff_adapter()
        .unwrap();
    let session_id = create_session(&engine);
    let proposal_id = undo_point(
        &approved_write(&engine, session_id, "note.txt", "created\n"),
        "note.txt",
    );

    std::fs::remove_file(directory.path().join("note.txt")).unwrap();
    symlink(&victim, directory.path().join("note.txt")).unwrap();

    let events = rollback(&engine, session_id, &proposal_id);
    assert!(
        matches!(error_code(&events).as_deref(), Some("diff_failed")),
        "{events:?}"
    );
    assert_eq!(
        read_bytes(outside.path(), "victim.txt").as_deref(),
        Some("created\n".as_bytes())
    );
    assert!(std::fs::symlink_metadata(directory.path().join("note.txt")).is_ok());
}

/// Overwriting bytes the browser could not have read is refused outright rather
/// than written and then left un-displayable: the proposal's old side has to be
/// text, so the target has to be text before the write starts.
#[test]
fn a_binary_target_refuses_the_write_and_keeps_its_bytes() {
    let directory = tempdir().unwrap();
    let binary: Vec<u8> = vec![0x7f, b'E', b'L', b'F', 0xff, 0xfe, 0x00, 0x01];
    std::fs::write(directory.path().join("blob.bin"), &binary).unwrap();
    let engine = Engine::with_workspace(directory.path())
        .unwrap()
        .with_workspace_diff_adapter()
        .unwrap();
    let session_id = create_session(&engine);

    let events = approved_write(&engine, session_id, "blob.bin", "now it is text\n");
    assert!(
        matches!(error_code(&events).as_deref(), Some("file_not_text")),
        "{events:?}"
    );
    assert_eq!(
        read_bytes(directory.path(), "blob.bin").as_deref(),
        Some(&binary[..])
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ServerMessage::FileWritten { .. })),
        "{events:?}"
    );
    assert!(
        events.iter().any(|event| matches!(
            event,
            ServerMessage::Audit { outcome, .. } if outcome == "failed"
        )),
        "{events:?}"
    );
}

/// An unknown or already handled id is refused without touching the file the
/// browser happens to be showing.
#[test]
fn an_unknown_proposal_id_changes_nothing() {
    let directory = tempdir().unwrap();
    std::fs::write(directory.path().join("note.txt"), b"keep me\n").unwrap();
    let engine = Engine::with_workspace(directory.path())
        .unwrap()
        .with_workspace_diff_adapter()
        .unwrap();
    let session_id = create_session(&engine);

    let events = rollback(&engine, session_id, "never-proposed");
    assert!(
        matches!(error_code(&events).as_deref(), Some("diff_failed")),
        "{events:?}"
    );
    assert_eq!(
        read_bytes(directory.path(), "note.txt").as_deref(),
        Some("keep me\n".as_bytes())
    );
}

/// Every undo point keeps a copy of what it replaced, so the log is bounded and
/// the oldest change is the first to stop being undoable. The write that pushed
/// it out still landed -- eviction costs the undo, never the file.
#[test]
fn only_the_newest_writes_stay_undoable() {
    // The loop below is written against the constant, so nothing here would
    // notice a bound raised to a size that stops being a limit at all. The value
    // is the product decision: an undo point holds both sides of a write, each
    // capped at WORKSPACE_READ_LIMIT, so this is how much a long session may keep.
    assert_eq!(DIFF_PROPOSAL_LIMIT, 32);
    let directory = tempdir().unwrap();
    let engine = Engine::with_workspace(directory.path())
        .unwrap()
        .with_workspace_diff_adapter()
        .unwrap();
    let session_id = create_session(&engine);

    let mut oldest = String::new();
    for index in 0..=DIFF_PROPOSAL_LIMIT {
        let path = format!("f{index}.txt");
        let events = approved_write(&engine, session_id, &path, "written\n");
        let id = undo_point(&events, &path);
        if index == 0 {
            oldest = id;
        }
    }
    assert_eq!(
        read_bytes(directory.path(), "f0.txt").as_deref(),
        Some("written\n".as_bytes()),
        "an evicted undo point must not roll the file back on its own"
    );

    let evicted = rollback(&engine, session_id, &oldest);
    assert!(
        matches!(error_code(&evicted).as_deref(), Some("diff_failed")),
        "{evicted:?}"
    );
    assert_eq!(
        read_bytes(directory.path(), "f0.txt"),
        Some(b"written\n".to_vec())
    );
}

/// Two proposals for one path are two undo points, each with its own old side:
/// rolling the newer one back restores the older write's bytes, not the
/// original file, and rolling the older one back afterwards still works.
#[test]
fn stacked_writes_to_one_path_undo_back_through_each_other() {
    let directory = tempdir().unwrap();
    std::fs::write(directory.path().join("note.txt"), b"one\n").unwrap();
    let engine = Engine::with_workspace(directory.path())
        .unwrap()
        .with_workspace_diff_adapter()
        .unwrap();
    let session_id = create_session(&engine);
    let first = undo_point(
        &approved_write(&engine, session_id, "note.txt", "two\n"),
        "note.txt",
    );
    let second = undo_point(
        &approved_write(&engine, session_id, "note.txt", "three\n"),
        "note.txt",
    );
    assert_ne!(first, second);

    rollback(&engine, session_id, &second);
    assert_eq!(
        read_bytes(directory.path(), "note.txt").as_deref(),
        Some("two\n".as_bytes())
    );
    rollback(&engine, session_id, &first);
    assert_eq!(
        read_bytes(directory.path(), "note.txt").as_deref(),
        Some("one\n".as_bytes())
    );
}

/// Without the seam a host still writes -- the undo point is an addition to the
/// write path, not a prerequisite for it -- and simply offers nothing to undo.
#[test]
fn a_host_without_the_seam_writes_and_offers_no_undo_point() {
    let directory = tempdir().unwrap();
    let engine = Engine::with_workspace(directory.path()).unwrap();
    let session_id = create_session(&engine);

    let events = approved_write(&engine, session_id, "note.txt", "written\n");
    assert_eq!(error_code(&events), None, "{events:?}");
    assert_eq!(
        read_bytes(directory.path(), "note.txt").as_deref(),
        Some("written\n".as_bytes())
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ServerMessage::DiffPreview { .. })),
        "{events:?}"
    );

    let events = rollback(&engine, session_id, "anything");
    assert!(
        matches!(error_code(&events).as_deref(), Some("diff_failed")),
        "{events:?}"
    );
}

struct RefusingDiff;

impl DiffAdapter for RefusingDiff {
    fn accept(&self, _proposal_id: &str, _summary: &str) -> Result<(), String> {
        Err("占位 adapter".into())
    }

    fn rollback(&self, _proposal_id: &str) -> Result<(), String> {
        Err("占位 adapter".into())
    }
}

/// The seam is attached by the host on purpose: a construction that already has
/// a diff adapter is refused rather than quietly swapped, and a host with no
/// workspace has nothing to record against.
#[test]
fn the_seam_refuses_to_be_installed_twice_or_without_a_workspace() {
    let directory = tempdir().unwrap();
    let refused = Engine::with_workspace_and_diff_adapter(directory.path(), RefusingDiff)
        .unwrap()
        .with_workspace_diff_adapter();
    let Err(error) = refused else {
        panic!("a host that already has a diff adapter must not get a second one");
    };
    assert!(error.to_string().contains("Diff adapter"), "{error}");

    let no_workspace = Engine::new().with_workspace_diff_adapter();
    let Err(error) = no_workspace else {
        panic!("an undo point needs a workspace to write back into");
    };
    assert!(error.to_string().contains("workspace"), "{error}");
}

/// The refusal above is what keeps a host's own adapter in charge: it is still
/// the one answering, and its answer is what the browser sees.
#[test]
fn a_host_that_brought_its_own_diff_adapter_keeps_it() {
    let directory = tempdir().unwrap();
    let engine = Engine::with_workspace_and_diff_adapter(directory.path(), RefusingDiff).unwrap();
    let session_id = create_session(&engine);

    let events = engine.handle(ClientMessage::AcceptDiff {
        client_msg_id: msg_id("accept"),
        session_id,
        proposal_id: "p".into(),
        summary: "s".into(),
    });
    assert!(
        events.iter().any(|event| matches!(
            event,
            ServerMessage::Error { code, message }
                if code == "diff_failed" && message == "占位 adapter"
        )),
        "{events:?}"
    );
}

/// `preview_diff` for a live proposal reports the same pair of sides the write
/// offered, because the browser's 加载差异 button goes through this call after a
/// reload, when it no longer holds the id from the write.
#[test]
fn preview_diff_reports_the_recorded_sides_again() {
    let directory = tempdir().unwrap();
    std::fs::write(directory.path().join("note.txt"), b"one\n").unwrap();
    let engine = Engine::with_workspace(directory.path())
        .unwrap()
        .with_workspace_diff_adapter()
        .unwrap();
    let session_id = create_session(&engine);
    let proposal_id = undo_point(
        &approved_write(&engine, session_id, "note.txt", "two\n"),
        "note.txt",
    );

    let events = engine.handle(ClientMessage::PreviewDiff {
        client_msg_id: msg_id("preview"),
        session_id,
        proposal_id: proposal_id.clone(),
    });
    match events.as_slice() {
        [
            ServerMessage::Ack { .. },
            ServerMessage::DiffPreview { preview, .. },
        ] => {
            assert_eq!(preview.proposal_id, proposal_id);
            assert_eq!(preview.before.as_deref(), Some("one\n"));
            assert_eq!(preview.after, "two\n");
        }
        other => panic!("unexpected {other:?}"),
    }
    // A preview read must not consume the undo point.
    assert!(
        error_code(&rollback(&engine, session_id, &proposal_id))
            .as_deref()
            .is_none(),
        "the undo point should still be there after a preview read"
    );
}
