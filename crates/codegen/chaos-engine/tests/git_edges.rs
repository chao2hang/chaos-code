use chaos_engine::{ClientMessage, Engine, GitAdapter, ProcessGitAdapter, ServerMessage};
use std::process::Command;
use tempfile::tempdir;

fn git(cwd: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .unwrap()
}

fn adapter_for(path: &std::path::Path) -> ProcessGitAdapter {
    ProcessGitAdapter::new(path).unwrap()
}

#[test]
fn process_git_adapter_rejects_non_git_workspace_and_preserves_staged_state_on_validation_failure()
{
    let non_git = tempdir().unwrap();
    let adapter = adapter_for(non_git.path());
    std::fs::write(non_git.path().join("note.txt"), "hello").unwrap();
    assert!(adapter.stage("note.txt").is_err());

    let repository = tempdir().unwrap();
    assert!(git(repository.path(), &["init", "-q"]).status.success());
    std::fs::write(repository.path().join("note.txt"), "hello").unwrap();
    let adapter = adapter_for(repository.path());
    assert!(adapter.stage("note.txt").is_ok());
    assert!(adapter.commit("").is_err());
    let staged = git(repository.path(), &["diff", "--cached", "--name-only"]);
    assert_eq!(String::from_utf8_lossy(&staged.stdout).trim(), "note.txt");
}

#[test]
fn process_git_adapter_reports_detached_head_and_missing_branch_without_mutating_head() {
    let repository = tempdir().unwrap();
    assert!(git(repository.path(), &["init", "-q"]).status.success());
    assert!(
        git(repository.path(), &["config", "user.name", "Test User"])
            .status
            .success()
    );
    assert!(
        git(
            repository.path(),
            &["config", "user.email", "test@example.invalid"]
        )
        .status
        .success()
    );
    std::fs::write(repository.path().join("note.txt"), "hello").unwrap();
    assert!(
        git(repository.path(), &["add", "--", "note.txt"])
            .status
            .success()
    );
    assert!(
        git(repository.path(), &["commit", "-m", "initial"])
            .status
            .success()
    );
    assert!(
        git(repository.path(), &["checkout", "--detach", "HEAD"])
            .status
            .success()
    );

    let adapter = adapter_for(repository.path());
    assert!(adapter.checkout_branch("missing-branch").is_err());
    let head = git(repository.path(), &["symbolic-ref", "--short", "HEAD"]);
    assert!(!head.status.success());
    let abbrev = git(repository.path(), &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(String::from_utf8_lossy(&abbrev.stdout).trim(), "HEAD");
}

#[test]
fn engine_git_status_fails_closed_for_non_git_workspace() {
    let directory = tempdir().unwrap();
    let engine = Engine::with_workspace(directory.path()).unwrap();
    let result = engine.handle(ClientMessage::GetGitStatus {
        client_msg_id: "git-status".into(),
    });
    assert!(matches!(
        result.as_slice(),
        [ServerMessage::Error { code, .. }] if code == "git_failed"
    ));
}
