use super::*;

const CHILD_WORKTREE: &str = "XAI_FAST_WORKTREE_SAFETY_CHILD_WORKTREE";
const CHILD_SOURCE: &str = "XAI_FAST_WORKTREE_SAFETY_CHILD_SOURCE";
const CHILD_TEST: &str = "git::safety::tests::gate::verdict_under_a_foreign_git_dir";
const CHILD_SNAPSHOT_TEST: &str = "git::safety::tests::gate::snapshot_under_a_foreign_clean_filter";
const CHILD_LOST_CWD_TEST: &str =
    "git::safety::tests::gate::a_child_that_loses_its_cwd_still_checks_the_worktree";

/// Every check that keeps for a reason other than dirty work logs why, but a
/// bare `Keep(CheckFailed)` in the child's assertion message does not, and the
/// failure it was added for only shows up under load. The child runs a single
/// test, so a global subscriber is cheap; the test writer means it stays
/// silent unless the child fails, and then the log lines travel back through
/// `run_child`, which appends the child's stderr to its own panic.
fn log_child_diagnostics() {
    if let Some(cwd) = unlinked_cwd() {
        eprintln!("[diag] the process cwd is unreadable; /proc/self/cwd still names {cwd:?}");
    }
    let _ = tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_test_writer()
            .with_max_level(tracing::Level::WARN)
            .finish(),
    );
}

/// Once the process cwd has been unlinked, `getcwd()` fails and every
/// `gix::open` in the process comes back as `NotARepository`
/// (`gix-discover-0.51.0/src/is.rs:36`), so a red here no longer says which
/// directory went away. `/proc/self/cwd` still names it, deleted or not.
/// Returns `None` while the cwd is readable, so callers stay silent in the
/// normal case; without `/proc` (macOS) it is a no-op.
fn unlinked_cwd() -> Option<PathBuf> {
    if std::env::current_dir().is_ok() {
        return None;
    }
    std::fs::read_link("/proc/self/cwd").ok()
}

fn run_child(test: &str, worktree: &Path, source: &Path, envs: &[(&str, &std::ffi::OsStr)]) {
    let mut child = std::process::Command::new(std::env::current_exe().unwrap());
    child
        .args([test, "--exact", "--ignored", "--test-threads=1"])
        .env(CHILD_WORKTREE, worktree)
        .env(CHILD_SOURCE, source)
        // libtest shares one process, and `gix_discover::is_git` calls
        // `current_dir()` before it looks at the path it was given
        // (gix-discover-0.51.0/src/is.rs:36). A test elsewhere in this crate
        // chdirs into its own temp directory, so a child spawned here can
        // inherit a cwd that has already been deleted — and then every
        // `gix::open` in the child fails with `NotARepository`, which the gate
        // reports as `CheckFailed`. The temp dir always exists and is in no
        // repository, which is the same cwd class the child had before.
        .current_dir(std::env::temp_dir());
    for (key, value) in envs {
        child.env(key, value);
    }
    let out = child.output().expect("the test binary re-runs itself");
    let report = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && report.contains("1 passed"),
        "{report}{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn git_dir_in_the_environment_does_not_change_the_verdict() {
    let fixture = Fixture::new("");
    let worktree = fixture.linked_worktree("inherits-a-git-dir");
    std::fs::write(worktree.join("tracked.txt"), "two\n").unwrap();
    run_git(&worktree, &["commit", "-am", "work no remote holds"]);
    let decoy = fixture.source.with_file_name("decoy");
    run_git(
        fixture.source.parent().unwrap(),
        &[
            "clone",
            fixture.remote.to_str().unwrap(),
            decoy.to_str().unwrap(),
        ],
    );

    run_child(
        CHILD_TEST,
        &worktree,
        &fixture.source,
        &[("GIT_DIR", decoy.join(".git").as_os_str())],
    );

    assert_eq!(
        std::fs::read(worktree.join("tracked.txt")).expect("the commit's bytes are gone"),
        b"two\n"
    );
}

