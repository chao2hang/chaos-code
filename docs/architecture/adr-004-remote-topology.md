# ADR-004: remote topology and current support boundary

- Status: accepted security boundary; remote implementation Deferred outside first stable
- Decision record: [ADR-007](adr-007-first-stable-product-scope.md)
- Date: 2026-10-01

The first supported topology is local Agent plus local workspace. Web and
Desktop transports do not expose remote execution, arbitrary file writes,
terminal sessions, or port forwarding. Remote Agent plus remote tools remains a
future M4 implementation and must use a dedicated authenticated transport,
host-key verification, workspace confinement, cancellation, and upgrade
rollback.

The UI capability declaration must keep remote PTY, detached Agent, container
execution, and public Web deployment as unsupported until those tests and
security controls exist. This decision prevents a local loopback prototype from
being mistaken for a remote control plane.
