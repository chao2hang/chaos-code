# Changelog

## Unreleased

### 修复：Windows 那条 CI 腿把 WSL 启动器当成 bash，installer 的四条平台分支从此真被执行

`scripts/ci/test-installer-asset-names.py` 比对的是一件事：四个地方（`install.sh` 的 bash
`case`、`install.ps1`、`install.bat`、`chaos update`）各自拼出来的 release 资产名，是否都
在 `release.yml` 那六行 `copy_one` 真正上传的名字里。它对 `install.sh` 原本只做「本机跑一次」
的验证：`subprocess.run(["bash", "-c", ...])`。`platform tests (windows-latest)` 就红在这一句，
run 313 的原文是 `FAILED running the shipped detect_platform failed:`——冒号后面**什么都没有**，
即退出码非 0 而 stderr 为空。`install.sh` 拒绝 Windows 时是往 stderr 打
`error: use PowerShell scripts/install.ps1 on Windows` 的，代码里那句
`if "use PowerShell" in proc.stderr` 的按设计豁免因此不可能触发；CRLF（本机复现：rc=2 且
bash 语法错误可见）与打桩 `uname` 走 MINGW 分支（rc=1 且消息在）两条无辜解释都被逐一排除。
剩下的是解释器本身：Windows runner 的 `PATH` 把 `C:\Windows\System32` 排在
`C:\Program Files\Git\bin` 之前，而 `System32\bash.exe` 是 WSL 启动器、不是这台机器的 bash，
没装发行版时它退出非 0、抱怨写在 **stdout** 上。Linux/macOS 两条腿看不见这个形状，所以同一份
代码在三分之二的腿上是绿的。

取 bash 的地方新增 `find_bash()`：`$BASH` 优先（`shell: bash` 那一步里它就是正在执行这一步的
解释器），其次按 `ProgramFiles` / `ProgramFiles(x86)` / `LOCALAPPDATA` 找 Git for Windows，
最后才信 `PATH`；Windows 上路径含 `system32` 的候选直接丢弃；**每个候选都要真跑一条
`printf ok`**——「路径存在」和「是 bash」不是一回事。取不到时不再静默降级：POSIX 主机记
failure，Windows 主机记 note（`install.sh` 本来就不是 Windows 的安装入口）。失败信息现在
把退出码、stderr、stdout 三者都印出来，这类故障下次会自报家门。

顺带补掉一处覆盖空洞：本机那一次探针只覆盖本机走得到的分支，另外三条平台分支此前是靠正则读
`OS_KEY=` / `ARCH_KEY=` **赋值推**出来的。现在 `uname -s`/`uname -m` 打桩，`detect_platform`
的七条分支**逐条真跑**：linux/darwin × x64/arm64 四条出资产的断言 `ASSET` 与 `PLATFORM`
（自动更新存盘用的那套 `macos-aarch64` 名字）都等于预期且资产名在 `copy_one` 集合内；
Git Bash / FreeBSD / `ppc64le` 三条断言退出非 0 且报错原文含 `use PowerShell` /
`unsupported OS` / `unsupported arch`。桩由 bash 自己 `mktemp -d` 写出——Git for Windows 里
python 的临时目录是 DOS 路径，拼进 `PATH` 解析不了。`case` 里若新增一个 `OS_KEY=` 而探针表
没有对应行，判失败而不是跳过；本机探针保留。

新增 `scripts/ci/test-installer-bash-resolution.py`（13 用例）钉住取 bash 的规则：退出非 0 的、
只往 stdout 抱怨（WSL 启动器的形状）的、是个目录的候选都不能中选；`System32` 下**能用**的
bash 在 Windows 上必须落选（从 `PATH` 与 `$BASH` 两侧各进一次），在 POSIX 上又必须照常可用；
取不到 bash 时 POSIX 必须失败、Windows 只记 note，且 `check_install_sh` 其余部分照常跑。
五个变异逐个注入：删 `system32` 过滤 → 2 用例红；候选不执行即接受 → 3 用例红；探针表写错
Darwin/arm64 → 检查红；`install.sh` 把 `ARCH_KEY` 写成 `x86` → 检查 4 条红；改掉
`use PowerShell` 措辞 → 检查红。每次改回后 `cmp` 校验逐字节一致。第一版的 System32 用例
**删掉过滤也不会红**（那个排列里 Git 候选本来就排在 PATH 之前），是变异测出来才改对的。
两条腿都接上：`platform tests` 的 `Parse the PowerShell installers` 步骤与 ubuntu `rust`
任务的脚本检查串，外加 `scripts/verify-in-docker.sh` 新增 `installer asset names` 门禁。
证据见 `docs/verification/installer-asset-probe-2026-10-03.log`；Windows 腿的真实结论要由
下一次 push 的 platform job 给出，本机无法替它作保。（2026-10-03；
`scripts/ci/test-installer-asset-names.py`、`scripts/ci/test-installer-bash-resolution.py`、
`.github/workflows/ci.yml`、`scripts/verify-in-docker.sh`）


### 修复：中文化守卫把「改名/删死代码」当成「中文被冲掉」，且两份守卫自测没人跑

`scripts/l10n-guard.sh` 守的是上游合并把 fork 的中文 UI 冲回英文这类事故，按
`crates/**/*.rs` 里「哪些文件有中文」比对两个 ref。它只看**路径**，于是三种完全不同的事
都落成同一个 FAIL：文件改名（中文跟着搬走）、故意删掉死代码、以及它真正要抓的那一种
（文件还在、中文没了）。本轮自己的树就被判成 6 个 regressed + 2 个 fortress breach，
而 `scripts/verify-in-docker.sh:134` 把它当门禁跑，等于本轮交付被自己的守卫挡住。

六个文件逐个查实（先量「该文件独有的中文行有多少在别处逐行原样出现」，再对着代码核）：
`xai-grok-update/src/signature.rs` 是改名——拆出的 `xai-grok-signature/src/lib.rs` 里
30 行中文都在（20/20 行存活）；`views/dashboard/{state,render}_tests.rs` 的中文是测试输入
`"中\r\n文"`，那些行原样活到了宿主模块的内联 `mod tests`（6/6、2/2）；
`scrollback/blocks/credit_limit.rs` 在 HEAD 的 `blocks/mod.rs` 里根本没有 `mod credit_limit`，
**从没编译过**，额度上限卡片的中文在实际发射处 `app/dispatch/billing.rs` 的 `CreditLimitCopy`
（「已达到消费上限。」「已达到当前计划的额度上限。」「提高限额」「按量付费」）；
`views/usage_detail.rs` 与孤儿测试 `dispatch/tests/usage_partial_failure.rs` 随不可达的用量
覆盖层一起删除。**没有一条用户可见文案失去翻译**——这是守卫的判定粒度问题，不是翻译问题。

改法是把「中文消失」拆开判：中文行 ≥ 90 %（且 ≥ 2 行）在别处逐行原样出现 → `moved`，
由**内容**决定而不是文件名；路径确实不在且已登记在 `scripts/ci/l10n-removed-allowlist.tsv`
（`<path>\t<理由>`，理由必填、缺理由直接失败）→ `removed-recorded`；其余 → `regressed`。
**登记也救不了硬失败**：allowlist 只对「路径真的不在了」生效，只要文件还在、中文没了，
无论是否登记都仍然 exit 1。fortress 检查跳过已判为 moved/recorded 的文件，否则在强保护
目录里改名或删死代码永远过不去。登记项的失效口径改成看**工作树**而不是 `--after`：
指向工作树里仍存在之文件的条目即「记录的删除没发生」，必须删掉条目——原来的 ref 口径既会让
`--before HEAD --after HEAD` 把每条登记都判成失效（这是加 CI 步骤时本地先跑出来的），也漏掉
「文件在比对范围之外被还原」这种真正的腐烂。

守卫本身新增 `scripts/l10n-guard-selftest.py`：13 个用例各自建一个临时 git 仓库、跑真实
脚本、同时断言退出码与文件落在哪一节（只断言 exit 0 的自测，对一个什么都不查的守卫也会通过）。
其中 `clobber_cannot_be_allow_listed` 是承重的：它把被抽掉中文的那个路径本身就登记进
allowlist，仍要求 exit 1 且该路径出现在 `regressed`。三条变异各自让对应用例变红
（把「先查存在再查登记」颠倒 → 2 用例红；去掉改名判定阈值 → 阈值用例红；关掉失效检查 →
失效用例红），每次改回后 `cmp` 校验逐字节一致。实现过程中修掉两个自己的错：
`grep -c -Fxf -f` 里组合的 `-f` 把后面那个 `-f` 当成文件名吃掉，模式文件成了字面量 `-f`，
所有匹配数恒为 0（报告里写着「0 of 20 行在别处出现」）；`is_excluded` 在 `set -u` 下展开空数组
`${EXCLUDE[@]}`，macOS 自带的 bash 3.2 会当作未绑定变量，改成 `${EXCLUDE[@]+"${EXCLUDE[@]}"}`。

顺带清掉一类「有守卫却没人跑」的洞：`scripts/check-doc-l10n-selftest.py`（51 用例）此前
**没有任何地方调用**（`rg -n "selftest" .github/workflows/*.yml scripts/*.sh` 零命中），
它的断言就算早已咬不动也照样绿。现在两份自测都进了 CI 的 `docs-l10n` 作业与
`scripts/verify-in-docker.sh` 门禁，CI 里还额外跑一次 `l10n-guard.sh --before HEAD --after HEAD`
——它会遍历真实的 393 个中文文件，是能在 Linux 之外（bash 3.2）挂掉的那类结构的落脚检查。
文档同步：`sync/doc-l10n-conventions.md` 验收口径第 6 条与上游同步 skill 的报告表都补齐了
新的六份输出、登记要求，以及「登记拦不住就地换成英文」这条边界。
证据见 `docs/verification/l10n-guard-classification-2026-10-02.log`（含存活率实测表、
13/13 自测、三份变异记录、YAML/端口性/workflow shell 三项复验）。

顺带把同一类噪音从文档检查器里清掉：把 `check-doc-l10n.py --links` 扩到 CI 从没跑过的
`sync/**/*.md`，报出 8 条「死链」，逐条查下来 7 条是**行内代码里的链接语法**——约定文档把
`](...)`、`](NN-xxx.md#...)` 当作「要盯的写法」列出来，`doc-claims-verification.md` 更是
**在描述某个锚点已死**时引用它。`strip_code` 已经会让围栏代码块失效，只是止步于围栏边界，
没有覆盖到一个反引号的跨度。新增 `mask_inline()` 原位抹平行内代码（保持字符数不变，行列报告
与其它模式不受影响），仅 `check_links` 使用；`--before/--after` 那几项**故意不抹**，因为它们
正是靠比对行内 span 来发现标识符被改。自测加一条双向用例（同一目录跑两次：只有示例必须干净，
补一条正文真链必须点名失败），总数 **52/52**；删掉 `mask_inline` 调用即让该用例变红。
第 8 条是真错：约定文档里引用权威表述的那段引文带着 `[CHAOS.md](../../../../CHAOS.md)`，
而发行的指南实际用五层（`../../../../../CHAOS.md`），这个深度在哪边都解析不到，已改为
`../CHAOS.md`。`--english` 侧 8 行里有 6 行同源：`sync/recon/*.md` 是
`scripts/upstream-recon.sh` **生成**的，而模板是英文——模板改为中文并对真实 API 重跑生成，
`sync/**` 现为 `--links` 0 条、`--english` 仅剩 2 行「…」内的上游原文引用（按约定保留原文）。

**没覆盖到**：改名判定按整行比对，翻译时重新断句或改写会算成「丢失」而不是「改名」——这是
有意的保守，宁可响亮地失败也不猜；`removed-recorded` 信任理由文字本身，守卫只强制理由必须
存在，不会读中文句子判断它对不对；`l10n-guard.sh` 仍只看 `crates/**/*.rs`，Markdown 归
`check-doc-l10n.py`，这个不对称没变。本改动不碰任何界面，故浏览器验证口径在此无对象可跑。

### 新增：模型「只报计划不收尾」的自动重试（opt-in，默认关闭）

`session/acp_session_impl/incomplete_end_turn.rs` 和它的测试之前同属「没有任何构建会编译它」
那一类：文件在，检测器写得完整，但没人调用，开关也不存在。本轮把它接成一条真能开的功能。

判据是纯函数 `should_retry_incomplete_end_turn`，只在**这一轮跑过工具、却没有任何写/编辑工具
落地**时才考虑重试：写工具名按 `WRITE_TOOL_NAMES` 认（`search_replace`、`write`、`hashline_edit`、
`apply_patch`、`edit`/`Edit`/`Write`/`MultiEdit`/`NotebookEdit` 等，覆盖本仓与 codex/opencode/Claude
风格别名）。命中两种理由之一才重试——收尾文本读起来像「接下来我要做 X」的计划
（`intent_without_write`），或本轮工具数 ≥ 2 而收尾文本 ≤ 80 字符（`short_after_tools`）。
两组误报防护是实打实写出来的：命中「已完成」类措辞会压掉计划判定（`接下来我已完成所有修改`
不该重试），收尾文本为空一律交给别的恢复路径。每条 prompt 最多 2 次（`MAX_RETRIES`），因为每次
重试就是一整轮采样。

接线：`[session] auto_retry_incomplete_end_turn`（默认 `false`）→ `spawn.rs` 解析成会话字段，
**子 agent 会话强制关闭**（委派出去的子任务由父会话负责判断做没做完，不该各自重试）→
`turn.rs` 的包装循环 `process_conversation_turn_with_incomplete_end_turn_retry` 在判定命中时
注入 `ConversationItem::auto_recovery` 提醒再采样。提醒文案直接点名「不要只复述计划，需要改
就调工具」，避免重试只是把同一段计划再说一遍。