#[test]
fn git_configuration_in_the_environment_does_not_reach_the_snapshot() {
    let fixture = Fixture::new("");
    let worktree = fixture.linked_worktree("inherits-a-filter");
    attributes(&worktree, "*.txt filter=conv\n", "inherits-a-filter");
    std::fs::write(worktree.join("notes.txt"), "the only copy\n").unwrap();
    let foreign = fixture.source.with_file_name("foreign.gitconfig");
    std::fs::write(
        &foreign,
        "[filter \"conv\"]\n\tclean = sed s/only/nothing/\n",
    )
    .unwrap();

    run_child(
        CHILD_SNAPSHOT_TEST,
        &worktree,
        &fixture.source,
        &[("GIT_CONFIG_GLOBAL", foreign.as_os_str())],
    );
}

/// Puts the process cwd back on the way out, so a test that unlinks its own cwd
/// does not leave the rest of the process unable to resolve any path at all.
struct RestoreCwd(PathBuf);

impl Drop for RestoreCwd {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}

#[test]
fn the_child_does_not_depend_on_the_cwd_it_inherits() {
    // Deterministic form of the flake this file could not explain for 100+
    // rounds; see the ignored test below for the mechanism. The reproduction has
    // to happen in a child process of its own: libtest runs every test in this
    // binary in one process, and a test that unlinks the process cwd makes all
    // ~50 other tests in this module fail with `Keep(CheckFailed)` while it runs,
    // which is the very bug being pinned rather than a way to pin it.
    let fixture = Fixture::new("");
    let worktree = fixture.linked_worktree("inherits-a-filter");
    attributes(&worktree, "*.txt filter=conv\n", "inherits-a-filter");
    std::fs::write(worktree.join("notes.txt"), "the only copy\n").unwrap();
    let foreign = fixture.source.with_file_name("foreign.gitconfig");
    std::fs::write(
        &foreign,
        "[filter \"conv\"]\n\tclean = sed s/only/nothing/\n",
    )
    .unwrap();

    run_child(
        CHILD_LOST_CWD_TEST,
        &worktree,
        &fixture.source,
        &[("GIT_CONFIG_GLOBAL", foreign.as_os_str())],
    );
}

#[test]
#[ignore = "run by the_child_does_not_depend_on_the_cwd_it_inherits"]
fn a_child_that_loses_its_cwd_still_checks_the_worktree() {
    // `gix_discover::is_git` calls `current_dir()` before it looks at the path it
    // was handed (gix-discover-0.51.0/src/is.rs:36), so a deleted cwd makes
    // *every* `gix::open` — even one on an absolute path that exists — come back
    // as `NotARepository`, which the gate reports as `Keep(CheckFailed)`. Tests
    // elsewhere in this crate chdir into a temp directory and then drop it, so a
    // child spawned by `run_child` used to inherit a cwd that was already gone.
    // This process is a single `--exact --ignored` test run, so nothing else in
    // it can observe the unlink.
    log_child_diagnostics();
    let worktree = PathBuf::from(
        std::env::var_os(CHILD_WORKTREE)
            .expect("the parent sets CHILD_WORKTREE; without it this test proves nothing"),
    );
    let source =
        PathBuf::from(std::env::var_os(CHILD_SOURCE).expect("the parent sets CHILD_SOURCE"));
    let config = std::env::var_os("GIT_CONFIG_GLOBAL")
        .expect("the parent passes the filter config down to the grandchild");

    let _restore = RestoreCwd(std::env::current_dir().expect("a fresh child has a real cwd"));
    let doomed = tempfile::TempDir::new().unwrap();
    let doomed_path = doomed.path().to_path_buf();
    std::env::set_current_dir(&doomed_path).unwrap();
    drop(doomed);
    assert!(
        std::env::current_dir().is_err(),
        "the reproduction needs a cwd that has been unlinked; otherwise the grandchild \
         passes for the wrong reason"
    );
    // The one line that turned 100+ unexplained reds into a diagnosis: the
    // diagnostic must still name the directory that is gone. `/proc` is how it
    // names it, so this half of the check is Linux-only.
    #[cfg(target_os = "linux")]
    {
        let named = unlinked_cwd().expect("a deleted cwd still shows up in /proc/self/cwd");
        let text = named.to_string_lossy().into_owned();
        assert_eq!(
            PathBuf::from(text.strip_suffix(" (deleted)").unwrap_or(&text)),
            doomed_path,
            "the diagnostic must name the directory that was unlinked"
        );
    }

    run_child(
        CHILD_SNAPSHOT_TEST,
        &worktree,
        &source,
        &[("GIT_CONFIG_GLOBAL", config.as_os_str())],
    );
}

