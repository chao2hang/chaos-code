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
- M1 tool adapter boundary: approvals are resolved before a `ToolAdapter` can
  execute; missing adapters fail closed and tool output is recorded in the
  session timeline/audit.
- M1 Diff adapter boundary: accept and rollback are session-scoped adapter calls;
  success emits `diff_resolved`, failure emits `diff_failed`, and no UI event
  claims a file was changed before the adapter reports success. WebSocket tests
  assert ordered ACK/resolution/audit events for accept and rollback.
- M1 interaction events now include question request/response and explicit
  protocol variants for tool progress, tool result, file change, and usage;
  WebSocket tests assert tool started/progress/result/usage ordering. Provider-
  backed production emission remains a later adapter task.
- M2 workspace boundary now canonicalizes a configured root and supports bounded
  list/read/search requests; path escapes and missing workspace adapters fail
  closed before any browser-controlled path reaches filesystem I/O.
- M2 JSON snapshot persistence now carries a schema version, rejects newer
  schemas, and backs up legacy raw snapshots before loading. A canonical
  `SqliteSessionStore` boundary now uses the existing journal-mode policy and
  tests round-trip/newer-schema rejection; production engine wiring and full
  migration/concurrency gates remain open.
- M2 attachment policy validates filename/content type/size before any upload,
  and the workspace Git seam only runs fixed status arguments under the
  canonical workspace root; a fixed-cwd terminal adapter now also requires
  approval and caps output, while full upload/Git mutation/PTY remains gated.
- M3 settings seam exposes only non-secret Base URL/model fields, rejects unsafe
  URLs, and reports `has_api_key` as a boolean; provider shape validation returns
  `network_not_attempted` without touching credentials or making network calls.
  API key storage remains outside the GUI protocol.
- Axum loopback HTTP/WebSocket transport with bearer authorization, Origin
  checks, request-size limit, CSP, `nosniff`, and health endpoint.
- Transitional JSON snapshot and canonical `SqliteSessionStore` persistence, with
  schema/version tests, corrupt/missing-parent rejection, legacy backup, and real
  Web SQLite re-open recovery.
- React client connected to the real WebSocket transport with reconnect/resume,
  visible streaming state, cancel, approval/question cards, and tested event
  reducer projection.
- Independent CI job for GUI Rust, frontend unit, typecheck, and build checks.

## Remaining gates

| Area | State | Evidence required |
|---|---|---|
| Real Agent adapter | Partial | Explicit headless process adapter and Web integration tests; async provider lifecycle remains open |
| Tauri Desktop | Open | Tauri three-platform builds and real desktop flow |
| Browser E2E | Open | Playwright or equivalent installed and exercised at desktop/narrow viewports |
| Provider/config/secrets | Partial | Non-secret settings/provider shape validation; real provider/keyring tests remain open |
| M1 tools/Diff | Partial | Tool/Diff adapters, approval, question, audit and ordered Web tests; real workspace hunk adapter remains open |
| M2 persistence/workspace | Partial | Root-confined workspace, Git status, attachment policy, JSON/SQLite boundaries and Web recovery; terminal/full Git/migration concurrency remain open |
| M3 ecosystem | Open | MCP/plugin/skill/workflow/subagent integration tests |
| M4 remote | Partial | Typed capability/host-key boundary; clean Linux remote, SSH transport and forwarding remain open |
| M5 release | Partial | GUI CI, signing preflight, fail-closed installers and policy fixtures; packaging/signing assets/SBOM/performance/manual acceptance remain open |

The open rows are intentionally not marked complete in `TODO.md`. Mock engine
responses, library tests, and a curl check prove the local protocol seam only;
they do not prove the product milestones above. Existing TUI session persistence
uses `xai-grok-shell` session directories and `summary.json` through its actor,
so compatibility with GUI SQLite is an explicit migration task rather than an
implicit format match.
