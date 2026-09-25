# TODO open-item classification (2026-09-25)

This is a classification, not a claim that all remaining roadmap items are done.
The source of truth remains `TODO.md`; an adapter seam/fixture does not mean a
product/platform acceptance gate has passed.

## Current status counts

The current line-level inventory scans 151 `[ ]`/`[~]` rows: 64 unchecked and 87
partial (including conditional criteria, future/dated work and rows with a
completed slice plus an open gate). The current open/partial split for
maintenance items reflects the MT-6 review/evidence partial rows. The reproducible command is
`python3 scripts/ci/classify-open-todos.py`; its latest output is preserved in
private goal scratch `open-items-current.tsv`. The initial audit export remains
in private goal scratch `todo-open-items.txt`.

| Roadmap group | Unchecked | Partial |
|---|---:|---:|
| M-1 | 1 | 5 |
| M0 | 0 | 13 |
| M1 | 1 | 15 |
| M2 | 0 | 24 |
| M3 | 6 | 7 |
| M4 | 24 | 3 |
| M5 | 18 | 9 |
| Maintenance items / §8 | 14 | 11 |

## Locally delivered in this audit pass

| Item | Implementation/evidence |
|---|---|
| Contribution safety | `CONTRIBUTING.md` describes four-job Cargo concurrency and conservative WSL `target/` cleanup. |
| Ignored-test guard | The existing Rust-aware parser has 8 fixtures; the new baseline checker adds add/remove negative fixtures. Live inventory: 434 ignored attrs, 218 bare attrs, zero delta against explicit package/path/function baseline. CI runs parser tests plus live bidirectional check. The baseline does not approve the 218 missing reasons/dates; `docs/ci-test-debt.md` and the Q4 summary now point to the repaired scanner and preserve the owner review as open. |
| User-visible brand guard | `check-brand-protocol.py` checks bounded shipped CLI/help/reference-doc text and direct rendered UI source strings, not all source literals; it leaves crate/wire/env/home compatibility identifiers, history, comments and fixtures alone. Mutation tests prove CLI, docs and UI name regressions fail, while legacy compatibility prose passes. |
| Conversation rendering and approvals | `react-markdown` and `remark-gfm` render actual Engine transcript text in Web timeline. Raw HTML is not parsed; Markdown images render only inert alt text with no image node or network request; only in-page anchors, HTTP(S), and mailto become links; external links use `noopener noreferrer`. Playwright desktop and 390×844 flows exercise bold/code/lists, injected DOM mutation, blocked relative/javascript URLs, image request suppression, external link attributes, session switching/reload/archive, mobile multiline composer, demo approval rejection, allow followed by a visible missing-ToolAdapter failure, and question response, with status exposed through a polite live region. `/approve-tool` is a deterministic demo protocol, not production tool execution. Tool-progress/result cards, approved mutation/Diff forms and native Desktop cards remain open. |
| Rust/CI gates | Full workspace `fmt`, all-target `check`, strict `clippy` and tests passed locally. Remote run `36133400667` passed all jobs (42m33s Rust job, under 60m), including the approval-outcome browser path. Subsequent CI `36165469964` exposed `xai-grok-shell` current-thread actor test stack overflows with the Rust test harness default stack. Raising `RUST_MIN_STACK` in the CI test step to 16 MiB fixes the package's full library suite (6,804 passed), the full workspace tests locally, and GitHub CI run `36178108811` (Rust job 33m07s, under 60m). Docs-only CI run `36181915268` surfaced a pager history-daemon fixture that exceeded its fixed 10s deadline under remote load; isolated test passes locally, and after raising the finite polling budget to 60s, final GitHub run `36186580928` passed every CI job. Historical/peak RSS/disk comparison remains an Actions telemetry-owner prerequisite. Local ignore-baseline/brand/Markdown/mutation/approval gates passed in `final-verification-round3.log`. |
| Localization | `l10n-guard.sh --before main --after WORKTREE` passed with no regressions/shrinks/fortress breaches. |

## Remaining items with local code surface

These can be developed in bounded work after their behavior is specified; this pass deliberately did not create unapproved product contracts:

- M1: message round grouping, virtual list/scroll anchor/cache, reasoning collapse, approval timeout/rules, client cursor/snapshot catch-up and multi-tab sequencing; tool cards need actual adapter payload/result forms. Composer Enter/IME guards and history projection are tested, including no history navigation during IME composition; browser-native IME candidate integration still depends on platform keyboard testing. Markdown rendering is a slice, not the timeline milestone.
- M2: incremental file tree/`@` candidates and a real terminal route. The Engine has one host-configured workspace/Git root; multi-root needs an owner-approved host-owned root ID/map and a root provisioning/trust policy before implementing two-root access.
- M3: settings/workflow views after settings schema and workflow/subagent event/state contracts are selected. The current demo `/ask` and `/approve-tool` prompt hooks are not production workflow execution.
- MT-5: source-by-source reason/date cleanup of the legacy bare ignores. The CI baseline only stops invisible inventory drift.
- MT-6: a single `xai-grok-update` network retry-loop unwrap was replaced with a structured fallback and is covered by the existing connection-refused integration test. Remaining update progress-template unwraps and sandbox unwrap/unsafe sites are not bulk-edited; the old audit counts need fresh AST-aware classification and security review.

## External, decision, and schedule gates

| Area | Required prerequisite |
|---|---|
| M-1.5 / M4 SSH | Maintainer chooses candidate dependency, auth scope and ProxyCommand arbitrary-execution trust rule; controlled SSH host/key/agent/forwarding test runner must be approved. Local endpoint types do not exercise transport. |
| Tauri and platform support | Tauri/WebView integration plus macOS/Windows runners, packaging/signing/notarization resources, real native entry-flow acceptance. Linux browser automation is Web only. |
| Provider/keyring/MCP/plugin execution | Keyring/security ADR, approved extension origin/signature and approval policy, disposable credentialed live endpoint/registry. Shape tests reporting `network_not_attempted` are not connection proof. |
| Multi-root/NFS/storage faults | Host-owned path lifecycle/product decision and controllable second root, independent process/network filesystem/disk fault harness. Browser cannot nominate roots. |
| Release/npm | Package owner resolves Windows npm placeholder names; release owner provisions matching signing secrets/assets and Windows runner. Do not create a new release tag to bypass this gate. |
| M-1.6 resource isolation | Latest Rust CI finished under the 60-minute budget, but historical before/after baseline, peak RSS and disk delta require Actions telemetry access. |
| WSL issue | Windows host, WSL version and prior boot logs are missing from this Linux runner. Report submission/retention belongs to the issue owner. |
| Quarterly audit/upstream decisions | 2026-10 ignore review is not due yet. Dependency/MCP-admission deferrals and `telemetry status` draft require the next owner/security review; don't decide silently. |
| M5 release/performance/manual review | Supported packaging matrix, benchmark hardware/data/threshold and release go/no-go owner require prior approval/resources. |

The open checklist intentionally remains open for these conditions. Counts are
status lines rather than distinct defects, and partial rows can contain several
separate outcomes.
