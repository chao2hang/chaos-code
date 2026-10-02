//! What a configured remote has promised, and the host-key policy it has to
//! satisfy before a session may be attempted.
//!
//! This is the *client's* view of the remote — a statement about a host, made
//! before connecting and checked against what the host actually says it can do.
//! The server has its own list ([`super::server::ServerConfig`]); the two are
//! deliberately separate, because the useful rule is "the server may not offer
//! more than I agreed to", and that rule needs a second opinion to be worth
//! anything.

use serde::{Deserialize, Serialize};

/// Something a remote host can be asked to do.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RemoteCapability {
    WorkspaceList,
    WorkspaceRead,
    WorkspaceSearch,
    WorkspaceWrite,
    Git,
    ToolExecution,
    /// A port on the *client* machine that carries each connection to a target
    /// reachable from the server — SSH's local forwarding (`-L`), which is what
    /// "expose a service on the remote host to my laptop" actually means.
    PortForward,
    /// A port on the *server* machine that carries each connection back to a
    /// target reachable from the client — SSH's remote forwarding (`-R`).
    ///
    /// Named separately from [`RemoteCapability::PortForward`] because the trust
    /// question is opposite: a local forward asks "may I reach that service from
    /// there", a remote forward asks "may I make something reachable *to everyone
    /// who can reach this host*", which is a different decision and not one the
    /// workspace grant implies.
    RemoteForward,
    InteractivePty,
    DetachedAgent,
}

impl RemoteCapability {
    /// The capabilities a session can be granted by anything that exists today.
    ///
    /// The others are named by the protocol so that an endpoint can refuse them
    /// by name, not because something implements them; listing them here would
    /// advertise them.
    pub fn all() -> &'static [RemoteCapability] {
        &[
            RemoteCapability::WorkspaceList,
            RemoteCapability::WorkspaceRead,
            RemoteCapability::WorkspaceSearch,
            RemoteCapability::WorkspaceWrite,
            RemoteCapability::Git,
            RemoteCapability::ToolExecution,
            RemoteCapability::PortForward,
        ]
    }

    /// The name this capability is written as on a command line.
    pub fn as_str(&self) -> &'static str {
        match self {
            RemoteCapability::WorkspaceList => "list",
            RemoteCapability::WorkspaceRead => "read",
            RemoteCapability::WorkspaceSearch => "search",
            RemoteCapability::WorkspaceWrite => "write",
            RemoteCapability::Git => "git",
            RemoteCapability::ToolExecution => "tool-execution",
            RemoteCapability::PortForward => "port-forward",
            RemoteCapability::RemoteForward => "remote-forward",
            RemoteCapability::InteractivePty => "interactive-pty",
            RemoteCapability::DetachedAgent => "detached-agent",
        }
    }
}

