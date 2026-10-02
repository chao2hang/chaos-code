# M-1 scope matrix

Date: 2026-10-01. The first stable GUI target is a single-user local developer
workbench. It is not a public multi-user service. First-stable boundaries were
made explicit in [ADR-007](adr-007-first-stable-product-scope.md); that scope
decision does not substitute for implementation or platform acceptance.

| Capability | Decision | Initial support |
|---|---|---|
| Session list/timeline/stream | Parity target | M0/M1 |
| Text Prompt and cancel | Parity target | M0 |
| Tools, questions, approval | Replacement using Chaos engine envelope | M1 |
| Diff review/rollback | Replacement using existing Rust diff/hunk crates | M1/M2 |
| Files/search/Git/terminal | Replacement using workspace/PTY adapters | M2 |
| Settings/provider/model | Accepted roadmap target; not yet fully supported | Complete settings UX; real credential storage and Provider acceptance require reviewed security design and provider evidence |
| MCP/plugins/skills/workflows/subagents | Accepted roadmap targets; not yet supported | Reviewed trust policy, approval-gated adapters, real lifecycle contracts, and end-to-end acceptance |
| Remote workspace | Deferred; excluded from first stable | Reconsider after approved remote threat model, authenticated transport, server ownership, and controlled-host acceptance |
| Remote interactive PTY | Unsupported for first stable | Separate product/security review and platform acceptance |
| Detached remote Agent | Unsupported for first stable | Separate product/security review and platform acceptance |
| Embedded browser/CDP | Unsupported for first stable | Reconsider only after dedicated security review and host acceptance |
| Cloud sharing/sync/login wall | Unsupported for first stable | Reconsider only after separately approved product, privacy, and operations design |
| Voice/CUA/idle automation | Deferred; not part of first stable | Separate product and security review |

Platform scope: Linux desktop is the local development/acceptance platform for
M0; macOS and Windows require independent CI build jobs before being supported.
Web is a loopback local service and supports narrow viewports as an adaptation,
not as a separately guaranteed mobile platform. Public binding and multi-user
operation are outside the first stable scope.

The GUI does not read or write provider API keys through the browser protocol.
Provider configuration and secret storage are a later M3 boundary.
