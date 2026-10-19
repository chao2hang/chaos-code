use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use zcode_rpc_protocol::RpcValue;

const STATUS_LIMIT: usize = 512 * 1024;
const DIFF_LIMIT: usize = 1024 * 1024;
const UNTRACKED_STAT_LIMIT: usize = 1024 * 1024;

#[derive(Default)]
struct Status {
    branch: Option<String>,
    upstream: Option<String>,
    head_type: &'static str,
    ahead: u64,
    behind: u64,
    entries: Vec<Entry>,
}

struct Entry {
    path: String,
    original: Option<String>,
    kind: &'static str,
    x: Option<char>,
    y: Option<char>,
    untracked: bool,
    conflicted: bool,
}

#[derive(Clone, Default)]
struct Stat {
    added: u64,
    removed: u64,
    kind: Option<&'static str>,
    original: Option<String>,
}

struct Repo {
    workspace: PathBuf,
    root: PathBuf,
    prefix: String,
    git_dir: PathBuf,
    common_dir: PathBuf,
    status: Status,
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn run(cwd: &Path, args: &[String]) -> Result<Output, String> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_PAGER", "cat")
        .env("PAGER", "cat")
        .env("TERM", "dumb")
        .env("LC_ALL", "C")
        .env("LANG", "C");
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_PREFIX",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        command.env_remove(name);
    }
    command
        .output()
        .map_err(|error| format!("无法启动 git: {error}"))
}

fn git_error(args: &[String], output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("exit status {}", output.status)
    };
    format!("git {} failed: {detail}", args.join(" "))
}

fn checked(cwd: &Path, args: &[String]) -> Result<Output, String> {
    let output = run(cwd, args)?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(git_error(args, &output))
    }
}

fn git_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn workspace_prefix(prefix: &str) -> String {
    let prefix = git_path(prefix);
    let prefix = prefix.trim_matches('/').trim_start_matches("./");
    if prefix.is_empty() || prefix == "." {
        ".".to_owned()
    } else {
        prefix.to_owned()
    }
}

fn workspace_relative(path: &str, prefix: &str) -> Option<String> {
    let path = git_path(path);
    let path = path.trim_start_matches("./");
    let prefix = workspace_prefix(prefix);
    if prefix == "." {
        return Some(path.to_owned());
    }
    if path == prefix {
        return Some(".".to_owned());
    }
    path.strip_prefix(&format!("{prefix}/")).map(str::to_owned)
}

fn in_scope(path: &str, original: Option<&str>, prefix: &str) -> bool {
    workspace_relative(path, prefix).is_some()
        || original.is_some_and(|path| workspace_relative(path, prefix).is_some())
}

fn kind_from_status(xy: &str) -> &'static str {
    match xy.chars().find(|value| *value != '.').unwrap_or('M') {
        'A' | '?' => "added",
        'D' => "deleted",
        'R' | 'C' => "renamed",
        _ => "modified",
    }
}

fn parse_ahead_behind(value: &str) -> (u64, u64) {
    let mut ahead = 0;
    let mut behind = 0;
    for item in value.split_whitespace() {
        if let Some(count) = item.strip_prefix('+') {
            ahead = count.parse().unwrap_or(0);
        } else if let Some(count) = item.strip_prefix('-') {
            behind = count.parse().unwrap_or(0);
        }
    }
    (ahead, behind)
}

fn parse_status(stdout: &str) -> Status {
    let records: Vec<&str> = stdout
        .split('\0')
        .filter(|record| !record.is_empty())
        .collect();
    let mut status = Status {
        head_type: "branch",
        ..Status::default()
    };
    let mut index = 0;
    while index < records.len() {
        let record = records[index];
        index += 1;
        if let Some(head) = record.strip_prefix("# branch.head ") {
            if head == "(detached)" {
                status.branch = None;
                status.head_type = "detached";
            } else {
                status.branch = Some(head.to_owned());
                status.head_type = "branch";
            }
        } else if let Some(upstream) = record.strip_prefix("# branch.upstream ") {
            status.upstream = Some(upstream.to_owned());
        } else if let Some(counts) = record.strip_prefix("# branch.ab ") {
            (status.ahead, status.behind) = parse_ahead_behind(counts);
        } else if let Some(path) = record.strip_prefix("? ") {
            status.entries.push(Entry {
                path: git_path(path),
                original: None,
                kind: "added",
                x: None,
                y: Some('?'),
                untracked: true,
                conflicted: false,
            });
        } else if record.starts_with("1 ") {
            let fields: Vec<&str> = record.splitn(9, ' ').collect();
            if fields.len() == 9 && fields[1].len() == 2 {
                let xy = fields[1];
                status.entries.push(Entry {
                    path: git_path(fields[8]),
                    original: None,
                    kind: kind_from_status(xy),
                    x: xy.chars().next(),
                    y: xy.chars().nth(1),
                    untracked: false,
                    conflicted: false,
                });
            }
        } else if record.starts_with("2 ") {
            let fields: Vec<&str> = record.splitn(10, ' ').collect();
            if fields.len() == 10 && fields[1].len() == 2 {
                let original = records.get(index).map(|path| git_path(path));
                index += 1;
                let xy = fields[1];
                status.entries.push(Entry {
                    path: git_path(fields[9]),
                    original,
                    kind: "renamed",
                    x: xy.chars().next(),
                    y: xy.chars().nth(1),
                    untracked: false,
                    conflicted: false,
                });
            }
        } else if record.starts_with("u ") {
            let fields: Vec<&str> = record.splitn(11, ' ').collect();
            if fields.len() == 11 && fields[1].len() == 2 {
                let xy = fields[1];
                status.entries.push(Entry {
                    path: git_path(fields[10]),
                    original: None,
                    kind: "modified",
                    x: xy.chars().next(),
                    y: xy.chars().nth(1),
                    untracked: false,
                    conflicted: true,
                });
            }
        }
    }
    status
}

fn status_output(root: &Path) -> Result<Output, String> {
    let detailed_args = args(&[
        "status",
        "--porcelain=v2",
        "--branch",
        "--untracked-files=all",
        "-z",
    ]);
    let detailed = run(root, &detailed_args)?;
    if !detailed.status.success() {
        return Err(git_error(&detailed_args, &detailed));
    }
    if detailed.stdout.len() <= STATUS_LIMIT {
        return Ok(detailed);
    }
    let collapsed_args = args(&[
        "status",
        "--porcelain=v2",
        "--branch",
        "--untracked-files=normal",
        "-z",
    ]);
    let collapsed = run(root, &collapsed_args)?;
    if collapsed.status.success() {
        Ok(collapsed)
    } else {
        Err(git_error(&collapsed_args, &collapsed))
    }
}

