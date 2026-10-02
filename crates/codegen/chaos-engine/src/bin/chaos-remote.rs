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
    InstallOutcome, RemoteCapability, RemoteEndpoint, RemoteError, RemoteWorkspace,
    RemoteWorkspaceConfig, SessionToken, TokenFile, WriteFile, parse_capability,
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
        let transport = match (&self.unix, &self.tcp) {
            (Some(path), None) => Transport::Unix(path.clone()),
            (None, Some(addr)) => Transport::Tcp(*addr),
            _ => return Err("no transport chosen".into()),
        };
        let (token, remaining) = self.credential()?;
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
        let token = SessionToken::from_text(&token);
        let config = RemoteWorkspaceConfig::new().capabilities(capabilities);
        let session = match transport {
            #[cfg(unix)]
            Transport::Unix(path) => AnySession::Unix(Box::new(
                RemoteWorkspace::connect_unix(&path, &endpoint, &token, config)
                    .await
                    .map_err(describe)?,
            )),
            #[cfg(not(unix))]
            Transport::Unix(path) => {
                return Err(format!(
                    "a Unix socket is not available on this platform, but {} was asked for",
                    path.display()
                ));
            }
            Transport::Tcp(addr) => AnySession::Tcp(Box::new(
                RemoteWorkspace::connect_tcp(addr, &endpoint, &token, config)
                    .await
                    .map_err(describe)?,
            )),
        };
        // Spent: the file must not offer it to the next run.
        if let Some(path) = &self.token_file
            && let Err(error) = TokenFile::new(path).write(&remaining)
        {
            eprintln!(
                "chaos-remote: the credential was used but could not be removed from \
                 {}: {error}",
                path.display()
            );
        }
        Ok(session)
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
