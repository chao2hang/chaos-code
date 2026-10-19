# GUI protocol v1

The Rust `chaos-engine` crate is the single source for the legacy JSON envelope
used by its HTTP and `/ws` compatibility transports. ZCode does **not** consume
that legacy JSON protocol: the current ZCode frontend speaks its versioned binary
RPC protocol on `/ws/zcode`, translated by `xai-grok-web/src/chaos_bridge.rs`.

The old TypeScript mirror (`chaos-protocol-schema`, `protocol.ts`) and the
`apps/chaos-ui` client retired on 2026-10-09. The retained JSON surface is guarded
in-tree by `chaos_engine::client_message_samples()` (the single protocol inventory,
compiled against every `ClientMessage` variant), the exhaustive `safe_mode_tag`
match, and `xai-grok-web` integration tests that walk every sample over a real
socket. ZCode V4 framing and row/state contracts are documented separately in
[`../verification/zcode-v4-contract-2026-10-11.md`](../verification/zcode-v4-contract-2026-10-11.md).

## Envelope rules

- Every client command has a non-empty `client_msg_id`.
- `session_id` identifies the session; `sequence` is monotonic per session.
- `SessionSnapshot.sequence` is an atomic cut point. Events with a sequence at or
  below that point are already included in the snapshot.
- Duplicate `client_msg_id` values are acknowledged without repeating the side
  effect. The current in-memory scope is the engine process; persistent engines
  persist the deduplication set with the snapshot.
- A reconnect sends `resume`; the server returns the last atomic snapshot. The
  client discards local timeline state before applying that snapshot.
- Text is emitted as bounded `text_delta` events. A delta is never a file,
  attachment, or credential channel.
- Unknown capabilities and message types fail with a typed error; they never
  fall back to an approximate destructive action.

## M0 message set

Client: `create_session`, `resume`, `submit`, `cancel`, `snapshot`, `approve`,
`reject`.

Server: `handshake`, `session_created`, `session_snapshot`, `ack`, `text_delta`,
`completed`, `cancelled`, `tool_approval_requested`, `approval_resolved`,
`audit`, `error`.

M1 adds reasoning, tool progress/results, questions, file changes, usage, and
structured errors without changing the M0 meanings. M2 adds range/file/upload
channels outside the ordinary event queue.