fn resolve(workspace: PathBuf) -> Result<(bool, Option<Repo>), String> {
    let available = matches!(
        Command::new("git").arg("--version").output(),
        Ok(output) if output.status.success()
    );
    if !available {
        return Ok((false, None));
    }
    let resolve_args = args(&[
        "rev-parse",
        "--show-toplevel",
        "--show-prefix",
        "--absolute-git-dir",
        "--git-common-dir",
    ]);
    let output = run(&workspace, &resolve_args)?;
    if !output.status.success() {
        return Ok((true, None));
    }
    let lines: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_owned())
        .collect();
    let root = lines
        .first()
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "Failed to resolve Git repository root".to_owned())?;
    let prefix = workspace_prefix(lines.get(1).map(String::as_str).unwrap_or(""));
    let git_dir = lines
        .get(2)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(".git"));
    let common = lines.get(3).map(String::as_str).unwrap_or("");
    let common_dir = if common.is_empty() {
        git_dir.clone()
    } else {
        let path = PathBuf::from(common);
        if path.is_absolute() {
            path
        } else {
            workspace.join(path)
        }
    };
    let status = status_output(&root)?;
    let status = parse_status(&String::from_utf8_lossy(&status.stdout));
    Ok((
        true,
        Some(Repo {
            workspace,
            root,
            prefix,
            git_dir,
            common_dir,
            status,
        }),
    ))
}

fn request_workspace(request: &Value) -> Result<PathBuf, String> {
    let path = request
        .get("workspacePath")
        .and_then(Value::as_str)
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from);
    let path = match path {
        Some(path) => path,
        None => std::env::current_dir().map_err(|error| error.to_string())?,
    };
    Ok(std::fs::canonicalize(&path).unwrap_or(path))
}

fn summary(repo: &Repo) -> Value {
    let git_dir = repo.git_dir.to_string_lossy().into_owned();
    let common_dir = repo.common_dir.to_string_lossy().into_owned();
    let mut watches = vec![json!({ "path": git_dir, "recursive": true })];
    if common_dir != git_dir {
        watches.push(json!({ "path": common_dir, "recursive": true }));
    }
    json!({
        "workspacePath": repo.workspace.to_string_lossy(),
        "repoRoot": repo.root.to_string_lossy(),
        "workspaceInRepoPath": repo.prefix,
        "autoRefreshWatchPaths": watches,
        "branchName": repo.status.branch.clone(),
        "trackingBranchName": repo.status.upstream.clone(),
        "headRefType": repo.status.head_type,
        "ahead": repo.status.ahead,
        "behind": repo.status.behind,
        "isDirty": !repo.status.entries.is_empty(),
        "isGitAvailable": true,
        "isRepository": true,
    })
}

fn empty_summary(workspace: &Path, available: bool) -> Value {
    let path = workspace.to_string_lossy();
    json!({
        "workspacePath": path,
        "repoRoot": path,
        "workspaceInRepoPath": ".",
        "autoRefreshWatchPaths": [],
        "branchName": null,
        "trackingBranchName": null,
        "headRefType": "branch",
        "ahead": 0,
        "behind": 0,
        "isDirty": false,
        "isGitAvailable": available,
        "isRepository": false,
    })
}

fn numstat_value(value: &str) -> u64 {
    value.parse().unwrap_or(0)
}

fn parse_numstat(stdout: &str) -> HashMap<String, Stat> {
    let records: Vec<&str> = stdout.split('\0').collect();
    let mut stats = HashMap::new();
    let mut index = 0;
    while index < records.len() {
        let record = records[index];
        index += 1;
        if record.is_empty() {
            continue;
        }
        let fields: Vec<&str> = record.split('\t').collect();
        if fields.len() < 3 {
            continue;
        }
        let added = numstat_value(fields[0]);
        let removed = numstat_value(fields[1]);
        let path = fields[2..].join("\t");
        if !path.is_empty() {
            stats.insert(
                git_path(&path),
                Stat {
                    added,
                    removed,
                    ..Stat::default()
                },
            );
            continue;
        }
        let original = records.get(index).copied().unwrap_or_default();
        let renamed = records.get(index + 1).copied().unwrap_or_default();
        index += 2;
        if !renamed.is_empty() {
            stats.insert(
                git_path(renamed),
                Stat {
                    added,
                    removed,
                    kind: Some("renamed"),
                    original: Some(git_path(original)),
                },
            );
        }
    }
    stats
}

fn untracked_lines(repo: &Repo, path: &str) -> u64 {
    let path = repo.root.join(path);
    let Ok(metadata) = std::fs::metadata(&path) else {
        return 0;
    };
    if !metadata.is_file() || metadata.len() as usize > UNTRACKED_STAT_LIMIT {
        return 0;
    }
    let Ok(bytes) = std::fs::read(path) else {
        return 0;
    };
    if bytes.contains(&0) {
        return 0;
    }
    bytes.iter().filter(|byte| **byte == b'\n').count() as u64
        + if bytes.last().is_some_and(|byte| *byte != b'\n') {
            1
        } else {
            0
        }
}

fn change(
    repo: &Repo,
    path: &str,
    section: &str,
    kind: &str,
    x: Option<char>,
    y: Option<char>,
    stat: &Stat,
    untracked: bool,
    conflicted: bool,
) -> Value {
    let path = git_path(path);
    let workspace_path = workspace_relative(&path, &repo.prefix).unwrap_or_else(|| path.clone());
    let native_path = path.replace('/', std::path::MAIN_SEPARATOR_STR);
    let absolute = repo.root.join(native_path);
    let mut value = json!({
        "path": absolute.to_string_lossy(),
        "repoRelativePath": path,
        "workspaceRelativePath": workspace_path,
        "kind": stat.kind.unwrap_or(kind),
        "section": section,
        "added": stat.added,
        "removed": stat.removed,
        "isStaged": section == "staged",
        "isUntracked": untracked,
        "isConflicted": conflicted,
    });
    if let Some(x) = x {
        value["x"] = json!(x.to_string());
    }
    if let Some(y) = y {
        value["y"] = json!(y.to_string());
    }
    value
}