顺带修掉一个真实缺陷：`/settings` 里这一行会发 `Effect::PersistSetting`，但 `persist_setting`
**没有对应 arm**——也就是说这个开关在 UI 上能拨、当场生效，写盘却返回 "unknown setting key"，
回滚后 `config.toml` 里永远没有它，下次启动回到默认 off。现已补 arm 并配 `session` 文档行
（`26-config-reference.md`）。同时修好本轮自己写坏的守护测试
`every_persist_setting_key_has_a_persist_arm`：它扫源码找 `Effect::PersistSetting { key: "…" }`，
却用「最近的 `{` 到最近的 `}`」取块，被只提这个名字的散文（自己的文档注释、
`app/dispatch/tests/settings.rs` 的 assert 文案）带成 `begin <= end (474 <= 171)` panic。现在要求
路径与 `{` 之间只能有空白，块边界用深度计数求匹配并跳过字符串字面量；另加两条自我约束——
每个结构字面量都必须被归类为「字面键」或「运行时构造」，且扫到的分发点数必须 > 50（全仓实测
97 处），否则直接报「扫描器坏了，下面的断言证明不了任何事」。

测试：`incomplete_end_turn.rs` 内联检测器用例（两种理由、写工具名识别、已完成措辞压报、空文本、
重试上限）+ 新文件 `acp_session_tests/turn/incomplete_end_turn_loop_tests.rs`（343 行，驱动真实的
包装循环）+ `util/config/persist_tests.rs::session_auto_retry_incomplete_end_turn_round_trips`
（配置往返）+ pager 侧守护。守护非空转的注入变异（按字节 `cmp` 还原）：把 `persist_setting` 里该
arm 的键名改成 `…_TYPO` → 守护失败并点名 `("session.auto_retry_incomplete_end_turn", "setters.rs")`。
覆盖限制：真端到端「模型真的只给计划」需要活的采样端，本环境无法复现；判定与循环用真实函数驱动，
`/settings` 拨动到落盘的完整链路只由 round-trip 测试与守护共同覆盖。

### 修复：快捷键窗口漏列已绑定的命令，「累计 token」状态栏 chip 点了没反应

同一批「没有任何构建会编译它」的孤儿测试文件，逐个判定后收口为**空集**
（`scripts/ci/uncompiled-sources.txt` 现为 0 个文件，`DELIBERATELY_UNDECLARED` 例外表同时清空）。
判定不靠猜：把文件临时声明进各自宿主跑 `cargo check`，再把它自己的测试函数名集合与宿主内联
`mod tests` 求差。

`views/dashboard/render_tests.rs`（4,333 行）与 `state_tests.rs`（6,101 行）报 **0 个错误位点**
——它们本来就能编译，被删的理由是覆盖集合：`render_tests.rs` 的 120 个函数名与宿主内联测试
**完全重合**，`state_tests.rs` 的 265 个 vs 宿主 384 个，两者 **orphan-only 均为 0**。
`views/shortcuts_help_tests.rs` 有 12 个 orphan-only 名字，逐条回读源码：三条断言的行为已被
**有意反转**（活文件里是 `enter_on_search_pseudo_row_does_not_open_detail`、
`search_pseudo_row_does_not_expand`、`build_entries_omits_scrollback_search_in_simple_mode`），
undo/redo/history 三条对应的常量与 `ActionId` 变体早已不存在（`shortcuts_help.rs` 的
`*_LONG_HELP` 只剩 `PASTE_LONG_HELP`）；唯一仍成立的 `build_entries_lists_prompt_stash_with_ctrl_s_and_alt_s`
已移植回活文件——`ActionId::StashPrompt` 至今带着 Ctrl+S / Alt+S 的完整 `ActionDef` 且有真实处理方。

移植时补上这批文件一直在掩盖的那一类缺口：`build_entries` 是 registry 驱动（函数文档原文
"All registered actions are included"），但**没有任何测试遍历 registry**，所以一个绑了键的新命令
可以永远不在快捷键窗口里出现、而全套测试照常绿。新增
`every_keybound_registry_action_gets_a_cheatsheet_row`：遍历真实 `ActionRegistry::defaults()`，
只允许三种有依据的缺席（无键的 slash-only、voice 门控关闭时的 `VoiceToggle`、被同类目同键
dedup 让位且让位对象确实上榜），其余一律红。`xai-grok-pager --lib views::shortcuts_help`
**67 passed / 0 failed**。非空转注入变异（按字节 `cmp` 还原）：给 `build_entries` 加一行
`if def.id == ActionId::StashPrompt { continue; }` → 守护与被移植的用例同时红，报错原文
`keybound actions with no row in the shortcuts cheatsheet: ["StashPrompt (label "暂存")"]`。

`app/dispatch/tests/usage_partial_failure.rs` 的 11 个测试**全部 orphan-only**（`tests/` 下 20 个
兄弟模块 0 覆盖），但驱动的是已经不存在的函数：`fill_session_usage_detail` /
`fill_aggregate_usage_detail` 及其 `_failed` 变体在源码里没有任何定义。顺着这条线查出**一个
用户可见的真实缺陷**：状态栏「累计 token」chip 悬停会高亮（`hit_total_tokens` 的 rect 每帧写入、
hover 每帧更新并改变配色），可全仓唯一的点击处理分支写在 `if self.usage_detail.is_some() { … }`
里，而 `usage_detail` 这个字段在生产代码里**只有 `= None` 一处写入**、从未被置成 `Some`，
`usage_detail_generation` 也从不自增。于是 `views/usage_detail.rs`（849 行）连同它的 `[✗]`
关闭按钮 hit-rect、Esc/`q`/滚轮吞键分支，是一整套任何操作路径都进不去的死面；配套的 6 条测试
靠手工 `agent.usage_detail = Some(UsageDetail::Loading)` 摆出生产根本造不出来的状态来测处理逻辑。

修法按现有产品事实：被 `views::usage_modal`（`open_usage_info_modal`）取代的弹层不复活，
改为让 chip 兑现自己的承诺——`app/mouse.rs` 中紧邻 `hit_context` 处新增分支，点击
`hit_total_tokens` 返回 `InputOutcome::Action(Action::ShowUsage)`，正是 `/usage` 自己走的
dispatch；沿用 `CONTEXT_CLICK_DEBOUNCE_MS`，但用独立的 `last_usage_chip_click_at`，避免两个
chip 共用时间戳时先点一个会吞掉另一个 300ms 内的点击。死面一并删除：该视图模块与其声明、
`usage_detail` / `hit_usage_close` / `usage_detail_generation` 三个字段及初始化、
`close_usage_detail`、render 的弹层分支、`notices.rs` 的提示遮挡判定、`input.rs` 的吞键分支与
Esc 消费者判定、`panes.rs` 的滚轮吞掉、`minimal/api.rs` 的表面可用性判定，以及那 6 条自摆状态的
测试；`render.rs` 里指涉该模块的注释改为直接说明宽字符伪空格本身。全仓 `grep usage_detail`
命中 0。3 条新测试经**真实 `handle_mouse` 入口**驱动：chip 内点击必须返回 `Action::ShowUsage`、
连点第二次必须被 debounce 吞成 `Unchanged`、chip 右边界外一格不得触发。

顺带修掉本轮自己写坏的一条守护测试：`every_persist_setting_key_has_a_persist_arm` 扫源码找
`Effect::PersistSetting { key: "…" }`，但用「最近的一个 `{` 到最近的一个 `}`」取块，遇到只提这个名字
的散文（本套件自己的文档注释、`app/dispatch/tests/settings.rs` 里的 assert 文案）就
`begin <= end (474 <= 171)` 直接 panic。改为：`{` 之前出现 `}` 即判为散文跳过，块边界用深度计数
求匹配并跳过字符串字面量；再加两条自我约束——每个结构字面量都必须被归类（字面键 / 运行时构造），
且扫到的站点数必须 > 50，否则报「扫描器坏了，下面的断言证明不了任何事」。全仓 97 处真实分发点。
覆盖限制：chip 的可见效果（配色、弹层消失后的行为）只能靠 `handle_mouse` 的返回值与源码可达性
证明，本环境无终端可跑 TUI；`/usage` 之后的取数与渲染由既有用量模态测试覆盖，本轮未改动它。

### 新增：备用模型链终于有了读取方（`/fallback` 复活）

`/fallback` 写 `[fallback] models`，全仓没有任何代码读它。因为这个，命令被**故意**留在未
声明状态——`slash/commands/mod.rs` 的 `DELIBERATELY_UNDECLARED` 里写的就是这条理由：
advertise 一个改变不了任何行为的开关，比不 advertise 更糟。它掩盖的真实缺口是：当会话的
模型不可用时，产品的做法是 `available.keys().find(…)`，即同族里 HashMap 迭代顺序碰到的第一
个模型；同族没有可选项就直接把会话标成 unavailable，之后每次 prompt 都返回「请开新会话」。
用户在这两件事上都没有发言权，而那个「第一个」本身还不稳定。

`[fallback]` 现在是一等配置：`agent::config::FallbackConfig { models: Vec<String> }`，
`/fallback` 写它，`MvpAgent::select_fallback_model` 读它。挑选集中在
`agent::models::first_selectable_fallback`：逐条按 persisted 模型同一套 catalog key / 路由
slug 解析，跳过账号当前选不中的，也跳过它正要替换的那个模型，取第一个真能服务的。接线落在
「模型不见了」被解决的两处——`restore_persisted_model`（排在内置的同族自选**之前**：用户
显式说过要什么，就不该先被一次随机选择代替），以及 `prompt()` 里「模型在 load 时就不可用」
的那条阻塞路径（换不上就重新 latch，绝不带着一个 catalog 里没有的模型继续跑 prompt）。

范围说清楚：这条链管的是**可用性**，不是单请求重试。turn 中途跨 family 切换会让历史里
model-minted 的 reasoning 条目失效（`encrypted_content_mismatch` 那条错误就是这个形状），
所以限流与 5xx 仍走各自的 retry 与终态路径；`/fallback` 的提示语与 `fallback.models` 的文
档行都按这个写，不留下「配了就会自动兜底」的想象空间。

测试：pager 侧 7 项——缺文件 / 坏 TOML / 非数组一律读成空链，`set|add|remove|clear` 在真
实 `config.toml` 上往返并保留其他键，无参数形态必须说明「只在不可用时切换」，被拒的参数不
得创建文件，`parse_models` 去重；其中 `the_written_chain_is_read_by_the_shell_config_loader`
直接调 `Config::new_from_toml_cfg`，写方与读方的形状对不上就红。shell 侧 3 项——跳过不可
选项、按 slug 解析、绝不返回被替换的那个模型、空链什么都不选。外加两条守护：
`every_command_file_in_this_directory_is_declared` 的例外表已清空（本仓最后一份「无构建编
译」的源码到此归零），`the_fallback_chain_is_reachable_through_the_registry` 盯 `Arc::new`
那一行——声明守护看不见它被删。`fallback` 同时进 `PAGER_COMMAND_KEYS`，否则同名 skill 会把
它遮蔽。非空转由五处注入变异证明（每次一处，文件按字节还原）：去掉「跳过被替换模型」→
slug 用例红；只取链首不 walk → walk 与 slug 两用例红；把链分支挪到同族自选之后 → 结构守护
红；写方键名改成 `chain` → 往返、报告与跨 crate 读取三项红；删掉 `Arc::new(…)` 注册 → 注册
表守护红。覆盖限制：两个接线的调用点都需要活的
`MvpAgent` 与真 catalog，只有结构守护，未端到端驱动。

### 修复：动态上下文裁剪（DCP）从「文档说它一直在跑、其实没被编译」到能用

`CHAOS.md` 写着 DCP「约 60% 上下文时自动注入裁剪提醒」，读起来像默认行为；实际上
`session/dcp_config.rs` 与 `session/acp_session_impl/selective_compaction.rs` 两个文件
**没有任何构建会编译它们**（crate root 的 `mod` 链到不了），文档描述的那套东西在二进制里不
存在。真实取值也不是 60%：30% 提醒档（带 `m0001` 形式的索引提纲）、90% 应急档（隔轮重新注
入）、外加按 turn 数触发的 iteration 档，而整套子系统的总开关是
`[compaction] strategy = "threshold"(默认) | "dynamic" | "both"`。文档现在按这些写。

`strategy` 之前只管工具那一半，阈值路径完全不听：`Compaction::should_auto_compact` 与
`should_prefire_two_pass` 没有任何 strategy 判断，于是 `strategy = "dynamic"` 的用户照样被
百分比触发的全量压缩打断——这正是他关掉的那套。两个入口现在都先过
`strategy.threshold_active()`；手动 `/compact` 不走这两个判断，任何 strategy 下都还在。

turn 循环侧的三个 hook（提醒注入、工具定义下推、`compress` 派发）之前分散三处、且其中两处
与门控隔得太远，守护测试只能整体放松；现在派发收敛为
`selective_compaction::run_session_compress_calls`，turn.rs 只在 `self.compaction.dcp_active()`
后调用它。顺带修掉一个假阳性：`advertised` 原本在整个请求体里搜工具名，而模型自己发出的
`compress` 调用会被回显进后续请求体，于是「默认配置不该 advertise 该工具」的断言永远为真——
改成只看 `tools[].name`。

