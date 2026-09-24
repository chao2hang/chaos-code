use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RemoteCapability {
    WorkspaceList,
    WorkspaceRead,
    WorkspaceSearch,
    WorkspaceWrite,
    Git,
    ToolExecution,
    PortForward,
    InteractivePty,
    DetachedAgent,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum HostKeyPolicy {
    Strict,
    TrustOnFirstUse { fingerprint: String },
}

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
        if let HostKeyPolicy::TrustOnFirstUse { fingerprint } = &self.host_key {
            if fingerprint.trim().is_empty() {
                return Err("TOFU requires a host fingerprint".into());
            }
        }
        if self.capabilities.contains(&RemoteCapability::DetachedAgent) {
            return Err("detached Agent is not supported by the current topology".into());
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
}
