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
| `xai-grok-update` | 2026-08-13 | Reinstated from the exclusion list. 47 gh-release tests were initially `#[ignore]`'d (`fetch_gh_release_version` switched from `gh` CLI to GitHub HTTP API). Rewrote with wiremock via `GhApiMockGuard`: 9 `fetch_gh_release_*` + 7 `check_update_status`/`auto_update_target` gh-release tests + 19 `install_internal_*` (GCS path, binary name `grok-`→`chaos-` fix) + 12 `downgrade_matrix` internal/disk-aware tests. This is a historical rewrite account: the 2026-10-01 scanner reports one updater `#[ignore]` attribute, the opt-in 100k stress test; five stale channel-parameterized URL expectations were replaced with running Chaos installer-contract tests. |
| `xai-grok-pager` | 2026-08-13 | 142 tests fixed: lib 121 + settings_e2e 21. Root causes: (1) Chinese localization vs English assertions (~80); (2) billing features removed (16 `#[ignore]`); (3) real bugs in paste/links/scrollback/slash/acp_handler (~33); (4) settings meta-tests (~10); (5) CHAOS logo height + CJK spacing (~12). |

The aggregate figure recorded when the job was introduced was roughly **209
failing tests** across seven crates. After per-crate audit and repair, **all
209 are resolved**: 0 non-ignored failures remain. The 2026-10-02
inventory reports 428 ignored attributes total and **0 bare attributes**;
the 2026-10-03 rescan reports **429** and still **0 bare** — the single addition
is a subprocess entry point a parent test drives via `--exact --ignored`, so it
joins the "not a test" family rather than changing any disposition
(`docs/ignored-audit-2026q4-summary.md`, "Rescan on 2026-10-03"). Either figure
is a workspace-wide scanner count, not the count of fork-specific entries
in the table above.

## Ignored tests

Tests marked `#[ignore]` are a separate debt. Their reasons must stay
readable and be revisited periodically; a permanent `#[ignore]` is a deleted
test with extra steps. `python3 scripts/ci/ignored-tests.py --check-baseline
scripts/ci/ignored-tests-baseline.tsv` checks the grandfathered bare-attribute
inventory in both directions; that baseline is now intentionally empty. The live
scan of 2026-10-03 reports 429 ignored attributes and 0 bare attributes
(428 on 2026-10-02; the CSV in `docs/ignored-audit-2026q4.csv` was regenerated
from the newer scan), and
`--require-reasons` fails CI if any attribute loses its reason. The checked-in
CSV and per-source owner audit still determine review status. The Q4 CSV currently contains one
`xai-grok-update` ignored attribute: the opt-in 100k stress test. Five tests
that asserted upstream installation URLs were replaced with running tests of
the Chaos fork's actual `reinstall_hint` behavior; this source-level correction
does not constitute the Q4 owner/reviewer audit. The remaining updater stress ignore has a runnable `scripts/test-blitz-stress.sh` entry point; that script's real test passed at 120 iterations. The full default 100k run completed on 2026-10-01: 1 passed, 0 failed in 4306.69 seconds through `scripts/test-blitz-stress.sh` (not-retained log `blitz-stress-100k.log`). Test completion does not substitute for the Q4 source-by-source owner/reviewer audit. CI runs the repository inventory scanner against
`scripts/ci/ignored-tests-baseline.tsv`; existing bare attributes are grandfathered
by explicit package/path/function keys. Added attributes fail until reviewed and
added to the baseline; stale entries for removed attributes also fail until the
baseline is cleaned. The baseline does not approve reasons or replace the
quarterly source-level audit. Current bare-ignore additions are rejected by the
explicit package/path/function inventory gate; removing an existing ignored test
requires removing its matching stale baseline key in the same change.

> 口径说明：下表只列“Chaos fork 引入的债务”；全工作区清单见
> [`ignored-audit-2026q4-summary.md`](ignored-audit-2026q4-summary.md)。初次统计工具输出并非可靠 CSV，
> 解析逻辑还把行尾注释误判为 reason；之前记录的 452 / 228 / 49 / 403 数字撤回，不能用于治理。
> 当前修复后的可信快照和扫描口径见 `ignored-audit-2026q4-summary.md`。

> 口径说明：下表只列"Chaos fork 引入的债务"；全工作区清单见
> [`ignored-audit-2026q4-summary.md`](ignored-audit-2026q4-summary.md)。初次统计工具输出并非可靠 CSV，
> 解析逻辑还把行尾注释误判为 reason；之前记录的 452 / 228 / 49 / 403 数字撤回，不能用于治理。
> 当前修复后的可信快照和扫描口径见 `ignored-audit-2026q4-summary.md`。

| Crate | Fork 债务数 | 原因 | Owner | 下次重审 |
| --- | ---: | --- | --- | --- |
| `xai-grok-pager` | 19 | 16 billing/subscription (fork removed); 3 connectors URL (`MANAGED_SECTION_CONNECTORS_URL` empty in Chaos). | @chaos-devs | 2026-10 |
| `xai-grok-shell` | 28 | agent/config 等上游 xAI 默认值；cli_models 默认值；grok.com 登录；app.rs `PRODUCTION_ENDPOINTS`；external_auth SSO；远程 settings 抓取路径（BYOK 下不可达，见下节）。 | @chaos-devs | 2026-10 |
| `xai-grok-shell-base` | 1 | Fork empties `PROD_CLI_CHAT_PROXY_BASE_URL`. | @chaos-devs | 2026-10 |
| `xai-chat-state` | 1 | Pre-existing fork gap: selective-compaction projection. | @chaos-devs | 2026-10 |
| `xai-grok-update` | 0 | Five stale upstream-URL expectations now exercise the Chaos fork's real installer hint; the remaining 100k stress test is not a fork-specific exclusion. | @chaos-devs | 2026-10 |
| `xai-fast-worktree` | 2 | Grove pin 后端缺失（`pin_exists` 恒 `Ok(false)`、`delete_pin_ref_gated` 拒绝），pin 剪除类用例无法运行。 | @chaos-devs | 2026-10 |

**Fork 债务合计：51**（2026-10-01 清除五条过期 updater installer-URL ignored expectations 后重算）。现有 fork-specific ignored tests 带有 review reason/date；Q4 是否续期或恢复仍需各 owner/reviewer 逐条决定。

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
python3 scripts/ci/ignored-tests.py --csv > docs/ignored-audit-2026q4.csv
python3 scripts/ci/ignored-tests.py --check-baseline scripts/ci/ignored-tests-baseline.tsv
python3 scripts/ci/test-ignored-tests-baseline-fixture.py
```

步骤：

1. 跑上面脚本，对比 3 个月前的数字。
2. 逐个 review 已过 review date 的条目：
   - 修了 → 去掉 `#[ignore]`
   - 还得放着 → 把 reason 里的日期推后 1 季度，写一句"为什么还不能恢复"
3. 更新本节表格里的"下次重审"列。
4. 发一个 PR，标题 `chore(test): Q? ignore audit YYYY-MM`。

### 规则

- 目标是**禁止未经审查新增**裸 `#[ignore]`。2026-10-02 存量 218 条已全部就地补上 reason（理由取自该测试所属 Cargo target 与 harness，不是手写猜测），baseline 因此清空；`--require-reasons` 门禁无豁免表，缺 reason 的属性（含 `#[ignore = ""]`）直接让 CI 失败。新增、删除仍需审查 baseline diff。
- 对带 reason 的 `#[ignore]`，Reason 里**必须**有 `review YYYY-MM` 或等价的重审日期。无日期的算
  “永久债务”，需季度审计时处理。裸属性已不是合法例外：CI 不传 `--ignored`，被跳过的测试在 CI 里完全不可见，reason 字符串是唯一让跳过行为可追溯的东西。
- 新增 fork 专属 ignore → 必须同时更新本节表格计数和原因描述。

2026 Q4 source-by-source owner audit is due as of 2026-10-01 and remains pending;
no source-by-source owner/reviewer decisions are recorded in this inventory refresh.
The parser is repaired and covered by fixtures, and the baseline gate prevents silent
inventory drift; neither result is an approval or renewal of the existing ignore debt.
See [`ignored-audit-2026q4-summary.md`](ignored-audit-2026q4-summary.md).

## 2026-09-22：一组「从不执行」的守护测试（已接回）

`xai-grok-shell` 的 `config_docs` 模块把提交进仓库的配置参考页
（`crates/codegen/xai-grok-pager/docs/user-guide/26-config-reference.md`）与实时
注册表逐行对齐：每个 `FEATURES` 键必须在页面上有行、Requirements 必须是
`pin`、Managed 列必须与 `MANAGED_WINS_OVER_USER` 一致、MCP 已知字段与
`telemetry.otel_*` 必须齐全、Details 不得泄漏内部系统名。

它在 `lib.rs` 里写作 `#[cfg(all(test, feature = "config-docs"))]`，而
`config-docs` 不在 cargo `default` 里：上游把打开它的责任交给内部 bazel 的
`default-bazel` 集合，公开树没有对应机制，本仓库也没有任何依赖边打开它。于是
这组守护在本仓库从未编译过——`cargo test -p xai-grok-shell --lib` 是
`running 0 tests`，`--workspace` 同样跑不到——模块文档里「CI fails when …」的
承诺是空的。`features.feedback` 与 `features.two_pass_compaction` 的默认值在
页面上反着写了很久，没有任何东西会响。

已处理（`91eadcfe`）：

- 中文化之后必然失败的两条英文断言（页面标题 `# Configuration reference`、
  英文表头）改成中文页面的实际形态；这等于原先在钉「这页还没翻」。
- 同一用例读 `crates/codegen/xai-grok-shell/AGENTS.md`，该文件既不在本分叉
  也不在上游公开树里，`find_monorepo_root().join(...)` 之后 `read_to_string`
  直接 panic；改成「文件存在才断言」。
- 新增 `feature_rows_state_the_registry_default`，逐行把 `FEATURES` 里 19 个
  特性写出的 `默认 true|false` 与 `default_enabled` 对齐，凡出现「默认」的行
  都必须能解析成该形态（避免用例空转）。
- 在 `crates/codegen/xai-grok-pager/Cargo.toml` 的 dev-dependency 边上打开
  `config-docs`（上游同一行已有同型做法：给 `xai-grok-shell` 开 `test-support`），
  于是 `cargo test --workspace` 会跑到它，`cargo check/clippy --all-targets`
  也会编译它。`.github/workflows/ci.yml` 未改动。
- 变异验证：把 `features.dock` 改成「默认 true」，新用例报
  `features.dock says 默认 true, but registered default_enabled is false`。

### 同类残留：还有哪些 `#[cfg(all(test, feature = …))]` 不编译

`scripts/ci/ignored-tests.py` 数的是 `#[ignore]`，「根本没编译」的测试不在它的
视野里。按「模块特性 vs 有没有依赖边打开」对全仓扫一遍（2026-09-22，
`grep -rn 'cfg(all(test, feature'` 对照各 `Cargo.toml` 的 `features = [...]`）：

| 模块 | 特性 | 是否有边打开 | 状态 |
|---|---|---|---|
| `xai-grok-shell::config_docs` | `config-docs` | 本轮补上（pager dev-dep） | 已接回 |
| `xai-grok-pager::app::acp_handler::permissions` 的 `local-workspace` 用例 | `local-workspace` | 否（只在 pager/pager-bin 的 `default-bazel` 里） | **仍未编译** |
| `xai-grok-pager::views::welcome::workspace_mode` 的 `local-workspace` 用例 | `local-workspace` | 同上 | **仍未编译** |
| `xai-fast-worktree`（`api.rs`/`git/mod.rs`） | `metadata` | 是（shell / workspace / pager 的依赖边） | 正常 |
| `xai-computer-hub-sdk::metrics` | `metrics` | 是（workspace / workspace-daemon） | 正常 |
| `xai-grok-voice::pipeline` | `audio` | 是（pager 的依赖边） | 正常 |
| `xai-grok-tools::notification::types` | `serde` | 是（tools 的 `default`） | 正常 |
| `xai-grok-sandbox::read_deny_verify` | `enforce` | 是（pager 的 `sandbox-enforce` 默认） | 正常 |
| `xai-grok-pager-bin::main`（jemalloc） | `jemalloc` | 是（pager-bin 的 `default`） | 正常 |
| `xai-grok-shell`（loom）、`xai-grok-shell`（dhat-heap）、`xai-grok-announcements::bindings_export` | `loom` / `dhat-heap` / `ts` | 否，**有意**（各自 `Cargo.toml` 写了手跑命令或 generate.sh） | 非债务 |

剩下那两条 `local-workspace` 用例**不**照抄本轮的接法：`local-workspace` 是一个
真实功能开关（workspace_server 的 own/attach 路径），不是纯测试门闩，打开它会让
整个 lib 测试构建的编译面变化，可能把「该功能默认关闭」的断言一起掀翻。要么
先确认打开后全绿再接，要么把这两条用例改成不依赖该特性的契约断言。

**教训**：新增 `#[cfg(all(test, feature = …))]` 的测试模块时，必须同时说明
「谁打开这个特性」，否则它和删掉没有区别。

## 2026-09-22：全量 test 暴露的五类遗留失败（已修）

`cargo test --workspace --no-fail-fast` 与随后的复现实验找出五类问题，都不是
本轮改动引入的：三个文件在本分叉与 `SOURCE_REV` 逐字节相同，第四类是上游
`75810042` 已经修过、本分叉还停在旧写法上，第五类在改动前后同样比例地出现（见下）。

| 用例 | 表现 | 成因 | 修法 |
|---|---|---|---|
| `acp_session_tests::auto_wake_suppression_tests` 的两条 | 确定性（3/3 复现） | 用「资源里没有这条状态」当「没报告过」，而提醒流水线的任何一次工具调用都会经 `get_or_default` 把空状态建出来 | 采纳上游 `75810042` 的只读 `is_reported`，断言换成只读探针 |
| `prompt_queue_actor_tests::drain_at_safe_point_with_steer_off_does_not_promote_held_row` | 偶发（全量跑 2/3 次挂 1 次） | steer 缓存是进程全局的，同文件另两条用例会写它 | 两个写入者补 `#[serial_test::serial]` |
| `session_search::{bootstrap::tests::test_claimant_reindexes_even_when_marker_exists, manager::tests::test_recheck_bootstrap_reruns_reindex_when_marker_missing}` | 偶发（加压后 4/120） | `recovery::CACHE_EPOCH` 是进程全局的，同二进制里别的用例 heal 自己的缓存会把它自增 | 断言容忍外来 heal，写法照抄同文件 `test_concurrent_gates_single_flight` |
| `auth::manager::lock::tests::dropping_the_guard_silences_the_heartbeat_before_anyone_else_can_hold_the_lock` | 全量 6819 条里挂过 1 次（`lock_tests.rs:491` `WouldBlock`） | 别的用例 `Command::spawn` 时 fork 把当刻开着的锁文件 dup 带进子进程，那份 dup 压着 flock 到 exec | 抢锁改成有界重试；真泄漏仍会超时失败 |
| `telemetry::span_profile::tests::nested_timer_folds_under_parent_without_enter` | 偶发且**整条测试二进制 SIGABRT**（单跑 8 次挂 2 次；70 次挂 3 次） | `InstrumentationTimer` 找不到线程内父跨度时回退到进程全局的 `startup::current_phase_span()`，而那份 span 属于另一个用例的线程局部 subscriber，克隆进自己的 registry 即 panic，析构再踩污染锁 → 非展开 panic → abort | `span_profile` 的夹具拿 `startup` 用例那把 `SERIAL` 锁，两族互斥；另给相邻的覆盖断言加 1ms 采样偏斜界 |

### 一：`is_none()` 不是「没报告过」

两条用例都写 `resources.get::<State<ReportedTaskCompletions>>().is_none()`，注释说
这是「拒绝入队不得报告」「入队本身不算报告」。问题是提醒流水线上**任何一次**工具
调用都会经 `get_or_default` 把这条状态建出来（空集），于是「容器不存在」与
「没有报过」是两件事：用例断言的是前者，想说的是后者。

同文件里那个 `already_reported` 帮手更值得记一笔：它靠
`!reported.mark_reported(task_id)` 回答，**问谁就标记谁**。于是等待「本轮把它标记
了」的轮询第一次就自证成功，而断言「它没被标记」的用例在提问的瞬间把答案改了。

修法与上游 `75810042` 一致：`ReportedTaskCompletions::is_reported` 只读访问器
＋ 只读探针 ＋ 断言 `!already_reported(&actor, …)`。

### 二：进程全局的 steer 缓存

`drain_at_safe_point_with_steer_off_does_not_promote_held_row` 读 steer 缓存，
而 `promote_queued_as_interjections_skips_auto_wake`、
`drain_at_safe_point_with_steer_on_leaves_protected_row_queued` 会写它。同文件里
它们的孪生用例早就带了 `#[serial_test::serial]`（本分叉上一轮补的），这两条漏了。
补齐后全量跑不再复现。

### 三：`CACHE_EPOCH` 是进程全局的

`reindex_all` 在收尾时检查 `epoch.changed()`：只要进程里发生过一次
`heal_unusable` 的「隔离并重建」，它就**按契约**扣下完成标记并返回 `RunAgain`
（标记留给下一次重跑写）。而同一测试二进制里
`bootstrap_tests::test_shared_index_reopens_after_epoch_change` 与
`recovery::tests::heal_quarantines_only_on_confirmed_corruption` 各自会在自己的
tmpdir 上触发一次自增。两条用例却把「reindex 写完了标记」当确定事实。

证据（诊断塞在 `reindex_all` 的每个扣标记分支里，20 个 `yes` 压满 CPU，直接跑
测试二进制 120 次）：4 次失败，且每次都打印
`DIAG withhold reason=cache_healed_during_reindex` 与
`DIAG claimant epoch 0 -> 1 outcome RunAgain marker Some("123")`；
`reason=claim_lost`、`reason=index_replaced` 各 0 次，即机制只有 epoch 一条。
另有 13/120 次落在别的不做标记断言的用例窗口里，因此静默通过。

修法沿用同文件已有的写法（`test_concurrent_gates_single_flight` 就是先取
`epoch_before`，再用 `healed || read_marker(…)` 容忍外来 heal）：先取
`epoch_before`，把「被外来 heal 打断」当成合法分支。**不**改成让用例自己重跑：
`RunAgain` 的合同是把重跑交还给调用方（`manager::handle_job` 会
`bootstrap_once` 重新入队），直接调 `bootstrap_with_lease_inner` 的用例没有这个
循环，替它补一个等于替生产代码做决定。修后同条件 150 次全绿。

### 四：fork 会把 flock 的 dup 带进子进程

失败了 1 次的那条用例在 `drop(AuthFileLock{..})` 之后立刻重新打开同一个锁文件并
`try_lock_exclusive()`。先排除了「`join()` 不保证关掉心跳线程那份 dup」：把该
模式（`try_clone` → 线程持有 → signal → `join` → drop → 再抢锁）单独跑 3000 次，
0 失败。再把 fork→exec 窗口人为拉长（`Command::pre_exec` 里睡 400ms）后**稳定
复现**：

```
right after the parent dropped its fd: Some("WouldBlock")
after the child exec'd:               None
```

flock 挂在打开文件描述上，`fork` 出来的子进程带着当刻开着的锁文件 dup；父进程
关掉自己的 fd 之后，子进程那份仍然压着锁，直到它 `exec` 触发 CLOEXEC。同一测试
二进制里 `auth::manager::lock::tests::subprocess_lock_holder` 那一组正是 spawn
大户，所以这条只在全量并行时偶发。

修法：把「重新抢到锁」当成用例的**前置条件**重试（5s，10ms 轮询）。真泄漏
（心跳线程没死、或某个子进程卡在 exec 前）仍会走到超时 panic；用例真正要钉的
属性——30ms 后文件里还是 `sentinel`——没有放宽。

### 五：跨测试的 `tracing` 注册表

`xai-grok-telemetry` 的 `span_profile::tests::nested_timer_folds_under_parent_without_enter`
全量跑时偶发，而且**整条测试二进制 SIGABRT**——同一二进制里其余 275 条结果一起丢：

```
thread 'span_profile::tests::nested_timer_folds_under_parent_without_enter' panicked at .../tracing-subscriber-0.3.23/src/registry/sharded.rs:306:32:
tried to clone Id(57005), but no span exists with that ID
...
thread '...' panicked at crates/codegen/xai-grok-telemetry/src/startup.rs:783:45:
called `Result::unwrap()` on an `Err` value: PoisonError { .. }
panic in a destructor during cleanup
thread caused non-unwinding panic. aborting.
```

在 `timer_parents::open` 里加一行诊断，原因当场暴露：

```
DIAG open name=parent.work top=false global=true current=false
```

`InstrumentationTimer` 找父跨度时先看本线程的父栈，栈空则回退到**进程全局**的
`crate::startup::current_phase_span()`。而 `startup` 的用例会把一个活的 phase span
放进那份全局状态，那个 span 属于**它自己那份线程局部 subscriber**。回退把它克隆进
`span_profile` 用例自己的 registry，「Id 在我这儿不存在」直接 panic；panic 展开时
析构又踩到被污染的锁，于是非展开 panic → abort。

修法：让 `span_profile` 的测试夹具 `folded_with_layer` 去拿 `startup` 用例早就在用的
那把 `SERIAL`（本次从 `mod tests` 提升为 `#[cfg(test)] pub(crate) static`），两族用例
互斥。实测：修前 70 次挂 3 次（更早的 8 次里挂 2 次，即 25%），修后 150 次 0 次。

同一轮还清掉紧邻的**另一条**临界断言：`startup_phases_emit_spans_with_durations`
断言「根 span 必须覆盖它所有的 phase」，而两侧都是各自回调里采样出的墙钟时间，
真实差值在 0.4µs / 0.8µs / 2.3µs 这种量级就翻符号（窗口是 30ms）。它与本次改动无关
（改前 70 次里出现 2 次，改后同样比例），所以给它加了 1ms 的采样偏斜界：最小的一段
phase 也有 10ms，真出现覆盖缺口时超出量至少是它的十倍。

**教训**：进程全局状态（`CACHE_EPOCH`、steer 缓存、fd 继承、tracing 注册表）不会
出现在用例自己的 tmpdir 里，却决定它读到什么。这样的用例要么显式互斥，要么把
「全局变化了」当成合法分支写进断言——把它当成不可能的巧合，就换来一条只在全量跑时
冒头的偶发失败。附带一条：**测量类断言必须给出测量方式对应的容差**，两侧由不同回调
采样时，`a >= b` 在微秒量级上不成立。

## 2026-10-03：变异证明还原之后，cargo 可能继续跑被变异的那个二进制

本仓的变异证明纪律是：把生产代码改坏，看那条测试是否变红，再逐字节还原并 `cmp` 校验。
还原这一步有一个不显眼的坑——用 `shutil.copy2` 还原会把**变异前的 mtime** 一起写回去，
而 cargo 的新鲜度判断就是比 mtime。源文件的时间戳比上一次构建的产物更早，cargo 于是判定
「没有变化」，直接复用**带着变异**的测试二进制。结果是内容已经还原的树测出红色：本轮
`computer::local::terminal::tests::a_persistent_request_without_a_persistent_shell_is_refused_and_said_once`
在还原后失败，它断言「平台给不出常驻 shell 时就不得发出常驻 shell」，而磁盘上的函数是正确的。

危险不在这一条红，而在它**恰好不红**的那些情形：如果变异影响的是一条断言较松的测试，
「变异后仍全绿」会被记成等价变异，一个真实有效的守卫就被误判成空转，而台账上写着它被证明过。
做法因此固定为两条：还原之后把 mtime 顶到当前（`os.utime(path, None)`），且还原后的第一轮
验证要跑**被改动那个 crate 的全量**而不是只跑受影响的那条过滤。本轮全量为
`cargo test -p xai-grok-tools --lib`，3201 通过 / 0 失败 / 3 忽略。

同一条规则对「临时插一行探针取真实输出再删掉」的取证同样适用，那种做法确实会产出一个
带着探针的二进制；本轮用它抓出 LSP 启动诊断的真实文本，见
`docs/verification/windows-test-failures-2026-10-03.log` 的 `== 3.` 节。

## 2026-10-04：同一个 mtime 坑第二次踩到，这次在还原那一侧；判据要从「读对文本」升级为「算术上不可能」

