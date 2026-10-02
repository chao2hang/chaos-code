//! The remote workspace transport over real sockets and a real process.
//!
//! The unit tests in the library drive client and server over an in-process pipe.
//! These drive the shipped `chaos-remote-server` binary over a Unix socket and a
//! loopback TCP port, because the things that only go wrong in production are the
//! socket path length, the mode on the credential file, the port that is not
//! loopback, and the process that is still running when the test ends.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use chaos_engine::remote::{
    Payload, RemoteCapability, RemoteEndpoint, RemoteError, RemoteWorkspace, RemoteWorkspaceConfig,
    SessionToken, TokenFile,
};
use xai_tty_utils::ProcessScope;

const ALL_CAPABILITIES: &[RemoteCapability] = &[
    RemoteCapability::WorkspaceList,
    RemoteCapability::WorkspaceRead,
    RemoteCapability::WorkspaceSearch,
    RemoteCapability::WorkspaceWrite,
    RemoteCapability::Git,
    RemoteCapability::ToolExecution,
];

/// A started server, with the directory it serves and the scope that owns it.
struct ServerProcess {
    workspace: PathBuf,
    token_file: PathBuf,
    endpoint: RemoteEndpoint,
    /// Where the server's own output went. It is never inherited: a child that
    /// outlived its test would keep the harness's captured-output pipe open, and
    /// the harness waits for an end that never arrives.
    log: PathBuf,
    child: tokio::process::Child,
    group: std::sync::Arc<xai_tty_utils::ProcessGroup>,
    /// The scope that enrolled the child. Dropping the last handle of a scope
    /// reaps everything it owns, so the server would die the moment `start`
    /// returned if this were only a local there.
    _scope: xai_tty_utils::ProcessScope,
    /// Kept only so the directory outlives the server that wrote into it.
    _root: tempfile::TempDir,
}

impl ServerProcess {
    async fn start(socket: Option<&Path>, tcp: Option<u16>, extra: &[&str]) -> ServerProcess {
        let root = tempfile::tempdir().expect("tempdir");
        let workspace = root.path().join("work");
        std::fs::create_dir(&workspace).expect("workspace");

        let token_file = root.path().join("tokens");
        let mut argv: Vec<String> = vec![
            env!("CARGO_BIN_EXE_chaos-remote-server").to_string(),
            "--workspace".into(),
            workspace.display().to_string(),
            "--token-file".into(),
            token_file.display().to_string(),
            "--allow".into(),
            "echo".into(),
            "--capability".into(),
            "tool-execution".into(),
        ];
        if let Some(socket) = socket {
            argv.push("--unix".into());
            argv.push(socket.display().to_string());
        }
        if let Some(port) = tcp {
            argv.push("--tcp".into());
            argv.push(format!("127.0.0.1:{port}"));
        }
        argv.extend(extra.iter().map(|arg| arg.to_string()));

        // One file for both streams, sharing one offset: the server writes its
        // refusals to stderr and its progress to stdout, and a failure that says
        // nothing is the worst possible test failure.
        let log = root.path().join("server.log");
        let handle = std::fs::File::create(&log).expect("create the server log");
        let mut command = tokio::process::Command::new(&argv[0]);
        command
            .args(&argv[1..])
            .stdout(Stdio::from(handle.try_clone().expect("clone the log")))
            .stderr(Stdio::from(handle));
        let scope = ProcessScope::new();
        let (child, group) = scope.spawn(command).expect("start the server");

        // `port` is the port of the remote host itself, which the socket or the
        // loopback port stands in for; it is not the address being dialled.
        let endpoint = RemoteEndpoint {
            host: "buildbox".into(),
            port: 22,
            host_key: chaos_engine::remote::HostKeyPolicy::Strict,
            capabilities: ALL_CAPABILITIES.to_vec(),
        };
        ServerProcess {
            workspace,
            token_file,
            log,
            endpoint,
            child,
            group,
            _scope: scope,
            _root: root,
        }
    }

