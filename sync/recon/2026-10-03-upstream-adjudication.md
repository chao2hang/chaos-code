# 上游变更分流判定 2026-10-03

配套事实清单见 [`2026-10-03-2bdd1d6a6.md`](2026-10-03-2bdd1d6a6.md)（`SOURCE_REV`
`72a61251fcffb464bcc687aeb5a998e5a98ec0c9` → 上游 `2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8`，
`ahead=0 behind=9`，窗口 3325 files / +480025 / −173343，其中 290 个文件落在中文化强保护路径）。

本文件把那个窗口**按项目**分成三类：**直接移植** / **需改造后移植** / **跳过**。
本轮仍然只做只读评估，没有改动任何被移植的源文件；判定是给用户确认范围用的，不是执行许可
（`SKILL.md` 工作流 B 要求先确认）。

判定口径：
- **直接移植** = 与本分叉的改造不同区、不需要重接信任根或品牌，冲突可用三方合流解决。
- **需改造后移植** = 能力要，但落地形态必须换（品牌/路径/信任根/接缝），移植量以小项计。
- **跳过** = 与本分叉已冻结的决定直接冲突（品牌、验签 fail-closed、遥测弱化、未批准的 auth/包管理范围），
  或依赖本分叉不存在的上游基础设施。

结果没有「直接移植」这一档落在第 1 节：**跳过 3、需改造后移植 5、无需动作 1**（共 9 条）；
第 2 节十条里只有 `xai-grok-file-lock`、`xai-grok-image` 记为直接移植候选，且都带前置对照。
读源码读出来的东西基本都是「形态要换」，这本身就是本轮最值得记的结论。

## 1. 已逐项读过源码的条目（高置信）

| # | 项目 | 证据 | 判定 | 理由 |
|---|---|---|---|---|
| 1 | `xai-grok-update/src/winget.rs`（上游新增） | `INSTALL_COMMAND = "winget install --id xAI.GrokBuild -e"`，另有 `--force` pin 与 `upgrade` 形态 | **跳过** | 双重冲突：包 id 是上游品牌（`chaos update` 不可能装 `xAI.GrokBuild`），且信任根换成 winget 源，**绕开**本分叉 `require-sig` 的 ed25519 `.sig` fail-closed 链。要收必须先决定支持 winget 并另设验签。 |
| 2 | `xai-grok-update/src/windows_payload.rs`（上游新增） | 完整性依据是 `.zip.sha256` sidecar：`parse_sha256_sidecar` + `sha256_hex_of_file` | **需改造后移植** | 摘要由发送字节的一方自己算，正是本分叉 remote provenance（`crates/codegen/chaos-engine/src/remote/{provenance,artifact_format}.rs`）与新 crate `xai-grok-signature` 要消除的形态。移植必须改接 `xai-grok-signature`，否则等于把已修好的洞再挖开。 |
| 3 | `xai-grok-update/src/cleanup_downloads.rs`（上游新增，`auto_update.rs:13` + `lib.rs:10` 接线） | 上游把清理逻辑抽成模块，并新增 `executable_is_in_use` / `any_process_executing` / `ExecutableId`，`cleanup_old_downloads_with(..., is_in_use)` 留了可注入接缝 | **需改造后移植**（本轮唯一「上游确实比我们安全」的确认项） | 本分叉**已有同一套算法**，内联在 `crates/codegen/xai-grok-update/src/auto_update.rs:2264`–`2358`：同样「绝不动当前版本」、排序后 `skip(1)`（保留 current + 1 个旧版本作回滚）、`.tmp` 与新写入 mtime 的 STALE 判定、跳过 symlink。**缺的是 in-use 守卫**：mtime 超过 `STALE_TMP_AGE`（1h）且正被活进程执行的旧版本，本分叉会直接 `remove_file`；上游在 macOS 上把「进程表读不出来」也当作 in-use（Linux 可以 unlink 已映射文件，故扫描失败按 not-in-use）。移植点=该模块 + 守卫 + 接缝，保留本分叉的三个调用前缀（`auto_update.rs:1743`–`1745` 的 `"chaos"` / `"grok"` / `"grok-pager"`）与 `~/.chaos/downloads`。**与「清理保留回滚版本」的既有结论不冲突**：上游 `test_cleanup_old_downloads_keeps_current_plus_one` 断言的正是同一条保留规则。 |
| 4 | compaction：`xai-chat-state/src/compaction_utils*.rs`、`image_context*.rs`、`xai-compaction-transcript/src/lib.rs` | 上游有改动；本分叉 DCP 复活落在 `common/xai-grok-compaction` 的 `strategies` 与 `xai-grok-shell` turn loop | **需改造后移植** | 同区。按 `port-playbook.md` B4 第 3 条手工合流，禁止整文件 theirs/ours。 |
| 5 | pager 交互层：`xai-grok-pager/src/app/acp_handler/{background,follow_ups,interactions}.rs` + 315 个 `xai-grok-pager-pty-harness` 测试文件 | 同区改造：`/fallback`、`/adhd` 接通、「累计 token」chip 改走 `Action::ShowUsage` | **需改造后移植** | 冲突面主要是中文化断言与 fork-only 命令；`l10n-guard.sh --before` 快照必须在移植前拍。 |
| 6 | `views/dashboard/render_tests.rs`、`views/dashboard/state_tests.rs`、`views/shortcuts_help_tests.rs` | 上游把这三块测试**外部化**：宿主文件末尾有 `#[cfg(test)] #[path = "…_tests.rs"] mod tests;`（`render.rs:3168`、`state.rs:4509`、`shortcuts_help.rs:1462`），宿主文件自身 `#[test]` 计数为 0。本分叉**没有这三个文件**，同名模块**内联**在宿主里（`render.rs:4029` / `state.rs:4512` / `shortcuts_help.rs:1400`，`#[test]` 分别 116 / 246 / 67 个），`panic-site-census.py --check-uncompiled` 报 `0 files` | **需改造后移植（结构重构，不是取回丢失文件）** | 第一轮把这条读成「分叉丢了那三行声明」，实测不成立：分叉侧根本没有未编译源，测试一直在跑。真实形态是上游做了「测试挪出宿主文件」的重构，且两侧内容已分叉——模块内 `fn` 名比对 `render` 上游 124 / 分叉 122 / 共有 95、`state` 291/266/261、`shortcuts_help` 87/81/75。收的话是一次带内容合并的重构；直接整文件覆盖上游宿主会撞 `E0428`（同名 `mod tests` 定义两次）。收益是宿主文件变小、后续同步冲突变少，不是修 bug，**排到低风险批次之后**。 |
| 7 | 上游删除 `views/usage_detail.rs`、`scrollback/blocks/credit_limit.rs`、`app/dispatch/tests/usage_partial_failure.rs` | `git cat-file -e upstream/main:<path>` 三者皆否 | **无需动作** | 与本分叉独立得出的死面判定一致（`usage_detail` 覆盖层已退役、chip 改走 `ShowUsage`）。 |
| 8 | 安装入口 | `git ls-tree upstream/main scripts/` 为空；上游另有一份 `crates/codegen/xai-grok-pager/scripts/install.ps1` | **跳过** | 上游没有仓库根 `scripts/`，`scripts/install.sh\|ps1\|bat` 与 `scripts/ci/test-installer-asset-names.py` 门禁只约束本分叉；那份 pager 内的 `install.ps1` **不是同一个入口**，不能拿它验我们的发布产物。 |
| 9 | `/adhd`、`/fallback` slash 命令 | 上游不存在 | **跳过**（无移植问题） | fork-only；移植时不存在「上游也有」的退路，回归必须由我们自己的测试承担。 |