上一节写下的规则在同一天被自己违反了一次。取证脚本要把 HEAD 那份 `manager.rs` 逐字放回、
在 wine 下跑一遍、再还原，三步都用 `shutil.copy2`。变异那一步之后补了 `os.utime`，
还原那一步**没有**，于是「还原后复跑」跑的仍是带变异的二进制——报出来是
`running 6 tests` 而文件里写着 8 条 `#[test]`，如果只盯红绿，这一句会被读成「HEAD 的测试在
Windows target 上通过」，正好把整轮结论倒过来。跨 target 的构建（`CARGO_TARGET_DIR` 指到容器
卷里）比本机更容易踩，因为产物时间戳与宿主工作区的时间戳来自两次不同的挂载。

修法分两层，第一层是补上两侧都 `os.utime`；第二层才是重点：**驱动加一条与文本无关的结构校验**
——libtest 打印的 `running N tests` 必须等于源文件里声明的 `#[test]` 条数。这条不解释红绿，
只问「跑到的是不是我以为的那份源码」，复用旧二进制在算术上就过不去。同日另一个崩溃也在同一
方向上：驱动用 `int(line.split()[2])` 读汇总行，而 libtest 那行是
`test result: ok. 8 passed; ...`，第 3 个 token 是 `ok.`，`ValueError` 直接把整轮取证打断；
改成读 `running (\d+) tests?` 之后，同一次解析既给出条数也给出校验基准。

判据：**取证驱动在宣布任何红绿之前，先证明它跑到的是它以为的那份源码**。能证明这件事的只有
源码里独立可数的量（`#[test]` 条数、模块名、函数名），不是构建系统的新鲜度，也不是「我记得
刚刚写过这个文件」。凡是一个数字要从测试输出里解析出来，那个数字所在的行就同时当作校验对象。

## 2026-10-04：断言引用的是夹具自己的拼法时，变异打不死它

同一批变异证明里有两条（「拒绝理由不再重复候选说了什么」「非零退出被当作没问题」）在第一轮**活了下来**，
而测试本身没有 bug：`resolve_python` 的失败信息会把候选的**整条命令行**引用进去，而夹具把 stub 要说的
话写在 `python -c <正文>` 的正文里，那句话本来就在那条命令行上。于是
`assert!(refusal.contains(消息))` 被夹具自己的拼法满足，与被测代码有没有读过 stdout/stderr 无关。
改成把消息写进临时文件、由 stub 在运行时读出来之后，同一个变异立刻变红；临时文件也刻意取名 `a.txt`，
不含任何被断言的子串，免得那条被引用的路径第二次替被测代码「说话」。

这与上一节是同一类错误的两面：上一节是「绿灯可能是假的」，这一节是「红灯可能打不到」。判据是同一个：
**断言里的每一个字符串常量都要问它可能从哪里来**。如果它同时也能来自夹具自己的文本（命令行、路径、
文件名、临时目录名、环境变量名），那这条断言测量的就不是被测代码。写「错误消息包含 X」这类断言时，
X 必须来自被测程序在运行时产出的数据，而不是来自夹具的源码文本。

## 2026-10-04：自测里重述被接线处的数字，重述的那一份会变成说谎的那一份

把预算降下来之后跑全量门禁，唯一变红的是 `platform-gated tests`，而红的是自测本身：
`AssertionError: 1106 != 1108` 与 `441 != 443`。台账和守护脚本都没错——那四个预算写在**三个**地方
（`.github/workflows/ci.yml`、`scripts/verify-in-docker.sh`，以及自测文件顶部的一对常量），
降预算那一步只改了前两个。于是那条名为「台账恰好停在 CI 所强制的上限上」的用例，
实际在拿台账跟自己的过期副本比，并把改动正确的一侧报告成出错的一侧。

判据：**测试不要重述配置里的常量，要么解析出来，要么就检查两处一致**。这里改成从两个被接线处
解析出四个 `--max-*` 值再跑（解析锚在调用命令本身，注释里引用旧数字不算），并补两条此前缺失的
用例：两个被接线处的四个数字必须相同（`check-guard-wiring.py` 只证明两处都调用了这个门，从不比较
数字，所以本地不跑的那条腿完全可能强制着另一个上限），以及把四个上限同时下调 1 之后必须全部报错
（漂到债务之上很远的上限能通过前面所有检查却不再管任何事）。

同一批变异还暴露了驱动脚本自身的同类问题：M4b 被记为「杀掉」，但驱动只找 `FAIL:` 行，而实际发生
的是导入期异常、一条测试都没跑（`rc=1`，没有任何测试结果行）。**能区分「测试失败」与「测试根本没
跑」的驱动，才有资格报告非空洞性**；这一点与「绿灯可能是假的」是同一件事，只是高了一层。

## 2026-10-04：可达性规则查到「有没有人跑」就停了，参数那边没有人管

同一天里连着两件事是同一件事的两层。第一层：预算写在三个地方，降预算只改了两处，说谎的是没改
的那份（上一节）。修法是把第三份删掉、从被接线处把数字读出来——于是自测从此与接线处一致，也就
再没有任何检查去过问**这两处该不该一致**。第二层就是这个缺口：`check-guard-wiring.py` 的两条规则
都是可达性，回答的是「有没有人跑这个守卫」；两处都跑了之后各自问它要什么，无人过问。

补上的第三条规则形状很简单：一个 flag 只有一处传，那是选择（`--require` 只属于 Windows 那条腿）；
两处都传的 flag，值必须相同。它的价值要落在真实仓库上才看得见——把 `ci.yml` 抬到 1107、本地入口
留在 1106，HEAD 版的这个检查说 OK，拿着这笔预算的门自己在 1107 也 `exit 0`，只有新规则报红。出错
的是这一对，而站在任何一个被接线处里面都看不见另一个。

两条可以带走的结论。

其一，**可达性类门禁要往下追问一层**：跑到了，然后呢？同一个门在两个地方被调用时，会漂移的不止
「有没有被调用」，还有参数、阈值与期望基线，而这一族漂移恰好全都发生在只被一条腿强制的地方。

其二，**比较类规则必须先说清它比的是哪一对**，而且这一句往往不是设计出来的。本仓库比的是「本地入口
vs 每一个 workflow」：两两比较会让第二条合法地想要不同上限的腿根本加不上来，按名字只认 `ci.yml`
又会让第二条腿随便漂。写出「只跟 ci.yml 比」的第一版是被一条失败的夹具推翻的——那条夹具要抓的正是
「多一条腿就没人看」这件事。同理，归属也比值更容易出错：预算写在续行里、写在 `&&` 之后、被注释里的
旧数字冒充，三种情况下规则都会安静地少比一项，而**少比一项的门禁与没有门禁的区别只剩下虚假的安心**。

## 2026-10-04：一条打不到任何可达行为的变异，被记成「测试有洞」

LSP 那批变异里 W5（删掉 mock `touch_beside` 里 `except OSError` 的 `raise`）活了下来。第一反应
是「这条 mock 的失败路径没被测到」，但那句话不成立：URI 解码正确时 `except` 分支根本不会执行，
删掉 `raise` 改不了任何一条能被执行到的指令。这是一条**惰性变异**，它的存活说明驱动选错了改动点，
说明不了测试的质量——与「还原后 cargo 仍跑旧二进制」那一节同族：驱动给出的信号与被测对象的真实
状态之间断了线。

正确的做法是换一个能改变可达行为的改动点重测，本轮是把「切片代替解码」与两种 `except` 处理组合
成 W4（切片 + raise）与 W5a（切片 + 吞掉）两条：两条都被杀，报同一句
`timed out waiting for the server to start its first pull`。这条重测顺带证伪了一句注释——它写着
「mock 会自己说清楚」，而实测两种情况下 mock 的 stderr 都没有出现在测试输出里，因为
`ServerStderr` 只在**启动**失败时才引用 stderr 尾部。注释里凡是声称「出错时报告会怎样」的，都要
像断言一样去跑一遍：本轮真实报告失败的是测试自己的等待，不是 mock 的那行字。

判据：**报告 SURVIVED 之前先问这条变异改动的指令是否可达**。不可达就重报为「驱动无效」，另选改动
点；把惰性变异记成覆盖缺口，台账上就多了一条谁都不会去修的假账。

## 2026-10-04：测量脚本把 `.py` 交给 bash 跑，顺手在真仓库里跑了一次真命令

批量取证用的临时 shell 脚本里写了一行 `bash scripts/ci/test-upstream-recon.py`。bash 不认识
Python，却也不会停：它把能当命令执行的片段**真的执行了**，其中一行是
`scripts/upstream-recon.sh` 的真实调用，于是这一轮在真仓库、真网络之上写出了一份真实的侦察记录
（未跟踪，随即删除）。测试没有跑，副作用却发生了——这是取证脚本能造成的最坏一类事故：它既不失败，
也不留下「测过」的证据。

同一段脚本还暴露了第二处：它的退出码取的是管道最后一段（`... | tail -20`），而 `tail` 永远成功。
被测量的那一环红不红，与脚本报告的红不红没有关系。两处都属于同一个疏漏——**测量工具自己没有被
测过**。固定做法：解释器与被测文件的类型要匹配（`.py` 交给 `python3`），取退出码时永远取被测那一
段（`cmd; rc=$?` 之后再 `tail`，或用 `set -o pipefail`），而取证脚本本身在用于取证之前，先拿一条
已知会红的命令跑一遍，看它是否真的报红。

## 2026-10-04：驱动把「有 Traceback」当崩溃信号，二十条变异一起被记成 BROKEN

新写的 `scripts/ci/check-evidence-commands.py` 第一次跑变异矩阵，二十条全部报 BROKEN，没有一条
KILLED，也没有一条 SURVIVED。那一刻的读法是「夹具整体坏了」，而夹具下一轮自己跑是 19/19 绿。

断点在驱动的三分类上。驱动原来这样区分「测试杀掉了变异」与「改动把测试跑崩了」：输出里出现
`Traceback` 就算崩。这句话在 unittest 上是错的——**每一个失败断言都会打印一段 Traceback**，那
正是「杀掉」的样子，不是「跑不起来」的样子。于是所有 KILLED 都被吸进 BROKEN，而 BROKEN 恰好是
驱动里唯一不需要给出条数的那一格，一个信号都没丢出来。

崩溃只有一个可靠信号：这一次运行有没有走到汇总行 `Ran N tests`。没走到才是崩；走到了而
`rc != 0` 就是杀，并且要把被杀的条数报出来（本轮 M1 杀 10 条、M7 杀 9 条，只报「红了」会把
「一条断言偶然变红」与「这一族断言全红」混成同一个词）。改成这条判据后，十五条的判定一次全部
落位：15 KILLED、0 SURVIVED、0 BROKEN，还原后夹具与真仓库同时 `rc=0`。

这与「变异证明还原之后，cargo 可能继续跑被变异的那个二进制」是同一个疏漏的两面：驱动给出的
信号与被测对象的真实状态之间断了线，而断裂发生在**没人看的那一格**。所以两条固定做法写进驱动
本身：测量之前先要基线为绿（夹具 `rc=0` 且真仓库 `rc=0`，否则拒测并退出码 4）；BROKEN 的判据
必须是「没跑到汇总行」这种**缺失**信号，不能用「出现了某个词」这种**存在**信号——后者在测试框架
里几乎总会以另一种原因出现。

判据：**三分类里，「崩」这一格只能由缺失定义**。用存在定义的崩溃，第一次跑就会把整套结果吸进
去，而且吸进去的那一格看起来最像「环境问题」，最不容易被怀疑。

同日还有一条同族的：M20 第一次报 SURVIVED，原因不在夹具，而在变异本身没到位。M20 要还原的是
「引号里的解释器名也算命令」这个旧行为。旧代码匹配解释器用的是 `tok.strip(QUOTE_CHARS)`，后来
加上的跳过（首字符是引号就跳过）与那次去引号是同一件事的两半：驱动只删掉跳过那半，`"python3`
仍然因为带着引号而匹配不上任何解释器，整条改动在语义上是空操作，夹具自然杀不掉它。把另一半补上
（查表也退回带 strip 的写法）之后，M20 由 2 条夹具杀掉，真仓库同时 `rc=1`。

判据：**一条变异要还原旧行为的整体，不是删掉旧行为留下的某一行**。删完仍然语义等价的变异，它
报出的 SURVIVED 描述的是那条变异，不是那套夹具，而这两种 SURVIVED 在报告里长得一模一样；唯一
分开它们的方法是写完变异后问一句「这段代码现在和改动前等价吗」。

## 2026-10-04：期限挂在 spawn 上，它量的就是机器的快慢；五条偶发红其实是同一条

五条测试长期偶发，形状毫不相像（`left: "" right: "hello123"`、`left: Some(127)`、
`fallback warning must be in the command output, got: "/tmp\n"`、
`spawn must fail when both directories are missing`），单跑全绿，只在 32 路并发的整轮套件里
随机挑一条红。归因在这里有一个陷阱：越是被怀疑的组件（读管道的那段 poll 循环）越是清白的，
而真正的缺陷没有名字——reader 的 5 秒期限是在命令 **spawn** 的那一刻起跑的，于是它覆盖的是
「命令跑多久 + 约 454 KB 的 dump 从 64 KiB 管道里排空多久」，负载一高就必然有人超。超时之后
`read_dump_from_pipe` 交回空串，`parse_dump` 拒收，session 静默留在那条命令之前的状态。

取证只靠一次 instrumentation：在状态交接处把三支（accept / reject / read_timeout）打出来，
并且让 libtest 的失败回显替我们做归属——`failures:` 底下那一段属于失败测试自己，于是
「这条测试的第一条命令撞钟 → 调用方拒收空串 → 第二条命令的行为因此不对」在同一段输出里连
成一条线。判据：**偶发的一族红，先找一个能同时容纳所有症状的单点去插桩，而不是逐条测试改
断言**；插桩点要选在状态真正交接的那一处，这里就是唯一调用 `update_from_dump` 的地方。

同族第二个疏漏是那句日志的等级。`update_from_dump` 明明返回「收没收」这个 bool，调用方把
它丢了，只打一行 `debug`，于是生产上任何一次状态丢失都是不可见的。判据：**返回值表示「这次
没成立」而调用方不消费，就是缺陷本身**，与它有没有造成红无关；修完之后干净退出被拒收是
`warn`（带字节数，空串与残缺分得开），被杀掉的 shell 仍留 `debug`，因为那是预期内的事。

## 2026-10-04：新写的计时测试第一轮全量就红了——预算要在两侧都留出余量

这条是本轮自己踩的。修完 shell state 之后新增的 `test_wedged_dump_pipe_does_not_stall_the_reply`
单跑 6 秒全绿，写进全量套件第一轮就红：那个测试用 20 秒的预算去等一条「本应在 5 秒 grace 到
点时回来」的命令，而 32 路并发下这条命令光起 shell 就要好几秒，20 秒不够；同一轮里去掉 collect
期限的变异侧也要很久才红，两侧离阈值都太近。

第一版改法是把间隔拉开到不需要讨论：被后台继承的管道改成一条长 `sleep` 一直握着，测试的预算提
到 60 秒，于是「修好的」在 ~10 秒内回、「坏掉的」要几分钟才回，阈值两边各有数量级的余量。同一
条纪律用在另一条上：`test_persistent_shell_state_survives_a_slow_command` 里那条 6 秒命令带着默
认的 30 秒请求超时，在高负载下有被自己的 deadline 杀掉的风险，于是显式提到 240 秒——deadline
触发会让测试红在一个它不钉的原因上。

但「把余量拉开」只是把墙钟推远，判据本身仍然是墙钟，而那一轮同时证明了墙钟在这个套件里有多不
值钱（见下一节）。第二版把判据换成结构事实：命令退出之后去看那个握着管道的后台进程还活着没有
（`kill(pid, None)`），活着就证明回复是 grace 送回来的、不是等它松手之后才回来的；外层再套一层
`timeout(150 s, ...)` 专门接「根本没回复」这一种。变异侧（去掉 collect 的期限）于是红在「回复到
达时那个进程已经没了」上，而不是红在秒数上；它实测 `elapsed 121.747539416s`，因为回复真的等到了
那个 `sleep` 自己结束。清理（按 pid SIGKILL 掉那个后台持有者）放在任何断言之前，免得一次失败顺
带把测试拖成几分钟。同族的另一条 `test_giving_up_on_the_dump_releases_the_pipe` 里也留着 20 秒，
但那一处秒数只是「最多愿意等多久去观察那个事实」的兜底，断言的是进程消失这件事本身。

判据：**能观察结构事实就不要观察墙钟**。余量拉到数量级也只是把偶发的概率压低，而「进程还在/不
在」「文件在/不在」这类事实与机器快慢无关；新写的计时测试要跑过一整轮全量套件才算写完，单跑绿
只证明它在空机器上成立。这条与「一条测试把 2 秒写死在自己身上」是同一件事的两侧：一侧的预算贴着
「修好的样子」，另一侧就必须贴着「坏掉的样子」，两头都贴就没有判决可言。

## 2026-10-04：reader 搬到 reactor 上才暴露它一直在阻塞读——AsyncFd 要求 fd 先 non-blocking

放弃一次 dump 必须连那次读一起放弃：旧实现把 `read_to_string` 放在 `spawn_blocking` 里，
`abort()` 只丢得掉 `JoinHandle`，线程和它握着的读端一起活到那条后台命令自己结束。搬去
`AsyncFd` 之后，新写的 actor 测试红了 61 秒，而第一次诊断是错的：我以为 `readable()` 交出来的
readiness 没被清掉，于是补了一条 EAGAIN 分支去清——方法名就不对（`clear_ready` 才对），而翻开
tokio 1.52 的 `Guard::try_io` 更发现它在闭包返回 `WouldBlock` 时自己就清了，那条分支根本不可达。

不可达本身就是答案：**阻塞 fd 上的 `read(2)` 永远不会返回 EAGAIN，它直接睡**。`AsyncFd` 只把
「等」交给 reactor，读还是那一次系统调用；`os_pipe()` 用的是 `pipe2(O_CLOEXEC)`，没有
`O_NONBLOCK`，于是内核把 current-thread runtime 唯一那条线程收走，同进程里所有别的任务一起停。
新写的单测把这件事写成了红字：`150ms of sibling sleeping took 1.603740898s`——那 1.6 秒正是它
被告知的「下一个 chunk 再等一等」，而 actor 侧的探针形状是 `arm:tick` 整段消失。

`O_NONBLOCK` 加在读端要单独实测一次：管道的两端是两个独立的 open file description，把读端设成
non-blocking 之后写端 flags 仍是 `O_WRONLY`，往满管道写 1 MiB 依旧阻塞而不是 EAGAIN。这一条不能
想当然，因为 dump 约 454 KB 要穿过 64 KiB 的管道，写侧全靠阻塞才能写完；若两端共享 flag，dump
会被静默截断成半个，而那比丢状态更难查。

判据：**把一次阻塞 I/O 换成 reactor 驱动时，「设 non-blocking」属于迁移本身，不是之后的优化**；
AsyncFd 用错的表征不是 panic 也不是报错，是「同运行时里另一个毫不相干的任务停了」，所以给这种
读写的测试必须自带一个同运行时的兄弟任务当哨兵。另一条可复用的推理是：**分支不可达往往在说它
的前提不成立**，而不是在说那段防御代码多余。

## 2026-10-04：机器上挂着 52 个孤儿自旋循环，那一天所有计时数字都是废的

那天所有「预算不够」的判断都建立在计时上，而计时本身是坏的。症状：`zsh -ic 'echo M'` 从 0.88 秒
量到 1.3~1.45 秒，`uptime` 报 59~61 的负载平均，单跑 6 秒全绿的测试进了全量套件连 20 秒预算都
不够。`ps -eo pid,ppid,pcpu,etime,args --sort=-pcpu` 查到 52 个 `PPID=1` 的自旋循环：24 个
`/usr/bin/zsh -c` 起的 `while :; do :; done`（已跑 22h48m，每个 ~94% CPU），28 个
`/bin/sh -c 'for i in $(seq 1 $(($(nproc)/2))); do (while :; do :; done) & done; wait'`
（1h04m，每个 ~72%），全部是本轮被中断的探针脚本留下的——那些命令本来就打算用满半个机器。

清场本身也踩了两次：`kill -9 -<pgid>` 和 `os.killpg(0, ...)` 都把**我自己的** shell 一起杀了，
因为这些孤儿的 PGID 是 0。逐个 PID 点名杀才是安全的做法。清完之后负载回到 0.2 左右，同一个
`zsh -ic 'echo M'` 三次量到 0.88 / 0.83 / 0.88 秒，同一时刻 `bash -lc 'echo M'` 是 0.01 / 0.00
秒——也就是说当天之前所有以秒为单位的结论（包括写进证据日志的那三个数字）量的都不是被测代码，
而是那 52 个循环。

判据：**任何以时间为证据的测量，先证明机器是空的**——`uptime` 和按 `%CPU` 排序的 `ps` 各留一行
进日志，再留时间数字；否则你改的是测试预算，不是缺陷。清理后台进程按 PID 点名，不要按进程组，
尤其当你自己就在那个组里（PGID 0 是这一族孤儿最常见的落点）。同一组数字旁边留一条「几乎不做
事情」的对照组（这里是 `bash -lc`），它把「这台机器起 shell 就是这个价」与「负载把一切都拖慢了」
区分开。

## 2026-10-04：探针把被量的代码改坏了，多写一个 match 分支，dump 的字节就没了

为了查 actor 在高负载下为什么不按 5 秒宽限回话，驱动往 `read_dump_from_pipe` 的循环里插了一条
chunk 计数探针。插法保留了原分支，却在它上面又加了一条同 pattern 的分支：

    Ok(Ok(n)) => probe(&format!("reader chunk n={n} buf={}", buf.len() + n)),
    Ok(Ok(n)) => buf.push_str(&String::from_utf8_lossy(&chunk[..n])),

第一条赢，于是 reader 只数不存。同一轮 74 个测试红了 7 个，全是 `cwd should persist across
commands, got: /tmp`、`the export made by the slow command should persist, got: "/tmp\nstate=\n"`
这类「状态没带上」的老症状，探针自己也老老实实打出 `collect got-dump id=… len=0`。当时那一轮
反而比基线快十倍（12.32 秒 vs 124 秒），看起来更像「修好了」。如果相信这一轮，接下来就会去追一个
根本不存在的第二个缺陷。

线索在同一份构建输出里：`warning: xai-grok-tools (lib test) generated 1 warning`，重复 pattern
触发的就是 unreachable_patterns。插桩把编译器警告数改动了，这一轮就不再是测量。

判据：**插桩之后先跑基线绿集**，探针版本必须先复现同一组绿，才有资格解释它跑出来的红；加语句要加
在原分支体内部（把 `=> expr` 改成 `=> { probe(..); expr }`），不要在它上面叠同 pattern 的新分支；
插桩构建的 warning 数一变，这一轮按无效测量处理。

## 2026-10-04：探针把 fcntl 常量抄错，于是它报告「管道两端 flags 都没变」

要证明的事只有一句：`O_NONBLOCK` 只加在读端，写端必须仍然阻塞——约 454 KB 的 dump 要穿过 64 KiB
的管道，写侧全靠 `write` 等读者排空，两端共享 flag 的话 dump 会被静默截断成半个。探针打印两端的
flags，前后各一次，它给出的结论是「读端和写端一样，都没有 O_NONBLOCK」。差一步就被写成「Linux 上
管道两端共享 flag，这次是运气」。

错在常量：探针里写的是 `F_GETFL = 2`。Linux x86-64 上真正的常量是 `F_DUPFD=0`、`F_GETFD=1`、
`F_SETFD=2`、`F_GETFL=3`、`F_SETFL=4`、`F_GETPIPE_SZ=1032`。`fcntl(fd, 2)` 是**设置** close-on-exec
那个 fd 级 flag，第三个参数缺省为 0，于是这次调用不但没读到任何东西，还顺手把两个管道端的
`FD_CLOEXEC` 清掉了，返回 0。`describe()` 把 0 当作 flags 打印成 `O_RDONLY (0o0)`，前后两次一模
一样——一个错误的常量在返回值恰好是合法 flag 组合时，看起来完全像一次成功的读取。

用回 3 之后量到的是：读端 `O_RDONLY (0o0)` → `O_RDONLY|O_NONBLOCK (0o4000)`，写端前后都是
`O_WRONLY (0o1)`，往没人读的满管道写 1 MiB 在内核里停了 2.0071 秒没回来。

判据：**跨语言搬运系统调用常量之前，先用一个已知答案的调用把常量本身验一遍**——这里只要断言
`fcntl(fd, F_GETFL) & O_ACCMODE` 等于 `O_RDONLY`(0) 或 `O_WRONLY`(1) 就能立刻发现读错了；对不上
说明常量错，而不是被测代码错。带写语义的常量（`F_SETFL`、`F_SETFD`）与带读语义的那一组只差几个
编号，探针要挑读的那一个，并在文档里把平台写清楚（这些编号是 Linux 的，不是 POSIX 的）。