真实缺陷：`ChannelChatPersistence::persist_selective_compaction` 是空实现，注释说这件事由
chat-state actor 负责，而 actor 成功提交块之后恰恰是回调这里——模型 commit 的裁剪块从来没有
落过盘，resume 之后那些 token 悄悄还回来了。现在它经
`PersistenceMsg::SelectiveCompaction` 写进会话目录的 `selective_compaction.json`（独立文
件，不改写 `chat_history.jsonl`：块只是历史的一次请求期投影，写失败只损失投影），
`load_session` 与 `load_session_without_updates` 都会带出来，`spawn` 在 chat-state 建好之后
调 `restore_selective_blocks` 重新施加；某个块与恢复后的历史不再吻合时逐块丢弃，不让一个陈
旧块赔掉整份投影，最终仍由 chat-state actor 的结构保护规则复核。

测试 27 项通过（`--lib -- dcp restore_tests strategy selective_compaction
channel_persistence_sends_selective`），新增的包括：strategy 决定阈值路径的四格断言、
`SelectiveState` 经两条 load 路径的往返（含撕裂文件读成 `None`）、恢复路径用真实
`ChatStateActor` 驱动的三个用例、以及 spawn 调用点的结构守护。非空转三处：删掉
`threshold_active()` 门 → strategy 测试红；工具定义下推改成无条件 → 「默认不 advertise」与
门控守护双双失败；派发同理。覆盖限制：persistence actor 那四行分支未被直接驱动，spawn 的调
用点只有结构守护。

### 新增：remote 部署的产物要先说清「谁签的、给哪台机器」

`chaos-remote install` 的验收清单上原本只有一项完整性检查：sha256。而那个摘要正是
**发送字节的人自己算的**——它只能证明字节完整到达，证不了字节是谁产的。拿到一次凭据、
或者能碰到构建流水线的人，于是可以让自己的字节变成目标机上下一启动的
`chaos-remote-server`，全程没有任何东西说「不」。仓库里其实**早有**签名基础设施
（release 的 ed25519 `.sig` sidecar、`CHAOS_SIGNING_PUBLIC_KEY`、
`docs/release-signing.md`、`scripts/verify-release-signature.sh`），缺的一直只是
remote 这条路径；TODO 里「本仓库没有签名基础设施」那句话是错的，已连同它掩盖的真实缺口
一起改正。

验签原语先被抽成叶子 crate `crates/codegen/xai-grok-signature`：`chaos-engine` 只被
`xai-grok-web`/`xai-grok-desktop` 依赖，而 `xai-grok-update` 依赖 `xai-grok-shell`，
让 engine 反向依赖 update 是一条形状错误的边（近似环）。`xai-grok-update` 以
`pub use xai_grok_signature as signature` 保留原来的 `signature::` 路径，调用方与
`tests/test_signature_integration.rs` 一行没改；`require-sig` feature 转发给新 crate。
新 crate 只做验证，公钥仍只能在编译期由 `CHAOS_SIGNING_PUBLIC_KEY` 固定（运行期可换的
信任锚等于把选择权交给被更新的远端），但把 `parse_public_key_b64` 与
`extract_signature_body` 露了出来，供「操作员在自己的环境/命令行里指定 key」的宿主使用。

`commit()` 的顺序就是它的安全性质：digest → 签名 → 平台文件头 → 才第一次 `mkdir`/
`rename`。host 默认 fail-closed（没配 key 也照样要求签名，并说明它无从校验），唯一退出口
是具名的 `--allow-unsigned-artifact` / `CHAOS_REMOTE_REQUIRE_SIGNATURE=0`，而关掉「必须
有」并不等于接受一个坏签名——送上门的签名照旧验。启动横幅打印本机策略，因为一台读不到
自己策略的 host 会在凌晨三点被误诊。拒绝理由带原因码：`signature_missing`、
`signature_malformed`、`signature_invalid`、`no_trusted_key`、`artifact_too_large`、
`wrong_platform`。平台那一查读 ELF/Mach-O/PE 头部自己声明的 target，与
`std::env::consts` 不符即在发布指针前拒绝；`access(X_OK)` 只回答「允不允许 exec」，
一个 0755 的错误架构文件照样通过，而分类不了的脚本包装**不拒**——错的方向是拦掉今天
能用的部署。

顺带修掉 sidecar 读取里一个会让人白跑一趟的坑：`openssl ... | base64` 会把 64 字节签名
折成两行，而 `extract_signature_body` 取第一行，于是好签名被当成 `signature_malformed`；
现在折行会被拼回来，`-----` armor 与 minisign 的 `trusted comment:` 行不再被当成签名体。

测试：`xai-grok-signature` 20 项、`chaos-engine --lib` 190 项、
`tests/remote_workspace.rs` 13 项（真 socket、真 CLI、真协议）。变异验证：把 `commit`
里的验签一行去掉 → 6 项 lib + 2 项集成测试红（未签名产物真的成了 current）；
`SIGNATURE_BODY_B64_LEN` 取 0 / 取 `usize::MAX` / 去掉 `trusted comment:` 过滤 →
各自恰好红一项。Docker 验收台 `scripts/remote-acceptance-in-docker.sh` 加了三种操作员
（给了 key 的、从没给过的、明说接受未签名的）与签名/未签名/换 key 签/改字节重算摘要/
非签名文件/跨平台产物六类部署，2026-10-02 在干净的 Debian 容器里实跑 **130 项检查全通过、
0 失败**（含「装上的东西与密钥签名的字节逐字节相同」「同一份未签名产物在 opted-out 的
host 上装得上、在 keyless host 上被拒」这类配对断言）。仍未覆盖：Linux 之外的远端主机
（platform CI 那两条腿跑的是单机测试），以及 exec capability 本身——那是一道独立的边界，
本改动不假装关闭它。

### 修复：`/adhd` 在发布构建里根本不存在

`crates/codegen/xai-grok-pager/src/slash/commands/adhd.rs` 随 release 0.2.123 发出去过，
但 `slash/commands/mod.rs` 里从来没有 `mod adhd;`（`git log -S"mod adhd"` 对该文件返回
空）。rustc 对"目录里有个 .rs 文件却没人声明它"完全静默：它不参与编译、不报错、不进
二进制，而文件自己的测试照常绿。结果是半接线状态——`[adhd].enabled` 仍被
`xai-grok-shell/src/agent/config.rs` 解析、`xai-grok-pager/src/acp/mod.rs` 仍按它注入
ADHD 规则，但用户没有任何命令可以打开它，只能手改 `config.toml`。现已补上 `pub mod adhd;`
与 `builtin_commands()` 注册，并按 shell 侧既有契约把 `adhd` 加进
`xai-grok-shell/src/session/slash_commands.rs::PAGER_COMMAND_KEYS`（漏掉这一步的话，同名
skill 会遮蔽或被遮蔽——仓库里那条 `pager_builtin_triggers_are_reserved_in_shell` 正是这样
发现遗漏的）。同批把该文件里一条只做 `assert!(!false)` 的占位测试换成四条真实回归：缺
文件/坏 TOML/非 bool 一律读成 off、`/adhd` 与 `on|true|1|yes|off|false|0|no` 在真实
`config.toml` 上往返、切换保留其他键、未知参数经**真实 `AdhdCommand::run` 入口**返回带
用法的错误且不创建配置文件；持久化逻辑改为接受显式路径（`run` 只是用真实 config home 调
它），测试因此不碰用户自己的 `~/.chaos`。`/fallback` 同批**故意不复活**：全仓没有任何代码
解析 `[fallback] models`，采样器也没有 fallback 链，声明它只会 advertised 一个静默无操作
的命令——这个例外连同理由一起写进了守护测试的 `DELIBERATELY_UNDECLARED` 表。

### 新增：panic-site census 与两条 CI 棘轮（含「没有任何构建会编译的源文件」清单）

`docs/audit-followup-report.md` §2 的「2,292 个生产 unwrap」是 2026-08 手工 grep 出来的，
口径没有记录，既不能重跑也不能和以后比较。`scripts/ci/panic-site-census.py` 把它换成可
复现口径：先对注释/字符串/字符字面量做保位空白化，再按三分法判定测试归属（位于
`tests|benches|examples` / 整文件 `#![cfg(test)]` / 命中点落在提及 `test` 的 `cfg(...)`
区间内，`cfg_attr(not(test), …)` 明确**不**算），最后只统计被 crate root 经 `mod` 链可达
的文件。2026-10-02 实测：31,621 个 `.unwrap()` 里 **351 个在生产（1.1%）**，生产
`expect` 596、生产 `panic!` 类 132，unsafe 位点 654（596 block / 25 fn / 7 impl /
26 extern）其中生产 420。两条棘轮进 `ci.yml`：生产位点只许降不许升
（`panic-site-baseline.tsv`，97 个 crate），以及"无构建编译的文件"集合一字不许变
（`uncompiled-sources.txt`，登记当时是 12 个；随后几轮判定完毕并清空，见上面那条快捷键
窗口的条目）。扫描器本身由
`scripts/ci/test-panic-site-census.py` 的 fixture 加六组注入变异证明非空转（最严重的
一次——"不承认任何文件是 crate root"——红 9 条检查）。清单里 12 个文件当下都是**已知**
状态而非新缺陷：`/adhd` 是其中唯一已证实的用户可见回归（见上一条），`/fallback` 是有意
保留，其余 10 个（含 12,750 行未编译的 dashboard/shortcuts 测试）在 TODO MT-6 逐条登记待
判定"接上还是删"。

### 新增：预览代理（`/preview/<port>/`，`crates/codegen/xai-grok-web/src/preview.rs`）

跑在 `127.0.0.1:3000` 上的 dev server 对「按名字访问这台 host」的浏览器是不可达的，
而常见答案——把 3000 端口一起开放——等于把一台没有鉴权的服务器放到网络上。
`CHAOS_WEB_PREVIEW_PORTS=3000,5173` 现在把每个被点名的端口挂到 `/preview/<port>/`
后面，宿主侧原有的 `Host`/`Origin`/loopback 规则一并生效。挂在前缀下会坏四样，
重写这四样正是它作为代理存在的理由：`Host` 与 `Origin` 改成上游自己的
（没带 `Origin` 的请求补一个上游同源值——直连它本来就是这个样子），`Set-Cookie` 的
`Path` 收窄到前缀并丢掉 `Domain`，`Location` 放回前缀之内，WebSocket upgrade 由代理
用自己的握手桥接（浏览器的 `Sec-WebSocket-Key`/版本/`Connection`/`Upgrade` 属于另一条
连接，不转发；子协议 offer 与 Cookie 转发）。请求体按 32 MiB 缓冲，不再吃 API 那条
64 KiB 上限。

安全上的取舍写进 ADR-005：这条路径**前后都不带**宿主的 bearer token——本机上一个
应用对「认证本服务器的凭据」没有索取权，而浏览器对页面里的任何子资源请求都不会附
`Authorization`，要求它只会拒掉页面而不是保护页面；作为代价，`/preview/<port>/` 底下
与 dev server 本身一样不设认证，因此端口白名单默认为空、默认只从 loopback 可达，
即使声明了 `CHAOS_WEB_PUBLIC_ORIGIN` 也要另一个开关（`CHAOS_WEB_PREVIEW_ALLOW_PUBLIC=1`）
才允许走公开名字——公开这台 host 和公开某个项目的 dev server 是两个决定。从别的机器
访问的正路仍是 `chaos-remote forward`。代理也无法重写应用自己 HTML 里写死的
`/main.js`，这条限制连同 `X-Forwarded-Prefix` 一起写在模块文档与 `CONTRIBUTING.md` 里。

测试面对的是两个真东西：一个 axum stand-in 像 Vite 一样拒绝不属于它的 `Host` 与
`Origin`（重写退化为透传时，测试会看到 app 的 403 而不是代理的 200），一个 raw socket
服务器逐字节读代理发出的握手、并用**它实际收到的 key** 推导 accept（复用浏览器的 key
在这里会直接握手失败）。15 项单测 + 14 项集成测试。变异验证抓到五处：外层 `Host`
透传、`Set-Cookie` 的 Path 不收窄、loopback 默认被关（两条测试各自抓到）、`Location`
不加前缀、bearer 被转发上游。另有八处变异是等价变异，没有改变线上一个字节：
`tungstenite` 的 `generate_request` 对 `Host`/`Connection`/`Upgrade`/
`Sec-WebSocket-Version`/`Sec-WebSocket-Key` 这五个头名各取 map 里的**第一个**值写上线、
把该名字的其余值全部丢掉（`HeaderMap::remove` 一次清空），所以「不丢外层值」「改用
append」「不 push 重写后的 Host」都不会有可观测差异；复用浏览器 key 那条则是另一回事
——每一跳都拿自己实际发出的 key 去校验 accept，换了源头也测不出来。代码注释原先把前者
写成「会拒绝带重复头的请求」，那是错的，已按事实改正，并新增一条测试钉住这个依赖的实际
行为：故意让 URI（3000）与 header map（9999）不一致，断言上线的是 map 的值——这既证明
重写确实是 app 看到的那个值，也说明上面八处变异为什么不可见。将来依赖行为变了，这条
测试会在原地报出来。

### 修复：CI 的 `installer integrity labs` 作业在这台 runner 上永远红

CI run `37014349712` 里这个作业 15 秒就失败，而且两个 lab 步骤全被跳过——红的是前置步骤
`Install the crypto binding the fixture signs with`，最后一行是 `unshare -rn true`：

```
unshare: write failed /proc/self/uid_map: Operation not permitted
```

