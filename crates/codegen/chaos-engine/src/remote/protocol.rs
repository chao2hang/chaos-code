//! The wire protocol: framing, messages, and the version negotiation.
//!
//! Length-prefixed JSON over any bidirectional stream. A stream rather than
//! HTTP because the transport is chosen by whoever tunnels us (a Unix socket,
//! a stdio pipe, an SSH-forwarded loopback port) and the protocol should not
//! care which; JSON because a workspace session is dominated by text and the
//! cost of parsing is nothing next to reading the file.
//!
//! Two rules keep the framing honest. A frame is capped, so a peer — or a
//! byte-stream desync — cannot ask for a 4 GiB allocation before saying hello.
//! And every request carries an id that the reply repeats, so a client that
//! sees an answer to a different question knows the session has desynced
//! instead of quietly returning the wrong file's contents.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::endpoint::RemoteCapability;

/// The protocol this build of the server speaks at its newest.
///
/// Bump only for a change an *older* peer could not ignore — a new optional
/// field is not a version bump, a changed meaning is.
pub const PROTOCOL_VERSION: u32 = 1;

/// The oldest protocol this build will still agree to speak.
///
/// One for now: there is no older build in the wild. It exists so that the
/// negotiation below is a range test rather than an equality test, which is what
/// makes the downgrade hint meaningful the first time the two sides diverge.
pub const MIN_PROTOCOL_VERSION: u32 = 1;

/// The largest frame either side will read.
///
/// Generous for source files and diffs, small enough that a garbled length
/// prefix costs a rejected connection rather than an out-of-memory kill.
pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;

/// The inclusive range of protocol versions one side accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionRange {
    pub min: u32,
    pub max: u32,
}

impl VersionRange {
    pub fn current() -> Self {
        Self {
            min: MIN_PROTOCOL_VERSION,
            max: PROTOCOL_VERSION,
        }
    }
}

/// Why the two sides have no protocol in common, with what to do about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionMismatch {
    pub client: VersionRange,
    pub server: VersionRange,
    pub hint: String,
}

impl std::fmt::Display for VersionMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "no protocol version in common: client speaks {}, server speaks {}; {}",
            range_text(&self.client),
            range_text(&self.server),
            self.hint
        )
    }
}

impl std::error::Error for VersionMismatch {}

fn range_text(range: &VersionRange) -> String {
    if range.min == range.max {
        range.max.to_string()
    } else {
        format!("{}-{}", range.min, range.max)
    }
}

/// Pick the newest protocol both sides speak, or explain the gap.
///
/// The hint names the direction. "Upgrade" is not useful advice when the *server*
/// is the old one and the operator has just deployed a newer client; the useful
/// sentence is "this server is behind, redeploy it" — which is exactly the case a
/// stale installed artifact produces.
pub fn negotiate(client: VersionRange, server: VersionRange) -> Result<u32, VersionMismatch> {
    if client.max >= server.min && server.max >= client.min {
        return Ok(client.max.min(server.max));
    }
    let hint = if client.max < server.min {
        format!(
            "the server is ahead: it needs protocol {} at least. Redeploy the server \
             from a build matching the client, or update the client.",
            server.min
        )
    } else {
        format!(
            "the server is behind: it speaks at most protocol {}. Redeploy the server \
             from a newer build; the client can still speak {} and will do so once the \
             server offers it.",
            server.max, client.min
        )
    };
    Err(VersionMismatch {
        client,
        server,
        hint,
    })
}

/// Who is on the other end, for logs and for the mismatch message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Implementation {
    pub name: String,
    pub version: String,
}

impl Implementation {
    /// The build this code is part of.
    pub fn local() -> Self {
        Self {
            name: "chaos-remote-server".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

impl std::fmt::Display for Implementation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.name, self.version)
    }
}

