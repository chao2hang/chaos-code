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
