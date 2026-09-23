# GUI capability matrix

Status: initial M-1 baseline, 2026-09-24.

| Capability | Existing implementation | Walking-skeleton decision | Owner/next step |
|---|---|---|---|
| Agent turn | `xai-grok-pager::headless::run_single_turn` | Keep behind an engine adapter; no GUI crate imports `ratatui` | Engine extraction in M0 |
| Session events | `xai-grok-session-events`, pager session startup | Define a small versioned GUI envelope first | Engine |
| File/Git/hunk | `xai-grok-workspace*`, `xai-grok-pager-diff`, `xai-hunk-tracker` | Reuse after protocol baseline | M2 |
| Web transport | Existing Axum dependencies in unrelated servers | New loopback-only `xai-grok-web` host | Web |
| Desktop transport | No Tauri host exists | `xai-grok-desktop` owns host boundary; Tauri integration follows after spike | Desktop |
| Persistence | `xai-sqlite-journal`, dashboard/session stores | In-memory store for M0; migration ADR required before persistence | M2 |
| Auth/secrets | Existing config/secrets crates | No API key crosses the GUI envelope | Security ADR |

The initial implementation is deliberately a protocol and host seam, not a second
Agent lifecycle. The engine's mock responder proves the client flow without cloud
credentials; wiring the existing headless lifecycle is the next M0 engine task.