fn worktree_changes(repo: &Repo) -> Result<(Vec<Value>, Vec<Value>), String> {
    let unstaged_args = args(&["diff", "--numstat", "-z", "--find-renames", "--"]);
    let staged_args = args(&[
        "diff",
        "--cached",
        "--numstat",
        "-z",
        "--find-renames",
        "--",
    ]);
    let unstaged = checked(&repo.root, &unstaged_args)?;
    let staged = checked(&repo.root, &staged_args)?;
    let unstaged_stats = parse_numstat(&String::from_utf8_lossy(&unstaged.stdout));
    let staged_stats = parse_numstat(&String::from_utf8_lossy(&staged.stdout));
    let mut unstaged_changes = Vec::new();
    let mut staged_changes = Vec::new();
    for entry in &repo.status.entries {
        if !in_scope(&entry.path, entry.original.as_deref(), &repo.prefix) {
            continue;
        }
        if entry.conflicted {
            let stat = unstaged_stats.get(&entry.path).cloned().unwrap_or_default();
            unstaged_changes.push(change(
                repo,
                &entry.path,
                "conflicted",
                entry.kind,
                entry.x,
                entry.y,
                &stat,
                false,
                true,
            ));
            continue;
        }
        if entry.untracked {
            let stat = Stat {
                added: untracked_lines(repo, &entry.path),
                ..Stat::default()
            };
            if entry.path.ends_with('/') || stat.added > 0 || stat.removed > 0 {
                unstaged_changes.push(change(
                    repo,
                    &entry.path,
                    "untracked",
                    entry.kind,
                    entry.x,
                    entry.y,
                    &stat,
                    true,
                    false,
                ));
            }
            continue;
        }
        if entry.x.is_some_and(|code| code != '.') {
            let stat = staged_stats.get(&entry.path).cloned().unwrap_or_default();
            staged_changes.push(change(
                repo,
                &entry.path,
                "staged",
                entry.kind,
                entry.x,
                entry.y,
                &stat,
                false,
                false,
            ));
        }
        if entry.y.is_some_and(|code| code != '.') {
            let stat = unstaged_stats.get(&entry.path).cloned().unwrap_or_default();
            unstaged_changes.push(change(
                repo,
                &entry.path,
                "unstaged",
                entry.kind,
                entry.x,
                entry.y,
                &stat,
                false,
                false,
            ));
        }
    }
    Ok((unstaged_changes, staged_changes))
}

#[derive(Default)]
struct ConfigValue {
    scope: Option<String>,
    source: Option<String>,
    value: Option<String>,
}

fn config_value(repo: &Repo, key: &str) -> Result<ConfigValue, String> {
    let args = vec![
        "config".to_owned(),
        "--show-scope".to_owned(),
        "--show-origin".to_owned(),
        "--get".to_owned(),
        key.to_owned(),
    ];
    let output = run(&repo.root, &args)?;
    if output.status.code() == Some(1) {
        return Ok(ConfigValue::default());
    }
    if !output.status.success() {
        return Err(git_error(&args, &output));
    }
    let line = String::from_utf8_lossy(&output.stdout)
        .trim_end_matches(['\r', '\n'])
        .to_owned();
    let mut fields = line.splitn(3, '\t');
    let scope = fields.next().unwrap_or_default();
    match (fields.next(), fields.next()) {
        (Some(source), Some(value)) => Ok(ConfigValue {
            scope: Some(scope.to_owned()),
            source: Some(source.to_owned()),
            value: Some(value.to_owned()),
        }),
        _ => Ok(ConfigValue {
            value: (!line.is_empty()).then_some(line),
            ..ConfigValue::default()
        }),
    }
}

fn identity(repo: &Repo) -> Result<Value, String> {
    let name = config_value(repo, "user.name")?;
    let email = config_value(repo, "user.email")?;
    Ok(json!({
        "userName": name.value,
        "userEmail": email.value,
        "nameSource": name.source,
        "emailSource": email.source,
        "scopeLabel": name.scope.or(email.scope),
    }))
}

fn branch_comparison(repo: &Repo) -> Result<Value, String> {
    let mut changes = Vec::new();
    if let Some(upstream) = repo.status.upstream.as_deref() {
        let args = vec![
            "diff".to_owned(),
            "--numstat".to_owned(),
            "-z".to_owned(),
            "--find-renames".to_owned(),
            format!("{upstream}...HEAD"),
            "--".to_owned(),
        ];
        let output = checked(&repo.root, &args)?;
        let stats = parse_numstat(&String::from_utf8_lossy(&output.stdout));
        let mut paths: Vec<&String> = stats.keys().collect();
        paths.sort();
        for path in paths {
            let stat = &stats[path];
            if !in_scope(path, stat.original.as_deref(), &repo.prefix) {
                continue;
            }
            let kind = stat.kind.unwrap_or({
                if stat.added > 0 && stat.removed == 0 {
                    "added"
                } else if stat.removed > 0 && stat.added == 0 {
                    "deleted"
                } else {
                    "modified"
                }
            });
            changes.push(change(
                repo, path, "branch", kind, None, None, stat, false, false,
            ));
        }
    }
    let head = repo.status.branch.as_deref().unwrap_or("HEAD");
    let label = repo
        .status
        .upstream
        .as_deref()
        .map(|upstream| format!("{head} -> {upstream}"));
    Ok(json!({
        "baseRef": repo.status.upstream,
        "headRef": head,
        "comparisonLabel": label,
        "changes": changes,
    }))
}

pub(super) fn refresh(arg: &RpcValue) -> Result<RpcValue, String> {
    let request = super::arg_json(arg).unwrap_or_else(|| json!({}));
    let workspace = request_workspace(&request)?;
    let include_identity = request
        .get("includeIdentity")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let include_branch = request
        .get("includeBranchComparison")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let (git_available, repo) = resolve(workspace.clone())?;
    let Some(repo) = repo else {
        let branch = if include_branch {
            json!({ "baseRef": null, "headRef": "HEAD", "comparisonLabel": null, "changes": [] })
        } else {
            Value::Null
        };
        let empty_identity = json!({
            "userName": null,
            "userEmail": null,
            "nameSource": null,
            "emailSource": null,
            "scopeLabel": null,
        });
        return Ok(RpcValue::Json(json!({
            "summary": empty_summary(&workspace, git_available),
            "identity": if include_identity { empty_identity } else { Value::Null },
            "unstagedChanges": [],
            "stagedChanges": [],
            "branchComparison": branch,
        })));
    };
    let id = if include_identity {
        identity(&repo)?
    } else {
        Value::Null
    };
    let branch = if include_branch {
        branch_comparison(&repo)?
    } else {
        Value::Null
    };
    let (unstaged, staged) = worktree_changes(&repo)?;
    Ok(RpcValue::Json(json!({
        "summary": summary(&repo),
        "identity": id,
        "unstagedChanges": unstaged,
        "stagedChanges": staged,
        "branchComparison": branch,
    })))
}