/// A request from the side that opened the session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "method")]
pub enum Request {
    /// First frame, and the only way to become authorised.
    Hello {
        protocol: VersionRange,
        client: Implementation,
        /// One-time credential from the server's token file.
        token: String,
        /// What this session wants to use. Anything outside what the server
        /// offers is refused per call rather than silently downgraded.
        capabilities: Vec<RemoteCapability>,
    },
    /// List the workspace, depth-bounded.
    List {
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        depth: Option<usize>,
        #[serde(default)]
        max_entries: Option<usize>,
    },
    /// Read bytes, with an optional window — the Range read the file tree needs.
    Read {
        path: String,
        #[serde(default)]
        offset: Option<u64>,
        #[serde(default)]
        len: Option<u64>,
    },
    /// Substring search across text files under `path` or the whole workspace.
    Search {
        query: String,
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        max_results: Option<usize>,
        #[serde(default)]
        case_sensitive: bool,
    },
    /// Replace a file's contents.
    Write {
        path: String,
        content_b64: String,
        #[serde(default)]
        create_parents: bool,
        /// Refuse if the file on disk no longer has this hash: the edit was
        /// made against a version that is gone.
        #[serde(default)]
        expected_sha256: Option<String>,
    },
    /// `git diff` for the workspace.
    Diff {
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        staged: bool,
        #[serde(default)]
        context: Option<u32>,
    },
    /// Run one program, without a shell.
    Exec {
        argv: Vec<String>,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default)]
        timeout_ms: Option<u64>,
    },
    /// Begin streaming a server artifact to the host.
    InstallBegin {
        version: String,
        sha256: String,
        total_bytes: u64,
    },
    /// One chunk of that artifact. `seq` is checked so a dropped or reordered
    /// frame corrupts the install loudly instead of silently.
    InstallChunk { seq: u64, data_b64: String },
    /// Verify the digest, install atomically, and say what is current now.
    InstallFinish { version: String },
    /// Cheap liveness probe that costs nothing.
    Ping,
}

/// One file or directory in a listing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Workspace-relative, `/`-separated — the same form a request names.
    pub path: String,
    pub is_dir: bool,
    pub len: u64,
}

/// One matching line of one file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHit {
    pub path: String,
    /// 1-based, so it can be pasted into an editor.
    pub line: usize,
    pub text: String,
}

/// What a successful request produced.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Payload {
    Hello {
        protocol_version: u32,
        server: Implementation,
        /// What the session may actually use: offered by the server, and no
        /// broader than what the client asked for.
        capabilities: Vec<RemoteCapability>,
        session_id: String,
        /// What one read or one write may carry. Said at handshake rather than
        /// discovered by a large file being refused halfway through a transfer.
        max_transfer_bytes: u64,
    },
    List {
        entries: Vec<Entry>,
        truncated: bool,
    },
    Read {
        data_b64: String,
        /// Total size of the file, so a caller can page without a second stat.
        total_len: u64,
        offset: u64,
        sha256: String,
    },
    Search {
        hits: Vec<SearchHit>,
        truncated: bool,
        files_scanned: usize,
    },
    Write {
        bytes_written: u64,
        sha256: String,
    },
    Diff {
        diff: String,
        /// False means the tree matches HEAD.
        dirty: bool,
    },
    Exec {
        /// `None` when the process was killed rather than exiting.
        exit_code: Option<i32>,
        stdout: String,
        stderr: String,
        timed_out: bool,
        /// Output past the cap is dropped and this is set.
        truncated: bool,
    },
    InstallBegin {
        received_bytes: u64,
    },
    InstallChunk {
        received_bytes: u64,
    },
    InstallFinish {
        version: String,
        current: bool,
        previous_version: Option<String>,
    },
    Pong {
        server: Implementation,
        protocol_version: u32,
    },
}

/// Why a request did not happen.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "error")]
pub enum RemoteError {
    /// The handshake never completed, or was not first.
    Unauthorized { reason: String },
    /// The credential was valid once and has been spent.
    TokenReused { reason: String },
    /// The credential is past its lifetime.
    TokenExpired { reason: String },
    /// No protocol version both sides accept.
    VersionMismatch {
        server_version: u32,
        supported_min: u32,
        supported_max: u32,
        hint: String,
    },
    /// The call needs something this session was not granted.
    CapabilityNotGranted { capability: RemoteCapability },
    /// The path names something outside the workspace.
    PathRejected { reason: String },
    /// The bytes are not there.
    NotFound { path: String },
    /// The request itself is not sensible.
    InvalidRequest { reason: String },
    /// The file on disk is not the one the caller meant to replace.
    Conflict {
        expected_sha256: String,
        actual_sha256: String,
    },
    /// A `git` invocation could not be run or did not succeed.
    Git { reason: String },
    /// A program could not be run.
    Exec { reason: String },
    /// The artifact stream or the install step failed; what is current did not
    /// change, or was put back.
    Install { reason: String, rolled_back: bool },
    /// The stream is not speakable as this protocol.
    Protocol { reason: String },
    /// A reply did not arrive within the caller's deadline. The session is over:
    /// the peer may still be working on that request, and a late answer would be
    /// read as the answer to the next one.
    Timeout { request: String, after: Duration },
    /// This call was never sent, because an earlier one left the session
    /// abandoned. Continuing would desync request ids, so the way forward is a
    /// new session — which needs a credential the server has not handed out yet.
    Abandoned { reason: String },
    /// Anything the host refused to explain further.
    Io { context: String, reason: String },
}

