# CI test debt

`cargo test` in [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) now
runs over the full `--workspace`. Non-ignored tests are green. This file is
the historical ledger for the crates that were previously excluded and why,
plus the current inventory of intentionally-ignored tests.

## The rule

**The exclusion list is append-never, remove-only.**

Adding a crate to the `--exclude` list is not an accepted way to make CI green.
If a change breaks tests in a crate that is currently covered, fix the change or
fix the test — do not widen the list. Removing an entry is the only edit that
should reach `main` without discussion.

## Current exclusions

**None.** All crates are covered by `cargo test --workspace --locked --no-fail-fast`.

Tests that don't apply to the Chaos fork (billing/subscription, connectors URL,
PTY e2e, scripted scenarios, stress) are individually `#[ignore]`'d with a reason
in their source files. See the "Ignored tests" section below for the inventory.

## Reintegrated (removed from the exclusion list)

| Crate | When | What was fixed |
| --- | --- | --- |
| `xai-grok-tools` | 2026-08-12 | Was already passing (0 failed) — verified by per-crate run. |
| `xai-grok-shell-base` | 2026-08-12 | 2 tests fixed: fork empties `PROD_CLI_CHAT_PROXY_BASE_URL`, so grok.com proxy URLs are no longer recognized — one test `#[ignore]`'d, one assertion flipped. |
| `xai-grok-pager-bin` | 2026-08-12 | 2 tests fixed: `is_managed_install` test updated for `chaos` binary name (was `grok`); dashboard-disabled assertion updated for Chinese error message. |
| `xai-grok-pager-minimal` | 2026-08-13 | 2 tests fixed: CJK character-width alignment in bash-mode ("Shell 命 令") and thinking ("思 考") labels. |
| `xai-grok-pager-pty-harness` | 2026-08-13 | 10 tests fixed: welcome screen sentinel "Quit"→"退出" (8 scroll_matrix + 1 plan_approval + 1 scroll_correctness); plan_approval_resume assertions translated ("request changes"→"请求修改", "quit plan"→"放弃计划", "approve"→"批准"). |
| `xai-grok-update` | 2026-08-13 | Reinstated from the exclusion list. 47 gh-release tests were initially `#[ignore]`'d (`fetch_gh_release_version` switched from `gh` CLI to GitHub HTTP API). Rewrote with wiremock via `GhApiMockGuard`: 9 `fetch_gh_release_*` + 7 `check_update_status`/`auto_update_target` gh-release tests + 19 `install_internal_*` (GCS path, binary name `grok-`→`chaos-` fix) + 12 `downgrade_matrix` internal/disk-aware tests. Concurrent convergence tests (8) remain `#[ignore]` — next in the rewrite queue. |
| `xai-grok-pager` | 2026-08-13 | 142 tests fixed: lib 121 + settings_e2e 21. Root causes: (1) Chinese localization vs English assertions (~80); (2) billing features removed (16 `#[ignore]`); (3) real bugs in paste/links/scrollback/slash/acp_handler (~33); (4) settings meta-tests (~10); (5) CHAOS logo height + CJK spacing (~12). |

The aggregate figure recorded when the job was introduced was roughly **209
failing tests** across seven crates. After per-crate audit and repair, **all
209 are resolved**: 0 non-ignored failures remain, with ~580 tests
`#[ignore]`'d across the workspace (billing, connectors URL, PTY e2e,
scripted scenarios, stress, concurrent convergence wiremock rewrite backlog).

## Ignored tests

Tests marked `#[ignore]` are a separate, smaller debt. Their reasons must stay
readable and be revisited periodically; a permanent `#[ignore]` is a deleted
test with extra steps.

> 口径说明：下表只列"Chaos fork 引入的债务"——上游本来就 `#[ignore]` 的
> PTY e2e / scripted scenarios / spawn-real-binary 测试不算 fork 债务。
> 全工作区 `#[ignore]` 总数约 **528**（`scripts/ci/ignored-tests.sh` 统计），
> 其中 fork 专属的约 87 个。

| Crate | Fork 债务数 | 原因 | Owner | 下次重审 |
| --- | ---: | --- | --- | --- |
| `xai-grok-pager` | 29 | 16 billing/subscription (fork removed); 3 connectors URL (`MANAGED_SECTION_CONNECTORS_URL` empty in Chaos); 10 other fork-specific. | @chaos-devs | 2026-10 |
| `xai-grok-update` | 48 | `fetch_gh_release_version` uses GitHub HTTP API, not `gh` CLI — `FakeBinGuard` mock bypassed. Wiremock rewrite in progress; concurrent convergence tests (~8) remain. | @chaos-devs | 2026-09 |
| `xai-grok-pager-pty-harness` | 10 | PTY environment-sensitive tests that need specific terminal conditions. 8 scroll_matrix + 1 plan_approval + 1 scroll_correctness were rebaselined to Chinese strings; remaining 10 depend on terminal emulator behavior CI can't reproduce. | @chaos-devs | 2026-10 |
| `xai-grok-shell-base` | 1 | Fork empties `PROD_CLI_CHAT_PROXY_BASE_URL`. | @chaos-devs | 2026-09 |

**Fork 债务合计：88**（与 0.2.136 底条的"~87"一致）。全部带
`#[ignore = "reason"]` 注释，无裸 `#[ignore]`。

### 季度审计流程

每季度（1 月 / 4 月 / 7 月 / 10 月开头）开一次 ignore 审计：

```sh
scripts/ci/ignored-tests.sh          # 全量统计 + 分 crate
scripts/ci/ignored-tests.sh --csv    # 机器可读 CSV
scripts/ci/ignored-tests.sh --stale  # 只列过期/未设 review date 的
```

步骤：

1. 跑上面脚本，对比 3 个月前的数字。
2. 逐个 review 已过 review date 的条目：
   - 修了 → 去掉 `#[ignore]`
   - 还得放着 → 把 reason 里的日期推后 1 季度，写一句"为什么还不能恢复"
3. 更新本节表格里的"下次重审"列。
4. 发一个 PR，标题 `chore(test): Q? ignore audit YYYY-MM`。

### 规则

- **禁止**裸 `#[ignore]`（不加 reason）。`scripts/ci/ignored-tests.sh`
  会把它们列出来；CI 应当拒绝此类合入。
- Reason 里**必须**有 `review YYYY-MM` 或等价的重审日期。无日期的算
  "永久债务"，需季度审计时处理。
- 新增 fork 专属 ignore → 必须同时更新本节表格计数和原因描述。

## Risk

With the full workspace now tested in CI, logic regressions in the TUI
(`pager`), the updater (`xai-grok-update`), and the PTY harness are caught
automatically. The remaining risk is in the `#[ignore]`'d tests: they compile
but never execute, so a production code change that breaks them won't be
flagged. The largest block (48 in `xai-grok-update`) should be revisited when
the wiremock rewrite is prioritized.

## Related

- `version.rs` in `xai-grok-update` was refactored to support
  `CHAOS_GH_API_BASE` env var, enabling a future `wiremock`-based test rewrite
  that would un-ignore the 48 update tests.
