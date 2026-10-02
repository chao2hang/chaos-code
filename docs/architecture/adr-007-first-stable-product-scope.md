# ADR-007: first stable product scope and support claims

- Status: conservative scope clarification recorded under delegated execution responsibility; independent product/security sign-off remains a release gate
- Decision owner: delegated implementation owner
- Review by: 2027-10-01, or before any deferred capability is enabled
- Date: 2026-10-01
- Supersedes: unresolved first-stable scope entries identified below; it does not waive accepted roadmap work or implementation/release gates.

## Decision

The accepted product goal remains the single-user local developer workbench described in `TODO.md`, including its Web and Tauri desktop targets. This record does not cancel those deliverables. A capability is not supported for release merely because a protocol type, UI seam, fixture, or local-only adapter exists; support claims require their existing acceptance gates.

The following first-stable boundaries are explicit:

| Capability | Scope decision | Release claim condition |
|---|---|---|
| Sharing, quotas, cloud sync, and login wall | Unsupported for first stable | Reconsider only after a separately approved product, privacy, and operations design |
| Remote workspace execution and port forwarding | Deferred from first stable; retained as post-first-stable M4 roadmap work | Approved remote threat model, authenticated transport, server ownership, and controlled-host acceptance |
| Remote interactive PTY, detached Agent, and container execution | Unsupported for first stable | Separate product and security review plus platform acceptance |
| Public/multi-user Web deployment | Unsupported for first stable | Separate deployment, authentication, CSRF, audit, and operational security review |
| Linux Tauri desktop | Accepted roadmap target, not yet supported | Real Tauri executable and visible-path acceptance |
| macOS and Windows desktop | Accepted only if later added to the support matrix; not currently supported | Independent target-platform builds and user-path acceptance |
| Provider, MCP/plugins, workflows, and subagents | Accepted roadmap targets, not yet supported | Reviewed secret/trust policy, real adapters/lifecycle contracts, and controlled end-to-end acceptance |

These decisions prevent release materials and capability declarations from implying an untested feature is available. Linux loopback Web acceptance does not establish native desktop support. Narrow browser layouts are an adaptation, not a mobile support promise.

## Decisions intentionally not made

This record does not authorize API-key storage without a reviewed OS keyring/secret-store design; real Provider calls without approved disposable credentials/endpoints; MCP/plugin execution without an approved source, signature, and permission policy; SSH without an approved transport/authentication/host-key policy and controlled host; or signing and publishing without release-owner secrets, platform runners, and release approval. Existing fail-closed boundaries remain in force.

The read-only `chaos telemetry status` command is implemented and covered by real-binary regression tests. The earlier `disable`/`enable` write-command design remains a post-release proposal, not an accepted command contract. A telemetry owner and security review must resolve configuration precedence, requirements lock behavior, and safe file updates before implementation.

No individual maintainer, reviewer, package owner, or external service owner is assigned by this record. The user delegated execution responsibility in this session; that does not grant account access or attest external approvals. TODO rows requiring those controls remain open until evidence is available.

## Consequences

- `TODO.md` remains the sole status checklist. This record clarifies first-stable scope and support claims, not task completion state.
- The compatibility matrix and remote/security status documents must match these decisions.
- Deferred/unsupported features must not be advertised as delivered; re-entry or support claims require the stated review and acceptance conditions.