fn unavailable_diff(path: &str, reason: &str) -> Value {
    json!({
        "path": path,
        "availability": "unavailable",
        "patch": null,
        "beforeContent": null,
        "afterContent": null,
        "summary": reason,
    })
}

fn diff_result(path: &str, output: Output, allow_exit_one: bool, no_diff: &str) -> Value {
    if !output.status.success() && !(allow_exit_one && output.status.code() == Some(1)) {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let reason = if !stderr.is_empty() { stderr } else { stdout };
        return unavailable_diff(
            path,
            if reason.is_empty() {
                "git diff failed"
            } else {
                &reason
            },
        );
    }
    if output.stdout.len() > DIFF_LIMIT {
        return json!({
            "path": path,
            "availability": "truncated",
            "patch": null,
            "beforeContent": null,
            "afterContent": null,
            "summary": "Git diff output exceeded the preview limit.",
        });
    }
    let patch = String::from_utf8_lossy(&output.stdout).into_owned();
    if patch.trim().is_empty() {
        return unavailable_diff(path, no_diff);
    }
    if patch.contains("GIT binary patch") || patch.contains("Binary files ") {
        return json!({
            "path": path,
            "availability": "binary",
            "patch": null,
            "beforeContent": null,
            "afterContent": null,
            "summary": "Binary diff is not previewable.",
        });
    }
    json!({
        "path": path,
        "availability": "patch",
        "patch": patch,
        "beforeContent": null,
        "afterContent": null,
        "summary": null,
    })
}

fn resolve_diff_path(repo: &Repo, raw: &str) -> Result<(PathBuf, String), String> {
    let path = PathBuf::from(raw);
    let path = if path.is_absolute() {
        path
    } else {
        repo.workspace.join(path)
    };
    let absolute = std::fs::canonicalize(&path).unwrap_or(path);
    let relative = absolute
        .strip_prefix(&repo.root)
        .map_err(|_| format!("Path is outside repository scope: {raw}"))?;
    if relative.as_os_str().is_empty() {
        return Err(format!("Path is outside repository scope: {raw}"));
    }
    let repository_path = git_path(&relative.to_string_lossy());
    Ok((absolute, repository_path))
}

// ===========================================================================
// 官方壳的完整 IGitService 合同：分支管理、提交图、暂存、提交、推送。
// 每个响应的字段名与 @zcode/shared 的 Git* 类型一致（camelCase、可空字段）。
// =======================================================================

