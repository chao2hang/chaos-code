use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;
use zcode_rpc_protocol::RpcValue;

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git starts");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn call_git(method: &str, request: Value) -> Value {
    let arg = RpcValue::Json(json!([request]));
    match xai_grok_web::chaos_bridge::handle_call("git", method, &arg)
        .unwrap_or_else(|error| panic!("git.{method}: {error}"))
    {
        RpcValue::Json(value) => value,
        other => panic!("git.{method} returned unexpected value: {other:?}"),
    }
}

fn contains_change(changes: &Value, path: &str, section: &str) -> bool {
    changes.as_array().is_some_and(|changes| {
        changes
            .iter()
            .any(|change| change["repoRelativePath"] == path && change["section"] == section)
    })
}

#[test]
fn refresh_returns_git_status_and_diff_data() {
    let workspace = tempfile::tempdir().expect("temporary repository");
    git(workspace.path(), &["init", "-q"]);
    git(workspace.path(), &["config", "user.name", "Bridge Test"]);
    git(
        workspace.path(),
        &["config", "user.email", "bridge-test@example.invalid"],
    );
    std::fs::write(workspace.path().join("tracked.txt"), "before\n").expect("seed file");
    git(workspace.path(), &["add", "tracked.txt"]);
    git(workspace.path(), &["commit", "-q", "-m", "initial"]);

    std::fs::write(workspace.path().join("tracked.txt"), "before\nafter\n")
        .expect("modify tracked file");
    std::fs::write(workspace.path().join("staged.txt"), "staged line\n")
        .expect("create staged file");
    git(workspace.path(), &["add", "staged.txt"]);
    std::fs::write(workspace.path().join("untracked.txt"), "untracked line\n")
        .expect("create untracked file");

    let workspace_path = workspace.path().to_string_lossy().into_owned();
    let result = call_git(
        "refresh",
        json!({
            "workspacePath": workspace_path,
            "includeIdentity": true,
            "includeBranchComparison": true,
        }),
    );
    assert!(result.is_object(), "refresh fulfilled with null: {result}");
    assert_eq!(result["summary"]["isRepository"], true);
    assert_eq!(result["summary"]["isGitAvailable"], true);
    assert_eq!(result["summary"]["isDirty"], true);
    assert_eq!(result["summary"]["workspaceInRepoPath"], ".");
    assert!(result["summary"]["autoRefreshWatchPaths"].is_array());
    assert_eq!(result["identity"]["userName"], "Bridge Test");
    assert_eq!(
        result["identity"]["userEmail"],
        "bridge-test@example.invalid"
    );
    assert!(result["branchComparison"]["changes"].is_array());
    assert!(contains_change(
        &result["unstagedChanges"],
        "tracked.txt",
        "unstaged"
    ));
    assert!(contains_change(
        &result["unstagedChanges"],
        "untracked.txt",
        "untracked"
    ));
    assert!(contains_change(
        &result["stagedChanges"],
        "staged.txt",
        "staged"
    ));

    let diff = call_git(
        "getDiff",
        json!({
            "workspacePath": workspace_path,
            "path": workspace.path().join("tracked.txt").to_string_lossy().into_owned(),
            "sourceId": "unstaged",
        }),
    );
    assert_eq!(diff["availability"], "patch");
    assert!(diff["patch"].as_str().unwrap().contains("+after"));

    let staged_diff = call_git(
        "getDiff",
        json!({
            "workspacePath": workspace_path,
            "path": workspace.path().join("staged.txt").to_string_lossy().into_owned(),
            "sourceId": "staged",
        }),
    );
    assert_eq!(staged_diff["availability"], "patch");
    assert!(
        staged_diff["patch"]
            .as_str()
            .unwrap()
            .contains("+staged line")
    );

    let untracked_diff = call_git(
        "getDiff",
        json!({
            "workspacePath": workspace.path().to_string_lossy().into_owned(),
            "path": workspace.path().join("untracked.txt").to_string_lossy().into_owned(),
            "sourceId": "unstaged",
        }),
    );
    assert_eq!(untracked_diff["availability"], "patch");
    assert!(
        untracked_diff["patch"]
            .as_str()
            .unwrap()
            .contains("+untracked line")
    );
}

#[test]
fn refresh_returns_a_valid_empty_contract_outside_git() {
    let workspace = tempfile::tempdir().expect("temporary workspace");
    let result = call_git(
        "refresh",
        json!({
            "workspacePath": workspace.path().to_string_lossy().into_owned(),
            "includeIdentity": true,
            "includeBranchComparison": true,
        }),
    );
    assert!(result.is_object(), "refresh must not fulfill with null");
    assert_eq!(result["summary"]["isRepository"], false);
    assert!(result["summary"]["isGitAvailable"].is_boolean());
    assert_eq!(result["unstagedChanges"], json!([]));
    assert_eq!(result["stagedChanges"], json!([]));
    assert_eq!(result["identity"]["userName"], Value::Null);
    assert_eq!(result["branchComparison"]["headRef"], "HEAD");
    assert_eq!(result["branchComparison"]["changes"], json!([]));
}
