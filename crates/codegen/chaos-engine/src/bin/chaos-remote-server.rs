//! Serves one workspace directory to one authorised Chaos client at a time.
//!
//! This is the artifact that [`InstallLayout`] puts onto a remote host: its name
//! matches [`ARTIFACT_NAME`], so a build deployed through the `install_*` requests
//! is the same program a operator would have copied by hand.
//!
//! What it will do is fixed by the command line before it accepts anything, and no
//! session can widen it:
//!
//! ```text
//! chaos-remote-server --workspace /srv/work/chaos \
//!     --unix /run/user/1000/chaos-remote.sock \
//!     --allow cargo --allow git --capability tool-execution
//! ```
//!
//! A Unix socket is the usual transport, because reaching the socket already means
//! reaching the account. `--tcp` is accepted for loopback only: this speaks no
//! transport security of its own, so the way across a network is a tunnel that
//! somebody else secured, and `127.0.0.1:4100` is what the client dials.

use std::path::PathBuf;

use chaos_engine::remote::{
    DEFAULT_INSTALL_DIR, ForwardLimits, ForwardTarget, Implementation, InstallLayout,
    ProvenancePolicy, RemoteCapability, Server, ServerConfig, TokenFile, parse_capability,
    parse_forward_target, parse_loopback_host, requirement_requested_by_env,
};

const USAGE: &str = "\
chaos-remote-server — serve one workspace to one authorised client at a time

  --workspace <path>      the only directory this server can see (required)
  --unix <path>           listen on a Unix socket
  --tcp <host:port>       listen on a loopback TCP port (127.0.0.1 or ::1)
  --token-file <path>     where to write the one-time session credentials
  --allow <program>       a program `exec` may run; repeatable, bare names only
  --capability <name>     offer a capability the defaults do not already include
                          (list, read, search, write, git, tool-execution)
  --no-write              do not offer file writes or deployments
  --install-dir <name>    artifact directory under the workspace (default .chaos-server)
  --trust-signing-key <base64|@FILE>
                          the ed25519 public key an artifact must be signed by: the
                          bare base64 of the 32-byte key, or @path to read that text
                          from a file. Without it the key is CHAOS_SIGNING_PUBLIC_KEY,
                          and before that the key this build was compiled with
  --allow-unsigned-artifact
                          install an artifact that offers no signature. Off by
                          default: a host requires a signature and refuses with the
                          reason it checked (signature_missing, signature_invalid,
                          no_trusted_key). Also CHAOS_REMOTE_REQUIRE_SIGNATURE=0
  --token-ttl <seconds>   how long an unused credential stays usable (default 600)
  --tokens <count>        how many one-time credentials to publish at startup
                          (default 8; each session spends one)
  --max-exec-output <bytes>  output cap for one tool run (default 262144)
  --allow-forward-to <host:port>
                          a service a session may reach through a forwarded local
                          port; repeatable, and the only way forwarding is enabled
  --forward-ttl <seconds> longest grant the server will agree to (default 1800)
  --forward-max-uses <count>  connections one grant may open (default 256)
  --forward-connect-timeout <seconds>  how long a forward may wait for its target
                          to answer (default 10)

By default list, read, search, write and git are offered. Tool execution is not,
because it needs an `--allow` list to mean anything, and an empty one runs nothing.
Port forwarding is not offered either: with no `--allow-forward-to` there is nowhere
a forward would be allowed to go, and a server advertising it would only collect
refusals.

Without --unix or --tcp there is nowhere to connect, which is refused rather than
silently defaulting to a port.";