fn run_string(cwd: &Path, args: &[String]) -> Result<String, String> {
    let output = checked(cwd, args)?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `git check-ref-format --branch` 通过的名字才算合法分支名；失败原因原样带回。
fn invalid_branch_issue(cwd: &Path, branch_name: &str) -> Option<Value> {
    let check = args(&["check-ref-format", "--branch"]);
    let mut check = check;
    check.push(branch_name.to_owned());
    match run(cwd, &check) {
        Ok(output) if output.status.success() => None,
        Ok(output) => {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            Some(json!({
                "code": "invalid-branch-name",
                "message": "Branch name is invalid.",
                "detail": if detail.is_empty() { None } else { Some(detail) },
            }))
        }
        Err(error) => Some(json!({
            "code": "invalid-branch-name",
            "message": "Branch name is invalid.",
            "detail": error,
        })),
    }
}

/// merge/rebase/cherry-pick 进行中的标记文件探测，与上游 GIT_OPERATION_MARKERS 一致。
fn operation_in_progress(repo: &Repo) -> bool {
    const MARKERS: &[&str] = &[
        "MERGE_HEAD",
        "rebase-merge",
        "rebase-apply",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "BISECT_LOG",
    ];
    let mut rev_args = vec!["rev-parse".to_owned()];
    for marker in MARKERS {
        rev_args.push("--git-path".to_owned());
        rev_args.push((*marker).to_owned());
    }
    let Ok(stdout) = run_string(&repo.root, &rev_args) else {
        return false;
    };
    stdout.lines().any(|line| {
        let line = line.trim();
        !line.is_empty() && repo.root.join(line).exists()
    })
}

/// 把 `git switch` 的原生报错归一成稳定的 issue code，与上游 parseGitBranchMutationIssues 对齐。
fn branch_mutation_issues(output: &Output) -> Vec<Value> {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let detail = if !stderr.is_empty() { stderr } else { stdout };
    if detail.is_empty() {
        return vec![json!({
            "code": "unknown",
            "message": "Git operation failed.",
            "detail": null,
        })];
    }
    let normalized = detail.to_lowercase();
    let code = if normalized.contains("would be overwritten by checkout")
        || normalized.contains("would be overwritten by switch")
    {
        "tracked-changes-would-be-overwritten"
    } else if normalized.contains("untracked working tree files would be overwritten") {
        "untracked-changes-would-be-overwritten"
    } else if normalized.contains("already exists") {
        "branch-already-exists"
    } else if normalized.contains("invalid reference:") {
        "target-branch-not-found"
    } else if normalized.contains("is already used by worktree at") {
        "branch-in-other-worktree"
    } else if normalized.contains("resolve your current index first")
        || normalized.contains("unmerged files")
    {
        "conflicts-present"
    } else {
        "unknown"
    };
    vec![json!({
        "code": code,
        "message": "Git operation failed.",
        "detail": detail,
    })]
}

fn mutation_result(
    ok: bool,
    action: &str,
    branch_name: Option<&str>,
    did_change: bool,
    created: bool,
    repo: Option<&Repo>,
    workspace: &Path,
    git_available: bool,
    issues: Vec<Value>,
) -> Value {
    let summary = match repo {
        Some(repo) => summary(repo),
        None => empty_summary(workspace, git_available),
    };
    json!({
        "ok": ok,
        "action": action,
        "branchName": branch_name,
        "didChange": did_change,
        "created": created,
        "summary": summary,
        "issues": issues,
    })
}

/// 绝对路径（或相对 workspace 的路径）→ 仓库内相对路径。前端 GitFileChange.path 全是绝对路径。
fn normalize_repo_path(repo: &Repo, raw: &str) -> Result<String, String> {
    let (_, relative) = resolve_diff_path(repo, raw)?;
    Ok(relative)
}

fn repo_paths(repo: &Repo, paths: &[String]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for path in paths {
        let normalized = normalize_repo_path(repo, path)?;
        if !out.contains(&normalized) {
            out.push(normalized);
        }
    }
    Ok(out)
}

pub(super) fn get_repository_summary(arg: &RpcValue) -> Result<RpcValue, String> {
    let request = super::arg_json(arg).unwrap_or_else(|| json!({}));
    let workspace = request_workspace(&request)?;
    let (available, repo) = resolve(workspace.clone())?;
    let value = match repo {
        Some(repo) => summary(&repo),
        None => empty_summary(&workspace, available),
    };
    Ok(RpcValue::Json(value))
}

pub(super) fn get_workspace_repository_info(arg: &RpcValue) -> Result<RpcValue, String> {
    let request = super::arg_json(arg).unwrap_or_else(|| json!({}));
    let workspace = request_workspace(&request)?;
    let (available, repo) = resolve(workspace.clone())?;
    let kind = match &repo {
        None => "not-repository",
        // `.git` 是文件 → linked worktree（或独立 git-dir）；目录 → 主工作树。
        Some(r) if r.git_dir != r.common_dir => "linked-worktree",
        Some(_) => "main-tree",
    };
    Ok(RpcValue::Json(json!({
        "workspacePath": workspace.to_string_lossy(),
        "kind": kind,
        "isGitAvailable": available,
    })))
}

pub(super) fn get_local_branches(arg: &RpcValue) -> Result<RpcValue, String> {
    let request = super::arg_json(arg).unwrap_or_else(|| json!({}));
    let workspace = request_workspace(&request)?;
    let (available, repo) = resolve(workspace.clone())?;
    let Some(repo) = repo else {
        return Ok(RpcValue::Json(json!({
            "headRefType": "branch",
            "currentBranchName": null,
            "branches": [],
        })));
    };
    let _ = available;
    let format = "%(refname:short)%00%(upstream:short)%00%(objectname)%00%(committerdate:unix)";
    let for_each = args(&["for-each-ref", "refs/heads", &format!("--format={format}")]);
    let stdout = run_string(&repo.root, &for_each)?;
    let current = repo.status.branch.clone();
    let head_type = repo.status.head_type;
    let mut branches: Vec<Value> = stdout
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let mut fields = line.split('\u{0}');
            let name = fields.next()?.trim();
            if name.is_empty() {
                return None;
            }
            let upstream = fields.next().unwrap_or_default().trim();
            let hash = fields.next().unwrap_or_default().trim();
            let timestamp = fields.next().unwrap_or_default().trim();
            let timestamp_ms = timestamp.parse::<u64>().ok().map(|seconds| seconds * 1000);
            json!({
                "name": name,
                "isCurrent": current.as_deref() == Some(name),
                "upstreamName": if upstream.is_empty() { None } else { Some(upstream) },
                "commitHash": if hash.is_empty() { None } else { Some(hash) },
                "commitTimestampMs": timestamp_ms,
            })
            .into()
        })
        .collect();
    // 与上游 parseBranchRefRecords 的排序一致：当前分支优先，然后按提交时间倒序，最后按名字。
    branches.sort_by(|left, right| {
        let left_current = left["isCurrent"].as_bool().unwrap_or(false);
        let right_current = right["isCurrent"].as_bool().unwrap_or(false);
        if left_current != right_current {
            return if left_current {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }
        let left_time = left["commitTimestampMs"].as_u64().unwrap_or(0);
        let right_time = right["commitTimestampMs"].as_u64().unwrap_or(0);
        right_time.cmp(&left_time).then_with(|| {
            left["name"]
                .as_str()
                .unwrap_or_default()
                .cmp(right["name"].as_str().unwrap_or_default())
        })
    });
    Ok(RpcValue::Json(json!({
        "headRefType": head_type,
        "currentBranchName": repo.status.branch,
        "branches": branches,
    })))
}

/// `%D` 装饰字段 → refs 数组，与上游 parseGitGraphRefs 的 kind 判定一致。
fn graph_refs(decorations: &str) -> Vec<Value> {
    let mut refs: Vec<Value> = Vec::new();
    let mut push = |name: &str, kind: &str| {
        if !refs
            .iter()
            .any(|r| r["kind"] == json!(kind) && r["name"] == json!(name))
        {
            refs.push(json!({ "name": name, "kind": kind }));
        }
    };
    for raw in decorations.split(',') {
        let decoration = raw.trim();
        if decoration.is_empty() {
            continue;
        }
        const HEAD_POINTER: &str = "HEAD -> ";
        if let Some(pointed) = decoration.strip_prefix(HEAD_POINTER) {
            push("HEAD", "head");
            parse_graph_decoration(pointed, &mut push);
            continue;
        }
        parse_graph_decoration(decoration, &mut push);
    }
    refs
}

fn parse_graph_decoration(raw: &str, push: &mut impl FnMut(&str, &str)) {
    let reference = raw.trim();
    if reference.is_empty() {
        return;
    }
    if reference == "HEAD" {
        push("HEAD", "head");
        return;
    }
    if let Some(tag) = reference.strip_prefix("tag: ") {
        let tag = tag.trim();
        let name = tag.strip_prefix("refs/tags/").unwrap_or(tag);
        if !name.is_empty() {
            push(name, "tag");
        }
        return;
    }
    if let Some(name) = reference.strip_prefix("refs/heads/") {
        if !name.is_empty() {
            push(name, "branch");
        }
        return;
    }
    if let Some(name) = reference.strip_prefix("refs/remotes/") {
        if !name.is_empty() {
            push(name, "remote");
        }
        return;
    }
    if let Some(name) = reference.strip_prefix("refs/tags/") {
        if !name.is_empty() {
            push(name, "tag");
        }
        return;
    }
    if reference.contains('/') {
        push(reference, "remote");
    } else {
        push(reference, "branch");
    }
}