## 2026-10-04：`abort()` 之后用 `yield_now` 等 fd 关闭，一百次也不到

`giving_up_on_the_dump_reader_closes_our_end_of_the_pipe` 单跑绿、全量套件五轮红两轮，红在写被
接受而不是被拒：

    assertion `left == right` failed: our end of the pipe should be closed once the read is given up on
      left: None
     right: Some(EPIPE)

第一版用 `yield_now` 等一百次，仍然红。原因分两层。前一层是从 tokio 1.52.3 源码读出来的：
`JoinHandle::abort()` 走 `remote_abort` → `transition_to_notified_and_cancel` → `schedule`，被取
消的任务它的 future（里面的 `AsyncFd`，以及 `AsyncFd` 里那个 `OwnedFd`）要等运行时**下一次 poll
到这个任务**才被释放，`abort()` 本身不关闭任何东西。后一层是实测的：在 current-thread 运行时里，
`yield_now` 一百次也换不来一次「看到那条队列」，而一次 1 ms 的 timer park 可以——actor 自己在等
它的 10 Hz tick 时做的就是后者。改成 timer 轮次（上限五十次）之后，八轮全量套件全绿。

失败消息里同时加了一次 fd 普查（`inode=… holders=["3691751:12(r)", "3691751:13(w)"]`，从
`/proc/<pid>/fd` 的符号链接按管道 inode 匹配，方向取自 `/proc/<pid>/fdinfo/<n>` 的 `flags:` 行），
因为「读端还开着」在「持有者是我自己」与「是个陌生进程」两种情况下是完全不同的结论。libtest 会把
通过测试的 stdout 藏起来，所以这个普查只能放在失败消息里——它至今没响过，这是一句实话：它是为一次
已经解释清楚的红写的。

判据：**断言「取消之后资源确实被释放」时，等待要用 timer 而不是 `yield_now`**，两者的调度含义在
current-thread 运行时里不等价；并且要把观测点放到运行时之外（让对端进程的死活当证人，或者像这里
一样直接数 `/proc`），否则测量者与被测量者在同一个被卡住的东西里面。

## 2026-10-04：加一个标记等于重写台账，因为台账的「身份」是六个字段

全量门禁 28 门里同时红了两门，其中平台门那一次红得没有道理：新增未记的门控明明是 3 条，报出来却是
`53 problem(s)`——25 条 `stale baseline row` 加上 28 条 `unlisted platform-gated test`，而 fixture
套件 42 例里红的 3 例是同一件事从自测里读出来的结果（那 3 例跑的是活体检查并且断言 rc 0）。

原因写在门禁自己的 docstring 里，只是当时没预料到它会这样发作：`assumptions` 是行身份
`(file, function, runs_on, kind, extra_cfg, assumptions)` 的一列。为了让 `posix-shell` 语法进台账
而加的那个标记，改变了 25 条既有行的标签，于是这 25 行每一行都不再匹配自己那一行，同一条既被报成
「行过期」又被报成「新增未记」——一行匹配不上时这两种描述同时成立。

先判断标记说的是不是真话，再谈还原。28 处命中全部打印出触发它的那一行并逐条读过：26 处是交给 shell
执行的命令串（`cmd.args(["-c", "sleep 600 & echo $!; wait"])`、`.args(["-c", "kill -ABRT $$"])`、
`write_pty_input(&pty_id, b"sleep 300 & echo pid=$!\n")`），2 处是被测代码要去 source 的 rc 文件内容
（`home/.bashrc` 与 `config.rc`）。后者里 `config.rc` 那条 shell 根本不运行，标记描述的是夹具的语法
是 POSIX shell 语法，而不是「执行了 shell」；这两行本来就从自己体内带 `file-mode` 与
`unix-ext-trait`，没有任何一行因为它们从点名变成不点名。

还原走 `--write-baseline`，而判断这次加宽是否诚实的是它产出的 diff，不是它退出 0：
1,106 → 1,109 行，新增 3 条全是本次新写的测试，**删除 0 条**，25 条只多了 `posix-shell` 一个标记，
19 条只是行号提示移动，理由改动 0 条。删除 0 是这件事的重点：一个标记可以把一行从「可以拆」的清单
里免掉，它不能删掉那一行记着的门。三条新行没有用导入标记，是手写理由，所以 `--max-unreviewed`
停在 1106 而不是涨到 1109。

还有一个数字方向值得记：点名数从我新增三条测试之后的 442 变成 427。加了测试，「值得拆的门」清单
反而短了 14 条——因为标记免掉的（15 条）比新增的（1 条）多。这类反直觉的数字如果不写下来，下一次
没人敢把它收紧；两处接线现在都写 427，并且用 426 打过一次红。

判据：**凡是被放进身份的每一列，改变「这一列怎么算」就是一次数据迁移**，动手前先数它会碰到多少
行，动手之后用迁移 diff 当评审对象（新增、删除、只变标签、只变提示各多少），而不是用「门禁绿了」。
反过来，同时看到「新增未记」与「行过期」两种消息时，先怀疑身份的计算方式变了，而不是测试被改了。

## 2026-10-04：把两条 copy/restore 塞进一条命令，还原出来的是另一个文件

为了把 unsafe 站点的增长归因到具体文件，需要逐个文件回退到 HEAD 再跑普查。一条命令里嵌了
`basename` 与 `sed` 的二次求值，结果把 `shell_state.rs` 的内容写进了 `terminal.rs`——不是改错，是
整个文件被换掉。

救回来的是同一条命令末尾那句 `cmp`：它当场报了不一致，于是从 scratch 里的副本 `cp` 回去，再用三条
独立证据复核（`cmp` 逐字节、`git diff --stat` 的改动规模、`host_load` 与
`test_giving_up_on_the_dump_releases_the_pipe` 这两个只存在于该文件的符号仍在）。

判据：**会写源码的命令必须一条只做一件事**，源与目标逐字写明；把「保住现场」这种动作和它的校验
放在同一条命令里是对的（这次就是它发现的），但校验之前不允许出现需要靠引号嵌套或变量二次求值才能
读懂的东西。同一条纪律也管还原之后的 mtime（见本篇另一节），两者都是「还原」的一半。

## 2026-10-04：一条测试把 `GIT_BIN_PATH` 指向自己的壳脚本，等于换掉了整个测试二进制的 git

容器全量（commit `8a52ff0a`，镜像 `chaos-verify:frozen`）里 `cargo test` 只红一条：
`session::goal_classifier::evidence::tests::changed_files_complete_when_git_diff_exceeds_byte_cap`，
报的是它自己的前提断言 `test premise: the diff must exceed the byte cap`；同一次跑里造出它的
`session::goal_classifier::tests::baseline_capture_timeout_kills_the_git_it_abandoned` 反而是绿的。
那条 timeout 测试把 `GIT_BIN_PATH`（`util/subprocess.rs:32` 的 `git_bin()` 认这个变量）指到一个
壳脚本，脚本把自己的 pid 写进一个文件、再 `exec sleep 8`，用它来观察「被 timeout 放弃的那个 git
有没有死」。

机制是进程全局的。`EnvVarGuard` 只与**同样取 `ENV_LOCK` 的测试**互斥，`git_bin()` 却是裸读环境
变量，所以 guard 活着的那一秒多里（观测被劫持时会拖到近六秒），同一个 `--lib` 二进制里任何一条
测试的 git 调用都会 exec 到那个壳，而壳对任何参数都写下 pid 然后以 0 退出、stdout 为空。
`git add`、`git commit`、`git rev-parse`、`git diff` 于是统统「成功」，`git diff <baseline>` 交回
零字节，那条 evidence 测试的前提断言当场炸——它红的是夹具，不是产品。

第二个缺陷更贵：老壳脚本**无条件**写那个 pid 文件。落在窗口里的受害调用会把 pid 文件改写成
**它自己的** pid，于是 timeout 测试盯上的是别人的进程；别人的壳要睡满 8 秒，它必然报
`the abandoned git shim (pid …) was still in state S 6.0s after the capture budget expired`。
这句话与上一轮变异证明记进 CHANGELOG 的那句几乎逐字相同：同一条错误消息，一次是「缺陷被证明
存在」，一次是「观测被劫持」，而红绿本身分不出这两种。

实测都在同一个 `--lib` 二进制里，`--test-threads=8`，选中该测试加 `evidence` 模块共 47 条。
老形状五轮全红（每轮 2~5 条失败；`changed_files_complete_when_git_diff_exceeds_byte_cap` 红 4/5，
timeout 测试自己红 4/5）；把壳改成只对它瞄准的那次调用变慢、其余原样 `exec` 真 git 之后，同一
命令 47 通过 / 0 失败 / 1.07s；老形状单跑（`--test-threads=1 --exact`）本来就通过（1.02s）。
所以它不是一条稳定红的测试，而是一条命运由并发决定的测试——`cargo test` 不在宿主门禁里（见下一
节），本机默认根本碰不到它。

判据：**任何把可执行文件塞进进程全局环境变量的测试，注入物必须只对瞄准的那一次调用改变行为**
——识别手段是参数，加上一个只在自家目录里存在的标记文件——其余调用原样 `exec` 真二进制，并且
要有条断言真的用被测函数问一次「无关的仓库还看得见真 git 吗」（新测试里那条 `assert_eq!` 就是
这个问题）；观测用的临时文件只能在被瞄准的那条分支里写。把新壳的识别条件改成 `if true`（等于
恢复出厂形状），那条断言与同一条 evidence 测试立刻一起红。这三半缺任何一半，红绿的含义都不
属于被测代码。

## 2026-10-04：宿主门禁跳过的那四条腿，正是这一轮两个缺陷的藏身处

同一个 commit 在容器里红了两条门禁：`cargo test`（上一条）与 `cargo clippy`（新测试里的
`collapsible_if` 和 `while_let_loop` 各一处，`-D warnings` 把两者都判死）。而 push 之前本机跑过
的 `scripts/verify-gates.sh` 打印的是 `all gates passed on the host (30 run, 4 skipped)`，被跳过
的四条恰好是 `cargo check`、`cargo clippy`、`cargo test`、`GUI protocol types`。那句
`all gates passed` 在结构上不可能看见这两个缺陷：它一条 clippy 没跑，一条测试没跑。

`scripts/verify-gates.sh --with-build` 一直是把那四条补回来的开关（脚本头那条「the build gates are
skipped unless --with-build」的注释写了理由，
`CONTRIBUTING.md` 的「Fast local gate loop」一节也列了它）。缺的不是工具，是收工条件：改动落在
Rust 代码里时，不带 `--with-build` 的那一轮不算验证过，它覆盖的是那 30 条不重建 workspace 的
门禁。

判据：引用一轮宿主门禁时把 `N run, M skipped` 原样写出来，不要写「全绿」，并对每一条 skip 问
一遍「我这次改的东西有没有可能只有它看得见」。skip 是结构性的，不是运气；`all gates passed`
这句话的主语是那 30 条，不是这 34 条。

## 2026-10-04：宿主 runner 抄了容器的命令行，没抄容器的环境，于是 `--with-build` 自己红

修完上一条之后带 `--with-build` 重跑宿主门禁：`34 gate(s) run, 0 skipped, 1 failed`，红的还是
`cargo test`，还是 `error: 1 target failed: -p xai-grok-shell --lib`。但这次容器里那轮同类命令是
绿的。直接在宿主上跑同一条命令、只多一个环境变量，结论就反过来了：

    $ RUST_MIN_STACK=16777216 cargo test --workspace --locked --no-fail-fast
    ...
    386 个 test result 块，passed: 31594  failed: 0  ignored: 485，exit 0，858 s

差的那一份在 `scripts/verify-in-docker.sh` 的 `run_args` 里：`--env RUST_MIN_STACK=16777216`，那
一行自己的注释写明 `xai-grok-shell` 的 current-thread actor 测试会撑爆 harness 默认栈，并指向
`docs/architecture/todo-open-item-classification.md` 与 CI run `36165469964`。`.github/workflows/ci.yml`
的两条 test step 也各自设了它。也就是说三个执行环境里有三个都设了这个变量，只有宿主 runner 没设
—— 它的设计是「只从 `gates=()` 里抄命令行，别的都不动」，而环境正是被这句「别的」漏掉的部分。

现在 `scripts/verify-gates.sh` 自己导出它（调用者已设的值优先，与容器一致），并在表头把那行的实际
取值打出来。`--self-test` 里加了三个用例：一个 fixture 门把 `${RUST_MIN_STACK}` 读回来断言它等于
预期值，预期值不是抄来的，而是 `sed` 从 `verify-in-docker.sh` 的 `--env RUST_MIN_STACK=` 那一行现读
（改掉任何一边都会红）；一个用例钉住「调用者显式设的值不被默认值覆盖」；一个用例钉住容器那一行还
在。变异复核：把默认值改成 `8388608` → 恰好 `not ok a gate sees the stack size this runner exports`
一条红，47 例里 46 passed / 1 FAILED，`cmp` 逐字节还原后 47/47。

另一条变异顺带暴露了 `set -u` 的形状：把 `export` 那一行换成 no-op，`--self-test` 28 条全红而不是
1 条，因为表头 `echo "... ${RUST_MIN_STACK}"` 在变量未设时直接让脚本以 1 死掉。这不是好信号——一次
让整套夹具一起塌掉的变异不能定位任何一条断言。表头改成 `${RUST_MIN_STACK:-unset}`；写读取未设变量
的 echo 时要当它是会致命的。

**判据：镜像另一个 runner 时，被镜像的是「执行条件」整体（命令行、环境变量、工作目录），不是只有
命令行；凡是从别处抄来的常量，self-test 必须回到被抄的那一处现读比对，不许在测试里留第二份副本。**
后半句与同日「预算写在三个地方」那条同源：那次的红是台账跟自己的过期副本比，这次的差是宿主跑的是
一条在任何其他 runner 里都不存在的命令。

## 2026-10-04：同一个赋值形状在三个「会打印报告」的脚本里，把报告本身吃掉了

上一条查到底之后，同一形状（`set -euo pipefail` 脚本里的裸赋值 `name="$(管道)"`）在别的会打印报告的
脚本里被专门找了一遍，一共三处，共同点是**退出码早就对了，缺的只有字**，所以上游没有任何一处会抱怨。

其一是容器 runner 自己：`moved="$(diff "${tree_before}" "${tree_after}" | sed -n … | sort -u)"`。
`diff` 在两份文件不同时 exit 1，而「不同」正是这个分支存在的唯一理由，于是脚本死在赋值里，树中途被
改动这一事实连同整轮判决一起消失。撞上它的是 `--only "cargo clippy"`：164 s 后 clippy 只打完
`Finished dev profile … in 2m 40s`，没有 `PASS` 也没有 `FAILED gates:`。复现（第二个窗口在门跑到一半
时改一个被跟踪文件）：修复前 `EXIT=1` 且判决零行；加 `|| true` 之后同样 exit 1，但
`UNATTRIBUTABLE: the source tree changed while the gates ran.` 与变更路径都打出来了。判决本来就排在
指纹比较之后，死在那里连这一轮的门禁结果都保不住。

其二是 `scripts/ci/check-versions.sh`：`declared_names="$(grep -v '^$' <<<"$declared" | cut -d' ' -f1 | sort)"`。
`grep` 没有行可打时 exit 1，而「没有行可打」正是 `optionalDependencies` 被清空时的状态——也就是第 5 项
最该报告的那种破坏。删掉那个字段实测：exit 1，stdout 停在第一行信息行，stderr 零字节；上游第 4 项还
静默通过了（那个循环按声明条目跑，条目数为零）。改法不是压状态而是去掉失败模式：换 `sed '/^$/d'`，
它没有「一行都没匹配上」这个退出码。

其三是 `scripts/install.sh` 的 `download_github`：`size="$(wc -c < "$dest" 2>/dev/null | tr -d '[:space:]')"`。
`2>/dev/null` 和紧接下一行的 `[[ -n "$size" ]] || size=0` 都说明作者预期 `wc` 会失败，可这句赋值先让
脚本死掉，那行兜底永远不可达，于是「换下一个镜像」这件事连同它的 `why:` 报告一起没了。对着 127.0.0.1
上真实 HTTP 端点（4 KiB 正文）与一个总是 exit 1 的假 `wc` 实测：修复前两个候选都递上去却只打印一行
`try:`；加 `|| true` 之后两个候选都试完，两条 `why: too small (0 bytes)` 都在。

两份新夹具各自做了变异：`test-check-versions.py` 6 例里对着 `HEAD` 版门禁恰好红 2 例（空集那两条），
`test-installer-download-size.py` 5 例里恰好红 1 例；两边其余用例都仍绿，说明它们钉的是别的路径，不是
被这次修复顺带点亮的。`check-versions.sh` 的夹具把门禁与 npm 树拷进临时目录跑副本，仓库不动；
`test-installer-download-size.py` 只桩掉候选列表（真的那份要解析 github.com），函数从 `install.sh`
原样抽出。扫描这形状的那条 `git grep` 及其盲区（`[^"]*` 停在第一个引号，被修的两行正是这样躲过它的）
记在 `docs/verification/shell-pipefail-silent-report-2026-10-04.log`，剩下九处逐条判过、都该停。

**判据：一个会打印报告的门，它的报告与退出码同等重要；凡「非零是正常答案」的命令（`grep` 没匹配、
`diff` 有差异、`wc` 量不到）都不许待在 `set -euo pipefail` 脚本的裸赋值里。要么换成没有这种状态的写法
（`sed '/^$/d'`），要么显式吸收它（`|| true`）并让下一行的兜底真的可达。夹具必须断言 stderr 而不只是
断言退出码，否则它抓不到「对了码、丢了字」这一整类缺陷。**

## 2026-10-04：抓「报告被吃掉」的那条新门禁，自己两次把报告写错

新规则 `scripts/ci/check-pipefail-report.py` 在有人能用之前跑了三遍，三遍的判决都有问题，而错的
方式和它要抓的缺陷同形。

第一遍是规则报出的头一处命中，它是个误报。原型扫真树打印 `tracked *.sh: 31  hazard sites: 1`，指到
`scripts/verify-gates.sh` 的 `lines="$(printf '%s\n' "$list" | grep -c .)"`，我照着把 `grep -c .`
换成 awk 计数并写了注释说「这个 `set -euo pipefail` 脚本会死在这里」。回头核实才发现那个 runner
第 43 行是 `set -uo pipefail`，从来没开 `-e`——它要聚合各门禁的失败，不是死在第一个上。用 bash 把
两种拼法各跑一遍才把边界定下来：`-uo pipefail` 下赋值的下一句照常执行并拿到 `lines=0`，也就是那条
自测本来就打印得出 `not ok`；`-euo pipefail` 下执行不到、shell exit 1。改动回退到 HEAD，误报本身
固化成 fixture 里的 `OUT_OF_SCOPE_LINE` 加一条把两种拼法对着跑的用例，另有一条变异专门把它请回来
（M11，杀 2 例）。

第二遍打印的是 `pipefail-report: OK (31 shell script(s), 0 of them strict)`。`OK` 是真的，`0` 也是
真的：判「这个脚本有没有开 `set -e`」的正则里 `^` 没带 `re.MULTILINE`，于是它只可能匹配「文件第一个
字节就是 `set`」的脚本，而这棵树上的 `set` 行落在第 5 到第 55 行。23 个本该进范围的脚本一个都没进，
门禁拿着空集合报告通过。「没有命中」和「一次都没比」打印出来是同一句话。

第三遍报出 12 处，全是假的。找命令替换的收尾 `)"` 时先剥掉引号里的内容，而 `name="$(f)"` 的收尾正好
在引号里面，于是一起被剥掉，正文于是读到文件末尾，把后面无关行里的 `grep` 算成这条流水线的阶段：
`start="$(date +%s)"`、`tree_dir="$(mktemp -d)"` 都在名单上。收尾得靠按引号与 `$(` 嵌套走一遍上下
文栈来找，既不能搜字符串也不能数括号。

最后 17 例 fixture 在未改动的门禁上全绿，11 个变异全被杀（其中「`^` 不带 MULTILINE」那一个一次杀
13 例），接线在 `.github/workflows/ci.yml` 与 `scripts/verify-in-docker.sh` 两处。

顺带查清了一件被当成惯例接受了很久的事。`scripts/verify-in-docker.sh` 一直被记成「只能从干净克隆里跑」，
理由是它会指纹化当前工作树；但真正拦住人的是仓库没有 `.dockerignore`：镜像只 `COPY` 一个 740 字节的
`rust-toolchain.toml`，树是 bind 挂载的，可 `docker build` 每次仍要先上传整棵上下文（`target` 270 GB、
`.git` 210 MB、`node_modules` 143 MB）。补上 `*` + `!rust-toolchain.toml` 之后同一条命令从工作树跑只要
15 秒，上下文 41 字节。白名单的风险由负向对照兜住：探针改成 `COPY CHANGELOG.md /` 就构建失败
`"/CHANGELOG.md": not found`，不会静默产出少一个文件的镜像。

**判据：新写静态检查的第一份证据必须是「它对已知必红的输入红了」，不是「它在真树上是绿的」；判决的
那一行要把「扫了多少」和「有没有问题」一起打印出来，否则 0 命中与 0 扫描无法区分。规则报出的第一处
命中要先当成规则的嫌疑对象去实测——用被测语言自己把那一行跑一遍——而不是当成已确认的缺陷去修；
适用范围的边界由被测实现定，不由正则的作者定。还有一条：一个工具入口「只能从干净克隆/特殊目录里跑」
是可测的症状，不是可以继承的惯例，先量它慢在哪、卡在哪，再决定要不要绕开它。**

## 2026-10-04：容器化验证工具还有一面不是慢，是它写回宿主的文件归谁

清理上一轮留下的冻结克隆时 `rm -rf` 甩出 14 行 `Permission denied`，源头是那个容器化门禁入口：它
以 root 跑，工作树 bind-mount 在 `/src`，而 `scripts/ci` 的 29 个测试模块里有 16 个是用
`importlib.util.spec_from_file_location` 载入自己的门禁模块的，import 经过挂载就要写缓存。跑一条
门禁留 1 个 root 所有的 `.pyc`，`--full` 留 2 个。中间一度以为「一个缓存文件而已」，实测才分清两件事：
文件删得掉（unlink 要的是目录的写权限），容器 *新建* 的那个 `__pycache__` 目录删不掉，`rm -rf` exit 1。
同一个形状放到构建目录上就是 40 GB——入口里那句
`--volume chaos-verify-target:/src/target` 不是性能优化而是归属隔离，把它去掉，容器里一句
`mkdir -p /src/target` 就在工作树里造出 root 所有的目录。

修法分三处是有原因的：镜像的 `ENV` 管住裸跑 `docker run` 和入口自己提供的 `--shell`，入口的
`--env` 管住 `IMAGE_TAG` 指向别处构建的镜像那种情况，缺一个就各留一个洞；第三处是入口在起容器之前
先在宿主上 `mkdir -p "${repo_root}/target"`，这一条是修完前两处、拿干净克隆重跑一遍才暴露出来的：
命名卷挡住的是**内容**，挡不住挂载点那个目录，镜像里没有的目录是运行时以 root 身份 *在父挂载里面*
建的，而父挂载就是工作树。新门禁 `check-container-hygiene.py` 的判据是「谁把仓库挂进了容器」而不是
「谁的名字像容器入口」——按文件名划范围的话，改个文件名就能让自己免于检查。它同时把
`CARGO_TARGET_DIR` 指到 `/src/build` 判为不合格，也把只写在门禁命令里的 `mkdir` 判为不合格：前者换
个路径仍在挂载里面，归属没变，后者跑在容器那一侧，也就是这条检查不信任的那一侧。这类规则一旦按关键字
放行就会慢慢长回原样。

**判据：容器化的验证入口要同时回答「它读什么」和「它写回的每一处归谁」；凡是 bind-mount 进来的宿主
路径，写进去的东西就得假定 root 所有、宿主删不掉。要么让那个挂载只读，要么把所有写路径挪到命名卷与
宿主之外，并且连挂载点本身也要在宿主上先建好——命名卷只保证内容不落在工作树里，那个空目录照样是容器
造的。这些都要用一条静态检查把「谁挂了宿主目录、谁盖住了写路径、谁建了那个目录」打印成判决行里可核对
的事实。判定按挂载还是按文件名，是这类检查能不能被一次改名绕过的分水岭。**

## 2026-10-04：静态那条规则管的是形状，归属只在事后存在，所以另一半只能去量

