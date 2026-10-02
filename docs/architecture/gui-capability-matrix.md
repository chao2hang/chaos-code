# GUI capability matrix

Status: M-1 baseline, updated 2026-10-01. Existing implementation locations are listed
before each decision; `reuse`, `adapter`, and `new` distinguish the boundary.
First-stable product decisions are in [ADR-007](adr-007-first-stable-product-scope.md).

| Capability | Existing implementation | M0/M4 decision | Implementation kind |
|---|---|---|---|
| Agent turn | `xai-grok-pager::headless::run_single_turn` | Keep behind an engine adapter; no GUI crate imports `ratatui` | adapter |
| Session events | `xai-grok-session-events`, pager session startup | Versioned GUI envelope in `chaos-engine`; adapter later maps real events | new + adapter |
| File/Git/hunk | `xai-grok-workspace*`, `xai-grok-pager-diff`, `xai-hunk-tracker` | Reuse after M1/M2 protocol boundary | reuse |
| Workspace server | `xai-grok-workspace` `xai-workspace-server` binary | Deferred outside first stable; do not deploy until ADR-007 gates are met; daemon is lifecycle-only | unsupported for first stable |
| Workspace client | `xai-grok-workspace-client` hub-proxied RPC | Deferred outside first stable; transport adapter requires ADR-007 gates | unsupported for first stable |
| Workspace daemon | `xai-grok-workspace-daemon` daemonize/pidfile/preview supervision | Not treated as a complete RPC daemon | reuse |
| Web transport | Existing Axum appears in diagnostics/PTY tools | New loopback-only Axum host with bearer/Origin/security headers | new |
| Desktop transport | No Tauri host exists | `xai-grok-desktop` host boundary; Tauri integration is a gated spike | new |
| Persistence | `xai-sqlite-journal`, dashboard/session stores | M0 atomic JSON snapshot; M2 canonical SQLite schema/migration | adapter |
| Auth/secrets | `xai-grok-config`, `xai-grok-secrets` | No API key crosses GUI envelope; provider settings are M3 | reuse |
| MCP/plugins/workflows | Existing MCP, plugin marketplace and `xai-workflow` crates | M3 adapters after approval/audit envelope | adapter |
| Remote SSH | No approved GUI transport yet | Deferred outside first stable; requires selected transport/auth policy, host-key rules, security review, and controlled host | deferred |
| PTY/detached Agent/container | Not a supported GUI capability | Explicitly unsupported/deferred | unsupported |

The initial implementation is deliberately a protocol and host seam, not a
second Agent lifecycle. The deterministic responder proves client flow without
cloud credentials; wiring the existing headless lifecycle is the next M0 engine
task.
