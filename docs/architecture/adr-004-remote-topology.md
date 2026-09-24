# ADR-004: remote topology and current support boundary

- Status: accepted boundary, remote implementation deferred
- Date: 2026-09-24

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
