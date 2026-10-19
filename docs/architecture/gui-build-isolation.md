# GUI build and CI isolation

Date: 2026-09-24. Updated: 2026-10-17.

## Current Web build boundary

The browser client is the official ZCode Web frontend. Its source is managed
by this project in a frontend checkout separate from the `chaos-code` Rust
workspace. This is a build-boundary decision, not a transfer of frontend
ownership. The retired `apps/chaos-ui` was removed on 2026-10-09. The ZCode
source/lockfile remains project-managed and is built with its frontend toolchain,
not through Cargo.

- ZCode UI changes are linted and built from the project-managed ZCode source
  checkout with its Node 24+/pnpm toolchain (`@zcode/ui`, `@zcode/web`). Its
  `packages/web/dist/` output is deployed to the directory configured by
  `CHAOS_WEB_ASSETS_DIR`. The source/build process and deployment checks are documented in
  [`zcode-web-frontend.md`](zcode-web-frontend.md).
- `xai-grok-web` remains the Rust host: it serves the deployed assets, handles
  authentication, and bridges the ZCode binary RPC at `/ws/zcode` to
  `chaos-engine`. Its Rust build and socket integration tests remain in this
  workspace and CI.
- The `gui` CI job covers the Rust host/engine packages only. It does not fetch,
  typecheck, build, or browser-test the project-managed ZCode frontend checkout.
  Run its checks there before deploying changed assets.
- The Desktop crate still has no Tauri dependency; native Desktop build and
  three-platform acceptance remain separate roadmap gates.

The old `apps/chaos-ui` Node installation/build isolation notes below the former
M0 plan are historical and no longer describe an active workspace member.
