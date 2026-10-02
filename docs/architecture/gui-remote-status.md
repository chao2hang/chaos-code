# Remote capability status

Date: 2026-10-01. See [ADR-007](adr-007-first-stable-product-scope.md) for
first-stable scope; this status records implementation boundaries, not product
availability.

The current GUI topology is local Agent plus local workspace. The repository now
contains a typed remote capability boundary in `chaos-engine::remote` so future
transports cannot silently advertise unsupported behavior.

- Host key policy is either strict verification or TOFU with a non-empty recorded
  fingerprint.
- Remote workspace and port forwarding are Deferred and excluded from first stable. They require the approved design and controlled-host acceptance in ADR-007 before implementation or advertisement.
- Detached Agent, interactive PTY, and container execution are Unsupported for first stable until separate product/security review and acceptance.
- Public/multi-user Web deployment is Unsupported for first stable; the host remains loopback-only pending the independent deployment security gate.
- A real SSH transport, remote workspace server deployment, port forwarding,
  retries, host-key change handling, and upgrade rollback require a clean Linux
  remote and SSH library spike. No remote connection is attempted by M0/M1.