pub(super) fn get_commit_graph(arg: &RpcValue) -> Result<RpcValue, String> {
    let request = super::arg_json(arg).unwrap_or_else(|| json!({}));
    let workspace = request_workspace(&request)?;
    let max_count = request
        .get("maxCount")
        .and_then(Value::as_u64)
        .filter(|count| *count >= 1)
        .unwrap_or(200)
        .min(500);
    let skip = request.get("skip").and_then(Value::as_u64).unwrap_or(0);
    let (available, repo) = resolve(workspace.clone())?;
    let Some(repo) = repo else {
        let _ = available;
        return Ok(RpcValue::Json(json!({ "commits": [], "hasMore": false })));
    };
    let log_args = vec![
        "log".to_owned(),
        "HEAD".to_owned(),
        "--branches".to_owned(),
        "--tags".to_owned(),
        "--remotes".to_owned(),
        "--date-order".to_owned(),
        "--topo-order".to_owned(),
        format!("--skip={skip}"),
        format!("--max-count={}", max_count + 1),
        "--format=%H%x00%P%x00%an%x00%at%x00%s%x00%D%x1e".to_owned(),
    ];
    let output = run(&repo.root, &log_args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
        // 空仓库（还没有任何提交）不是错误，与上游的宽容分支一致。
        if stderr.contains("does not have any commits yet")
            || stderr.contains("your current branch")
            || stderr.contains("bad default revision")
            || stderr.contains("ambiguous argument 'head'")
        {
            return Ok(RpcValue::Json(json!({ "commits": [], "hasMore": false })));
        }
        return Err(git_error(&log_args, &output));
    }
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let commits: Vec<Value> = stdout
        .split('\u{1e}')
        .map(str::trim)
        .filter(|record| !record.is_empty())
        .filter_map(|record| {
            let mut fields = record.split('\u{0}');
            let hash = fields.next()?.trim();
            if hash.is_empty() {
                return None;
            }
            let parents_raw = fields.next().unwrap_or_default().trim();
            let author = fields.next().unwrap_or_default().trim();
            let authored_at = fields.next().unwrap_or_default().trim();
            let subject = fields.next().unwrap_or_default();
            let decorations = fields.next().unwrap_or_default().trim();
            let parents: Vec<String> = parents_raw.split_whitespace().map(str::to_owned).collect();
            json!({
                "hash": hash,
                "parents": parents,
                "refs": graph_refs(decorations),
                "subject": subject,
                "authorName": if author.is_empty() { None } else { Some(author) },
                "authoredAtMs": authored_at.parse::<u64>().ok().map(|seconds| seconds * 1000),
            })
            .into()
        })
        .collect();
    let has_more = commits.len() > max_count as usize;
    Ok(RpcValue::Json(json!({
        "commits": commits.into_iter().take(max_count as usize).collect::<Vec<_>>(),
        "hasMore": has_more,
    })))
}

pub(super) fn get_branch_comparison(arg: &RpcValue) -> Result<RpcValue, String> {
    let request = super::arg_json(arg).unwrap_or_else(|| json!({}));
    let workspace = request_workspace(&request)?;
    let (_, repo) = resolve(workspace)?;
    let Some(repo) = repo else {
        return Ok(RpcValue::Json(json!({
            "baseRef": null,
            "headRef": "HEAD",
            "comparisonLabel": null,
            "changes": [],
        })));
    };
    Ok(RpcValue::Json(branch_comparison(&repo)?))
}

pub(super) fn get_changes(arg: &RpcValue) -> Result<RpcValue, String> {
    let request = super::arg_json(arg).unwrap_or_else(|| json!({}));
    let workspace = request_workspace(&request)?;
    let (_, repo) = resolve(workspace)?;
    let Some(repo) = repo else {
        return Ok(RpcValue::Json(json!([])));
    };
    let (unstaged, staged) = worktree_changes(&repo)?;
    let source = request
        .get("sourceId")
        .and_then(Value::as_str)
        .unwrap_or("unstaged");
    let changes = if source == "staged" { staged } else { unstaged };
    Ok(RpcValue::Json(json!(changes)))
}