`check-container-hygiene.py` 收工之后，它的 docstring 里留着一段「看不见什么」：运行时拼出来的挂载
路径、塞在变量里的 `docker run`。这段话不是客套，是那条检查的真实边界——它读的是 shell 文本，能禁止
的是它认得的形状。而「这个文件归谁」这种属性只在事后的树上存在：把入口改得再规矩，也不能保证下一个
往 `/src` 里写的东西不出问题。于是补了 `check-tree-ownership.py`：走一遍工作树，逐条比对每个路径的
`st_uid` 和根目录的 `st_uid`。

比的是根目录而不是字面上的 uid 0，这一点值得单独记：Windows 上每条路径报上来的都是 0，根目录也是，
按 uid 0 写规则就等于宣告整棵树都是入侵者，第一条真树 fixture 就会红。比根目录自己说的是「这里没有
东西跟包含它的目录作对」，那才是要害，平台分支一个都不需要。量下来宿主上 13083 条路径（含 `.git`）
0.165 秒，成本可以忽略，所以它是常规门禁而不是可选的诊断。

难的地方不在走树，在划界：得先说清哪些路径根本不属于这棵树。`target` 连自己带内容一起跳过，是因为在
容器里那个名字是 cargo 命名卷的挂载点，卷的根本就是 root 所有的——在那里判归属，第一条真跑的判决行就
是假警报，而一条会误报的门禁活不过下一次赶工；`/proc/self/mountinfo` 里的挂载点连子树一起跳过，同理
（那份文件把路径里的空格写成 `\040`，反转义这一步由变异 M19 接住）。`/src` 本身在容器里也是一个
bind-mount，但根不会是它自己目录列表里的一项，所以这棵树一定走得到，这条由 fixture 钉住。

配对的两半在同一个树上对比过：克隆按入口的方式挂进容器，容器里造一个 root 所有的
`__pycache__` 加一个 `.pyc`，静态那条报 `OK (… 0 problem(s))`（入口确实没毛病），新的那条报 2 处、
exit 1；回到宿主 `find -user root` 两条、`rm -rf` 报 `Permission denied`、exit 1。挂载点那条也是现场
量的：`--tmpfs /src/docs` 挂进去后带 mountinfo 是 OK，把挂载表指到读不到的文件上就报 `docs` 归 root。

28 例 fixture 里有一类是「只有特权环境才跑得起来」的：真改归属需要能撤销它的权力，否则 fixture 自己就
在开发者机器上造出它要报的那个损害。这一类在宿主上跳过、在容器里以 root 真跑——而它第一次真跑就抓出了
自己的 bug：`os.chown` 默认跟随符号链接，改了目标归属却把链接本身留在 root 手里，于是期望 1 条、实测
3 条。门禁是对的，fixture 是错的。「跳过」不写清楚就会变成「从来没跑过」，这一类必须在它真的能跑的环境
里跑过一次。22 个变异 0 存活（M5、M21 各 14 秒，因为摘掉 prune 之后走的是这台机器真实的 `target/`）。

**判据：一条静态检查只能禁止它认得的形状，凡是只能在事后测量的属性（归属、权限、谁写的），都得配一条
真去测的门禁；而真去测的第一步是划清「哪些路径不属于这棵树」——构建输出、命名卷、tmpfs、bind-mount
进来的宿主目录——划错了第一次真跑就全是假警报，规则会被连同问题一起关掉。比较基准要选树自己的根，不
要选某个具体 uid，否则没有 POSIX 归属的环境第一条用例就红。fixture 里凡是「只在特权环境执行」的那一
类，必须在特权环境里真的执行过一次并把输出记下来，不然它和没写没有区别。**

## 2026-10-04：「这条路径在不在」不能问文件系统，问了之后每个 checkout 各有一个结论

CI 上 `Documented commands can actually be run` 那一步红了，本地全量门禁却跑过它好几次。翻下来不是
文档写错，也不是规则太严：那条规则的上半截问 git——一条路径得首段是被跟踪的顶层条目，才算「对仓库内容
的断言」——下半截却用 `(root / path).exists()` 回答「它在不在」。一半 git 一半文件系统，就是这个形状的
分裂：`apps/chaos-ui/node_modules` 被 `.gitignore` 点名、一行没进过 commit，本机 `npm install` 之后它
在，runner 的干净 workspace 里它不在，同一个 commit 两套结论。

fixture 没把它救回来，因为那一例叫「对着本仓库跑一遍应当是干净的」，它读的就是开发者的工作树，跟着宿主
一起绿。这类 fixture 越像真实仓库越有用，也越容易把宿主的偶然状态当成仓库的性质吸收进来。

修法是把两个问题都交给 git：被跟踪的是内容，跟踪文件的祖先目录也是内容，`.gitignore` 点名的是仓库声明
自己不带的 generated 产物——读它是「前面还有一步构建」，不是「某个文件不见了」。这里还埋着二级坑：
`.gitignore` 那条写的是 `apps/.../node_modules/`，带斜杠的模式只匹配目录，而 `git check-ignore` 判断
「它是不是目录」又得回头看文件系统，所以在没构建过的树里问不带斜杠的那条会答「没匹配」。得一次问两种
形状（原样、带斜杠），否则分裂只往下挪一层。补的三例把契约直接写成断言：同一棵树的产物不在时跑一次、
在时再跑一次，两次的输出必须逐字节相同；镜像那一例钉住「没被跟踪也没被忽略、只是恰好躺在盘上」仍然是
断言——只修前一种误判的改法会放过它。

同一类盲区还有第三处：容器入口 bind-mount 的是工作树而不是干净 checkout，所以容器里跑的门禁也继承宿主
状态，依赖装好的机器上它会对着 CI 判定有问题的 commit 报绿。

**判据：一条检查只要声称说的是「这个 commit」，它的每一个输入就必须只由这个 commit 决定——git 的索引、
被跟踪的内容、`.gitignore`；只要有一个输入来自工作树的偶然状态（装过的依赖、构建产物、上一次会话留下
的目录），它的绿就只属于这台机器，红也只属于那台 runner。检验不是把本机输出多看一眼，是造两棵只差那一
个偶然状态的树，要求结论逐字相同；造不出这两棵树，就还没资格说这条检查是确定的。**

## 2026-10-04：守「值为空」的那条 `if [ -z "$1" ]` 守的是标志本身，`--only ""` 于是从未被拒绝

那条守卫的注释把风险写得比代码准：「空 pattern 匹配每一个标签，于是 `--only ""` 会是一次全量扫描」。问题
在 `case` 分支的那一刻 `$1` 是字面量 `--only`，`[ -z ]` 对它永远为假，所以那段只可能由 `[ $# -lt 2 ]` 那
一支生效——它测的是「值缺席」，不是「值为空」。空串于是顺利走进过滤器，而 `matches_only` 用的是子串语义，
空 pattern 匹配每一个标签。测量只改一个变量：同一份 `docker` 桩、同一套参数，只
差 `scripts/verify-in-docker.sh` 是哪个 revision，`HEAD` 那份 `rc=0`、36 次 `docker run`（预检 1 次
加 quick 表 35 条），判据行是 `all gates passed in stub:1`；改成判 `$2` 之后同样命令退出 2 且一次调用都
没有。

真正值得写下来的是那三条「被过滤的运行不可能被引用成全量绿」的规则为什么一条都没响。第一条「匹配不到任何
标签的 pattern 直接 exit 2 并点名它」，前提是存在一个匹配不到东西的 pattern，空 pattern 匹配全部；第二条
把摘要措辞交给 `selected != total` 这个判据，空 pattern 恰好让两者相等，于是带 `--only` 起跑的运行打印出
的正是 `--only` 那套设计专门留给未过滤扫描的那一句；第三条 `nothing ran` 只在确实一条没跑时成立，而这里
跑了 35 条。三条护栏的共同前提是过滤器正常工作。**一条防止误引的规则，如果它的触发条件依赖被它防止的那个
机制本身正常工作，那它在机制失效时与机制正常时给出同样的绿。**

宿主 runner 是同一个洞的另一半，而且那边更难看见。`--only "" --list` 什么都不打、退
出 0：`matches_only` 里 `[ -n "$pattern" ] || continue` 让空 pattern 跳过每一个标签（列表于是为空），
而 `--list` 分支打印完直接 `exit 0`，跑模式那条 `nothing ran` 护栏不在它的路上。于是「这条标签还不存在」
与「这条命令什么也没做」在 stdout 与退出码上完全同形。当天上午为「匹配不到任何标签」补的那条预检，恰好只
覆盖了非空 pattern 的那一半。

修复是两边各改一个字符（`$1` → `$2`），外加一句说明它凭什么在那里的注释——否则那半句读起来像多余的冗余，
下一次清理会把它当重复删掉。`--self-test` 47 → 50 例，其中一条是 `reject_line`「报错之前一条门都没跑」：
只断言退出码 2 的夹具，对一个「先把整张表跑完再报错」的实现同样给绿，而那种实现真实存在的代价是一
轮 1509 秒的宿主扫描换一个错误信息。`--only ""` 也不是假想的调用形式，它是遍历一个未设变量的循环所产出的
东西。

另一半缺口是这个容器入口从来没有自己的夹具：它拥有那张 `gates` 表，宿主 runner 逐条解析它，于是这张表从
读取那一端被反复检验，从被读的那一端一次也没有。新夹具把被测脚本逐字节复制进一次性 git 仓库（它
从 `$0` 推自己的 `repo_root`，复制才是封闭性的来源），`PATH` 前置一个把每次调用记成一行的 `docker` 桩。
桩不是 `dockerd`：它证明不了镜像能构建、卷里有什么、`/src` 挂载能否解析、真实工具的退出码对不对，它证明
的是「入口把什么 argv 交给了 docker」。这条边界要写在证据里，否则 26 例全绿很容易被读成「容器已经被验证
过了」。

**判据：写「空值或缺失值要拒绝」的守卫之前，先问清这一支里那个值是第几个参数，然后把你打算拒绝的那个值原
样喂进去，要求它退出 2 并且一条门都没跑；守着一个永远非空的 token 的守卫，测试永远是绿的，因为它唯一可
测的分支是另一个错误。同理，任何「被过滤的运行不许冒充全量」的规则都必须配一条「过滤器必须拒绝那个会让它
选中全部的值」——前者住在后者的下游，上游失效时下游必然沉默。**

## 2026-10-04：验收实验室确实存在、确实通过过，只是没有任何机器再跑它们

`scripts/*-in-docker.sh` 六个文件每一个都是一次真机验收：真镜像、真容器、真 nginx、真 TLS。清点的方式
不是问「有没有这个实验室」，是问「哪个 workflow 的哪一行命令里出现了它的文件名」——扫
`.github/workflows/` 的可执行行，六个里只有两个被点到名。剩下四个最近一次运行距当时 48 到 70 个提交，
而这段距离里 `install.sh`
改过、`docker/verify.Dockerfile` 改过、web 宿主改过。它们的绿色结论住在 `docs/verification/` 的转录里，
转录是死的：把 `install.sh` 改坏，没有任何东西会变红。

规则因此不能写成「仓库里要有实验室」，只能写成「要么被调度点名，要么在账本里留一条没过期的行」：
`check-lab-coverage.py` 对每个 `scripts/*-in-docker.sh` 要求二者之一，账本 `scripts/ci/docker-labs.tsv`
那一行要写脚本、转录、日期、当时的 sha、判决与理由，超过 30 天即红。没有豁免名单——npm 那条因此只能写
`red`，理由照实写（六个平台的 native 包从未发布，`win32` 那两个名字属于抢注的 `0.0.1-security`），而不
是把那一行从表里删掉或给它开个特例。

同一批还修掉了这类盲区的上游：三个 workflow 门禁各自硬写着 `[ci.yml, release.yml]`。这不是「清单少了一
项」，是「任何新增文件在被加入的那一天自动从三个门禁的视野里消失」，而本仓库这次新加的 workflow 正好就是
第一个。三者改为发现 `.github/workflows/*.yml` 的全部文件，发现为空即失败——「清单为空」不能以「全绿」
的样子通过。

**判据：一条只由人和转录验证过的验收等于没有验收。清点验收不要问「有没有」，要问「哪一行调度命令提到了
它」，答不出文件与行号的就是没有；同理，任何按名字硬写清单的门禁都要配一条「清单为空即失败」，否则它加入
的第一个文件就是它最后一次生效。**

## 2026-10-04：`| grep -c ''` 把「我读不到」折成 0，读不到的运行于是说出与干净树一样的话

容器入口在跑门前后各取一次树指纹。`fingerprint()` 的函数体是一条管道，返回值就是管道的返回值，调用处从不
看它；`git status` 那一半更彻底：`2>/dev/null` 丢掉唯一的解释，`| grep -c ''` 把前面发生的一切折成一个
数字。`git` 拒绝执行 `status`（object database 读不了）时那条管道退出 0、打出 `0`，于是「不知道工作区跟
提交是什么关系」的运行打印出的正是干净树那套输出——一句话都没有。

第二条比沉默更糟：树的 ignore 规则覆盖一切、索引被清空时，`git ls-files` 合法地列 0 条并退出 0，而 GNU
`xargs` 在空输入上仍会执行一次命令，`cksum` 没有任何文件参数、转身去读自己的标准输入，脚本于是打印
`== source tree: 1 files, checksum …`，跑完门并宣布 `selected gates passed`。那个 `1` 数的是和文件的
行数，不是路径数：它与「取了几条校验和」严丝合缝，与「仓库里有多少文件」毫无关系。

计数换了实现以后又踩到第二层：清单是 NUL 分隔的，而含换行的路径名在 git 里合法，GNU grep 3.7 对 NUL
分隔段落仍按换行计数，于是那条本用来让数字可信的消息自己把数字放大了，只能改成数 NUL 字节。夹具里那个带
换行的路径名不是装饰，它存在的唯一理由就是钉住这一层。

同一晚门禁自己拦下了本批的新代码：`count_paths()` 里那句 `bytes="$(tr … | wc -c)"` 是 `set -e` 脚本里的
裸赋值，`wc -c` 读不到输入时的非零退出经由赋值传给 errexit，脚本在正要打印解释的那条分支上被计数行掐断。
`pipefail report` 在整仓扫描里报了它，而 34 个入口夹具全绿——计数在夹具里从来都是成功的，只有读不到时
才非零。补上 `|| true` 之后重跑：门禁 `OK (… 0 hazard assignment(s))`，主机门禁 35 全绿。折叠数字的管道
要问上游拒绝时数字是什么，把数字取回来的那条命令本身也一样：它必须能在自己失败时把话讲完。

**判据：任何把上游成败折叠成一个数字的管道（`| grep -c`、`| wc -l`、`| head`），先问「上游拒绝时这个
数字是什么」；如果那个值与「一切正常」时的值相同，这条检查就永远绿。修法不是调大阈值，是让上游自己声明：
单独捕获退出码，把「列了多少条」与「应当有多少条」写进同一句话，并给「零条」单独一条出路——它既是合法
结果，也是最常见的失败形状。**

## 2026-10-04：一条断言要同一次 tick 既报重绘又数到结果，跑得快的守护线程让这两件事永不落在一起

`needs_animation_gates_prompt_history_tick_delivery` 在 2026-10-04 把两次 CI 各拖红一条，两次都是
`9199 passed; 1 failed`，都是 `finished in 62 s`，其余 job 全绿，而两个红提交都没碰过它附近。测试
自带的注释写着「CI 机器会把历史搜索的守护线程饿死」，并把 deadline 从秒级抬到 60 秒。这个解释与
症状无关：失败不需要机器慢，它需要守护线程**快**。

分清两个谓词是关键。`needs_animation()` 才是「面板还开着」，它的理由列表里就有
`history_search.is_active()`；`tick()` 返回的是逐项状态变化的累加值，是「有没有东西变了」，事件循环
的动画分支只在它点头时才请求重绘。`poll()` 每个 generation 只点一次头（`generation == last_gen` 就
返回 false）。而 `activate()` 在把条目发给守护线程之后会自己去读一次共享快照，并把读到的 generation
记成 `last_gen`。把这三条放在一起：守护线程若在 send 之后、那次读之前发布，`activate()` 返回时结果
已在手上、水位也已追平，此后每一次 `tick()` 都点头称「什么都没变」，于是「同一次 tick 既报重绘、
结果数又等于 2」永不为真——不是 60 秒不够，是这个条件不存在。

复现的弯路值得记：第一版探针在 `activate()` 之后睡 400 ms，改前的正文照样绿（0.41 s）。睡眠无法把
一次发布搬进一个 `activate()` 已经走出来的窗口。把 200 ms 插进 `refresh_items` 的 send 之后，交错
才被逼出来，改前正文 3.21 s 红在 CI 那句原话上，改后正文在同一交错下 0.21 s 通过。

修法在测试侧：结果数单独等，「报重绘」交给一条**激活之后**才发出的查询去验证——那条查询的
`result_count()` 只可能在 `tick()` 驱动的 `poll()` 里变动，重绘与投递于是落在同一个可观察点上。
产品代码未动：`activate()` 自带快照是刻意的，打开面板的事件自己会经输入分支重绘，一个在激活时就已到
手的结果集不需要向后来的 tick 欠一个变化标志。两个变异各接一条断言：删掉 `tick()` 里的 `poll()` 在
60.01 s 红在第一条，保留调用但丢掉返回值的在 0.01 s 红在第二条。

还有一处测量陷阱：变异还原用 `cp -p`，还原后的文件带着备份的旧 mtime，比刚为变异构建好的产物还旧，
cargo 认它新鲜，紧随的那轮于是报出 `259 passed; 1 failed`——那是变异二进制的结果，看着像修复自己引入
了第二次失败。**判据：凡是「断言同一次调用既改变状态又报告改变」的测试，先问这两件事是否可能分家；
若被测函数本来就允许其中一件先发生（抢先发布、提前返回、缓存命中），断言必须拆成两次观察，并把强断言
放到一个只有该路径能触发的后续动作上。变异还原之后要 `touch` 再跑，否则 `cargo` 会拿旧产物当新答案；
判据是 `cmp` 认字节、`touch` 认重建，二者缺一，数字就是上一轮的。**

## 2026-10-04：`-w '%{http_code}'` 是 curl 的一句话不是它的判决，探针把对照组接进被试那条路就什么都没测

给安装脚本补停滞下限时，两次测量陷阱各造出过一个假结论，两个都在写代码之前。

第一个：`curl ... -w '%{http_code}'` 的输出在传输被中止时**照打 `200`**，判决在退出码里。实测一条
128 B/s 的涓流在 `--speed-limit 2048 --speed-time 2` 下退出 28，而同一条命令的 `%{http_code}` 是
`200`。如果采信输出，代码就认定字节到手，body 只有 3072 字节，于是撞上 `min_bytes` 那道下限，报出
`too small (3072 bytes)`——一句指控「产物被截断」的话，而真相是镜像停滞。第二层：同一个中止换到
`-o /dev/null` 会变成退出 23，并且 `-w` 一个字都不写；23 不在 `--retry` 的可重试集合里，28 在。实测
`--retry 2 --retry-delay 1` 下真实文件那一路 8.03 s（三次尝试加两次 retry-delay），`/dev/null`
那一路 2.01 s（一次就放弃）。也就是说「把字节写到哪」同时改变退出码、状态输出与是否还有下一个候选，
而这三样恰好都是报错要引用的东西。

第二个更便宜也更隐蔽：为了复现上面那张表，探针里有个 `/fast` 路由本该整块全速吐 512 KiB，作为「下限
不误伤正常传输」的对照组。第一版把分支选择写成 `self.path != "/slowhead"`，于是 `/fast` 也落进
128 B/s 的涓流分支。它照样跑完、照样出数，只是那两行测的是被试路由而不是对照路由。露馅的是对照组自己：
`/fast floor=1024 rc=23`——一行本该证明「不受影响」的输出报的是中止。改法是按路由自己的名字选分支，
并且先读对照行再信实验行。

顺带一处同类：`--speed-time` 的窗口不是「字节在动之后才开始数」，它从传输开始计时，第一个响应字节之前
也算。3 秒才回头应答的端点在 1 秒窗口下 1.20 s 被掐、关掉下限则 3.02 s 装完。若照直觉把 0 字节那种失败
写成 `stalled under N B/s`，报错就描述了一段从未开始的传输。**判据：任何取自子进程的「事实」都要先问它
在失败时是否仍然被写出——`-w`、`%{time_*}`、被截断的 stdout 都属于「失败时仍会有值」的一类，判决只能来自
退出码，输出用来解释退出码而不是取而代之。写探针时先跑对照组：如果对照组的输出与它名义上的含义相反，那是
探针错了，先修探针再读实验组；两条规则同一个形状——先确认观察装置本身说的是真话，再拿它去说被测对象的
话。**

## 2026-10-04：一条校验和的证据强度不超过它对「量的是哪棵树」的说法，而只带一半 `cd` 的命令量到的是空气

上一轮留下一句判决：条数一致、校验和不同，所以差额在文件内容而不在读了哪些路径。这句当时就写进了日志，
今天推翻它的不是新工具，是把三种树形状各测一遍。`cksum` 逐文件写 `crc bytes path`，报出的数是对这整份
清单再取一次 `cksum`，于是一条路径对结果贡献两次：一次因为它在场，一次因为它装着什么。「文件条数」与
「校验和」因此是两个独立的问题，而旧的那行只带前一个数。实测最要命的一种形状是两个未跟踪路径互换：两棵
树都是 4021 条路径，路径集摘要 `2997202510` 对 `1581763344`，字节摘要也不同；把这一行与「只有内容动过」
那一种摆在一起，两者长得一模一样。另一种是 `mv` 而不同步 git 索引：条数不降反升（4020 → 4021），因为
清单同时带着搬走那条路径的幽灵与新名字，而 `cksum` 读不到前者——「读了多少条」这个数在这种树上连方向
都是错的。这也解释了入口为什么宁可在这样的树上拒绝跑门禁，而不是把能读到的那些加起来。

第二个陷阱是改这段代码的过程中被自己的夹具撞出来的：把 `fingerprint()` 拆成两半之后，`git ls-files`
进了 `cd "${repo_root}"` 的子 shell，`cksum` 那半留在脚本被启动时的目录里。清单里的路径是相对仓库的，
于是从别处启动时一条也读不到，运行在第一个门禁之前就退 1，一个门也不跑。拆之前那是一整条管道，`cd` 与
`cksum` 天生在同一条语句里；拆成两段各自可检查的半句，恰好把这条隐式绑定剪断——「让每一步都能单独报错」
这次换来一个更糟的整体，而且它对默认启动方式完全隐形。

第三处不是 bug 是证据纪律：被要求留证据却留不下的运行不能继续打印判决。`CHAOS_TREE_MANIFEST` 指到一个
建不出来的路径时，脚本在建镜像之前就退 2；目录已接受之后仍可能写失败（盘满、路径被换成文件），那时只
报错不中断，因为一次已经在报告门禁的运行不该被复制失败打死。诚实的残留是 `2692478763` 至今对不上任何
一棵树——当时的转录未留存、克隆已删；能对上的是当天另外两个裸校验和，它们各自在日志所称提交的一份干净
克隆里、由那份克隆自带的脚本原样重录。

**判据：任何用作「同一份输入」证据的摘要，必须在同一行说得出被量对象的身份（哪个目录、哪个提交，以及
它是一次完整测量还是残缺测量），否则读它的人会把「数字不同」误读成对差异的定性。写这种行时优先补一个
正交的数（内容摘要配路径集摘要、条数配字节数），而不是把措辞写得更强；改指纹代码时至少有一次从与出货
目录不同的当前目录跑它——一半命令带 `cd`、一半不带，这种形状从默认路径上看不出来。**

## 2026-10-04：两条测试的前提是「这段时间/这个端口归我」，可内核与 CI 机器都没答应

这一批的变异跑批把一个不相关的门测成了「被杀掉」。驱动脚本按「有没有具名的失败测试」判杀伤，
它分不清「变异让该测的东西变红了」与「另有一只本来就在抖的测试正好红了」：

    KILLED   m11-target-drops-parent-confinement: exit=101 failing=1
        remote::client::tests::a_transport_that_comes_up_later_is_waited_for