impl std::fmt::Display for RemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized { reason } => write!(f, "unauthorised: {reason}"),
            Self::TokenReused { reason } => write!(f, "token already used: {reason}"),
            Self::TokenExpired { reason } => write!(f, "token expired: {reason}"),
            Self::VersionMismatch {
                server_version,
                supported_min,
                supported_max,
                hint,
            } => write!(
                f,
                "server speaks protocol {server_version} (accepts {supported_min}\
                 -{supported_max}); {hint}"
            ),
            Self::CapabilityNotGranted { capability } => {
                write!(f, "capability not granted to this session: {capability:?}")
            }
            Self::PathRejected { reason } => write!(f, "path rejected: {reason}"),
            Self::NotFound { path } => write!(f, "not found: {path}"),
            Self::InvalidRequest { reason } => write!(f, "invalid request: {reason}"),
            Self::Conflict {
                expected_sha256,
                actual_sha256,
            } => write!(
                f,
                "the file changed since it was read (wanted {expected_sha256}, \
                 found {actual_sha256})"
            ),
            Self::Git { reason } => write!(f, "git: {reason}"),
            Self::Exec { reason } => write!(f, "exec: {reason}"),
            Self::Install {
                reason,
                rolled_back,
            } => write!(
                f,
                "install failed{maybe}: {reason}",
                maybe = if *rolled_back {
                    " and was rolled back"
                } else {
                    ""
                }
            ),
            Self::Protocol { reason } => write!(f, "protocol error: {reason}"),
            Self::Timeout { request, after } => write!(
                f,
                "no reply to {request} after {} \
                 (the session is abandoned; a reply that arrives later would answer \
                 the wrong question)",
                seconds(*after)
            ),
            Self::Abandoned { reason } => write!(
                f,
                "session abandoned, nothing sent: {reason} (a credential is one-time, \
                 so reconnecting means a new session and a new credential)"
            ),
            Self::Io { context, reason } => write!(f, "{context}: {reason}"),
        }
    }
}

/// A duration in the shortest form worth reading in an error line.
fn seconds(elapsed: Duration) -> String {
    let millis = elapsed.as_millis();
    if millis < 1000 {
        return format!("{millis}ms");
    }
    format!("{}s", millis as f64 / 1000.0)
}

impl std::error::Error for RemoteError {}

/// A request or a reply, with the id it answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope<T> {
    pub id: u64,
    pub body: T,
}

/// The reply to one request.
pub type Reply = Result<Payload, RemoteError>;

/// Write one frame: a big-endian `u32` length, then the payload.
pub async fn write_frame<W>(writer: &mut W, bytes: &[u8]) -> Result<(), RemoteError>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;
    let len = u32::try_from(bytes.len()).map_err(|_| RemoteError::Protocol {
        reason: format!("frame of {} bytes cannot be length-prefixed", bytes.len()),
    })?;
    writer
        .write_all(&len.to_be_bytes())
        .await
        .map_err(|e| RemoteError::Io {
            context: "write frame length".into(),
            reason: e.to_string(),
        })?;
    writer.write_all(bytes).await.map_err(|e| RemoteError::Io {
        context: "write frame body".into(),
        reason: e.to_string(),
    })?;
    writer.flush().await.map_err(|e| RemoteError::Io {
        context: "flush frame".into(),
        reason: e.to_string(),
    })
}

/// Read one frame, refusing anything over [`MAX_FRAME_BYTES`].
pub async fn read_frame<R>(reader: &mut R, max_frame: usize) -> Result<Option<Vec<u8>>, RemoteError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    let mut header = [0u8; 4];
    match reader.read_exact(&mut header).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => {
            return Err(RemoteError::Io {
                context: "read frame length".into(),
                reason: e.to_string(),
            });
        }
    }
    let len = u32::from_be_bytes(header) as usize;
    if len > max_frame {
        return Err(RemoteError::Protocol {
            reason: format!("peer offered a {len}-byte frame; the limit is {max_frame}"),
        });
    }
    let mut body = vec![0u8; len];
    reader
        .read_exact(&mut body)
        .await
        .map_err(|e| RemoteError::Io {
            context: "read frame body".into(),
            reason: e.to_string(),
        })?;
    Ok(Some(body))
}

/// Serialize and frame a value.
pub async fn send<T, W>(writer: &mut W, value: &T) -> Result<(), RemoteError>
where
    T: Serialize,
    W: tokio::io::AsyncWrite + Unpin,
{
    let bytes = serde_json::to_vec(value).map_err(|e| RemoteError::Protocol {
        reason: format!("serialise: {e}"),
    })?;
    write_frame(writer, &bytes).await
}

