# 2026 Q4 ignored test audit

> Status: reviewed on 2026-10-02. Every `#[ignore]` attribute now carries an in-source reason, and CI rejects any attribute that lacks one. The per-family disposition is below. Executing the skipped suites is still not done: 426 of 428 need a built binary, an external tool, or a specific OS capability, and this repository's CI runs none of them (see “CI does not run ignored tests”).

Inventory regenerated on 2026-10-02 with `scripts/ci/ignored-tests.py`; the machine-readable result is `docs/ignored-audit-2026q4.csv`.

## What changed on 2026-10-02

| Measure | Before | After |
| --- | --- | --- |
| `#[ignore]` attributes | 429 | 428 |
| Attributes with no reason string | 218 | 0 |
| Attributes carrying a `YYYY-MM` review date | 37 | 254 |
| Bare attributes approved in `scripts/ci/ignored-tests-baseline.tsv` | 218 | 0 |

The 218 reason-less attributes were filled in from evidence rather than from judgement calls about what the author probably meant. For a test file, the reason names the Cargo target that actually runs it, found by following `#[path]` and `mod x;` declarations transitively from the `tests/<target>.rs` roots — so the reason string is a runnable command, and 147 of the 218 sit in files shared by several targets. The 11 sites not derivable that way were read individually.

One attribute was deleted outright. `xai-grok-shell`'s `session_thread_detects_panic` was run with `--ignored`, passed, and had no recorded reason, so the `#[ignore]` was removed. It now runs in the normal set: `1 passed; 0 failed; 0 ignored`.

One attribute gained a reason from a measured failure. `remote::pull_smoke_test::tests::smoke_push_pull_round_trip` was run with `--ignored` and fails at `pull_smoke_test.rs:33` with `No auth.json — Chaos does not sign in to xAI`. That is the recorded reason: the fork never creates the auth file this smoke test expects, so it needs a seeded OAuth entry before it can be restored.

## Disposition by family

Counts are from the regenerated CSV. Every family's disposition is recorded on the attribute itself; the table is the summary.

| Family | Count | Disposition | Owner | Next review |
| --- | --- | --- | --- | --- |
| Needs a built pager/chaos binary or a PTY session | 362 | Keep ignored; run explicitly per the command in the reason string | project owner (chao2hang) | 2027-01 |
| Fork removed the feature the test asserts (billing/subscription, upstream xAI login and endpoint defaults, connectors URL band, Grove pin backend) | 33 | Keep ignored; the asserted behaviour does not exist in this fork. Restore only if the feature returns | project owner | 2027-01 |
| Manual, soak, or performance measurement | 8 | Keep ignored; prints or measures rather than asserting a verdict | project owner | 2027-01 |
| Flaky under parallel execution, or needs single-process isolation | 7 | Keep ignored; run with `--test-threads=1` as the reason states | project owner | 2027-01 |
| Not a test: subprocess entry point or helper invoked by a parent test | 7 | Keep ignored permanently; these must be registered as `#[test]` only so the binary contains them, and they return early when their env var is unset | project owner | 2027-01 |
| Needs a specific OS capability (Linux cgroupv2 delegation, X11) | 5 | Keep ignored; conditional on the host | project owner | 2027-01 |
| Needs an external language server (`typescript-language-server`, `ROSLYN_DLL`) | 2 | Keep ignored; needs a third-party binary | project owner | 2027-01 |
| Reads the real `$HOME` for user-scope skills | 1 | Keep ignored; host-dependent | project owner | 2027-01 |
| Other single-case gaps (container signal timing, leader-acceptance reconnect window) | 3 | Keep ignored; each reason names the specific defect | project owner | 2027-01 |

## CI does not run ignored tests

Two claims in the previous reason strings were wrong and are corrected:

- 13 attributes said `spawns the real pager binary; CI/Bazel provides PAGER_BINARY`, and one said `PTY e2e; CI runs the ignored pty_e2e suite`. This repository contains **no Bazel files at all** (`git ls-files` matches no `BUILD`, `WORKSPACE`, `MODULE.bazel`, or `*.bzl`), and `.github/workflows/ci.yml` contains **no `--ignored` or `--include-ignored` flag**. GitHub Actions therefore never executes a single ignored test.
- The reason strings now name the command a human must run, and `crates/codegen/xai-grok-pager/Cargo.toml` no longer describes Bazel wiring that does not exist here.

This is why the reason gate exists: if CI never executes a skipped test, the reason string is the only thing that keeps the skip accountable.

## The gate

`scripts/ci/ignored-tests.py --require-reasons` fails on any attribute with no reason string, including `#[ignore = ""]`, and there is deliberately no allow list. `scripts/ci/test-ignored-tests-reasons.py` proves it fails, injecting a bare attribute and an empty-string reason into fixtures and asserting exit 1; it also asserts that a trailing `// comment` survives, that `#[ignore]` inside a doc comment is not counted, and that the real tree passes. Both run in the `rust` job.

`--check-baseline` still runs, against a baseline that is now intentionally empty.

## What this review does not claim

- It does not mean the suites pass. Only three attributes were executed this round: `session_thread_detects_panic` (passed, restored), `smoke_push_pull_round_trip` (failed, reason recorded), and the two candidates checked before choosing. The other 425 were classified from their harness and module documentation, not from a green run.
- Family-level disposition is not per-test functional triage for the 362 binary-dependent cases. Each now names the command that would run it; actually running them is a separate, longer job.
- The 33 fork-gap rows are recorded as intentional, not as passing.
