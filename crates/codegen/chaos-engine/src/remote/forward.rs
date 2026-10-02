//! Local port forwarding: a port here, a service there.
//!
//! This is SSH's local forwarding (`-L`), and the naming matters because the two
//! directions get confused constantly. A *local* forward listens on the machine
//! that started it and carries each connection to a target the *server* can reach;
//! a *remote* forward would listen on the remote host and carry connections back
//! the other way. Only the first one exists here, and the reason it is the one
//! worth having is the trust question: a local forward answers "may I reach that
//! service from over there", which the operator already answered when they listed
//! the target. A remote forward would answer "may something now be reachable by
//! everyone who can reach that host", which no workspace grant implies.
//!
//! What this is not: a proxy. It never looks at the bytes, so HTTP, a WebSocket
//! upgrade, a database handshake and a file download are all the same job. That is
//! also why nothing here needs to know about TLS.
//!
//! The lifecycle is the part worth reading twice. A tunnel is authorised by a
//! [`ForwardGrant`] that a live session asked for, so it ends in exactly three ways:
//! the grant runs out of connections, the grant's lifetime passes, or the tunnel is
//! dropped — and dropping the listener is what releases the local port, while the
//! session closing is what revokes the ticket on the server. Both ends therefore
//! close on their own; there is no half-open forward left behind to be discovered
//! months later.

use std::future::Future;
use std::net::SocketAddr;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};

use super::client::{ForwardGrant, RemoteWorkspace, RemoteWorkspaceConfig};
use super::credentials::{ForwardTarget, SessionToken};
use super::endpoint::{RemoteCapability, RemoteEndpoint};
use super::protocol::RemoteError;
use super::server::require_loopback;

/// What a tunnel has carried.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ForwardStats {
    /// Connections that got as far as being connected to the target.
    pub connections: usize,
    /// Connections that were refused before any of the target's bytes moved.
    pub refused: usize,
    pub bytes_to_target: u64,
    pub bytes_to_client: u64,
}

/// A local listener that carries each connection it accepts to one remote target.
pub struct ForwardTunnel {
    endpoint: RemoteEndpoint,
    grant: ForwardGrant,
    config: RemoteWorkspaceConfig,
    listener: TcpListener,
    stats: ForwardStats,
    /// Kept so a tunnel that never carried anything can say why rather than
    /// reporting a bare zero.
    last_error: Option<String>,
}

impl std::fmt::Debug for ForwardTunnel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForwardTunnel")
            .field("target", &self.grant.target)
            .field("local", &self.listener.local_addr().ok())
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

impl ForwardTunnel {
    /// Bind the local end of a forward that `grant` authorises.
    ///
    /// Port 0 means "give me a free one", and [`Self::local_addr`] is how the
    /// caller finds out which one it got. A port that is already held is refused by
    /// name: silently binding elsewhere would leave whatever was told about the
    /// listener talking to a port that forwards nothing.
    pub async fn bind(
        endpoint: RemoteEndpoint,
        grant: ForwardGrant,
        config: RemoteWorkspaceConfig,
        listen: SocketAddr,
    ) -> Result<Self, RemoteError> {
        if !endpoint
            .capabilities
            .contains(&RemoteCapability::PortForward)
        {
            return Err(RemoteError::InvalidRequest {
                reason: format!(
                    "the endpoint {host} is not configured to allow port forwarding; \
                     add `port-forward` to its capabilities if you mean to allow it",
                    host = endpoint.host
                ),
            });
        }
        if !config.capabilities.contains(&RemoteCapability::PortForward) {
            return Err(RemoteError::InvalidRequest {
                reason: "this session never asked for the port-forward capability, so \
                         no connection it makes will be authorised to forward"
                    .into(),
            });
        }
        config
            .validate()
            .map_err(|reason| RemoteError::InvalidRequest { reason })?;
        // The local end is loopback-only for the same reason the server's own
        // listener is: putting a forwarded service on a routable interface is what
        // publishing it means, and that is not what "let me reach it from here"
        // asked for.
        require_loopback(listen).map_err(|reason| RemoteError::InvalidRequest { reason })?;
        let listener = TcpListener::bind(listen)
            .await
            .map_err(|e| RemoteError::Io {
                context: format!("bind the local end of the forward to {listen}"),
                reason: if e.kind() == std::io::ErrorKind::AddrInUse {
                    format!(
                        "{e} — something already holds port {port}; pass port 0 to be given \
                     a free one",
                        port = listen.port()
                    )
                } else {
                    e.to_string()
                },
            })?;
        tracing::debug!(
            local = %listener.local_addr().map(|a| a.to_string()).unwrap_or_default(),
            target = %grant.target,
            uses = grant.max_uses,
            "port forward listening"
        );
        Ok(Self {
            endpoint,
            grant,
            config,
            listener,
            stats: ForwardStats::default(),
            last_error: None,
        })
    }

