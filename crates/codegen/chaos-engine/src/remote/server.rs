//! The workspace server: one session per connection, one request at a time.
//!
//! What this is *not* matters as much as what it is. It is not a shell — nothing
//! here hands a string to a program for interpretation; `exec` takes an argv whose
//! first element has to be a bare name on a list fixed before the server started.
//! It is not a general file server either: every path is resolved inside one
//! workspace root, and a path that lands outside it is refused before anything is
//! opened.
//!
//! A session is sequential. A request is answered in full before the next is read,
//! so a reply's id always matches its request without a multiplexer, and one slow
//! tool call cannot stall another session because each connection is its own task.

use std::fs::DirEntry;
use std::io::SeekFrom;
use std::net::{IpAddr, SocketAddr};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, join};
use tokio::sync::Mutex;
use xai_tty_utils::ProcessScope;

use super::credentials::{
    ForwardLimits, ForwardTarget, ForwardTicketError, ForwardVault, SessionToken, TokenVault,
};
use super::endpoint::RemoteCapability;
use super::install::{InstallLayout, sha256_hex};
use super::path::RemotePath;
use super::protocol::*;

/// Everything the server will do, decided before it accepts a connection.
///
/// There is no way to widen any of this from a session: a fully authorised client
/// can use what the server was configured to offer and nothing beyond it.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// The one directory this server can see.
    pub workspace_root: PathBuf,
    pub implementation: Implementation,
    pub protocol: VersionRange,
    /// What this server is able to do. A session gets the intersection of this and
    /// what it asked for.
    pub capabilities: Vec<RemoteCapability>,
    pub token_ttl: Duration,
    /// Tokens minted for one session each; how many to hand out at startup.
    pub initial_tokens: usize,
    pub max_frame_bytes: usize,
    /// Largest window a single read, or a single write, may carry.
    pub max_transfer_bytes: u64,
    pub max_list_entries: usize,
    pub max_list_depth: usize,
    pub max_search_results: usize,
    /// Files bigger than this are skipped by the search rather than read.
    pub max_search_file_bytes: u64,
    /// How many files one search may look at before giving up.
    pub max_search_files: usize,
    pub max_exec_output_bytes: usize,
    pub exec_default_timeout: Duration,
    pub exec_max_timeout: Duration,
    /// The only programs `exec` may run.
    pub allowed_executables: Vec<String>,
    /// The only places a port forward may be pointed.
    ///
    /// Empty means forwards cannot be granted at all, which is the default: the
    /// server's operator decides where connections may go, because a server that
    /// connects wherever a client asks is a proxy, and a proxy that anyone can
    /// authorise is an SSRF with good manners.
    pub forward_targets: Vec<ForwardTarget>,
    pub forward_limits: ForwardLimits,
    /// How long a forward may wait for its target to answer.
    pub forward_connect_timeout: Duration,
    /// Directories the listing and the search do not descend into.
    pub skip_dirs: Vec<String>,
    pub install: InstallLayout,
}

impl ServerConfig {
    /// A configuration for `workspace_root` with production defaults.
    ///
    /// `allowed_executables` starts empty. A server that runs nothing is a
    /// perfectly good server; one that runs whatever a client asks for is not a
    /// server, it is a remote shell.
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            implementation: Implementation::local(),
            protocol: VersionRange::current(),
            capabilities: vec![
                RemoteCapability::WorkspaceList,
                RemoteCapability::WorkspaceRead,
                RemoteCapability::WorkspaceSearch,
                RemoteCapability::WorkspaceWrite,
                RemoteCapability::Git,
            ],
            token_ttl: Duration::from_secs(600),
            initial_tokens: 8,
            max_frame_bytes: MAX_FRAME_BYTES,
            max_transfer_bytes: 4 * 1024 * 1024,
            max_list_entries: 5_000,
            max_list_depth: 12,
            max_search_results: 200,
            max_search_file_bytes: 2 * 1024 * 1024,
            max_search_files: 20_000,
            max_exec_output_bytes: 256 * 1024,
            exec_default_timeout: Duration::from_secs(30),
            exec_max_timeout: Duration::from_secs(600),
            allowed_executables: Vec::new(),
            forward_targets: Vec::new(),
            forward_limits: ForwardLimits::default(),
            forward_connect_timeout: Duration::from_secs(10),
            skip_dirs: [".git", "target", "node_modules", ".venv", "__pycache__"]
                .iter()
                .map(|name| name.to_string())
                .collect(),
            install: InstallLayout::new(Path::new(".")),
        }
    }

    pub fn capabilities(mut self, capabilities: Vec<RemoteCapability>) -> Self {
        self.capabilities = capabilities;
        self
    }

    pub fn allowed_executables(
        mut self,
        executables: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.allowed_executables = executables.into_iter().map(Into::into).collect();
        self
    }

    pub fn token_ttl(mut self, ttl: Duration) -> Self {
        self.token_ttl = ttl;
        self
    }

    /// The only targets a port forward may be pointed at.
    ///
    /// Names are matched as written, after trimming and lower-casing, with the
    /// brackets around an IPv6 literal dropped. Because a name can be made to
    /// resolve anywhere, an operator who means to pin a target should put the
    /// address in the allowlist rather than a hostname.
    pub fn forward_targets(mut self, targets: Vec<ForwardTarget>) -> Self {
        self.forward_targets = targets;
        self
    }

    pub fn forward_limits(mut self, limits: ForwardLimits) -> Self {
        self.forward_limits = limits;
        self
    }

    pub fn forward_connect_timeout(mut self, timeout: Duration) -> Self {
        self.forward_connect_timeout = timeout;
        self
    }

    pub fn initial_tokens(mut self, count: usize) -> Self {
        self.initial_tokens = count;
        self
    }

    pub fn max_frame_bytes(mut self, bytes: usize) -> Self {
        self.max_frame_bytes = bytes;
        self
    }

    pub fn max_exec_output_bytes(mut self, bytes: usize) -> Self {
        self.max_exec_output_bytes = bytes;
        self
    }

    /// Largest window a single read or write may carry.
    ///
    /// The client learns this number at handshake and pages around it, so setting
    /// it small is a way to exercise paging rather than a way to break a client.
    pub fn max_transfer_bytes(mut self, bytes: u64) -> Self {
        self.max_transfer_bytes = bytes;
        self
    }

    /// Where server artifacts get installed. Anchored at the workspace root so a
    /// deployment does not have to configure a second path.
    pub fn install_layout(mut self, layout: InstallLayout) -> Self {
        self.install = layout;
        self
    }

    /// Refuse to start rather than discover this mid-session.
    pub fn validate(&self) -> Result<(), String> {
        if !self.workspace_root.is_dir() {
            return Err(format!(
                "workspace root {} is not a directory",
                self.workspace_root.display()
            ));
        }
        if self.protocol.min > self.protocol.max {
            return Err(format!(
                "protocol range is inverted: {} > {}",
                self.protocol.min, self.protocol.max
            ));
        }
        if self.max_frame_bytes == 0 {
            return Err("the frame limit cannot be zero; the server would answer nothing".into());
        }
        for name in &self.allowed_executables {
            if name.is_empty() || name.contains('/') || name.contains('\\') {
                return Err(format!(
                    "allowed executable {name:?} must be a bare program name: an \
                     allowlist of paths lets the allowlist decide where programs live, \
                     and that is the caller's decision to make"
                ));
            }
        }
        for target in &self.forward_targets {
            if target.host.trim().is_empty() {
                return Err("a forward target has no host to connect to".into());
            }
            if target.port == 0 {
                return Err(format!(
                    "forward target {target} has a port of 0, which is not something \
                     to connect to"
                ));
            }
        }
        Ok(())
    }

    /// Whether this server will connect to `host:port` on someone's behalf.
    ///
    /// Matching is on the literal, normalised the same way on both sides, so
    /// `localhost` and `127.0.0.1` are different answers — deliberately, because
    /// deciding they are the same host is a decision about the network, not about
    /// strings, and the allowlist is the operator's.
    fn allows_forward(&self, host: &str, port: u16) -> bool {
        let wanted_host = normalize_forward_host(host);
        self.forward_targets.iter().any(|target| {
            target.port == port && normalize_forward_host(&target.host) == wanted_host
        })
    }

    /// The capability a request needs, if it needs one.
    fn required_capability(request: &Request) -> Option<RemoteCapability> {
        match request {
            Request::List { .. } => Some(RemoteCapability::WorkspaceList),
            Request::Read { .. } => Some(RemoteCapability::WorkspaceRead),
            Request::Search { .. } => Some(RemoteCapability::WorkspaceSearch),
            Request::Write { .. } => Some(RemoteCapability::WorkspaceWrite),
            Request::Diff { .. } => Some(RemoteCapability::Git),
            Request::Exec { .. } => Some(RemoteCapability::ToolExecution),
            // Both halves of forwarding are the same capability: one asks for the
            // permission, the other spends it. Which target is allowed is a
            // separate question the allowlist answers.
            Request::ForwardGrant { .. } | Request::ForwardOpen => {
                Some(RemoteCapability::PortForward)
            }
            // Deploying a new server build is a write to the host, and nothing
            // less than that: a session that may edit files may stage a file.
            Request::InstallBegin { .. }
            | Request::InstallChunk { .. }
            | Request::InstallFinish { .. } => Some(RemoteCapability::WorkspaceWrite),
            Request::Ping | Request::Hello { .. } => None,
        }
    }
}