那只测试在 `crates/codegen/chaos-engine/src/remote/client.rs:2287`，与路径逃逸毫无关系。它自己
绑一个 `127.0.0.1:0` 拿到端口、`drop` 掉，再让一个任务睡 250 ms 之后回来占同一个地址，然后拨号，
断言这次拨号至少花了 200 ms——即「端口一开始不在，所以必然重试过」。它 panic 在 `client.rs:2312`
那句 `at least one retry happened: the port was not there first time`，本批跑批里见到两次。原因是
`drop` 之后那个端口号就还回内核的临时端口池了，而 Linux 的临时端口区间从 32768 起、正是 `:0` 分
配的那一段：同一时刻另一个测试目标（本批正在把整个 `chaos-engine` 包连同十几个测试目标一起跑）
调 `bind(:0)`，完全可能拿到同一个号并立刻开始 accept，于是第一次拨号就成功，200 ms 那句断言当场
不成立。测试要证的「等待存在」是真的，但它借的这件资源在它的整个前提期间都不属于它。

第二条只在 CI 上出现过，同一个任务在下一个提交上又是绿的（run 37194865831 的 `rust check |
clippy | test`，`test result: FAILED. 3211 passed; 1 failed; 3 ignored`；随后 `6826fce0` 同任务
成功）：

    thread 'implementations::lsp::tests::a_refresh_we_cannot_act_on_does_not_discard_what_we_know'
    panicked at crates/codegen/xai-grok-tools/src/implementations/lsp/tests.rs:86:5:
    no summary mentioning "a real problem" within the deadline

它等的是 `drain_until_reported`，预算 `WAIT_TIMEOUT + WAIT_TIMEOUT` 共 8 秒，中间每次
`drain_lsp_diagnostics` 让出 500 ms 再去 sleep 25 ms。被等的那个 mock 是一个 python 子进程：它要
先解释器起来、握手、在 didOpen 上发 `textDocument/publishDiagnostics`、再回答 `workspace/
diagnostic/refresh`（id 9002）。CI runner 是 4 核，整个 workspace 的测试挤在同一时刻，8 秒的墙上
时间在这种机器上不是「足够久」，而本地开发机上一次也没撞见过——这类差异不会出现在任何一份本地
跑批里，只会以「CI 偶发」的面目出现，然后被下一次绿默默冲掉。

两条的共性是断言依赖一个自己并不持有的资源：一条依赖端口号在 250 ms 内不被重新分配，一条依赖
一台机器在两秒内不忙。前者可以在测试里根治（换成与内核分配器无关的资源：一个 bind 住不放的具体
端口、或一个临时目录里的 Unix socket 路径——那个路径没有第二个进程会去占；本仓库已有的
`dial_unix_waiting` 正是同一套重试代码的另一条腿）；后者要改的是预算的来源，让等待跟着实际观察
到的进度走，而不是跟着墙上时间走。这一批没有动这两只测试，只负责把它们量清楚记在这里：那次误判
是靠在变异跑批里认出「失败的名字与本批改的东西无关」才发现的，重跑单只变异之后才有真的判决，
来龙去脉写在 `docs/verification/workspace-diff-undo-2026-10-04.log` 第 4 节。

**判据：一只测试若把前提建立在一件自己不持有的资源上（内核随时可以另派的端口号、别人正在用的
CPU 时间片），它就在每一次并发运行里重新掷一次骰子；写这种测试时先问「这段时间里谁还能拿到这件
资源」，拿得到就得换个不属于别人的资源（临时目录里的路径、自己 bind 住不放的地址）或换掉墙上时
间这个量纲。反过来，任何按「有没有具名失败」下判断的工具（变异跑批、flaky 隔离跑）都必须先确认
那个失败的名字属于本次被改的东西，否则一只在抖的测试能把无关的变异判成已杀——那比没有跑批更糟，
因为它给出了假的安心。**

## 2026-10-04：上一节点名的两只测试当天就修了，端口那侧量到了危害率，时钟那侧没能复现

修的方向与上一节给的一致。端口那一只不再向内核借号：`crates/codegen/chaos-engine/src/remote/client.rs`
里那只测试现在用一个常量 21787，低于每个系统临时端口区间的下界（Linux 从 32768 起，macOS 与 Windows 从
49152 起），开头仍然探一次，真被占用的机器会直接说
`port 127.0.0.1:21787 has to be free to test anything`，而不是因为一个它看不见的理由通过。危害是量过的：
把旧形状写成探针，八个线程按兄弟测试目标那样发号还号，四十轮里有三轮（复跑一次是五轮）在那 250 ms 里被外
来监听者占走；同一把 churn 之下 21787 是 0/40；`bind(:0)` 连抽 120 次全部落在 32768-60999 之内，一次例外
也没有。这就是为什么「绑一个 `:0` 拿到号再 `drop`」在这台机器上不是预约，只是抽签。修完之后在同一把
churn 里连跑十五次全绿，而同一时间那个旧形状的探针又丢了五轮。

时钟那一只换了量纲。`drain_until_reported` 的预算从 `WAIT_TIMEOUT + WAIT_TIMEOUT`（8 秒墙上时
间）改成 `DRAIN_ROUNDS = 15` 轮，一轮是一次 `drain_lsp_diagnostics` 加 25 ms 一口气。上限取自被
等的那些流程自己的需求量：helper 被临时改成报出它返回在第几轮，模块跑五轮（三轮常规、两轮
`taskset -c 0-3`），三十五次完成最大落在第 2 轮；十五又约等于旧预算在空闲机上按每轮约 525 ms 买
得到的轮数，所以上限没有放松，只是不再用秒计。

**这一批没能复现那次 CI 的红**：整个 `lsp::` 钉在四核（`taskset -c 0-3`）加八个空转进程、单模块
钉在一核加四个空转进程，两种预算形状各自三轮全绿（一轮 14.9 s 与 15.6 s）。所以这里不写「新预算
挡得住 run 37194865831 那一次」，只写两件能说的：旧的预算是别人也要花的秒数，新的预算是流程自己
做的交换数；以及新的会结束——把 shipped 的 `drain_lsp_diagnostics` 改成把刚取到的 summary 咽掉，
四条流程在 1.03 秒内全部以 `no summary mentioning "…" in 15 drains` 红掉，同样这四处旧写法各要
空等满八秒。两只测试的非空转性都按上一节那条判据做了：先确认基线绿才动 shipped 代码（不是改测
试自己），失败的名字必须正是被改到的那几只，最后 `cp` 加 `cmp` 复原到字节一致。全部数字与两段被
改前的源码在 `docs/verification/flaky-port-and-clock-2026-10-04.log`。

**判据：换掉危险的量纲不需要先把故障复现出来——量清楚「这段时间里谁还能拿到这件资源」和「被等的
东西实际需要多少」就够了；但复现不出来必须写明复现不出来，把「这样改就能挡住那一次红」写成结论
就是又一次给出假的安心。上限要取自被测量本身（流程真正用掉的轮数），而不是取自旧写法留下的那个
数字，否则换的只是单位，不是根据。**

## 2026-10-04：同一条「跳过四条编译腿」的判据当天又红了一次，这次把它从散文搬进 runner

上面那节的结论是「缺的不是工具，是收工条件」，判据要求引用一轮宿主门禁时把 `N run, M skipped`
原样写出来、不要写「全绿」。当天后面的两批把这条判据完整走了一遍：push 前跑过
`scripts/verify-gates.sh`，它打印了 `all gates passed on the host (35 run, 4 skipped)`，那四条
被点名的确实是被跳过的四条，人也照着判据读了这句；然后 push，然后 CI 的 `rust check / clippy /
test` 红在 `crates/codegen/chaos-engine/tests/workspace_diff_undo.rs:698` 那条
`redundant_pattern_matching` 上（run 37206776703），下一批什么都没碰这个文件、又红一次
（run 37208735399），顺带把 `platform tests` 那两条长腿 skip 掉。

差别不在有没有说真话，而在谁承担说真话的后果。那句判据管的是一次**引用**：它要求写日志的人把
skip 写全，它不改变 runner 自己给的结论。于是一轮什么都没编译的 sweep 仍然以退出 0 结束，
「4 skipped」的含义仍然要由读的人在脑子里补一遍「我这轮改的是 Rust，clippy 恰好只看 Rust」——而
这一轮改动在他看来只是「一行测试断言的写法」，正是最容易觉得不至于的那一种。凡是只能在人读到
的那一刻才生效的规则，都会在这种时刻失效。

所以规则搬进 runner：`scripts/verify-gates.sh` 在准备打印那句绿灯之前，把「这轮跳过了哪些门」
与「这轮改了什么」两件本来都在它手上的事实做一次交叉。归那四条腿管的是 `*.rs`、`*.toml`、
`*.lock`，加上 `GUI protocol types` 唯一读在 Rust 树外面的输入
`apps/chaos-ui/src/generated/protocol.ts`；改动取 `git diff --name-only HEAD` 与
`@{u}..HEAD` 两段，前者是「扫的时候还没提交」，后者是「先提交再扫」。命中就是
`NOT COVERED`、点名文件、退出 1。`--allow-unbuilt-changes` 留给确实不打算在本机编译的那些改动，
但它换不来安静：通过行下面必须多一行写明有几个文件没被测到。

被跳过的那条腿值多少也顺手量了：暖缓存下 `cargo clippy --workspace --all-targets --locked --
-D warnings` 2m52s、93 行 `Checking`/`Compiling`、退出 0。也就是说 fast loop 躲开的东西不到
三分钟，把一轮 Rust 改动往它那边推是很便宜的建议；真正的成本从来在 `cargo test --workspace`
那一侧，而它仍然是 `--with-build` 才跑的。规则的非空转性用两条变异验过（各红五条断言、先红退出
码），真实树上也跑了一遍：那只 `.rs` 改着未提交，sweep 最后点名它并退出 1，同批在 `scripts/` 下
的另两个文件不计。数字与两段变异前的源码在
`docs/verification/host-gate-unbuilt-changes-2026-10-04.log`。

**判据：一轮验证的覆盖范围如果取决于本轮改了什么，收工条件就不能写成「引用结果时补一句说明」，
要写成 runner 能自己拒答的形式——「这轮跳过了什么」与「这轮改了什么」两件事实都在它手上，让它们
交叉，命中就不给退出 0。凡是只能靠人读到日志才生效的规则，都按这一条重审一遍：它拦住的应该是
一次放行，不是一句措辞。**

## 2026-10-04：一个只让编译不过的变异不算判决，一份没写下来的断言和一条要等满 120 秒的测试也不算测试

这批的产物是一条能被点的按钮（Git 页的「建议提交信息」），配套的 21 个变异里 20 个死在被点名的测试上，
剩下的那一个把驱动骗了过去：它删掉的是 engine 去重闸门里 `suggest_commit_message` 那条 match 臂，而
Rust 的 match 是穷尽的，于是它红在 `E0004: missing match arm`，没有任何一条测试给出过判决，驱动却按
「编译失败 = 被杀」记了一格。这类臂删掉必然编译不过，所以「编译不过」在这里恰恰是穷尽性替测试代答了一
句它没资格答的话。补法是加一个能编译的同义变异：闸门保留、只把这一条消息豁免出去，行为与删臂等价而类型
仍然完整， `a_replayed_suggestion_request_asks_the_provider_once` 在它上面红。判据因此收紧一格：变异要
能编译，不能编译的变异要改写到能编译为止，或者如实记成「无判决」，两者都不许记成「被杀」。顺带驱动本身
也坏过一次——它用 awk 从 cargo 的输出里抓「失败测试名」，抓到的是 panic 文本，于是那张矩阵一度是 13
行看不清主语的字符串；改成只抓缩进的测试名行（vitest 侧另抓 `× …`）之后，矩阵才是 21 行「哪个变异让
哪条具名测试变红」（多出的 6 行是下面第四件事那一轮补的）。没有具名测试的变异矩阵不构成证据，这点和没
有断言的截图一样。

第二件事发生在截图的时候，与提交信息无关。给新面板拍证据图时， `工作区与会话` 底下是空的，屏幕上没有任
何东西说明这些面板指向哪个仓库；当时用眼睛过了一遍图，把它当成了「这个宿主本来就只有一个工作区」。把它
写成 Playwright 断言之后 6 次红 6 次，桌面与手机视口都红（ `element(s) not found`）。机制是
`connect()` 的先后顺序：套接字一开先 `list_workspaces`、同一拍再 `create_session`，而新宿主上恰恰是后
者才创建那个兜底工作区，于是清单在它本该列出的东西存在之前就被答完了， `CreateSession` 又只回
`SessionCreated`，侧边栏再也听不到这次变化。它活了这么久是因为面包屑一直是对的——它从会话上取工作区名，
所以屏幕永远有一处正确的证据在替这处错误的状态说话。UI 状态的缺陷最容易在这种「另有一处显示对了」的配
置里存活：看的人确认了名字出现，就没确认名字出现在该出现的那个列表里。

修在客户端（活动工作区不在清单里才补问一次，且按 id 去重，免得一个不答的宿主把它变成请求循环），引擎侧
追加 `Workspaces` 的方案试过又被否掉，否掉的依据是量出来的：5 个 WebSocket 测试在 `create_session` 之
后按位置读帧， `workspace_diff_flow.rs` 一个文件就有 9 处 `next_message`，多推一帧要写 9 行
`let _ = next_message(...)`，每加一行都是把一条断言削薄一点，为了省三行客户端代码去削薄九条断言不划算。
这条修复自己也被变异验过非空转：把新 effect 里那行 `send` 删掉，两个视口同时红，还原后 8/8 绿，全量重
跑 75 通过 / 6 跳过加 8 通过。数字、两张视口的截图结论与被否方案的完整代价在
`docs/verification/commit-message-suggestion-2026-10-04.log`。

同一批里还有第三件事，坏的地方不在被测代码里，而在测试自己身上。那条「外部 diff 驱动留下的后台进程不能
活得比这次读取久」的测试，第一版是空转的，而发现它的唯一东西是本该被它杀掉的变异活了下来：删掉
`process_scope.kill_all()` 之后测试依然绿，用了 120 秒。那一次什么都没被回收，sleep 只是自己跑满了 120
秒；让它能量出这个长度的是驱动脚本而不是被测的那段代码—— `sleep 120 &` 继承了 git 的 stdout，也就是
这条读取正在排空的那根管道的写端，读线程直到 sleep 自己退出才看见 EOF，于是这次读取的寿命被泄漏出去的
进程决定，等测试回头去看那个 pid，它已经因为一个与 `kill_all()` 无关的原因不在了。给那个后台进程的重定
向补上（ `>/dev/null 2>&1`）之后，基线 0.02 秒绿、同一个变异 5.06 秒红。顺带一提，这次的缺陷第一次被说
出来也不靠测试：直接 `Command::spawn` 被 `clippy.toml` 的禁令拦下、红在编译腿，一条测试都没跑；一条只
被 lint 盖住的性质，等于没有测试押在它上面。

第四件事骗过的不是某条测试，而是「这批验过了」这句话本身。engine 与 web 两条腿先前都按名字过滤跑过
（`cargo test -p chaos-engine --lib -- commit` 这类），两次都绿，于是这批在「已完成」的状态下进了一次
全量门 `bash scripts/verify-gates.sh --with-build`， `cargo test` 腿当场点出两个 target：
`-p chaos-engine --lib` 与 `-p xai-grok-web --test host_info_flow`。红的两条不是提交信息的测试，而是专
门负责「新协议消息有没有被登记全」的两条守卫：安全模式清单加了 `suggest_commit_message`，而
`every_client_message()` 的样本表里没有这条消息， `host_info_flow.rs` 的 per-tag 样本表里也没有。第一
条的形状最值得记：清单写着「这条要拒」，测试语料里却没有任何一条消息带这个 tag，于是
`safe_mode_allows` 对这条新消息一次都没被问过——绿灯说的不是「拒得对」，而是「压根没问」。第二条红的
位置根本不是断言： `.expect("covered by the other test")` 因为另一个测试的前提不再成立而炸，一处清单缺
失在另一条测试里以「不该被走到的分支」的形式显形。补齐两条样本之后又牵出一处 UI 缺口： `session.ts` 认
六种失败码，而这个 handler 会答八种，安全模式宿主答 `safe_web_mode_blocked`（ `host_info_flow` 正是逐
条消息在真 socket 上断言这个 code 的那条测试）、宿主重启之后答 `session_not_found`，而这两者都是那条按
钮唯一会收到的来信——后面不会再有建议过来，于是它停在「建议生成中」等一个不会到的东西。八种码现在收进
一个具名常量，engine 侧此前没有测试押上的三种失败（不认识的 session id、没配 Git adapter 的宿主、仓库
在宿主运行期间被删掉因而读不出暂存差异）各有一条测试，这一轮补的六个变异全部死在被点名的测试上。教训不
是「多跑一次」，而是跑法的种类：按名字过滤的跑法只对它过滤进来的那几条作证，改到协议消息、错误码这种跨
语言的清单时，收工条件要写成「跑受影响 crate 的整个 target」，而不是「跑与本批功能同名的那些测试」。

当时仍然没被盖住的一件，本轮盖住了： `apps/chaos-ui/e2e/` 有两份 Playwright 配置分别认领不同的
`*.pw.ts`，而「某个规格文件不属于任何一份 testMatch」这种情况当时没有东西检查——文件写得出来、
`tsc` 也过、 `npm run test:e2e` 全绿，只是它一次都不会跑。这类洞在只有一份配置的时候是隐形的
（那时 catch-all 覆盖了整个目录），加了第二份才成为可能。闭合它的那条门禁，以及「规则要照着工具
量出来的行为写、而不是照着对文档的理解写」那一步，记在下一节。

**判据：变异要产生判决才算被杀——只能让编译器不满的变异记作「无判决」，改写到能编译为止；驱动输出的必
须是「变异 → 具名测试」，抓不到测试名的矩阵按没有矩阵处理。同理，一个状态的证明必须写在该状态该出现的
那个位置：屏幕上另一处显示对了，不等于这个列表非空，凡是只有人眼过一遍的界面结论，都按「没写下来的断言」
计价；一条测试若只在被测代码不参与的情况下才通过，它的通过时长就是判决：该毫秒级返回的断言跑到分钟级，
按空转处理，而只被 lint 或编译器盖住、没有测试押上的性质，按没有测试计价；按名字过滤的测试跑法只给过滤
进来的那几条作证，改到协议消息、错误码这类跨语言清单时，覆盖判据是受影响 crate 的整个 target，不是与本
批功能同名的那几条。**

## 2026-10-05：宿主在 socket 层拒掉的请求不写收件人，于是「谁还在等」成了唯一的归因依据

补 Safe Web Mode 那份规格的第一轮红，红在被测应用自己身上，不在断言里。页面一连上就发
`list_workspaces`，会话建好之后那个 effect 再发一次，这两条在这个模式里都该被拒；而拒答只有一句话，
`safe_web_mode_blocked` 配「Safe Web Mode 禁止此操作」——它从 socket 里出来，不带 session_id，也不说
拦下的是哪条消息。于是用户还什么都没做，徽标已经停在「请求错误」，这一轮根本没人 prompt 过的对话还被
`recordTurnOutcome` 结算成一次失败。规格里那句 `toHaveText('会话已创建')` 收到的正是
`Received: "请求错误"`。这是本轮唯一一次由新测试自己把缺陷提出来的，其余两处是逼出来的。

逼的办法是先把断言写下，再把 `apps/chaos-ui/src/session.ts` 里对应的归因删掉、重建 `dist`、在真浏览器
里跑那份规格。删掉 Git 那条归因，两个视口同时红在 `Git 请求失败：…` 找不到——页签一打开就发的那次
`refreshGitStatus()` 被拒之后，「Git 请求处理中…」一直挂着、「执行 Git 操作」永久 disabled；删掉终端
那条，红在 `终端执行失败：…`。还原之后 `cmp` 报逐字节一致、重建、规格 3 绿。分支次序也归测试管：把
「没有面板在等」那句兜底挪到归因之前，5 条具名测试一起红。

断言自己撞过一次：同一个面板上可以同时挂着两条 `role=alert`（页签那次自动读取的失败、建议的失败），
`getByRole('alert')` 于是直接触发 Playwright 的严格模式违规，定位器改成带可选文本过滤的函数。两份提交
规格的仓库准备与控件定位收进 `apps/chaos-ui/e2e/support/commit-form.ts` 共用，各自复制的那一份会漂移，
而漂移的那一份照样全绿。

一处「看着像缺陷其实不是」记下来防下次重犯： `safe_web_mode_blocked` 早就在 `ATTACHMENT_ERROR_CODES`
里，附件那条腿本来就没坏，先顺手加上的 `(refusedBySafeMode && uploadIsInFlight(...))` 是永不为假的多余
条件，核对清单之后删掉；只把 `apps/chaos-ui/src/main.tsx` 里两处手写的
`['done', 'failed', 'cancelled']` 收成 `attachments.ts` 的 `uploadIsInFlight()`，让按钮的 disabled 与
「取消上传」的显隐读同一个判据。动错误码清单之前先查它在别处登记过没有，否则会写下永不成立的条件，而这
种条件永远不会红。

还有一次红是自己造的：全量 `npm run test:e2e` 退出码 1、日志一个字都没有，因为上一个 runner 还活着的时
候起了第二个。两份 Playwright 配置各自挑空闲端口， `.chaos/e2e-git-workspace/` 与端点的 prompt 日志却
是共用的，两个 runner 并发时读到的暂存区与日志都不属于自己那一轮。e2e 真正的隔离单位是宿主进程和它的工
作区，不是配置文件的数量——安全模式那份规格因此自起一个 `CHAOS_SAFE_WEB_MODE=1` 的宿主（这个模式是进
程级环境变量，与另一份规格共用同一个进程是不可能的），跑完关掉。

**判据：不带请求标识的答复由「谁还在等」认领，兜底句排在归因之后、次序由变异锁住，否则它会把没人发起的
对话结算成一次失败；共享夹具（同一份工作区、同一份日志）意味着整条 e2e 只能有一个 runner，「起了第二个
就红」按测试自身不合格处理；夹具收成一个模块，两份复制的夹具即使已经各自漂移也照样全绿。**

## 2026-10-05：两份配置、五处手写名单，而「没被收集」与「全绿」在退出码上长得一模一样

`npm run test:e2e` 的全部登记机制就是两张手写名单： `e2e-runner.mjs` 里那份配置清单，和每份配置里的
`testMatch`。它朝两个方向静静地骗人——一个 `*.pw.ts` 不被任何 pattern 匹配，Playwright 不收它也不说话，
退出码照旧 0；一条 pattern 匹配不到任何文件也一样，于是改名之后留在名单里的旧名字，继续替一份已经不存在
的覆盖作证。新门禁 `scripts/ci/check-e2e-registration.py` 读这两张名单和磁盘上的文件名，判六件事：
每个规格恰好被一份配置认领（认领指那条 `testMatch` 真的会被 Playwright 咨询，且规格落在该配置的
`testDir` 之下）、所有 project 都重写了自己的 `testMatch` 时顶层那份还选不选得到东西、每个 project
至少收到一份规格、每条 pattern 以及 `(?:a|b|c)` 里每个分支都还匹配得到东西、runner 点名的配置集合
等于磁盘上的配置、`package.json` 的 `test:e2e` 仍然调用那个 runner。它读不出来的东西一律算失败而不是
跳过，理由是这个主题特有的不对称：套件每一种坏法都长得像一次通过的运行，所以「什么都没读到」的门禁给出的
绿灯，正是这件东西存在的理由。

规则一条都没有照文档写。先写探针把仓库装的这份 Playwright（ `Version 1.63.0`）按各种配置形状跑一遍
`--list`—— `--list` 回答「会跑什么」而不启动浏览器，也不启动宿主。量出来六条里有一条纠正了写法，一条划
清了边界。最有价值的是「某个 project 收不到东西」这组对照：只有当一个配置**整体**什么都收不到时，
Playwright 自己才抱怨（ `Error: No tests found`，退 1）；只要还有一个 project 找到测试，那条空 project
就彻底沉默，退 0。也就是说「手机视口那条 project 其实一份规格都没跑」这件事，恰好在全套别的规格都绿的时
候是不可见的——需要门禁的就是这种沉默。另一条：glob 形状的 `testMatch` 是有效的（实测
`**/*.pw.ts` 照收），所以门禁对 glob 不是「拒收无效写法」而是「我读不出来就不判」；那条 finding 的措辞把
限制写成读者自己的，而不是假装工具坏了：

    this reader understands a regex literal, an array of them, or a const naming one.
    Extend it rather than let a pattern go unread

