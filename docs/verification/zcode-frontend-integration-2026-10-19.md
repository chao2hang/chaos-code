# ZCode frontend integration into Chaos

- Date: 2026-10-19
- Destination: Chaos Code `frontend/zcode/`
- Source integration commit: `e106a0ae94fbbb0846a2b5051dac4baf36ab58b3`
- Source parents: ZCode `origin/main` `aac4755666d09fdcd70272fcf063c077a639015f` (v3.15.1) and `origin/feat/ui-plugin` `662c30bea4e833acaacbfb745a65eb09c23d55f8`; merge base `29628c9acdb81b703bbd4080c207a0e7ce5e276e` (v3.14.3).
- Imported revision tree: 16,881 tracked files; 187,552,749 source bytes, excluding ignored dependencies and build artifacts.

## Ownership and build boundary

ZCode's original history, including both merged source lines and its local UI work, remains reachable as the second parent of the Chaos integration commit and through the local `zcode/source-integration` branch. The Chaos main snapshot imports those source files at `frontend/zcode/`; its pnpm workspace is nested in the repository but remains outside the root Cargo workspace. No ZCode UI or Node dependency is added to a Rust crate.

Build static browser assets from the integrated source tree and deploy only `frontend/zcode/packages/web/dist/` contents to the verified `CHAOS_WEB_ASSETS_DIR`. The running `chaos-web` Rust host remains responsible for auth, static serving and `/ws/zcode`; no replacement server or port is introduced.

The source and its upstream Apache-2.0 license, `THIRD-PARTY-NOTICES.md`, and dependency/source metadata are retained. ZCode assets and UI remain upstream-derived, not clean-room Chaos-authored content.

## Checks performed on the source merge

- `pnpm typecheck`: pass with Node 24.21.0.
- Full `pnpm lint`: pass, 0 errors and 64 existing warnings; targeted modified-file lint: pass, 0 errors (3 existing unused-prop warnings in `WorkspaceShellLayout.tsx`).
- `pnpm architecture:check --changed`: pass, 0 violations.
- Targeted renderer/shell/history UI tests: 164 passed across 6 files.
- Plugin UI, Gen UI and MCP Apps contract/service tests: 49 passed across 6 files.
- `pnpm --filter @zcode/web build`: pass. Vite emitted existing large-chunk and ineffective-dynamic-import warnings.
- ZCode source merge conflict resolution tests cover floating status-panel layout, distinct assistant usage tail/topic preparation, remote mobile navigation, and preserved Chaos workspace-shell behavior.

## Terminal output improvement

The shared foreground Bash result now has an output heading and light panel boundary, primary text contrast, responsive 8/12-line viewport, no-wrap terminal columns with horizontal scroll by default, and an accessible localized wrap toggle. It retains the existing vertical follow/freeze owner and streaming updates. EN/ZH UI strings and tests are in `frontend/zcode/packages/ui/`.

The foreground Bash renderer does not expose a separate full-output or truncation path. Background Bash detail behavior remains unchanged.

## Lockfile note

The imported ZCode `pnpm-lock.yaml` initially did not match `packages/share-ui/package.json`: the lockfile contained four dependencies removed from the manifest. `pnpm install --no-frozen-lockfile --ignore-scripts` reconciled the ZCode checkout lock and completed locally; preserve the resulting lockfile update in the integrated source snapshot before relying on frozen installs in CI.