/// How this connection was authorised, which decides what it may then ask for.
enum Auth {
    /// A workspace session, opened with a one-time credential.
    Session,
    /// A connection opened with a forward ticket. The only thing it may do is
    /// reach the one target that ticket was minted for.
    Forward { target: ForwardTarget },
}

/// A running server: its configuration, its credentials, and the process scope
/// its tool children belong to.
pub struct Server {
    config: ServerConfig,
    vault: Arc<Mutex<TokenVault>>,
    /// The port forwards that have been authorised and not yet spent, closed or
    /// expired. Separate from the session vault on purpose: a forward ticket must
    /// not open a workspace session, and a session credential must not open a
    /// forward, so neither store is consulted as a fallback for the other's
    /// *success*.
    forwards: Arc<Mutex<ForwardVault>>,
    scope: ProcessScope,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The credential vault is deliberately not printed: it is a list of things
        // that open sessions.
        f.debug_struct("Server")
            .field("config", &self.config)
            .finish()
    }
}

impl Server {
    pub fn new(config: ServerConfig) -> Result<Arc<Self>, String> {
        config.validate()?;
        let ttl = config.token_ttl;
        let forward_limits = config.forward_limits;
        Ok(Arc::new(Self {
            config,
            vault: Arc::new(Mutex::new(TokenVault::new(ttl))),
            forwards: Arc::new(Mutex::new(ForwardVault::new(forward_limits))),
            scope: ProcessScope::new(),
        }))
    }

    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    /// Mint `count` credentials for a client to pick up. Expired ones leave first,
    /// so a long-lived server's hand-off does not accumulate dead credentials.
    pub async fn issue_tokens(&self, count: usize) -> Vec<SessionToken> {
        let mut vault = self.vault.lock().await;
        vault.sweep_expired();
        (0..count).map(|_| vault.issue()).collect()
    }

    /// Accept connections on a Unix socket until this future is dropped.
    ///
    /// The socket is created with owner-only permission: reaching it is the first
    /// half of authorisation, and the mode is what makes "reached it" mean
    /// "already this user".
    #[cfg(unix)]
    pub async fn serve_unix(
        self: Arc<Self>,
        path: &Path,
    ) -> Result<tokio::net::UnixListener, String> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        // A socket file left by a dead server would make bind() fail; the peer it
        // names cannot answer, so removing it is the only useful move.
        if tokio::fs::symlink_metadata(path).await.is_ok() {
            tokio::fs::remove_file(path)
                .await
                .map_err(|e| format!("remove stale {}: {e}", path.display()))?;
        }
        let listener = tokio::net::UnixListener::bind(path)
            .map_err(|e| format!("bind {}: {e}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| format!("chmod {}: {e}", path.display()))?;
        }
        tracing::info!(socket = %path.display(), "remote workspace server listening");
        Ok(listener)
    }