#[tokio::main(flavor = "multi_thread")]
async fn main() -> std::process::ExitCode {
    match run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        // The status alone tells an operator nothing, and `main` returning a `Result`
        // would print the message without saying which program said it.
        Err(message) => {
            eprintln!("chaos-remote-server: {message}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let options = Options::parse(std::env::args().skip(1).collect())?;
    if options.help {
        print!("{USAGE}");
        return Ok(());
    }
    if let Some(version) = options.version {
        println!("{version}");
        return Ok(());
    }
    let options = options.into_config()?;
    let provenance = provenance_policy(&options)?;

    let config = ServerConfig::new(&options.workspace)
        .capabilities(options.capabilities)
        .allowed_executables(options.allowed)
        .token_ttl(std::time::Duration::from_secs(options.token_ttl_secs))
        .max_exec_output_bytes(options.max_exec_output_bytes)
        .forward_targets(options.forward_targets.clone())
        .forward_limits(options.forward_limits)
        .forward_connect_timeout(options.forward_connect_timeout)
        .install_layout(
            InstallLayout::with_dir_name(&options.workspace, &options.install_dir)
                .with_provenance(provenance.clone()),
        );
    let server = Server::new(config)?;

    // Said before the first credential exists: what an `install` will be held to is
    // the thing an operator most often has to check when a deploy fails, and it
    // cannot be recovered from the log afterwards.
    println!("artifacts: {}", provenance.describe());

    // Credentials before the listener: a session that arrives a millisecond after
    // bind must find something to authenticate with, or the first connect fails for
    // a reason that has nothing to do with the client.
    let tokens = server.issue_tokens(options.initial_tokens).await;
    let token_file = options
        .token_file
        .clone()
        .unwrap_or_else(|| default_token_path(&options.socket_path, &options.workspace));
    // The default path lives under the install directory, which a first run has not
    // created yet; requiring the operator to mkdir it would be a startup failure
    // caused by the server's own choice of location.
    if let Some(parent) = token_file.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    TokenFile::new(&token_file).write(&tokens)?;
    println!(
        "credentials: {} ({} tokens, mode 0600)",
        token_file.display(),
        tokens.len()
    );

    let mut accepting = tokio::task::JoinSet::new();
    #[cfg(unix)]
    if let Some(path) = &options.socket_path {
        let listener = server
            .clone()
            .serve_unix(path)
            .await
            .map_err(|e| format!("--unix: {e}"))?;
        println!("listening: unix:{}", path.display());
        accepting.spawn(server.clone().accept_unix(listener));
    }
    if let Some(addr) = options.tcp_addr {
        let listener = server
            .clone()
            .serve_loopback_tcp(addr)
            .await
            .map_err(|e| format!("--tcp: {e}"))?;
        println!("listening: tcp://{addr}");
        accepting.spawn(server.clone().accept_tcp(listener));
    }
    if !options.forward_targets.is_empty() {
        // Said out loud because it is the line that tells someone reading the
        // startup output why a port exists on this host at all.
        println!(
            "forwarding: {} (a grant opens up to {} connections for up to {}s, \
             target must answer within {}s)",
            options
                .forward_targets
                .iter()
                .map(ForwardTarget::to_text)
                .collect::<Vec<_>>()
                .join(", "),
            options.forward_limits.max_uses,
            options.forward_limits.max_ttl.as_secs(),
            options.forward_connect_timeout.as_secs(),
        );
    }
    if accepting.is_empty() {
        return Err("nowhere to connect: pass --unix <path> and/or --tcp <host:port>".into());
    }
    // Whichever accept loop finishes first is the reason to stop: a server still
    // holding one socket is not worth keeping when the other one has gone.
    match accepting.join_next().await {
        Some(result) => result.map_err(|e| format!("accept loop: {e}"))?,
        None => return Err("the accept loops stopped".into()),
    }
    Ok(())
}

#[derive(Debug)]
struct Options {
    help: bool,
    version: Option<String>,
    workspace: PathBuf,
    socket_path: Option<PathBuf>,
    tcp_addr: Option<std::net::SocketAddr>,
    token_file: Option<PathBuf>,
    allowed: Vec<String>,
    capabilities: Vec<RemoteCapability>,
    install_dir: String,
    /// The signing key as it was typed: base64, or `@path` naming a file to read.
    signing_key: Option<String>,
    allow_unsigned_artifact: bool,
    token_ttl_secs: u64,
    max_exec_output_bytes: usize,
    initial_tokens: usize,
    /// The only services a forwarded local port may be pointed at.
    forward_targets: Vec<ForwardTarget>,
    forward_limits: ForwardLimits,
    forward_connect_timeout: std::time::Duration,
}

impl Options {
    /// Read the flags, keeping the difference between "not passed" and "passed
    /// empty" — `--allow ""` is a mistake worth reporting, not a missing flag.
    fn parse(argv: Vec<String>) -> Result<Self, String> {
        let mut help = false;
        let mut version = None;
        let mut workspace: Option<PathBuf> = None;
        let mut socket_path: Option<PathBuf> = None;
        let mut tcp_addr: Option<std::net::SocketAddr> = None;
        let mut token_file: Option<PathBuf> = None;
        let mut allowed: Vec<String> = Vec::new();
        // A workspace you can only read is not much use, so writes are offered
        // unless `--no-write` takes them away. `--capability` only adds.
        let mut capabilities: Vec<RemoteCapability> = vec![
            RemoteCapability::WorkspaceList,
            RemoteCapability::WorkspaceRead,
            RemoteCapability::WorkspaceSearch,
            RemoteCapability::WorkspaceWrite,
            RemoteCapability::Git,
        ];
        let mut offer_write = true;
        let mut install_dir = DEFAULT_INSTALL_DIR.to_string();
        let mut signing_key: Option<String> = None;
        let mut allow_unsigned_artifact = false;
        let mut token_ttl_secs = 600u64;
        let mut max_exec_output_bytes = 256 * 1024usize;
        // A credential is spent by one session, so a script that runs several
        // commands needs one per command.
        let mut initial_tokens = 8usize;
        // Forwarding is off until the operator names somewhere it may go, which is
        // the same rule `--allow` enforces for `exec`: a capability with an empty
        // list behind it can only ever refuse.
        let mut forward_targets: Vec<ForwardTarget> = Vec::new();
        let mut forward_limits = ForwardLimits::default();
        let mut forward_connect_timeout = std::time::Duration::from_secs(10);

        let mut index = 0;
        while index < argv.len() {
            match argv[index].as_str() {
                "--help" | "-h" => help = true,
                "--version" => version = Some(Implementation::local().to_string()),
                "--workspace" => {
                    workspace = Some(PathBuf::from(take(&argv, &mut index, "--workspace")?))
                }
                "--unix" => socket_path = Some(PathBuf::from(take(&argv, &mut index, "--unix")?)),
                "--tcp" => {
                    let text = take(&argv, &mut index, "--tcp")?;
                    let (host, port) = text
                        .rsplit_once(':')
                        .ok_or_else(|| format!("--tcp wants host:port, got {text:?}"))?;
                    let host = parse_loopback_host(host).map_err(|e| format!("--tcp: {e}"))?;
                    let port: u16 = port
                        .parse()
                        .map_err(|e| format!("--tcp port {port:?}: {e}"))?;
                    tcp_addr = Some(std::net::SocketAddr::new(host, port));
                }
                "--token-file" => {
                    token_file = Some(PathBuf::from(take(&argv, &mut index, "--token-file")?))
                }
                "--allow" => allowed.push(take(&argv, &mut index, "--allow")?),
                "--capability" => {
                    let text = take(&argv, &mut index, "--capability")?;
                    let capability =
                        parse_capability(&text).map_err(|e| format!("--capability: {e}"))?;
                    if !capabilities.contains(&capability) {
                        capabilities.push(capability);
                    }
                }
                "--no-write" => offer_write = false,
                "--install-dir" => install_dir = take(&argv, &mut index, "--install-dir")?,
                "--trust-signing-key" => {
                    signing_key = Some(take(&argv, &mut index, "--trust-signing-key")?)
                }
                "--allow-unsigned-artifact" => allow_unsigned_artifact = true,
                "--token-ttl" => {
                    token_ttl_secs = take(&argv, &mut index, "--token-ttl")?
                        .parse()
                        .map_err(|e| format!("--token-ttl: {e}"))?
                }
                "--tokens" => {
                    initial_tokens = take(&argv, &mut index, "--tokens")?
                        .parse()
                        .map_err(|e| format!("--tokens: {e}"))?
                }
                "--max-exec-output" => {
                    max_exec_output_bytes = take(&argv, &mut index, "--max-exec-output")?
                        .parse()
                        .map_err(|e| format!("--max-exec-output: {e}"))?
                }
                "--allow-forward-to" => {
                    let text = take(&argv, &mut index, "--allow-forward-to")?;
                    forward_targets.push(
                        parse_forward_target(&text)
                            .map_err(|e| format!("--allow-forward-to: {e}"))?,
                    );
                }
                "--forward-ttl" => {
                    let secs: u64 = take(&argv, &mut index, "--forward-ttl")?
                        .parse()
                        .map_err(|e| format!("--forward-ttl: {e}"))?;
                    forward_limits.max_ttl = std::time::Duration::from_secs(secs);
                }
                "--forward-max-uses" => {
                    forward_limits.max_uses = take(&argv, &mut index, "--forward-max-uses")?
                        .parse()
                        .map_err(|e| format!("--forward-max-uses: {e}"))?
                }
                "--forward-connect-timeout" => {
                    let secs: u64 = take(&argv, &mut index, "--forward-connect-timeout")?
                        .parse()
                        .map_err(|e| format!("--forward-connect-timeout: {e}"))?;
                    forward_connect_timeout = std::time::Duration::from_secs(secs);
                }
                other => {
                    return Err(format!("unknown argument {other:?}\n\n{USAGE}"));
                }
            }
            index += 1;
        }
        if !help && version.is_none() {
            let workspace =
                workspace.ok_or_else(|| format!("--workspace is required\n\n{USAGE}"))?;
            if !workspace.is_dir() {
                return Err(format!(
                    "--workspace {} is not a directory",
                    workspace.display()
                ));
            }
            if !offer_write {
                capabilities.retain(|capability| *capability != RemoteCapability::WorkspaceWrite);
            }
            return Ok(Self {
                help,
                version,
                workspace,
                socket_path,
                tcp_addr,
                token_file,
                allowed,
                capabilities,
                install_dir,
                signing_key,
                allow_unsigned_artifact,
                token_ttl_secs,
                max_exec_output_bytes,
                initial_tokens,
                forward_targets,
                forward_limits,
                forward_connect_timeout,
            });
        }
        Ok(Self {
            help,
            version,
            workspace: workspace.unwrap_or_default(),
            socket_path,
            tcp_addr,
            token_file,
            allowed,
            capabilities,
            install_dir,
            signing_key,
            allow_unsigned_artifact,
            token_ttl_secs,
            max_exec_output_bytes,
            initial_tokens,
            forward_targets,
            forward_limits,
            forward_connect_timeout,
        })
    }

    /// Anything the flag parser could not judge without the server's own rules.
    fn into_config(mut self) -> Result<Self, String> {
        if self.socket_path.is_none() && self.tcp_addr.is_none() {
            return Err("nowhere to connect: pass --unix <path> and/or --tcp <host:port>".into());
        }
        if self.initial_tokens == 0 {
            return Err("--tokens 0 would publish nothing to authenticate with".into());
        }
        if self.token_ttl_secs == 0 {
            return Err("--token-ttl 0 would expire every credential before it is used".into());
        }
        if self.capabilities.is_empty() {
            return Err("every capability has been turned off; there is no server left".into());
        }
        // Naming a target *is* the decision to forward; asking for the capability on
        // top of that would be two ways to say one thing.
        if !self.forward_targets.is_empty()
            && !self.capabilities.contains(&RemoteCapability::PortForward)
        {
            self.capabilities.push(RemoteCapability::PortForward);
        }
        if self.forward_targets.is_empty()
            && self.capabilities.contains(&RemoteCapability::PortForward)
        {
            return Err(
                "--capability port-forward with no --allow-forward-to would refuse \
                        every forward; name at least one target (host:port), or drop the \
                        capability"
                    .into(),
            );
        }
        if self.forward_limits.max_uses == 0 {
            return Err("--forward-max-uses 0 would refuse every forward before it started".into());
        }
        if self.forward_limits.max_ttl.is_zero() {
            return Err("--forward-ttl 0 would expire a grant before it could be used".into());
        }
        if self.forward_connect_timeout.is_zero() {
            return Err(
                "--forward-connect-timeout 0 would refuse a target before it had a \
                        chance to answer"
                    .into(),
            );
        }
        Ok(self)
    }
}

/// Consume the argument after a flag.
fn take(argv: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    *index += 1;
    argv.get(*index)
        .cloned()
        .ok_or_else(|| format!("{flag} needs a value"))
}

/// Where the credentials go when `--token-file` was not passed: beside the socket,
/// because that is the directory only this user is expected to reach.
fn default_token_path(socket: &Option<PathBuf>, workspace: &std::path::Path) -> PathBuf {
    match socket {
        Some(path) => path.with_extension("tokens"),
        None => workspace.join(".chaos-server").join("tokens"),
    }
}

/// The key material behind `--trust-signing-key`: base64 typed straight in, or
/// `@path` for a file, which is how a key that came out of a vault gets passed
/// without landing in a shell history line.
fn read_signing_key(value: &str) -> Result<String, String> {
    let Some(path) = value.strip_prefix('@') else {
        return Ok(value.to_string());
    };
    if path.is_empty() {
        return Err("--trust-signing-key wants @path or the base64 key itself".to_string());
    }
    std::fs::read_to_string(path)
        .map_err(|e| format!("--trust-signing-key: read {path}: {e}"))
        .map(|text| text.trim().to_string())
}

/// What this host will require of an artifact before it becomes current.
///
/// The key comes from the flag, then `CHAOS_SIGNING_PUBLIC_KEY`, then the build;
/// whether a signature must be present comes from `--allow-unsigned-artifact` and
/// `CHAOS_REMOTE_REQUIRE_SIGNATURE`. A key that was typed out wrong is a startup
/// failure here rather than a policy that refuses every install later — the operator
/// learns about it while they can still fix it.
fn provenance_policy(options: &Options) -> Result<ProvenancePolicy, String> {
    let policy = match options.signing_key.as_deref() {
        Some(typed) => {
            let key = read_signing_key(typed)?;
            ProvenancePolicy::trusting_key_b64(&key)
                .map_err(|e| format!("--trust-signing-key: {e}"))?
                .with_requirement(requirement_requested_by_env())
        }
        None => ProvenancePolicy::from_env(),
    };
    Ok(if options.allow_unsigned_artifact {
        policy.with_requirement(false)
    } else {
        policy
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn parse(argv: &[&str]) -> Result<Options, String> {
        Options::parse(argv.iter().map(|arg| (*arg).to_string()).collect())
    }

    /// The documented default: a session may read, search and change the workspace,
    /// and may not run anything, because running needs an allowlist to mean anything.
    #[test]
    fn writes_are_offered_by_default_and_tool_execution_is_not() {
        let options = parse(&["--workspace", "."]).expect("defaults");
        assert!(
            options
                .capabilities
                .contains(&RemoteCapability::WorkspaceWrite)
        );
        assert!(
            !options
                .capabilities
                .contains(&RemoteCapability::ToolExecution)
        );
        assert!(options.allowed.is_empty());
    }

    #[test]
    fn no_write_takes_writes_away_rather_than_being_a_no_op() {
        let options = parse(&["--workspace", ".", "--no-write"]).expect("parsed");
        assert!(
            !options
                .capabilities
                .contains(&RemoteCapability::WorkspaceWrite),
            "{:?}",
            options.capabilities
        );
        assert!(
            options
                .capabilities
                .contains(&RemoteCapability::WorkspaceRead)
        );
    }

    /// A capability the server cannot honour is named and explained; "unknown"
    /// would send the operator looking for a typo in a missing feature.
    #[test]
    fn a_capability_that_does_not_exist_says_which_one_and_why() {
        for (name, fragment) in [("interactive-pty", "PTY"), ("detached-agent", "detached")] {
            let err = parse(&["--workspace", ".", "--capability", name]).expect_err("not offered");
            assert!(err.contains(fragment), "{name}: {err}");
        }
        let err = parse(&["--workspace", ".", "--capability", "telepathy"])
            .expect_err("not a capability");
        assert!(err.contains("tool-execution"), "{err}");
    }

    /// Forwarding is off until a target is named, and naming one is enough: the
    /// capability follows the allowlist rather than being a second thing to remember.
    #[test]
    fn forwarding_stays_off_until_a_target_is_named_and_then_comes_on() {
        let plain = parse(&["--workspace", ".", "--unix", "/tmp/s"]).expect("defaults");
        assert!(plain.forward_targets.is_empty());
        assert!(
            !plain.capabilities.contains(&RemoteCapability::PortForward),
            "{:?}",
            plain.capabilities
        );

        let options = parse(&[
            "--workspace",
            ".",
            "--allow-forward-to",
            "127.0.0.1:8090",
            "--allow-forward-to",
            "[::1]:6379",
            "--unix",
            "/tmp/s",
        ])
        .expect("parsed")
        .into_config()
        .expect("a target is a decision to forward");
        assert_eq!(
            options
                .forward_targets
                .iter()
                .map(ForwardTarget::to_text)
                .collect::<Vec<_>>(),
            vec!["127.0.0.1:8090", "[::1]:6379"]
        );
        assert!(
            options
                .capabilities
                .contains(&RemoteCapability::PortForward),
            "{:?}",
            options.capabilities
        );
    }

    /// The ceilings are the server's, so they are worth being able to set; and a
    /// ceiling of zero is a refusal waiting to happen, which is refused now.
    #[test]
    fn the_forward_ceilings_can_be_raised_but_not_zeroed() {
        let options = parse(&[
            "--workspace",
            ".",
            "--allow-forward-to",
            "127.0.0.1:8090",
            "--forward-ttl",
            "90",
            "--forward-max-uses",
            "3",
            "--forward-connect-timeout",
            "2",
            "--unix",
            "/tmp/s",
        ])
        .expect("parsed");
        assert_eq!(options.forward_limits.max_ttl.as_secs(), 90);
        assert_eq!(options.forward_limits.max_uses, 3);
        assert_eq!(options.forward_connect_timeout.as_secs(), 2);

        for (flag, value) in [
            ("--forward-ttl", "0"),
            ("--forward-max-uses", "0"),
            ("--forward-connect-timeout", "0"),
        ] {
            let err = parse(&[
                "--workspace",
                ".",
                "--allow-forward-to",
                "127.0.0.1:8090",
                flag,
                value,
                "--unix",
                "/tmp/s",
            ])
            .expect("parsed")
            .into_config()
            .expect_err("a zero ceiling forwards nothing");
            assert!(err.contains(flag), "{flag}={value}: {err}");
        }
    }

    /// Advertising forwarding with nowhere allowed to go collects refusals; the
    /// operator is told which flag is missing instead.
    #[test]
    fn the_capability_alone_is_refused_because_it_can_only_refuse() {
        let err = parse(&[
            "--workspace",
            ".",
            "--capability",
            "port-forward",
            "--unix",
            "/tmp/s",
        ])
        .expect("parsed")
        .into_config()
        .expect_err("no targets");
        assert!(err.contains("--allow-forward-to"), "{err}");

        let err = parse(&[
            "--workspace",
            ".",
            "--allow-forward-to",
            "8090",
            "--unix",
            "/tmp/s",
        ])
        .expect_err("a port with no host is not a target");
        assert!(err.contains("--allow-forward-to"), "{err}");
    }

    #[test]
    fn a_flag_without_its_value_and_a_routable_host_are_both_refused() {
        assert!(parse(&["--workspace"]).is_err());
        let err = parse(&["--workspace", ".", "--tcp", "0.0.0.0:4100"]).expect_err("not loopback");
        assert!(err.contains("loopback"), "{err}");
        assert!(parse(&["--workspace", ".", "--tcp", "not-a-port"]).is_err());
    }

    /// Startup refuses the configurations that would leave nothing usable behind.
    /// Each session spends one credential, so a script that runs more commands than
    /// the default would otherwise stop working partway through.
    #[test]
    fn the_number_of_published_credentials_can_be_raised_but_not_emptied() {
        let options = parse(&["--workspace", ".", "--tokens", "64"]).expect("parsed");
        assert_eq!(options.initial_tokens, 64);
        assert_eq!(
            parse(&["--workspace", "."])
                .expect("default")
                .initial_tokens,
            8
        );
        let empty = parse(&["--workspace", ".", "--tokens", "0", "--unix", "/tmp/s"])
            .expect("parsed")
            .into_config();
        assert!(empty.expect_err("no credentials").contains("--tokens"));
    }

    #[test]
    fn a_configuration_with_nothing_to_serve_is_refused_before_binding() {
        let nowhere = parse(&["--workspace", "."]).expect("parsed").into_config();
        assert!(nowhere.expect_err("no listener").contains("--unix"));

        let ttl_zero = parse(&["--workspace", ".", "--unix", "/tmp/s", "--token-ttl", "0"])
            .expect("parsed")
            .into_config();
        assert!(ttl_zero.expect_err("ttl 0").contains("--token-ttl"));

        let missing_dir = parse(&["--workspace", "/definitely/not/here", "--unix", "/tmp/s"]);
        assert!(
            missing_dir
                .expect_err("not a directory")
                .contains("not a directory")
        );
    }

    /// The default credential path is under the install directory, which a first run
    /// has not created yet; the server has to create it rather than fail.
    #[test]
    fn the_default_token_path_is_beside_the_socket_or_under_the_install_dir() {
        let socket = Some(PathBuf::from("/run/user/1000/chaos.sock"));
        assert_eq!(
            default_token_path(&socket, Path::new("/srv/work")),
            PathBuf::from("/run/user/1000/chaos.tokens")
        );
        assert_eq!(
            default_token_path(&None, Path::new("/srv/work")),
            PathBuf::from("/srv/work/.chaos-server/tokens")
        );
    }
}