最容易踩的不是拼写而是优先级： project 的 `testMatch` **替换**顶层那份而不是收窄它，而
`playwright.config.ts` 的三个 project 各自重写了一份。只往顶层加名字（所有人都先看的那一处）等于什么都没
登记，那条 pattern 仍然匹配其余十二份规格，看上去毫无异样；本轮往镜像副本里注入这个错法时，门禁报的是
「顶层匹配它，但该配置每个 project 都设了自己的 `testMatch`，而它会替换顶层」。今天五处名单内容一致，所
以什么都没坏——而「五处得靠人手工同步」正是这句话的全部内容：同一个名字数要写五遍的套件，它是否在跑不是
一个可以凭眼睛确认的性质。四个错法（无人认领的规格、上述优先级、runner 不再点名某份配置、两份配置抢同一
份规格）都是在真实配置的镜像副本上打出来的，不是只在迷你夹具里；镜像之外的工作区一个字节没动。

fail-closed 的名单里有一处是写夹具时挖出来的真洞： flag 扫描读的是 `[gimsuy]*`，而 JavaScript 还定义了
`d`（hasIndices），于是 `/x\.pw\.ts/d` 被读成「没有 flag」——对仓库里现有的 pattern 恰好判对，作为一个
事实则判错，而且那种情况下本该说的「这个 flag 我不认识，所以这条 pattern 我不判」永远没机会说出口。现在
`[dgimsuvy]*` 都读，超出 `imsuvy` 的一律报出来。

非空洞性： 26 发变异 0 存活，其中 M23 第一版活了下来，而它活下来的原因值得单独记。抓它的那条夹具原本断言
`strip_js_comments` 原样带回一个字符类——这句话在扫描器懂不懂 `[a/]` 的两种情况下**都为真**，因为那个函
数对正则字面量本来就是整段复制；改成打在真正做决定的 `Config._read_regex` 上（说清它读出的 pattern 源码
是什么，而不是「复制一个来回没变」），M23 与它反面那一发 M25（去掉字符类跟踪，于是 `[` 不再开类、 `]` 也
无从关类）才同时倒下。第二处教训来自写证据日志本身：无人认领那条 finding 原本把整套 pattern 逐个列一遍，
在真实配置上等于同一串 11 个名字印三遍（顶层与两个视口 project 各抄一遍），三行噪声把唯二不同的那两条埋
了进去；改成每个不同 pattern 只提名一次之后，顺手把既有夹具加强去钉这个形状——因为在那之前没有任何测试
断言过这条消息长什么样，把它改坏不会有任何东西红（M26）。

**判据：一条门禁如果判的是「某样东西到底会不会被执行」，它所依赖的每一条工具语义都要有一次「用那个工具
量出来」的观测垫在下面，从文档读来的理解按没有验证计价；量出来与预想相反的那两条（glob 有效、空 project
只在整体为空时才被工具自己抱怨），恰恰是最该写进 finding 措辞的两条，否则门禁会把读者的注意力引向一个
工具根本没做的事。同理：「复制一个来回没变」这种对被整段复制的函数恒真的断言，按没有断言计价——断言必须
打在真正做决定的那个函数上；而一条从不被断言其形状的面向人的消息，等于没有消息，改动它的那一轮必须同时
留下一条钉住它的测试。**

## 2026-10-05：夹具的形状是人写的，所以它对外部工具一个字也不能证明

`THIRD-PARTY-NOTICES` 那一套（`scripts/notices_lib.py`、`check-notices-document.py`、
`check-notices-coverage.py`、`gen-third-party-notices.py`）在写下任何判定之前先去量了一次真实的
`cargo metadata --format-version 1 --all-features`，第一件事是推翻自己：过滤测试专用依赖那行读的是
依赖边上的布尔 `dev` 键，而 cargo 根本不产这个键。边的种类只在
`resolve.nodes[].deps[].dep_kinds[].kind` 里（本仓库实测 `null` 5448、`dev` 225、`build` 82），
`packages[].dependencies[]` 那 9984 条声明里带 `dep_kinds` 的是 0 条。那个过滤器从来没开火，读出的
是整张解析图 1210 个包，比真值多 71 个只经测试依赖到达的 crate；HEAD 那份文档里
`finl_unicode 1.4.0` 与 `wezterm-bidi 0.2.3` 两条条目走的正是这种边，它们各指一次 Part II 的
Unicode-DFS-2016，于是文件里替两个不进二进制的包背着 3042 字节、28 行的 Unicode 许可全文。

值得单独记的不是这个 bug，而是它为什么能在测试全绿的情况下活着：那批夹具是按「我以为 cargo 输出长
这样」写的，形状像真的，但它检验的是我对夹具的读法，不是我对 cargo 的假设——再多这类夹具也不会让
它红。唯一让它红的是把真工具的产物落到磁盘再数一遍。所以现在的夹具是从真实 metadata 的**形状**（含
`dep_kinds`、含同名两版本、含 build 边）构造的，而证据日志把量的那条命令原样贴出来，让下一个人重跑
而不是相信我的数字。同一轮里 `notices_lib.py` 关于 copyleft 的那段注释也这样倒下：它写着
`colored_json`（EPL-2.0）与 `terminfo`（WTFPL）「已经在发布的二进制里」，而按修正后的集合量的结果是
这两个都只经测试边到达；注释里的事实性断言同样要量，量不过就改注释，不是留着让下一个人误以为担了义
务。

`splitlines()` 造成两处同源缺陷：生成器自报行数 18747 而 `wc -l` 是 18738，换页符落在版权行中间时条
目只记下 `Copyright 2021 The`。根因是同一个——GNU GPL 全文按打印排版，每页结尾一个 `\f`（这份文件里
有 9 个），而 `str.splitlines()` 把它也当换行。这两处改之前都不可能红：仓库里没有一份夹具的许可证全文
带换页符（那些文本都是我手写的干净文本），文件里那 9 个 `\f` 对测试集是不可见的。现在各有一条夹具把
`\f` 放进去，其中第一条是拿 `wc -l` 的口径（数 `\n`）去断言工具自报的数，而不是拿同一个函数两边对拍。

prose 里的数字与路径是另一类无人看守的债。两处 docstring 点名的
`scripts/ci/check-third-party-notices.py` 是一个本仓库从未存在过的文件，而
`check-doc-path-refs.py` 只读 Markdown、不读 Python，所以没有任何东西会因为它红；把那条 guard 扩到
Python 字面量这轮没做——扫一遍得到 141 处指向未跟踪路径的提及，绝大多数是 `test-*.py` 里**故意**不
存在的夹具路径，先给它加一条「什么算真路径」的类别规则才谈得上扩边界。同批 docstring 与 `ci.yml` 步
骤注释里的 168/39/164 是拿那个被证伪的 1210 集合量的，现按修正后的集合改写为 135/28/191，而这三个
数出自把 guard 打在 HEAD 那份文档上的真实 FAIL 输出。

容器侧的降级是门禁自己说出口的，不是猜的：coverage guard 需要完整 workspace 的 metadata，登记在
`scripts/ci/docker-entry-ci-only.tsv` 并附原因；文档 guard、 70 例夹具、生成器测试在容器里跑；
`test-assemble-notices.sh` 在没有 npm 的容器里打印「npm not installed here; manifest allowlist
above is the only proof」。八发变异 0 存活（`dev` 键读法、 triage 认名字不认版本、关掉小节剪枝、剪枝
忽略正文提名、报错报错原因、末节区间吃掉收尾 banner、行数按 `splitlines()` 计、版权行按
`splitlines()` 切），每发之后源文件逐字节还原并 `cmp` 验证。

**判据：凡夹具的形状是人写出来的，它就只能证明写夹具的人当时怎么想；判定依赖某个外部工具的输出形状
时，那形状必须先由真工具的一次产物量出来，量的命令进证据日志，否则「测试全绿」与「读对了工具」是两
件事。同一口径也要统一：工具自报的数与 reviewer 能从磁盘得到的数（`wc -l`）不一致时以 reviewer 一侧
为准，并留一条钉住它的夹具；注释与 docstring 里凡是能被证伪的句子，都按代码计价，不按散文计价。**

## 2026-10-05：夹具里那条计数若是抄自被测对象的自报输出，它就在给缺陷背书

SBOM 那一套（`scripts/gen-sbom.py`、`scripts/ci/check-sbom.py`）写完生成器后，第一条真实命令是对本
仓库生成一份，它自报的是 `components: 1227 (third-party 1134, vendored 5, workspace 88); dependency
edges: 0`（那条命令与完整输出在 `docs/verification/sbom-2026-10-05.log` §2，命令要落盘所以路径在会话
私有的临时目录里，这也是它不进 `.md` 的原因）。

1227 个包、0 条依赖边。这不是稀疏图，是没有图：`depends()` 用 `child in known` 过滤，`known` 装的是
purl，`children` 装的是 cargo 的 package id，两个集合永不相交，于是每条边都被丢掉。当时那套夹具是全
绿的，包括那条看起来最硬的断言——基线 verdict 里写着期望的边数。问题恰恰在这：那个期望数字是从生成器
自己的输出上抄下来的。它测的是「生成器现在算出几」，不是「应当有几」。这类断言在缺陷存在的那一刻写成
绿色，并且在此后每一次把缺陷改对的过程中变红，也就是说它的方向和正确方向相反。

修法分三层，缺一层都还会漏。第一层是让判定不依赖生成器：`check-sbom.py` 只 import
`scripts/notices_lib.py`，绝不 import 生成器，自己从 `cargo metadata` 重新算一遍边集，并且两个方向都
比（少一条报 missing、多一条报 extra，附 `a -> b` 样例）。第二层是给真实路径一条错误答案满足不了的断
言：`RealWorkspace` 直接对本仓库生成再核对，断言 component 数与第三方数都 > 1000；0 条边那种答案在这
里连门槛都过不了。第三层是变异：校验器 24 发全灭、生成器 19 发全灭、0 存活，每发之后源文件逐字节还原
并 `cmp` 验证。现在基线断言写的是 `2 component(s): third-party 2 … 2 dependency edges`——这个数字来自
手算的两节点小图（root→middle→leaf 加 root→leaf），不是来自工具。

同一口径下另有两处只有变异才现形的判定。一是与 notices 文档对齐那条：原先「哪些组件算第三方」是拿构建
侧的 origin 分类算的，于是组件谎报 `chaos:origin` 时这一发照旧通过——要抓的谎报恰好是自己的输入；改
成按文档自己声称的 `chaos:origin` 算之后，才有那条「谎称 workspace 会红、vendored 谎报成别的不红」的
分辨力夹具（`test_the_origin_and_the_notices_membership_are_two_assertions`）。二是 purl 命名空间：形
状原先只从 `bom-ref` 解析，变异只改 `purl` 字段，于是那发漏网；现在 `bom-ref` 与 `purl` 两个字符串各自
验一遍形状、名字与命名空间。这两处的共同点是：判定看着在，输入却来自它本应怀疑的那一侧。

同一类失效在补齐 `hashes` 那一轮又来一次，这次藏在夹具的形状里。生成器有条规则：只给 crates.io 的包发
SHA-256，`path` 与 `git` 来源的包一个都不发，因为 cargo 只为它从索引下载过的字节写 `checksum`。矩阵里
对应那一发是「也给本地与 git 包发摘要」，它第一轮活了下来 —— 不是判定弱，是夹具的 `Cargo.lock` 里非注册
表包压根没有 `checksum` 行，变异体没有东西可搬，输出与原件逐字节相同。修法有两半：把变异改打在真正做决
定的那个 `else:` 分支上，以及把夹具改成能表达这句假话（给一个非注册表包也写上 `checksum`），再生成器和
校验器各补一条 refusal 断言。前者让那发跑得到那段代码，后者让那发跑得出区别。一个表达不出假话的夹具，
等于没有断言。

校验器那条顺带长出一层：它现在把 lock 文件本身也当证据读。非注册表包带 `checksum`、同一个包两处写两个
摘要、摘要不是 64 位小写十六进制，三种都算 findings —— cargo 写不出这样的文件，所以这样一份 lock 里的
摘要不是关于任何事的证据。夹具现在 70 例，`Digests` 一类占 17 例。

上游 schema 的处理是同一件事的反面。`bom-1.6.schema.json`（262 666 字节）取下来用 `jsonschema 3.2.0`
对真实文档验过一次：0 个错误，但这道验证刻意不进仓库——那份 schema 本身是一件需要许可证条目的再分发
物，而 CI 容器里没有 `jsonschema`。也就是说线上跑的是手写校验器，它的不空转没有任何外部权威可以借，
只能由变异矩阵自己承担；「本机对着官方 schema 验过一次」这句写进证据日志，供下一个人重跑，而不是当作
CI 红绿的替代品。

**判据：夹具的期望值如果来自被测量对象的自报输出（stdout 的一行、工具自己写的报告、它此刻的 exit
code），这条断言按没有断言计价，因为它在缺陷存在的那一刻自动追随缺陷；期望值必须能从另一个方向手算或
独立重算出来。判定所依赖的输入不能来自它本应怀疑的那一侧——用构建侧的分类去校验文档侧的声称，等于让嫌
犯作证。而当一道校验刻意不借助任何外部权威（schema、官方工具、第三方服务）时，它的不空转只能由变异证
明，且那些变异要打在真正做决定的那个字段上，打在旁边那个字段上的变异会给出虚假的存活率。打在规则上的
变异还要夹具里有能让那条规则出错的输入：两类输入都不存在的夹具里那发必然存活，而存活的原因是夹具表达不
出这句假话，不是判定弱。**

## 2026-10-05：端到端夹具的覆盖面，等于它构造出的前置状态，而不是它调用过的函数

npm 那套完整性记录（`bin/install-lib.js`、`scripts/ci/check-npm-integrity.py`）第一轮矩阵 79 发留下
6 发存活，本轮打死 5 发。打死的动机都不是「覆盖率数字难看」，而是每一发都对应一句写在代码里、却没人
能证伪的声称。五种坏法里至少有三种是同一类夹具缺陷的不同投影。

第一种：夹具的覆盖面等于它构造出的前置状态。`installVersionedBinary` 有个 `verifyInstalled` 参数，
安装器传 true，含义是「这个 home 里已经在位的那个文件也要被摘要核对」。变异体把它改成 false，活了——
因为端到端那套夹具每次装的都是一个全新的空目录，`fs.existsSync(versionedPath)` 那一支从来没进过。
函数被测过，那条分支没有。补的那行是「装好、把 record 删掉、再装进同一个 home」，一次调用就把前置
状态造出来了。同一件事的另一个版本是启动器那发：删掉「record 读不出来就直接运行已装好的二进制」那
一行，变异体不是静默通过，是当场崩在一句 `integrity.record.binary.bytes` 上——可此前没有任何一条
用例会带着「record 不存在」去调用启动器，所以连崩都没崩给谁看。一个入口被多少个不同前置状态调用过，
才是它的覆盖单位。

第二种：只试一个错误值的断言，测的是那个值，不是那条规则。guard 拒绝 schema 拼写不对的 record 这条
早已有断言，但它只试 `chaos-npm-integrity/0`；把 guard 改成「`/1` 和 `/2` 都接受」立刻活下来，因为
`/2` 不在被试的输入里。这类变异不是判定弱，是判定的输入集太窄，而窄输入集在「我们只用当前这一版
schema」这句话面前永远显得合理。现在 `/0` 与 `/2` 各一条：往前的拼法和往后的拼法都得被拒。

第三种，也是这轮唯一一处真要改代码的一处：廉价检查的 fall-through。安装失败之后启动器会退回平台包里
那个未压缩的 `bin/<name>`，而对它只比长度——这是刻意的（150 MB 的哈希不该出现在每次启动上）。变异体
把「安装失败的原因是摘要不符就拒绝运行」那一行去掉之后，一个被 pad 到记录里那个长度的假二进制被原样
跑了。此前所有夹具构造的长度差异都是「更短」或「更长」，于是那条只比长度的路径在夹具里从来没被要求
说错话。要测长度检查，夹具必须能造出**长度恰好相等而内容不同**的输入；造不出这种输入的夹具，对长度
检查等于没有断言。

两处判定粒度也被纠正。一是拒绝的**理由**属于契约：`compressed.sha256` 不是一条摘要时，去掉形状检查
仍然会拒绝，但归因从「这份 record 不可用」变成「摘要不符」，于是用户听到的是「你的包被动过」而实际
是「这个包没写清自己是什么」。只断言「拒绝发生了」的测试对这两种给同一个绿，现在 reason 与措辞分别
钉住。二是拒绝要断两件事：退出码非零，以及 chaos home 里没留下可执行文件。后者用 `find … -type f
| wc -l` 而不是 `ls`——空目录和不存在的目录在 `ls` 下同样安静，前者会让一条断言在什么都没写和写全了
之间给不出分辨。

最后一发（`install-lib.js` 里 swap 之后那条 `dangling` 后置检查）本轮选择留着不打死。它成立的前提是
swap 报告成功却没留下能解析的名字，而 `swapCanonical` 要么留下能解析的东西要么抛错，所以没有调用方能
把它走进去；它是留给「对一次写入报告成功但实际没写」的文件系统的。它被记作未覆盖，注释里写明为什么
没有夹具，而不是配一个假造文件系统的测试把绿色拿回来——对假文件系统证明的行为，不能兑换成真文件系统
上的任何保证。

**判据：端到端夹具的覆盖面按「它构造过多少个不同的前置状态」计价，不按「它调用过哪些函数」计价——
一条只有在全新目录里才会走到的分支，等于没有测试。规则类判定要按等价类给输入（往前的拼法与往后的
拼法是同一类，各试一个值只证明那个值），长度类检查必须有一条长度相等而内容不同的输入，否则那条检查
在夹具里说不出任何错。拒绝类断言要断到 reason 和文案，还要断「有没有留下东西」，并且不要用 `ls` 去
判断一个目录是不是空的。跑不到的防御分支就记为未覆盖并写明原因，用假造的依赖补出来的绿，比缺口本身
更贵。**

## 2026-10-05：夹具的目录形状写错，六条断言就全绿着测另一件事；矩阵的期望串写错，被抓到的变异记成存活

`crates/codegen/xai-grok-shell/src/agent/auth_method.rs` 的 `mod tests` 门着 `#[cfg(any())]`。空
`any()` 恒假，rustc 在名字解析之前丢掉整个条目，于是那个模块 23 条测试不进任何构建、不进 `#[ignore]`
台账，也被 `panic-site-census.py` 记成生产位点（它的 `cfg` 求值器只问「是否**要求** `test`」，空
`any()` 什么都不要求）。逐提交读这个属性：2026-07-16 还是 `#[cfg(test)]`（21 条测试），2026-07-26 第
一次换成 `#[cfg(any())]`，两次上游同步（2026-08-03、2026-08-31）把它冲回真属性，两次 fork 层合并
（2026-08-07 的 "merge fixup: shell fork-layer re-merge (WIP round 1)"、2026-09-04 的 "sync upstream
changes into chaos 0.3.1"）又盖回去，并且盖回时同时把上游较新的模块版本换掉（24 条变 21 条、24 条变
23 条）。没有一处信号会响——这不是疏忽一次，是同一个决定被合并反复重新做出。三件事随之而来，其中两件
是本轮补的门禁与口径，第三件是关于矩阵自己的两条账。

**新门禁把「永不为真」当成一类错误。** `scripts/ci/check-dead-cfg.py` 把 `crates/` 与 `bin/` 里每一条
`cfg`、`cfg_attr`、`cfg!` 谓词折成三值（真 / 假 / 看不清），只报判为假的；`cfg_attr` 只折第一个逗号之前
的谓词部分，因为它的条目**永远**构建。读数与扫描器共用 `scripts/cfg_lib.py`：两处各写一个 `cfg` 求值
器正是这类盲区的来源。这份库第一次放在 `scripts/ci/` 时把自己搞红了 —— `check-guard-wiring.py` 把该目
录下每个 `.py` 都视为一条必须被某个入口跑起来的门禁，而 `HERE / "cfg_lib.py"` 里不含带斜杠的路径 token，
于是它既是库又被迫当门禁。库挪到 `scripts/`（`scripts/notices_lib.py` 的先例），那个红起来的门禁
顺手证明了自己有效。

**扫描器多了第三种状态。** 「永远为假」不是测试代码也不是生产代码，是没有任何构建编译它：这类 span 从生
产列摘出，但不并入测试列。推论是模块声明也归它管 —— `#[cfg(any())] mod x;` 丢掉的是声明本身，被点名的
文件属于 uncompiled 台账（`scripts/ci/uncompiled-sources.txt`）。同一棵树两个扫描器：生产 unwrap
300 → 293、生产 expect 595 → 589，unsafe 不动；`xai-grok-shell` 从 `46/116/28/13` 变 `39/110/28/13`，
`--list` 从 203 条变 190 条。

**M13 那发是夹具在测另一件事。** 为了守住上面那条推论，夹具写了一对文件：`src/dropped_decl_host.rs`
里 `#[cfg(any())] mod dropped_decl;`，旁边放着 `src/dropped_decl.rs`。六条断言全绿，把
`module_declarations` 里那句 `continue` 换成 `if False` 之后输出逐字节相同。原因是 Rust 的解析规则：
`src/host.rs` 里的 `mod x;` 找的是 `src/host/x.rs`，所以那条声明根本不解析到任何文件，被"丢掉"的文件是
因为「没有任何根声明它」落进台账 —— 那正是同目录 `nobody_declares.rs` 落进去的原因，两条断言测的是同一件
不相干的事。改成 Rust 真会有的目录形状（`src/dropped_decl_host/mod.rs` 声明同目录的 `dropped_decl.rs`）
之后，同一发让 `unwrap_prod` 从 13 变 14、uncompiled 台账从 2 个文件变 1 个，六条断言同时红。夹具注释里
把这条解析规则写死了，免得下一个人为了省事把两个文件拍平回 `src/`。

**M14 那发是判据打错了对象。** 变异是把容器门禁列表里那条 `dead cfg` 删掉，`check-guard-wiring.py`
当场非零退出，指名那两个文件「runs in CI but not in scripts/verify-in-docker.sh」——抓到了。矩阵却记成
存活，因为它拿门禁的**标签**（`dead cfg`）去比对，而 guard 打印的是它够不着的**路径**。判据改成
`scripts/ci/check-dead-cfg.py` 之后同一发立刻记为被抓。这类错的代价是双向的：期望串写宽了会把无关的失败
算成判决，写窄了会把真判决记成存活，两边都让存活率不再等于"这条断言有没有用"。顺带一条同族教训：
本轮矩阵的早期版本只数了 `old` 出现几次却没真的把 `new` 写回去，14 发全是 no-op 却报告"跑完了"——一个
从不生效的矩阵和一个全都抓到的矩阵，日志长度是一样的，所以矩阵自己也必须先证明它会失败（本轮做法是手工
打一发 M1 看它是否真的让某条测试变红，再把还原后的树 `cmp` 回原件）。

**测试文本的去向也记账。** 模块回到 `#[cfg(test)]` 后 23 条逐条判定：11 条删（断言的是分叉已删的排序与
pin 语义，模块头按名字逐条写理由）、11 条改写留下、1 条折进分叉不变量、3 条新增。改完 20 条（4 条
runtime key、14 条本模块、2 条分叉不变量），后者跑 `preferred_method` × 5 个布尔 ×
`login_label` 的 96 种组合，钉的是「这 5 个输入字段没有一个能改变输出」——一个恒等的构造函数仍然需要
有人每次改掉它时让 CI 变红。

**判据：一条规则如果被打在它上面的变异不改变任何输出，它就没有测试，无论夹具多少条断言是绿的；判断这件事
只能靠真的打一发了再看输出，而那一发必须逐字节确认变异写在盘上。夹具要按被测语言真实的解析规则构造——
目录形状错了，声明解析不到任何文件，断言就在为另一件事作证；表达不出那句假话的夹具等于没有断言。矩阵的
期望串要打在对方真实打印的内容上，标签、别名、人记得的措辞都不算。而当一条 `cfg` 的假值来自构造（空
`any()`）而不是拼写错误时，编译器不会给任何信号：这类东西的存活率只取决于有没有人把「永不为真」当成一类
错误去扫。**

## 2026-10-05：矩阵里那句「结构性不可达」，一条是编译期事实、一条是运行时配置事实，后者没人给它计时

`crates/codegen/xai-grok-shell/src/agent/models/startup_prefetch.rs` 剩着的 16 个生产 `.unwrap()` 全是
那两个 `Mutex` 的加锁调用，而这个模块本来就是为「预取线程死了」写的：`FinishGuard` 在展开途中把 fetch
标成完成，`State::panicked` 记下它发生过，`accept` 打日志并空手启动。偏偏唯一被计划好的那次失败，同时
也是让之后每一个读锁的人跟着 panic 的那次 —— `begin_inner` 是在持有全局注册表 `INFLIGHT` 的路径里
`std::thread::spawn` 的，系统拒绝建线程时注册表当场被毒化；`FinishGuard::drop` 取的是状态锁，而它恰好
只在已经展开的路径上运行，展开途中再 panic 直接 abort 整个进程。16 处换成
`unwrap_or_else(PoisonError::into_inner)`：两个被守护的值跨 panic 都结构完好（`State` 只有 `Option`
字段，注册表是 `Option<Arc<_>>`），这里根本不存在「中毒时必须谁都读不到」的不变量。