GitHub 托管的 ubuntu runner 关掉了非特权用户命名空间，`unshare -rn` 在那里根本走不通，
而这一句被放在前置步骤里，于是它把同作业内那个本来能跑的容器 lab（`--network none`，
不依赖用户命名空间）也一起拖死了。现在：前置步骤只打印这台机器允许哪条路（不再断言），
命名空间由 lab 自己解析——先试 `unshare -rn`，不行再用 `sudo -n unshare -n`，两条都没有
才照旧 exit 2 并报出原因（「没有跳过路径」这个性质保留）。root 那条刻意不带 `-r`：加
`-r` 会把 lab 放进子用户命名空间，那里的进程对命名空间外的任何资源都没有特权，主目录是
`drwxr-x---` 时连本仓库都读不到（这台机器上实测就是 `Permission denied`）；只建网络命名空间
的 root 仍保留正常访问，而 `ip link set lo up` 依然可行，因为该网络命名空间属于 root 自己的
用户命名空间。`CHAOS_PS1_LAB_NS_ROUTE` 可以钉住走哪条：CI 只会走 root 那条，所以本机
（两条都允许）用它把 root 分支真跑了一遍——`unshare` 与 `sudo` 两条各 30/30 全绿，
`loopback is up` 与 `github.com cannot be resolved from this namespace (curl exit 6)`
两条前提在两条路上都真实通过，跑完 `/tmp` 下不留残余。

### 工程：Docker lab 的容器不再在一小时后从检查脚下消失

四个 lab（`install-sh-in-docker.sh`、`install-integrity-in-docker.sh`、
`npm-install-in-docker.sh`、`remote-acceptance-in-docker.sh`，共六处）都用
`docker run -d --entrypoint sleep <image> 3600` 把容器空跑起来，后续每一步都是
`docker exec`。那个 `3600` 是容器 PID 1 的寿命：一小时一到，`sleep` 退出、容器停止，
正在跑的 `docker exec` 被 SIGKILL。`install-sh-in-docker.sh` 真的撞上了——release 产物
150 MB 起，限速链路光下载就超过一小时，于是 lab 报的是
`FAILED install.sh exited 137 using only its built-in key`，外加两条「checksum/signature
没有 OK 行」，看起来像安装器坏了，实际是这套 harness 自己把机器撤掉了。现在时长改成
`CHAOS_LAB_KEEPALIVE`（默认 6 小时）并在头部打印出来：保留上限是因为如果进程被杀到连
`trap cleanup EXIT` 都没跑，无限寿命的容器会永久留在机器上；上限又必须长过一次慢但成功
的运行。`install-sh` 额外加了归因：安装器以 137/143 死掉且容器已不在 running 时，报的是
「容器停在半路，这说明不了 install.sh 的任何事」，而不是把退出码当作产品结论。

### 工程：`scripts/verify-in-docker.sh` 给自己的运行签指纹

容器 bind-mount 的是活动工作树，门禁读到的是别人正在写的文件。一次 `--full` 运行的
`cargo test` 就是这样红的：rustdoc 报一个尚未写完的模块缺 `reqwest`，其余门禁全绿，
而这次失败无法归因给任何 commit。脚本现在在第一个门禁之前、最后一个门禁之后各做一次
全量 `cksum`（tracked 与未 ignored 的 untracked 都算，新模块正是会被撞上的那类文件），
不一致就打印 `UNATTRIBUTABLE`、列出差异路径并以非零退出——一次和编辑撞车的运行既不能
算 commit 的结论，也不能算干净的通过。这段声明现在印在门禁结论**之前**：移动本身就是
失败最可能的解释，而只看到 `FAILED gates` 的人会先去怪代码；第二次 `cksum` 自己失败
（有路径恰好在求和时被改名或删掉）也归为 unattributable，而不是在结论之后静默中止。
顺带把 CI 两个 job 都设的 `RUST_MIN_STACK` 补进
容器环境，否则「跑 CI 同一串命令」这句话在 `xai-grok-shell` 的 actor 测试上不成立。

### 工程：发行物文件名从此四处对齐（`scripts/ci/test-installer-asset-names.py`）

一个 release 的产物名由 `.github/workflows/release.yml` 的六行 `copy_one` 决定，而问它要
文件的一共有四处：`install.sh` 用 bash `case` 拼、`install.ps1` 用一个 PowerShell 函数
（还带环境变量兜底）、`install.bat` 用三行 `set`、`chaos update` 用 `version.rs` 里的
`gh_release_asset_name`。任何一边改名，另外几边都不会知道，直到用户装的时候吃到一个 404，
而 404 看起来像「没有这个版本」，不像「这两个文件对不上」。新增的检查把四处逐一对照
`copy_one` 列表：本机跑得动的两处（`detect_platform`、`Get-AssetName`）是真的执行，跑不动
的分支（arm64）用字面量覆盖；Rust 侧的测试不再复述名字，而是真的去读 release.yml。检查已挂进
CI（`workflows-present`，以及 platform legs 上带 `--require`——那两条腿上缺 PowerShell 必须
让构建失败，而不是退化成只读字面量）。两次变异验证都能被抓住：把 release.yml 里的
`chaos-win32-arm64.exe` 改名 → `install.ps1` 与 `install.bat` 两条同时红；把 `install.bat` 的
`.exe` 去掉 → 只有它自己红。

### 工程：两个完整性实验台从此每次 push 都会跑

`install-integrity-in-docker.sh` 与新的 `install-integrity-powershell.sh` 此前都只在写出
它们的那台机器上跑过。只有作者会跑的验收台，和已经被删掉的验收台没有区别——而它们守的
恰恰是安装器的**校验逻辑**，那部分只在发版时才改动，是最糟糕的发现时机。CI 新增
`installer integrity labs` 任务（ubuntu-latest）把两个都跑起来：运行期不需要网络、不需要
release、不需要签名私钥，夹具自己生成密钥对并自签。`pwsh --version` 与 `unshare -rn true`
在第一步就断言，runner 镜像哪天不再自带 pwsh 会当场点名，而不是三分钟后死在 docker 步骤里。
两个实验台都没有跳过路径——缺 pwsh、缺 cryptography、或内核不肯建网络命名空间都是 exit 2
并给出原因，所以这个任务不可能因为「什么都没测」而变绿。

### 新增：Windows 安装器第一次被真实执行（`scripts/install-integrity-powershell.sh`）

`install.ps1` 的全部保障此前只被证明「能解析」：`check-powershell-syntax.py` 用真实
PowerShell 解析每个 `*.ps1`，但解析不等于安装。新脚本在 Linux 上用 pwsh 7.4.6 直接跑
`install.ps1`，不需要 Windows 也不需要 docker；脚本把自己 re-exec 进 `unshare -rn`，
于是整个运行只有一条 loopback 路由，github.com 解析不出任何东西（这一条同样被断言）。
它面对的 release 与 shell 实验台**完全相同**——生成器、镜像、请求日志断言都抽到
`scripts/ci/release-integrity-{fixture,serve,request-log}.py` 共用，故意不给两个安装器
各写一份近似但不同的夹具。30 项检查：一条正路（`checksum OK` + `signature OK` +
落 `~/.chaos/bin/chaos.exe` 的字节与夹具摘要逐个对上 + 夹具是第一个候选、从未尝试
origin 或公共镜像 + 夹具只被取过产物 / `SHA256SUMS` / `.sig` 三个文件），七种拒绝各种
两问（为什么拒、拒后什么都没装）：产物被改、`SHA256SUMS` 被重算、`.sig` 缺失、公钥
合法但不是我们的、manifest 无本资产行、manifest 是 HTML 错误页、下载体过小；外加公钥
设为空串时夹具记录到 0 次请求，以及两个逃生开关的代价。

它明确不覆盖三件事（脚本头部自己写着）：`[RuntimeInformation]::OSArchitecture` 会选哪个
资产名、Windows 会不会真的执行这些字节、以及注册表 `PATH` 写入（每次运行都带
`-NoPath`）。所以「Windows 安装器把可执行文件放上 PATH」仍是未测断言；「Windows 安装器
接受这串字节、拒绝那串」已经不是了。

实验台是否可能失败是被验证过的：把 `install.ps1` 里 `Download-GitHubFile` 打印
`why:` 的那段循环删掉重跑，恰好 3 项失败（`html_sums`、`empty_artifact`、404 可见性），
其余 27 项照绿，用户此时看到的最后一行是 `last error: Resource temporarily unavailable
(mirror.ghproxy.com:443)`——把真正答话的候选（夹具返回的 404）说成是公共镜像的 DNS 故障。
完整记录：`docs/verification/install-integrity-powershell-linux-2026-10-02.log`。

### 修复：`install.sh` 与 `install.bat` 会把截断的产物直接送去哈希

写上面那个实验台时发现 `install.ps1` 一直有 `-MinBytes 1MB`：产物不足 1 MiB 就在哈希
之前拒掉。`install.sh` 没有对应闸门，一条被中途截断的响应体会被原样哈希、然后把结果
报给用户；`install.bat` 是第三种版本——它用 1 MiB 这个数字**只**决定要不要去嗅探 HTML，
所以一段被截断的非 HTML 响应体直接落到 `certutil` 上，最后以「摘要不匹配」的面目出现。
`download_github` 现在接受同样的下限（第 5 个参数，1048576），错误信息是
`too small (N bytes) from <候选>`；`install.bat` 在太小且不是 HTML 时直接拒收。shell
实验台新增第 35 项检查，同时读三个安装脚本，盯住这三个数字不再漂移。同一轮也修掉 PowerShell 侧的同类报告缺陷（`install.sh` 的那半已在上一节
记过）：所有候选都失败时两个安装器都只报**最后一个**的原因，现在各打印最多 4 条去重后的
`why:`。

### 新增：离线的发行物完整性验收台（`scripts/install-integrity-in-docker.sh`）

`scripts/install-sh-in-docker.sh` 已经会用 README 头条那条命令装**真实** release，并确认
摘要与签名都报 OK。但一台只会拿到正确产物的机器回答不了唯一要紧的问题：字节不对的
时候它怎么办。装完之后才发现「摘要不匹配」根本不可能触发，和摘要检查被静默跳过，是
同一个结果。

新实验台因此自己造一份 release：产物、`SHA256SUMS` 里的一行、以及对产物字节签名的
ed25519 `.sig`，再用 `install.sh` 本来就支持的 ghproxy 镜像路径
（`${CHAOS_GITHUB_MIRROR}/https://github.com/...`）把它端出去。容器跑在
`--network none` 下：loopback 可用、DNS 什么都解析不出来，所以安装器消费的每个字节都
来自这份夹具，github.com 是物理不可达的（这一条本身是被断言的，不是假设）。35 项检查
（初版 34 项，第 35 项是上面那对 1 MiB 下限的防漂移断言）覆盖一条正路（`checksum OK` + `signature OK` + 落地可执行 + 相对 symlink 布局 + 二次运行
不重复下载 + 夹具只被取过那三个文件）与这些拒绝：产物被改一个字节、`SHA256SUMS` 被
按篡改后的字节**重算**（此时只剩签名拦着）、`.sig` 缺失、公钥合法但不是我们的、公钥
存在但为空串（且在发起任何请求之前就拒，夹具记录到零次请求）、manifest 里没有本资产
这一行、manifest 被换成 200 的 HTML 错误页、下载体为空；每个拒绝都同时断言
`bin/chaos` 没有留下。两个逃生开关的代价也被量化：只跳摘要时篡改仍被签名拦住，两个
都跳则真的装进去（这正是文档里「你就是在信任这次下载」的含义）。

第一次运行是 33/34，`html-sums` 那一项拒了却没说为什么——顺着它挖出
`download_github` 的真实缺陷（见下）。完整记录：
`docs/verification/install-integrity-linux-2026-10-02.log`。

### 修复：Windows 上编译不过的 kill-on-drop 测试，以及镜像失败原因被吞掉

`xai-tty-utils` 的 `kill_on_drop_tests.rs` 调用 `#[cfg(unix)]` 的
`process_not_running`，Windows 上整个测试目标编译失败（`E0425`），CI 的 Windows platform
leg 因此在 ripgrep 修好之后仍然一步测试都没跑。现在补上 Windows 侧的实现
（`OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` + `GetExitCodeProcess`，打不开即视为已消失，
读不到退出码则报「仍在运行」让调用方自己发现），测试夹具也换成两平台都有的常驻进程
（`sleep 300` / `ping -n 300 127.0.0.1`）与立刻退 0 的夹具（`true` / `cmd /C exit 0`），
Windows 由此真正执行这三个用例而不是被 `#[cfg(unix)]` 跳过。同轮清掉该 crate 在
Windows 上的 4 个 unused-import 警告。`cargo check --all-targets` 在
x86_64/aarch64-pc-windows-msvc 与 x86_64-apple-darwin 三个目标上均零警告零错误。

`install.sh` 的 `download_github` 原先只报**最后一个**候选源的失败原因：镜像返回 200 却
是一段 HTML 错误页时，用户看到的是「HTTP 000 from 最后一个公共镜像」，跟
`tip:` 建议再换一个镜像，于是永远看不到真正的原因。现在按顺序打印去重后的前四条原因
（`why:`），`last:` 一行保持原样。这个缺陷是上面那个实验台的 `html-sums` 检查第一次
运行时暴露的。

同一轮还修掉一处时间性 flake：`xai-grok-pager` 的
`tool_media_same_length_same_mtime_rewrite_retries_failed_load` 假设「原地重写一定会推进
ctime」，而内核给 inode 时间戳打的是粗粒度时钟——本机实测 2000 次连续原地重写里有
1896 次 ctime 纹丝不动，也就是说这个用例此前只在两次写恰好跨过时钟刻度时才通过。
现在它重复重写直到 ctime 真的移动，并断言长度、inode、mtime 三者全程不变。

### 新增：本地端口转发（`chaos-remote forward`）