#[test]
fn no_combination_of_working_tree_shapes_loses_uncarried_work() {
    struct Shape {
        name: &'static str,
        write: fn(&Path),
        carried: bool,
    }

    let shapes = [
        Shape {
            name: "plain",
            write: |at| write_at(at, "notes.txt", b"ordinary work\n"),
            carried: true,
        },
        Shape {
            name: "ignored",
            write: |at| {
                write_at(at, ".gitignore", b".env\n");
                write_at(at, ".env", b"SECRET=1\n");
            },
            carried: false,
        },
        Shape {
            name: "converted",
            write: |at| {
                write_at(at, ".gitattributes", b"*.csv text\n");
                write_at(at, "export.csv", b"a,b\r\n");
            },
            carried: false,
        },
        Shape {
            name: "dot-git",
            write: |at| write_at(at, "testdata/.git/fixture.txt", b"the only copy\n"),
            carried: false,
        },
    ];

    for subset in 0..(1u8 << shapes.len()) {
        let chosen: Vec<_> = shapes
            .iter()
            .enumerate()
            .filter(|(at, _)| subset & (1 << at) != 0)
            .map(|(_, shape)| shape)
            .collect();
        let name = format!("subset-{subset}");
        let fixture = Fixture::new("");
        let worktree = fixture.linked_worktree(&name);
        for shape in &chosen {
            (shape.write)(&worktree);
        }
        let carried = chosen.iter().all(|shape| shape.carried);
        let ref_name = format!("refs/grok/subagents/{name}");

        let safety = reclaim_after_snapshot(&worktree, &fixture.source, &ref_name);

        let held: Vec<&str> = chosen.iter().map(|shape| shape.name).collect();
        assert_eq!(
            safety == Safety::Delete,
            carried,
            "{held:?} answered {safety:?}"
        );
        if carried {
            assert!(!worktree.exists(), "{held:?} was not removed");
            if subset != 0 {
                assert_eq!(
                    run_git(&fixture.source, &["show", &format!("{ref_name}:notes.txt")]),
                    "ordinary work"
                );
            }
        } else {
            assert!(worktree.exists(), "{held:?} was removed");
        }
    }
}

fn write_at(worktree: &Path, path: &str, bytes: &[u8]) {
    let at = worktree.join(path);
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::fs::write(at, bytes).unwrap();
}

