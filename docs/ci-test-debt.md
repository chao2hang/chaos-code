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
| `xai-grok-pager` | 19 | 16 billing/subscription (fork removed); 3 connectors URL (`MANAGED_SECTION_CONNECTORS_URL` empty in Chaos). | @chaos-devs | 2026-10 |
| `xai-grok-shell` | 28 | agent/config 等上游 xAI 默认值；cli_models 默认值；grok.com 登录；app.rs `PRODUCTION_ENDPOINTS`；external_auth SSO；远程 settings 抓取路径（BYOK 下不可达，见下节）。 | @chaos-devs | 2026-10 |
| `xai-grok-shell-base` | 1 | Fork empties `PROD_CLI_CHAT_PROXY_BASE_URL`. | @chaos-devs | 2026-10 |
| `xai-chat-state` | 1 | Pre-existing fork gap: selective-compaction projection. | @chaos-devs | 2026-10 |
| `xai-grok-update` | 5 | `fetch_gh_release_version` 用 GitHub HTTP API 而非 `gh` CLI；并发收敛类用例等 wiremock 重写。 | @chaos-devs | 2026-09 |
| `xai-fast-worktree` | 2 | Grove pin 后端缺失（`pin_exists` 恒 `Ok(false)`、`delete_pin_ref_gated` 拒绝），pin 剪除类用例无法运行。 | @chaos-devs | 2026-10 |

**Fork 债务合计：56**（2026-09-22 复核修正）。全部带
`#[ignore = "reason; review YYYY-MM"]` 注释，review date 已补全到
`2026-10`。

## 2026-09-22：本地全量基线清账

同步到 `SOURCE_REV 72a61251` 后跑本地全量 `cargo test --workspace`，得到
**46 条失败基线**。逐条查明后全部处置完毕，按性质分为六类（每类一个提交）：

| 类别 | 条数 | 处置 |
| --- | ---: | --- |
| 机械性过期期望（与 fork 无关，实现改了用例没跟） | 9 | 改断言，不动实现 |
| 文案漏在英文（该中文的地方没中文） | 2 | 改成中文并钉住常量 |
| 远程抓取默认关导致的行为缺陷 | 3 | **改实现**（含一条跨身份判决泄漏，见提交 `ab61a53e`） |
| 上游未跑到的真实 git bug（`checkout -b … --end-of-options`） | 2 | **改实现** + 用例按真实契约重写 |
| 分叉语义用例（前提是上游云端形态） | 8 | 能钉 fork 契约的改写，其余 3 条 `#[ignore]` |
| 进程级全局态竞争 / 缺 Grove 后端的用例 | 10 | 加互斥锁；pin 存活断言改写，2 条 `#[ignore]` |

### 本轮新增的 ignore 与丢失的覆盖面

上表计数已按 2026-09-22 实测重新核对（此前几轮同步新增的 ignore 未回填本表，
一并补齐：shell 20→28、update 1→5）。**本轮清账显式新增 5 条**：

- `xai-grok-shell`（远程 settings 抓取路径，BYOK 下不可达）：
  `post_auth_settings_non_xai_keeps_local_but_still_emits`、
  `post_auth_settings_failure_resolves_gate_onto_local_policy`、
  `settings_self_heal_refetches_after_token_rotation`。
  **代价**：settings 抓取成功与失败两条分支、以及 401 令牌轮换自愈的用例
  覆盖被移除。恢复条件：fork 提供可观测的抓取后端（或在集成测试里用独立进程
  打开 `features.remote_fetch`，`tests/common/mod.rs` 的启动预取桩已按后者做）。
- `xai-fast-worktree`（缺 Grove pin 后端）：
  `nfs::liveness::tests::aborted_partial_removal_prunes_after_grace`、
  `api::gc::tests::run_pass_prunes_orphan_grove_pins_after_grace`。
  **代价**：pin 超过宽限期后被真正剪除的用例覆盖被移除；同一文件里其余 3 条
  pin 用例已改为断言可验证的契约（`git cat-file` 仍能读到被 pin 保护的提交、
  孤儿不被剪、在飞创建不被回收）。恢复条件：`nfs::liveness::pin_exists` 接上
  真实 ref 读取器（`gix` 已在依赖里），届时同时恢复 `delete_pin_ref_gated`。

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
