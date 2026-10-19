# GUI build and CI isolation

Date: 2026-09-24. Updated: 2026-10-19.

## Current Web build boundary

The browser client is the official ZCode Web frontend. Its source is integrated
in this repository at `frontend/zcode/`; its pnpm workspace remains independent
of the `chaos-code` Cargo workspace. This is a build-boundary decision, not a
transfer of frontend ownership. The retired `apps/chaos-ui` was removed on
2026-10-09. ZCode source, lockfile and third-party notices are project-managed
in the integrated directory and built with the frontend toolchain, not through Cargo.

- ZCode UI changes are linted and built from `frontend/zcode/` with its Node
  24+/pnpm toolchain (`@zcode/ui`, `@zcode/web`). Its
  `frontend/zcode/packages/web/dist/` output is deployed to the directory
  configured by `CHAOS_WEB_ASSETS_DIR`. The source/build process and deployment
  checks are documented in [`zcode-web-frontend.md`](zcode-web-frontend.md).
- `xai-grok-web` remains the Rust host: it serves the deployed assets, handles
  authentication, and bridges the ZCode binary RPC at `/ws/zcode` to
  `chaos-engine`. Its Rust build and socket integration tests remain in this
  workspace and CI.
- The `gui` CI job currently covers the Rust host/engine packages only; it does
  not yet typecheck, build, or browser-test `frontend/zcode/`. Run the frontend
  checks from that directory before deploying changed assets.
- The Desktop crate still has no Tauri dependency; native Desktop build and
  three-platform acceptance remain separate roadmap gates.

The old `apps/chaos-ui` Node installation/build isolation notes below the former
M0 plan are historical and no longer describe an active workspace member.