**14 发变异，2 发存活，两发的理由不是同一种东西。** M14 是编译期事实：那三行写在 `if cfg!(test)` 的
另一侧，单测构建里那个分支不参与编译，谁都打不到它，这条豁免不会腐烂。M8 是运行时配置事实：它是
`wait_settings` 最后一行的读锁，挡在 `still_accepted` 之后，而后者读的是磁盘上的
`resolve_remote_fetch_enabled()`（默认关、故意不提供环境变量覆盖、单测不得改写真实 `$HOME`）。门关上
时那行代码在单测里永远走不到，于是「打不到」与「缺一条测试」在这一发上给出逐字节相同的输出。区别在于
M8 的豁免只在门保持关上的这段时间里成立：外发开关哪天多一个测试接缝，它立刻变成一条真缺口，而现场不
会有任何东西变红来告知这件事。矩阵的日志因此把这两个名字后面写的原因当作被检对象的一部分 —— 记下原因
的唯一意义是它有一天会失效，而失效的方式是外发的形状变了，不是有人去读这段日志。

**第一轮矩阵的红退出码抓到的不是产品缺陷，是测试自己的洞。** 判据把存活分成两类：「不可达」按名字豁免，
「与预期相反」让整轮非零退出。第一轮报的是 `11 caught, 3 survived, 1 contrary to expectation`，第三条
正是 `Finished::take` 里那次状态锁读取 —— 它活得好好的，因为既有测试只毒化过注册表，从没有一次带着中毒
的 `state` 守卫走进 `accept`。补一条专门毒第二条守卫的测试之后，第二轮它由**恰好一条**测试杀掉，那是
最紧的信号：一个位点，一个读者。如果判据只报「2 survived」（或者更糟：只报存活率），这一发就会和两条
真豁免混在一起，绿着过去。矩阵的期望表因此不是文档的复制品，它是这轮唯一在读取「覆盖」这件事的断言。

**环境相关的期望不能写成 skip。** `resolve_remote_fetch_enabled()` 默认关，一条想断言「设置被交到手上」
的测试在这个宿主和 CI 上都得 skip —— 而 skip 在退出码上与全绿长得一模一样，那是本篇 2026-10-05 早先那
节记过的同一件事。两条测试的写法是：期望值从被测试代码读的同一个旋钮推导，另外再断言两种分支下都必须
成立的不变量（被消费的 fetch 必须从注册表摘掉、之后的 `inject` / `clear` 仍能穿过中毒的守卫）。这样门
开着还是关着都能跑，且两边都真的在断言什么。恢复数据但不清除中毒状态这件事也有一条测试钉住
（`assert!(guard.is_poisoned())`），否则下一个人会把它「整理」成 `clear_poison()`，别的模块的 `.unwrap()`
就此再也看不见「有持有人死了」这个信号。

**判据：把变异判成「不可达」是一次豁免而不是一次测量，它必须带上一句能被判定的理由，还得说清那句理由
靠什么成立 —— 编译期形状可以不管，运行时配置得有人计时。存活率相同的两发可能一个是盲区一个是洞，把
它们分开的只有矩阵自己那条按名字的期望；同理，一条在默认配置下必须 skip 的断言等于没有断言，期望要
从被测代码读的同一个旋钮推导，再把两种分支下都成立的不变量单独钉住。**

## 2026-10-05：判据是退出码的矩阵，必须先有一格什么都没改；被 `Drop` 吃掉的变异体表现为 abort，而期望表的错会把测试助记写成产品覆盖

`xai-grok-shell` 读 `std::sync` 锁的两种脾气（中毒即 panic / 当场恢复）这一批被并到一对共享 trait 上，
28 个 panic 站点分布在 11 个文件里。批量的覆盖只能用变异矩阵量：改一个字节，看测试是否变红。这一批在
这件事上踩了三个坑，每个坑都会让「有覆盖」这句话在绿着退出时被说出口。

**矩阵的第一格必须什么都没改，而它第一次就是红的 —— 红在环境形状，不红在代码。** 判据只有一个来源就是
退出码，这隐含「什么都没改时它是绿的」。第一轮的 `B0` 直接 abort：
`session::acp_session::cancel_running_task_tests::handle_prompt_frames_interrupt_on_user_message has
overflowed its stack`。原因是矩阵用裸 `cargo test -p xai-grok-shell --lib` 跑 FULL 格，而出厂的两个 runner
（`scripts/verify-gates.sh` 的 `RUST_MIN_STACK_DEFAULT=16777216`，容器那边 `--env RUST_MIN_STACK=`）都会导出
这个栈底线 —— 也就是说矩阵跑的那条命令，跟没有任何一个门禁跑过的命令长得像但不一样。同一格导出栈底线之后
`exit=0`。**更糟的是后果**：`B0` 之外，两个 FULL 格（12 个 actor 站点、4 个杂项站点）也被同一次 abort 判成
「被抓到」，于是「缺覆盖」这两格在 30 格里读起来像「有覆盖」，方向恰好相反。矩阵现在改成：基线格不是绿的就
用独立退出码中止整轮，而不是让后面 29 格挂在一个坏基准上出结论。

**同一个 harness 的退出码当时是恒等于 0 的。** `return 0` 无条件写在收尾处，所以第一轮那 9 个
「与预期相反」是把警告打印进一个绿色退出码。凡是把判定写进日志的脚本，必须自己检查「判据不成立时进程是否
非零」，否则它只是一个会打印真相的旁观者。

**期望表的依据必须是「这是不是产品站点、有没有能构造出中毒前置状态的读者」，而不是「这行我改过」。**
第一轮 `T2`/`C4`/`A4` 三格被判为「与预期相反」，它们的真身是 `set_depth_for_test`、
`reset_marker_cache_for_test`、`reset_runtime_api_key_for_test` —— 全在 `#[cfg(test)]` 之下、没有任何出厂调用者。
变异体在这里只能通过测试代码被观察到，为它写测试就是拿测试脚手架当被检对象。这也解释了为什么 28 处改写只让
`expect_prod` 减 24：计量口径本来就把 `cfg(test)` 的跨度排除在外，把「改过的行数」当成减量的来源会把它写成
28（再把 10 处「本来就恢复、只是各自拼了一遍 `unwrap_or_else`」的收编算进去，就成了 38）。豁免要按名字带
理由写进矩阵自己的期望表，理由要能被下一个人判真伪。

**被 `Drop` 覆盖的站点，它的变异体是 abort 而不是断言失败，把它记成环境噪声就等于把这批要修的缺陷丢掉。**
现场原文是 `panic in a destructor during cleanup` / `thread caused non-unwinding panic. aborting.` /
`signal: 6, SIGABRT`，整条 cargo 命令异常终止。这一类必须算「被抓到」。

**"覆盖面 = 夹具能构造出的前置状态" 这一批反过来咬了一口。** 12 处守的是 session actor 的
`current_prompt_id`：要让它们在中毒的守卫上被读到，得有一个活着的 actor 会话先发布过 prompt id 再把那格毒化，
单测没有这个接缝，把恢复换回 `unwrap()` 之后全库照样绿。诚实的处置是让它们在矩阵里以「存活」出现、带原因，
并落成 TODO，而不是从批次覆盖面里悄悄摘掉，也不是给 helper 自身补一条永远碰不到出厂调用点的测试。反过来，
另外三处（`BlockingWaitGuard::enter`、`is_claude_import_marked` 里那次填缓存的写、`set_runtime_api_key`）确实是
测试的洞：中毒之后只读过、只清过，没写过。补法不是加断言，是换前置状态 —— 先毒化再 `enter`、缓存留空让冷路径
真的去读盘再写回、毒化之后再发布一次 key。三格立刻由绿变红，且各自点名到那条新测试。

**判据：退出码型变异矩阵必须自带一格未变异基线，且该基线不成立时整轮作废；写矩阵的人要单独验证「判据不
成立时进程非零」；每条存活豁免必须写明它是产品站点还是 `#[cfg(test)]` 助记、以及缺的是哪一种前置状态；
`Drop` 里的变异体 abort 属于被抓到；空过滤器与未覆盖在退出码上同形，所以每一格还要留下 `N passed` 的 N。**

## 2026-10-05：作业唯一的天花板是 `timeout-minutes` 时，测试里一个等不到事件的裸 `await` 就把「2 秒能命名的失败」换成半小时静默

CI run 37259667663（`a8274d9f`）被判 `cancelled`，用的恰好是 `.github/workflows/ci.yml` 的
`timeout-minutes: 60` 那一格。日志里最后一个完成的测试打在 04:00:01，libtest 自己的看门狗在 04:00:55 点名
`session::workflow::manager::tests::active_run_admission_is_bounded_per_session`，此后 29 分钟再没有一行输出，
直到 runner 在 04:30:09 杀掉 `cargo` 和测试进程。整个作业没有 report 失败，它只是不再说话 —— 而一份「没有失败」
的日志和一份「还在跑」的日志长得一模一样。

**「慢」和「死」在退出码上是同一个形状，除非等待自己带期限。** 那条测试在它唯一的断言之前只有一个没有期限的
等待：`subagent_rx.recv().await`。同文件早就有一个带 2 秒期限的 `recv_spawn`，还有 4 条测试在用它 —— 其中
`parallel_panel_respects_concurrency_cap` 与 `cancel_drops_queued_spawns_before_coordinator` 恰恰在这次被拖死的
run 里跑完并 ok。同一个 runner、同一类事件、同一个 2 秒：这既说明这个期限在那台机器上够用，也说明「该走哪条形
状」在本仓库里早有先例（`scheduler/actor.rs` 的 `next_event` 是第三种同一形状，3 秒）。这一批把模块内 15 处裸
`recv().await` 全收进 `recv_spawn`，并给它加一个 `what` 标签，让期限到点时说的是「哪一个 spawn 没来」，而不是
一个模块级的泛指；测试代码里的 `.recv().await.expect(` 从 84 处降到 69 处，`manager.rs` 归零。

**期限要被单独钉一次，否则重构会冒充修复。** 把测试自己的中继任务改成丢弃 `Spawn` 事件：改前的源码真的挂死
（30 秒硬超时被杀，整段输出只有 `running 1 test` 一行），改后 2.06 秒失败并打印
`spawn event: no spawn event within 2s`。可这还证明不了救场的是期限而非重构 —— 于是把 `recv_spawn` 的超时分支
换成 `pending()`，其余一概不动，测试又挂回去（30.01 秒）。同一批还得有一格反向证明重写没削弱测试本来要抓的
东西：把准入阈值改成 `>= 1000`，`active_run_admission_is_bounded_per_session` 仍然红（0.79 秒，红在
`called \`Result::unwrap_err()\` on an \`Ok\` value`）。

**判据：作业唯一的天花板是 `timeout-minutes` 时，测试里每一次等外部事件都必须自带期限和一句带名字的消息，新
写的等待要复用仓库里已有的那个带期限 helper 而不是再裸写 `recv().await`；而「我加了期限」这句话要由两格矩阵
兑现 —— 只删期限必须重新挂死，动真正的被测逻辑必须仍然变红。计时只能罩住测试二进制本身：把 `timeout` 套在
`cargo test` 上，一次 180 秒的编译会被判成挂死，第一轮的 4 格「存活」全是这么来的。**

## 2026-10-05：夹具在两个机制上同时成立，就等于在任何一个机制上都没测到；而暂停的时钟会让「有界」在生产计时器前面抢跑

给 `check-unbounded-recv.py` 写矩阵时，`G3`（把死 cfg 的减法整个删掉）第一轮报 `MISSED`。查下去不是变异没生效，
是那条夹具本来就是空转的：它把 `#[cfg(any())] mod parked_tests` 写在文件**顶层**，于是它同时被两条独立规则救活
——「不在任何测试区间内」和「被死 cfg 减掉」。删掉后者，前者还站着，断言照旧绿。把死模块改成嵌在
`#[cfg(test)] mod tests` 里面，同一格立刻该红；可它在**未变异的闸门**上就先红了 —— 于是顺出闸门一个真 bug：
死 cfg 是按「整段测试区间被死区间包含」减的，嵌在测试区间内部的死模块减不掉。方向是只多报不漏报，所以线上没有
假阴性，但谁哪天在 `mod tests` 里写一个 `#[cfg(any())]` 的备用模块，就会被 CI 判成「测试代码里的裸等待」，而那段
代码没有任何构建会编译。改成按站点自己的偏移量做减法，并给夹具补一条同文件的活测试作为正对照 —— 否则「一条都没
报」永远可能是「一条都没读到」。

**「两个机制都能让它绿」这件事，靠读夹具看不出来，只有把每个机制单独删一次才看得见**；而且矩阵要先在**未变异**
的树上跑一遍夹具，否则第二条（夹具自己红在错的地方）会被当成变异体的功劳。

第二类是暂停的时钟。`#[tokio::test(start_paused = true)]` 下运行时不是慢，是快：所有任务都 pending 时 tokio 把
时钟拨到**最短**的那个已上弦计时器。一个 2 秒的 `timeout` 也是已上弦计时器，于是它和生产代码里 30 秒的
`PARENT_ACK_TIMEOUT` 竞争，并且**赢**——时钟跳到 2s，期限先到，测试报「no event within 2s」，而生产路径当时只是
在按设计等它的 ack。这条测试在改造前是绿的，在改造后必红（不是偶发），指纹是 `finished in 0.00s`：真实时间一秒没
花。33 个改造文件里 12 处落在暂停时钟内，11 处等的事件在生产计时器之前就发好了所以无感，只有这 1 处撞上了机制。
预算改成用生产常量表达并高于它（`within_option_budget(10 * PARENT_ACK_TIMEOUT, ..)`），机制本身由一对测试钉住：
一格断言默认界**必须**抢跑，一格断言 10 倍界必须看得见 —— 这样将来有人把默认值调大调小，红的是有语义的那一条。

同一批里还有一次是矩阵自己的问题：中途为了省重编译把测试二进制缓存了下来，于是 5 个 helper 格跑的全是**没被
改过**的产物，报出 5 个根本没被测过的 `MISSED`；改成每格重建之后它们全部 `CAUGHT`，最终 14 格 0 存活。

同一类「一处真值写了两遍、只有一遍被检查」的事在文档里也有一次。`docs/architecture/todo-open-item-classification.md`
的分组计数同时写在开头那段散文和下面的表里，而 `classify-open-todos.py --check-doc` 只读表。这批给 TODO 加了
一行，散文与表立刻分岔 —— 但分岔不是今天才有的：散文当时写的是 maintenance `3/17`，表里是 `2/18`，当天真值是
`2/18`，也就是说散文早就错了一格，而所有检查都是绿的。处置是把散文里那串数字删掉，只留一句「数字在下面那张
表里」，让真值只剩一处。

**判据：一条夹具只能由它声称的那一个机制判定，凡是「同时也会绿」的夹具要改到那个机制是唯一支撑点，且改完必须
先在未变异的树上跑一次；变异矩阵不得跨格复用编译产物，缓存的 binary 里那个改动并不存在，报出来的是没测过的
「存活」；同一份数字若有第二处副本，那处副本要么被同一个检查读到，要么就地删掉 —— 没人核对的副本不是文档，是
下一个错误结论的来源；而在 `start_paused` 的测试里，有界等待的预算必须以它所等的那个生产常量表达并高于它 ——
那种测试里「用保守的默认值」不是保守，是抢跑；判定某次失败属于机制而非负载，看 `finished in 0.00s`。**

## 2026-10-05：装在错误模式里的测试是零覆盖而不是覆盖不足，而唯一会说出这句话的，是把被测逻辑真的改回去的那一格

这批把 19 处 `.lock().ok()` 一类的静默读者转成 `*_or_recover()`，并给它们加了硬零闸门
`scripts/ci/check-lock-poison.py`（无基线、无测试豁免）。代码与闸门都不是这批值得记下来的部分，值得记的是变异
矩阵第一轮报的那一格 `MISSED`。

`xai-grok-telemetry` 有两条门：`is_enabled()` 只在 `TelemetryMode::Enabled` 为真，`is_session_metrics_enabled()`
在 `SessionMetrics | Enabled` 都为真。我给转换写的那条测试装的是 `SessionMetrics`。于是在这个模式下，**转换前后
`is_enabled()` 都答 false**，`assert!(!is_enabled())` 之类的写法（以及它对应的正向断言）无从分辨「读者会把中毒读
成关」和「这个模式本来就关」。结果是全绿：测试过、闸门过、全量扫描过、`cargo test -p
xai-grok-telemetry` 11 条全过。只有把 `is_enabled` 拼回 HEAD 原文的那一格说了一句真话：这个测试测不到这件事。

它不是「覆盖得还不够多」，它就是**零覆盖**，而且是最坏的那种零覆盖 —— 它挂着一个正确的名字（`mode_readers_survives
_a_poisoned_client_lock`），于是所有读名字的人（包括写它的我）都以为那一条门有人看着。**测试的 setup 决定了它能
看见哪一片行为**：一条断言的价值上限，就是它自己造出来的那个前置状态与预期值之间的差异。补的那条
`product_event_gate_survives_a_poisoned_client_lock` 装的是 `Enabled`，并且多断言一句
`tokio::runtime::Handle::try_current().is_err()` —— 那是同一想法低一层：这条测试之所以安全（不会真把事件发到
Mixpanel），是因为它跑在没有运行时的线程上；把前提写成断言，将来谁给它加上 `#[tokio::test]`，红的是「前提没了」，
而不是退化成「还没走到那一步」的默默通过。`M8` 重指向新测试后 `--only telemetry` 跑 2/2。所以准确的说法是
**加了一条测试之后** 13/13，而不是第一次就 13/13。

同一批还第二次踩到同一件事：开工的站点清单来自单行 `grep`，写的是 6 处；HEAD 上单行正则是 11；把闸门自己的整文
件扫描指到 HEAD 的临时 worktree 是 19。差的 8 处全是 rustfmt 把 `.lock().ok()` 拆到不同行。上一批
`.recv().await.expect(` 从 69 重算成 232 是同一个形状。**清单必须由闸门那个扫描器产出**，否则「修完了」这句话里的
分母是假的，而闸门上线第一天就会因为「只剩 13 处历史遗留」被放宽成棘轮。

另一条边界是关于闸门能承诺什么：HEAD 的 `doom_loop::take()` 写的是 `match self.inner.lock() { Ok(..) => ..,
Err(_) => Vec::new() }`，与 `.lock().ok()` 是同一句谎话换了衣服，任何正则都看不见它。所以一个基于拼写的闸门只在
**它声明的那一种拼法**上有资格说「零」，换拼法的等价物由行为测试兜（`M3` 因它而在）。把闸门的承诺写成「这类问题消
绝了」，就是把正则的可见范围冒充成语义的覆盖范围。

最后一件是这批证据链本身的问题：远端 CI 在约 30 分钟里对同一个 run 给了十份互相矛盾的读数 —— 一个 404 的幽灵
run、`status=completed` 配 `conclusion=in_progress`、一份 HTTP 200 且内含 `all gates passed`、13 秒后重读变成
`BlobNotFound` 的日志块。这不是「CI 慢」，是**读数端不可信**，而一个说谎的端点最擅长生产自信的错误结论。处置是丢掉
所有判定、拆掉 Actions 监视器、把记录证据换成本机可重跑的宿主全量扫描与点名单元运行，并在日志里明写「本批 CI 结论
从这台机器不可知」。测试债文档里凡是只能靠远端读数支撑的结论，都要按这个标准降级。

**判据：一条测试如果在其 setup 造出的状态里、对被改坏的被测代码仍然给出同一个答案，它就是零覆盖，判据不是「名字
对不对」「跑过了没」，而是把被测逻辑真的退回改前它必须变红；变异矩阵的格子要覆盖到每一条语义分支而不是每一个
函数，因为一个函数里可以有两道门、一条测试只看得到其中一道；测试若依赖某个让它安全的前提（无运行时、无客户端、
缓存为空），就把前提写成断言，否则将来的重构会把它悄悄退化成「还没走到那一步」；开工清单必须由闸门自己的扫描器
产出，单行 `grep` 连续两批各少算一半以上；拼写型闸门只能就它声明的那种拼法宣称清零，等价的其他拼法由行为测试负责，
两者不许互相冒充；而远端读数自相矛盾时，记录证据必须是本机能重跑的那一份，宁可写「结论不可知」。**

## 2026-10-05：跑在安全 origin 上的前端测试证明不了任何事，而第一遍变异矩阵被自家一个缺失的 import 污染了

用户要求把 web 端起在 `0.0.0.0` 上自己点，于是当场暴露：明文 HTTP + 可路由地址的页面**一条消息也发不出去**，
而同一份构建在 `127.0.0.1` 上分毫不差。原因是 `crypto.randomUUID` 只在 secure context 里存在，`apps/chaos-ui`
把全部 35 处 `client_msg_id` 都押在它身上，且 id 写在发送调用的实参表里 —— 抛错发生在 `send()` 之前，消息从未
上线，界面也没有痕迹。既有 Playwright 腿全部跑在 `127.0.0.1` 与 `localhost` 上，那两个环境**天生不可能触发**
这个缺陷：绿色证明的是测试选了什么环境，不是代码对不对。

- 修法是把环境条件搬进被测页面：`addInitScript` 在页面任何脚本之前把 `Crypto.prototype.randomUUID` 遮蔽成
  `undefined`（自有属性遮蔽原型访问器），于是本地测试与 CI 都能确定性地造出「不安全上下文」的形状。关键一条是
  **先断言遮蔽真的生效**（`typeof globalThis.crypto.randomUUID === 'undefined'`），否则 fixture 哪天失效，这条
  测试就无声退化成 happy path 的复述 —— 与上一批「装在 `SessionMetrics` 模式里的遥测测试」是同一类零覆盖。
- 顺带抓到同族的第二条静默失败：`send()` 写成「socket 不是 OPEN 就什么都不做」，`submit()` 又在发送**之前**把
  prompt 乐观写进转录 —— 页面会显示一条宿主从没收到的消息。可达性判断必须排在乐观追加之前，未发出的输入不进
  转录、不进历史、不清草稿。这类「看不见就是没有」的失败，UI 断言要读**徽标出现过的序列**（共享
  `recordStatusBadge` / `statusSequence`），而不是断言此刻等于某个字符串：重连每 500 ms 一次，瞬态必然被错过。
- 第一遍变异矩阵作废重跑。`offline-send.pw.ts` 少 import 了一个 helper，一条测试因完全无关的原因红着，五格里
  三格的「抓到」因此不可信 —— 它们本来就红。`npm run typecheck` 点出了它（`TS2304: Cannot find name`），
  驱动改成先跑基线、任一 spec 非全绿就拒绝开跑。矩阵的价值取决于基线是绿的，这句话以前只写在文档里，现在是
  驱动的代码。格子的粒度也在此显形：去掉抛错兜底（W2）与去掉丢弃可见性（W4）各只染红守护该行为的那一条测试，
  而一把让全部变红的大变异只能证明浏览器开着。
- 计数第三次朝同一方向订正：起手 TODO 行写 34（31 + 3）是**行**数，按出现次数是 32 + 3 = 35（有一行两次调用）。
  前两批分别是 `.lock().ok()` 与 `.recv().await.expect(` 被 rustfmt 拆行而少算一半。规则要落到命令上：散文里的
  数字必须来自按出现次数计数的工具，或干脆由闸门自己的扫描器产出。
- 一条环境事实，因为它的症状与产品缺陷一模一样：直接 `npx playwright test -c playwright.config.ts` 而不给
  `CHAOS_E2E_ORIGIN` 与 `CHAOS_E2E_ALLOW_DYNAMIC_ORIGIN=1`，宿主会按 origin 允许名单拒绝 WebSocket 升级，
  徽标同样停在「连接断开，正在重连」，十条测试全红。`e2e-runner.mjs` 一直在给这两个变量；绕开 runner 手动跑
  就要自己给，否则会把一个 harness 记账问题诊断成分布式缺陷。

**判据：一条前端测试如果跑在一个让缺陷不可能出现的浏览器上下文里（secure context、有 `crypto.randomUUID`、
`localhost`），它就是零覆盖，跟它跑没跑过无关；把那个上下文制造出来，并且先断言制造成功，否则 fixture 失效
的当天测试就开始造假。瞬态可见性只能断言「序列里出现过」，不能断言「此刻等于」。变异驱动必须先验证基线全绿，
否则一格无关的红灯会把整张矩阵变成安慰剂；格子要一格只撤一道门，好让红灯点名到门。环境参数（origin 允许名单、
端口、令牌）造成的失败长得像产品缺陷，先复现 runner 的参数再下结论；而数量词只能来自按出现次数计数的命令。**