pub(super) fn get_ignored_paths(arg: &RpcValue) -> Result<RpcValue, String> {
    let request = super::arg_json(arg).unwrap_or_else(|| json!({}));
    let workspace = request_workspace(&request)?;
    let inputs: Vec<String> = request
        .get("paths")
        .and_then(Value::as_array)
        .map(|paths| {
            paths
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    if inputs.is_empty() {
        return Ok(RpcValue::Json(json!([])));
    }
    let (_, repo) = resolve(workspace)?;
    let Some(repo) = repo else {
        return Ok(RpcValue::Json(json!([])));
    };
    let mut repo_paths = Vec::new();
    let mut absolute_by_relative: HashMap<String, String> = HashMap::new();
    for input in &inputs {
        let Ok(relative) = normalize_repo_path(&repo, input) else {
            continue;
        };
        if !absolute_by_relative.contains_key(&relative) {
            repo_paths.push(relative.clone());
            absolute_by_relative.insert(relative.clone(), input.clone());
        }
    }
    if repo_paths.is_empty() {
        return Ok(RpcValue::Json(json!([])));
    }
    let mut check_args = vec!["check-ignore".to_owned(), "--".to_owned()];
    check_args.extend(repo_paths.iter().cloned());
    let output = run(&repo.root, &check_args)?;
    // exit 1 = 没有任何路径被忽略；这不是错误。
    if output.status.code() == Some(1) {
        return Ok(RpcValue::Json(json!([])));
    }
    if !output.status.success() {
        return Err(git_error(&check_args, &output));
    }
    let ignored: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| absolute_by_relative.get(&*git_path(line.trim())).cloned())
        .collect();
    Ok(RpcValue::Json(json!(ignored)))
}

pub(super) fn get_identity(arg: &RpcValue) -> Result<RpcValue, String> {
    let request = super::arg_json(arg).unwrap_or_else(|| json!({}));
    let workspace = request_workspace(&request)?;
    let (_, repo) = resolve(workspace)?;
    let Some(repo) = repo else {
        return Ok(RpcValue::Json(json!({
            "userName": null,
            "userEmail": null,
            "nameSource": null,
            "emailSource": null,
            "scopeLabel": null,
        })));
    };
    Ok(RpcValue::Json(identity(&repo)?))
}

/// switch / create-and-switch 的公共实现：前置校验 → `git switch` → 回读 summary。
fn switch_branch_impl(
    arg: &RpcValue,
    action: &str,
    create: bool,
    branch_key: &str,
) -> Result<RpcValue, String> {
    let request = super::arg_json(arg).unwrap_or_else(|| json!({}));
    let workspace = request_workspace(&request)?;
    let target = request
        .get(branch_key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| format!("git.{action} 需要 {branch_key}"))?;
    let start_point = request
        .get("startPoint")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|point| !point.is_empty());
    let (available, repo) = resolve(workspace.clone())?;
    let Some(repo) = repo else {
        return Ok(RpcValue::Json(mutation_result(
            false,
            action,
            None,
            false,
            false,
            None,
            &workspace,
            available,
            vec![json!({
                "code": "target-branch-not-found",
                "message": "Workspace is not inside a Git repository.",
                "detail": null,
            })],
        )));
    };
    let branch_name_owned = target.to_owned();
    if let Some(issue) = invalid_branch_issue(&repo.root, &branch_name_owned) {
        return Ok(RpcValue::Json(mutation_result(
            false,
            action,
            Some(&branch_name_owned),
            false,
            false,
            Some(&repo),
            &workspace,
            true,
            vec![issue],
        )));
    }
    // 已在目标分支上：直接成功，不动工作树。
    if !create && repo.status.head_type == "branch" && repo.status.branch.as_deref() == Some(target)
    {
        return Ok(RpcValue::Json(mutation_result(
            true,
            action,
            Some(&branch_name_owned),
            false,
            false,
            Some(&repo),
            &workspace,
            true,
            Vec::new(),
        )));
    }
    if repo.status.entries.iter().any(|entry| entry.conflicted) {
        return Ok(RpcValue::Json(mutation_result(
            false,
            action,
            Some(&branch_name_owned),
            false,
            false,
            Some(&repo),
            &workspace,
            true,
            vec![json!({
                "code": "conflicts-present",
                "message": "Repository still has unresolved conflicts.",
                "detail": null,
            })],
        )));
    }
    if operation_in_progress(&repo) {
        return Ok(RpcValue::Json(mutation_result(
            false,
            action,
            Some(&branch_name_owned),
            false,
            false,
            Some(&repo),
            &workspace,
            true,
            vec![json!({
                "code": "operation-in-progress",
                "message": "Another Git operation is still in progress.",
                "detail": null,
            })],
        )));
    }
    let mut switch_args = vec!["switch".to_owned(), "--no-guess".to_owned()];
    if create {
        switch_args.push("-c".to_owned());
        switch_args.push(branch_name_owned.clone());
        if let Some(point) = start_point {
            switch_args.push("--".to_owned());
            switch_args.push(point.to_owned());
        }
    } else {
        switch_args.push(branch_name_owned.clone());
    }
    let output = run(&repo.root, &switch_args)?;
    if !output.status.success() {
        return Ok(RpcValue::Json(mutation_result(
            false,
            action,
            Some(&branch_name_owned),
            false,
            create,
            Some(&repo),
            &workspace,
            true,
            branch_mutation_issues(&output),
        )));
    }
    // 切换后重读一次状态，回给前端最新的 summary。
    let (_, refreshed) = resolve(workspace.clone())?;
    Ok(RpcValue::Json(mutation_result(
        true,
        action,
        Some(&branch_name_owned),
        true,
        create,
        refreshed.as_ref(),
        &workspace,
        true,
        Vec::new(),
    )))
}

pub(super) fn switch_branch(arg: &RpcValue) -> Result<RpcValue, String> {
    switch_branch_impl(arg, "switch", false, "targetBranchName")
}

pub(super) fn create_branch_and_switch(arg: &RpcValue) -> Result<RpcValue, String> {
    switch_branch_impl(arg, "create-and-switch", true, "branchName")
}

