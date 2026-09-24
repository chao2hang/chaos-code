# Remote capability status

Date: 2026-09-24.

The current GUI topology is local Agent plus local workspace. The repository now
contains a typed remote capability boundary in `chaos-engine::remote` so future
transports cannot silently advertise unsupported behavior.

- Host key policy is either strict verification or TOFU with a non-empty recorded
  fingerprint.
- Detached Agent is rejected by the boundary and remains Deferred.
- Interactive PTY, containers, and public Web deployment remain unsupported until
  their own threat model and acceptance environment exist.
- A real SSH transport, remote workspace server deployment, port forwarding,
  retries, host-key change handling, and upgrade rollback require a clean Linux
  remote and SSH library spike. No remote connection is attempted by M0/M1.