/// Read and parse a frame; `Ok(None)` is a clean end of stream.
pub async fn recv<T, R>(reader: &mut R, max_frame: usize) -> Result<Option<T>, RemoteError>
where
    T: serde::de::DeserializeOwned,
    R: tokio::io::AsyncRead + Unpin,
{
    let Some(bytes) = read_frame(reader, max_frame).await? else {
        return Ok(None);
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| RemoteError::Protocol {
            reason: format!("parse frame: {e}"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(min: u32, max: u32) -> VersionRange {
        VersionRange { min, max }
    }

    #[test]
    fn the_newest_version_both_sides_speak_wins() {
        assert_eq!(negotiate(range(1, 2), range(1, 3)), Ok(2));
        assert_eq!(negotiate(range(1, 5), range(2, 3)), Ok(3));
        assert_eq!(negotiate(range(1, 1), range(1, 1)), Ok(1));
    }

    /// Overlap at a single version is still a working session.
    #[test]
    fn a_single_shared_version_is_enough() {
        assert_eq!(negotiate(range(1, 2), range(2, 4)), Ok(2));
        assert_eq!(negotiate(range(3, 9), range(1, 3)), Ok(3));
    }

    #[test]
    fn disjoint_ranges_are_refused_in_both_directions() {
        let stale_client = negotiate(range(1, 1), range(2, 3)).expect_err("no overlap");
        assert!(
            stale_client.hint.contains("server is ahead"),
            "{}",
            stale_client.hint
        );
        let stale_server = negotiate(range(4, 5), range(1, 2)).expect_err("no overlap");
        assert!(
            stale_server.hint.contains("server is behind"),
            "{}",
            stale_server.hint
        );
    }

    /// The hint is the whole point of the failure: it has to name which side to
    /// redeploy, because "version mismatch" alone sends an operator to the wrong
    /// machine half the time.
    #[test]
    fn the_mismatch_message_names_the_side_to_fix() {
        let err = negotiate(range(1, 1), range(2, 3)).expect_err("no overlap");
        let text = err.to_string();
        assert!(text.contains("client speaks 1"), "{text}");
        assert!(text.contains("server speaks 2-3"), "{text}");
        assert!(text.contains("Redeploy the server"), "{text}");
    }

    #[tokio::test]
    async fn a_frame_survives_the_round_trip() {
        let (mut a, mut b) = tokio::io::duplex(4096);
        let request = Request::Read {
            path: "src/main.rs".into(),
            offset: Some(3),
            len: Some(9),
        };
        send(
            &mut a,
            &Envelope {
                id: 7,
                body: &request,
            },
        )
        .await
        .unwrap();
        let got: Envelope<Request> = recv(&mut b, MAX_FRAME_BYTES).await.unwrap().unwrap();
        assert_eq!(got.id, 7);
        assert_eq!(got.body, request);
    }

    #[tokio::test]
    async fn a_clean_end_of_stream_is_not_an_error() {
        let (a, mut b) = tokio::io::duplex(64);
        drop(a);
        assert!(
            recv::<Envelope<Request>, _>(&mut b, MAX_FRAME_BYTES)
                .await
                .is_ok()
        );
        assert!(
            recv::<Envelope<Request>, _>(&mut b, MAX_FRAME_BYTES)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn an_oversized_frame_is_refused_before_it_is_allocated() {
        use tokio::io::AsyncWriteExt;
        let (mut a, mut b) = tokio::io::duplex(64);
        let handle = tokio::spawn(async move {
            // Announce 1 GiB and never send it. A reader that trusted the
            // length would allocate before knowing anything about the peer.
            a.write_all(&(1024u32 * 1024 * 1024).to_be_bytes())
                .await
                .unwrap();
            a.flush().await.unwrap();
        });
        let err = read_frame(&mut b, MAX_FRAME_BYTES)
            .await
            .expect_err("must refuse");
        assert!(matches!(err, RemoteError::Protocol { .. }), "{err:?}");
        handle.abort();
    }

    /// Requests are named by their `method` tag and payloads by `kind`, so a
    /// capture of traffic reads as `{"method":"read",…}` rather than nesting
    /// every variant under its own name.
    #[test]
    fn the_wire_shape_is_stable() {
        let request = serde_json::to_value(Request::Ping).unwrap();
        assert_eq!(request, serde_json::json!({"method": "ping"}));
        let reply = serde_json::to_value(Payload::Write {
            bytes_written: 3,
            sha256: "ab".into(),
        })
        .unwrap();
        assert_eq!(
            reply,
            serde_json::json!({"kind": "write", "bytes_written": 3, "sha256": "ab"})
        );
    }

    #[test]
    fn a_missing_optional_field_reads_as_none() {
        let request: Request = serde_json::from_str(r#"{"method":"read","path":"a.txt"}"#).unwrap();
        assert_eq!(
            request,
            Request::Read {
                path: "a.txt".into(),
                offset: None,
                len: None
            }
        );
    }
}
