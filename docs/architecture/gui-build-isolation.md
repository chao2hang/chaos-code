# GUI build and CI isolation

Date: 2026-09-24.

- GUI Rust crates are independent workspace members in the marked fork-layer
  block. The Desktop crate has no Tauri dependency yet, so the default TUI
  binary does not compile a WebView stack.
- Frontend dependencies and output live under `apps/chaos-ui`; `node_modules`
  and `dist` are ignored. They are never copied into Rust `target/`.
- The existing `rust` CI job remains unchanged in scope. The separate `gui` job
  runs GUI Rust tests and frontend install/typecheck/build.
- Local low-memory builds use `CARGO_BUILD_JOBS=4`; cleanups distinguish
  `target/debug`, `target/release`, and `target/release-dist`.
- If a future Tauri integration makes the main job exceed its 60-minute budget
  or materially expands the default TUI dependency graph, remove the GUI host
  from the shared workspace and publish the engine/host as an independent
  package before continuing.

The current isolation evidence is the passing GUI job structure and the absence
of Tauri dependencies in the default Cargo dependency graph. Three-platform
Tauri builds remain a later M5 gate.