#[test]
fn reclaiming_a_worktree_runs_none_of_its_hooks() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new("");
    let worktree = fixture.linked_worktree("has-hooks");
    std::fs::write(worktree.join("tracked.txt"), "work\n").unwrap();
    std::fs::write(worktree.join("untracked.rs"), b"more work").unwrap();

    let ran = fixture.source.with_file_name("the-hook-ran");
    let hooks = fixture.source.join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    for hook in ["reference-transaction", "post-index-change", "pre-commit"] {
        let at = hooks.join(hook);
        std::fs::write(&at, format!("#!/bin/sh\ntouch {}\n", ran.display())).unwrap();
        std::fs::set_permissions(&at, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    snapshot_into(&worktree, &fixture.source, "refs/grok/subagents/has-hooks");

    assert!(
        !ran.exists(),
        "the snapshot ran a hook the repository ships"
    );
}

#[test]
fn the_open_failure_log_carries_the_cause_and_not_only_the_path() {
    // `Keep(CheckFailed)` was an unexplainable verdict for as long as the warn
    // that logged it printed only `gix::open`'s outer message, which is the same
    // sentence for every one of the eleven discovery causes. This pins the two
    // halves of the fix: the outer message really is cause-independent (so a
    // warn that stops at it proves nothing), and the helper the warn uses walks
    // far enough to name the cause and the errno behind it. An earlier attempt
    // here used `{:#}`, which this test caught as a no-op — gix's Display writes
    // no source, alternate flag or not.
    let error = gix::open::Error::NotARepository {
        source: gix::discover::is_git::Error::MissingCommonDir {
            missing: "/w/.git/worktrees/x/commondir".into(),
            source: std::io::Error::from_raw_os_error(24),
        },
        path: "/w".into(),
    };
    let plain = format!("{error}");
    assert!(
        !plain.contains("commondir"),
        "the outer message is supposed to be cause-independent, otherwise the \
         logged chain below proves nothing: {plain}"
    );
    let logged = open_error_chain(&error);
    assert!(
        logged.contains("commondir") && logged.contains("Too many open files"),
        "the form the warn uses must name the cause and its errno: {logged}"
    );
}

#[test]
fn gate_that_panics_keeps_the_worktree() {
    let answer = answer_within(Path::new("/nonexistent"), Duration::from_secs(30), || {
        panic!("the gate fell over")
    });

    assert_eq!(answer, Safety::Keep(KeepReason::CheckFailed));
}

#[test]
fn gate_that_does_not_answer_keeps_the_worktree() {
    let answer = answer_within(Path::new("/nonexistent"), Duration::from_millis(50), || {
        std::thread::sleep(Duration::from_secs(30));
        Safety::Delete
    });

    assert_eq!(
        answer,
        Safety::Keep(KeepReason::GateTimedOut),
        "a hang is reported apart from a check that failed cheaply: only this one \
         leaves a thread behind, and a soak has to be able to count it"
    );
}

#[test]
#[ignore = "run by git_dir_in_the_environment_does_not_change_the_verdict"]
fn verdict_under_a_foreign_git_dir() {
    log_child_diagnostics();
    let worktree = std::env::var_os(CHILD_WORKTREE)
        .expect("the parent sets CHILD_WORKTREE; without it this test proves nothing");
    assert!(
        std::env::var_os("GIT_DIR").is_some(),
        "the parent must set the variable this test is about"
    );
    assert_eq!(
        reclaim(Path::new(&worktree)),
        Safety::Keep(KeepReason::Unpushed)
    );
}

#[test]
#[ignore = "run by git_configuration_in_the_environment_does_not_reach_the_snapshot"]
fn snapshot_under_a_foreign_clean_filter() {
    log_child_diagnostics();
    let worktree = PathBuf::from(
        std::env::var_os(CHILD_WORKTREE)
            .expect("the parent sets CHILD_WORKTREE; without it this test proves nothing"),
    );
    let source =
        PathBuf::from(std::env::var_os(CHILD_SOURCE).expect("the parent sets CHILD_SOURCE"));
    assert!(
        std::env::var_os("GIT_CONFIG_GLOBAL").is_some(),
        "the parent must set the variable this test is about"
    );
    let ref_name = "refs/grok/subagents/inherits-a-filter";
    snapshot_into(&worktree, &source, ref_name);

    assert_eq!(
        run_git(&source, &["show", &format!("{ref_name}:notes.txt")]),
        "the only copy",
        "the snapshot stored what a foreign filter rewrote, so the file's bytes are in no tree"
    );
    assert_eq!(
        safe_to_delete_worktree_after_snapshot(&worktree, Some(&source), ref_name),
        Safety::Delete
    );
}