    /// Accept connections on a loopback TCP port.
    pub async fn serve_loopback_tcp(
        self: Arc<Self>,
        addr: SocketAddr,
    ) -> Result<tokio::net::TcpListener, String> {
        require_loopback(addr)?;
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| format!("bind {addr}: {e}"))?;
        tracing::info!(%addr, "remote workspace server listening");
        Ok(listener)
    }

    /// Serve every connection this listener accepts, each in its own task.
    #[cfg(unix)]
    pub async fn accept_unix(self: Arc<Self>, listener: tokio::net::UnixListener) {
        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let server = Arc::clone(&self);
                    tokio::spawn(async move {
                        if let Err(e) = server.serve(stream).await {
                            tracing::debug!(error = %e, "remote session closed");
                        }
                    });
                }
                Err(e) => tracing::warn!(error = %e, "accept failed"),
            }
        }
    }

    pub async fn accept_tcp(self: Arc<Self>, listener: tokio::net::TcpListener) {
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    let server = Arc::clone(&self);
                    tokio::spawn(async move {
                        if let Err(e) = server.serve(stream).await {
                            tracing::debug!(%addr, error = %e, "remote session closed");
                        }
                    });
                }
                Err(e) => tracing::warn!(error = %e, "accept failed"),
            }
        }
    }

    /// Run one session over an already-connected stream.
    ///
    /// Returning `Err` is not an alarm: a refused handshake comes back as an error
    /// here *and* as a typed reply to the peer, which is the part anyone acts on.
    pub async fn serve<S>(self: &Arc<Self>, stream: S) -> Result<(), RemoteError>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let (mut reader, mut writer) = tokio::io::split(stream);
        let max_frame = self.config.max_frame_bytes;

        let hello: Envelope<Request> = match recv(&mut reader, max_frame).await {
            Ok(Some(envelope)) => envelope,
            Ok(None) => return Ok(()),
            Err(e) => {
                let _ = send(&mut writer, &error_envelope(0, &e)).await;
                return Err(e);
            }
        };
        let Request::Hello {
            protocol,
            client,
            token,
            capabilities,
        } = &hello.body
        else {
            let error = RemoteError::Unauthorized {
                reason: "the first frame must be a hello".into(),
            };
            let _ = send(&mut writer, &error_envelope(hello.id, &error)).await;
            return Err(error);
        };
        let client = client.clone();
        let token = token.clone();
        let requested = capabilities.clone();

        let negotiated = match negotiate(*protocol, self.config.protocol) {
            Ok(version) => version,
            Err(mismatch) => {
                let reason = mismatch.to_string();
                tracing::info!(client = %client.name, client_version = %client.version, %reason);
                let error = RemoteError::VersionMismatch {
                    server_version: self.config.protocol.max,
                    supported_min: self.config.protocol.min,
                    supported_max: self.config.protocol.max,
                    hint: mismatch.hint,
                };
                let _ = send(&mut writer, &error_envelope(hello.id, &error)).await;
                return Err(error);
            }
        };

        let auth = match self.authenticate(&token).await {
            Ok(auth) => auth,
            Err(e) => {
                // Which credential was refused is not worth a log line; that one
                // address has been refused twice is a different matter, and that
                // belongs to the accept loop, which can see addresses.
                tracing::debug!(client = %client.name, reason = %e, "session refused");
                let _ = send(&mut writer, &error_envelope(hello.id, &e)).await;
                return Err(e);
            }
        };
        let granted = match &auth {
            Auth::Session => self.grant(&requested),
            // A forwarded connection gets the one capability that describes it,
            // and only if it asked for it. It is not a workspace session that
            // happens to carry bytes.
            Auth::Forward { .. } => {
                if requested.contains(&RemoteCapability::PortForward) {
                    vec![RemoteCapability::PortForward]
                } else {
                    Vec::new()
                }
            }
        };

        let session_id = uuid::Uuid::new_v4().simple().to_string();
        tracing::debug!(
            client = %client.name,
            client_version = %client.version,
            protocol_version = negotiated,
            session = %session_id,
            granted = ?granted,
            "remote session open"
        );
        send(
            &mut writer,
            &Envelope {
                id: hello.id,
                body: Ok::<Payload, RemoteError>(Payload::Hello {
                    protocol_version: negotiated,
                    server: self.config.implementation.clone(),
                    capabilities: granted.clone(),
                    session_id: session_id.clone(),
                    max_transfer_bytes: self.config.max_transfer_bytes,
                }),
            },
        )
        .await?;

        // A chunked artifact transfer spans requests, so its state cannot live
        // inside the dispatch call that starts it.
        let mut upload: Option<Upload> = None;

        let served = async {
            loop {
                let Some(envelope) = recv::<Envelope<Request>, _>(&mut reader, max_frame).await?
                else {
                    return Ok(());
                };
                let id = envelope.id;
                // Opening a forward is the one request after which the stream stops
                // carrying frames, so it cannot take the answer-and-continue path
                // below: its reply has to be the last thing either side frames.
                if matches!(envelope.body, Request::ForwardOpen) {
                    return self.open_forward(&mut reader, &mut writer, id, &auth).await;
                }
                let reply = if matches!(envelope.body, Request::Hello { .. }) {
                    Err(RemoteError::Unauthorized {
                        reason: "this session is already authorised; a second hello is not \
                                 how re-authentication would work"
                            .into(),
                    })
                } else {
                    self.dispatch(id, envelope.body, &granted, &mut upload, &auth, &session_id)
                        .await
                };
                let reply = match reply {
                    Ok(payload) => Envelope {
                        id,
                        body: Ok(payload),
                    },
                    Err(error) => {
                        if !matches!(error, RemoteError::CapabilityNotGranted { .. }) {
                            tracing::debug!(session = %session_id, error = %error, "request refused");
                        }
                        error_envelope(id, &error)
                    }
                };
                send(&mut writer, &reply).await?;
            }
        }
        .await;

        // Closing the session takes the forwards it authorised with it. Without
        // this, a tunnel would outlive the session whose grant was the only thing
        // that justified it, and "close the session, the port is released" would be
        // true of only one of the two ends.
        if matches!(auth, Auth::Session) {
            self.revoke_forwards(&session_id).await;
        }
        served
    }

    /// Decide which of the two credential kinds this connection presented.
    ///
    /// A session credential is tried first: it is the common case, and the order
    /// is also what keeps a forward ticket from ever being read as permission to
    /// touch a workspace. A token that the forward store knows about but will not
    /// honour gets the forward's own refusal, because "not issued" would send
    /// someone looking for a typo in a ticket that simply ran out.
    async fn authenticate(&self, token: &str) -> Result<Auth, RemoteError> {
        match self.redeem(token).await {
            Ok(()) => Ok(Auth::Session),
            Err(session_refusal) => match self.forwards.lock().await.spend(token) {
                Ok((target, _)) => Ok(Auth::Forward { target }),
                Err(ForwardTicketError::Unknown) => Err(session_refusal),
                Err(e) => Err(RemoteError::ForwardDenied {
                    reason: e.to_string(),
                }),
            },
        }
    }

    /// Spend one credential on this session. Each credential opens exactly one.
    async fn redeem(&self, token: &str) -> Result<(), RemoteError> {
        let mut vault = self.vault.lock().await;
        vault.sweep_expired();
        vault.redeem(token).map_err(|e| RemoteError::Unauthorized {
            reason: e.to_string(),
        })
    }

    /// What the session may use: offered here, asked for there.
    fn grant(&self, requested: &[RemoteCapability]) -> Vec<RemoteCapability> {
        self.config
            .capabilities
            .iter()
            .filter(|capability| requested.contains(capability))
            .cloned()
            .collect()
    }

    /// Authorise one local port forward, if this server connects there.
    ///
    /// The allowlist is consulted here rather than at connect time so that the
    /// refusal arrives as a typed answer to the request that asked — a client that
    /// is told "granted" and then fails on the first connection has been told a
    /// lie about its own listener.
    async fn forward_grant(
        &self,
        host: &str,
        port: u16,
        ttl_secs: u64,
        max_uses: usize,
        session_id: &str,
    ) -> Result<Payload, RemoteError> {
        let target = ForwardTarget::new(normalize_forward_host(host), port);
        if !self.config.allows_forward(&target.host, target.port) {
            return Err(RemoteError::ForwardDenied {
                reason: format!(
                    "{target} is not somewhere this server will connect to; the operator \
                     lists {} forward target(s){listed}",
                    self.config.forward_targets.len(),
                    listed = if self.config.forward_targets.is_empty() {
                        ", and there are none".to_string()
                    } else {
                        format!(
                            ": {}",
                            self.config
                                .forward_targets
                                .iter()
                                .map(ForwardTarget::to_text)
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    }
                ),
            });
        }
        // Clamped before it reaches the vault, and the vault clamps again: the
        // number a client sends is a request, not an agreement.
        let ttl = Duration::from_secs(ttl_secs.min(60 * 60 * 24));
        let (ticket, granted_ttl, granted_uses) =
            self.forwards
                .lock()
                .await
                .issue(target.clone(), session_id, ttl, max_uses);
        tracing::debug!(session = %session_id, target = %target, uses = granted_uses, "port forward authorised");
        Ok(Payload::ForwardGrant {
            ticket: ticket.as_str().to_string(),
            host: target.host.clone(),
            port: target.port,
            expires_in_secs: granted_ttl.as_secs().max(1),
            max_uses: granted_uses,
        })
    }

    /// Answer a `forward_open` and then stop speaking the protocol.
    ///
    /// The reply is sent before a byte of the target is moved, because the client
    /// cannot tell "the tunnel is up" from "the target was slow" any other way.
    /// After it, the two streams are simply joined: nothing inspects the bytes, so
    /// a WebSocket upgrade, a database handshake and an HTTP request all get the
    /// same treatment — which is the point of forwarding rather than proxying.
    async fn open_forward<R, W>(
        &self,
        reader: &mut R,
        writer: &mut W,
        id: u64,
        auth: &Auth,
    ) -> Result<(), RemoteError>
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let Auth::Forward { target } = auth else {
            let error = RemoteError::ForwardDenied {
                reason: "a forward is opened with the ticket it was granted; this \
                         connection was authorised as a workspace session, which does \
                         not get to choose a target"
                    .into(),
            };
            let _ = send(writer, &error_envelope(id, &error)).await;
            return Err(error);
        };
        let timeout = self.config.forward_connect_timeout;
        let (host, port) = (target.host.clone(), target.port);
        let dialled = tokio::time::timeout(timeout, tokio::net::TcpStream::connect((host, port)));
        let mut target_stream = match dialled.await {
            Ok(Ok(stream)) => stream,
            Ok(Err(e)) => {
                let error = RemoteError::ForwardDenied {
                    reason: format!("{target} did not accept a connection: {e}"),
                };
                let _ = send(writer, &error_envelope(id, &error)).await;
                return Err(error);
            }
            Err(_) => {
                let error = RemoteError::ForwardDenied {
                    reason: format!(
                        "{target} had not accepted a connection after {}s",
                        timeout.as_secs()
                    ),
                };
                let _ = send(writer, &error_envelope(id, &error)).await;
                return Err(error);
            }
        };
        send(
            writer,
            &Envelope {
                id,
                body: Ok::<Payload, RemoteError>(Payload::ForwardOpen {
                    host: target.host.clone(),
                    port: target.port,
                }),
            },
        )
        .await?;
        let (to_target, to_client) = match tokio::io::copy_bidirectional(
            &mut target_stream,
            &mut join(reader, writer),
        )
        .await
        {
            Ok(copied) => copied,
            Err(e) if super::forward::ended_cleanly(&e) => return Ok(()),
            Err(e) => {
                return Err(RemoteError::Io {
                    context: format!("forward to {target}"),
                    reason: e.to_string(),
                });
            }
        };
        tracing::debug!(target = %target, to_target, to_client, "port forward closed");
        Ok(())
    }

    /// Take back every forward a session authorised, which is what closing a
    /// session means on the server's side.
    async fn revoke_forwards(&self, session_id: &str) {
        let revoked = self.forwards.lock().await.revoke_session(session_id);
        if revoked > 0 {
            tracing::debug!(session = %session_id, revoked, "port forwards closed with the session");
        }
    }

    async fn dispatch(
        &self,
        id: u64,
        request: Request,
        granted: &[RemoteCapability],
        upload: &mut Option<Upload>,
        auth: &Auth,
        session_id: &str,
    ) -> Result<Payload, RemoteError> {
        let result = self
            .dispatch_granted(&request, granted, upload, auth, session_id)
            .await;
        if let Err(error) = &result
            && matches!(error, RemoteError::Io { .. })
        {
            // Worth a line: it is the one class of refusal the operator can act
            // on, and without it the host's own problem looks like a silent no.
            tracing::warn!(request_id = id, error = %error, "workspace request failed");
        }
        result
    }

    async fn dispatch_granted(
        &self,
        request: &Request,
        granted: &[RemoteCapability],
        upload: &mut Option<Upload>,
        auth: &Auth,
        session_id: &str,
    ) -> Result<Payload, RemoteError> {
        // A forwarded connection is not a workspace session, so nothing in this
        // list is for it. Its one request (`forward_open`) is answered by the
        // session loop, which has the stream to hand over.
        if let Auth::Forward { target } = auth {
            return Err(RemoteError::Unauthorized {
                reason: format!(
                    "this connection was authorised for one forward to {target}; it is \
                     not a workspace session"
                ),
            });
        }
        if let Some(capability) = ServerConfig::required_capability(request)
            && !granted.contains(&capability)
        {
            return Err(RemoteError::CapabilityNotGranted { capability });
        }
        match request {
            Request::Ping => Ok(Payload::Pong {
                server: self.config.implementation.clone(),
                protocol_version: self.config.protocol.max,
            }),
            Request::ForwardGrant {
                host,
                port,
                ttl_secs,
                max_uses,
            } => {
                self.forward_grant(host, *port, *ttl_secs, *max_uses, session_id)
                    .await
            }
            Request::List {
                path,
                depth,
                max_entries,
            } => self.list(path.as_deref(), *depth, *max_entries),
            Request::Read { offset, len, path } => self.read(path, *offset, *len),
            Request::Search {
                query,
                path,
                max_results,
                case_sensitive,
            } => self.search(query, path.as_deref(), *max_results, *case_sensitive),
            Request::Write {
                path,
                content_b64,
                create_parents,
                expected_sha256,
            } => self.write(
                path,
                content_b64,
                *create_parents,
                expected_sha256.as_deref(),
            ),
            Request::Diff {
                path,
                staged,
                context,
            } => self.diff(path.as_deref(), *staged, *context).await,
            Request::Exec {
                argv,
                cwd,
                timeout_ms,
            } => self.exec(argv, cwd.as_deref(), *timeout_ms).await,
            Request::InstallBegin {
                version,
                sha256,
                total_bytes,
            } => self.install_begin(version, sha256, *total_bytes, upload),
            Request::InstallChunk { seq, data_b64 } => self.install_chunk(*seq, data_b64, upload),
            Request::InstallFinish { version } => self.install_finish(version, upload),
            Request::ForwardOpen => Err(RemoteError::Protocol {
                reason: "a forward open is answered by the session loop, because it \
                         ends the framed part of the stream"
                    .into(),
            }),
            Request::Hello { .. } => Err(RemoteError::Unauthorized {
                reason: "a second hello is not how re-authentication would work".into(),
            }),
        }
    }

    // ---- paths -------------------------------------------------------------

    fn resolve(&self, relative: &str) -> Result<RemotePath, RemoteError> {
        RemotePath::parse(&self.config.workspace_root, relative).map_err(|e| {
            RemoteError::PathRejected {
                reason: e.to_string(),
            }
        })
    }

    fn resolve_local(&self, relative: &str) -> Result<PathBuf, RemoteError> {
        Ok(self
            .resolve(relative)?
            .to_local(&self.config.workspace_root))
    }

    /// Whether a directory is one the listing and the search do not descend into.
    fn skipped(&self, path: &Path) -> bool {
        path.file_name().is_some_and(|name| {
            self.config
                .skip_dirs
                .iter()
                .any(|skip| name == std::ffi::OsStr::new(skip))
        })
    }

    // ---- listing -----------------------------------------------------------

    fn list(
        &self,
        path: Option<&str>,
        depth: Option<usize>,
        max_entries: Option<usize>,
    ) -> Result<Payload, RemoteError> {
        let base = match path {
            Some(relative) => self.resolve_local(relative)?,
            None => self.config.workspace_root.clone(),
        };
        if !base.exists() {
            return Err(RemoteError::NotFound {
                path: path.unwrap_or("<workspace root>").to_string(),
            });
        }
        let depth_limit = depth
            .unwrap_or(self.config.max_list_depth)
            .min(self.config.max_list_depth);
        let entry_limit = max_entries
            .unwrap_or(self.config.max_list_entries)
            .min(self.config.max_list_entries);
        let mut entries: Vec<Entry> = Vec::new();
        let mut truncated = false;
        let mut stack: Vec<(PathBuf, usize)> = vec![(base, 0)];
        while let Some((dir, depth)) = stack.pop() {
            if depth >= depth_limit {
                truncated = true;
                continue;
            }
            for child in sorted_children(&dir)? {
                let is_dir = child.file_type().map_err(|e| io_error(&dir, e))?.is_dir();
                if is_dir && self.skipped(&child.path()) {
                    // Skipped, not missing — but the caller has to be able to tell
                    // a complete listing from a filtered one.
                    truncated = true;
                    continue;
                }
                if entries.len() >= entry_limit {
                    truncated = true;
                    break;
                }
                let len = if is_dir {
                    0
                } else {
                    child.metadata().map(|m| m.len()).unwrap_or(0)
                };
                entries.push(Entry {
                    path: relative_of(&child.path(), &self.config.workspace_root),
                    is_dir,
                    len,
                });
                if is_dir {
                    stack.push((child.path(), depth + 1));
                }
            }
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Payload::List { entries, truncated })
    }

    // ---- reading -----------------------------------------------------------

    fn read(
        &self,
        relative: &str,
        offset: Option<u64>,
        len: Option<u64>,
    ) -> Result<Payload, RemoteError> {
        use std::io::{Read, Seek};
        let path = self.resolve_local(relative)?;
        let mut file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(RemoteError::NotFound {
                    path: relative.to_string(),
                });
            }
            Err(e) => return Err(io_error(&path, e)),
        };
        let total_len = file.metadata().map_err(|e| io_error(&path, e))?.len();
        let offset = offset.unwrap_or(0);
        if offset > total_len {
            return Err(RemoteError::InvalidRequest {
                reason: format!("offset {offset} is past the end of a {total_len}-byte file"),
            });
        }
        let wanted = len
            .unwrap_or(self.config.max_transfer_bytes)
            .min(self.config.max_transfer_bytes);
        let take = wanted.min(total_len - offset) as usize;
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| io_error(&path, e))?;
        let mut buffer = vec![0u8; take];
        file.read_exact(&mut buffer)
            .map_err(|e| io_error(&path, e))?;
        Ok(Payload::Read {
            data_b64: base64::engine::general_purpose::STANDARD.encode(&buffer),
            total_len,
            offset,
            // The digest of the bytes returned. A read of the whole file therefore
            // yields a digest usable as `expected_sha256` on the next write; a
            // windowed read does not, and passing one of those gets a Conflict,
            // which is the right answer — not a silent overwrite.
            sha256: sha256_hex(&buffer),
        })
    }

    // ---- searching ---------------------------------------------------------

    fn search(
        &self,
        query: &str,
        path: Option<&str>,
        max_results: Option<usize>,
        case_sensitive: bool,
    ) -> Result<Payload, RemoteError> {
        if query.is_empty() {
            return Err(RemoteError::InvalidRequest {
                reason: "an empty query would match every line of every file".into(),
            });
        }
        let base = match path {
            Some(relative) => self.resolve_local(relative)?,
            None => self.config.workspace_root.clone(),
        };
        let limit = max_results
            .unwrap_or(self.config.max_search_results)
            .min(self.config.max_search_results);
        let needle = if case_sensitive {
            query.to_string()
        } else {
            query.to_lowercase()
        };
        let mut hits: Vec<SearchHit> = Vec::new();
        let mut files_scanned = 0usize;
        let mut truncated = false;
        let mut stack = vec![base];
        'search: while let Some(dir) = stack.pop() {
            for child in sorted_children(&dir)? {
                let file_type = child.file_type().map_err(|e| io_error(&dir, e))?;
                if file_type.is_dir() {
                    if !self.skipped(&child.path()) {
                        stack.push(child.path());
                    }
                    continue;
                }
                if !file_type.is_file() {
                    continue;
                }
                if files_scanned >= self.config.max_search_files {
                    truncated = true;
                    break 'search;
                }
                files_scanned += 1;
                if child.metadata().map(|m| m.len()).unwrap_or(u64::MAX)
                    > self.config.max_search_file_bytes
                {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(child.path()) else {
                    // Not text, or unreadable: neither is a match, and a vendored
                    // binary should not fail a search.
                    continue;
                };
                for (index, line) in text.lines().enumerate() {
                    let haystack = if case_sensitive {
                        line
                    } else {
                        &line.to_lowercase()
                    };
                    if !haystack.contains(&needle) {
                        continue;
                    }
                    if hits.len() >= limit {
                        truncated = true;
                        break 'search;
                    }
                    hits.push(SearchHit {
                        path: relative_of(&child.path(), &self.config.workspace_root),
                        line: index + 1,
                        text: line.to_string(),
                    });
                }
            }
        }
        hits.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
        Ok(Payload::Search {
            hits,
            truncated,
            files_scanned,
        })
    }

    // ---- writing -----------------------------------------------------------

    fn write(
        &self,
        relative: &str,
        content_b64: &str,
        create_parents: bool,
        expected_sha256: Option<&str>,
    ) -> Result<Payload, RemoteError> {
        let path = self.resolve_local(relative)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(content_b64)
            .map_err(|e| RemoteError::InvalidRequest {
                reason: format!("content is not valid base64: {e}"),
            })?;
        if bytes.len() as u64 > self.config.max_transfer_bytes {
            return Err(RemoteError::InvalidRequest {
                reason: format!(
                    "{} bytes is over the {}-byte transfer limit",
                    bytes.len(),
                    self.config.max_transfer_bytes
                ),
            });
        }
        if let Some(expected) = expected_sha256 {
            // "It is not there" and "it is not what you expect" are different
            // answers, and both have to be distinguishable from success: creating
            // a file the caller believed existed is its own kind of data loss.
            let existing = match std::fs::read(&path) {
                Ok(existing) => Some(sha256_hex(&existing)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(io_error(&path, e)),
            };
            match existing {
                None => {
                    return Err(RemoteError::Conflict {
                        expected_sha256: expected.trim().to_string(),
                        actual_sha256: "<absent>".to_string(),
                    });
                }
                Some(actual) if !actual.eq_ignore_ascii_case(expected.trim()) => {
                    return Err(RemoteError::Conflict {
                        expected_sha256: expected.trim().to_string(),
                        actual_sha256: actual,
                    });
                }
                Some(_) => {}
            }
        }
        if let Some(parent) = path.parent() {
            if create_parents {
                std::fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
            } else if !parent.is_dir() {
                return Err(RemoteError::InvalidRequest {
                    reason: format!(
                        "the directory for {relative} does not exist; pass create_parents \
                         to make it"
                    ),
                });
            }
        }
        // A same-directory temporary name plus a rename: a reader sees either the
        // old file or the new one. Writing in place is a window in which the file
        // is half of one version and half of another.
        let tmp = path.with_extension(format!("chaos-tmp-{}", uuid::Uuid::new_v4().simple()));
        let result = (|| -> std::io::Result<()> {
            std::fs::write(&tmp, &bytes)?;
            preserve_mode_of(&tmp, &path)?;
            std::fs::rename(&tmp, &path)
        })();
        if let Err(e) = result {
            let _ = std::fs::remove_file(&tmp);
            return Err(io_error(&path, e));
        }
        Ok(Payload::Write {
            bytes_written: bytes.len() as u64,
            sha256: sha256_hex(&bytes),
        })
    }

    // ---- git ---------------------------------------------------------------

    /// `git diff` for the workspace.
    ///
    /// `git` is not on the executable allowlist and does not need to be: the argv
    /// is assembled here, the only caller-supplied piece is a pathspec that has
    /// already been resolved inside the workspace, and `Git` is a capability of its
    /// own that a session has to have been granted.
    async fn diff(
        &self,
        path: Option<&str>,
        staged: bool,
        context: Option<u32>,
    ) -> Result<Payload, RemoteError> {
        let mut argv: Vec<String> = vec!["git".into(), "diff".into()];
        if staged {
            argv.push("--staged".into());
        }
        argv.push(format!("-U{}", context.unwrap_or(3).min(40)));
        argv.push("--color=never".into());
        if let Some(relative) = path {
            // Resolve first, then hand git the workspace-relative form. Git already
            // knows the root; an absolute path here would be a second way to name
            // something outside it.
            argv.push(self.resolve(relative)?.relative().to_string());
        }
        let output = self
            .run(
                &argv,
                &self.config.workspace_root.clone(),
                Duration::from_secs(60),
            )
            .await?;
        // `git diff` exits 1 when there is a difference and 0 when there is not,
        // so a non-zero status is only an error past 1.
        match output.exit_code {
            Some(0) | Some(1) => {}
            other => {
                return Err(RemoteError::Git {
                    reason: format!(
                        "git diff exited {}: {}",
                        other
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "signal".into()),
                        String::from_utf8_lossy(&output.stderr).trim()
                    ),
                });
            }
        }
        let diff = String::from_utf8_lossy(&output.stdout).into_owned();
        Ok(Payload::Diff {
            dirty: !diff.trim().is_empty(),
            diff,
        })
    }

    // ---- tool execution ----------------------------------------------------

    /// Run one program, with no shell, inside the workspace, under a deadline.
    async fn exec(
        &self,
        argv: &[String],
        cwd: Option<&str>,
        timeout_ms: Option<u64>,
    ) -> Result<Payload, RemoteError> {
        let Some(first) = argv.first() else {
            return Err(RemoteError::InvalidRequest {
                reason: "argv is empty; there is nothing to run".into(),
            });
        };
        if first.contains('/') || first.contains('\\') {
            return Err(RemoteError::InvalidRequest {
                reason: format!(
                    "{first:?} must be a bare program name; the server resolves it, so a \
                     path would bypass the allowlist"
                ),
            });
        }
        if !self
            .config
            .allowed_executables
            .iter()
            .any(|allowed| allowed == first)
        {
            return Err(RemoteError::Exec {
                reason: format!(
                    "{first:?} is not on this server's allowlist ({:?})",
                    self.config.allowed_executables
                ),
            });
        }
        let root = self.config.workspace_root.clone();
        let cwd = match cwd {
            Some(relative) => {
                let dir = self.resolve_local(relative)?;
                if !dir.is_dir() {
                    return Err(RemoteError::NotFound {
                        path: relative.to_string(),
                    });
                }
                dir
            }
            None => root,
        };
        let requested = timeout_ms
            .map(Duration::from_millis)
            .unwrap_or(self.config.exec_default_timeout);
        let timeout = requested.min(self.config.exec_max_timeout);
        let output = self.run(argv, &cwd, timeout).await?;
        Ok(Payload::Exec {
            exit_code: output.exit_code,
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            timed_out: output.timed_out,
            truncated: output.truncated,
        })
    }

    // ---- install -----------------------------------------------------------

    fn install_begin(
        &self,
        version: &str,
        sha256: &str,
        total_bytes: u64,
        upload: &mut Option<Upload>,
    ) -> Result<Payload, RemoteError> {
        if upload.is_some() {
            return Err(RemoteError::Install {
                reason: "an upload is already in progress on this session".into(),
                rolled_back: false,
            });
        }
        if sha256.trim().len() != 64 {
            return Err(RemoteError::Install {
                reason: "the expected sha256 must be 64 hex digits".into(),
                rolled_back: false,
            });
        }
        let staged = self
            .config
            .install
            .begin_staging(version)
            .map_err(|reason| RemoteError::Install {
                reason,
                rolled_back: false,
            })?;
        if total_bytes > self.config.max_transfer_bytes * 64 {
            return Err(RemoteError::Install {
                reason: format!("a {total_bytes}-byte artifact is over the transfer budget"),
                rolled_back: false,
            });
        }
        *upload = Some(Upload {
            version: version.to_string(),
            sha256: sha256.trim().to_string(),
            total_bytes,
            received_bytes: 0,
            next_seq: 0,
            staged,
        });
        Ok(Payload::InstallBegin { received_bytes: 0 })
    }

    fn install_chunk(
        &self,
        seq: u64,
        data_b64: &str,
        upload: &mut Option<Upload>,
    ) -> Result<Payload, RemoteError> {
        use std::io::Write;
        let fail = |reason: String| RemoteError::Install {
            reason,
            rolled_back: false,
        };
        let state = upload.as_mut().ok_or_else(|| {
            fail("no upload is in progress; install_chunk follows install_begin".into())
        })?;
        if seq != state.next_seq {
            // Nothing useful can be done with an out-of-order chunk. The staged
            // file is left as it is and the caller restarts the transfer, which is
            // what `install_begin` does to the staging file anyway.
            return Err(fail(format!(
                "expected chunk {expected}, got {seq}; restart the transfer",
                expected = state.next_seq
            )));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data_b64)
            .map_err(|e| fail(format!("chunk {seq} is not valid base64: {e}")))?;
        let arrived = state.received_bytes + bytes.len() as u64;
        if arrived > state.total_bytes {
            return Err(fail(format!(
                "chunk {seq} would make the artifact {arrived} bytes, past the announced {}",
                state.total_bytes
            )));
        }
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&state.staged)
            .map_err(|e| fail(format!("append {}: {e}", state.staged.display())))?;
        file.write_all(&bytes)
            .map_err(|e| fail(format!("append {}: {e}", state.staged.display())))?;
        state.received_bytes = arrived;
        state.next_seq += 1;
        Ok(Payload::InstallChunk {
            received_bytes: state.received_bytes,
        })
    }

    fn install_finish(
        &self,
        version: &str,
        upload: &mut Option<Upload>,
    ) -> Result<Payload, RemoteError> {
        let fail = |reason: String| RemoteError::Install {
            reason,
            rolled_back: false,
        };
        let Some(state) = upload.take() else {
            return Err(fail("no upload is in progress".into()));
        };
        let short = |reason: String| {
            self.config.install.discard_staging(&state.version);
            fail(reason)
        };
        if state.version != version {
            return Err(short(format!(
                "the upload was announced as {} and finished as {version}",
                state.version
            )));
        }
        if state.received_bytes != state.total_bytes {
            return Err(short(format!(
                "the transfer ended at {} of {} bytes",
                state.received_bytes, state.total_bytes
            )));
        }
        match self
            .config
            .install
            .commit(&state.version, &state.staged, &state.sha256)
        {
            Ok(outcome) => {
                tracing::info!(version = %outcome.version, previous = ?outcome.previous, "server artifact installed");
                Ok(Payload::InstallFinish {
                    version: outcome.version,
                    current: true,
                    previous_version: outcome.previous,
                })
            }
            Err(reason) => {
                self.config.install.discard_staging(&state.version);
                // The layout says whether it put the old pointer back; passing that
                // through is how a caller learns whether the host is still serving
                // the version it was serving before.
                let rolled_back = reason.contains("restored");
                Err(RemoteError::Install {
                    reason,
                    rolled_back,
                })
            }
        }
    }

    // ---- processes ---------------------------------------------------------

    /// Run one program with no shell, in one directory, under a deadline.
    ///
    /// Shared by `exec` and by `git`. The child belongs to the server's
    /// [`ProcessScope`], so a server that goes away takes its tool processes with
    /// it instead of leaving a build running against a workspace nobody is serving.
    async fn run(
        &self,
        argv: &[String],
        cwd: &Path,
        timeout: Duration,
    ) -> Result<RawOutput, RemoteError> {
        use std::process::Stdio;
        use tokio::process::Command;
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]);
        cmd.current_dir(cwd);
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        // The environment is the server's, unmodified. Stripping it would break
        // `git`, which wants HOME, and would be theatre anyway: what is in there is
        // what the operator put in the process that started this server.
        let (mut child, group) = self.scope.spawn(cmd).map_err(|e| RemoteError::Exec {
            reason: format!("spawn {}: {e}", argv[0]),
        })?;
        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();
        let cap = self.config.max_exec_output_bytes;

        let (exit_code, timed_out) = match tokio::time::timeout(timeout, child.wait()).await {
            Ok(Ok(status)) => (status.code(), false),
            // The wait itself failed, which is not a timeout but is not a result
            // either; treating it as killed is the only answer that does not hang.
            Ok(Err(_)) => (None, true),
            Err(_) => {
                // Kill the group, not just the direct child: a tool that forked has
                // left its workers behind, and those are the ones still writing to
                // the workspace.
                let _ = group.kill();
                let _ = child.start_kill();
                (None, true)
            }
        };
        let stdout_bytes = read_capped(stdout.as_mut(), cap).await;
        let stderr_bytes = read_capped(stderr.as_mut(), cap).await;
        // Reap the corpse so it does not sit in the process table until the scope
        // is next visited.
        if timed_out {
            let _ = child.wait().await;
        }
        Ok(RawOutput {
            exit_code,
            stdout: stdout_bytes.bytes,
            stderr: stderr_bytes.bytes,
            timed_out,
            truncated: stdout_bytes.truncated || stderr_bytes.truncated,
        })
    }
}

