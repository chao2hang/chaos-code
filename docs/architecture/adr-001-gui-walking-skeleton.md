# ADR-001: GUI walking skeleton boundaries

- Status: superseded on 2026-10-09 for the frontend boundary (decision ①)
- Date: 2026-09-24

> **2026-10-09 amendment:** `apps/chaos-ui` and the self-built React client were
> retired in full. The engine protocol and the Rust host boundaries in this
> ADR remain as decided; the frontend boundary is now the ZCode official
> frontend over the Rust RPC bridge (`crates/codegen/xai-grok-web/src/chaos_bridge.rs`),
> with `xai-grok-desktop` still the desktop host boundary.

## Historical decision

Chaos Web and Desktop share a Rust engine protocol. `chaos-engine` owns the
engine handle; `xai-grok-web` owns the Axum transport and WebSocket host;
`xai-grok-desktop` is the separate desktop host boundary. The original decision
paired these crates with a React/TypeScript client in `apps/chaos-ui`, which has
since been retired and deleted.

The first engine responder was a deterministic mock so the transport could be
tested without provider credentials. The existing headless agent remains the
behavioral source for the CLI adapter; it is not duplicated in the Web UI.

## Current Web client

The current Web client is the official ZCode frontend, whose source is managed
by this project in `frontend/zcode/`, a pnpm workspace separate from the
`chaos-code` Cargo workspace. It connects to the Chaos host through the binary
ZCode RPC bridge at
`/ws/zcode`; the Rust host continues to own authentication, static asset serving,
authorization, and Engine integration. See
[`zcode-web-frontend.md`](zcode-web-frontend.md) for build, deploy and test
boundaries.

## Alternatives

1. Put Web/Desktop routes directly in `xai-grok-pager`: rejected because it
   pulls terminal rendering concerns into GUI builds and increases upstream sync
   conflicts.
2. Build separate Web and Desktop session implementations: rejected because it
   creates two cancellation, sequencing, and recovery state machines.
3. Extract all headless code before a walking skeleton: deferred. It is safer to
   prove the protocol and host seam first, then move the adapter with regression
   tests.

## Compatibility and security

The protocol is versioned and every command carries a client message ID. M0
binds Web to loopback; authentication, Origin/Host checks, CSP, request limits,
persistence, and real provider wiring are required before non-loopback use.
The mock must never be presented as a connected provider.

## Rollback

Removing the three new workspace members and `apps/chaos-ui` restores the TUI
build. No existing TUI crate is changed by the walking skeleton.