    /// The address to point a browser or a client program at.
    pub fn local_addr(&self) -> Result<SocketAddr, RemoteError> {
        self.listener.local_addr().map_err(|e| RemoteError::Io {
            context: "read back the local forward address".into(),
            reason: e.to_string(),
        })
    }

    pub fn target(&self) -> &ForwardTarget {
        &self.grant.target
    }

    pub fn grant(&self) -> &ForwardGrant {
        &self.grant
    }

    pub fn stats(&self) -> ForwardStats {
        self.stats
    }

    /// Accept one connection and carry it to the end.
    ///
    /// Exposed separately from [`Self::serve`] because a caller that wants to prove
    /// one thing, or to stop at a known point, should not have to arrange for the
    /// shutdown itself.
    pub async fn serve_one<S, F, Fut>(&mut self, dial: &mut F) -> Result<ForwardTarget, RemoteError>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<S, RemoteError>>,
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let (local, peer) = self.listener.accept().await.map_err(|e| RemoteError::Io {
            context: "accept a connection on the local forward port".into(),
            reason: e.to_string(),
        })?;
        tracing::debug!(%peer, target = %self.grant.target, "forward connection accepted");
        self.carry(local, dial).await
    }

    /// Accept connections until the grant runs out or its lifetime passes.
    ///
    /// The server is the authority on both of those; stopping here as well is
    /// courtesy, and it is what makes a finished tunnel give its port back without
    /// anyone having to kill it.
    ///
    /// A connection that is refused is counted and logged, not fatal: a browser
    /// that gives up on one of twelve parallel requests is not a broken tunnel. But
    /// a tunnel that never carried *anything* failed, and comes back as an error
    /// carrying the reason the first connection was refused — otherwise the only
    /// evidence that the forward never worked is a line saying so on stderr.
    pub async fn serve<S, F, Fut>(&mut self, dial: &mut F) -> Result<ForwardStats, RemoteError>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<S, RemoteError>>,
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let deadline = tokio::time::Instant::now() + self.grant.expires_in;
        while self.stats.connections + self.stats.refused < self.grant.max_uses {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                tracing::info!(target = %self.grant.target, "the forward's grant expired");
                break;
            }
            let accepted = match tokio::time::timeout(remaining, self.listener.accept()).await {
                Ok(result) => result,
                Err(_) => {
                    tracing::info!(target = %self.grant.target, "the forward's grant expired");
                    break;
                }
            }
            .map_err(|e| RemoteError::Io {
                context: "accept a connection on the local forward port".into(),
                reason: e.to_string(),
            })?;
            match self.carry(accepted.0, dial).await {
                Ok(_) => {}
                Err(error) => {
                    self.stats.refused += 1;
                    self.last_error = Some(error.to_string());
                    tracing::warn!(error = %error, target = %self.grant.target, "forwarded connection refused");
                }
            }
        }
        if self.stats.connections == 0 {
            return Err(match self.last_error.take() {
                Some(reason) => RemoteError::ForwardDenied { reason },
                None => RemoteError::Timeout {
                    request: "the first connection on the forward".into(),
                    after: self.grant.expires_in,
                },
            });
        }
        Ok(self.stats)
    }

    /// One connection, start to finish.
    async fn carry<S, F, Fut>(
        &mut self,
        mut local: TcpStream,
        dial: &mut F,
    ) -> Result<ForwardTarget, RemoteError>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<S, RemoteError>>,
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let stream = dial().await?;
        // The ticket goes in through the same door as a session credential, and the
        // server decides what it means. That is not a shortcut: it is the reason a
        // ticket cannot smuggle itself into a workspace session.
        let session = RemoteWorkspace::connect(
            stream,
            &self.endpoint,
            &SessionToken::from_forward_ticket(&self.grant.ticket),
            self.config.clone(),
        )
        .await?;
        let (target, mut pipe) = session.into_forward_stream().await?;
        self.stats.connections += 1;
        // `copy_bidirectional` reports (this side -> far side, far side -> this
        // side), which is the opposite way round from how the tunnel reads.
        match tokio::io::copy_bidirectional(&mut local, &mut pipe).await {
            Ok((to_target, to_client)) => {
                self.stats.bytes_to_client += to_client;
                self.stats.bytes_to_target += to_target;
            }
            // A conversation that ends with one side hanging up is how every TCP
            // session finishes. Only the interesting kinds of failure are worth a
            // warning, and `copy_bidirectional` has already told us which this was.
            Err(e) if ended_cleanly(&e) => {}
            Err(e) => {
                tracing::debug!(target = %target, error = %e, "forward closed early");
            }
        }
        Ok(target)
    }
}

