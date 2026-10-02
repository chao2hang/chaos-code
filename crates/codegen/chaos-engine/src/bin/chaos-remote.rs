//! Drive one remote workspace from the command line.
//!
//! This is the client half of [`chaos-remote-server`], and it exists so that the
//! transport can be driven, scripted and accepted without writing Rust first. The
//! library is the API; this is its thinnest possible wrapper plus the output shape
//! a shell or a CI step can assert on.
//!
//! ```text
//! chaos-remote --unix /run/user/1000/chaos.sock --token-file ./tokens list src
//! chaos-remote --tcp 127.0.0.1:4100 --token-file ./tokens exec -- cargo test -p chaos-engine
//! ```
//!
//! Global flags come before the command; everything after `exec --` is handed to the
//! remote program untouched.
//!
//! The credential file holds one-time credentials, one per line, oldest first. A
//! successful connect spends one, so this program rewrites the file without it —
//! otherwise the next invocation of the same script would fail on a credential that
//! was already used.

use std::io::{Read as _, Write as _};
use std::path::PathBuf;
use std::time::Duration;

use chaos_engine::remote::{
    DialRetry, ForwardTarget, ForwardTunnel, InstallOutcome, RemoteCapability, RemoteEndpoint,
    RemoteError, RemoteWorkspace, RemoteWorkspaceConfig, SessionToken, TokenFile, WriteFile,
    parse_capability, parse_forward_target, parse_listen_endpoint,
};

const USAGE: &str = "\
chaos-remote — drive one remote workspace

  chaos-remote [global flags] <command> [command flags]

connection (exactly one is required)
  --unix <path>           the server's Unix socket
  --tcp <host:port>       the server's loopback TCP port (a tunnel's local end)

credential (exactly one is required)
  --token <text>          the one-time session credential
  --token-file <path>     a file of credentials, oldest first; the one used is
                          removed from the file so the next run has a live one

  --host <name>           the remote host this endpoint stands for (default buildbox)
  --port <port>           that host's own port (default 22); not the dial address
  --connect-wait <secs>   keep re-dialling for this long while the transport (a
                          tunnel, a socket-activated server) is not up yet; only
                          the dial is repeated, never the credential
  --reply-timeout <secs>  give up on a reply — the handshake's or a request's —
                          that has not arrived, and end the session: a late reply
                          would answer the wrong question, so this bounds being
                          stuck rather than retrying a slow server
  --capability <name>     ask for less than everything; repeatable
  --json                  machine-readable output
  --quiet                 only what the command itself produces
  --help, -h              this text

commands
  ping                    round-trip and report the negotiated protocol
  info                    protocol, server, session and granted capabilities
  list [path] [--depth N] [--max N]
  cat <path>              the whole file, paged
  read <path> [--offset N] [--length N] [--out FILE]
  search <query> [path] [--max N] [--case-sensitive]
  write <path> (--from FILE | --text TEXT | --stdin) [--mkdir] [--expect SHA256]
  diff [path] [--staged] [--context N]
  exec [--cwd PATH] [--timeout SECONDS] -- <program> [args...]
  install <version> --from FILE
  forward --to HOST:PORT [--listen ADDR] [--connections N] [--ttl SECONDS]
                          (default --listen 0, i.e. a free loopback port, up to
                          16 connections, for 1800s — both of which the server
                          answers with what it will actually agree to)
                          listen on a local port and carry every connection that
                          arrives there to that service, through the session;
                          the listener is on loopback and goes when the grant
                          does, so nothing is left forwarding behind

forwarding is the client's local end: the service it names is somewhere the
*server* can reach, and the server only agrees to targets its operator listed.

Exit status: 0 on success, 1 when the request failed. `exec` alone propagates the
remote program's exit status, so a failing build and a failing connection are
distinguishable by whoever called the script.";