## 2026-10-05：扫描只贴 40 行尾巴，而 cargo 把失败名字放在尾巴上方 41 行处

44 格全量扫描第一次跑红时，汇总里关于那次失败的全部信息是 `FAIL  cargo test (exit 101)`、`running 0 tests`、
`-p chaos-engine --lib` 三行，`running 0 tests` 还是后面某个 crate 的 doctest 腿。完整输出只活在 shell 变量里，
进程一退就没了，于是「哪个测试红了」在扫描产物里根本不存在。同一条命令直接重跑又全绿 —— 也就是说，唯一见过这次
间歇失败的人正是那个把它丢掉的东西。

- 机制不是巧合而是结构：闸门命令写着 `cargo test --workspace --locked --no-fail-fast`，`--no-fail-fast` 让 cargo
  继续跑完剩余 target，并把 `error: N target failed:` 归责块压到输出的最末尾。失败测试名在中段，归责块在末尾，
  `tail -n 40` 系统性地在错误的一端，最多只能捞到其中之一 —— 实际捞到的是归责块。
- 修法是三件事一起：失败格的**完整**输出落盘并把路径打进失败块与汇总；失败名从完整输出里提而不是从尾巴里找；
  `--no-fail-fast` 的多 target 结果按 cargo 自己的 `Running`/`Doc-tests` 头归并成「哪个 target 的哪个测试红了」的
  一行摘要。落盘目录与原 TODO 行的字面要求有偏差：不是固定的 `$TARGET_DIR/gate-logs/<slug>.log`，而是每跑一个带
  时间戳的目录，因为同一棵树先后两次扫描会互相覆盖，而「拿这次扫描的日志去对稍后重跑的同一条命令」正是这次最不能
  被覆盖的场景。
- 一个形状不能靠想象：doc-test 的失败名不是标识符，cargo 写作 `src/lib.rs - add (line 3)`，第一版按标识符匹配把它
  静默丢掉。形状是从一个故意跑挂 doc-test 的临时 crate 实测来的，而不是照记忆写正则。归属同样要有反例才可信：
  夹具带三个 target，另配一格「缩进散文不是测试名」作负控，否则把标识符规则放松成整行匹配也不会有人变红。
- 自测 60 格增至 **96/96**；12 格变异矩阵全部 CAUGHT、树按 sha256 回到字节相同。其中三格专打归属本身（名字报对
  但归到错误的 target 与一个都不报同样没用）。矩阵也如实记下一格：`--log-dir` 空值那半条守卫当时背后没有测试，
  变异合法地存活，补格之后才被抓住 —— 存活的是驱动记账，不是结论。
- 最后拿真 cargo 失败的原始字节（302 行，第 5 次尝试才抓到）灌进入厂 runner 重放：失败名在第 222 行，旧尾巴会从
  第 263 行才开始。四个间歇失败（`chaos-engine` 的 ETXTBSY 与端口未释放、`xai-tty-utils` 的线程计数与 `PR_SET_PDEATHSIG`）
  因此从看不见变成有名有姓；同一测试独跑 12/12 全绿，并发下约每 5 到 20 次红一次，所以「修它」之前先要能看见它。

**判据：一个扫描器如果只保留失败输出的末尾若干行，它保留的是随机的一段，而不是失败的定义；凡是被测命令自己会
把关键信息放在别处的（`--no-fail-fast` 把归责推到末尾、libtest 把名字放在中段），落盘必须是全文、点名必须从全文
里提，且要点名到「哪个 target 的哪个测试」。解析对方工具的输出形状时，形状必须来自一次真实运行而不是回忆，且必须
有负控说明为什么不能放宽成整行匹配。落盘路径要能容纳同一棵树上的多次扫描，否则第二次扫描就把第一次的证据覆盖了。
间歇失败在能被点名之前不要动手修，因为看不清名字的修法只能是猜。**

## 2026-10-05：「双视口全绿」是上一次运行的结论，被当成提交前那一次的结论抄了下去

手机视口里 `offline-send.pw.ts` 第二条每次必红：`composer-submit` 点不下去，Playwright 说
`aside.rightbar-col` 里的一个 `.panel-section` 拦截指针事件。而当天写进证据日志的是一份 16 例全绿的转写，桌面与手机
各 8 例，行号与提交的那份一模一样。日志没错，错的是把它的结论往后搬。

- 断在哪一步是可查的：那条测试后来多了一步「点 📁 文件 标签」，为的是让页面自己发起一次非输入框的请求（打开文件树
  要向宿主列举目录），同一个死 socket 上第二条该被看见的丢弃。这一步加在第二条测试**内部**，测试起始行号不动，所以
  「行号对得上、跑过」看起来仍然像是重跑过。而那一步之后的每一次 Playwright 都出自一个把命令行钉在
  `--project=desktop-chromium` 的一次性变异驱动 —— 手机腿从改动到提交之间跑了 0 次。
- 先证伪自己最容易爱上的解释：「大概是同一个宿主被前面的 spec 污染了会话状态（`isRightbarOpen` 里那个
  `session.activeFile`）」。把提交前的那份 spec 放回工作副本跑了四组（单跑、按全量里的真实前缀跑、只加 `diff-undo`、
  只加 `approved-workspace-ops`，四副不同端口），四组都红在同一个地方，单跑也红 —— 顺序无关，是确定性失败。驱动在
  还原时按 sha256 校验回固定版本（`restore sha … ok=True`）。
- 归因用几何而不是用直觉：`elementFromPoint` 打在发送按钮中心，390×844 新页面时面板 `display: none`、命中的就是
  `composer-submit`；点过 📁 文件 之后面板变成 `position: fixed`、`x 0, y 52`、390×844、`z-index 100`，同一个点命中的
  是面板里的 `BUTTON.file-item-btn`；1440×1000 下面板是 `static` 的旁边一列（x=1020、宽 420），同一个点始终命中发送
  按钮。遮挡只在手机宽度发生，而且只在**打开过那个标签之后**发生。
- 判读要落在产品上：手机上文件/终端/Git 面板就是覆盖整屏的抽屉，回到输入框的用法是头部「对话」标签或「关闭侧栏」，
  界面从未提供「隔着抽屉点输入框」；只有左侧栏那个抽屉会置 `inert`（`main.tsx` 的 `drawerCoversShell`），这个不对称是
  有意的。所以红的是 spec —— 改 spec 之前先回答「真人手机上是这么用的吗」，否则很容易把界面改出一个真缺陷来。
- 改法：helper 先回「对话」标签，再断言 `[data-shell-column="details"]` 已隐藏，然后才 fill 与 submit。中间那句隐藏
  断言不是装饰，没有它的话，将来抽屉改成不关，这条测试会变回一条 30 秒超时、报文里只提面板不提意图的失败。量到的
  数字写进注释，下一个人在同样位置怀疑时不必再量一遍。
- 再验证：该 spec 单独跑两视口 4 例全绿；默认配置 85 通过 / 6 跳过 / 退出码 0；`playwright.git.config.ts` 14 例全绿
  （这批把 `recordStatusBadge` / `statusSequence` 搬进了 `e2e/support/shell.ts`，六条 spec 引用它，其中两条只在 git
  配置里跑）。

**判据：一条关于某个视口的结论，只有覆盖该视口的运行才能支持它。spec 在某次双视口运行之后被改过，就等于第二个视口
从没跑过 —— 测试起始行号不变不构成「同一次运行」的证据，改动之后要按 `npm run test:e2e` 重跑，而不是按驱动钉住的那
个 project 重跑。遮挡类失败必须用 `elementFromPoint` 这类真实命中量出来归因，先分清是界面错了还是测试假设了不存在的
用法，再决定改哪一侧；顺序依赖之类的解释要先用「单跑一次」证伪，不要留着它当结论。**

## 2026-10-06：账本里「设置改了当前 engine 即时生效」这条从来没有接口能支持，它是从「存下来了」推出来的

比对 web 端与 ZCode 时要写一条「模型/端点不可切换」的差距行，需要先确认 Chaos 现状。子代理的取证包照抄了
`TODO.md` 里 M3.1 那条已有行的说法 —— 「Base URL/model 更新在当前 engine 内即时生效」。这句话若抄进新的取证文档，
差距行就会被判定为「已实现一半」，而差距本身就消失了。动手写之前去看了被引用的那个接口：
`crates/codegen/chaos-engine/src/lib.rs` 的 `PromptAdapter::run_prompt(&self, prompt: &str)`（trait 声明在
`lib.rs:36`，方法在 `lib.rs:37`）只有 prompt 一个入参，两个调用点也只传 prompt —— 正常轮次在
`produce_run` 里的 `lib.rs:2854`（流式形态 `run_prompt_stream`，同文件 `:51`），提交信息生成在 `lib.rs:3417`；
真正决定发给谁的
`HttpPromptAdapter` 把 `chat_endpoint`、`models_endpoint`、`model` 存成构造期字段（`crates/codegen/chaos-engine/src/provider.rs:48`），
由 `provider.rs:153` 的 `from_env` 在进程启动时填好，装配点在 `crates/codegen/xai-grok-web/src/main.rs:23`。
`GuiSettings.model` / `base_url` 的读写只出现在 `lib.rs:3665`（GetSettings）、`lib.rs:3688`（UpdateSettings）与
`ValidateProvider` 那一条腿上。也就是说「改完能存住、GetSettings 读得回来、进程重启后新值在用」是真的，
「这一轮对话就换了模型」在源码里没有任何承载它的地方。

- 这条断言的成因不是笔误而是推理：行内另外一半（持久化）有测试与实现支撑，于是「生效」被当作同一件事的下一半
  顺手写了进去。凡是形如「X 改了会生效」的账本行，生效那半必须有把值带到执行处的接口，否则只能写「X 改了会存下来」。
- 账本不是信源。同一批次里已有的行会被子代理当既有事实引用，再被引用进取证文档与差距判定，一处夸大就会沿链条扩散成
  一处「不用做」。重新引用即重新核对，核对到接口签名为止。
- 处理方式是不删行、只把断言改回它撑得住的范围，并就地写下反证行号；该行保持 `[~]`（持久化那一半确实完成），
  撤回同时记进 `docs/architecture/todo-open-item-classification.md` 的当日段落，让下一个读到老结论的人看得见它被推翻过。

**判据：账本行里「改了会生效」这类关于运行时行为的半句，只能由把值送到执行处的那个接口来支持，接口签名里没有的
参数就不算生效；引用一条已有的账本行等于重新对它负责，要当场对签名复核，不得当作信源转抄。**

## 2026-10-06：实验台在宿主上编译、在另一个发行版里运行，一条关于容器的判决量的是宿主

`scripts/web-deployment-in-docker.sh` 与 `scripts/remote-acceptance-in-docker.sh` 是六个 `-in-docker.sh`
里唯一两个「先编译本仓库、再问部署后的程序问题」的实验台，也是 `.github/workflows/docker-labs.yml`
每天 03:17 唯一真正跑的两个。它们的构建一直在**启动它的宿主**上完成，再把二进制拷进容器。计划运行
`37297270032`（提交 `e90e1582`）因此红了整整五天里的一次，而它留下的全部线索是一句
`the backend never answered on loopback inside its container; giving up.`。

- 根因是 libc 版本方向：runner 镜像是 Ubuntu 24.04（glibc 2.39），实验台镜像
  `chaos-verify:local` 是 Debian bookworm（glibc 2.36）。2.39 上编出来的二进制要 `GLIBC_2.38`，
  在 2.36 的容器里连加载都过不去；而写这条实验台的那台机器是 Ubuntu 22.04（glibc 2.35），产物实测
  `objdump -T` 的 libc 上限只有 `GLIBC_2.34`，于是同一条命令在本地永远绿。把 `/bin/ls` 从
  `ubuntu:24.04` 拷进 `chaos-verify:local` 执行即复现，与 Rust 无关。
- 一句「量容器的话」量的其实是宿主，且两种失败在输出里长得一模一样（都是后端不应答），这是它能在本地
  连续数周通过而没人察觉的全部原因。修法是让产物运行的那个镜像自己编译它：`cargo build` 变成一次
  `${IMAGE}` 的 `docker run`，暖名卷复用 `scripts/verify-in-docker.sh` 已有的三个，本机暖编译 0.5 s；
  宿主 libc 自此与结论无关。runner 上不再需要工具链，dotslash、protoc、rustup 与 cargo 缓存四步一并下线。
- 光有「在镜像里编」还不够：部署点若仍指向别处，实验台照样能对着一份没人这次构建的旧二进制打满 41 或
  130 个 `ok`。因此每个部署点都改为从 `${lab_root}/artifacts/` 取字节并逐一比对 sha256，
  不一致即退（`chaos-web: the deployed artifact is X… but chaos-verify:local built Y…`）。
  变异 W1 与 R1 就是被这一条当场拦下；W2（部署 24.04 的 `/bin/ls` 且同时让比对失效）复现出 CI 原文。
- 同一分支的第二个缺陷是可诊断性：后端是 `docker exec -d` 起在 `sleep infinity` 容器里的，输出重定向到
  `/work/backend.log`，而失败分支打的是 `docker logs --tail 20` —— 那是 `sleep` 的输出，永远为空。
  CI 那句读不懂的失败不是措辞问题，是脚本读错了文件。

完整过程与三段变异见 `docs/verification/docker-lab-glibc-2026-10-06.log`。

**判据：实验台必须在产物将要运行的那个镜像里编译产物 —— 宿主与镜像不同源时，宿主上的绿不构成镜像里的绿，
而「产物载不起来」与「产物行为不对」在只看应答与否的探测里无法区分。同理，`docker exec -d` 起的进程其
输出不属于 `docker logs`，失败分支要读脚本自己写下的那个日志文件：读错文件的失败信息比没有失败信息更贵，
它把可查的现场换成了与现场无关的一句话。**

## 2026-10-06：仓库外的取证路径不在任何门禁的视野里，写错一个路径没有东西会红

同日给「web 端与 ZCode 的差距」补的第二遍取证。第一版提交后逐条复量，改掉了四处：CLI 库的表数（21 → 19）、
`checkpoint` 的命中数（22 → 86，22 是 `worktree` 的数）、把 `automations`/`automation_runs` 说成「表」
（它们只是 `zcode-server.cjs` 里的 `CREATE TABLE IF NOT EXISTS` 字符串，本机两个库里都没有这张表）、
以及对 Electron 的结论过弱（那 14 次命中里 13 次是 ZCode 自己的更新通道、设置项与请求头）。

- 这些断言全部关于**仓库之外**的文件：`/home/chaos/.zcode/…` 的 SQLite 库与打包产物。
  `scripts/ci/check-doc-path-refs.py` 的规则 4 明确只认「首段是本仓库有跟踪内容的顶层条目」的引用，
  仓库外路径按设计不进它的视野；`check-evidence-paths.py` 只管会话私有目录；`check-evidence-commands.py`
  只看以 `$ ` 开头的行与标了 shell 的围栏。也就是说，一份纯外部取证文档里的路径可以完全凭空写，
  44 条闸门一条都不会红。
- 缺口不在门禁那侧，而在写的那侧：本批第一版把一份未复述的二手摘录直接抄进了文件。凡是关于外部系统的
  事实（库、日志、安装目录、进程），落笔前必须当场 `find`/`count(*)`/`ls -l` 一遍，并把命令与其输出
  一起留在同一份文档里 —— 第 5 节那几条复现命令就是这次实跑的那些。
- 「表在 DDL 里」与「这台机器真用过它」是两件事。第二遍补了一张按 `count(*)` 分的证据等级表
  （A 有行 / B 零行 / C 只在打包产物里），差距的排序改按它走：`session_input` 16 行、`input_history` 15 行、
  三张 usage 表 67/56/16 行是 A 级；`permission`、`todo`、`session_target`、`workflow_*` 本机零行是 B 级；
  `automations` 是 C 级。

**判据：仓库的路径门禁覆盖不到仓库之外，所以关于外部系统的每一句断言的验收只能是人手实测一次，且复现命令
与输出要写在它旁边；未经复述的摘录不得入册。引用一条已有的账本行或既有文档等于重新对它负责，
要当场对签名或对文件复核。**

## 2026-10-06：一条测试写死的 12 帧，是在另一个常量还是 8 的时候定的 —— 变异注入到与它无关的方向才把它照出来

把「一轮回答跑到自己的线程上」这件事做完之后，先跑的是变异而不是新测试：如果反证不红，新加的三条断言
（chunk 在 `Completed` 之前到达、起了一轮的连接照样答别的问题、cancel 打断的是活着的一轮）就只是装饰。

- 变异 A：宿主里把工作线程的循环体包回 `std::thread::spawn`，即恢复「一帧一条线程」，只改顺序，其余不碰。
  `slices_sent_back_to_back_are_written_in_the_order_they_arrived` 变红，红得比预期更早 —— 它没等到逐字节比对，
  在收事件那一步就报 `unexpected AttachmentProgress { received: 98304 }`（应是 4096 的第一片）。这说明
  「分片按到达顺序落盘」这条不变量此前只活在客户端注释里，任何一次并发派发都会静默改变文件内容。
- 变异 A 的第一版是错的，而且错得会被误当成「测试没本事」：它把 `frames_rx.recv()` 一起挪回了 async 任务体，
  于是阻塞的是 tokio 工作线程，帧永远进不到 `frames_tx`，测试是**挂住**而不是变红。挂住的变异不算反证，
  丢弃重做 —— 变异必须只动被质疑的那一件事。
- 变异 B：engine 里在 `ack` 之后插一条「有 sink 就就地产出」，即把这一轮放回提问那一帧。宿主 6 条红 3 条，
  三条名字正是这条改动要保的三件事。**但其中一条红得不对劲**：`a_connection_holding_too_many_requests_is_told_so_by_name`
  的原文是 `one socket queued 12 further requests on top of a running command with no refusal`，而它是在变异里红的
  —— 还原生产代码之后单跑，它**还是红**。
- 还原后仍红，说明缺陷在测试侧。它写死 `const FURTHER: usize = 12;`，是在 `MAX_IN_FLIGHT_PER_CONNECTION`
  还是 8 的年代定的数；本轮为了让 10 MiB 附件不被自家连接的上限掐掉，把上限抬到 16，于是 12 帧已经落进
  限值之内，这条测试从此变成一条「什么都不肯拒绝」的空断言，而它一直是绿的。修法不是把 12 换成 20，
  而是让它从被量的那个常量取数：`const FURTHER: usize = MAX_IN_FLIGHT_PER_CONNECTION + 1;`（该常量因此
  从私有升为 `pub`）。同一个常量在 `apps/chaos-ui/src/attachments.test.ts` 里也被读了一次，用来断言
  客户端窗口必须严格小于主机的在飞上限 —— 那两个数字一个是「浏览器敢发几片」，一个是「主机肯压几帧」，
  它们之间的关系是产品合同，不该靠两边各自记得。
- 顺带记一条同类：`an_adapter_that_panics_ends_the_turn_and_frees_the_session` 断言第二轮的 `TextDelta` 条数
  为 1，而它自定的夹具（`PanicsOnTheFirstAnswer`）第二次进来先 `emit("写了一半")` 再 `emit("这一次写完了")`
  ——本来就是两条。这条断言量的性质是「会话被释放、第二轮能正常答」，条数属于夹具的 chunking，
  于是改断言为「第二轮必须把它那一句独有的文字送到」，而不是把期望数改成 2 继续量一个无关的东西。

- 同一轮改动把 `PromptAdapter` 的两个调用点与 settings 两条 handler 的行号都挪走了，本文件上面
  「设置改了当前 engine 即时生效」那一节引用的位置因此重写为当前行号（`lib.rs:36/37/51/2854/3417`、
  `provider.rs:48`、`main.rs:23`、`lib.rs:3665/3688`）。行号引用不会有任何门禁替它响：
  `check-doc-path-refs.py` 只查路径在不在仓库里，`lib.rs:2616` 只要 `crates/codegen/chaos-engine/src/lib.rs`
  还在就算通过，指向的却是一句文档注释。

**判据：断言里凡是「刚好够过」的数量，必须写成从被量的那个常量推导出来的式子；一个手调出来的数字在常量
动过一次之后就会变成一条永远绿的空断言，而它变绿的过程没有任何东西会响。变异要注入到与目标相邻但
不同方向的改动上 —— 本轮那条写死帧数的测试，只有在「改的是线程模型、不是拒绝逻辑」的变异里才被顺带照出，
因为它红的根本不是自己声称在量的那件事。改了被文档引用的那些位置，就得当场把引用重抄一遍，因为
没有门禁会替你发现行号已经指向别处。**

## 2026-10-06：夹具「等文件出现」的那句 `exists()`，等的是一个已经存在但还没写完的文件

Windows 那条 LSP 用例 `advertises_and_accepts_file_watch_registration_without_error`
（`crates/codegen/xai-grok-tools/src/implementations/lsp/tests.rs:2528`）间歇性红在
`parse register_reply.json: EOF while parsing a value at line 1 column 0`。测试侧的等待是
`wait_until("registerCapability reply", || reply_path.exists())`，随后 `read_json` 直接
`serde_json::from_str`；夹具侧是 mock LSP 服务里的 `dump()`：

```python
with open(path, "w") as f:      # 这一句就把文件建出来了，而且是空的
    json.dump(obj, f)
```

`open(..., "w")` 在**打开**的那一刻就把文件创建（或截断）成 0 字节，写完才收手。于是
「文件存在」与「文件完整」之间有一段时间窗，`exists()` 恰好落在窗里就读到空串。Linux 上
`json.dump` 到这个小对象基本一次 `write` 就出去了，窗口窄到量不出来；Windows 的文件系统缓冲把它
撑宽到足以被撞见。

修法不是把 `wait_until` 的超时加长或加一次重试 —— 那只是把窗口挪窄一点。改成先写
`<name>.json.part` 再 `os.replace` 到真名（POSIX 与 Windows 上 rename 都是原子的），让「存在」与
「完整」变成同一瞬间。这条用例现在等的是它能真正等到的东西。

值得记进测试债的是这个**形状**，不是这一次：`exists()` 表达的是「这个名字出现了」，而读方要的几乎总是
「这份内容齐了」。同一个测试文件里其余的 `json.dump` 站点没有这个问题，因为它们写的是 LSP 的 stdout
分帧（`Content-Length` 前缀 + 定长体），读方按长度取字节，看不见半个对象；只有「写一个文件、另一侧轮询
这个文件」这种跨进程夹具才有这个缝。判断标准就一句：**只要写方是分两步落地（先创建、再填内容），读方的
就绪判据就必须落在内容上，或者写方必须改成一步落地（临时名 + 原子改名）。**

这个缝在 Linux 开发机上量不出来，唯一撞到它的是 CI。GitHub Actions run `37346951334`（head
`ac19c73a`）的 `platform tests (windows-latest)`（job `111909810254`）逐字记录是：

```text
test implementations::lsp::tests::advertises_and_accepts_file_watch_registration_without_error ... FAILED
thread '...' (7976) panicked at crates\codegen\xai-grok-tools\src\implementations\lsp\tests.rs:2519:52:
parse C:\Users\RUNNER~1\AppData\Local\Temp\.tmpHSYGMZ\register_reply.json: EOF while parsing a value at line 1 column 0
test result: FAILED. 3118 passed; 1 failed; 2 ignored; 0 measured; 0 filtered out; finished in 74.94s
```

同一轮里 `workflows present` 也是红的（另一个问题，见 `e49469e9`）。本地把那一条用例单独重跑多轮全绿，
所以这条修法的证据不是「本地复现 → 修好」，而是 CI 上那份 0 字节的 `register_reply.json`：窗口宽度
取决于对端文件系统，本机测不出来的竞态仍然可以是真实的缺陷。

## Risk

With the full workspace now tested in CI, logic regressions in the TUI
(`pager`), the updater (`xai-grok-update`), and the PTY harness are caught
automatically. The remaining risk is in the `#[ignore]`'d tests: they compile
but never execute, so a production code change that breaks them won't be
flagged. The updater's obsolete installer URL expectations were replaced with
fork-contract tests; its remaining ignored stress test is intentionally opt-in.

## Related

- `version.rs` in `xai-grok-update` was refactored to support
  `CHAOS_GH_API_BASE` env var, enabling the completed `wiremock`-based rewrite
  of the updater's GitHub API tests; the remaining 100k stress test stays opt-in.