/// The capability called `text`, or why it cannot be asked for.
///
/// A name with no implementation is refused with the reason rather than as
/// "unknown", because "unknown" sends the operator looking for a typo in a feature
/// that has not been built. Both the spelling this module uses and the wire form
/// (`snake_case`) are accepted, since a name typed once is usually typed from
/// whichever of the two was read last.
pub fn parse_capability(text: &str) -> Result<RemoteCapability, String> {
    if let Some(reason) = unavailable_capability(&text.replace('_', "-").to_ascii_lowercase()) {
        return Err(reason.to_string());
    }
    Ok(match text.replace('-', "_").to_ascii_lowercase().as_str() {
        "list" | "workspace_list" => RemoteCapability::WorkspaceList,
        "read" | "workspace_read" => RemoteCapability::WorkspaceRead,
        "search" | "workspace_search" => RemoteCapability::WorkspaceSearch,
        "write" | "workspace_write" => RemoteCapability::WorkspaceWrite,
        "git" => RemoteCapability::Git,
        "tool_execution" => RemoteCapability::ToolExecution,
        "port_forward" => RemoteCapability::PortForward,
        _ => {
            return Err(format!(
                "unknown capability {text:?}; expected {}",
                RemoteCapability::all()
                    .iter()
                    .map(|capability| capability.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    })
}

/// Why a name the protocol carries has nothing behind it.
fn unavailable_capability(name: &str) -> Option<&'static str> {
    match name {
        "interactive-pty" => Some("interactive PTY is not implemented by this build"),
        "remote-forward" => Some(
            "remote forwarding (a port on the remote host pointing back at this \
             machine) is not implemented: it publishes a service to whoever can reach \
             that host, which the workspace grant does not authorise. A local forward \
             (--capability port-forward) is the one that maps a remote service onto a \
             local port",
        ),
        "detached-agent" => Some(
            "detached agents are not part of the supported topology: the agent runs \
             locally and a remote host serves a workspace",
        ),
        _ => None,
    }
}

/// How the remote's identity is established.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum HostKeyPolicy {
    /// A key seen and pinned before; a mismatch is fatal.
    Strict,
    /// Accept on first contact, but only against a fingerprint the user checked.
    ///
    /// "Trust on first use" without a fingerprint to compare against is not a
    /// policy, it is a race in the attacker's favour, so the fingerprint is part
    /// of the variant rather than an optional field.
    TrustOnFirstUse { fingerprint: String },
}

/// A remote host the user has configured.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RemoteEndpoint {
    pub host: String,
    pub port: u16,
    pub host_key: HostKeyPolicy,
    pub capabilities: Vec<RemoteCapability>,
}

impl RemoteEndpoint {
    pub fn validate(&self) -> Result<(), String> {
        if self.host.trim().is_empty() {
            return Err("remote host is empty".into());
        }
        if self.port == 0 {
            return Err("remote port is invalid".into());
        }
        if let HostKeyPolicy::TrustOnFirstUse { fingerprint } = &self.host_key
            && fingerprint.trim().is_empty()
        {
            return Err("TOFU requires a host fingerprint".into());
        }
        if self.capabilities.contains(&RemoteCapability::DetachedAgent) {
            return Err("detached Agent is not supported by the current topology".into());
        }
        // Nothing implements it, and unlike the others it would publish something
        // on the remote host rather than reach out from it — so it is refused here
        // too, which is the path a hand-written config file takes.
        if self.capabilities.contains(&RemoteCapability::RemoteForward) {
            return Err(
                "remote forwarding is not implemented; port-forward maps a remote \
                 service onto a local port"
                    .into(),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_endpoint_requires_host_key_and_rejects_detached_agent() {
        let mut endpoint = RemoteEndpoint {
            host: "remote.example".into(),
            port: 22,
            host_key: HostKeyPolicy::TrustOnFirstUse {
                fingerprint: "sha256:abc".into(),
            },
            capabilities: vec![RemoteCapability::WorkspaceRead],
        };
        assert!(endpoint.validate().is_ok());
        endpoint.host_key = HostKeyPolicy::TrustOnFirstUse {
            fingerprint: String::new(),
        };
        assert!(endpoint.validate().is_err());
        endpoint.host_key = HostKeyPolicy::Strict;
        endpoint.capabilities.push(RemoteCapability::DetachedAgent);
        assert!(endpoint.validate().is_err());
    }

    #[test]
    fn a_zero_port_is_not_a_place_to_connect() {
        let endpoint = RemoteEndpoint {
            host: "127.0.0.1".into(),
            port: 0,
            host_key: HostKeyPolicy::Strict,
            capabilities: vec![RemoteCapability::WorkspaceRead],
        };
        assert_eq!(endpoint.validate(), Err("remote port is invalid".into()));
    }

    /// The point of listing a capability in `all()` is that a session may be
    /// granted it, so this is the assertion that port forwarding is real.
    #[test]
    fn port_forwarding_is_advertised_because_it_is_implemented() {
        assert!(RemoteCapability::all().contains(&RemoteCapability::PortForward));
        assert_eq!(
            parse_capability("port-forward"),
            Ok(RemoteCapability::PortForward)
        );
        assert_eq!(
            parse_capability("port_forward"),
            Ok(RemoteCapability::PortForward)
        );
        let endpoint = RemoteEndpoint {
            host: "buildbox".into(),
            port: 22,
            host_key: HostKeyPolicy::Strict,
            capabilities: vec![RemoteCapability::PortForward],
        };
        assert!(endpoint.validate().is_ok());
    }

    /// The other direction of forwarding is named so that the refusal can explain
    /// the difference instead of leaving someone to guess that they mistyped the
    /// one that works.
    #[test]
    fn remote_forwarding_is_named_but_refused_with_the_alternative() {
        assert!(!RemoteCapability::all().contains(&RemoteCapability::RemoteForward));
        let reason = parse_capability("remote-forward").expect_err("not implemented");
        assert!(
            reason.contains("port-forward"),
            "the refusal should point at the thing that does the job: {reason}"
        );
        let endpoint = RemoteEndpoint {
            host: "buildbox".into(),
            port: 22,
            host_key: HostKeyPolicy::Strict,
            // A config file does not go through `parse_capability`, so this is the
            // path that has to catch it as well.
            capabilities: vec![RemoteCapability::RemoteForward],
        };
        assert!(endpoint.validate().is_err());
    }
}