    /// The credentials the server published. It writes them before it binds, so the
    /// file appearing is the signal that a connect is worth trying.
    async fn tokens(&mut self) -> Vec<SessionToken> {
        for _ in 0..400 {
            if let Ok(tokens) = TokenFile::new(&self.token_file).read()
                && !tokens.is_empty()
            {
                return tokens;
            }
            // A server that died is not going to write anything, and the reason it
            // died is the only useful thing a failure can say.
            if let Some(status) = self.child.try_wait().expect("wait") {
                panic!(
                    "the server exited with {status} before writing {}; it said {:?}",
                    self.token_file.display(),
                    std::fs::read_to_string(&self.log).unwrap_or_default(),
                );
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!(
            "the server never wrote {}; it is still running as pid {} and said {:?}",
            self.token_file.display(),
            self.child.id().unwrap_or(0),
            std::fs::read_to_string(&self.log).unwrap_or_default(),
        );
    }

    async fn connect_unix(&mut self, socket: &Path) -> RemoteWorkspace<tokio::net::UnixStream> {
        let tokens = self.tokens().await;
        let mut last = None;
        for _ in 0..200 {
            match RemoteWorkspace::connect_unix(
                socket,
                &self.endpoint,
                &tokens[0],
                RemoteWorkspaceConfig::new().capabilities(ALL_CAPABILITIES.to_vec()),
            )
            .await
            {
                Ok(workspace) => return workspace,
                Err(error) => last = Some(error),
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("could not connect to {}: {last:?}", socket.display());
    }

    async fn connect_tcp(
        &mut self,
        port: u16,
        token: &SessionToken,
    ) -> RemoteWorkspace<tokio::net::TcpStream> {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
        for _ in 0..200 {
            match RemoteWorkspace::connect_tcp(
                addr,
                &self.endpoint,
                token,
                RemoteWorkspaceConfig::new().capabilities(ALL_CAPABILITIES.to_vec()),
            )
            .await
            {
                Ok(workspace) => return workspace,
                Err(error) => {
                    // A refused credential will not start working on the next
                    // attempt, and looping on it hides the reason.
                    assert!(
                        matches!(error, RemoteError::Io { .. }),
                        "the port is not answering: {error:?}"
                    );
                }
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("could not connect to {addr}");
    }

    fn write_file(&self, relative: &str, contents: &str) {
        let path = self.workspace.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent");
        }
        std::fs::write(path, contents).expect("write fixture");
    }

    async fn stop(&mut self) {
        let _ = self.group.kill();
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
    }
}

/// Whatever happens to the test body, the server it started goes with it.
impl Drop for ServerProcess {
    fn drop(&mut self) {
        let _ = self.group.kill();
        let _ = self.child.start_kill();
    }
}

/// A directory short enough to hold a Unix socket name on every platform.
///
/// `TMPDIR` on macOS is `/var/folders/…/T/`, which leaves little of the 104-byte
/// `sun_path` for the socket itself, so a test fixture cannot assume the temporary
/// directory is a safe place to put one.
fn short_socket_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let leaf = format!("cr{tag}{}{}", std::process::id(), nanos % 100_000);
    let tmpdir = dunce::canonicalize(std::env::temp_dir()).unwrap_or_else(|_| std::env::temp_dir());
    // Leave room for a nested endpoint name, and keep well inside macOS's 104.
    let base = if tmpdir.as_os_str().len() + leaf.len() + 1 + 32 <= 104 {
        tmpdir
    } else {
        dunce::canonicalize("/tmp").unwrap_or_else(|_| PathBuf::from("/tmp"))
    };
    let dir = base.join(leaf);
    std::fs::create_dir_all(&dir).expect("socket dir");
    let dir = dunce::canonicalize(&dir).unwrap_or(dir);
    assert!(
        probe_socket_fits(&dir),
        "{dir:?} cannot hold a Unix socket name; the fixture is too deep"
    );
    dir
}

#[cfg(unix)]
fn probe_socket_fits(dir: &Path) -> bool {
    use std::os::unix::net::UnixListener;
    let endpoint = dir.join("probe.sock");
    match UnixListener::bind(&endpoint) {
        Ok(_) => {
            let _ = std::fs::remove_file(&endpoint);
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => false,
        Err(error) => panic!("probe bind under {}: {error}", dir.display()),
    }
}

#[cfg(not(unix))]
fn probe_socket_fits(_dir: &Path) -> bool {
    true
}

/// The whole point of the feature, over a real socket to a real process: the
/// workspace on the other end is listed, read, searched, changed, and diffed.
#[tokio::test]
#[cfg(unix)]
async fn a_real_server_serves_a_workspace_over_a_unix_socket() {
    let dir = short_socket_dir("u");
    let socket = dir.join("s.sock");
    let mut server = ServerProcess::start(Some(&socket), None, &[]).await;
    let mut session = server.connect_unix(&socket).await;

    assert_eq!(
        session.protocol_version(),
        chaos_engine::remote::PROTOCOL_VERSION
    );
    assert!(session.has(&RemoteCapability::ToolExecution));

    server.write_file("src/main.rs", "fn main() { count_beans(3) }\n");
    server.write_file("README.md", "# beans\n\nA crate about beans.\n");
    std::fs::create_dir(server.workspace.join("target")).expect("target");
    std::fs::write(server.workspace.join("target/noise.bin"), [7u8; 8]).expect("noise");

    let (entries, truncated) = session.list(None, None, None).await.unwrap();
    let paths: Vec<&str> = entries.iter().map(|entry| entry.path.as_str()).collect();
    assert!(paths.contains(&"src/main.rs"), "{paths:?}");
    assert!(paths.contains(&"README.md"), "{paths:?}");
    assert!(
        !paths.contains(&"target"),
        "build output is not the workspace: {paths:?}"
    );
    assert!(truncated, "a filtered listing must say it is filtered");

    let beans = session.read_file("README.md").await.unwrap();
    assert_eq!(beans.data, b"# beans\n\nA crate about beans.\n");

    let found = session
        .search("count_beans", None, None, false)
        .await
        .unwrap();
    assert_eq!(found.hits.len(), 1, "{found:?}");
    assert_eq!(found.hits[0].path, "src/main.rs");
    assert_eq!(found.hits[0].line, 1);

    // A tool the server named runs; the workspace is the working directory.
    let out = session
        .exec(&["echo", "hello from the remote host"], None, None)
        .await
        .unwrap();
    assert!(out.success(), "{out:?}");
    assert_eq!(out.stdout.trim(), "hello from the remote host");

    // The edit is made against the version that was read, and the stale one loses.
    let (written, digest) = session
        .write(
            "README.md",
            chaos_engine::remote::WriteFile {
                content: b"# beans\n\nA crate about beans, now with lentils.\n",
                expected_sha256: Some(&beans.sha256),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(written, 48, "the server reports the bytes it wrote");
    let replacement = b"# beans\n\nA crate about beans, now with lentils.\n";
    assert_eq!(digest, chaos_engine::remote::sha256_hex(replacement));
    assert!(
        std::fs::read_to_string(server.workspace.join("README.md"))
            .unwrap()
            .contains("lentils")
    );

    server.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// A loopback port is for a tunnel. A routable one is refused by the server before
/// it binds, and refused by the client before it dials.
#[tokio::test]
async fn only_loopback_may_be_listened_on_or_dialled() {
    let refusal = chaos_engine::remote::require_loopback(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        4100,
    ));
    assert!(refusal.is_err(), "0.0.0.0 is not loopback");

    // The client refuses to dial a routable address, whatever the server does.
    let endpoint = RemoteEndpoint {
        host: "buildbox".into(),
        port: 4100,
        host_key: chaos_engine::remote::HostKeyPolicy::Strict,
        capabilities: ALL_CAPABILITIES.to_vec(),
    };
    let routable = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)), 4100);
    let err = RemoteWorkspace::connect_tcp(
        routable,
        &endpoint,
        &SessionToken::from_text("t"),
        RemoteWorkspaceConfig::new().capabilities(ALL_CAPABILITIES.to_vec()),
    )
    .await
    .expect_err("this would cross a network in plaintext");
    assert!(matches!(err, RemoteError::InvalidRequest { .. }), "{err:?}");

    // And the server says no to the same request on its side.
    let dir = short_socket_dir("t");
    let refusal_log = dir.join("refusal.log");
    let log_file = std::fs::File::create(&refusal_log).expect("refusal log");
    let mut server = tokio::process::Command::new(env!("CARGO_BIN_EXE_chaos-remote-server"));
    server
        .args([
            "--workspace",
            dir.to_str().unwrap(),
            "--tcp",
            "0.0.0.0:4113",
        ])
        .stdout(Stdio::from(log_file.try_clone().expect("clone the log")))
        .stderr(Stdio::from(log_file));
    let scope = ProcessScope::new();
    let (mut child, group) = scope.spawn(server).expect("start the server");
    let status = tokio::time::timeout(Duration::from_secs(20), child.wait())
        .await
        .expect("the server exits rather than listening widely")
        .expect("wait");
    assert!(!status.success(), "a routable bind must fail the server");
    let said = std::fs::read_to_string(&refusal_log).unwrap_or_default();
    assert!(
        said.contains("loopback"),
        "the refusal has to say why it refused: {said:?}"
    );
    let _ = group.kill();

    // With a loopback port it does start, and a session works over it.
    let mut server = ServerProcess::start(None, Some(4114), &[]).await;
    let tokens = server.tokens().await;
    let mut session = server.connect_tcp(4114, &tokens[0]).await;
    session.ping().await.unwrap();
    server.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// Two sessions, one credential. The second connect is the replay, and it fails
/// over a real socket for the same reason it fails in the library tests.
#[tokio::test]
#[cfg(unix)]
async fn one_credential_opens_one_session_over_a_real_socket() {
    let dir = short_socket_dir("r");
    let socket = dir.join("s.sock");
    let mut server = ServerProcess::start(Some(&socket), None, &[]).await;
    let tokens = server.tokens().await;
    let first = server.connect_unix(&socket).await;
    assert!(first.has(&RemoteCapability::WorkspaceRead));

    let err = RemoteWorkspace::connect_unix(
        &socket,
        &server.endpoint,
        &tokens[0],
        RemoteWorkspaceConfig::new().capabilities(ALL_CAPABILITIES.to_vec()),
    )
    .await
    .expect_err("the credential is spent");
    assert!(matches!(err, RemoteError::Unauthorized { .. }), "{err:?}");

    // The next credential in the file still works, so the refusal is about the
    // credential and not about the socket.
    let mut second = RemoteWorkspace::connect_unix(
        &socket,
        &server.endpoint,
        &tokens[1],
        RemoteWorkspaceConfig::new().capabilities(ALL_CAPABILITIES.to_vec()),
    )
    .await
    .expect("a fresh credential");
    assert_eq!(
        second.ping().await.unwrap(),
        chaos_engine::remote::PROTOCOL_VERSION
    );

    server.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// Connecting late is not the same failure as connecting twice, and the server
/// has to know the difference because it sweeps stale credentials before it
/// redeems them: without that, the message a late client gets accuses it of
/// replay.
#[tokio::test]
#[cfg(unix)]
async fn a_late_client_is_told_its_credential_expired() {
    let dir = short_socket_dir("x");
    let socket = dir.join("s.sock");
    let mut server = ServerProcess::start(Some(&socket), None, &["--token-ttl", "1"]).await;
    let tokens = server.tokens().await;

    // Past the lifetime, so the credential is stale by the time it is presented.
    tokio::time::sleep(Duration::from_millis(1_300)).await;

    let err = RemoteWorkspace::connect_unix(
        &socket,
        &server.endpoint,
        &tokens[0],
        RemoteWorkspaceConfig::new().capabilities(ALL_CAPABILITIES.to_vec()),
    )
    .await
    .expect_err("the credential has expired");
    let RemoteError::Unauthorized { reason } = &err else {
        panic!("expected an authorization refusal, got {err:?}");
    };
    assert!(
        reason.contains("expired"),
        "a client that was merely late is not a replay: {reason}"
    );

    server.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// A server with no allowlisted program can still serve files, and says so rather
/// than running what it is handed.
#[tokio::test]
#[cfg(unix)]
async fn a_server_without_an_allowlist_runs_nothing() {
    let dir = short_socket_dir("n");
    let socket = dir.join("s.sock");
    let root = tempfile::tempdir().expect("tempdir");
    let workspace = root.path().join("work");
    std::fs::create_dir(&workspace).unwrap();
    let token_file = root.path().join("tokens");

    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_chaos-remote-server"));
    let log = root.path().join("server.log");
    let log_file = std::fs::File::create(&log).expect("server log");
    command
        .args([
            "--workspace",
            workspace.to_str().unwrap(),
            "--token-file",
            token_file.to_str().unwrap(),
            "--unix",
            socket.to_str().unwrap(),
        ])
        .stdout(Stdio::from(log_file.try_clone().expect("clone the log")))
        .stderr(Stdio::from(log_file));
    let scope = ProcessScope::new();
    let (mut child, group) = scope.spawn(command).expect("start the server");

    let tokens = {
        let mut attempts = 0;
        loop {
            if let Ok(tokens) = TokenFile::new(&token_file).read()
                && !tokens.is_empty()
            {
                break tokens;
            }
            attempts += 1;
            assert!(
                attempts < 200,
                "the server never wrote its credentials; it said {:?}",
                std::fs::read_to_string(&log).unwrap_or_default()
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    };
    let endpoint = RemoteEndpoint {
        host: "buildbox".into(),
        port: 22,
        host_key: chaos_engine::remote::HostKeyPolicy::Strict,
        capabilities: ALL_CAPABILITIES.to_vec(),
    };
    let mut session = {
        let mut attempts = 0;
        loop {
            match RemoteWorkspace::connect_unix(
                &socket,
                &endpoint,
                &tokens[0],
                RemoteWorkspaceConfig::new().capabilities(ALL_CAPABILITIES.to_vec()),
            )
            .await
            {
                Ok(session) => break session,
                Err(RemoteError::Io { .. }) => {
                    attempts += 1;
                    assert!(attempts < 200, "the socket never answered");
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                Err(error) => panic!("a valid credential was refused: {error:?}"),
            }
        }
    };

    // ToolExecution is not offered, so the client refuses without asking.
    assert!(!session.has(&RemoteCapability::ToolExecution));
    assert_eq!(
        session.exec(&["echo", "hi"], None, None).await.unwrap_err(),
        RemoteError::CapabilityNotGranted {
            capability: RemoteCapability::ToolExecution
        }
    );
    // Everything else still works.
    session.ping().await.unwrap();
    let (entries, _) = session.list(None, None, None).await.unwrap();
    assert!(entries.is_empty(), "{entries:?}");

    let _ = group.kill();
    let _ = child.start_kill();
    let _ = child.wait().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// A request that no session of any kind should get through, sent from a client
/// that is otherwise perfectly well authorised.
#[tokio::test]
#[cfg(unix)]
async fn an_escaping_path_is_refused_by_the_running_server() {
    let dir = short_socket_dir("p");
    let socket = dir.join("s.sock");
    let mut server = ServerProcess::start(Some(&socket), None, &[]).await;
    std::fs::write(dir.join("outside.txt"), "not in the workspace").expect("outside file");
    let mut session = server.connect_unix(&socket).await;

    for attempt in ["../outside.txt", "/etc/passwd", "..", "a/../b/../../c"] {
        let err = session
            .read(attempt, None, None)
            .await
            .expect_err("nothing outside the workspace is reachable");
        assert!(
            matches!(err, RemoteError::PathRejected { .. }),
            "{attempt}: {err:?}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(dir.join("outside.txt")).unwrap(),
        "not in the workspace"
    );

    server.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// The artifact a deployment would put on the host, deployed through the running
/// server itself, ending as the program a client would then connect to.
#[tokio::test]
#[cfg(unix)]
async fn a_build_can_be_deployed_through_the_running_server() {
    let dir = short_socket_dir("d");
    let socket = dir.join("s.sock");
    let mut server = ServerProcess::start(Some(&socket), None, &[]).await;
    let mut session = server.connect_unix(&socket).await;

    // The artifact is this very binary: that is what `ARTIFACT_NAME` means.
    let artifact = std::fs::read(env!("CARGO_BIN_EXE_chaos-remote-server")).expect("binary");
    let outcome = session
        .install_artifact("1.2.3", &artifact)
        .await
        .expect("deploy the build we are running");
    assert!(outcome.current);
    let layout = chaos_engine::remote::InstallLayout::new(&server.workspace);
    assert_eq!(layout.current_version().as_deref(), Some("1.2.3"));
    let deployed = layout.current_artifact().expect("current artifact");
    assert_eq!(std::fs::read(&deployed).unwrap(), artifact);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            std::fs::metadata(&deployed).unwrap().permissions().mode() & 0o111,
            0,
            "an artifact that cannot be executed is not a deployment"
        );
    }
    // And it is the same program: it answers `--version`.
    let scope = ProcessScope::new();
    let mut command = tokio::process::Command::new(&deployed);
    command
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let (child, group) = scope.spawn(command).expect("run the deployed build");
    let output = tokio::time::timeout(Duration::from_secs(30), child.wait_with_output())
        .await
        .expect("--version should not hang")
        .expect("run");
    let _ = group.kill();
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(text.starts_with("chaos-remote-server "), "{text}");

    server.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// A session on a loopback port, retried while the port is still not answering.
async fn server_session_on(
    port: u16,
    token: &SessionToken,
) -> RemoteWorkspace<tokio::net::TcpStream> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let endpoint = RemoteEndpoint {
        host: "buildbox".into(),
        port: 22,
        host_key: chaos_engine::remote::HostKeyPolicy::Strict,
        capabilities: ALL_CAPABILITIES.to_vec(),
    };
    let mut last = None;
    for _ in 0..400 {
        match RemoteWorkspace::connect_tcp(
            addr,
            &endpoint,
            token,
            RemoteWorkspaceConfig::new().capabilities(ALL_CAPABILITIES.to_vec()),
        )
        .await
        {
            Ok(session) => return session,
            Err(error) => last = Some(error),
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("no session on {addr}: {last:?}");
}

/// With no `--token-file` and no socket to put them beside, the credentials go under
/// the install directory — which the first run of all has not created. A server that
/// refused to start until the operator had made that directory would be blaming them
/// for the server's own choice of location.
#[tokio::test]
async fn a_first_run_creates_the_directory_it_chose_for_its_credentials() {
    let dir = short_socket_dir("f");
    let workspace = dir.join("work");
    std::fs::create_dir(&workspace).expect("workspace");
    let port = 4131;

    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_chaos-remote-server"));
    command
        .args([
            "--workspace",
            workspace.to_str().unwrap(),
            "--tcp",
            "127.0.0.1:4131",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let scope = ProcessScope::new();
    let (mut child, group) = scope.spawn(command).expect("start the server");

    let token_file = workspace
        .join(chaos_engine::remote::DEFAULT_INSTALL_DIR)
        .join("tokens");
    let mut tokens = Vec::new();
    for _ in 0..400 {
        if let Ok(read) = TokenFile::new(&token_file).read()
            && !read.is_empty()
        {
            tokens = read;
            break;
        }
        if child.try_wait().expect("wait").is_some() {
            panic!(
                "the server gave up before publishing {}",
                token_file.display()
            );
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(
        !tokens.is_empty(),
        "no credentials at {}",
        token_file.display()
    );

    // And the session those credentials open is an ordinary one.
    let mut session = server_session_on(port, &tokens[0]).await;
    session.ping().await.expect("pong");

    let _ = group.kill();
    let _ = child.start_kill();
    let _ = child.wait().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// `--connect-wait` is for the transport that is not up yet — a tunnel being
/// brought up, a socket-activated server. The wait repeats the dial only: the
/// credential is presented exactly once, which is what makes waiting safe at all.
///
/// The late arrival is a symlink put in place after the client started, against a
/// server that has been running the whole time. That is the real shape of the
/// race (the path the caller was told to use does not resolve yet), and it keeps
/// the token with the server that will be asked to authenticate it.
#[tokio::test]
#[cfg(unix)]
async fn the_client_waits_for_a_socket_that_appears_a_moment_later() {
    let dir = short_socket_dir("late");
    let real = dir.join("real.sock");
    let announced = dir.join("chaos.sock");
    std::fs::create_dir_all(&dir).expect("socket dir");

    let mut server = ServerProcess::start(Some(&real), None, &[]).await;
    let tokens = server.tokens().await;

    let log = dir.join("client.log");
    let log_file = std::fs::File::create(&log).expect("client log");
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_chaos-remote"));
    command
        .args([
            "--connect-wait",
            "30",
            "--unix",
            &announced.display().to_string(),
            "--token",
            tokens[0].as_str(),
            "ping",
        ])
        .stdout(Stdio::from(log_file.try_clone().expect("clone the log")))
        .stderr(Stdio::from(log_file));
    let scope = ProcessScope::new();
    let (mut child, group) = scope.spawn(command).expect("start the client");

    // The announced path resolves to nothing yet, so the client is still trying.
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert!(
        child.try_wait().expect("poll the client").is_none(),
        "the client gave up on a transport it was told to wait for"
    );
    std::os::unix::fs::symlink(&real, &announced).expect("point the announced path at the server");

    let status = tokio::time::timeout(Duration::from_secs(30), child.wait())
        .await
        .expect("the client finishes once the socket resolves")
        .expect("wait");
    let _ = group.kill();
    let said = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(status.success(), "the session should have opened: {said}");
    assert!(
        said.contains("protocol"),
        "ping reports what was negotiated: {said}"
    );

    server.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// `--reply-timeout` bounds how long a caller can be stuck behind a peer that
/// stops answering. The peer here answers the handshake — so the session is real —
/// and then says nothing, which is what a wedged server looks like.
#[tokio::test]
async fn the_client_gives_up_on_a_peer_that_stops_answering() {
    use chaos_engine::remote::protocol::{Envelope, recv, send};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("loopback listener");
    let addr = listener.local_addr().expect("addr");
    let peer = tokio::spawn(async move {
        let (mut stream, _peer) = listener.accept().await.expect("accept");
        let hello: Envelope<chaos_engine::remote::Request> =
            recv(&mut stream, chaos_engine::remote::MAX_FRAME_BYTES)
                .await
                .expect("hello")
                .expect("a frame");
        send(
            &mut stream,
            &Envelope::<chaos_engine::remote::Reply> {
                id: hello.id,
                body: Ok(Payload::Hello {
                    protocol_version: chaos_engine::remote::PROTOCOL_VERSION,
                    server: chaos_engine::remote::Implementation::local(),
                    capabilities: ALL_CAPABILITIES.to_vec(),
                    session_id: "wedged".into(),
                    max_transfer_bytes: 1024 * 1024,
                }),
            },
        )
        .await
        .expect("hello reply");
        // Open, and silent from here.
        tokio::time::sleep(Duration::from_secs(60)).await;
    });

    let dir = short_socket_dir("stall");
    std::fs::create_dir_all(&dir).expect("dir");
    let log = dir.join("client.log");
    let log_file = std::fs::File::create(&log).expect("client log");
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_chaos-remote"));
    command
        .args([
            "--reply-timeout",
            "1",
            "--tcp",
            &addr.to_string(),
            "--token",
            "0123456789abcdef0123456789abcdef",
            "ping",
        ])
        .stdout(Stdio::from(log_file.try_clone().expect("clone the log")))
        .stderr(Stdio::from(log_file));
    let scope = ProcessScope::new();
    let (mut child, group) = scope.spawn(command).expect("start the client");

    let started = std::time::Instant::now();
    let status = tokio::time::timeout(Duration::from_secs(25), child.wait())
        .await
        .expect("the deadline fires instead of the client waiting forever")
        .expect("wait");
    let elapsed = started.elapsed();
    let _ = group.kill();
    peer.abort();

    let said = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !status.success(),
        "an unanswered request must fail the run: {said}"
    );
    assert!(
        said.contains("no reply to ping"),
        "the message names the request that went unanswered: {said}"
    );
    assert!(
        elapsed >= Duration::from_millis(900) && elapsed < Duration::from_secs(20),
        "the caller waited about the second it asked for, not the default: {elapsed:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A peer that accepts the connection and never speaks is the other half of the
/// same deadline: here nothing was ever answered, so the run dies at the
/// handshake. What is being pinned down is the credential. Sending it spends it,
/// so a run that dies this way has to take it out of the file anyway — otherwise
/// the file goes on offering a credential the server has already seen, and every
/// later run in that directory refuses its own credential as a replay.
#[tokio::test]
async fn a_run_that_dies_in_the_handshake_still_retires_its_credential() {
    use chaos_engine::remote::{SessionToken, TokenFile};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("loopback listener");
    let addr = listener.local_addr().expect("addr");
    let peer = tokio::spawn(async move {
        let (_stream, _peer) = listener.accept().await.expect("accept");
        // Open, and mute. The bytes the client sends are left unread.
        tokio::time::sleep(Duration::from_secs(60)).await;
    });

    let dir = short_socket_dir("mute-peer");
    std::fs::create_dir_all(&dir).expect("dir");
    let spent = SessionToken::from_text("0123456789abcdef0123456789abcdef");
    let keeper = SessionToken::from_text("fedcba9876543210fedcba9876543210");
    let tokens = dir.join("tokens");
    TokenFile::new(&tokens)
        .write(&[spent.clone(), keeper.clone()])
        .expect("token file");

    let log = dir.join("client.log");
    let log_file = std::fs::File::create(&log).expect("client log");
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_chaos-remote"));
    command
        .args([
            "--reply-timeout",
            "1",
            "--tcp",
            &addr.to_string(),
            "--token-file",
            tokens.to_str().expect("utf-8 path"),
            "ping",
        ])
        .stdout(Stdio::from(log_file.try_clone().expect("clone the log")))
        .stderr(Stdio::from(log_file));
    let scope = ProcessScope::new();
    let (mut child, group) = scope.spawn(command).expect("start the client");

    let started = std::time::Instant::now();
    let status = tokio::time::timeout(Duration::from_secs(25), child.wait())
        .await
        .expect("the handshake deadline fires instead of waiting forever")
        .expect("wait");
    let elapsed = started.elapsed();
    let _ = group.kill();
    peer.abort();

    let said = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !status.success(),
        "a peer that never answers must fail the run: {said}"
    );
    assert!(
        said.contains("no reply to the handshake"),
        "the message says which wait went unanswered: {said}"
    );
    assert!(
        elapsed >= Duration::from_millis(900) && elapsed < Duration::from_secs(20),
        "the handshake got the --reply-timeout bound, not the 30s default: {elapsed:?}"
    );

    let left = TokenFile::new(&tokens)
        .read()
        .expect("token file still readable");
    assert_eq!(
        left.iter().map(SessionToken::as_str).collect::<Vec<_>>(),
        vec![keeper.as_str()],
        "the credential that went on the wire is gone; the one that did not stays"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The payload types are named here so a change to the wire format has to be made
/// in this file too, and noticed.
#[allow(dead_code)]
fn payloads_are_exhaustive(payload: Payload) -> &'static str {
    match payload {
        Payload::Hello { .. } => "hello",
        Payload::List { .. } => "list",
        Payload::Read { .. } => "read",
        Payload::Search { .. } => "search",
        Payload::Write { .. } => "write",
        Payload::Diff { .. } => "diff",
        Payload::Exec { .. } => "exec",
        Payload::InstallBegin { .. } => "install_begin",
        Payload::InstallChunk { .. } => "install_chunk",
        Payload::InstallFinish { .. } => "install_finish",
        Payload::Pong { .. } => "pong",
    }
}