#[tokio::main(flavor = "multi_thread")]
async fn main() -> std::process::ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match run(argv).await {
        Ok(status) => std::process::ExitCode::from(status),
        Err(message) => {
            eprintln!("chaos-remote: {message}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run(argv: Vec<String>) -> Result<u8, String> {
    let args = Args::parse(argv)?;
    if args.help {
        print!("{USAGE}");
        return Ok(0);
    }
    let mut session = args.connect().await?;
    let status = dispatch(&mut session, &args).await;
    // The server drops the session when the socket closes; saying goodbye is
    // courtesy, and a refusal there must not disguise the command's own result.
    session.close().await;
    status
}

async fn dispatch(session: &mut AnySession, args: &Args) -> Result<u8, String> {
    match &args.command {
        Command::Ping => {
            let version = session.ping().await.map_err(describe)?;
            if args.json {
                emit(&serde_json::json!({ "protocol_version": version }))?;
            } else if !args.quiet {
                println!("pong: protocol {version}");
            }
            Ok(0)
        }
        Command::Info => {
            let granted: Vec<String> = session
                .capabilities()
                .iter()
                .map(|capability| capability.as_str().to_string())
                .collect();
            if args.json {
                emit(&serde_json::json!({
                    "protocol_version": session.protocol_version(),
                    "server": session.server().to_string(),
                    "session": session.session_id(),
                    "capabilities": granted,
                }))?;
            } else {
                println!("protocol: {}", session.protocol_version());
                println!("server: {server}", server = session.server());
                println!("session: {}", session.session_id());
                println!("capabilities: {}", granted.join(", "));
            }
            Ok(0)
        }
        Command::List { path, depth, max } => {
            let (entries, truncated) = session
                .list(path.as_deref(), *depth, *max)
                .await
                .map_err(describe)?;
            if args.json {
                emit(&serde_json::json!({ "entries": entries, "truncated": truncated }))?;
            } else {
                for entry in &entries {
                    let kind = if entry.is_dir { "d" } else { "-" };
                    println!("{kind} {:>12}  {}", entry.len, entry.path);
                }
                if truncated {
                    eprintln!("(truncated)");
                }
            }
            Ok(0)
        }
        Command::Cat { path } => {
            let file = session.read_file(path).await.map_err(describe)?;
            emit_bytes(args, &file.data, &file.sha256)?;
            Ok(0)
        }
        Command::Read {
            path,
            offset,
            length,
            out,
        } => {
            let window = session
                .read(path, *offset, *length)
                .await
                .map_err(describe)?;
            if let Some(out) = out {
                std::fs::write(out, &window.data)
                    .map_err(|e| format!("write {}: {e}", out.display()))?;
            } else if args.json {
                emit(&serde_json::json!({
                    "path": path,
                    "offset": window.offset,
                    "total_len": window.total_len,
                    "sha256": window.sha256,
                    "bytes": window.data.len(),
                }))?;
            } else {
                let stdout = std::io::stdout();
                let mut stdout = stdout.lock();
                stdout
                    .write_all(&window.data)
                    .map_err(|e| format!("write stdout: {e}"))?;
                stdout.flush().map_err(|e| format!("flush stdout: {e}"))?;
            }
            if !args.quiet && !args.json && out.is_none() {
                eprintln!(
                    "(bytes {} of {} at offset {}, sha256 {})",
                    window.data.len(),
                    window.total_len,
                    window.offset,
                    window.sha256
                );
            }
            Ok(0)
        }
        Command::Search {
            query,
            path,
            max,
            case_sensitive,
        } => {
            let found = session
                .search(query, path.as_deref(), *max, *case_sensitive)
                .await
                .map_err(describe)?;
            if args.json {
                emit(&serde_json::json!({
                    "hits": found.hits,
                    "truncated": found.truncated,
                    "files_scanned": found.files_scanned,
                }))?;
            } else {
                for hit in &found.hits {
                    println!("{}:{}:{}", hit.path, hit.line, hit.text);
                }
                if found.truncated {
                    eprintln!("(truncated after {} files)", found.files_scanned);
                }
            }
            Ok(0)
        }
        Command::Write {
            path,
            from,
            text,
            stdin,
            mkdir,
            expect,
        } => {
            let content: Vec<u8> = if let Some(from) = from {
                std::fs::read(from).map_err(|e| format!("read {}: {e}", from.display()))?
            } else if let Some(text) = text {
                text.as_bytes().to_vec()
            } else if *stdin {
                let mut buffer = Vec::new();
                std::io::stdin()
                    .read_to_end(&mut buffer)
                    .map_err(|e| format!("read stdin: {e}"))?;
                buffer
            } else {
                return Err("write needs --from, --text or --stdin".into());
            };
            let (written, sha256) = session
                .write(
                    path,
                    WriteFile {
                        content: &content,
                        create_parents: *mkdir,
                        expected_sha256: expect.as_deref(),
                    },
                )
                .await
                .map_err(describe)?;
            if args.json {
                emit(&serde_json::json!({
                    "path": path, "written": written, "sha256": sha256,
                }))?;
            } else if !args.quiet {
                println!("wrote {written} bytes to {path} (sha256 {sha256})");
            }
            Ok(0)
        }
        Command::Diff {
            path,
            staged,
            context,
        } => {
            let (diff, dirty) = session
                .diff(path.as_deref(), *staged, *context)
                .await
                .map_err(describe)?;
            if args.json {
                emit(&serde_json::json!({ "diff": diff, "dirty": dirty }))?;
            } else {
                print!("{diff}");
                if !dirty && !args.quiet {
                    eprintln!("(nothing changed)");
                }
            }
            Ok(0)
        }
        Command::Exec { argv, cwd, timeout } => {
            let Some((program, rest)) = argv.split_first() else {
                return Err("exec needs a program after `--`".into());
            };
            let mut request: Vec<&str> = Vec::with_capacity(rest.len() + 1);
            request.push(program.as_str());
            request.extend(rest.iter().map(String::as_str));
            let outcome = session
                .exec(&request, cwd.as_deref(), *timeout)
                .await
                .map_err(describe)?;
            if args.json {
                emit(&serde_json::json!({
                    "exit_code": outcome.exit_code,
                    "stdout": outcome.stdout,
                    "stderr": outcome.stderr,
                    "timed_out": outcome.timed_out,
                    "truncated": outcome.truncated,
                }))?;
            } else {
                print!("{}", outcome.stdout);
                eprint!("{}", outcome.stderr);
                if outcome.timed_out {
                    eprintln!("chaos-remote: the program hit its timeout and was killed");
                }
                if outcome.truncated {
                    eprintln!("chaos-remote: output was cut off at the server's cap");
                }
            }
            // The remote program's status is the caller's status; a killed or
            // timed-out program is a failure with no status of its own.
            Ok(match outcome.exit_code {
                Some(0) => 0,
                Some(code) => code.clamp(1, 255) as u8,
                None if outcome.timed_out => 124,
                None => 1,
            })
        }
        Command::Install { version, from } => {
            let artifact =
                std::fs::read(from).map_err(|e| format!("read {}: {e}", from.display()))?;
            let outcome: InstallOutcome = session
                .install_artifact(version, &artifact)
                .await
                .map_err(describe)?;
            if args.json {
                emit(&serde_json::json!({
                    "version": outcome.version,
                    "current": outcome.current,
                    "previous_version": outcome.previous_version,
                }))?;
            } else if outcome.current {
                println!("installed {version} and made it current");
                // Worth saying out loud: the process answering this call was
                // started from the old artifact and keeps running. `install`
                // changes what the next start runs, not what is running now, and
                // an operator who misses that believes the host was upgraded.
                eprintln!(
                    "the server answering this session is still the previous build; \
                     restart it to run {version}"
                );
            } else {
                println!(
                    "installed {version} but it did not become current; still on {:?}",
                    outcome.previous_version
                );
            }
            Ok(if outcome.current { 0 } else { 1 })
        }
        Command::Forward {
            target,
            listen,
            connections,
            ttl,
        } => {
            // The grant is asked for first, so a refusal — a target the operator
            // did not list, a capability this session does not hold — arrives
            // before anything is listening that would suggest otherwise.
            let grant = session
                .forward_grant(target, *ttl, *connections)
                .await
                .map_err(describe)?;
            let (allowed, lifetime) = (grant.max_uses, grant.expires_in);
            let (endpoint, config) = args.endpoint_and_config();
            let mut tunnel = ForwardTunnel::bind(endpoint, grant, config, *listen)
                .await
                .map_err(describe)?;
            let local = tunnel.local_addr().map_err(describe)?;
            // The port is only known once the listener exists, and whoever started
            // the forward needs it before they can use it. It goes to stderr so
            // that stdout stays a single thing to parse.
            eprintln!(
                "chaos-remote: forwarding {local} -> {target} (the server agreed to \
                 {allowed} connection(s) for {}s); Ctrl-C or closing the session stops it",
                lifetime.as_secs()
            );
            let stats = match args.transport()? {
                #[cfg(unix)]
                Transport::Unix(path) => {
                    let mut dial = move || Box::pin(redial_unix(path.clone()));
                    tunnel.serve(&mut dial).await
                }
                Transport::Tcp(addr) => {
                    let mut dial = move || Box::pin(redial_tcp(addr));
                    tunnel.serve(&mut dial).await
                }
            }
            .map_err(describe)?;
            if args.json {
                emit(&serde_json::json!({
                    "listen": local.to_string(),
                    "target": target.to_string(),
                    "connections": stats.connections,
                    "refused": stats.refused,
                    "bytes_to_target": stats.bytes_to_target,
                    "bytes_to_client": stats.bytes_to_client,
                }))?;
            } else if !args.quiet {
                println!(
                    "forwarded {} connection(s) to {target}: {} bytes to the target, \
                     {} back{}",
                    stats.connections,
                    stats.bytes_to_target,
                    stats.bytes_to_client,
                    if stats.refused == 0 {
                        String::new()
                    } else {
                        format!(", {} refused", stats.refused)
                    },
                );
            }
            Ok(0)
        }
    }
}

/// A session over either transport. The CLI has to hold one value across an `await`,
/// and the two stream types are unrelated, so the transport is chosen at runtime.
enum AnySession {
    Unix(Box<RemoteWorkspace<tokio::net::UnixStream>>),
    Tcp(Box<RemoteWorkspace<tokio::net::TcpStream>>),
}

macro_rules! proxy {
    ($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty) => {
        async fn $name(&mut self $(, $arg: $ty)*) -> $ret {
            match self {
                AnySession::Unix(session) => session.$name($($arg),*).await,
                AnySession::Tcp(session) => session.$name($($arg),*).await,
            }
        }
    };
}

impl AnySession {
    proxy!(ping() -> Result<u32, RemoteError>);
    proxy!(
        list(path: Option<&str>, depth: Option<usize>, max: Option<usize>)
            -> Result<(Vec<chaos_engine::remote::Entry>, bool), RemoteError>
    );
    proxy!(
        read(path: &str, offset: Option<u64>, length: Option<u64>)
            -> Result<chaos_engine::remote::ReadWindow, RemoteError>
    );
    proxy!(
        read_file(path: &str) -> Result<chaos_engine::remote::FileContents, RemoteError>
    );
    proxy!(
        search(query: &str, path: Option<&str>, max: Option<usize>, case_sensitive: bool)
            -> Result<chaos_engine::remote::SearchOutcome, RemoteError>
    );
    proxy!(
        write(path: &str, file: WriteFile<'_>) -> Result<(u64, String), RemoteError>
    );
    proxy!(
        diff(path: Option<&str>, staged: bool, context: Option<u32>)
            -> Result<(String, bool), RemoteError>
    );
    proxy!(
        exec(argv: &[&str], cwd: Option<&str>, timeout: Option<Duration>)
            -> Result<chaos_engine::remote::ExecOutcome, RemoteError>
    );
    proxy!(
        install_artifact(version: &str, artifact: &[u8])
            -> Result<InstallOutcome, RemoteError>
    );
    proxy!(
        forward_grant(target: &ForwardTarget, ttl: Duration, max_uses: usize)
            -> Result<chaos_engine::remote::ForwardGrant, RemoteError>
    );

    fn protocol_version(&self) -> u32 {
        match self {
            AnySession::Unix(session) => session.protocol_version(),
            AnySession::Tcp(session) => session.protocol_version(),
        }
    }

    fn server(&self) -> String {
        match self {
            AnySession::Unix(session) => session.server().to_string(),
            AnySession::Tcp(session) => session.server().to_string(),
        }
    }

    fn session_id(&self) -> String {
        match self {
            AnySession::Unix(session) => session.session_id().to_string(),
            AnySession::Tcp(session) => session.session_id().to_string(),
        }
    }

    fn capabilities(&self) -> &[RemoteCapability] {
        match self {
            AnySession::Unix(session) => session.capabilities(),
            AnySession::Tcp(session) => session.capabilities(),
        }
    }

    async fn close(self) {
        match self {
            AnySession::Unix(session) => session.close().await,
            AnySession::Tcp(session) => session.close().await,
        }
    }
}

#[derive(Debug)]
struct Args {
    help: bool,
    json: bool,
    quiet: bool,
    unix: Option<PathBuf>,
    tcp: Option<std::net::SocketAddr>,
    token: Option<String>,
    token_file: Option<PathBuf>,
    host: String,
    port: u16,
    /// How long to keep re-dialling a transport that is not up yet.
    connect_wait: Duration,
    /// Per-request reply deadline; `None` waits, which is what `exec` needs.
    reply_timeout: Option<Duration>,
    capabilities: Option<Vec<RemoteCapability>>,
    command: Command,
}

#[derive(Debug)]
enum Command {
    /// The placeholder a parse starts from; `parse` replaces it or fails.
    Ping,
    Info,
    List {
        path: Option<String>,
        depth: Option<usize>,
        max: Option<usize>,
    },
    Cat {
        path: String,
    },
    Read {
        path: String,
        offset: Option<u64>,
        length: Option<u64>,
        out: Option<PathBuf>,
    },
    Search {
        query: String,
        path: Option<String>,
        max: Option<usize>,
        case_sensitive: bool,
    },
    Write {
        path: String,
        from: Option<PathBuf>,
        text: Option<String>,
        stdin: bool,
        mkdir: bool,
        expect: Option<String>,
    },
    Diff {
        path: Option<String>,
        staged: bool,
        context: Option<u32>,
    },
    Exec {
        argv: Vec<String>,
        cwd: Option<String>,
        timeout: Option<Duration>,
    },
    Install {
        version: String,
        from: PathBuf,
    },
    Forward {
        target: ForwardTarget,
        listen: std::net::SocketAddr,
        connections: usize,
        ttl: Duration,
    },
}

impl Default for Args {
    fn default() -> Self {
        Self {
            help: false,
            json: false,
            quiet: false,
            unix: None,
            tcp: None,
            token: None,
            token_file: None,
            host: "buildbox".into(),
            port: 22,
            connect_wait: Duration::ZERO,
            reply_timeout: None,
            capabilities: None,
            command: Command::Ping,
        }
    }
}

impl Args {
    fn parse(argv: Vec<String>) -> Result<Self, String> {
        let mut args = Args {
            host: "buildbox".into(),
            port: 22,
            ..Args::default()
        };
        let mut index = 0;
        // Global flags first, then the command, then whatever the command wants.
        // Splitting there is what lets `exec -- git commit -m "x"` carry `-m`
        // through to the remote program instead of into this parser.
        while index < argv.len() {
            match argv[index].as_str() {
                "--help" | "-h" => args.help = true,
                "--json" => args.json = true,
                "--quiet" => args.quiet = true,
                "--unix" => args.unix = Some(PathBuf::from(take(&argv, &mut index, "--unix")?)),
                "--tcp" => {
                    let text = take(&argv, &mut index, "--tcp")?;
                    args.tcp = Some(text.parse().map_err(|e| format!("--tcp {text:?}: {e}"))?);
                }
                "--token" => args.token = Some(take(&argv, &mut index, "--token")?),
                "--token-file" => {
                    args.token_file = Some(PathBuf::from(take(&argv, &mut index, "--token-file")?))
                }
                "--host" => args.host = take(&argv, &mut index, "--host")?,
                "--connect-wait" => {
                    let secs: u64 = take(&argv, &mut index, "--connect-wait")?
                        .parse()
                        .map_err(|e| format!("--connect-wait: {e}"))?;
                    args.connect_wait = Duration::from_secs(secs);
                }
                "--reply-timeout" => {
                    let secs: u64 = take(&argv, &mut index, "--reply-timeout")?
                        .parse()
                        .map_err(|e| format!("--reply-timeout: {e}"))?;
                    if secs == 0 {
                        return Err(
                            "--reply-timeout 0 would abandon every request before its reply".into(),
                        );
                    }
                    args.reply_timeout = Some(Duration::from_secs(secs));
                }
                "--port" => {
                    args.port = take(&argv, &mut index, "--port")?
                        .parse()
                        .map_err(|e| format!("--port: {e}"))?
                }
                "--capability" => {
                    let text = take(&argv, &mut index, "--capability")?;
                    let capability = parse_capability(&text)?;
                    args.capabilities
                        .get_or_insert_with(Vec::new)
                        .push(capability);
                }
                other if other.starts_with('-') => {
                    return Err(format!("unknown flag {other:?}\n\n{USAGE}"));
                }
                _ => break,
            }
            index += 1;
        }
        if args.help {
            return Ok(args);
        }
        if index >= argv.len() {
            return Err(format!("no command given\n\n{USAGE}"));
        }
        let command = argv[index].as_str();
        let rest = argv[index + 1..].to_vec();
        let (parsed, allowed, positionals): (Command, &[&str], usize) = match command {
            "ping" => (Command::Ping, &[][..], 0),
            "info" => (Command::Info, &[][..], 0),
            "list" => (
                Command::List {
                    path: positional(&rest, 0),
                    depth: value_of(&rest, "--depth")?
                        .map(|d| d.parse().map_err(|e| format!("--depth: {e}")))
                        .transpose()?,
                    max: value_of(&rest, "--max")?
                        .map(|d| d.parse().map_err(|e| format!("--max: {e}")))
                        .transpose()?,
                },
                &["--depth", "--max"],
                1,
            ),
            "cat" => (
                Command::Cat {
                    path: require_positional(&rest, 0, "cat needs a path")?,
                },
                &[],
                1,
            ),
            "read" => (
                Command::Read {
                    path: require_positional(&rest, 0, "read needs a path")?,
                    offset: value_of(&rest, "--offset")?
                        .map(|d| d.parse().map_err(|e| format!("--offset: {e}")))
                        .transpose()?,
                    length: value_of(&rest, "--length")?
                        .map(|d| d.parse().map_err(|e| format!("--length: {e}")))
                        .transpose()?,
                    out: value_of(&rest, "--out")?.map(PathBuf::from),
                },
                &["--offset", "--length", "--out"],
                1,
            ),
            "search" => (
                Command::Search {
                    query: require_positional(&rest, 0, "search needs a query")?,
                    path: positional(&rest, 1),
                    max: value_of(&rest, "--max")?
                        .map(|d| d.parse().map_err(|e| format!("--max: {e}")))
                        .transpose()?,
                    case_sensitive: rest.iter().any(|arg| arg == "--case-sensitive"),
                },
                &["--max", "--case-sensitive"],
                2,
            ),
            "write" => (
                Command::Write {
                    path: require_positional(&rest, 0, "write needs a path")?,
                    from: value_of(&rest, "--from")?.map(PathBuf::from),
                    text: value_of(&rest, "--text")?,
                    stdin: rest.iter().any(|arg| arg == "--stdin"),
                    mkdir: rest.iter().any(|arg| arg == "--mkdir"),
                    expect: value_of(&rest, "--expect")?,
                },
                &["--from", "--text", "--stdin", "--mkdir", "--expect"],
                1,
            ),
            "diff" => (
                Command::Diff {
                    path: positional(&rest, 0),
                    staged: rest.iter().any(|arg| arg == "--staged"),
                    context: value_of(&rest, "--context")?
                        .map(|d| d.parse().map_err(|e| format!("--context: {e}")))
                        .transpose()?,
                },
                &["--staged", "--context"],
                1,
            ),
            "exec" => {
                let divider = rest
                    .iter()
                    .position(|arg| arg == "--")
                    .ok_or("exec needs `--` before the program to run")?;
                (
                    Command::Exec {
                        argv: rest[divider + 1..].to_vec(),
                        cwd: value_of(&rest[..divider], "--cwd")?,
                        timeout: value_of(&rest[..divider], "--timeout")?
                            .map(|seconds| {
                                seconds
                                    .parse::<u64>()
                                    .map(Duration::from_secs)
                                    .map_err(|e| format!("--timeout: {e}"))
                            })
                            .transpose()?,
                    },
                    &["--cwd", "--timeout"],
                    // The program and its arguments are the command's own, so there
                    // is no count to check past the divider.
                    usize::MAX,
                )
            }
            "install" => (
                Command::Install {
                    version: require_positional(&rest, 0, "install needs a version")?,
                    from: PathBuf::from(value_of(&rest, "--from")?.ok_or("install needs --from")?),
                },
                &["--from"],
                1,
            ),
            "forward" => (
                forward_command(&rest)?,
                &["--to", "--listen", "--connections", "--ttl"],
                0,
            ),
            other => {
                return Err(format!("unknown command {other:?}\n\n{USAGE}"));
            }
        };
        // Only the flags before the divider are ours on `exec`.
        let flags = match command {
            "exec" => {
                let divider = rest.iter().position(|arg| arg == "--").unwrap_or(0);
                rest[..divider].to_vec()
            }
            _ => rest.clone(),
        };
        // How this program should talk is not the command's business, so the two
        // output flags are accepted on either side of the command name — but only
        // before the divider on `exec`, past which arguments are the program's.
        for token in &flags {
            match token.as_str() {
                "--json" => args.json = true,
                "--quiet" => args.quiet = true,
                _ => {}
            }
        }
        check_command_flags(&flags, allowed, command)?;
        let given = positional_count(&flags);
        if given > positionals {
            return Err(format!(
                "{command} takes at most {positionals} argument(s), got {given}"
            ));
        }
        args.command = parsed;
        args.check_connection()?;
        Ok(args)
    }

    /// The connection arguments are refused here rather than at connect time, so a
    /// missing credential is reported before the command has even been chosen.
    fn check_connection(&self) -> Result<(), String> {
        let transport: Result<(), String> = match (&self.unix, &self.tcp) {
            (Some(_), Some(_)) => Err("choose --unix or --tcp, not both".into()),
            (None, None) => Err("no transport: pass --unix or --tcp".into()),
            _ => Ok(()),
        };
        transport?;
        match (&self.token, &self.token_file) {
            (Some(_), Some(_)) => Err("choose --token or --token-file, not both".into()),
            (None, None) => Err("no credential: pass --token or --token-file".into()),
            _ => Ok(()),
        }
    }

    /// Open the session: one transport, one credential, one handshake.
    async fn connect(&self) -> Result<AnySession, String> {
        let transport = self.transport()?;
        let (token, remaining) = self.credential()?;
        let (endpoint, config) = self.endpoint_and_config();
        let token = SessionToken::from_text(&token);
        // Dialed first, and only the dial is retried: presenting the credential
        // twice is not possible even by accident, because the first attempt spends
        // it whether or not a reply comes back.
        let stream = self.dial(&transport).await?;
        // The credential goes out of the file before it is sent, not after the
        // handshake succeeds. Sending is spending: a hello that goes unanswered
        // burned it just as surely as one that was answered, and a file that went
        // on offering it would leave every later run refusing its own credential.
        if let Some(path) = &self.token_file
            && let Err(error) = TokenFile::new(path).write(&remaining)
        {
            eprintln!(
                "chaos-remote: the credential was presented but could not be removed from \
                 {}: {error}",
                path.display()
            );
        }
        let session = match stream {
            Dial::Unix(stream) => AnySession::Unix(Box::new(
                RemoteWorkspace::connect(stream, &endpoint, &token, config)
                    .await
                    .map_err(describe)?,
            )),
            Dial::Tcp(stream) => AnySession::Tcp(Box::new(
                RemoteWorkspace::connect(stream, &endpoint, &token, config)
                    .await
                    .map_err(describe)?,
            )),
        };
        Ok(session)
    }

    /// Which server this invocation talks to, and with what agreement.
    ///
    /// Shared with `forward` because a forwarded connection is a second connection
    /// to the same server: the endpoint and the config have to match the session's
    /// or the grant would be presented under different terms than it was issued.
    fn endpoint_and_config(&self) -> (RemoteEndpoint, RemoteWorkspaceConfig) {
        let capabilities = self
            .capabilities
            .clone()
            .unwrap_or_else(|| RemoteCapability::all().to_vec());
        let endpoint = RemoteEndpoint {
            host: self.host.clone(),
            port: self.port,
            host_key: chaos_engine::remote::HostKeyPolicy::Strict,
            capabilities: capabilities.clone(),
        };
        (
            endpoint,
            RemoteWorkspaceConfig::new()
                .capabilities(capabilities)
                // The handshake waits for a reply like any other request, so the same
                // bound covers it. A peer that accepts a connection and then says
                // nothing is the case this is for, and it looks the same from here
                // whether the tunnel reached a silent host or a wrong port.
                .handshake_timeout(
                    self.reply_timeout
                        .unwrap_or(chaos_engine::remote::DEFAULT_HANDSHAKE_TIMEOUT),
                )
                .request_timeout(self.reply_timeout),
        )
    }

    /// The transport named on the command line, exactly one of them.
    fn transport(&self) -> Result<Transport, String> {
        match (&self.unix, &self.tcp) {
            (Some(path), None) => Ok(Transport::Unix(path.clone())),
            (None, Some(addr)) => Ok(Transport::Tcp(*addr)),
            _ => Err("no transport chosen".into()),
        }
    }

    /// Open the stream, waiting out a transport that is not up yet.
    ///
    /// Kept apart from the handshake on purpose: `--connect-wait` repeats this and
    /// only this, so waiting for a tunnel can never burn a one-time credential.
    async fn dial(&self, transport: &Transport) -> Result<Dial, String> {
        let retry = DialRetry::new(self.connect_wait);
        match transport {
            #[cfg(unix)]
            Transport::Unix(path) => RemoteWorkspace::dial_unix_waiting(path, retry)
                .await
                .map(Dial::Unix)
                .map_err(describe),
            #[cfg(not(unix))]
            Transport::Unix(path) => Err(format!(
                "a Unix socket is not available on this platform, but {} was asked for",
                path.display()
            )),
            Transport::Tcp(addr) => RemoteWorkspace::dial_tcp_waiting(*addr, retry)
                .await
                .map(Dial::Tcp)
                .map_err(describe),
        }
    }

    /// The credential to present, and the ones to leave in the file behind it.
    fn credential(&self) -> Result<(String, Vec<SessionToken>), String> {
        match (&self.token, &self.token_file) {
            (Some(_), Some(_)) => Err("choose --token or --token-file, not both".into()),
            (None, None) => Err("no credential: pass --token or --token-file".into()),
            (Some(token), None) => Ok((token.clone(), Vec::new())),
            (None, Some(path)) => {
                let tokens = TokenFile::new(path)
                    .read()
                    .map_err(|e| format!("read {}: {e}", path.display()))?;
                let (first, rest) = tokens
                    .split_first()
                    .ok_or(format!("{} holds no unused credentials", path.display()))?;
                Ok((first.as_str().to_string(), rest.to_vec()))
            }
        }
    }
}

enum Transport {
    #[allow(dead_code)]
    Unix(PathBuf),
    Tcp(std::net::SocketAddr),
}

/// A stream to the server that has not been handshaked over yet.
enum Dial {
    #[cfg(unix)]
    Unix(tokio::net::UnixStream),
    Tcp(tokio::net::TcpStream),
}

fn emit(value: &serde_json::Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    println!("{text}");
    Ok(())
}

/// `cat` output goes to stdout raw unless `--json`, because the point of `cat` is a
/// file that can be piped somewhere.
fn emit_bytes(args: &Args, data: &[u8], sha256: &str) -> Result<(), String> {
    if args.json {
        return emit(&serde_json::json!({
            "sha256": sha256,
            "bytes": data.len(),
            "text": String::from_utf8_lossy(data),
        }));
    }
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    stdout
        .write_all(data)
        .map_err(|e| format!("write stdout: {e}"))?;
    stdout.flush().map_err(|e| format!("flush stdout: {e}"))
}

/// Turn a transport failure into a line that says what to do next.
fn describe(error: RemoteError) -> String {
    match error {
        RemoteError::Unauthorized { reason } => {
            format!(
                "refused: {reason} (a credential is one-time; use the next line of --token-file)"
            )
        }
        other => other.to_string(),
    }
}

/// Port 0: ask the operating system for whatever is free.
const DEFAULT_FORWARD_LISTEN: &str = "0";

/// How many connections a `forward` asks for when it was not told. A page load can
/// reasonably produce a dozen; a port scan produces thousands, and the server's own
/// ceiling is what actually decides.
const DEFAULT_FORWARD_CONNECTIONS: usize = 16;

/// How long a `forward` asks its grant to last when it was not told. The server
/// clamps it, so this is only the longest the client will hope for.
const DEFAULT_FORWARD_TTL: Duration = Duration::from_secs(30 * 60);

/// Read the `forward` command.
///
/// All four of its flags describe the grant rather than the conversation, and the
/// part worth writing down is what each one defaults to: the target is the only
/// thing with no sensible default, and the local end defaults to a free loopback
/// port rather than a fixed one, because a fixed one is a collision waiting to
/// happen on a machine already running something.
fn forward_command(rest: &[String]) -> Result<Command, String> {
    let target = parse_forward_target(
        &value_of(rest, "--to")?
            .ok_or("forward needs --to HOST:PORT, the service as the *server* reaches it")?,
    )?;
    let listen = parse_listen_endpoint(
        &value_of(rest, "--listen")?.unwrap_or_else(|| DEFAULT_FORWARD_LISTEN.to_string()),
    )?;
    let connections = match value_of(rest, "--connections")? {
        Some(text) => {
            let uses: usize = text.parse().map_err(|e| format!("--connections: {e}"))?;
            if uses == 0 {
                return Err("--connections 0 would forward nothing".into());
            }
            uses
        }
        None => DEFAULT_FORWARD_CONNECTIONS,
    };
    let ttl = match value_of(rest, "--ttl")? {
        Some(text) => {
            let secs: u64 = text.parse().map_err(|e| format!("--ttl: {e}"))?;
            if secs == 0 {
                return Err(
                    "--ttl 0 would let the grant expire before anything could use it".into(),
                );
            }
            Duration::from_secs(secs)
        }
        None => DEFAULT_FORWARD_TTL,
    };
    Ok(Command::Forward {
        target,
        listen,
        connections,
        ttl,
    })
}

/// Dial the server again for one forwarded connection.
///
/// A forward is not a second command on the session's own connection: each
/// connection that arrives on the local port gets its own, handshaked with the
/// forward ticket rather than the session token. The session that asked for the
/// grant stays open meanwhile, because closing it is what revokes them.
#[cfg(unix)]
async fn redial_unix(path: PathBuf) -> Result<tokio::net::UnixStream, RemoteError> {
    tokio::net::UnixStream::connect(&path)
        .await
        .map_err(|e| RemoteError::Io {
            context: format!("dial {} again for a forwarded connection", path.display()),
            reason: e.to_string(),
        })
}

/// The TCP form of [`redial_unix`].
async fn redial_tcp(addr: std::net::SocketAddr) -> Result<tokio::net::TcpStream, RemoteError> {
    tokio::net::TcpStream::connect(addr)
        .await
        .map_err(|e| RemoteError::Io {
            context: format!("dial {addr} again for a forwarded connection"),
            reason: e.to_string(),
        })
}

fn take(argv: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    *index += 1;
    argv.get(*index)
        .cloned()
        .ok_or_else(|| format!("{flag} needs a value"))
}

/// The nth non-flag argument. `--max 5` must not be mistaken for the second path.
fn positional(argv: &[String], nth: usize) -> Option<String> {
    let mut seen = 0;
    let mut index = 0;
    while index < argv.len() {
        let arg = &argv[index];
        if arg.starts_with("--") {
            // A flag with a value consumes it; a bare switch consumes nothing.
            let takes_value = matches!(
                arg.as_str(),
                "--depth"
                    | "--max"
                    | "--offset"
                    | "--length"
                    | "--out"
                    | "--from"
                    | "--text"
                    | "--expect"
                    | "--context"
                    | "--cwd"
                    | "--timeout"
                    | "--to"
                    | "--listen"
                    | "--connections"
                    | "--ttl"
            );
            index += usize::from(takes_value) + 1;
            continue;
        }
        if seen == nth {
            return Some(arg.clone());
        }
        seen += 1;
        index += 1;
    }
    None
}

fn require_positional(argv: &[String], nth: usize, message: &str) -> Result<String, String> {
    positional(argv, nth).ok_or_else(|| message.to_string())
}

/// A flag the command does not take is refused rather than ignored. Silently
/// dropping a misspelled `--json` would hand a script text where it expected JSON,
/// which is worse than an error at the point of the mistake.
fn check_command_flags(rest: &[String], allowed: &[&str], command: &str) -> Result<(), String> {
    for token in rest {
        if !token.starts_with("--") {
            continue;
        }
        // `--flag=value` is the same flag with the value attached.
        let name = token
            .split_once('=')
            .map_or(token.as_str(), |(head, _)| head);
        if allowed.contains(&name) || matches!(name, "--json" | "--quiet") {
            continue;
        }
        return Err(format!("unknown flag {name} for {command}"));
    }
    Ok(())
}

/// How many arguments of this command are not flags or a flag's value.
fn positional_count(argv: &[String]) -> usize {
    let mut count = 0;
    let mut index = 0;
    while index < argv.len() {
        let arg = &argv[index];
        if arg.starts_with("--") {
            let takes_value = matches!(
                arg.split_once('=').map_or(arg.as_str(), |(head, _)| head),
                "--depth"
                    | "--max"
                    | "--offset"
                    | "--length"
                    | "--out"
                    | "--from"
                    | "--text"
                    | "--expect"
                    | "--context"
                    | "--cwd"
                    | "--timeout"
                    | "--to"
                    | "--listen"
                    | "--connections"
                    | "--ttl"
            );
            index += usize::from(takes_value) + 1;
            continue;
        }
        count += 1;
        index += 1;
    }
    count
}

fn value_of(argv: &[String], flag: &str) -> Result<Option<String>, String> {
    let mut index = 0;
    while index < argv.len() {
        if argv[index] == flag {
            let value = argv
                .get(index + 1)
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))?;
            return Ok(Some(value));
        }
        index += 1;
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(argv: &[&str]) -> Result<Args, String> {
        Args::parse(argv.iter().map(|arg| (*arg).to_string()).collect())
    }

    /// A forward needs exactly one thing from the person typing it: the service.
    /// Everything else has a default that cannot collide with anything, and the
    /// server's answer — not these numbers — is what the tunnel is finally bound by.
    #[test]
    fn a_forward_needs_only_the_service_it_points_at() {
        let args = parse(&[
            "--tcp",
            "127.0.0.1:4100",
            "--token",
            "t",
            "forward",
            "--to",
            "10.0.0.7:6379",
        ])
        .expect("parsed");
        match args.command {
            Command::Forward {
                target,
                listen,
                connections,
                ttl,
            } => {
                assert_eq!(target.to_text(), "10.0.0.7:6379");
                assert_eq!(listen.port(), 0, "the default local end is a free port");
                assert!(listen.ip().is_loopback(), "{listen} is not local");
                assert_eq!(connections, DEFAULT_FORWARD_CONNECTIONS);
                assert_eq!(ttl, DEFAULT_FORWARD_TTL);
            }
            other => panic!("wrong command: {other:?}"),
        }
    }

    /// The two ends are different questions and are spelled separately: where to
    /// listen here, what to reach there.
    #[test]
    fn each_end_of_a_forward_is_named_separately_and_a_positional_is_refused() {
        let args = parse(&[
            "--tcp",
            "127.0.0.1:4100",
            "--token",
            "t",
            "forward",
            "--to",
            "[::1]:8080",
            "--listen",
            "8081",
            "--connections",
            "3",
            "--ttl",
            "60",
        ])
        .expect("parsed");
        match args.command {
            Command::Forward {
                target,
                listen,
                connections,
                ttl,
            } => {
                assert_eq!(target.to_text(), "[::1]:8080");
                assert_eq!(listen.to_string(), "127.0.0.1:8081", "a bare port is local");
                assert_eq!(connections, 3);
                assert_eq!(ttl, Duration::from_secs(60));
            }
            other => panic!("wrong command: {other:?}"),
        }

        // A value taken from a flag must not be counted as an argument of the
        // command, which is the difference between this and a refusal.
        assert!(
            parse(&[
                "--tcp",
                "127.0.0.1:4100",
                "--token",
                "t",
                "forward",
                "--to",
                "h:1",
                "extra"
            ])
            .expect_err("forward takes no positional")
            .contains("at most 0"),
        );
    }

    /// A forward that would forward nothing is a typo, and the listener is refused
    /// before it exists rather than after.
    #[test]
    fn a_forward_that_could_forward_nothing_is_refused_before_it_starts() {
        let refused = |extra: &[&str]| {
            let mut argv = vec!["--tcp", "127.0.0.1:4100", "--token", "t", "forward"];
            argv.extend_from_slice(extra);
            parse(&argv).expect_err("nothing here should start a forward")
        };
        assert!(refused(&[]).contains("--to"), "no target at all");
        assert!(refused(&["--to", ""]).contains("host:port"));
        assert!(refused(&["--to", "redis-only"]).contains("host:port"));
        assert!(refused(&["--to", "h:0"]).contains("cannot be 0"));
        assert!(refused(&["--to", "h:1", "--connections", "0"]).contains("--connections"));
        assert!(refused(&["--to", "h:1", "--ttl", "0"]).contains("--ttl"));
        assert!(refused(&["--to", "h:1", "--listen", "0.0.0.0:8080"]).contains("loopback"));
    }

    /// `exec` is the one command whose arguments are not ours, and the only way to
    /// keep them separate is a divider that everything after belongs to the program.
    #[test]
    fn everything_after_the_divider_belongs_to_the_remote_program() {
        let args = parse(&[
            "--unix", "/s", "--token", "t", "exec", "--cwd", "src", "--", "git", "commit", "-m",
            "-amend",
        ])
        .expect("parsed");
        match args.command {
            Command::Exec { argv, cwd, timeout } => {
                assert_eq!(
                    argv,
                    ["git", "commit", "-m", "-amend"],
                    "flags after `--` are the program's"
                );
                assert_eq!(cwd.as_deref(), Some("src"));
                assert_eq!(timeout, None);
            }
            other => panic!("wrong command: {other:?}"),
        }
    }

    #[test]
    fn a_positional_after_a_flagged_value_is_still_the_positional() {
        let args = parse(&[
            "--unix", "/s", "--token", "t", "search", "beans", "src", "--max", "5",
        ])
        .expect("parsed");
        match args.command {
            Command::Search {
                query,
                path,
                max,
                case_sensitive,
            } => {
                assert_eq!(query, "beans");
                assert_eq!(path.as_deref(), Some("src"), "--max 5 is not a path");
                assert_eq!(max, Some(5));
                assert!(!case_sensitive);
            }
            other => panic!("wrong command: {other:?}"),
        }
    }

    #[test]
    fn one_transport_and_one_credential_are_required() {
        assert!(parse(&["ping"]).unwrap_err().contains("no transport"));
        assert!(
            parse(&["--unix", "/s", "ping"])
                .unwrap_err()
                .contains("no credential")
        );
        assert!(
            parse(&[
                "--unix",
                "/s",
                "--tcp",
                "127.0.0.1:1",
                "--token",
                "t",
                "ping"
            ])
            .unwrap_err()
            .contains("not both")
        );
        assert!(
            parse(&["--token-file", "/t", "--token", "t", "--unix", "/s", "ping"])
                .unwrap_err()
                .contains("not both")
        );
    }

    /// The two waits are different things and are named differently: one is for a
    /// transport that is not there yet and may be retried, the other is a bound on
    /// a request that will never be retried.
    #[test]
    fn the_two_waits_are_separate_and_both_are_bounded() {
        let args = parse(&[
            "--connect-wait",
            "20",
            "--reply-timeout",
            "3",
            "--unix",
            "/s",
            "--token",
            "t",
            "ping",
        ])
        .expect("parsed");
        assert_eq!(args.connect_wait, Duration::from_secs(20));
        assert_eq!(args.reply_timeout, Some(Duration::from_secs(3)));

        let default = parse(&["--unix", "/s", "--token", "t", "ping"]).expect("parsed");
        assert_eq!(
            default.connect_wait,
            Duration::ZERO,
            "nobody waits for a transport they did not ask to wait for"
        );
        assert_eq!(
            default.reply_timeout, None,
            "`exec` runs a command of any length, so no reply deadline by default"
        );

        assert!(
            parse(&[
                "--reply-timeout",
                "0",
                "--unix",
                "/s",
                "--token",
                "t",
                "ping"
            ])
            .unwrap_err()
            .contains("--reply-timeout"),
            "a zero deadline would abandon every request before its reply"
        );
        assert!(
            parse(&[
                "--connect-wait",
                "soon",
                "--unix",
                "/s",
                "--token",
                "t",
                "ping"
            ])
            .unwrap_err()
            .contains("--connect-wait")
        );
    }

    #[test]
    fn an_unknown_command_or_flag_is_reported_with_the_usage() {
        assert!(
            parse(&["--unix", "/s", "--token", "t", "teleport"])
                .unwrap_err()
                .contains("unknown command")
        );
        assert!(
            parse(&["--teleport", "x", "--unix", "/s", "--token", "t", "ping"])
                .unwrap_err()
                .contains("unknown flag")
        );
        assert!(
            parse(&["--unix", "/s", "--token", "t"])
                .unwrap_err()
                .contains("no command")
        );
    }

    /// Asking for less is allowed; it is how a script proves a server really gated a
    /// capability rather than merely not being asked.
    #[test]
    fn a_capability_narrows_what_is_asked_for() {
        let args = parse(&[
            "--unix",
            "/s",
            "--token",
            "t",
            "--capability",
            "read",
            "--capability",
            "git",
            "ping",
        ])
        .expect("parsed");
        assert_eq!(
            args.capabilities.as_deref(),
            Some(&[RemoteCapability::WorkspaceRead, RemoteCapability::Git][..])
        );
        assert!(
            parse(&[
                "--unix",
                "/s",
                "--token",
                "t",
                "--capability",
                "detached-agent",
                "ping"
            ])
            .is_err()
        );
    }

    #[test]
    fn a_write_names_its_source_and_its_precondition() {
        let args = parse(&[
            "--unix",
            "/s",
            "--token",
            "t",
            "write",
            "README.md",
            "--from",
            "/tmp/x",
            "--mkdir",
            "--expect",
            "abc",
        ])
        .expect("parsed");
        match args.command {
            Command::Write {
                path,
                from,
                mkdir,
                expect,
                ..
            } => {
                assert_eq!(path, "README.md");
                assert_eq!(from.as_deref(), Some(std::path::Path::new("/tmp/x")));
                assert!(mkdir);
                assert_eq!(expect.as_deref(), Some("abc"));
            }
            other => panic!("wrong command: {other:?}"),
        }
    }
}
