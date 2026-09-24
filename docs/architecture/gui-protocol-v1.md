# GUI protocol v1

The Rust `chaos-engine` crate is the single source for the logical envelope used
by WebSocket, HTTP, and Desktop transports. The wire encoding is JSON in M0.

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
