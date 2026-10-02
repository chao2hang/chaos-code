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

## Failure model for a session (2026-10-02)

A remote session has two clocks, and the difference between them is the decision.
Opening the transport may be retried: a dial presents nothing, so waiting out a
tunnel that is still being set up costs time and nothing else. A request that has
already been sent may not be retried: its reply is either the answer to it or
nothing at all, because a late reply would be read as the answer to whichever
request comes next. So the client bounds both waits — the handshake at
`DEFAULT_HANDSHAKE_TIMEOUT`, each request at whatever the caller sets — and ends
the session when a reply does not arrive, instead of pretending the stream is
still usable.

There is deliberately no automatic reconnect. A credential opens exactly one
session, so reconnecting means obtaining a new credential from whoever runs the
command, and the transport does not get to decide that on their behalf. A run that
fails still retires the credential it sent, so the next invocation starts from a
token file that has not been left holding a spent credential.
