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
    DEFAULT_INSTALL_DIR, Implementation, InstallLayout, RemoteCapability, Server, ServerConfig,
    TokenFile, parse_capability, parse_loopback_host,
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
  --token-ttl <seconds>   how long an unused credential stays usable (default 600)
  --tokens <count>        how many one-time credentials to publish at startup
                          (default 8; each session spends one)
  --max-exec-output <bytes>  output cap for one tool run (default 262144)

By default list, read, search, write and git are offered. Tool execution is not,
because it needs an `--allow` list to mean anything, and an empty one runs nothing.

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

    let config = ServerConfig::new(&options.workspace)
        .capabilities(options.capabilities)
        .allowed_executables(options.allowed)
        .token_ttl(std::time::Duration::from_secs(options.token_ttl_secs))
        .max_exec_output_bytes(options.max_exec_output_bytes)
        .install_layout(InstallLayout::with_dir_name(
            &options.workspace,
            &options.install_dir,
        ));
    let server = Server::new(config)?;

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
    token_ttl_secs: u64,
    max_exec_output_bytes: usize,
    initial_tokens: usize,
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
        let mut token_ttl_secs = 600u64;
        let mut max_exec_output_bytes = 256 * 1024usize;
        // A credential is spent by one session, so a script that runs several
        // commands needs one per command.
        let mut initial_tokens = 8usize;

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
                token_ttl_secs,
                max_exec_output_bytes,
                initial_tokens,
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
            token_ttl_secs,
            max_exec_output_bytes,
            initial_tokens,
        })
    }

    /// Anything the flag parser could not judge without the server's own rules.
    fn into_config(self) -> Result<Self, String> {
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
        for (name, fragment) in [
            ("port-forward", "port forwarding"),
            ("interactive-pty", "PTY"),
            ("detached-agent", "detached"),
        ] {
            let err = parse(&["--workspace", ".", "--capability", name]).expect_err("not offered");
            assert!(err.contains(fragment), "{name}: {err}");
        }
        let err = parse(&["--workspace", ".", "--capability", "telepathy"])
            .expect_err("not a capability");
        assert!(err.contains("tool-execution"), "{err}");
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
