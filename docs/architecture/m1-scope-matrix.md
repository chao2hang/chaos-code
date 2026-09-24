# M-1 scope matrix

Date: 2026-09-24. The first stable GUI target is a single-user local developer
workbench. It is not a public multi-user service.

| Capability | Decision | Initial support |
|---|---|---|
| Session list/timeline/stream | Parity target | M0/M1 |
| Text Prompt and cancel | Parity target | M0 |
| Tools, questions, approval | Replacement using Chaos engine envelope | M1 |
| Diff review/rollback | Replacement using existing Rust diff/hunk crates | M1/M2 |
| Files/search/Git/terminal | Replacement using workspace/PTY adapters | M2 |
| Settings/provider/model | Replacement using Chaos config boundary | M3 |
| MCP/plugins/skills/workflows/subagents | Replacement using existing Rust crates | M3 |
| Remote workspace | Degraded/explicit M4 | M4 |
| Remote interactive PTY | Deferred | post-M4 evaluation |
| Detached remote Agent | Deferred | post-M4 evaluation |
| Embedded browser/CDP | Unsupported for first stable | post-M5 review |
| Cloud sharing/sync/login wall | Unsupported for first stable | product review |
| Voice/CUA/idle automation | Deferred | separate review |

Platform scope: Linux desktop is the local development/acceptance platform for
M0; macOS and Windows require independent CI build jobs before being supported.
Web is a loopback local service and supports narrow viewports as an adaptation,
not as a separately guaranteed mobile platform. Public binding and multi-user
operation are outside the first stable scope.

The GUI does not read or write provider API keys through the browser protocol.
Provider configuration and secret storage are a later M3 boundary.