/// Whether a copy error is just the conversation ending.
///
/// A TCP session normally finishes with one side hanging up, and the copier calls
/// that an error — of one of several kinds, depending on which half closed and how
/// fast. Treating them as failures would mean a warning for every ordinary
/// download, which is how logs stop being read.
pub(crate) fn ended_cleanly(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::NotConnected
    )
}

/// Parse what was typed for the local end of a forward.
///
/// A bare port and a bare `:port` both mean loopback, because that is what "give me
/// a local port" means. A name is refused rather than resolved: the thing being
/// decided here is where to *listen*, and picking an interface by asking DNS is not
/// a decision anyone would want made for them.
pub fn parse_listen_endpoint(text: &str) -> Result<SocketAddr, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("the local end of a forward is empty".into());
    }
    let with_host = if text.starts_with(':') {
        format!("127.0.0.1{text}")
    } else if text.parse::<u16>().is_ok() {
        format!("127.0.0.1:{text}")
    } else {
        text.to_string()
    };
    let addr: SocketAddr = with_host.parse().map_err(|_| {
        format!(
            "{text:?} is not an address to listen on; use a port (8080), a loopback \
             address (127.0.0.1:8080, [::1]:8080) or 0 for any free port"
        )
    })?;
    require_loopback(addr)?;
    Ok(addr)
}

/// Parse what was typed for the target of a forward: `host:port`, with brackets
/// around an IPv6 literal.
///
/// Unlike the local end this is a name to *connect to*, and it may be any host the
/// server will agree to reach — which is a question the server's allowlist answers,
/// not this parser.
pub fn parse_forward_target(text: &str) -> Result<ForwardTarget, String> {
    let text = text.trim();
    let (host, port) = match (text.find('['), text.rfind(':')) {
        (Some(start), Some(colon)) if text[start..].starts_with('[') && colon > start => {
            let host = &text[start + 1..colon - 1];
            if !text[..start].trim().is_empty() || colon + 1 >= text.len() {
                return Err(format_target_usage(text));
            }
            (host, &text[colon + 1..])
        }
        (_, Some(colon)) if colon != 0 && text.matches(':').count() == 1 => {
            (&text[..colon], &text[colon + 1..])
        }
        _ => return Err(format_target_usage(text)),
    };
    let host = host.trim().to_string();
    if host.is_empty() {
        return Err("a forward target needs a host to connect to".into());
    }
    let port: u16 = port
        .trim()
        .parse()
        .map_err(|_| format!("{text:?} does not end in a port number"))?;
    if port == 0 {
        return Err(
            "a forward target's port cannot be 0; there is nothing to connect to there".into(),
        );
    }
    Ok(ForwardTarget::new(host, port))
}

