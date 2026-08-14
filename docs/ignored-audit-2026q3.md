# Ignored Tests Audit — 2026 Q3

> Baseline audit of `#[ignore]` attributes across the workspace, conducted
> as the first execution of the quarterly review process defined in
> `docs/ci-test-debt.md`.

## Method

The `scripts/ci/ignored-tests.sh` script uses gawk-specific syntax that
doesn't run on Windows awk. This audit used ripgrep as a cross-platform
substitute:

```sh
# Total #[ignore] attributes (includes #[ignore = "..."] and bare #[ignore])
rg -c "#\[ignore" crates --type rust

# Bare #[ignore] (no reason)
rg -c "#\[ignore\]" crates --type rust

# With review date (20XX-XX in reason)
rg -c "#\[ignore.*20[0-9]{2}-[0-9]{2}" crates --type rust
```

## Baseline numbers (2026-08-14)

| Metric | Count |
|---|---|
| Total `#[ignore]` attributes | 540 |
| With reason (`#[ignore = "..."]`) | 366 |
| Bare `#[ignore]` (no reason) | 174 |
| With review date (`20XX-XX` in reason) | 0 → 42 (after this audit) |

## By crate

| Crate | Count |
|---|---|
| `xai-grok-pager` | 319 |
| `xai-grok-shell` | 147 |
| `xai-fsnotify` | 22 |
| `xai-grok-tools` | 15 |
| `xai-grok-pager-pty-harness` | 10 |
| `xai-file-utils` | 6 |
| `xai-grok-shared` | 4 |
| `xai-crash-handler` | 2 |
| `xai-grok-sandbox` | 2 |
| `xai-grok-update` | 2 |
| `xai-grok-pager-render` | 2 |
| `xai-grok-workspace` | 2 |
| `xai-chat-state` | 1 |
| `xai-hunk-tracker` | 1 |
| `xai-grok-voice` | 1 |

## Fork-specific debt

The "fork debt" (tests ignored because of Chaos fork divergences from
upstream xAI) was previously tracked at ~87. This audit found **42**
`#[ignore]` attributes with "fork" in the reason string, all of which
were missing a review date.

### Action taken

All 42 fork-specific `#[ignore]` attributes were annotated with
`; review 2026-10` (the next quarterly review window):

| File | Count | Reason |
|---|---|---|
| `xai-grok-pager/src/app/dispatch/tests/billing.rs` | 16 | billing/subscription features removed |
| `xai-grok-shell/src/agent/config.rs` | 14 | upstream xAI defaults removed |
| `xai-grok-pager/src/app/agent_view/modals.rs` | 3 | MANAGED_SECTION_CONNECTORS_URL empty |
| `xai-grok-shell/src/cli_models.rs` | 2 | upstream xAI defaults |
| `xai-grok-shell/src/agent/mvp_agent/tests.rs` | 2 | grok.com login defaults |
| `xai-grok-shell-base/src/util/mod.rs` | 1 | PROD_CLI_CHAT_PROXY_BASE_URL empty |
| `xai-grok-shell/tests/external_auth_expired_credential.rs` | 1 | SSO login flow removed |
| `xai-grok-shell/src/agent/app.rs` | 1 | PRODUCTION_ENDPOINTS blanked |
| `xai-chat-state/src/actor/tests.rs` | 1 | selective-compaction projection gap |

## Remaining work

### Bare `#[ignore]` (174 attributes)

These are mostly upstream inherited debt (PTY e2e tests, scripted
scenarios, platform-specific tests). They predate the fork and are not
Chaos-specific. The `scripts/ci/ignored-tests.sh` script's `--stale`
mode flags these; CI should eventually reject new bare `#[ignore]`.

### `xai-grok-update` wiremock rewrite (48 tests)

These were `#[ignore]`'d when `fetch_gh_release_version` switched from
`gh` CLI to GitHub HTTP API. A wiremock rewrite is in progress
(`docs/ci-test-debt.md` tracks the queue). Priority: `test_concurrent_*`
series (8 tests).

## Comparison to 0.2.137 baseline

The 0.2.137 audit-followup report (`docs/audit-followup-report.md`)
recorded 528 total `#[ignore]`. This audit found 540 — the increase of
12 is from the DSH port (Code Mode / Ralph / Agent Preset tests added in
0.2.138).

## Next review

**2026-10** (October 2026). Steps:
1. Run `scripts/ci/ignored-tests.sh --csv > artifacts/ignored-2026q4.csv`
   (script uses gawk-specific syntax that fails on some awk builds — e.g.
   BSD/macOS and Windows Git-Bash; run on Linux/gawk or port the script
   first. This quarter's baseline was gathered via ripgrep instead:
   `rg -c '#\[ignore' --glob '*.rs'` per crate.)
2. Compare to this document's per-crate table (section "By crate") — the
   CSV snapshot for 2026q3 could not be generated on Windows (see note
   above), so the committed baseline is the table, not a CSV.
3. Review all entries whose `review 2026-10` date has passed:
   - Fixed → remove `#[ignore]`
   - Still needed → push date to `review 2027-01`, add a note
4. Update `docs/ci-test-debt.md` table
5. Open PR: `chore(test): Q4 2026 ignore audit`
