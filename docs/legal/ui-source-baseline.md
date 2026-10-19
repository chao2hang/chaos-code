# GUI source and asset baseline

- Date: 2026-09-24; current ZCode frontend boundary recorded 2026-10-17
- Product line: Chaos Web/Desktop
- Source decision: clean-room Rust host; ZCode Web frontend source managed in this project
- Review scope: `crates/codegen/chaos-engine`,
  `crates/codegen/xai-grok-web`, and `crates/codegen/xai-grok-desktop`

## Source and license decision

The initial Chaos GUI implementation used a clean-room React walking skeleton and
Rust host crates. The React client was retired on 2026-10-09; its source/asset
inventory below is historical and does not describe the current Web frontend.
The Rust host crates are written in this repository and use the repository's
Apache-2.0 project dependencies and third-party notice process. New Rust files
follow this repository's Apache-2.0 license convention.

## Current Web frontend: ZCode (updated 2026-10-17)

The self-built React client (`apps/chaos-ui`) was retired and deleted in full on
2026-10-09. The current Web client is the official ZCode frontend; its source is managed by
this project in a frontend checkout separate from the `chaos-code` Cargo
workspace. The frontend toolchain/build is separate; frontend source ownership
remains with this project. Chaos also owns the Rust host and RPC bridge
(`crates/codegen/xai-grok-web/src/chaos_bridge.rs`).

The deployment uses ZCode's built static assets and includes the corresponding
ZCode third-party notices. ZCode's source license is Apache-2.0; the exact
frontend source revision, package lock, asset inventory and notice output must
be tracked with this project's frontend release/build record. Do not describe
ZCode source or assets as Chaos clean-room work. The Rust host and Engine remain
covered by this repository's Apache-2.0 license and notices.

## Asset and brand inventory

| Class | Current GUI use | Decision |
|---|---|---|
| ZCode Web fonts, icons, illustrations and preview assets | Present in the project-managed ZCode static distribution, built separately from the Rust host | Source/license/notices belong to the pinned frontend revision and its build artifacts; not clean-room Chaos-authored assets |
| `Chaos` name and Rust host messages | Chaos host/product branding | Owned product use; do not relabel as ZCode source |
| Old `apps/chaos-ui` assets | Retired and removed | Not part of current deployment |

Before a release, record the exact ZCode source revision and dependency lock, the
build command/runtime, and third-party-notices artifact alongside the deployed
bundle. Deployment copies the ZCode distribution; it does not make the bundle
first-party Chaos source.

## Scope boundary

The current Web topology is the authenticated `chaos-web` Rust host serving the
project-managed ZCode browser client and bridging `/ws/zcode` to Chaos Engine.
Frontend source/toolchain and Rust host are separate workspaces. Desktop remains
a separate Rust host boundary;
a supported Tauri/WebView product is not implied. Public/multi-user deployment,
remote PTY, embedded browser, voice, and CUA remain subject to their own security
and product decisions.
