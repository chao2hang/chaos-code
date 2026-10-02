# Performance benchmark environment

Status: reproducibility protocol for a local Linux/Chromium comparison only. This is not a CI performance gate, a release benchmark, or a product performance claim. Record a fresh environment report for each run; do not compare results across materially different machines or browser builds.

## Reference environment observed on 2026-10-01

| Dimension | Observed value |
| --- | --- |
| OS/kernel | Ubuntu 22.04 host, Linux 6.8.0-138-generic x86_64 |
| CPU | Intel Core i7-14700; 20 cores / 28 online logical CPUs; one socket |
| Memory | 62 GiB RAM; 8 GiB swap (availability varies during tests) |
| Runtime | Node.js v22.23.2, npm 10.9.8 |
| Browser driver | Playwright 1.63.0 from `apps/chaos-ui` lockfile installation |
| Browser | Chrome Headless Shell 153.0.8010.12 (Playwright Chromium revision 1243); capture exact version at run time |
| GUI build | `npm run build` in `apps/chaos-ui`; record commit, lockfile hash and build output |
| Rust build | `cargo build --release -p xai-grok-web`; record rustc version and Cargo.lock hash |

This machine is a developer host; CPU frequency, background load, thermal state, kernel scheduling and browser cache are not controlled. Treat its numbers as exploratory and never use them as stable CI thresholds.

## Run controls

1. Record commit SHA, dirty-tree status, `rustc -Vv`, `cargo -V`, Node/npm versions, Playwright and Chromium versions, kernel, CPU model, logical CPU count, RAM/swap, power profile if known, and the exact commands.
2. Build release artifacts once before timing. Keep compilation, dependency installation and browser installation outside measured intervals.
3. Use the same host, power mode, browser binary, viewport, device scale factor, dataset seed, application configuration and cache policy for all compared revisions. Close unrelated load; record notable background load rather than silently discarding the run.
4. Run one unreported warm-up, then at least 20 measured samples per startup/idle metric and 10 long-session trials. Save raw samples as JSON; report sample count, median (p50), p95, min/max, and all exclusions with a reason. Do not discard outliers without retaining them.
5. Use a fixed synthetic seed and verify the fixture's item/file/message counts before each run. Never use production user data.
6. Record RSS from the measured process and relevant child processes, distinguish peak from steady-state RSS, and state whether shared memory is included. Record cold-start cache preparation and hot-start cache state explicitly.
7. Do not set regression thresholds until multiple baseline runs on a stable CI runner establish natural variance and release owners approve the allowed regression. A local developer-host run cannot authorize a release gate.

## Required benchmark matrix

| Scenario | Fixed workload and measurement |
| --- | --- |
| Cold startup | Fresh process and documented cold-cache condition; time from process launch to a defined ready marker. At least 20 samples. |
| Warm startup | Repeated launch with documented warm OS/browser caches; same ready marker. At least 20 samples. |
| Idle memory | After ready and a fixed 60-second settle period, sample process-tree RSS once per second for 60 seconds; report median and peak. |
| Long conversation scrolling | Seed 10,000 messages with fixed-length mixed Markdown; automate top-to-bottom and history navigation at desktop 1280x800 and mobile 390x844; record frame/long-task data and anchor correctness. |
| Streaming | Deliver 150 text deltas per second for 60 seconds (9,000 deltas), fixed payload size and deterministic source; measure enqueue-to-render latency and dropped/misordered deltas. |
| File search | Create 100,000 files with a fixed path/content distribution and seed; measure index/setup separately from repeated query latency and verify result correctness. |
| Large Diff | Use a checked-in or generated fixed-seed diff fixture with documented file and line counts; measure parse/render latency and peak RSS, and verify the expected rendered file/hunk counts. |

For UI workloads, use the repository Playwright version and a fixed viewport/device scale factor. Include a browser trace or raw timing JSON with each report. For Rust filesystem/search workloads, use a release build and keep fixture creation outside the measured interval. Report filesystem type and whether the OS page cache was warm; do not claim cold disk measurements without a reproducible cache-control method.

## Current local collector

`apps/chaos-ui/perf-benchmark.mjs` launches the local Vite UI on a dynamically allocated loopback port and emits a JSON report. For a connected Engine path, run `cargo run -p xai-grok-web` separately and configure `CHAOS_PERF_BASE_URL` to point to a UI page whose `/api`, `/health` and `/ws` proxy reaches that host; the collector does not launch or validate a Web host. Run it from `apps/chaos-ui` with `CHAOS_PERF_REPORT_DIR=/path/to/reports npm run perf:collect`; optionally set `CHAOS_PERF_SAMPLES` (1–100, default 20). It records web-page-ready time, 900 browser-side synthetic DOM append scheduler samples (nominally 150/s), and Node runner RSS after a 60-second idle settle plus 60 one-second samples. `npm run test:perf` exercises invalid input and the complete local smoke run.

Current collector results are deliberately incomplete. “Web page ready” is not application cold startup; the synthetic updates do not use the real Engine/WebSocket/React path; RSS excludes Chromium and Web host process trees; it does not generate the 100,000-file search or large Diff fixture. The local smoke used one page-ready sample to prove report generation, not to establish a baseline. CI artifact upload, stable-runner repetition, release profile and accepted regression thresholds remain unimplemented.

## Machine-readable result format

Save one JSON file per run alongside raw samples. At minimum:

```json
{
  "schema_version": 1,
  "commit": "<git-sha>",
  "dirty": true,
  "environment": {
    "os": "<distribution and kernel>",
    "cpu": "<model>",
    "logical_cpus": 0,
    "memory_bytes": 0,
    "rustc": "<rustc -Vv>",
    "node": "<node --version>",
    "playwright": "<version>",
    "browser": "<browser name and exact version>"
  },
  "scenario": "<matrix name>",
  "dataset": {"seed": "<seed>", "items": 0},
  "samples": [],
  "summary": {"count": 0, "p50": null, "p95": null, "unit": "ms"},
  "notes": []
}
```

The schema is a reporting contract, not a benchmark implementation. CI artifact retention, stable runners, automated workload generation, measured baselines and approved blocking thresholds remain open under M5.4.
