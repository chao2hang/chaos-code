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
209 are resolved**: 0 non-ignored failures remain. The repaired 2026-09-25
inventory reports 434 ignored attributes total, including 218 bare attributes;
these are workspace-wide scanner counts, not the count of fork-specific entries
in the table above.

## Ignored tests

Tests marked `#[ignore]` are a separate debt. Their reasons must stay
readable and be revisited periodically; a permanent `#[ignore]` is a deleted
test with extra steps. `python3 scripts/ci/ignored-tests.py --check-baseline
scripts/ci/ignored-tests-baseline.tsv` checks the grandfathered bare-attribute
inventory in both directions. The live scan currently reports 434 ignored
attributes and 218 bare attributes; the checked-in CSV and per-source owner
audit still determine review status. CI runs the repository inventory scanner against
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

- 目标是**禁止未经审查新增**裸 `#[ignore]`。当前存量 218 条按 package/path/function 受双向 baseline gate 管理；新增、删除均需审查 baseline diff。清理存量时逐条补理由，不批量伪造原因。
- 对带 reason 的 `#[ignore]`，Reason 里**必须**有 `review YYYY-MM` 或等价的重审日期。无日期的算
  “永久债务”，需季度审计时处理。Bare attributes remain explicit legacy exceptions until that audit; the inventory baseline is not an approval.
- 新增 fork 专属 ignore → 必须同时更新本节表格计数和原因描述。

2026 Q4 source-by-source owner audit remains pending until the October review cycle.
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