远程 workspace 会话此前只能读写文件和跑命令：远端主机上的服务（评审环境的
web 端口、只监听本机 loopback 的数据库）从开发机上是够不到的，而
`--capability port-forward` 是一个被点名拒绝的占位能力。现在它是真的。

语义取 `ssh -L`：`chaos-remote forward --to HOST:PORT` 在**执行命令的那台机器**上
开一个 loopback 监听口，每来一条连接就在 tunnel 的另一头重新拨一次 server、以
forward ticket 完成握手，然后把连接变成裸字节双向搬运。因此 HTTP 与 WebSocket 都能
原样穿过。反方向的 remote forwarding（`ssh -R`，由 server 开放监听口回连客户机）是
另一种信任方向，本 build 不实现：endpoint 类型 `remote-forward` 保留名字但直接拒绝，
并指向 `port-forward`，避免配置写反方向时静默按另一个方向生效。

四条规则把「会话可以开任意 socket」与这件事区分开：目标由运维方的
`--allow-forward-to host:port`（可重复、默认为空）决定，空列表只能一律拒绝，所以只给
`--capability port-forward` 而不列目标的 server 直接拒绝启动，列了目标则自动带上该能力；
forward ticket 存在与 session 凭据分开的 vault 里、签发时即绑死一个 `host:port`、
只授予 `port-forward` 一项能力（读不到也写不了它顺路连上的 workspace）；授予的
连接数与寿命由 server 用 `--forward-max-uses` / `--forward-ttl` 封顶后回答，用完或到期
本地监听口自行关闭，且关闭会话即撤回 ticket；本地端只允许 loopback，`--listen 0`
向系统要一个空闲端口并在有人连进来之前先打印出来，端口被占用是点名拒绝而不是换个
端口继续。

证据：`chaos-engine` 单测 16 项（`src/remote/forward.rs`，含 WebSocket 握手穿过隧道、
预算耗尽、目标不可达、ticket 不是 session 凭据），CLI 各 10 项，以及
`scripts/remote-acceptance-in-docker.sh` 在部署环境里的 22 项实测——远端容器起
`python3 -m http.server` 与一个按 RFC 6455 手写的 WebSocket 回显服务，另一侧容器经转发取回
的文件与该主机磁盘上的摘要一致、握手成功并读回只存在于远端主机上的字符串（lab 先断言开发机
容器 `grep` 不到它），未列出的目标虽然确有监听口仍被拒绝，两次连接用完即监听口自行释放。

### 修复：`install.ps1` 此前根本无法解析

`scripts/install.ps1` 有一个多余的右花括号：`21f5a186`（重排验签块）把外层 `try`
提前闭合了，留下一句没有 `try` 与之配对的 `} finally {`。PowerShell 7 的解析器报
`The Try statement is missing its Catch or Finally block`（538 行）与
`Unexpected token '}'`（541 行），也就是说 README 写的
`irm https://raw.githubusercontent.com/.../install.ps1 | iex` 在下载任何东西之前就报错。
用 `git show` 逐个解析历史版本定位到引入点：`74aa29ab` 及更早都是干净的。

同一件事的另一面是「为什么没人发现」：Linux 上的任何 job 都不会执行它，而 macOS/Windows
两条 platform leg 只构建和测试 Rust workspace。现在
`scripts/ci/check-powershell-syntax.py` 用真实 PowerShell 解析仓库里每一个 `*.ps1`
（四个文件：`scripts/install.ps1`、`scripts/test-platform.ps1`，以及 pager crate 里两个
上游 grok 安装脚本），并在 `platform tests` 两条 leg 上以 `--require` 运行——没有
PowerShell 就失败，而不是静默跳过。删改验证：给 `scripts/test-platform.ps1` 追加一段
没有闭合的 `try {`，门禁立刻以两行错误失败，还原后恢复绿色。

解析不等于安装：Windows 安装脚本能否真的把可用二进制放到 PATH，仍然没有任何实测。

### 修复：`install.sh` 从未真正比对 SHA256SUMS

`install.sh` 里 `verify_checksum()` 定义了、但**没有任何调用点**：`curl | bash`
传完 100–150MB 之后不会去取 release 的 `SHA256SUMS`，直接就安装。函数里每一条错误
（mismatch、缺条目、取不到 sums）都不可达，而它上方的注释写的是相反的行为。
`install.ps1` 与 `install.bat` 是内联实现，两者都真的执行了比对，只有 sh 版本漏了。

这条是 `scripts/install-sh-in-docker.sh` 在干净 Debian 容器里跑真实 release 时报出来的
（断言里包含「必须出现 `checksum OK`，而不是被静默跳过」）。现在 `verify_checksum` 在
`verify_signature` 之前调用，两者都在 `chmod +x` 之前；
`scripts/ci/test-installer-signature-policy.py` 加了结构性断言把这类缺陷钉住：两个检查
都必须有顶层调用点、顺序正确、且早于产物被赋予执行权限。

### 修复：文档里的安装命令原本装不完

`install.sh` / `install.ps1` / `install.bat` 验签所需的公钥此前**只**来自
`CHAOS_SIGNING_PUBLIC_KEY` 环境变量，而三处验签都是 fail closed。README 头条的
`curl -fsSL https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.sh | bash`
不会设置这个变量，因此这条被文档推荐的命令在任何干净机器上都无法完成安装——用户要先等
100–150MB 传完，才看到一句「缺少 CHAOS_SIGNING_PUBLIC_KEY」。

- 三个安装脚本现在内置同一把发布公钥（`DEFAULT_SIGNING_PUBLIC_KEY` /
  `$DefaultSigningPublicKey`），`CHAOS_SIGNING_PUBLIC_KEY` 仍可覆盖，供自行签名的 fork 使用。
  公钥本就是公开信息（同一值已在公开 repo variable 中），签名依赖的是只存在于 Actions
  secret 的私钥半边。显式把该变量设为空字符串仍按错误处理，以保证 fail-closed 分支可达。
- 公钥与 `python3` + `cryptography` 的检查移到**下载开始之前**：无法验签的安装应当立刻失败。
- 新增 `scripts/ci/test-installer-signature-policy.py` 断言：三端内置同一把 32 字节公钥、
  且前置检查排在下载之前。
- 新增 `scripts/install-sh-in-docker.sh`：在 stock Debian 容器里按 README 那条命令真实安装
  真实 release，并验证「外来公钥被拒且不动已装产物」「空白公钥在下载前即失败」两个反向对照。
- 新增 `scripts/verify-release-signature.sh`（配 `xai-grok-update` 的
  `verify_release_artifact` example）：用 shipped 的 `signature::verify_file` 校验真实发布产物。
  `--tag v0.4.2 --all` 下六个产物（含两个 Windows `.exe`）全部接受，翻转一字节即拒收。
- 新增 `crates/codegen/xai-grok-update/build.rs`：公钥经 `option_env!` 编译期注入，而 Cargo
  默认不跟踪环境变量，改公钥后重新构建可能静默沿用旧公钥；现由
  `cargo:rerun-if-env-changed=CHAOS_SIGNING_PUBLIC_KEY` 固定。
- 自动更新下载现在校验字节数（`check_complete_body` 与 range 字节数检查）。此前截断的
  响应体可被当成完整产物继续安装，`set_len` 预分配还会在短读的 range 请求里留下静默零洞。

### npm

- Windows 平台包名仍被 npm 的 `0.0.1-security` 占位，元包 pin 的版本从未存在，而 npm 对
  不可解析的 optional 依赖是静默跳过：Windows 上 `npm install -g chaos-code` 会成功，
  但每次执行 `chaos` 都失败。新增 `scripts/npm-install-in-docker.sh` 用
  `npm install --os=win32 --cpu=x64` 从 Linux 复现并报出该状态。
- 启动器在缺少平台二进制时的提示改为点名 pin 的版本并给出 `npm view <pkg> versions`；
  原文案只提 `--no-optional` 与「平台不支持」，两种都不是此处真因。

### 发布流程与版本

- 版本的唯一可信源是 npm meta 包 `crates/codegen/xai-grok-pager/npm/chaos/package.json`。
  必须与它相等的集合——六个 `chaos-code-<platform>` 包版本、meta 里对应的
  `optionalDependencies` pin、构建二进制的 `xai-grok-pager` + `xai-grok-pager-bin`、
  以及本文件的 `## <version>` 段落——现由 `scripts/ci/check-version-lockstep.py` 逐项比回
  该文件，并挂在 `ci.yml` 的 `workflows-present` 上；`--published`（需网络，非门禁）另比
  npm 上真实存在的版本。规则与四个独立版本 crate 的原因写在 `CONTRIBUTING.md`
  「Release versioning」。漂移之所以危险：更新器拿一个文件里的 `--version` 字符串去比
  另一个文件生成的 feed，公开标签之后才暴露。
- 新增 `docs/release-signing.md`（签名 runbook）：六个被签产物与 `.sig` 格式、私钥所在
  secret 与公钥所在 variable 的确切名称、跨 cryptography 版本可用的密钥生成片段、谁能签、
  以及轮换的真实后果——公钥经 `option_env!` 编译期固定，一个二进制只认它构建时那把钥匙，
  无 key id、无并行窗口、无吊销，因此跨轮换边界的 `chaos update` 必然拒签。
- `scripts/verify-release-signature.sh --tag v0.4.2 --all` 的 16 项实测记录归档在
  `docs/verification/release-signature-v0.4.2-2026-10-02.log`。

### 工程

- `xai-test-utils` 的 git 辅助函数禁用 `maintenance.auto` 与 `gc.auto`：`git commit` 会
  detach 一个后台维护子进程，它创建 `.git/objects/maintenance.lock` 并在结束时删除，
  于是拷贝 `.git` 的 fixture 与它竞态——lock 出现在目录列表里、被打开时已消失。CI 上
  `xai-fast-worktree` 的 snapshot 用例因此随机 `os error 2` 失败，而 rust job 一红，
  macOS/Windows 两条 platform leg 直接报 skipped，这才是它们至今没跑出证据的原因。
  容器内（git 2.55.0，与 runner 同版本）改前 12 次跑挂 2 次、改后 30 次跑 0 挂。

## 0.4.2 — 2026-09-23

本版本修复发布流水线与 npm 发布安全性，并默认将 npm 发布与 GitHub Release 解耦；功能内容延续 0.4.1。

- `release.yml` 的 `Read pinned toolchain` 增加 `shell: bash`，修复 Windows runner 默认 pwsh 导致的失败；新增 `scripts/ci/check-workflow-shells.py` CI 门禁。
- npm 发布不再有 `continue-on-error` 假成功；真实 npm 发布后会逐包核验 registry。默认只发布 GitHub Release，只有仓库变量 `CHAOS_NPM_PUBLISH_ENABLED=true` 才启用 npm。
- 发布脚本要求平台包归档完整，拒绝用 `.gitkeep` 冒充二进制；缺平台时不发布固定依赖六个平台的元包。新增回归测试覆盖空、部分、完整平台集合与 registry 假阳性。
- 已发现 npm 上两个 Windows 平台包名为 `0.0.1-security` 安全占位包，尚待包所有者通过 npm 支持处理。因此本版将 npm 默认关闭，不影响 GitHub Release 二进制交付。

## 0.4.0 — 2026-09-22

### 上游同步：精选外科手术式修复

- 从上游 `48271133`、`75810042` 两个批次里手工搬运与分叉自持章节相符的修复：
  压缩重试不再把 TPM 429 误判为上下文溢出；长任务计时显示进位到小时，不再停在
  分钟；CJK 与色深渲染测试引入固定色深机制。
- 依赖对齐上游：`quick-xml` 0.41、`tikv-jemalloc` 0.7。
- `SOURCE_REV` 维持 `72a61251fcffb464bcc687aeb5a998e5a98ec0c9`；本轮移植的每一处
  都在提交信息里写明上游提交号，不虚报对齐点。

### 用户指南中文化

- 补完 20 篇纯英文章节，全书 27 篇的正文、表格与目录条目均为中文。
- 正文 H1、`docs.rs` 里的文档名与目录条目统一为同一个中文名，`/docs` 里点进
  去不再中英混排。
- 删除本分叉不存在的功能章节与段落：Grove / `grok clone`、grok.com 浏览器登录
  与 OIDC、`docs/internal/*`、Terminal 主题；`/login`、`/logout` 按本分叉的
  兼容桩行为如实改写。
- 路径与命令按分叉实际改写：`chaos <cmd>`、`~/.chaos`（兼容读取 `~/.grok`），
  其余 `GROK_*`、`xai-grok-*`、`grok-<模型>` 保持原样。
- 应用内文案：快捷键详情页的 42 条 `long_help`、弹窗底栏的 30 个快捷键标签，
  以及 `stash`、`Shell` 两个条目改说中文；键位名（`Ctrl+P`、`j/k`）与命令名
  保持 ASCII。

### 中文化机器校验

- 新增 `scripts/check-doc-l10n.py`：把围栏代码块、行内代码 span、表格结构与
  单元格数、链接目标、标题层级、正文数字、上游旧名做成不变量，翻译不可能
  悄悄改掉默认值或改坏代码块。
- 新增 51 条自测（`scripts/check-doc-l10n-selftest.py`）覆盖每个不变量的「该拦」
  与「该放」两个方向；新增表格单元格词典，重复出现的短单元格一次决定。
- 新增 `scripts/l10n-guard.sh` 作为收尾硬门：比较相对于 `main` 的中文行数，
  报告 `regressed` / `shrunk` / `fortress-breach` 三类回退。
- 修掉校验器自身的三个缺陷，并各加自测钉住：`--fix-anchors` 重写锚点时多加一个
  右括号（15 篇 55 处，`--links` 看不见，只有渲染时看得出）；`links` 判据与它
  自己规定的收尾动作自相矛盾（改为按标题位置把两侧目标折算到译文 slug 再比）；
  `--glob` 匹配不到文件时静默全 0 通过（改为硬报错并提示正确写法）。
