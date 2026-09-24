# GUI execution status

Date: 2026-09-24. This document separates locally completed work from gates that
require platform runners, external services, or a later milestone.

## Completed locally

- M-1 clean-room source and asset baseline, scope matrix, capability matrix,
  ADR-001/002/003/004/005/006, and GUI fork-layer inventory.
- Independent GUI Rust packages and frontend build isolation.
- M0 protocol v1 for handshake, create/resume, submit, bounded UTF-8 deltas,
  completion, cancellation, snapshot, deduplication, approval, rejection, and
  audit events.
- Axum loopback HTTP/WebSocket transport with bearer authorization, Origin
  checks, request-size limit, CSP, `nosniff`, and health endpoint.
- Atomic JSON snapshot persistence used by the M0 engine.
- React client connected to the real WebSocket transport with reconnect/resume,
  visible streaming state, cancel, and approval card.
- Independent CI job for GUI Rust and frontend checks.

## Remaining gates

| Area | State | Evidence required |
|---|---|---|
| Real Agent adapter | Open | Adapter tests driving the existing headless lifecycle |
| Tauri Desktop | Open | Tauri three-platform builds and real desktop flow |
| Browser E2E | Open | Playwright or equivalent installed and exercised at desktop/narrow viewports |
| Provider/config/secrets | Open | M3 provider and keyring design plus failure tests |
| M1 tools/Diff | Open | Real workspace/tool adapters, approval enforcement, rollback tests |
| M2 persistence/workspace | Open | SQLite migration, workspace/file/search/terminal/Git tests |
| M3 ecosystem | Open | MCP/plugin/skill/workflow/subagent integration tests |
| M4 remote | Open | Clean Linux remote, SSH host-key and forwarding tests |
| M5 release | Open | CI packaging/signing/SBOM/performance/manual acceptance |

The open rows are intentionally not marked complete in `TODO.md`. Mock engine
responses, library tests, and a curl check prove the local protocol seam only;
they do not prove the product milestones above.
