# ADR-001: GUI walking skeleton boundaries

- Status: accepted for M-1/M0 implementation
- Date: 2026-09-24

## Decision

Chaos Web and Desktop share a small Rust engine protocol. `chaos-engine` owns
versioned client/server envelopes and an engine handle. `xai-grok-web` owns the
loopback Axum transport. `xai-grok-desktop` owns the desktop host boundary and
will add Tauri commands without putting Tauri dependencies into the default TUI
binary. `apps/chaos-ui` is a React/TypeScript client.

The first engine responder is a deterministic mock so the UI and transport can
be tested without provider credentials. The existing headless agent remains the
behavioral source for the eventual adapter; it is not duplicated in the UI.

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