- `scripts/doc-span-removals.tsv` 声明本分叉有意删除的行内字面量（15 条），
  删功能说明不再误报漂移，而理由留档可复核。
- 写作与执行约定记在 `sync/doc-l10n-conventions.md`，逐章核对结论记在
  `sync/doc-claims-verification.md`，全库漂移归因记在
  `sync/2026-09-18-curated-port.md`。

### 测试与构建修复

- 修复基线就失败的 8 条 CJK 渲染测试（采用上游 `75810042` 的色深固定机制）。
- 清零分叉侧残留的 32 条测试失败，并修复其暴露的 6 处真实缺陷。
- 修好公共快照里两个编译不过的测试目标：`registered_features_are_documented`
  依赖本仓库从未有过的 `docs/internal/`，改用 `internal-docs` feature 门控；
  `pty_e2e_scroll_selection` 丢了 `gap_row` 定义，按上游逐字恢复。
- 修好二进制改名后找不到二进制而失败的测试与测试工具。
- 恢复类与诊断类提示改说中文，并清掉二进制自称 `grok` 的残留。
- 清零并发下的偶发失败：整库 9192 条用例并发跑时每次挂 1–2 条且每次不同，
  根因是四类进程全局状态（主题 `Theme::current()`、语音开关、模态嵌入即
  minimal 标志、分叉新增的项目选择器）。按既有约定钉住/串行化，修完后整库按
  默认线程数并发连跑 6 次全绿。
- 修掉这些偶发失败暴露的真实缺陷：会话候选枚举在 cwd 桶被并发删除时硬报错，
  使按标题恢复会话直接失败（改为与「会话根不存在」同等对待）。
- 状态栏三个用例按译文标题定位指南章节，不再靠已中文化的英文锚点。
- 三条 Esc 用例仍在断言旧策略（中段 Esc 取消轮次），而本分叉早已按上游改为
  「中段 Esc 只提示 Ctrl+C，从不取消」；按上游 `75810042` 改名并重写为
  「提示 → Ctrl+C 取消」。

### CI 门禁恢复

- 全量验证时发现分叉的 CI 在 `main` 上本来就是红的，且三处互相掩盖。逐条修好：
  `cargo fmt --all`（48 个 hunk / 28 个文件）、`--all-targets` 下 3 个 crate 的 14 个
  测试代码编译错误（生产结构体加字段、改签名后测试没跟上）、30 条 clippy 告警
  （一条失效的 lint 配置、两处文档注释错位、一批策略性 allow 缺失）。
- 30 条告警在 1.92.0 与 1.94.0 下完全相同，不是工具链差异；处理原则是能改正确
  写法就改（`xai_dirs::home_dir()`、`download_client()`、
  `xai_grok_extra_ca::build_reqwest_client`、`dunce::canonicalize`），改不动才
  局部 allow 并写明理由。
- 顺带修好一处**空悬引用**：`xai-fast-worktree` 的测试调用了
  `crate::nfs::confined::tests::plant_journal`，该辅助函数在本仓库与上游的整个
  历史里都不存在（上游后来删掉了整个 `nfs` 模块）。删掉该行并就地写明原因，不
  新造一个来路不明的函数。
- 恢复后实测：`fmt`、`check --all-targets`、`clippy -D warnings`、release 构建、
  `--version`、`secret-scan` 全部通过。

### 命令行、README 与文档门禁（发版前补足）

- `chaos --help` 与 11 个子命令的帮助文案全部汉化：顶层 161 条 doc comment（`about`、
  `long_about` 与每个 `--flag` 的说明）加子命令模块 119 条，另含 `chaos du` 的
  `after_help` 与 `chaos mcp add` 的示例块。clap 的结构标签 `Arguments:` /
  `Options:` / `Commands:` **保持英文**——它们由 clap 渲染，顶层 `help_template` 里
  是字面量，子命令的 `{all-args}` 无法覆盖，中文化等于重写帮助输出层；顶层与子命令
  因此语言一致。
- 顺带修掉三处**本来就过期**的认证文案：`Logout` 声称「退出登录并清除缓存的凭据」，
  实际只打印一句说明就返回、不清任何东西；`Login` 的 `--oauth` / `--device-auth`
  仍在宣传本分支已移除的登录流程（`Command::Login` 忽略全部 flag）。英文原文同样
  是错的，忠实翻译只是让它在中文里第一次可见。
- 四份会被用户读到的 README 全部汉化：npm 元包与六个平台包、`xai-grok-pager` crate
  页，以及会随二进制落到 `~/.chaos/README.md` 的 `xai-grok-shell` README（61% 重写，
  同时把其中的 `~/.grok/leader.log` 改为实现真正使用的 `~/.chaos/leader.log`）。
- 用户可见文案不再写死 `~/.grok`：LSP 配置路径提示、workflow 与 skill 的工具描述与
  提示模板、托管配置写盘失败提示、缓存令牌说明、`voice_probe` 用法串——用户级一律
  走解析后的配置根，项目级 `.grok` 保持不变（实现确实只认它）。`builtin.rs:149` 与
  `:306-307` 与 sha256 常量耦合的字符串**逐字节未动**。
- 修两条跨文档**死锚点**（标题翻译改了锚点、入链未在同一次提交改）并补齐四个漏译
  标题。`crates/codegen/xai-grok-pager/docs/` 下 36 个文件此前没有任何自动化读过——
  这正是那两条死链能活过一整轮的原因。

### CI 门禁：版本、工具链与文档中文化

- **工具链版本单一来源**：`ci.yml` 与 `release.yml` 各加一步从 `rust-toolchain.toml`
  读 `channel`，删掉两处硬编码的 `RUST_TOOLCHAIN`（此前 CI 写 1.92.0、toml 写 1.94.0，
  CI 与本机构建长期跑不同编译器而无人察觉）。此后升级工具链只改一个文件。
- **新增 `scripts/ci/check-versions.sh`**：Cargo 的 `[package] version`、npm 元包、
  六个平台包版本与 `optionalDependencies` 钉版必须一致，平台包集合也必须与磁盘上的
  目录一致，否则构建失败。负向验证：把版本临时改成 `9.9.9`，脚本报 13 行 MISMATCH
  并退出 1。
- **新增 `docs-l10n` 作业**：对 `docs/**/*.md` 跑 `--english`、`--fork-names --strict`、
  `--links`、`--cells --strict` 四项。`--links` 是其中最容易漏的一项：翻译标题会改
  锚点，而入链可能写在别处。负向验证：把一处锚点改回英文，作业报 `dead anchor` 并
  退出 1。
- `workflows-present` 增加一行断言防止 `docs-l10n` 被静默删除。

### Compatibility

- 版本号统一为 `0.4.0`：`xai-grok-version`、`xai-grok-pager`、`xai-grok-pager-bin`、
  `xai-grok-shell` 联动；npm 侧的 `chaos-code` 与六个 `chaos-code-<平台>` 包，
  以及 `optionalDependencies` 里的钉版，按 `scripts/ci/stamp-npm-version.mjs`
  的同一口径钉到 `0.4.0`（此前平台包与钉版还停在 `0.2.121`）。
- 分叉层不变：二进制名 `chaos`、配置根 `~/.chaos`、遥测默认关闭、内置模型目录
  为空、`remote_fetch` 默认 false、不引入登录/OIDC 依赖。

## 0.3.0 — 2026-08-14 (unreleased)

### Security: 自更新验签链路

- `xai-grok-update`：`signature.rs` 新增 `is_placeholder_key()` helper，
  `lib.rs` 新增 `require_configured_public_key()`（启动时 gate，当
  `CHAOS_REQUIRE_SIG=1` 但公钥未配置时 fail-fast）。
- `auto_update.rs`：`install_gh_release` 和 `install_internal`（GCS）两条
  下载路径在 smoke-test **之前**插入 `verify_downloaded_artifact()` 调用，
  下载后先验签再 exec，避免 mmap 未验证二进制。
- 新增 `fetch_signature(url)` helper：拉 `<asset_url>.sig` 旁车文件。
- 验签行为受 `CHAOS_REQUIRE_SIG` 环境变量灰度控制：默认 `false`（过渡期），
  `true` 时拒绝未签名/签名不匹配的二进制。sig 文件 404 时 warn 并跳过
  （兼容旧 release），签名不匹配时删除文件并 abort。
- `xai-grok-update/Cargo.toml` 新增 `require-sig` feature flag：启用后
  `signature_required()` 默认返回 `true`（env 仍可覆盖）。release 构建可
  通过 `--features require-sig` 开启。
- `signature.rs` 新增 4 个单元测试 + `tests/test_signature_integration.rs`
  新增 7 个集成测试，覆盖合法/篡改/缺 sig/env 解析/占位符等场景。
- `.github/workflows/release.yml`：Build step 注入
  `CHAOS_SIGNING_PUBLIC_KEY`（repo variable）；新增 `Sign binaries` step
  用 Python `cryptography` 库对每个 `release-bins/*` 生成 `.sig`
  （原始 ed25519 签名，base64 编码），加入 release assets。
- `scripts/install.sh` / `install.ps1` / `install.bat`：在 checksum 验证后
  加 `verify_signature`（用 Python `cryptography` 库验证 ed25519 签名），
  失败时 abort。Python 或 `cryptography` 包不在 PATH 时静默跳过
  （兼容最小容器）。
- ed25519 密钥对已生成并上链：私钥（32 字节 seed）存入 GitHub Actions
  secret `CHAOS_SIGNING_PRIVATE_KEY`，公钥（32 字节）存入 repo variable
  `CHAOS_SIGNING_PUBLIC_KEY`。签名格式为原始 ed25519（无 minisign 头），
  与 Rust 验证代码、install 脚本全链路一致。
- 复审修复（4 处）：
  - install.sh/.ps1/.bat：验签前先探测 python + `cryptography` 包可用性，
    缺失时静默跳过而非误报"二进制被篡改"导致拒绝安装；
  - release.yml：签名 step 检测 `cryptography` 缺失时自动创建 venv 安装
    （ubuntu 24.04 PEP 668 环境不保证预装），避免整个 release 失败；
  - `auto_update.rs`：`fetch_signature` fail-open 行为（网络错误也当
    "无签名"放行）加 `SECURITY TODO(strict-sig)` 标注，后续收紧为仅
    404 放行。

### Behavior changes: Code Mode preset 可见性收窄

- `run_code` 工具确认仅在 `code`（`grok-build`）和 `ask`
  （`grok_build_ask_user`）preset 暴露，`explore`（只读）和 `plan`
  （只读）preset 不含 —— 此策略在 DSH 移植时已落地，本次补回归测试
  `run_code_tool_excluded_from_readonly_presets` 防止回退。
- 新增 `docs/code-mode-safety.md`：完整描述 Rhai 沙箱、预算上限
  （5K ops / 30s / 32 calls）、权限继承路径、preset 策略。
- `run_code/mod.rs` 模块注释加 `## Threat model` 段。

### Fixes: sampler reasoning_content 占位值标记

- `xai-grok-sampler/src/client.rs::apply_defaults`：提取常量
  `REASONING_PLACEHOLDER` 和 `REASONING_BACKFILL_WARN_INTERVAL`（5 分钟）。
  加 `TODO(net-gateway)` 注释，标记 bblbb 网关要求 thinking 模式下所有
  assistant 消息带非空 `reasoning_content` 是临时方案。
- 新增 `warn_reasoning_backfill()` 函数：throttle 到每 5 分钟最多 warn
  一次，避免长 thinking session 里每请求都刷 log。
- **修：backfill 触发条件过窄**——原条件仅 `reasoning_effort.is_some()`
  时回填，但 DeepSeek-R1 / Qwen3-Thinking / GLM-5 等内禀思维模型不带
  OpenAI `reasoning_effort` 参数，导致回填被跳过、网关仍 400
  （`The reasoning_content in the thinking mode must be passed back to the
  API`）。改为 `reasoning_effort.is_some()` 或「会话中已有 assistant 消息
  携带 `reasoning_content`」即触发——一旦模型发出过 reasoning，后续所有
  assistant 消息自动回填，无需参数。新增 3 个回归测试覆盖 effort 命中 /
  会话已进入思维模式 / 非思维会话不注入空字段三路径。

### Improvements: wrap SSH 间接路径支持

- `wrap_cmd.rs`：`SpawnPlan` 新增 `env` 字段，支持给子进程设置额外
  环境变量。`with_ssh_env_forwarding()` 扩展识别 `gcloud` / `aws` /
  `mosh` / `lftp` / `rssh` 等间接 SSH 工具（`KNOWN_SSH_FORWARDERS`）——
  这些工具不接受 `-o SendEnv`，改为直接在子进程 env 中设 `LC_GROK_*`
  变量，让内部 ssh 子进程继承。
- 新增 `program_is_ssh_forwarder()` 函数 + 3 个测试
  （`program_is_ssh_forwarder_matches_known_forwarders`、
  `with_ssh_env_forwarding_does_not_inject_send_env_for_gcloud`、
  `with_ssh_env_forwarding_does_not_inject_send_env_for_mosh`）。
- `pty_wrap.rs::run_wrapped_command` 签名加 `env: &[(String, String)]`，
  在 `CommandBuilder` 上设置额外 env。

### Test: 季度 ignore 审计 baseline

- 跑 `scripts/ci/ignored-tests.sh`（用 ripgrep 替代 awk 不兼容的 Windows
  环境），统计 540 个 `#[ignore]` 属性，174 个裸 `#[ignore]`（无 reason），
  0 个带 review date。
