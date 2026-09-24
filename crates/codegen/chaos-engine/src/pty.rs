use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PtyCapability {
    pub supported: bool,
    pub requires_approval: bool,
    pub supports_resize: bool,
    pub supports_reconnect: bool,
    pub supports_cancel: bool,
}

impl PtyCapability {
    pub fn local_controller() -> Self {
        Self {
            supported: true,
            requires_approval: true,
            supports_resize: true,
            supports_reconnect: true,
            supports_cancel: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pty_capability_declares_security_and_lifecycle_requirements() {
        let capability = PtyCapability::local_controller();
        assert!(capability.supported);
        assert!(capability.requires_approval);
        assert!(
            capability.supports_resize
                && capability.supports_reconnect
                && capability.supports_cancel
        );
    }
}