## 2. 上游新增、本分叉完全没有的 crate（分流级判定）

复现：`git diff --no-renames --numstat "$SRC"..upstream/main > /tmp/win.txt`，
再用 `awk -F'\t' 'index($3,"crates/codegen/<crate>/")==1'` 取每个 crate 的行；
「本分叉是否存在」用 `find crates -maxdepth 2 -type d -name <crate>` 取空集判定。
十个 crate 全部是**纯新增**（`-0`），合计 **208 files / +74097**。
**测量口径的坑（本轮实测）**：带默认重命名检测的 `git diff --numstat` 会把成对改名折叠成
`dir/{old => new}/x.rs` 形式，按路径前缀归组时这些行不会被算进目标 crate，于是同一个 crate
在两次测量里给出 23 与 28 两种文件数（`xai-grok-cloud-config`）。本表所有规模数字都取
`--no-renames`，与 `2026-10-03-2bdd1d6a6.md` 里那条 3325 files 的窗口总量（带重命名检测）
口径不同，不要互相换算。
**这一节的判定基于文件清单与规模，未逐个通读实现**，用于排序与定策，不用于放行。

| crate | 规模 | 内容（按文件名） | 判定 | 理由 |
|---|---|---|---|---|
| `xai-grok-permission-rules` | 41 files / +23518 | `bash_command_splitting`、`bash_permission_script`、`claude_settings`、`env_risk`、`exec_risk`、`folder_trust`、`gate_preflight`、`git_content_filter` | **跳过本轮，单独立项** | 窗口里最大的新能力，且整块落在安全边界上；`folder_trust` 与本分叉尚未批准的扩展信任策略同题（属于「需负责人签字」那一类阻断项）。先定策再谈移植，不能搭合并窗口的顺风车进来。 |
| `xai-grok-login` | 50 files / +28240 | `auth_provider`、`device_code`、`external_auth`、`api_key_probe`、`attribution`、`grok.rs` | **跳过** | 认证范围与 provider 选择是本分叉未批准的产品决定；`grok.rs` / attribution 还带着上游品牌与回传语义。 |
| `xai-grok-cloud-config` | 28 files / +5633 | `policy`、`cached_config`、`prefetch`、`supervisor`、`response`、`metrics` | **跳过** | 云端下发配置 = 新的电话回家面；`chaos-fork-map.md` 规则 4 要求上游强制遥测/回传不得原样合入，需先有契约与安全评审。 |
| `xai-grok-otel` | 8 files / +1462 | `otlp`、`provider`、`redact_common`、`timeout`、`trace_context` | **跳过** | 同上：OTLP 出口。本分叉只实现了只读 `chaos telemetry status`，写侧 `disable`/`enable` 尚在等契约与安全评审。 |
| `xai-grok-feedback` | 12 files / +2661 | `draft_store`、`draft_images`、`feedback_archive`、`taxonomy` | **跳过** | 反馈上报涉及外发与留存策略，属产品/隐私决策，非移植决策。 |
| `xai-grok-egress-proxy` | 19 files / +4612 | `connect`、`credential`、`decider`、`tls`、`ip`、`hold_budget`、`server` | **需改造后移植（建议立项评估）** | 安全增益方向与我们已记录的缺口对得上：`docs/audit-followup-report.md` §1.7 明确「网络过滤器挡不住已被子进程继承的已连接 socket 的写入」。这一项可能正是那条缺口的上游解法，但必须接本分叉的 `child_net`（私有模块）与 restricted-spawn 导出，不能平行再长一套。 |
| `xai-grok-lifecycle` | 25 files / +3891 | `exec_spec`、`exec_log`、`registry`、`broker`、`persist`、`image` | **需改造后移植（建议评估）** | 进程/执行生命周期，与本分叉 `ProcessScope`、进程组清理、PTY 后代判定同域；移植要先决定是替换 `ProcessScope` 还是只做适配层，否则会两套并存。 |
| `xai-grok-external-agent-migration` | 13 files / +1806 | `source_cla`、`hooks_cla`、`mcp`、`rewrite`、`scope`、`reporting` | **需改造后移植** | 从第三方 agent 配置迁移（Claude 系），`rewrite`/`scope` 里有品牌与路径改写；移植必须把目标改成本分叉的 `~/.chaos` 与 `chaos` 命名，并过 `check-brand-protocol.py`。 |
| `xai-grok-file-lock` | 9 files / +1175 | `lock`、`locked_file`、`slot`、`options` | **直接移植候选** | 自包含、带测试、不碰品牌与信任根；移植前只需确认与本分叉既有并发写的接缝（`xai-grok-test-support::env` 那类守卫不在同一层）。 |
| `xai-grok-image` | 3 files / +1099 | `image_validate` | **直接移植候选（先对照）** | 本分叉已有共享附件校验器 `AttachmentStager::validate_name_type_size`（跨平台分隔符、MIME/尺寸/配额），且 Web/Engine 两条入口都有真实回归。先对照覆盖面，若上游只是重复实现则改判跳过。 |

