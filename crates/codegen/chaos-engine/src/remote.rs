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
//! - [`install`]: putting a server build onto its host, and taking it back off.
//!
//! The topology itself is the one ADR-004 allows: a local agent driving a remote
//! workspace. Reasoning about a detached agent — an agent that keeps running with
//! nobody attached — is explicitly out of scope, and [`RemoteEndpoint::validate`]
//! refuses to advertise one so the gap cannot be closed by a typo in a config
//! file.

pub mod client;
pub mod credentials;
pub mod endpoint;
pub mod install;
pub mod path;
pub mod protocol;
pub mod server;

pub use client::{
    DEFAULT_HANDSHAKE_TIMEOUT, DialRetry, ExecOutcome, FileContents, InstallOutcome, ReadWindow,
    RemoteWorkspace, RemoteWorkspaceConfig, SearchOutcome, SessionState, WriteFile,
};
pub use credentials::{RedeemError, SessionToken, TokenFile, TokenVault};
pub use endpoint::{HostKeyPolicy, RemoteCapability, RemoteEndpoint, parse_capability};
pub use install::{ARTIFACT_NAME, CommitOutcome, DEFAULT_INSTALL_DIR, InstallLayout, sha256_hex};
pub use path::{PathRejection, RemotePath};
pub use protocol::{
    Entry, Implementation, MAX_FRAME_BYTES, PROTOCOL_VERSION, Payload, RemoteError, Reply, Request,
    SearchHit, VersionMismatch, VersionRange, negotiate,
};
pub use server::{Server, ServerConfig, parse_loopback_host, require_loopback};