/// A chunked artifact transfer in progress on one session.
struct Upload {
    version: String,
    sha256: String,
    total_bytes: u64,
    received_bytes: u64,
    next_seq: u64,
    staged: PathBuf,
}

struct RawOutput {
    exit_code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    timed_out: bool,
    truncated: bool,
}

struct CappedRead {
    bytes: Vec<u8>,
    truncated: bool,
}

/// Read a pipe, giving up once `cap` bytes have arrived.
///
/// The cap is what turns a tool that prints the internet into a truncated reply
/// rather than an out-of-memory kill.
async fn read_capped<R>(pipe: Option<&mut R>, cap: usize) -> CappedRead
where
    R: AsyncRead + Unpin,
{
    let Some(pipe) = pipe else {
        return CappedRead {
            bytes: Vec::new(),
            truncated: false,
        };
    };
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8 * 1024];
    let mut truncated = false;
    loop {
        match pipe.read(&mut buffer).await {
            Ok(0) => break,
            Ok(read) => {
                let room = cap.saturating_sub(bytes.len());
                let keep = read.min(room);
                truncated |= keep < read;
                bytes.extend_from_slice(&buffer[..keep]);
                if bytes.len() >= cap {
                    truncated = true;
                    break;
                }
            }
            // A read error after a kill is the normal shape of a timeout.
            Err(_) => break,
        }
    }
    CappedRead { bytes, truncated }
}