- **42 个 fork 专属 `#[ignore]`** 全部补 `; review 2026-10` 重审日期
  （billing/connectors URL/upstream defaults/SSO flow 等）。
- 新增 `docs/ignored-audit-2026q3.md`：完整审计报告（方法论/by crate 统计/
  fork 债务明细/下次重审步骤）。
- 更新 `docs/ci-test-debt.md`：fork 债务合计从 88 修正为 42（原口径含
  PTY e2e / scripted scenarios 等上游继承的 debt）。

## 0.2.138 — 2026-08-14

### DSH 移植（deepseek-harness）

承接 `docs/done/dsh-port-20260814.md`，三特性按 P3.1 → P2 → P1 顺序落地：

- **P3.1 Agent Preset**：扩展 preset 为 persona + 工具集 + prompt 的复合体。
  新增 `AgentDefinition` + `AgentPresetBuilder`、4 个 native preset（code /
  ask / explore / plan），`AgentSelectionConfig.preset`、`/preset` slash
  command、`--preset` CLI flag；7-tier 优先级链 `resolve_agent_definition`。
  共 + 579 xai-grok-agent 测试 + 7 resolve_agent_definition 测试。

- **P2 Ralph 循环**：复用 workflow 引擎 + spawn 基建，落地
  `session/workflows/ralph.rhai`（3-phase workflow + `default_schema()` +
  `schema_map` / `schema_text`）、`/ralph` slash command
  （`<objective> [--rounds N]`）、`BuiltinAction::Ralph` 调度；
  3 workflow engine 测试 + 3 ralph 集成测试。

- **P1 Code Mode**（独立 Rhai 运行时，不经 workflow 引擎）：
  `xai-grok-tools` 新增 `RunCodeTool` + `RunCodeHandle`（tool handle 模式）
  + `RunCodeToolInput/Output` + `RunCodeEnvelope`；6 个 active toolset
  显式包含 `RunCodeTool`（注册期 + 活跃集两步都必做），`ToolKind::RunCode`
  + `ALL_TOOL_KINDS` compile-time guard；`xai-grok-shell` 独立 Rhai 运行时
  `code_mode/mod.rs` + `SessionActor.run_code_tx` channel + listener
  桥接 `workspace_ops.call_tool()`；13 code_mode + 6 run_code 测试。

### wrap SSH 图片粘贴

`chaos wrap ssh user@host` 在 Windows 下 Ctrl+V 粘图片不工作的两条断链
都修了：

- **环境变量转发**：`chaos wrap` 给 ssh 子进程设的 `LC_GROK_OSC52_SINK`
  现在自动加 `-o SendEnv=LC_GROK_OSC52_SINK`（仅 ssh / ssh.exe，basename
  跨平台识别），piggyback 各发行版 sshd 默认的 `AcceptEnv LANG LC_*`，
  远端 `osc52_sink_active()` 终于能拿到信号。
- **bracketed paste 路径**：Windows Terminal 把 Ctrl+V 映射成只粘文字
  的 bracketed paste，图片被静默丢。`BracketedInserted` 来源在
  `osc52_sink_active()` 为真时也额外发 wrap 图片请求；本地有图就
  走 `try_handle_wrap_host_image_paste` 插入图片 chip。

`wrap_cmd` 6 个新单测 + `task_result` 3 个新单测 + 一个 pty_e2e
（沙盒 PTY 受限，本机未实际跑过端到端，需在真实环境复核）。

## 0.2.137 — 2026-08-13

### 审计跟进（Audit followup）

- 新增 `docs/telemetry-policy.md`：集中说明遥测默认关、谁收、收什么、
  怎么永久关，以及优先级解析顺序。
- 新增 `docs/telemetry-status-design.md`：`chaos telemetry status` /
  `disable` 子命令设计草稿，分三版落地（0.2.137 status / 0.2.138
  disable / 0.2.139 enable）。
- 新增 `docs/audit-followup-report.md`：unsafe / unwrap / ignored 测试
  三方向摸底报告 + 治理优先级。
- 新增 `scripts/ci/ignored-tests.sh`：全工作区 `#[ignore]` 统计脚本，
  支持 human / CSV / --stale 三种模式；配套
  `docs/ci-test-debt.md` 加季度审计流程。
- `xai-grok-update`：新增 `signature.rs` 模块（ed25519 离线验签，
  minisign 兼容格式，编译期公钥注入 + 运行时 `CHAOS_REQUIRE_SIG` 灰
  度开关，12 单测全过）。后续 PR 将接入下载链路和安装脚本。

### 采样 / 网关兼容

- 修：thinking 模式回传补全 `reasoning_content`，避免 bblbb 等代理网关
  报 `bad_response_status_code 400`。
- 修：plan_mode / scheduler 零参数工具流式丢失（vLLM 0.23 + GLM-5.2-fp8
  实测必现）—— `enter_plan_mode` / `exit_plan_mode` / `scheduler_list` 加
  可选 `note` 字段，保证 arguments 非空。
- 修：reasoning-only 重试风暴—— `request_task.rs` 改用内容判断
  （`empty_reason() == ReasoningOnly`）替代 thinking-model 白名单，
  覆盖任意思维模型首 turn 不再触发无谓重试。

### Sandbox

- 移植上游 `allow_path` 规范化，修复 trailing glob 误建字面 `**` 目录。

### CI

- 工作区全量测试打通：`cargo test --workspace` 取代 7-crate 排除列表；
  4 crate（pager / pager-minimal / pty-harness / update）经 per-crate 审计
  确认 0 failures 后移回。`xai-grok-update` 的 47 个 `gh-release` 测试
  暂时 `#[ignore]`（上游 `fetch_gh_release_version` 从 gh CLI 切到
  GitHub HTTP API（reqwest），需 wiremock 重写后恢复）—— 见
  `docs/ci-test-debt.md`。
- `xai-grok-pager-render` 新增 `test-support` feature，导出
  `is_ssh_session` / `set_test_*` 钩子；为 CI（NO_COLOR=1、SSH 会话、
  VS Code Remote 等环境）提供确定性的终端上下文与颜色支持。
- 测试栈溢出修复：CI 设 `RUST_MIN_STACK=8MB`；`auth_retry_budget` 栈溢出
  测试补 `#[ignore]`。

### 文档

- 引入上游 1.0.1 / 1.0.2 / 1.0.3 changelog（`xai-grok-shell/changelogs/`）
  作参考，不替换本地 `0.2.136` 顶条。
- 合并上游 `b13fa526` 的 fork-layer 清单 `sync/fork-layer-inventory.md`。

## 0.2.136 - 2026-08-11

### 上游同步

- 同步上游 grok-build `b13fa526`（SOURCE_REV `a51a1dc6`），含 fork 层再核对
  （`sync/fork-layer-inventory.md`）。
- 手建仓库（无 `origin/HEAD`）时，默认分支回退到唯一存在的
  `origin/main` / `origin/master`，再回退 `init.defaultBranch`；两者同时存在
  时不猜测。
- Markdown 表格在窄面板内改为单元格内换行/硬切，右边界 `│` 不再被裁掉
  （grapheme 硬切 + 带样式链接保留）。
- SQLite 会话存储加固：`open`/`open_readonly` 内部改用带截止时间的
  `SQLITE_BUSY` 重试预算（10s，共享 deadline 不叠加），网络挂载更稳。
- tracker 懒加载 model→tokenizer 同步（避免启动时全量加载 BPE 表）。

### 国产 provider 错误处理

- 新增 `ProviderErrorKind` 分类，识别国产 provider（freemodel / workbuddy 等）
  的永久性故障（计费拒绝、客户端标识拒绝等）并给出中文可操作提示，而不是
  笼统重试。
- 计费类错误 fast-fail（不消耗 retry budget）；`edge_client_china` 标记国产
  网关；`Retry-After` 支持 HTTP-date 解析。
- `/provider` 新增国产 provider 预设；README 补充国产网关接入说明。
- 默认 retry budget 上限收到 8（与 opencode 对齐），避免国产网关限流时无限重试。
- thinking 模型 reasoning-only 空响应不再触发重试（避免误判为失败）。

### 采样 / 重试修复

- 修：`classify()` 多 tag 精确匹配漏洞 + `exceed.*context` 正则误用，导致
  错误分类错位。
- 修：OpenAI 兼容网关返回极小 SSE chunk 时采样器 panic（EAGAIN 路径），
  现容忍最小 chunk。
- 修：`record_response_token_usage` 测试调用签名与上游 merge 后的新签名对齐。

### 速率统计修复

- 修：`decode_tokens_per_sec` 未从 `UsageTotals` 传递到 pager，导致响应结束后
  速率 chip 不显示。
- 修：回合均 tok/s 不再把静默时间计入分母；速率统计不再把等待时间计入解码时长。

### Modal panic 修复

- 修：渠道/客户端模态框按键未分发到子组件导致 `unreachable!()` panic。
- 补全 `ProviderModal` / `ClientModal` 的 render、paste、mouse 路径，硬化 panic
  边界；第三处 `unreachable!()` 替换为 safe fallback。

### 其他修复

- 修：搜索 bootstrap 标记检查加重试，消除并发单飞竞态。
- 修：工作流恢复排序先于截断；忽略 chat-mode 死代码测试。
- 修：`agent_view` 合并后重复的 `last_turn_summary` 字段移除。

### CI

- 修：`rust-toolchain.toml` 覆盖导致 macOS x64 交叉编译缺 target，显式
  `rustup target add` 修复。
- 修：测试栈溢出，设置 `RUST_MIN_STACK=8MB`；`auth_retry_budget` 栈溢出测试
  补 `#[ignore]`。
- 修：上游 merge 带来的 clippy errors（`xai-grok-pager` 56 个、`xai-fast-worktree`
  disallowed_methods、3 个 test target）。

## 0.2.135 - 2026-08-07

### Removed

- 移除整个 CatPaw 渠道：`xai-catpaw` 原生协议 crate、扫码登录、Remote Agent
  （pod）通道及相关配置/UI/测试全部删除，`ApiBackend` 与 `SamplingConfig`
  不再携带 CatPaw / Remote Agent 通道类型。`[model_providers.catpaw]` 与
  `[model."catpaw/*"]` 用户配置需手动清理（本机配置已清理，备份见
  `config.toml.bak-catpaw-*`）。

### Bug fixes

- 修：右上角实时速率（tok/s chip）默认不显示——没有任何速率样本时芯片整体
  隐藏。现改为渲染暗淡的 `🐢 0 tok/s` 占位，保证实时速率默认常驻，速率从无到有
  的过程中不再凭空消失。

## 0.2.133 - 2026-08-06

### Bug fixes

- 修：`--client workbuddy` 走 freemodel 网关（work.freemodel.dev）时 403
  `unsupported_client` 的问题。逆向真实 WorkBuddy 客户端（header dump + 对线上网关
  的消融实验）确认校验点在 body 而非 headers：
  - `messages[0]` 必须是 system 消息且以精确的 31 字符前缀 `This conversation is powered by`
    开头（大小写敏感）；未命中时自动注入 marker system 消息。
  - body 中不得出现指纹子串 `You are Chaos`（网关据此识别 Chaos 客户端并拒绝，即使
    marker 前缀正确）；现于每个文本块（字符串与 blocks 两种形态）中将其替换为
    `You are the Chaos`。
  验证：`chaos --single ping --client workbuddy --model gpt-5.6-sol` 由 403 变为返回
  `pong`；非 WorkBuddy 请求不受影响。
- 修：`--client workbuddy` 403 时错误文案补充「WorkBuddy profile is active」引导，避免
  用户误以为 `--client` 未生效。

### Refactors

- 共享 aux-model sampler finalizer（session summary / 标题等辅助请求复用同一构造路径）。

### Chores

- WorkBuddy client profile 更新至 5.3.8。

### Tests / Tooling

- 新增 mock 推理服务器（`mock_server.py` / `single_mock_server.py` / `run_mock_server.*`，
  含 xai-grok-test-support 的 mock_server bin）与 WorkBuddy 请求 header 捕获脚本
  （`test_workbuddy_headers*.py` / `test_simple_wb.py` / `capture_listener.py`），
  用于本地复现与验证 WorkBuddy API 路径。
- `chaos-upstream-sync` skill 触发范围收窄。

## 0.2.131 - 2026-08-03

### Bug fixes

- 修：`build_session_info()` 不再丢弃 `decode_tokens_per_sec` / `avg_output_tokens_per_sec`，
  让 chat-state ledger 的速率数据能传到 pager 状态栏 chip（之前 `acp_session_impl::session_setup.rs`
  里硬编码为 `None`，导致右上角 tok/s chip 永远不显示）。
- 修：`cargo test` 因 `ClientProfile` 缺 `extra_headers` / `env_http_headers` 字段而编译失败
  （workbuddy 提交给结构体加了字段但测试 fixture 没跟上）。
- 修：`/context set` 报错信息从 `Internal error: ErrorCode(InvalidRequest) { … }` 改为透传结构化
  `acp::Error`，UI 现在显示「Cannot change the context window while a turn is in flight;
  try again after the turn finishes.」类中文消息。
- 修：`Token 用量统计` overlay 报 `aggregate usage not supported by this agent version` —
  `MvpAgent::ext_method` 顶层 dispatcher 漏掉 `x.ai/usage/aggregate` 路由。补上后请求能真正到达
  usage handler，overlay 双栏（本次会话 + 累计）正常填充，并用生产分发链回归测试锁定。

### Features

