# TODO open-item classification (2026-09-25)

This is a classification, not a claim that all remaining roadmap items are done.
The source of truth remains `TODO.md`; an adapter seam/fixture does not mean a
product/platform acceptance gate has passed.

## Current status counts

At the inventory start this pass scanned 156 `[ ]`/`[~]` lines. After further
local work and reclassification, `TODO.md` currently has 66 unchecked and 85
partial rows (including conditional criteria, future/dated work and rows with a
completed slice plus an open gate). The full first-pass line export is preserved
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
| Maintenance items / §8 | 16 | 9 |

## Locally delivered in this audit pass

| Item | Implementation/evidence |
|---|---|
| Contribution safety | `CONTRIBUTING.md` describes four-job Cargo concurrency and conservative WSL `target/` cleanup. |
| Ignored-test guard | The existing Rust-aware parser has 8 fixtures; the new baseline checker adds add/remove negative fixtures. Live inventory: 434 ignored attrs, 218 bare attrs, zero delta against explicit package/path/function baseline. CI runs parser tests plus live bidirectional check. The baseline does not approve the 218 missing reasons/dates. |
| User-visible brand guard | `check-brand-protocol.py` checks bounded shipped CLI/help/reference-doc text and direct rendered UI source strings, not all source literals; it leaves crate/wire/env/home compatibility identifiers, history, comments and fixtures alone. Mutation tests prove CLI, docs and UI name regressions fail, while legacy compatibility prose passes. |
| Conversation rendering | `react-markdown` and `remark-gfm` render actual Engine transcript text in Web timeline. Raw HTML is not parsed; only in-page anchors, HTTP(S), and mailto become links; external links use `noopener noreferrer`. Playwright desktop and 390×844 flows exercise bold/code/lists, injected DOM mutation, blocked relative/javascript URLs, external link attributes, session switching/reload/archive, mobile multiline composer, real approval-rejection card, and question response. Tool-progress/result, approved mutation/Diff forms and native Desktop cards remain open. |
| Rust/CI gates | Full workspace `fmt`, all-target `check`, strict `clippy` and tests pass locally. Remote run `36094218127` passed all jobs (53m14s Rust job, under 60m); remote historical/peak RSS/disk comparison remains an Actions telemetry-owner prerequisite. New ignore-baseline/brand/Markdown/mutation/approval CI changes in this working patch pass the final local verification log and await remote CI triggered by the next push. |
| Localization | `l10n-guard.sh --before main --after WORKTREE` passed with no regressions/shrinks/fortress breaches. |

## Remaining items with local code surface

These can be developed in bounded work after their behavior is specified; this pass deliberately did not create unapproved product contracts:

- M1: message round grouping, virtual list/scroll anchor/cache, reasoning collapse, approval timeout/rules, client cursor/snapshot catch-up and multi-tab sequencing; tool cards need actual adapter payload/result forms. Markdown rendering is a slice, not the timeline milestone.
- M2: incremental file tree/`@` candidates and a real terminal route. The Engine has one host-configured workspace/Git root; multi-root needs an owner-approved host-owned root ID/map and a root provisioning/trust policy before implementing two-root access.
- M3: settings/workflow views after settings schema and workflow/subagent event/state contracts are selected. The current demo `/ask` and `/approve-tool` prompt hooks are not production workflow execution.
- MT-5: source-by-source reason/date cleanup of the legacy bare ignores. The CI baseline only stops invisible inventory drift.
- MT-6: unwrap/unsafe audits can be done locally, but require small security-reviewed crate batches, measured current numbers, real error behavior and separate tests. None has been bulk-mass-edited here.

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
