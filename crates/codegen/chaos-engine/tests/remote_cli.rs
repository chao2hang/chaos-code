//! The two shipped binaries driven against each other from a command line.
//!
//! `tests/remote_workspace.rs` drives the client *library* against the server
//! binary. This drives the client **binary**, because the thing a release is judged
//! on is `chaos-remote … list` working from a shell — including the argument
//! parsing, the exit status, and the credential file being rewritten so that the
//! next command in a script still has something to spend.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use chaos_engine::remote::{PROTOCOL_VERSION, RemoteCapability, TokenFile};
use xai_tty_utils::ProcessScope;

const SERVER: &str = env!("CARGO_BIN_EXE_chaos-remote-server");
const CLIENT: &str = env!("CARGO_BIN_EXE_chaos-remote");

/// A started server plus the three things a client invocation needs.
struct Lab {
    /// Kept so the directory outlives the server writing into it.
    _root: tempfile::TempDir,
    socket: PathBuf,
    token_file: PathBuf,
    workspace: PathBuf,
    child: tokio::process::Child,
    group: std::sync::Arc<xai_tty_utils::ProcessGroup>,
    /// Kept alive so the scope does not reap the server while the test runs.
    _scope: xai_tty_utils::ProcessScope,
}

impl Lab {
    /// A server with `echo` and `false` allowlisted, over a Unix socket, serving a
    /// git workspace that already has one commit and one uncommitted change.
    async fn start() -> Lab {
        let root = tempfile::tempdir().expect("tempdir");
        let workspace = root.path().join("work");
        std::fs::create_dir_all(workspace.join("src")).expect("src");
        std::fs::write(
            workspace.join("src/main.rs"),
            "fn main() {\n    count_beans(3);\n}\n",
        )
        .expect("main.rs");
        std::fs::write(workspace.join("README.md"), "# beans\n").expect("readme");
        git(&workspace, &["init", "-q", "."]);
        git(&workspace, &["config", "user.email", "lab@example.invalid"]);
        git(&workspace, &["config", "user.name", "Lab"]);
        git(&workspace, &["add", "-A"]);
        git(&workspace, &["commit", "-q", "-m", "beans"]);
        // One change waiting for `diff`, without the test having to write through
        // the session first.
        std::fs::write(
            workspace.join("README.md"),
            "# beans\n\nNow with lentils.\n",
        )
        .expect("dirty readme");

        let socket = short_socket_dir("c").join("s.sock");
        let token_file = root.path().join("tokens");
        let mut command = tokio::process::Command::new(SERVER);
        command
            .args([
                "--workspace",
                workspace.to_str().unwrap(),
                "--token-file",
                token_file.to_str().unwrap(),
                "--unix",
                socket.to_str().unwrap(),
                "--allow",
                "echo",
                "--allow",
                "false",
                "--capability",
                "tool-execution",
                "--tokens",
                "32",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let scope = ProcessScope::new();
        let (child, group) = scope.spawn(command).expect("start the server");

        let lab = Lab {
            _root: root,
            socket,
            token_file,
            workspace,
            child,
            group,
            _scope: scope,
        };
        lab.wait_for_socket().await;
        lab
    }

    async fn wait_for_socket(&self) {
        for _ in 0..400 {
            if std::fs::exists(&self.socket).unwrap_or(false)
                && !TokenFile::new(&self.token_file)
                    .read()
                    .unwrap_or_default()
                    .is_empty()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("the server never started serving {}", self.socket.display());
    }

    /// Run the shipped client binary with the connection flags already filled in.
    async fn client(&self, argv: &[&str]) -> Outcome {
        let mut command = tokio::process::Command::new(CLIENT);
        command
            .arg("--unix")
            .arg(&self.socket)
            .arg("--token-file")
            .arg(&self.token_file)
            .args(argv)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let scope = ProcessScope::new();
        let (child, group) = scope.spawn(command).expect("run the client");
        let output = tokio::time::timeout(Duration::from_secs(60), child.wait_with_output())
            .await
            .expect("the client should not hang")
            .expect("run the client");
        let _ = group.kill();
        Outcome {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    /// Same, but the credential is named on the command line, so the file is not
    /// touched at all.
    async fn client_with_token(&self, token: &str, argv: &[&str]) -> Outcome {
        let mut command = tokio::process::Command::new(CLIENT);
        command
            .arg("--unix")
            .arg(&self.socket)
            .arg("--token")
            .arg(token)
            .args(argv)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let scope = ProcessScope::new();
        let (child, group) = scope.spawn(command).expect("run the client");
        let output = tokio::time::timeout(Duration::from_secs(60), child.wait_with_output())
            .await
            .expect("the client should not hang")
            .expect("run the client");
        let _ = group.kill();
        Outcome {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    fn unused_tokens(&self) -> usize {
        TokenFile::new(&self.token_file)
            .read()
            .unwrap_or_default()
            .len()
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = self.group.kill();
        let _ = self.child.start_kill();
    }
}

#[derive(Debug)]
struct Outcome {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Outcome {
    fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|e| panic!("not JSON: {e}\n{}", self.stdout))
    }
    /// Assert the command succeeded and hand the caller what it printed.
    fn ok(self) -> Outcome {
        assert_eq!(
            self.code,
            Some(0),
            "exit {:?}\nstdout: {}\nstderr: {}",
            self.code,
            self.stdout,
            self.stderr
        );
        self
    }
}

fn git(cwd: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed");
}

/// A directory short enough to hold a Unix socket name on every platform.
fn short_socket_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let leaf = format!("crc{tag}{}{}", std::process::id(), nanos % 100_000);
    let tmpdir = dunce::canonicalize(std::env::temp_dir()).unwrap_or_else(|_| std::env::temp_dir());
    let base = if tmpdir.as_os_str().len() + leaf.len() + 1 + 16 <= 104 {
        tmpdir
    } else {
        dunce::canonicalize("/tmp").unwrap_or_else(|_| PathBuf::from("/tmp"))
    };
    let dir = base.join(leaf);
    std::fs::create_dir_all(&dir).expect("socket dir");
    dunce::canonicalize(&dir).unwrap_or(dir)
}

#[tokio::test]
#[cfg(unix)]
async fn a_script_can_drive_a_whole_workspace_through_the_two_binaries() {
    let lab = Lab::start().await;

    let info = lab.client(&["info", "--json"]).await.ok().json();
    assert_eq!(info["protocol_version"], PROTOCOL_VERSION, "{info}");
    let granted: Vec<&str> = info["capabilities"]
        .as_array()
        .expect("capabilities")
        .iter()
        .map(|value| value.as_str().expect("a capability name"))
        .collect();
    for expected in ["list", "read", "search", "write", "git", "tool-execution"] {
        assert!(granted.contains(&expected), "{granted:?}");
    }

    let listing = lab.client(&["list", "--json"]).await.ok().json();
    let paths: Vec<&str> = listing["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|entry| entry["path"].as_str().expect("path"))
        .collect();
    assert!(paths.contains(&"src/main.rs"), "{paths:?}");
    assert!(
        !paths.iter().any(|path| path.starts_with(".git/")),
        "git internals are not the workspace: {paths:?}"
    );

    let text = lab.client(&["cat", "src/main.rs"]).await.ok();
    assert_eq!(text.stdout, "fn main() {\n    count_beans(3);\n}\n");

    let found = lab
        .client(&["search", "count_beans", "--json"])
        .await
        .ok()
        .json();
    assert_eq!(found["hits"].as_array().expect("hits").len(), 1, "{found}");
    assert_eq!(found["hits"][0]["path"], "src/main.rs");
    assert_eq!(found["hits"][0]["line"], 2, "line numbers are 1-based");

    // The change the fixture left behind is visible as a diff, and only as one.
    let diff = lab.client(&["diff", "--json"]).await.ok().json();
    assert_eq!(diff["dirty"], true);
    let diff_text = diff["diff"].as_str().expect("diff");
    assert!(diff_text.contains("+Now with lentils."), "{diff_text}");

    // A write against the digest that was just read succeeds; the same digest is
    // then stale and the second attempt must lose.
    let readme = lab
        .client(&["cat", "README.md", "--json"])
        .await
        .ok()
        .json();
    let digest = readme["sha256"].as_str().expect("sha256").to_string();
    lab.client(&[
        "write",
        "README.md",
        "--text",
        "# beans\n\nNow with lentils and peas.\n",
        "--expect",
        &digest,
        "--json",
    ])
    .await
    .ok();
    assert!(
        std::fs::read_to_string(lab.workspace.join("README.md"))
            .expect("readme")
            .contains("peas")
    );
    let stale = lab
        .client(&[
            "write",
            "README.md",
            "--text",
            "overwritten",
            "--expect",
            &digest,
        ])
        .await;
    assert_eq!(stale.code, Some(1), "a stale digest must lose: {stale:?}");
    assert!(
        stale.stderr.contains("changed since"),
        "the refusal should name the reason: {}",
        stale.stderr
    );
    assert!(
        std::fs::read_to_string(lab.workspace.join("README.md"))
            .expect("readme")
            .contains("peas"),
        "the losing write must not have touched the file"
    );

    // A write into a directory that does not exist fails until it is asked for.
    let missing = lab
        .client(&["write", "docs/new.md", "--text", "hello"])
        .await;
    assert_eq!(missing.code, Some(1), "{missing:?}");
    lab.client(&["write", "docs/new.md", "--text", "hello", "--mkdir"])
        .await
        .ok();
    assert_eq!(
        std::fs::read_to_string(lab.workspace.join("docs/new.md")).expect("new"),
        "hello"
    );

    // A tool the server allowlisted runs with the workspace as its directory, and
    // the remote program's exit status becomes the client's.
    let ran = lab
        .client(&["exec", "--", "echo", "from the remote host"])
        .await;
    assert_eq!(ran.code, Some(0), "{ran:?}");
    assert_eq!(ran.stdout.trim(), "from the remote host");
    let failed = lab.client(&["exec", "--", "false"]).await;
    assert_eq!(
        failed.code,
        Some(1),
        "the remote program's status is the caller's: {failed:?}"
    );

    // A program nobody allowlisted is refused, and the refusal is not a crash.
    let refused = lab.client(&["exec", "--", "rm", "-rf", "/"]).await;
    assert_eq!(refused.code, Some(1), "{refused:?}");
    assert!(
        std::fs::exists(lab.workspace.join("src/main.rs")).unwrap(),
        "the workspace survived the attempt"
    );

    // Nothing outside the workspace is reachable from a shell either.
    let escape = lab.client(&["cat", "../../../etc/passwd"]).await;
    assert_eq!(escape.code, Some(1), "{escape:?}");
    assert!(escape.stderr.contains("path rejected"), "{}", escape.stderr);
}

/// Every command spends one credential, and the next command must find a live one
/// without the operator doing anything. A client that left a spent credential at the
/// head of the file would break on its own second invocation.
#[tokio::test]
#[cfg(unix)]
async fn each_command_spends_one_credential_and_the_file_keeps_working() {
    let lab = Lab::start().await;
    let issued = lab.unused_tokens();
    assert!(issued > 2, "the server publishes {issued}");

    for _ in 0..3 {
        lab.client(&["ping", "--quiet"]).await.ok();
    }
    assert_eq!(
        lab.unused_tokens(),
        issued - 3,
        "one credential per invocation, no more"
    );

    // A credential named on the command line is spent too, but the file is not
    // rewritten for it, because nothing in the file was used.
    let remaining = TokenFile::new(&lab.token_file).read().expect("tokens");
    let spare = remaining[0].as_str().to_string();
    lab.client_with_token(&spare, &["ping", "--quiet"])
        .await
        .ok();
    assert_eq!(
        lab.unused_tokens(),
        issued - 3,
        "--token does not consume from the file"
    );

    // And the spent one is refused the second time, over the same socket.
    let reused = lab.client_with_token(&spare, &["ping"]).await;
    assert_eq!(reused.code, Some(1), "{reused:?}");
    assert!(
        reused.stderr.contains("one-time") || reused.stderr.contains("refused"),
        "the message should say what to do next: {}",
        reused.stderr
    );
}

/// A client told to ask for less than the server offers is limited by its own
/// request, which is how a script proves the server really gated the capability.
#[tokio::test]
#[cfg(unix)]
async fn a_client_that_asks_for_less_gets_less() {
    let lab = Lab::start().await;

    // A flag the command does not take is refused, not quietly dropped: a script
    // that put `--json` after `info` would otherwise get text and parse it as JSON.
    let misplaced = lab.client(&["info", "--form", "json"]).await;
    assert_eq!(misplaced.code, Some(1), "{misplaced:?}");
    assert!(
        misplaced.stderr.contains("unknown flag"),
        "{}",
        misplaced.stderr
    );

    let tokens = TokenFile::new(&lab.token_file).read().expect("tokens");
    let outcome = {
        let mut command = tokio::process::Command::new(CLIENT);
        command
            .arg("--unix")
            .arg(&lab.socket)
            .arg("--token")
            .arg(tokens[0].as_str())
            .args([
                "--capability",
                "read",
                "--capability",
                "search",
                "info",
                "--json",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let scope = ProcessScope::new();
        let (child, group) = scope.spawn(command).expect("run the client");
        let output = tokio::time::timeout(Duration::from_secs(60), child.wait_with_output())
            .await
            .expect("no hang")
            .expect("run");
        let _ = group.kill();
        Outcome {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    };
    let info = outcome.ok().json();
    let granted: Vec<&str> = info["capabilities"]
        .as_array()
        .expect("capabilities")
        .iter()
        .map(|value| value.as_str().expect("name"))
        .collect();
    assert_eq!(
        granted,
        [
            RemoteCapability::WorkspaceRead.as_str(),
            RemoteCapability::WorkspaceSearch.as_str()
        ],
        "{granted:?}"
    );

    // Asking for a capability this build cannot grant fails at the argument parser,
    // before a socket is opened or a credential spent.
    let tokens_before = lab.unused_tokens();
    let refused = lab
        .client(&["--capability", "interactive-pty", "ping"])
        .await;
    assert_eq!(refused.code, Some(1), "{refused:?}");
    assert!(
        refused.stderr.contains("PTY") || refused.stderr.contains("capability"),
        "{}",
        refused.stderr
    );
    assert_eq!(
        lab.unused_tokens(),
        tokens_before,
        "a refused argument must not spend a credential"
    );
}