fn path_mutation(arg: &RpcValue, git_args: &[&str]) -> Result<RpcValue, String> {
    let request = super::arg_json(arg).unwrap_or_else(|| json!({}));
    let workspace = request_workspace(&request)?;
    let paths: Vec<String> = request
        .get("paths")
        .and_then(Value::as_array)
        .map(|paths| {
            paths
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    if paths.is_empty() {
        return Ok(RpcValue::Json(Value::Null));
    }
    let (_, repo) = resolve(workspace)?;
    let Some(repo) = repo else {
        return Err("Workspace is not inside a Git repository.".to_owned());
    };
    let repo_paths = repo_paths(&repo, &paths)?;
    if repo_paths.is_empty() {
        return Ok(RpcValue::Json(Value::Null));
    }
    let mut command: Vec<String> = git_args.iter().map(|value| (*value).to_owned()).collect();
    command.push("--".to_owned());
    command.extend(repo_paths);
    checked(&repo.root, &command)?;
    Ok(RpcValue::Json(Value::Null))
}

pub(super) fn stage_paths(arg: &RpcValue) -> Result<RpcValue, String> {
    path_mutation(arg, &["add"])
}

pub(super) fn unstage_paths(arg: &RpcValue) -> Result<RpcValue, String> {
    path_mutation(arg, &["restore", "--staged"])
}

pub(super) fn discard_paths(arg: &RpcValue) -> Result<RpcValue, String> {
    let request = super::arg_json(arg).unwrap_or_else(|| json!({}));
    let staged = request.get("staged").and_then(Value::as_bool) == Some(true);
    if staged {
        path_mutation(arg, &["restore", "--source=HEAD", "--staged", "--worktree"])
    } else {
        path_mutation(arg, &["restore", "--worktree"])
    }
}

pub(super) fn commit(arg: &RpcValue) -> Result<RpcValue, String> {
    let request = super::arg_json(arg).unwrap_or_else(|| json!({}));
    let workspace = request_workspace(&request)?;
    let message = request
        .get("message")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .ok_or_else(|| "git.commit 需要 message".to_owned())?;
    let staged_only = request.get("stagedOnly").and_then(Value::as_bool) == Some(true);
    let paths: Vec<String> = request
        .get("paths")
        .and_then(Value::as_array)
        .map(|paths| {
            paths
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let (_, repo) = resolve(workspace.clone())?;
    let Some(repo) = repo else {
        return Err("Workspace is not inside a Git repository.".to_owned());
    };
    let repo_paths = if staged_only {
        // stagedOnly 语义是「提交已暂存的内容」，路径列表只做范围提示；
        // Web 桥按仓库级 staged 提交，避免临时 index 的复杂度。
        Vec::new()
    } else {
        repo_paths(&repo, &paths)?
    };
    let mut command = vec!["commit".to_owned(), "-m".to_owned(), message.to_owned()];
    if !repo_paths.is_empty() {
        command.push("--".to_owned());
        command.extend(repo_paths);
    }
    checked(&repo.root, &command)?;
    let hash_args = args(&["rev-parse", "HEAD"]);
    let commit_hash = run_string(&repo.root, &hash_args)?.trim().to_owned();
    let (_, refreshed) = resolve(workspace.clone())?;
    let summary = refreshed.map(|repo| summary(&repo)).unwrap_or_else(|| {
        json!({
            "workspacePath": "",
            "repoRoot": "",
            "workspaceInRepoPath": ".",
            "autoRefreshWatchPaths": [],
            "branchName": null,
            "trackingBranchName": null,
            "headRefType": "branch",
            "ahead": 0,
            "behind": 0,
            "isDirty": false,
            "isGitAvailable": true,
            "isRepository": false,
        })
    });
    Ok(RpcValue::Json(json!({
        "commitHash": commit_hash,
        "summary": summary,
    })))
}

pub(super) fn push(arg: &RpcValue) -> Result<RpcValue, String> {
    let request = super::arg_json(arg).unwrap_or_else(|| json!({}));
    let workspace = request_workspace(&request)?;
    let (_, repo) = resolve(workspace.clone())?;
    let Some(repo) = repo else {
        return Err("Workspace is not inside a Git repository.".to_owned());
    };
    let branch_name = repo.status.branch.clone().unwrap_or_default();
    if repo.status.head_type != "branch" || branch_name.trim().is_empty() {
        return Err("Cannot push while HEAD is detached.".to_owned());
    }
    let has_tracking = repo.status.upstream.is_some();
    let push_args = if has_tracking {
        args(&["push"])
    } else {
        // 没有上游时取第一个远端（上游解析的常见形态是 origin），并建立跟踪。
        let remote = repo
            .status
            .upstream
            .clone()
            .and_then(|upstream| {
                upstream
                    .split('/')
                    .next()
                    .map(str::to_owned)
                    .filter(|name| !name.is_empty())
            })
            .or_else(|| {
                run_string(&repo.root, &args(&["remote"]))
                    .ok()
                    .and_then(|stdout| {
                        stdout
                            .lines()
                            .map(str::trim)
                            .find(|line| !line.is_empty())
                            .map(str::to_owned)
                    })
            })
            .unwrap_or_else(|| "origin".to_owned());
        args(&["push", "--set-upstream", &remote, &branch_name])
    };
    checked(&repo.root, &push_args)?;
    let (_, refreshed) = resolve(workspace.clone())?;
    let (summary_value, tracking) = match refreshed {
        Some(repo) => (summary(&repo), repo.status.upstream.clone()),
        None => (
            empty_summary(&workspace, true),
            repo.status.upstream.clone(),
        ),
    };
    let remote_name = tracking
        .as_deref()
        .and_then(|upstream| upstream.split('/').next())
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned);
    Ok(RpcValue::Json(json!({
        "branchName": branch_name,
        "trackingBranchName": tracking,
        "remoteName": remote_name,
        "setUpstream": !has_tracking,
        "summary": summary_value,
    })))
}

pub(super) fn get_diff(arg: &RpcValue) -> Result<RpcValue, String> {
    let request = super::arg_json(arg).unwrap_or_else(|| json!({}));
    let raw = request
        .get("path")
        .and_then(Value::as_str)
        .filter(|path| !path.trim().is_empty())
        .ok_or_else(|| "git.getDiff 需要 path".to_owned())?;
    let workspace = request_workspace(&request)?;
    let requested_path = PathBuf::from(raw);
    let requested_path = if requested_path.is_absolute() {
        requested_path
    } else {
        workspace.join(requested_path)
    };
    let requested_path = std::fs::canonicalize(&requested_path).unwrap_or(requested_path);
    let (available, repo) = resolve(workspace)?;
    let Some(repo) = repo else {
        let reason = if available {
            "Workspace is not inside a Git repository."
        } else {
            "Git binary is not available in the current environment."
        };
        return Ok(RpcValue::Json(unavailable_diff(
            &requested_path.to_string_lossy(),
            reason,
        )));
    };
    let (absolute, relative) = resolve_diff_path(&repo, raw)?;
    let path = absolute.to_string_lossy().into_owned();
    let source = request
        .get("sourceId")
        .and_then(Value::as_str)
        .unwrap_or("unstaged");
    let (command, allow_exit_one, no_diff) = if source == "branch" {
        let Some(upstream) = repo.status.upstream.as_deref() else {
            return Ok(RpcValue::Json(unavailable_diff(
                &path,
                "Current branch does not have an upstream branch.",
            )));
        };
        (
            vec![
                "diff".to_owned(),
                "--no-ext-diff".to_owned(),
                "--no-textconv".to_owned(),
                "--no-color".to_owned(),
                "--binary".to_owned(),
                "--find-renames".to_owned(),
                format!("{upstream}...HEAD"),
                "--".to_owned(),
                relative,
            ],
            false,
            "No branch comparison diff is available for this file.",
        )
    } else if source == "staged" || request.get("staged").and_then(Value::as_bool) == Some(true) {
        (
            vec![
                "diff".to_owned(),
                "--cached".to_owned(),
                "--no-ext-diff".to_owned(),
                "--no-textconv".to_owned(),
                "--no-color".to_owned(),
                "--binary".to_owned(),
                "--".to_owned(),
                relative,
            ],
            false,
            "No Git diff is available for this file.",
        )
    } else if repo
        .status
        .entries
        .iter()
        .any(|entry| entry.untracked && git_path(&entry.path) == relative)
    {
        let null_device = if cfg!(windows) { "NUL" } else { "/dev/null" };
        (
            vec![
                "diff".to_owned(),
                "--no-index".to_owned(),
                "--no-ext-diff".to_owned(),
                "--no-textconv".to_owned(),
                "--no-color".to_owned(),
                "--binary".to_owned(),
                null_device.to_owned(),
                requested_path.to_string_lossy().into_owned(),
            ],
            true,
            "No previewable diff is available for this file.",
        )
    } else {
        (
            vec![
                "diff".to_owned(),
                "--no-ext-diff".to_owned(),
                "--no-textconv".to_owned(),
                "--no-color".to_owned(),
                "--binary".to_owned(),
                "--".to_owned(),
                relative,
            ],
            false,
            "No Git diff is available for this file.",
        )
    };
    let output = run(&repo.root, &command)?;
    Ok(RpcValue::Json(diff_result(
        &path,
        output,
        allow_exit_one,
        no_diff,
    )))
}