## 3. 移植窗口的排序建议

1. **窗口打开前必须定的两件事**：`winget` 更新路径收不收（本文判定为跳过，需确认）、
   `windows_payload` 是否接 `xai-grok-signature`。这两处正是 `xai-grok-update` 冲突最密的地方，
   而该 crate 又与本分叉已交付的验签改造同区。
2. **先拍快照**：`scripts/l10n-guard.sh --before <移植前 HEAD>`，移植后再对照。290 个强保护路径
   文件若不先拍，「中文被冲回英文」会以百计出现在报告里，且无法归因。
3. **窗口内先做低风险项**：`xai-grok-file-lock`、`xai-grok-image` 的对照结论。它们能在冲突最大
   的区域之外先落地。第 6 条（宿主文件与外部化测试模块的合并）不在这一档——它是带内容合并的
   重构，放到低风险批次之后再排。
4. **窗口内手工合流区**：compaction、pager `acp_handler`、`xai-grok-lifecycle`（若决定接 `ProcessScope`）。
5. **不进窗口**：`winget`、`login`、`cloud-config`、`otel`、`feedback`、`permission-rules`。
   这六项各需要一个前置决定（包管理与验签、auth 范围、回传契约、遥测写侧、上报策略、扩展信任），
   跟代码合并解耦，混进来只会让窗口内的失败无法归因。

## 4. 本轮的边界

- 第 1 节的九条读过源码；第 2 节的十条只到文件清单与规模，判定用于排序，放行需另开一次通读。
- 本文件不构成移植许可。`SKILL.md` 工作流 B 要求用户确认范围后才动业务代码。
- **一条已更正的自测结论**：第 6 条第一轮写的是「分叉丢了那三行 `#[path]` 声明、留下三个从未
  编译的测试文件」。复核（`ls` 宿主目录 + `panic-site-census.py --check-uncompiled` + 41 条
  `#[path]` 声明的目标存在性核对）不成立，已按实测改写。留这条记录是因为这类误判很典型：
  上游删了宿主文件里的内联测试 ≠ 分叉也删了测试，两种布局可以指向同一套测试。
