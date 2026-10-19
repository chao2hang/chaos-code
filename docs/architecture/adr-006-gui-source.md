# ADR-006: GUI source and maintenance model

- Status: superseded for the Web frontend by the 2026-10-09 ZCode decision
- Date: 2026-09-24

## Historical decision

The original GUI plan selected a clean-room React client and Rust host, with the
Rust engine protocol as the shared logical contract. That React client was
implemented under `apps/chaos-ui`, then retired and deleted in full on
2026-10-09. Its clean-room source and asset decision remains historical; it is
not the current Web frontend source model.

## Current Web frontend boundary

Chaos Web now uses the official ZCode Web frontend, whose source is managed by
this project in a frontend checkout separate from the `chaos-code` Rust workspace
and built with its own frontend toolchain. This repository also owns the Rust `xai-grok-web`
host and `chaos_bridge` that serves the deployed static bundle and translates
ZCode RPC frames to Chaos Engine messages. The frontend is not a Cargo workspace
member, and the current Cargo CI does not build it.

ZCode source, assets, and dependency changes are part of this project's frontend
maintenance and must retain the applicable Apache-2.0 license, attribution, and
third-party notices. A deployed bundle must come from a reviewed revision in the
project-managed frontend checkout and retain its notices; the Chaos host and
Engine also follow this repository's Apache-2.0 and third-party notice process. See [`../legal/ui-source-baseline.md`](../legal/ui-source-baseline.md)
and [`zcode-web-frontend.md`](zcode-web-frontend.md).

This decision does not alter Chaos CLI/TUI identifiers, config paths, ZCode RPC
wire IDs, or the separation between frontend source and Rust host.
