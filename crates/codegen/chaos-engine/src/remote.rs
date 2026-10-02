//! The remote workspace topology: what may be asked of a remote host, and how
//! the asking works.
//!
//! Split by question rather than by side, because most of the answers apply to
//! both ends of a connection:
//! - [`endpoint`]: what a configured remote has promised, and the host-key
//!   policy it has to satisfy first;
//! - [`protocol`]: the frames on the wire, and how the two sides agree on which
//!   version of them they are speaking;
//! - [`path`]: a path that means "inside that workspace", kept a distinct type so
//!   it cannot be opened on the wrong machine;
//! - [`credentials`]: the one-time session credential and its expiry;
//! - `server` and `client`: the two ends;
//! - [`install`]: putting a server build onto its host, and taking it back off;
//! - [`provenance`]: whether the build being installed came from a key the host
//!   trusts, which is a different question from whether it arrived whole;
//! - [`artifact_format`]: which platform the build being installed says it is for,
//!   since a whole and signed build for another CPU still cannot start here.
//!
//! The topology itself is the one ADR-004 allows: a local agent driving a remote
//! workspace. Reasoning about a detached agent — an agent that keeps running with
//! nobody attached — is explicitly out of scope, and [`RemoteEndpoint::validate`]
//! refuses to advertise one so the gap cannot be closed by a typo in a config
//! file.

pub mod artifact_format;
pub mod client;
pub mod credentials;
pub mod endpoint;
pub mod forward;
pub mod install;
pub mod path;
pub mod protocol;
pub mod provenance;
pub mod server;

pub use client::{
    DEFAULT_HANDSHAKE_TIMEOUT, DialRetry, ExecOutcome, FileContents, ForwardGrant, ForwardPipe,
    InstallOutcome, ReadWindow, RemoteWorkspace, RemoteWorkspaceConfig, SearchOutcome,
    SessionState, WriteFile,
};
pub use credentials::{
    ForwardLimits, ForwardTarget, ForwardTicket, ForwardTicketError, ForwardVault, RedeemError,
    SessionToken, TokenFile, TokenVault,
};
pub use endpoint::{HostKeyPolicy, RemoteCapability, RemoteEndpoint, parse_capability};
pub use forward::{ForwardStats, ForwardTunnel, parse_forward_target, parse_listen_endpoint};
pub use install::{ARTIFACT_NAME, CommitOutcome, DEFAULT_INSTALL_DIR, InstallLayout, sha256_hex};
pub use path::{PathRejection, RemotePath};
pub use protocol::{
    Entry, Implementation, MAX_FRAME_BYTES, PROTOCOL_VERSION, Payload, RemoteError, Reply, Request,
    SearchHit, VersionMismatch, VersionRange, negotiate,
};
pub use provenance::{
    MAX_VERIFIED_ARTIFACT_BYTES, ProvenancePolicy, REQUIRE_SIGNATURE_ENV, SIGNING_PUBLIC_KEY_ENV,
    requirement_requested_by_env,
};
pub use server::{Server, ServerConfig, parse_loopback_host, require_loopback};