fn format_target_usage(text: &str) -> String {
    format!("{text:?} is not a forward target; expected host:port, or [::1]:port")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use futures_util::{SinkExt as _, StreamExt as _};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::task::JoinHandle;

    use crate::remote::endpoint::HostKeyPolicy;
    use crate::remote::server::{Server, ServerConfig};

    fn endpoint(capabilities: Vec<RemoteCapability>) -> RemoteEndpoint {
        RemoteEndpoint {
            host: "buildbox".into(),
            port: 22,
            host_key: HostKeyPolicy::Strict,
            capabilities,
        }
    }

    fn forward_config() -> RemoteWorkspaceConfig {
        RemoteWorkspaceConfig::new().capabilities(vec![RemoteCapability::PortForward])
    }

    fn target_of(addr: SocketAddr) -> ForwardTarget {
        ForwardTarget::new(addr.ip().to_string(), addr.port())
    }

    /// A server that will forward to `targets`, listening on loopback TCP — the
    /// same transport a real session uses, because the forwarding path only exists
    /// once something can dial it twice.
    async fn server(
        root: &std::path::Path,
        targets: Vec<ForwardTarget>,
    ) -> (Arc<Server>, SocketAddr) {
        let config = ServerConfig::new(root)
            .capabilities(vec![
                RemoteCapability::WorkspaceRead,
                RemoteCapability::PortForward,
            ])
            .forward_targets(targets)
            .forward_connect_timeout(Duration::from_secs(2))
            .allowed_executables(["true"]);
        let server = Server::new(config).unwrap();
        let listener = server
            .clone()
            .serve_loopback_tcp("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(server.clone().accept_tcp(listener));
        (server, addr)
    }

    /// A session that asked for `capabilities`, over a real socket.
    async fn session(
        server: &Arc<Server>,
        addr: SocketAddr,
        capabilities: Vec<RemoteCapability>,
    ) -> RemoteWorkspace<TcpStream> {
        let token = server.issue_tokens(1).await.remove(0);
        let stream = TcpStream::connect(addr).await.unwrap();
        RemoteWorkspace::connect(
            stream,
            &endpoint(capabilities.clone()),
            &token,
            RemoteWorkspaceConfig::new().capabilities(capabilities),
        )
        .await
        .unwrap()
    }

    /// What the far end is: a fixed answer, or a WebSocket that echoes.
    enum Service {
        Fixed(&'static [u8]),
        WebSocket,
    }

    /// A service on loopback that counts the connections it accepted. A test reads
    /// that counter to prove the bytes reached the far side rather than being
    /// answered by the tunnel.
    async fn service(kind: Service) -> (SocketAddr, Arc<AtomicUsize>, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&hits);
        let task = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                counted.fetch_add(1, Ordering::SeqCst);
                match kind {
                    Service::Fixed(reply) => {
                        let mut buf = [0u8; 4096];
                        // Answer once *something* has arrived, the way a real
                        // server does; waiting for EOF would make the test's
                        // half-close part of the protocol.
                        let _ = stream.read(&mut buf).await;
                        let (_, mut out) = stream.into_split();
                        let _ = out.write_all(reply).await;
                        let _ = out.shutdown().await;
                    }
                    Service::WebSocket => {
                        if let Ok(mut socket) = tokio_tungstenite::accept_async(stream).await {
                            use tokio_tungstenite::tungstenite::Message;
                            if let Some(Ok(Message::Text(text))) = socket.next().await {
                                let _ = socket.send(Message::Text(text)).await;
                            }
                            let _ = socket.close(None).await;
                        }
                    }
                }
            }
        });
        (addr, hits, task)
    }

    fn dialer(
        addr: SocketAddr,
    ) -> impl FnMut() -> std::pin::Pin<Box<dyn Future<Output = Result<TcpStream, RemoteError>> + Send>>
    {
        move || {
            Box::pin(async move {
                TcpStream::connect(addr).await.map_err(|e| RemoteError::Io {
                    context: "dial the server".into(),
                    reason: e.to_string(),
                })
            })
        }
    }

    /// Read a forwarded connection to its end and hang up.
    ///
    /// The pump ends when both directions have finished, and the second FIN comes
    /// from the client hanging up — a test that reads the answer and then awaits the
    /// tunnel while still holding the socket open is waiting for itself.
    async fn drain(mut client: TcpStream) -> Vec<u8> {
        let mut got = Vec::new();
        client.read_to_end(&mut got).await.unwrap();
        drop(client);
        got
    }

    async fn grant_for(
        control: &mut RemoteWorkspace<TcpStream>,
        target: &ForwardTarget,
        uses: usize,
    ) -> ForwardGrant {
        control
            .forward_grant(target, Duration::from_secs(60), uses)
            .await
            .unwrap()
    }

    /// The whole point of the feature, driven end to end: a listener here, a
    /// service there, and bytes that arrive unchanged.
    #[tokio::test]
    async fn a_forwarded_connection_reaches_the_service_behind_it() {
        let root = tempfile::tempdir().unwrap();
        let (service_addr, hits, _service) =
            service(Service::Fixed(b"hello from the service\n")).await;
        let (server, addr) = server(root.path(), vec![target_of(service_addr)]).await;

        let mut control = session(&server, addr, vec![RemoteCapability::PortForward]).await;
        let grant = grant_for(&mut control, &target_of(service_addr), 4).await;
        assert_eq!(grant.target, target_of(service_addr));
        assert_eq!(grant.max_uses, 4);

        let mut tunnel = ForwardTunnel::bind(
            endpoint(vec![RemoteCapability::PortForward]),
            grant,
            forward_config(),
            "127.0.0.1:0".parse().unwrap(),
        )
        .await
        .unwrap();
        let local = tunnel.local_addr().unwrap();
        assert!(local.ip().is_loopback());

        let mut dial = dialer(addr);
        let carried = tokio::spawn(async move { tunnel.serve_one(&mut dial).await });

        let mut client = TcpStream::connect(local).await.unwrap();
        client.write_all(b"ping\n").await.unwrap();
        let got = drain(client).await;

        assert_eq!(String::from_utf8(got).unwrap(), "hello from the service\n");
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "the service never saw the connection, so something else answered"
        );
        assert_eq!(
            carried.await.unwrap().unwrap(),
            target_of(service_addr),
            "the tunnel should report the target it reached"
        );
    }

    /// An HTTP exchange through the tunnel, byte for byte, with the connection
    /// closed by the far side — the shape every command-line client produces. This
    /// is what "we do not parse the bytes" has to be true about.
    #[tokio::test]
    async fn a_forwarded_http_exchange_arrives_byte_for_byte() {
        let root = tempfile::tempdir().unwrap();
        let body = b"HTTP/1.0 200 OK\r\nContent-Length: 5\r\n\r\nhello";
        let (service_addr, hits, _service) = service(Service::Fixed(body)).await;
        let (server, addr) = server(root.path(), vec![target_of(service_addr)]).await;
        let mut control = session(&server, addr, vec![RemoteCapability::PortForward]).await;
        let grant = grant_for(&mut control, &target_of(service_addr), 2).await;

        let mut tunnel = ForwardTunnel::bind(
            endpoint(vec![RemoteCapability::PortForward]),
            grant,
            forward_config(),
            "127.0.0.1:0".parse().unwrap(),
        )
        .await
        .unwrap();
        let local = tunnel.local_addr().unwrap();
        let mut dial = dialer(addr);
        let carried = tokio::spawn(async move { tunnel.serve_one(&mut dial).await });

        let mut client = TcpStream::connect(local).await.unwrap();
        client.write_all(b"GET / HTTP/1.0\r\n\r\n").await.unwrap();
        let response = drain(client).await;
        assert_eq!(
            response,
            body.to_vec(),
            "the tunnel changed the bytes it was carrying"
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert!(carried.await.unwrap().is_ok());
    }

    /// A WebSocket upgrade is a request, a 101, and then a framed protocol that
    /// must not be touched. It works because nothing here inspects anything.
    #[tokio::test]
    async fn a_forwarded_websocket_handshake_and_frame_complete() {
        let root = tempfile::tempdir().unwrap();
        let (service_addr, hits, _service) = service(Service::WebSocket).await;
        let (server, addr) = server(root.path(), vec![target_of(service_addr)]).await;
        let mut control = session(&server, addr, vec![RemoteCapability::PortForward]).await;
        let grant = grant_for(&mut control, &target_of(service_addr), 2).await;

        let mut tunnel = ForwardTunnel::bind(
            endpoint(vec![RemoteCapability::PortForward]),
            grant,
            forward_config(),
            "127.0.0.1:0".parse().unwrap(),
        )
        .await
        .unwrap();
        let local = tunnel.local_addr().unwrap();
        let mut dial = dialer(addr);
        let carried = tokio::spawn(async move { tunnel.serve_one(&mut dial).await });

        let (mut socket, response) = tokio_tungstenite::connect_async(format!("ws://{local}/ws"))
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 101, "not an upgrade");
        use tokio_tungstenite::tungstenite::Message;
        socket
            .send(Message::Text("through the tunnel".into()))
            .await
            .unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            Message::Text("through the tunnel".into()),
            "the echo came from the service behind the tunnel"
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        let _ = socket.close(None).await;
        // The close frame ends the WebSocket; the socket is what the pump is
        // waiting on, and that goes when the stream goes.
        drop(socket);
        assert!(carried.await.unwrap().is_ok());
    }

    /// A grant for somewhere the operator did not list is refused *as the answer to
    /// the request*, before a listener exists — so nothing can be told it is
    /// forwarding when it is not.
    #[tokio::test]
    async fn a_target_the_operator_did_not_list_is_refused_before_anything_listens() {
        let root = tempfile::tempdir().unwrap();
        let (service_addr, hits, _service) = service(Service::Fixed(b"nope")).await;
        let (server, addr) = server(root.path(), vec![]).await;

        let mut control = session(&server, addr, vec![RemoteCapability::PortForward]).await;
        let err = control
            .forward_grant(&target_of(service_addr), Duration::from_secs(60), 4)
            .await
            .expect_err("nothing is allowlisted");
        match err {
            RemoteError::ForwardDenied { reason } => {
                assert!(
                    reason.contains("not somewhere this server will connect to"),
                    "{reason}"
                );
                assert!(reason.contains("there are none"), "{reason}");
            }
            other => panic!("expected a forward refusal, got {other:?}"),
        }
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "the target was never touched"
        );
    }

    /// Port 0 is how you ask for a free port; a busy port is named, and the refusal
    /// says what to do about it rather than quietly moving.
    #[tokio::test]
    async fn a_busy_local_port_is_named_instead_of_quietly_moved() {
        let root = tempfile::tempdir().unwrap();
        let (service_addr, _hits, _service) = service(Service::Fixed(b"x")).await;
        let (server, addr) = server(root.path(), vec![target_of(service_addr)]).await;
        let mut control = session(&server, addr, vec![RemoteCapability::PortForward]).await;
        let grant = grant_for(&mut control, &target_of(service_addr), 4).await;

        let taken = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let busy = taken.local_addr().unwrap();
        let err = ForwardTunnel::bind(
            endpoint(vec![RemoteCapability::PortForward]),
            grant.clone(),
            forward_config(),
            busy,
        )
        .await
        .expect_err("the port is already held");
        let text = err.to_string();
        assert!(text.contains(&busy.port().to_string()), "{text}");
        assert!(text.contains("port 0"), "{text}");

        let tunnel = ForwardTunnel::bind(
            endpoint(vec![RemoteCapability::PortForward]),
            grant,
            forward_config(),
            "127.0.0.1:0".parse().unwrap(),
        )
        .await
        .unwrap();
        assert_ne!(tunnel.local_addr().unwrap(), busy);
    }

    /// The local end listens on loopback and nowhere else: putting a forwarded
    /// service on a routable interface is what publishing it means.
    #[tokio::test]
    async fn a_local_end_on_a_routable_address_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let (service_addr, _hits, _service) = service(Service::Fixed(b"x")).await;
        let (server, addr) = server(root.path(), vec![target_of(service_addr)]).await;
        let mut control = session(&server, addr, vec![RemoteCapability::PortForward]).await;
        let grant = grant_for(&mut control, &target_of(service_addr), 2).await;

        let err = ForwardTunnel::bind(
            endpoint(vec![RemoteCapability::PortForward]),
            grant,
            forward_config(),
            "0.0.0.0:0".parse().unwrap(),
        )
        .await
        .expect_err("0.0.0.0 is not a local end");
        assert!(err.to_string().contains("loopback"), "{err}");
        assert!(parse_listen_endpoint("0.0.0.0:8080").is_err());
    }

    /// The budget the server agreed to is what ends the tunnel, and ending it
    /// releases the port — both halves of the lifecycle promise at once.
    #[tokio::test]
    async fn a_grant_that_runs_out_closes_the_listener() {
        let root = tempfile::tempdir().unwrap();
        let (service_addr, hits, _service) = service(Service::Fixed(b"answer\n")).await;
        let (server, addr) = server(root.path(), vec![target_of(service_addr)]).await;
        let mut control = session(&server, addr, vec![RemoteCapability::PortForward]).await;
        let grant = grant_for(&mut control, &target_of(service_addr), 1).await;

        let mut tunnel = ForwardTunnel::bind(
            endpoint(vec![RemoteCapability::PortForward]),
            grant,
            forward_config(),
            "127.0.0.1:0".parse().unwrap(),
        )
        .await
        .unwrap();
        let local = tunnel.local_addr().unwrap();
        let serving = tokio::spawn(async move {
            let mut dial = dialer(addr);
            tunnel.serve(&mut dial).await
        });

        let mut client = TcpStream::connect(local).await.unwrap();
        client.write_all(b"one\n").await.unwrap();
        assert_eq!(drain(client).await, b"answer\n");

        let stats = serving.await.unwrap().unwrap();
        assert_eq!(
            stats,
            ForwardStats {
                connections: 1,
                refused: 0,
                bytes_to_target: 4,
                bytes_to_client: 7,
            }
        );
        // The grant is spent, the tunnel is gone, and so is the port.
        assert!(
            TcpListener::bind(local).await.is_ok(),
            "the local port was still held after the grant ran out"
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    /// A forward ticket buys its forward and nothing else.
    ///
    /// It goes in the same field as a session credential, so this is the boundary
    /// that matters: the server answers the hello of a ticket-holding connection
    /// with no workspace capability at all, and refuses a workspace request on it by
    /// name. Driven with hand-written frames, because the typed client would refuse
    /// the request locally and the thing worth proving is what the server answers.
    #[tokio::test]
    async fn a_forward_ticket_is_not_a_session_credential() {
        use crate::remote::protocol::{
            Envelope, Implementation, MAX_FRAME_BYTES, Payload, Reply, Request, VersionRange, recv,
            send,
        };

        let root = tempfile::tempdir().unwrap();
        let (service_addr, _hits, _service) = service(Service::Fixed(b"x")).await;
        let (server, addr) = server(root.path(), vec![target_of(service_addr)]).await;
        let mut control = session(&server, addr, vec![RemoteCapability::PortForward]).await;
        let grant = grant_for(&mut control, &target_of(service_addr), 2).await;

        let mut stream = TcpStream::connect(addr).await.unwrap();
        send(
            &mut stream,
            &Envelope {
                id: 1,
                body: Request::Hello {
                    protocol: VersionRange::current(),
                    client: Implementation::local(),
                    token: grant.ticket.as_str().to_string(),
                    capabilities: vec![RemoteCapability::WorkspaceRead],
                },
            },
        )
        .await
        .unwrap();
        let hello: Envelope<Reply> = recv(&mut stream, MAX_FRAME_BYTES).await.unwrap().unwrap();
        let Ok(Payload::Hello {
            capabilities: granted,
            ..
        }) = hello.body
        else {
            panic!(
                "a ticket should still open its own connection: {:?}",
                hello.body
            );
        };
        assert!(
            !granted.contains(&RemoteCapability::WorkspaceRead),
            "a forward connection was granted workspace access: {granted:?}"
        );

        send(
            &mut stream,
            &Envelope {
                id: 2,
                body: Request::List {
                    path: None,
                    depth: None,
                    max_entries: None,
                },
            },
        )
        .await
        .unwrap();
        let reply: Envelope<Reply> = recv(&mut stream, MAX_FRAME_BYTES).await.unwrap().unwrap();
        match reply.body {
            Err(RemoteError::Unauthorized { reason }) => {
                assert!(reason.contains("not a workspace session"), "{reason}");
            }
            other => panic!("the workspace should be out of reach: {other:?}"),
        }
    }

    /// The other direction: a session credential carries no target, so a session
    /// that asks to open a forward is refused by the server even though it holds the
    /// capability.
    #[tokio::test]
    async fn a_session_credential_cannot_open_a_forward() {
        let root = tempfile::tempdir().unwrap();
        let (service_addr, hits, _service) = service(Service::Fixed(b"x")).await;
        let (server, addr) = server(root.path(), vec![target_of(service_addr)]).await;
        let control = session(&server, addr, vec![RemoteCapability::PortForward]).await;

        let err = control
            .into_forward_stream()
            .await
            .expect_err("a session credential does not name a target");
        match err {
            RemoteError::ForwardDenied { reason } => {
                assert!(reason.contains("workspace session"), "{reason}");
            }
            other => panic!("expected a forward refusal, got {other:?}"),
        }
        assert_eq!(hits.load(Ordering::SeqCst), 0);
    }

    /// A session that was never granted the capability is refused locally, without
    /// spending a round trip on the question.
    #[tokio::test]
    async fn a_session_without_the_capability_is_refused_locally() {
        let root = tempfile::tempdir().unwrap();
        let (service_addr, _hits, _service) = service(Service::Fixed(b"x")).await;
        let (server, addr) = server(root.path(), vec![target_of(service_addr)]).await;
        let mut control = session(&server, addr, vec![RemoteCapability::WorkspaceRead]).await;

        let err = control
            .forward_grant(&target_of(service_addr), Duration::from_secs(60), 1)
            .await
            .expect_err("read is not forward");
        assert!(
            matches!(err, RemoteError::CapabilityNotGranted { .. }),
            "{err:?}"
        );
    }

    /// Closing the session takes the forward with it — the server-side half of
    /// "close the session and the port is released".
    #[tokio::test]
    async fn closing_the_session_closes_the_forward() {
        let root = tempfile::tempdir().unwrap();
        let (service_addr, _hits, _service) = service(Service::Fixed(b"x")).await;
        let (server, addr) = server(root.path(), vec![target_of(service_addr)]).await;
        let mut control = session(&server, addr, vec![RemoteCapability::PortForward]).await;
        let grant = grant_for(&mut control, &target_of(service_addr), 4).await;
        control.close().await;

        let stream = TcpStream::connect(addr).await.unwrap();
        let opened = RemoteWorkspace::connect(
            stream,
            &endpoint(vec![RemoteCapability::PortForward]),
            &SessionToken::from_forward_ticket(&grant.ticket),
            forward_config(),
        )
        .await;
        match opened {
            Err(RemoteError::ForwardDenied { reason }) => {
                assert!(reason.contains("closed"), "{reason}");
            }
            other => panic!("the forward should have closed with its session: {other:?}"),
        }
    }

    /// The client's side of the same promise: dropping the tunnel gives the port
    /// back, so the next thing that wants it can have it.
    #[tokio::test]
    async fn a_dropped_tunnel_gives_its_port_back() {
        let root = tempfile::tempdir().unwrap();
        let (service_addr, _hits, _service) = service(Service::Fixed(b"x")).await;
        let (server, addr) = server(root.path(), vec![target_of(service_addr)]).await;
        let mut control = session(&server, addr, vec![RemoteCapability::PortForward]).await;
        let grant = grant_for(&mut control, &target_of(service_addr), 4).await;

        let tunnel = ForwardTunnel::bind(
            endpoint(vec![RemoteCapability::PortForward]),
            grant,
            forward_config(),
            "127.0.0.1:0".parse().unwrap(),
        )
        .await
        .unwrap();
        let port = tunnel.local_addr().unwrap().port();
        drop(tunnel);
        let rebound = TcpListener::bind(("127.0.0.1", port)).await;
        assert!(rebound.is_ok(), "the port was not released: {rebound:?}");
    }

    /// A tunnel that never carried anything is a failure and has to say why, rather
    /// than reporting a clean zero.
    #[tokio::test]
    async fn a_tunnel_that_never_carries_anything_reports_the_reason() {
        let root = tempfile::tempdir().unwrap();
        // Nothing listening at the target: the grant is legal, the connect is not.
        let dead = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let dead_addr = dead.local_addr().unwrap();
        drop(dead);
        let (server, addr) = server(root.path(), vec![target_of(dead_addr)]).await;
        let mut control = session(&server, addr, vec![RemoteCapability::PortForward]).await;
        let grant = control
            .forward_grant(&target_of(dead_addr), Duration::from_secs(5), 4)
            .await
            .unwrap();

        let mut tunnel = ForwardTunnel::bind(
            endpoint(vec![RemoteCapability::PortForward]),
            grant,
            forward_config(),
            "127.0.0.1:0".parse().unwrap(),
        )
        .await
        .unwrap();
        let local = tunnel.local_addr().unwrap();
        let serving = tokio::spawn(async move {
            let mut dial = dialer(addr);
            tunnel.serve(&mut dial).await
        });
        let client = TcpStream::connect(local).await.unwrap();
        drop(client);
        let err = serving
            .await
            .unwrap()
            .expect_err("nothing was ever carried");
        assert!(
            err.to_string().contains("did not accept a connection"),
            "{err}"
        );
    }

    /// The tunnel checks its own preconditions rather than waiting to be refused.
    #[tokio::test]
    async fn a_tunnel_without_the_capability_on_both_sides_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let (service_addr, _hits, _service) = service(Service::Fixed(b"x")).await;
        let (server, addr) = server(root.path(), vec![target_of(service_addr)]).await;
        let mut control = session(&server, addr, vec![RemoteCapability::PortForward]).await;
        let grant = grant_for(&mut control, &target_of(service_addr), 2).await;

        let endpoint_without_forward = endpoint(vec![RemoteCapability::WorkspaceRead]);
        let err = ForwardTunnel::bind(
            endpoint_without_forward,
            grant.clone(),
            forward_config(),
            "127.0.0.1:0".parse().unwrap(),
        )
        .await
        .expect_err("the endpoint is the user's decision");
        assert!(
            err.to_string()
                .contains("not configured to allow port forwarding")
        );

        let err = ForwardTunnel::bind(
            endpoint(vec![RemoteCapability::PortForward]),
            grant,
            RemoteWorkspaceConfig::new().capabilities(vec![RemoteCapability::WorkspaceRead]),
            "127.0.0.1:0".parse().unwrap(),
        )
        .await
        .expect_err("a session that never asked cannot forward");
        assert!(err.to_string().contains("never asked"), "{err}");
    }

    #[test]
    fn a_local_end_can_be_asked_for_by_port_alone() {
        assert_eq!(
            parse_listen_endpoint("8080").unwrap(),
            "127.0.0.1:8080".parse().unwrap()
        );
        assert_eq!(
            parse_listen_endpoint(":0").unwrap(),
            "127.0.0.1:0".parse().unwrap()
        );
        assert_eq!(
            parse_listen_endpoint("[::1]:8080").unwrap(),
            "[::1]:8080".parse().unwrap()
        );
        assert!(parse_listen_endpoint("buildbox:8080").is_err());
        assert!(parse_listen_endpoint("").is_err());
    }

    #[test]
    fn a_target_is_a_host_and_a_port() {
        let target = parse_forward_target("db.internal:5432").unwrap();
        assert_eq!(target.host, "db.internal");
        assert_eq!(target.port, 5432);
        let v6 = parse_forward_target("[::1]:8080").unwrap();
        assert_eq!(v6.host, "::1");
        assert_eq!(v6.port, 8080);
        // A bare IPv6 address without brackets is ambiguous about where the port
        // starts, so it stays an error rather than becoming a guess.
        assert!(parse_forward_target("::1").is_err());
        assert!(parse_forward_target("db.internal").is_err());
        assert!(parse_forward_target("db.internal:0").is_err());
        assert!(parse_forward_target(":5432").is_err());
    }
}