- 实时 tok/s chip：streaming 中按 `cl100k_base` BPE 累加 token 数（编码器不可用时回退
  `chars/4`），并以 chunk 间隔 EMA 展示当前生成速率；静默超过 1 秒后不再显示旧速率。
  Post-hoc `decode_tokens_per_sec` 仍负责响应结束后的速率显示。
- Token 用量 overlay 降级：单边 fetch 失败时保留另一边数据 + dim 部分失败提示，
  pending 侧显示加载中；仅两边都明确失败时才进入 `Failed(error)`。每次打开携带请求代次，
  关闭后重开不会被上一轮 late result 污染；会话尚未建立时明确显示会话侧不可用。
- 子 Agent 完成后按实际 `output` 文本计入父 turn 的实时输出速率；累计上下文
  `tokens_used` 只用于任务用量展示，replay 和重复终态不会重复计费。速率优先复用子 Agent
  tracker 的真实 decode rate，缺失时以 `output_tokens / duration` 估算；父 Agent 尚未输出文本的
  subagent-only turn 也能立即显示正数 tok/s。无实时样本或静默衰减为 0 时显示 `📊 平均 N tok/s`
  对话累计平均值，不再重复显示上下文剩余百分比。

### Refactors

- `UsageDetail::Ready` 枚举新增 `partial_failure: Option<String>` 字段；`session` / `aggregate`
  改成 `Option<Box<PromptUsage>>` 以支持半数据状态。`UsageDetail::Failed` 仅在两边都失败时
  使用，合并错误信息。
- `LiveStreamingRate` 字段从 `total_chars: u64` 改为 `total_tokens: u64`（BPE token 数），
  配套 `AcpUpdateTracker.token_encoder: Option<Arc<tiktoken_rs::CoreBPE>>` 懒加载字段。

### Tests

- 新增 `usage_partial_failure` 状态机和 overlay 渲染/竞态测试，覆盖双请求成功/失败乱序、
  单边 pending、late success、关闭后重开、无 session 与带分号错误文本的精确清理。
- 新增实时 tok/s 测试，覆盖 BPE 累加、EMA/静默衰减、live 零值不回退旧速率，
  以及子 Agent output 的 context/replay/重复终态/subagent-only turn 隔离和状态栏可见性。

### Dependencies

- 新增 `tiktoken-rs = "0.7"` 到 `xai-grok-pager/Cargo.toml`。cl100k_base 表~1.5MB，
  lazy-init 在第一个 chunk 到达时加载（启动开销零）。

## 0.2.128

### `/provider` 添加渠道支持从 Cline 导入

- 新增 `cline_import` 模块（`xai-grok-shell`）：只读扫描 VS Code / Cursor / Windsurf / VSCodium 的 `globalStorage/state.vscdb`，提取 Cline 的接口配置（base_url / auth_scheme / api_backend / api_key / model id）。
- `/provider add` 预设列表末尾新增「从 Cline 导入」选项（仅当检测到可用渠道时显示）。选择后列出所有可导入渠道，Enter 即可落成 Chaos 渠道。
- Cline 通过 Electron `safeStorage` 加密的 API Key（`v1:` 密文）标记为 🔒已加密，选中后引导用户手动粘贴。
- 全程只读打开 Cline 数据库（`SQLITE_OPEN_READ_ONLY`），不写回、不日志记录 Key。

### #18 思考等级请求失败时自动回退重试

- 当 provider 返回 400 且错误信息包含 `reasoning_effort` / `reasoning.effort` 时，自动移除 `reasoning_effort` 参数并重试，而不是中止整轮对话。
- 在 `RetryDecision` 中新增 `RetryWithEffortFallback` 变体，在 `classify_error` 中检测相关 400 错误，在 `apply_retry_decision` 中执行 effort 剥离和重试。
- 仅触发一次：如果重试仍然失败，则按原有 Fatal 逻辑处理。

### #19 `/think` 作为 `/effort` 的别名

- `/think` 已作为 `/effort` 命令的别名实现，两者行为完全一致。

### #20 系统提示词品牌修正

- 系统提示词模板从 `You are ${{ system_prompt_label }} released by xAI` 改为 `You are ${{ system_prompt_label }}, an AI coding assistant`，不再硬编码 "released by xAI"。
- `DEFAULT_SYSTEM_PROMPT_LABEL` 从 `"Grok"` 改为 `"Chaos"`，使用非 Grok 模型时助手不再自称 Grok。

## 0.2.127

### 多客户端请求档案

- 新增 `/client` 交互窗口，可选择 Claude Code、Codex、Grok Build，并支持自定义客户端的新增、编辑、删除和默认设置。
- 新增 `--client`、`chaos clients` 与 `chaos clients --json`，支持在同一工具中管理多种请求客户端身份。
- 客户端配置仅保存公开身份信息和环境变量名，不保存或传递 API Key。

## 0.2.124

### 上游同步至 `xai-org/grok-build` `5da6962`

- **B1**: `agent/models` 模块拆分：将 `agent/models.rs` 19 万行拆分为 `agent/models/{cache,endpoint,fetch,resolution,tests}.rs`，Chaos 保留 `default_models.json` 为空时的 `ConfigModelOverride` 测试种子，去掉 upstream 的 `sync_managed` 逻辑（`fetch_settings_blocking` 直接返回 `Option<RemoteSettings>`），同时 hoist `STARTUP_*`/`SETTINGS_*` 常量到 `xai-grok-http`（Chaos 继续使用 `shared_blocking_client`）。
- **B2**: workspace crate：`file_system/fuzzy.rs` 全量合并（`+584/-162`）；`session/git.rs` 适配新 `CommitResult`（Chaos 保留 `DeployError` 支持）；`workspace-types/src/rpc/git.rs` 新增 `CommitOutcome` / `PushStatus`、`GitCommitReq` `stage_all`/`seed_default_excludes`/`expected_branch` 字段；跳过 `preview_supervisor.rs`（上游依赖 `metric_donate::active_metrics_sink`）。
- **B3**: `xai-grok-tools` + `xai-tty-utils` runtime：62 个 upstream-only 工具文件全量合并；`xai-grok-tools-api/src/slash_commands.rs` 新增 `LoopFireMode` enum 和双参数 `loop_schedule_instruction`；`xai-tty-utils/src/runtime.rs` 全新文件，`process_scope.rs` 新增 `is_closed()`/`register()` 返回值；Chaos 在所有 `TaskSnapshot` 字面量添加 `is_backgrounded: false` 占位符（无 `bg_status` 状态机），revert `reminders/task_completion.rs` 和 `task/{mod,coordinator*,types}.rs`（upstream 移除了 Chaos 私有 `MonitorEventNotification`）；修复 SearchReplaceParams 的 Default impl（现在与 `#[serde(default = "default_true")]` 对齐，`include_user_edit_hint` 默认为 true）；
- **B4**: `workspace-types/src/rpc/workspace.rs` 新增 `BackgroundTaskSnapshotWire.description` 可选字段，hub_server 中透传自 `TaskSnapshot.description`（batch3 已加入）；
- **B5**: workspace permission 子系统（9个文件）：全量合并，仅把 `preview_supervisor.rs` 继续跳过；
- **B6**: workspace core（config/hub/session/git/error…）：17个文件全量合并，Chaos 继续保留 daemonize.rs / discovery.rs / project_config.rs 的 dual-path（.grok + .chaos）支持；
- **B7**: `xai-grok-shell` 剩余部分（约 162 文件）：分7个子批次（session/storage → testkit → terminal → tools → upload → util/config → slash_commands）全量合并，Chaos 保留 `is_backgrounded: false` 占位符、`LoopFireMode::Detached` 透传、telemetry 弱化、`default_models.json` 为空的 catalog 行为。

### 下游修复

- `xai-grok-shell/src/session/acp_session_impl/spawn.rs` / `xai-grok-shell/src/inspect/mod.rs`：透传 `folder_trust::project_scope_allowed(cwd)` → `resolve_permissions_with_provenance` 的 `project_trusted` 第二参数（batch5 新增，修复 batch5 引入的编译错误）；
- `xai-grok-workspace/src/hub.rs`：给测试的 `TaskSnapshot` 字面量添加 `is_backgrounded: false` 占位符；
- `xai-grok-pager/src/slash/commands/loop_cmd.rs`：三处以 `loop_schedule_instruction(args)` 调用改为双参数形式；
- `xai-grok-tools/src/reminders/task_completion.rs`：给 10 处 `TaskSnapshot` 字面量添加 `is_backgrounded: false` 占位符。

### 阻塞项（后续处理）

- **B8**：`xai-grok-pager` UI 层上游端口尚未开工，留待下一次同步窗口专门评估（涉及中文文案 / logo / 欢迎页更新日志的冲突面较大）。
- **blk1** — `xai-grok-mcp`：新 API `McpSpawnCtx::for_session` + `start_mcp_servers(EventWriter, OauthInteractivity)` 未移植，导致 `xai-grok-workspace/src/handle.rs` 和 `xai-grok-shell/src/session/handle.rs` 暂时留在 Chaos 当前版本；需要对齐 Chaos 已剥离的 OIDC 路径后再合入。
- **blk2** — `xai-grok-hooks`：`HookProvenance` / `HookSpec.layer` 未移植，导致 `xai-grok-workspace/src/workspace_ops.rs` 测试加层逻辑和 `xai-grok-shell/src/util/hooks.rs` 相关部分暂时跳过上 upstream。
- `metric_donate::active_metrics_sink`：Chaos 明确剥离该遥测捐赠面，继续跳过 `preview_supervisor.rs` 相关部分。
- **GitHub issues #15 / #16 / #17**：待本次分支合入 `main` 且用户确认口径后统一回复并关闭（本次同步仅完成实现层，未做远端 issue 写操作）。
- **50 项 pager UI 单测预存失败**：来自 `0.2.122` 基线，本轮 `cargo test --workspace -j 4` 复现（7740 pass / 50 fail），未在本次 sync 中修复，留作独立技术债项。

## 0.2.123

### #15 `--disallowed-tools` TUI 支持

- 从 headless-only 警告列表中移除 `--disallowed-tools` 和 `--tools`，现在 TUI 模式也生效。
- CLI 传入的 `--disallowed-tools` 和 `--tools` 接入 `ConnectFlags` → `CliAgentOverrides`，TUI 会话中内置工具按列表过滤。
- Leader 模式下两个 flag 会被识别为 unsupported 并发出警告。

### #16 配置模型参数不再发起 HTTP 请求

- `go_configure_model` 移除了 `load_models_for` 调用，打开「配置模型参数」界面不再冻结 UI 等待远端模型列表拉取。
- 用户直接手动输入模型 ID 即可配置参数。

### `/fallback` 命令

- 新增 `/fallback` 斜杠命令，管理备用模型链。
- 子命令：`set`（替换整链）/ `add`（追加）/ `remove`（移除）/ `clear`（清空）。
- 持久化到 `~/.grok/config.toml` `[fallback].models`。
- Agent 配置新增 `FallbackConfig` 结构体，sampler 层可读取备用模型列表。

### `/adhd` 命令

- 新增 `/adhd` 斜杠命令，切换 ADHD 技能集成。
- 用法：`/adhd`（切换）/ `/adhd on` / `/adhd off`。
- 开启后自动将 ADHD 辅助规则注入每个会话的系统提示词。
- 规则来源：https://github.com/uditakhourii/adhd
- 持久化到 `~/.grok/config.toml` `[adhd].enabled`。

## 0.2.122

### Token 用量修复

- **自动持久化**：每轮对话结束时自动将 token 用量写入 sqlite，不再仅在打开 `/usage` 面板时才触发。
- **Sentinel 归一**：火山方舟等网关在 SSE `usage.model` 里回传 `"auto"` 而非配置的模型名（如 `ark-code-latest`），现在自动用 `sampling_config.model` 重写，避免全部归到 `auto` 桶。
- **去重**：`record_session_usage` 写库前先 DELETE 同 session 的旧行，防止 auto/真实模型双计。
- **历史回填**：新增 `scripts/backfill-usage.py`，扫描文件系统历史会话 JSONL，将 sentinel 模型名重写为配置模型并写入 sqlite。

### #17 TUI 汉化

- 目标详情视图（goal detail）：状态标签、进度条目、完成度评估、最近历史、命令提示等全面汉化。
- Agent 状态栏：`goal_phase_label` 各阶段（校验中/规划中/执行中/空闲/失败/已中断/预算/完成）及 chip 名（"目标"）。
- 权限提示：编辑/bash/MCP 授权选项、始终允许/始终拒绝前缀、followup placeholder。
- 计划提示（plan nudge）："在规划？可用计划模式，快捷键 …"。
- 回退对话（rewind）："当前有一个轮次正在运行。"/"是否在回退前取消它？"/"取消轮次并回退"/"让它继续跑完"。
- Dashboard 模式标签：`plan` → "计划"、`always-approve` → "总是批准"、`auto` → "自动"。
- 首启 folder-trust（pager-minimal）："是否信任该目录下的内容？"/"允许，继续"/"拒绝，退出"。
- 截断指示器：`Ctrl-F to expand` → `Ctrl-F 展开`。
- 上下文信息栏：技能/MCP 服务器/工具计数等汉化。
- Scrollback verb group：读取/运行/搜索/子代理等动词标签汉化。
- Dashboard 行状态：Working → "运行中"、Response → "回复" 等。
- Session-scoped 命令在 dashboard 上的错误提示："/{name} only works in a session" → "请先打开会话再运行 /{name}。"

### 已知限制

- CJK 标签在 context info bar 中的列对齐尚未使用 unicode-width-aware padding，可能导致视觉上轻微错位（功能不受影响）。
- 50 个预先存在的单元测试失败（品牌 Chaos vs Grok、subagent replay count、extensions modal assertion 等），与本版本无关。