fn error_envelope(id: u64, error: &RemoteError) -> Envelope<Reply> {
    Envelope {
        id,
        body: Err(error.clone()),
    }
}

fn io_error(path: &Path, e: std::io::Error) -> RemoteError {
    RemoteError::Io {
        context: format!("{}", path.display()),
        reason: e.to_string(),
    }
}

/// Directory children in name order.
///
/// Sorted so that the same tree lists and searches the same way twice. Relying on
/// the order the filesystem happens to return makes a listing, and the truncation
/// of a search, depend on nothing anyone can predict.
fn sorted_children(dir: &Path) -> Result<Vec<DirEntry>, RemoteError> {
    let mut children: Vec<DirEntry> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name() != ".")
            .collect(),
        Err(e) => return Err(io_error(dir, e)),
    };
    children.sort_by_key(DirEntry::file_name);
    Ok(children)
}

/// The workspace-relative form of an absolute path, `/`-separated.
fn relative_of(path: &Path, root: &Path) -> String {
    let Ok(relative) = path.strip_prefix(root) else {
        // Nothing to strip, so it comes back as it was given. Re-joining its
        // components instead would turn `/elsewhere/x.rs` into `elsewhere/x.rs`,
        // which reads as a path inside the workspace and is not one.
        return path.to_string_lossy().into_owned();
    };
    relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Only loopback.
///
/// This server authenticates with a file beside its socket and adds no transport
/// security of its own, so a routable listener would be a workspace open to anyone
/// who can route to the port. Tunnelling is somebody else's job and is the intended
/// arrangement: an SSH-forwarded loopback port is how this is reached from another
/// machine.
/// The way a forward host is spelled when two of them are being compared.
///
/// Brackets around an IPv6 literal are dropped, because a URL-ish
/// `[::1]:8080` and the address `::1` mean the same place, and case is dropped
/// because hostnames are case-insensitive on the wire even when a shell is not.
fn normalize_forward_host(host: &str) -> String {
    let trimmed = host.trim().to_ascii_lowercase();
    match trimmed
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
    {
        Some(inner) => inner.to_string(),
        None => trimmed,
    }
}

pub fn require_loopback(addr: SocketAddr) -> Result<(), String> {
    if addr.ip().is_loopback() {
        return Ok(());
    }
    Err(format!(
        "refusing to listen on {}: this server speaks no transport security. Bind a \
         loopback address and reach it through a tunnel you control.",
        addr.ip()
    ))
}

/// The same rule applied to a host the operator typed.
///
/// A hostname is not accepted, because whether `buildbox` is loopback depends on
/// `/etc/hosts`, and a rule that depends on `/etc/hosts` is not a rule. An address
/// is checked rather than trusted, because `0.0.0.0` parses fine and means the
/// opposite of what `--tcp` promises.
pub fn parse_loopback_host(host: &str) -> Result<IpAddr, String> {
    let addr = match host.parse::<IpAddr>() {
        Ok(addr) => addr,
        Err(_) if host == "localhost" => IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        Err(_) => {
            return Err(format!(
                "{host:?} is not a loopback literal; a hostname that might resolve \
                 elsewhere is not accepted — use 127.0.0.1 or ::1"
            ));
        }
    };
    if !addr.is_loopback() {
        return Err(format!(
            "{addr} is not a loopback address; this server speaks no transport \
             security, so it listens on 127.0.0.1 or ::1 and is reached through a \
             tunnel you control"
        ));
    }
    Ok(addr)
}

/// Give a replacement file the mode of the file it replaces, so editing a 0755
/// script does not quietly make it unrunnable.
///
/// The file being replaced usually does not exist yet — that is the ordinary case
/// for a first write — and a file that is not there has no mode to carry over.
fn preserve_mode_of(replacement: &Path, replaced: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let Ok(metadata) = std::fs::metadata(replaced) else {
            return Ok(());
        };
        let mode = metadata.permissions().mode() & 0o7777;
        std::fs::set_permissions(replacement, std::fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        // Windows has no POSIX mode bits, so there is nothing to carry over and the
        // replacement keeps the mode it was created with. Forcing one here would be
        // a new bug rather than a fix.
        let _ = (replacement, replaced);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_loopback_may_be_listened_on() {
        let loopback_v4 = SocketAddr::from(([127, 0, 0, 1], 4000));
        let loopback_v6 = SocketAddr::new(IpAddr::V6(std::net::Ipv6Addr::LOCALHOST), 4000);
        assert!(require_loopback(loopback_v4).is_ok());
        assert!(require_loopback(loopback_v6).is_ok());

        for routable in [
            SocketAddr::from(([10, 0, 0, 5], 4000)),
            SocketAddr::from(([192, 168, 1, 2], 4000)),
            SocketAddr::from(([0, 0, 0, 0], 4000)),
        ] {
            let err = require_loopback(routable).expect_err("must refuse a routable address");
            assert!(err.contains("tunnel"), "{err}");
        }
    }

    #[test]
    fn a_hostname_is_not_accepted_as_loopback() {
        assert!(parse_loopback_host("127.0.0.1").is_ok());
        assert!(parse_loopback_host("::1").is_ok());
        assert!(parse_loopback_host("localhost").is_ok());
        assert!(parse_loopback_host("buildbox").is_err());
    }

    /// `0.0.0.0` and a routable address both parse as `IpAddr`, and both mean
    /// "expose this to the network", which is the one thing refused here.
    #[test]
    fn an_address_that_is_not_loopback_is_refused_where_it_is_parsed() {
        for host in ["0.0.0.0", "::", "203.0.113.7", "192.168.1.20"] {
            let err = parse_loopback_host(host).expect_err("must refuse {host}");
            assert!(err.contains("loopback"), "{host}: {err}");
        }
        // The whole of 127/8 is loopback, not just the one address people type.
        assert!(parse_loopback_host("127.0.0.42").is_ok());
    }

    #[test]
    fn relative_paths_are_rendered_the_way_a_request_names_them() {
        let root = Path::new("/work/project");
        assert_eq!(
            relative_of(&root.join("src").join("main.rs"), root),
            "src/main.rs"
        );
        assert_eq!(relative_of(root, root), "");
        // A path outside the root has nothing to strip; it comes back as itself
        // rather than being silently rewritten.
        assert_eq!(
            relative_of(Path::new("/elsewhere/x.rs"), root),
            "/elsewhere/x.rs"
        );
    }

    #[test]
    fn a_configuration_is_refused_before_it_can_be_mistaken_for_a_server() {
        let missing = ServerConfig::new("/definitely/not/here/9e1c");
        assert!(missing.validate().is_err());

        let dir = tempfile::tempdir().unwrap();
        let config = ServerConfig::new(dir.path()).allowed_executables(["/bin/sh"]);
        assert!(
            config.validate().is_err(),
            "an allowlist entry with a path in it lets the entry decide where \
             programs live"
        );
        assert!(ServerConfig::new(dir.path()).validate().is_ok());
    }

    #[tokio::test]
    async fn output_is_capped_and_the_overrun_is_reported() {
        let noisy = vec![b'x'; 5000];
        let mut reader = &noisy[..];
        let read = read_capped(Some(&mut reader), 1000).await;
        assert_eq!(read.bytes.len(), 1000);
        assert!(read.truncated);

        let quiet = b"short".to_vec();
        let mut reader = &quiet[..];
        let read = read_capped(Some(&mut reader), 1000).await;
        assert_eq!(read.bytes, quiet);
        assert!(!read.truncated);
    }

    /// A session that asked for nothing is not a session that asked for everything.
    #[tokio::test]
    async fn an_empty_capability_request_grants_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::new(ServerConfig::new(dir.path())).unwrap();
        assert!(server.grant(&[]).is_empty());
        let granted = server.grant(&[
            RemoteCapability::WorkspaceRead,
            RemoteCapability::ToolExecution,
        ]);
        assert_eq!(granted, vec![RemoteCapability::WorkspaceRead]);
    }

    #[tokio::test]
    async fn a_server_refuses_its_own_missing_root() {
        let err = Server::new(ServerConfig::new("/definitely/not/here/9e1c"))
            .expect_err("no root, no server");
        assert!(err.contains("not a directory"), "{err}");
    }
}
