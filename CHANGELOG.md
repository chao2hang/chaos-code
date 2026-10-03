# Changelog

## Unreleased

### 修复：关闭 PTY 时先松开终端、后发挂断，shell 于是走 EOF 路径 `exit 0`，把后台 job 永远留在世上

`pty_session::tests::close_pty_kills_a_background_grandchild` 在 run `37091358203`（commit `6b8b3dad`）
又红了一次，而那一步报 `finished in 300.22s`——这条断言自己的超时只有 5 秒。`TODO.md` 此前把它按
「负载抖动」结案（run `36368578937` 红过一次、`36381870574` 全量通过、本地连续 25 次重跑通过），
可 300 秒这个数与抖动毫无关系。把 `6b8b3dad` 那份文件原样取回工作区，用同一负载形状跑到第 4 轮：
红，wall `300.10s`，通过轮 `0.09–0.14s`。**300 就是那条 `sleep 300` 自己睡醒的时刻**——幸存者占着最后
一个 slave fd，而 reader 任务持有 master 的一份 dup，pty 的规则是只要还有 slave fd 开着，master 上的
`read()` 就不返回 EIO，于是阻塞任务不结束、runtime 收不了尾，红一次的代价被拖成 300 秒。同一份 panic，
在 panic 之前先把幸存者收掉，三次红的 libtest 时间是 `5.08 / 5.12 / 5.07s`。

**job 是被谁杀死的必须先钉清楚，否则「修 close 路径」没有着力点。** 测试本来就有 `assert_ne!` 钉住前提：
job control 把后台 job 放进了它自己的进程组，`killpg(shell_pgid, …)` 结构上打不到它。给通过轮套一层
strace，三次的信号面一字不差：本仓库只发 `kill(-shell, SIGHUP)` 与 `kill(-shell, SIGCONT)`，紧接着
`1017300 kill(-1017310, SIGHUP)`——那是 **bash 自己**发给 job 所在进程组的——然后它 `kill(self, SIGHUP)`
把自己打死。本仓库对那个 job 一个信号都没发过，**shell 的转发是唯一通路**。

**红的那几轮，shell 是「自己走出去的」。** 旧断言只报一个裸 pid，改进后的报告把两边的 `/proc` 状态和
shell 的死法一起打出来，三轮红形状一字不差：job 是 `state=S ppid=1 pgid=自己 session=shell 的 pid
SigIgn=0 SigCgt=0`（独立进程组、已被 init 收养、没把 SIGHUP 设成忽略、也从未被信号过——这把「job 自己
免疫」和「job 没收到」分开了），shell 是 `(no /proc entry) (exited 0)`。`exited 0` 就是全部线索：
`reap()` 原先先 `master.take()` 并结束 writer（两份 dup 里的两份）再挂断，终端先没，shell 的 stdin 就能
报 EOF，它按用户敲 Ctrl-D 那条路离开，而转发不在那条路上。改后的 `reap()` 只 `hangup()`，确认 shell 退出
之后才 `close_terminal()`——reader 与 writer 的 dup 让终端在这一步仍然开着，shell 只能以 SIGHUP 的方式
离开，于是走到转发。

**交替抽样 800 对：改前 19 红、改后 0 红。** 主证据不用「先后各跑一段」：两支二进制各构建一次（唯一
差异是 `reap()` 里那两行的顺序，测试代码一字未改、无诊断注入），负载只起一次，之后 A、B、A、B 逐轮
交替，判决取 libtest 汇总行**与**进程退出码两者。环境漂移因此同时落在两支上。加上顺序抽样的几轮，
改前 41/2241 ≈ 1.8%、改后 5/3200 ≈ 0.16%。

**没有归零，而且残余是另一种形状，这一条写在这里而不是藏进「已通过」。** 改后残余的红里 shell 的死法是
`killed by Hangup`（改前是 `exited 0`）：它确实被挂断打死了，却没有转发。给改后的顺序套 strace 抓到一轮，
整条 trace 只有 104 次 `kill`，**从头到尾没有 `kill(-job_pgid, SIGHUP)`**——bash 收到挂断后直接对自己
重新举起，本仓库的 SIGKILL 是在那之后才到的，只改变了记录到的死法。要闭合它需要一个能覆盖整个会话的
机制（扫 `/proc` 找 `session == shell pid`，或 cgroup 整组回收），而那比这条测试要的语义更宽：
`nohup`/`disown` 而未 `setsid` 的进程也在同一个会话里，今天终端关窗时它们是活的——这是未决的产品语义
决定，不是能夹在偶发修复里的改动。另一个**未验证**假设也记在 TODO 里：那条 trace 里 SIGCONT 先于 SIGHUP
送达（通过轮是 SIGHUP 先到），而 `ProcessGroup::hangup()` 是无条件补 SIGCONT 的；分辨它需要每支几千轮
成对抽样，且该函数还被 `Shell::reap_now()` 与 `ProcessScope::kill_all()` 共用，因此本轮没有动。同样没有
被改动的是 `HANGUP_GRACE`：不带 strace 的 910+ 轮死法计数里只出现过 `killed by Hangup` 与 `exited 0`；
带 strace 抓到的那一轮虽然记成 `Killed`，但 SIGKILL 明显到得比 bash 自己的重新举起更晚，它没有抢走转发。

**测试侧那两处不是装饰。** 报告里每一项都被用过一次：`SigIgn`/`SigCgt` 排除「job 免疫」，`ppid=1` 与
`pgid==pid` 说明它已无人可管，`session` 是「扫会话能捞到它」的依据，shell 的死法把改前/改后两次红分
成两条不同的通路。`put_down()` 把一次红的代价从 300 秒压回 5 秒，且只打幸存者**自己**的进程组
（`group > 1 && group != getpgrp()` 才动手），绝不打测试自己所在的组。诊断辅助全部挂在 `#[cfg(test)]`
下，产品路径与 panic 站点基线都不受影响。本地覆盖不到非 Linux 分支：
`cargo check --target x86_64-apple-darwin -p xai-grok-shell-terminal --tests` 红在依赖 `aws-lc-sys` 的 C
交叉编译上（本机无 macOS 工具链），而 CI 的 `platform tests` 在这次 run 里是 `skipped`——前置 job 先红了。

（2026-10-03；`crates/codegen/xai-grok-shell-terminal/src/pty_session.rs`、
`docs/verification/pty-hangup-terminal-eof-2026-10-03.log`）

### 功能：设置面板九个区域第一次由「应答浏览器的那个进程」供值，Safe Web Mode 的拦截清单搬进引擎

设置页此前只有 model 与 Base URL 两个输入框，TODO M3.1 那条「覆盖通用、外观、模型、Provider、权限、
安全、快捷键、远程和更新」因此一直是 `[ ]`。真正的难点不是画九个卡片，而是**面板凭什么说自己是对的**：
版本、绑定地址、状态后端、工作区根、是否要求 token、预览代理放到哪一档、Safe Web Mode 到底拦下哪些操作——
只有服务端进程知道。前端自己抄一份常量，就会在主机其实只拦 18 类的时候继续显示 19 类，而读者会把这句话
当成安全边界来读。所以这一轮全部设计围绕一件事：**面板只能转述应答它的那个进程说的话**。

**协议上新增一次一问一答。** `ClientMessage::GetHostInfo { client_msg_id }` → `ServerMessage::HostInfo
{ info }`，`GetHostInfo` 一并进 `client_msg_id` 去重分支（HTTP 建会话那轮已经证明不去重的重试会让同一件事
发生两次）。`HostInfo` 的 12 个字段里，`SafeModeRefusal { message, capability }` 是成对给出的：既说
「`propose_file_write` 被拒」，也说「你因此少了*把文件写进工作区*」，因为只报前者对用户没有意义。
TypeScript 镜像仍由 `cargo run -p chaos-engine --bin chaos-protocol-schema` 生成，两道门都过：
`check-gui-protocol.sh` 比字节（`GUI protocol types are up to date`），`check-protocol-mirror.py`
解析真实 Rust 枚举做双向核对（`the mirror covers every protocol message and field`）。

**拦截策略从 Web crate 上提进引擎。** Safe Web Mode 的判定原本写在 `xai-grok-web` 里，现在
`safe_mode_tag`（穷尽 33 个变体的 `match`）与 `SAFE_MODE_REFUSALS`（19 条）住在 `chaos-engine`，
Web crate 的 `safe_mode_allows` 只剩一行委托。理由不是洁癖：拦截发生在传输层，而「拦了什么」这句话现在要
被面板引用，两处各写一份必然漂移。两条引擎测试钉住它——一条把 33 个变体各造一条样例消息，要求
`safe_mode_tag()` 的返回值与 serde 真正打出的 wire tag 逐个相同（键写错一个字母，拦截就会静默失配）；
另一条要求清单里每个 tag 都真由某条消息产生，且 13 个只读 tag（含 `get_host_info` 自己）必须放行。

**引擎不许拼装 host 的人谎报。** `with_host_info` 收尾时会**重新推导** `state_backend`、
`workspace_root`、`safe_mode_refusals` 三项，调用方写进去的值会被覆盖。`host_info_flow.rs` 就是照着这三项
撒三个谎（`Sqlite`、`/somewhere/else`、空清单），再断言回来的是引擎的真实值；另一个用例开一个真的
Safe Web Mode socket，把清单里每个 tag 对应的消息真发一遍，要求「清单说有但 socket 放行了」与
「socket 拒了但清单没列」两个集合都为空，另加 `listed.len() >= 15` 防止清单被清空后两轮循环同时空转通过。
第四个用例驱动 `chaos-web` 真正调用的 `serve_loopback_with_assets_and_safe_mode`，核对
`host_info.bind_addr` 就是 socket 实际拿到的地址、`host_version` 就是这个二进制的版本。

**九个区域，`host === null` 时每行都是同一句「host 尚未回报」。** `settings.ts` 的输入是
`{host, theme, model, baseUrl, hasApiKey, platform}`，输出 general/appearance/model/provider/permissions/
security/shortcuts/remote/updates 九个区域；红色告警是判断不是文案（绑定非回环、仅内存后端、未设 token、
预览代理放开任意来源、Safe Web Mode 已开启），拦截清单的条数直接取自清单长度。快捷键区域是**只读**展示
当前真实生效的 10 条绑定（`Mod+1..7` 切面板、`Mod+Shift+L` 换主题、`Mod+.` 取消、`Mod+K` 聚焦输入框），
`aria-keyshortcuts` 用 `Meta`/`Control` 拼写，与屏幕上写的 `Cmd`/`Ctrl` 分别由 `ariaShortcut` 与
`formatShortcut` 产生——两者不同形是故意的，屏幕给人看，ARIA 给读屏软件和 Windows/Mac 差异看。

**axe 抓到的两处都是新结构造成的真问题。** `workspace-flow.pw.ts:256` 每切一个面板跑一次 axe，新设置页
让它红了两个视口：`definition-list`（serious，我在 `<dl>` 里放了一个 `<p>` 说明）与
`scrollable-region-focusable`（serious，拦截清单写了 `max-height + overflow-y: auto` 却没给 `tabindex`，
键盘用户滚不动它）。两条都成立：说明改成第二个 `<dd>`，内层滚动整个去掉（清单只有 19 条，撑不爆面板）。
修法带来的第二次红也一并记下：e2e 的 `rowValue()` 原本取 `locator('dd')`，第二个 `<dd>` 一出现就撞
Playwright strict mode（`resolved to 2 elements`）四例全红——那是测试写法，改成取第一个 `<dd>` 后 8/8 恢复。

**顺带修掉自己埋的一条测试抖动。** `host_info_flow.rs` 里「驱动真实 serve 路径」那条测试要先占一个端口再
释放、再让被测函数去绑同一个端口，因此启动瞬间可能 `ConnectionRefused`。它写着重试循环，但循环包的是
`first_answer()`，而那个函数第一次 connect 被拒就 `unwrap()` panic——**重试形同虚设**。这一轮在本机被
并发负载压出 load 32 时真的红了（`Io(Os { code: 111, kind: ConnectionRefused })`）。现在拆成
`ask()` 返回 `Result`，循环真能重试，超时后把最后一次失败原因与服务任务是否已退出一起打印。非空证明是把
服务改到 `port + 1`：

    the server never answered get_host_info; last attempt: connect to 127.0.0.1:38159:
    IO error: Connection refused (os error 111); serve task already finished: false
    test result: FAILED. 0 passed; 1 failed; ... finished in 20.01s

**非空证明共五处，全部改生产代码、跑真测试、`cmp` 还原。** 清单里 `approve`→`aproove` →
`aproove is listed as refused but no client message carries that tag`；清单条数写成常量 `9` →
`Expected: "2 类操作" / Received: "…下面 9 类操作会被直接拒绝…"`；删掉 shell 里 `theme:cycle` 那一行分发 →
`performs every action the table advertises` 红；`Ctrl/Cmd 只能有一个` 放宽成 `modCount < 1` →
`AssertionError: expected 'run:cancel' to be null`；以及上面那条端口错位。

**测试。** `chaos-engine` + `xai-grok-web` 全量 39 个 target **342 通过 / 0 失败**；
`cargo clippy --all-targets -- -D warnings` 两个 crate 干净；前端 `vitest` **85 通过 / 9 文件**
（新增 `settings.test.ts` 14、`shortcuts.test.ts` 11、`session.test.ts` 4），`tsc --noEmit` 含 `e2e/`
无输出；Playwright `settings-panel` 两视口 **8 通过**，全套 **47 通过 / 0 失败**（同一套在机器被
cargo 整批压满时另有一次 45/2，抖的是 `tool-activity` 与时间线锚定这两条时序类用例，与本轮改动无代码交集，
`docs/verification/settings-panel-2026-10-03.log` 第 9 节把负载数字一并留下，没有把它算作通过）。

**仍未闭合。** 面板**能读不等于能改**：九区里可写的只有主题（写本机 localStorage）、model 与 Base URL，
API Key 那一行只显示 host 侧配没配，凭据存储仍等 M-1 的 keyring 选型；快捷键区域是只读展示，不是可编辑的
按键映射表。`host_info` 证明的是「面板说的就是进程知道的」，至于这些值在真实反代部署里是否正确，属
`web-deployment-tls-linux-2026-10-02.log` 那条线。浏览器证据全部来自 Linux Chromium 的两个视口，
macOS/Windows/Tauri WebView 未参与，axe 也不替代屏幕阅读器实机验收。逐字转录见
`docs/verification/settings-panel-2026-10-03.log`。（2026-10-03；`crates/codegen/chaos-engine/src/lib.rs`、
`crates/codegen/chaos-engine/src/protocol_schema.rs`、`crates/codegen/xai-grok-web/src/lib.rs`、
`crates/codegen/xai-grok-web/tests/host_info_flow.rs`、`apps/chaos-ui/src/{settings,shortcuts,session,main}.ts(x)`、
`apps/chaos-ui/src/style.css`、`apps/chaos-ui/e2e/settings-panel.pw.ts`、`TODO.md`、
`docs/architecture/todo-open-item-classification.md`）

### 门禁：TypeScript 协议镜像检查从「只能在 CI 跑」搬进本地 Docker 全轮

`scripts/ci/check-gui-protocol.sh` 用 `cargo run --bin chaos-protocol-schema` 重新生成
`apps/chaos-ui/src/generated/protocol.ts` 再逐字节比对，此前被登记在
`scripts/ci/docker-entry-ci-only.tsv` 里，登记理由是「本地入口的 cargo 门只到 check/clippy，这道门需要一次
dev 构建」。这条理由在 `--full` 下已经站不住：`--full` 本来就要跑 `cargo test --workspace --locked
--no-fail-fast`，那道门跑完时 `chaos-protocol-schema` 早就在 target 卷里了，镜像检查只剩一次 `cmp`。现在它
被追加在 `cargo test` 之后（顺序是有意的：先让 workspace 构建把二进制焐热），quick 模式不带它，因此
「quick 为什么 quick」没有被牺牲。`scripts/ci/docker-entry-ci-only.tsv` 相应少一行，分类从
`38 run / 5 CI-only` 变成 **`39 run / 4 CI-only`**（`check-guard-wiring: OK (44 files in scripts/ci/,
43 reachable, 39 run by scripts/verify-in-docker.sh, 4 recorded CI-only, 1 exempt)`）。

非空证明是把新加的那一行删掉：真实仓库的两条用例同时红，且点名的正是这道门——

      AssertionError: 1 != 0 :   scripts/ci/check-gui-protocol.sh runs in CI but not in scripts/verify-in-docker.sh;
      mirror it into the `gates` array or record what it needs in scripts/ci/docker-entry-ci-only.tsv

还原后 16 条用例全绿、`bash -n` 通过、文件按字节一致。这条也顺手说明 `docker-entry-ci-only.tsv` 不是
一次性登记：`check-guard-wiring.py` 对「清单里有、入口其实跑了」和「入口没跑、清单也没写」两个方向都表态，
所以搬门必须同时改两处，改一处就会红。

### 修复：Windows 平台腿第一次跑到测试，45m15s 里有 16m43s 卡在一个字符串上

`platform tests (windows-latest)` 此前的三种死法（解析、构建、撞作业上限）都在测试之前。上限抬到 75
分钟之后，run `37074944913` / job `111075816970` 第一次真的执行了测试，也把「Windows 上读不到失败
原文」这件事的原因一并暴露出来：

    23:48:37      步骤 9 起步：cargo test --locked --no-fail-fast
    00:14:20.97   第一条 `test result:`（编译花了 25m43s）
    00:14:24.04   xai_grok_tools-c0c13642df1bb12b.exe 起，running 3087 tests
    00:15:45.28   test ...cwd_and_worktree_isolation_are_mutually_exclusive has been running
                  for over 60 seconds
    00:15:59.27   任何线程的最后一条判决
    00:32:42.02   ##[error]The operation was canceled.

2960 ok / 124 FAILED / 2 ignored，加上**永不返回的那 1 条**正好是 3087。libtest 是逐条打印判决、
却要等全部线程返回才打印 `test result:` 汇总的，所以一个挂住的测试顺手抹掉了整个二进制的汇总和
`failures:` 段——那 124 条失败在全日志里一句断言原文都没有（该二进制输出中 `grep -c 'panicked at'`
是 0）。「没有原文」是被直接观察到的缺失，机制本身则按 libtest 的打印顺序推得。

**挂起点是一行存在性检查。** `TaskTool::run` 判断 `cwd` 与 `isolation="worktree"` 是否互斥，只看
`is_some_and(|p| std::path::Path::new(p).is_dir())`：

    let cwd = if cwd.is_some() && input.isolation == Some(SubagentIsolationMode::Worktree) {
        if cwd.as_deref().is_some_and(|p| std::path::Path::new(p).is_dir()) {
            return Err(ToolError::invalid_arguments("cwd and isolation=\"worktree\" ..."));
        }
        None                     // 路径不存在：清掉 cwd，worktree 赢
    } else { cwd };

测试写 `cwd: Some("/tmp".into())`，想说的是「某个已存在的目录」。Linux/macOS 上 `/tmp` 在，走「拒绝」
分支，毫秒返回；`windows-latest` 没有 `C:\tmp`，于是走「清掉 cwd 去 spawn 子代理」分支——而这类测试
**从不读自己的响应接收端 `rx`**（它预期在校验阶段就被拒），`Tool::run` 从此等待一个永远不会发出的响应。
挂起在盒子外面看，和机器慢完全一样。

**非空证明改的是生产分支，不是测试。** 把条件换成 `is_some_and(|_p| false)`，即让生产代码走 Windows
实际走过的那条路，本机该测试立刻红成理论预测的样子：

    cwd + worktree must be rejected by validation, not wait on a subagent: Elapsed(())
    test result: FAILED. 0 passed; 1 failed; ... finished in 30.00s

还原后 `1 passed ... finished in 0.00s`，且还原文件与变异前副本 `cmp` 逐字节一致。那 30 秒是本轮新加的
`VALIDATION_TIMEOUT`：两条校验测试的调用现在包在 `tokio::time::timeout(..)` 里，将来再误入 spawn 分支会
**带着自己的名字失败**，而不是吃掉整个作业预算。

**同一个字符串还放倒了三条，而它们的形态反过来证实了机制。** `cwd_strips_stray_leading_quote`、
`cwd_threads_to_request`、`cwd_with_isolation_none_is_allowed` 在 Windows 上是 FAILED 不是挂起——它们
断言「spawn 出去的请求里 cwd 是什么」，而那个 cwd 已被静默清掉；同模块用 `/nonexistent/...` 与
`/tmp/some-dir` + `resume_from` 的几条两端都绿。**危险从来不是 `/tmp` 这个名字，而是夹具依赖「存在」
这件事。** 修法因此不是把 `/tmp` 换成 `/var/tmp`，而是不再把「已存在的目录」写成字面量：`xai-grok-tools`
测试模块加 `existing_dir()`（`std::env::temp_dir().display().to_string()`），5 处 `cwd:` 夹具改用它；
同族夹具另在 8 处替换；`computer/local/terminal.rs` 那 14 处（就是 54 条失败那一族）改走 `shell_path()`
与 `pwd_reports_dir()`，断言仍然校验真实的 `pwd` 输出，而不是一个路径字符串。

另外两条**有原文可读**的 Windows 失败同轮修掉：`test_kill_returns_signal` 断言
`Some("signal 9")`，而 Windows 的 kill 不上报信号号（现按平台分别断言）；pager-bin 的
`corrupt_config_never_changes_update_outcome` 报 `os error 10106`（Winsock 不可用），因为测试把 updater
初始化所需的系统环境变量一起剥掉了，现由 `platform_essentials()` 透传。

**结构改动：一条挂起的测试不该有资格抹掉另外五个 crate 的结论。** 那条步骤原本一次跑六个 crate；现拆成
`cargo test (target-OS crates: except xai-grok-tools)` 与 `cargo test (target-OS crates: xai-grok-tools)`，
各自 `timeout-minutes: 35`、第二条带 `if: always()`，作业总预算仍是 75 分钟。124 条里除去 54 + 3 之后的
约 67 条（lsp 38、skill_discovery 7、`resolve_model_path` 5、read_file 5、bash 3、skills 2，其余各 1）
**没有断言原文，本轮也不给它们安根因**；分组列出来只为下一次 Windows 日志有一个可比对的已知集合。
本轮没有任何 Windows 机器参与：数字读自别的机器执行的运行日志，变异证明在本机对同一生产分支做的。
逐条时间线、分组明细与「本节不能证明什么」见 `docs/verification/platform-ci-2026-10-02.log` 的 `== 5.` 节。

### 新增门禁：workflow 得先证明自己是合法 YAML，才有资格决定今天跑哪些 job

上面那次拆分差点让整条 CI 静默消失。新步骤名写成

    - name: cargo test (target-OS crates: xai-grok-tools)

未加引号的 `": "` 是 YAML 的映射分隔符，PyYAML 在 `ci.yml` 第 293 行直接拒绝整个文件。**解析不过的
workflow 不跑任何 job**——比一条测试失败严重得多，因为它看起来像「今天还没跑」。更糟的是当时两个已有
守卫都报 OK：`check-workflow-shells.py` 与 `check-workflow-toolchain.py` 都用正则抠 `run:` 块，从不问
这份文档是不是一个合法的映射。

`scripts/ci/check-workflow-yaml.py` 补的正是这一层。它自己跟踪块标量状态（`run: |`、`run: >-` 之后的
更深缩进行不参与判断），按 YAML 的规则剥注释（引号内的 `#` 不是注释，引号外的还要求前置空格），再对
剩下的纯标量检查 `": "` 与结尾的 `:`。它**刻意不带引号感知**——这不是偷懒：`run: echo "a: b"` 在 YAML 里
同样是纯标量、同样会被解析器拒绝，加了引号感知就会把真缺陷判成 OK。12 条夹具各自带期望退出码，另有一条
夹具把 12 份样本逐个交给真实 `yaml.safe_load` 交叉核对裁决（环境没有 PyYAML 时打印说明并跳过），再加两条
块标量终止条件（块标量结束后第一行回归普通键值行必须正常判定）。两个守卫在破损文件上同时报 OK 这个事实，
本身也说明「regex 抽取」类检查不能互为替代，故两者都保留。已接入 `.github/workflows/ci.yml` 的
`workflows-present` 与 `scripts/verify-in-docker.sh` 的 `workflow yaml` 门禁，由 `check-guard-wiring.py`
双向锁定。

### 改进：`check-spawn-cwd-portability.py` 的两个盲区，其中一个本该拦住上面那次挂起

守卫早已存在，却对上面这件事完全无感，原因有两条。

**一，它只认 `working_directory` 字段和 `.current_dir()` 调用，不认 `TaskToolInput` 的 `cwd:` 字段**——
而这次把 Windows 平台腿钉住的字符串就写在 `cwd:` 里。新增第三个 sink 后，如何避免把合法夹具一并判红成了
主要问题：`cwd: Some("/nonexistent/does-not-exist")` 是**必要**的夹具（它测的正是「路径不存在」这条分支），
任何主机都不存在的路径不该管。判据因此是「顶级目录是否只存在于 POSIX 主机」——`tmp`/`var`/`usr`/`home`/
`private`/`Users`/`Library` 等一组根名；`/old`、`/new/dir` 这类哨兵照常放行。

**二，它取 crate 列表的方式会静默少拿一个 crate，而少拿的那个恰好是守卫最需要看的。** 旧正则要求 `-p`
前面是行首或续行反斜杠，于是单行写法的 `cargo test … -p xai-grok-tools` 被整体漏过：守卫扫 5 个 crate、
报「全绿」，而第 6 个从未被看过。现改为**并集**平台作业里所有名字匹配 `cargo test (target-OS crates` 的
步骤——这也是上面那次步骤拆分不会把某个 crate 悄悄排除在扫描之外的原因——并在四种情况下 fail closed：
没有 `platform-tests:` 作业、没有任何匹配步骤、crate 列表为空、平台上存在一条带 `-p` 的 `cargo test`
步骤而该前缀覆盖不到它（错误消息直接点名那个步骤）。解析器本轮踩到的三个坑各有夹具钉住：作业结束条件不能
写成 `^  \S`（下一个 job 之前常有两空格缩进的注释块，会被当成作业定义结束）、列表项缩进是 `steps:` 的
缩进 + 2、`-p` 的左边界必须允许空格。夹具从 14 条增至 19 条，新增的 5 条分别证明「拆开的两步都会被扫到
（并且那一步独有的 crate 真的被扫过）」「改名平台作业必须 fail closed」「步骤前缀漏掉一条 `cargo test -p`
必须 fail closed」「`cwd:` 写真实目录必须红」「`cwd:` 写任何主机都没有的路径必须绿」。

### 修复：`git::safety` 那条偶发红了一百多轮都说不出原因——原因被挡在两层之外，其中一层是本仓库自己写的 `{:#}`

`git::safety::tests::gate::snapshot_under_a_foreign_clean_filter`（父测试
`git_configuration_in_the_environment_does_not_reach_the_snapshot` 用 `--exact --ignored
--test-threads=1` 重新执行自己的测试二进制来跑它）在整仓 lib 测试里每隔几十轮红一次，报错只有一个
verdict 枚举值：`left: Keep(CheckFailed)` / `right: Delete`。`CheckFailed` 是 `decide_safety` 四条路共同
的兜底答案，每条只写一行 `tracing::warn!`，而测试二进制里**没装任何 subscriber**，四行 warn 全被丢弃。
装一个只给子测试用的 WARN 订阅者 `log_child_diagnostics()`（`with_test_writer` 平时静默；`run_child`
本来就会把子进程 stderr 拼进父测试的 panic，不需要新管道），同机同命令 20 轮里第 10 轮就抓到原因：

    WARN xai_fast_worktree::git::safety: path did not open as a git repository path=/tmp/.tmpmCoMtO/inherits-a-filter error="/tmp/.tmpmCoMtO/inherits-a-filter" does not appear to be a git repository reason=CheckFailed

范围从「四条路」收到一条：`gix::open` 失败，而 `.git` 当时存在（否则 reason 会是 `NoRepo`）。

**第二层遮蔽才是本轮要记的：本轮早先的改动把 warn 从 `%error` 换成 `%format!("{error:#}")`，
以为是打印错误链，实际是个空操作。** `gix-0.83.0/src/open/mod.rs` 的
`#[error("\"{path}\" does not appear to be a git repository")]` 模板里根本没有 `{source}`，thiserror 的
`Alternate` 走同一个模板；`gix-discover-0.51.0/src/lib.rs:47` 那 11 个 `is_git::Error` 变体（其中 5 个带
`std::io::Error`）全被压成同一句话。换成手走 `source()` 的 `open_error_chain()` 之后，再加一条能区分
两者的测试：它构造 `NotARepository → MissingCommonDir → io::Error(EMFILE)` 这条**三层**链，断言日志里
既有「哪个文件」也有「哪个 errno」。三条变异跑过（跑完 `cmp` 字节还原）：函数体换回
`format!("{error:#}")` → 红，报错尾部只有外层那一句；`push_str(&current.to_string())` 换成
`let _ = current;`（遍历但不打印）→ 红，尾部是 `…: : `，说明链被走完两层只是没印；另两条改法编译不过
（E0282/E0596），不算变异。还原后 `git::safety` 全绿（当时计数 49 passed / 0 failed / 2 ignored）。

**根因**：带错误链的二进制按原复现命令跑到第 26、94 轮各红一次，链的第二层是
`is_git::Error::CurrentDir`、第三层是 `ENOENT`。`gix-discover-0.51.0/src/is.rs:36` 在 `is_git()` 的
**开头**无条件执行 `gix_fs::current_dir(false)`，与传进去的路径是绝对还是相对无关。所以红的那一刻
要说的不是「这个目录不是仓库」，而是**调用方的当前目录已经被删掉了**；`gix::open` 把它包装成
`NotARepository`，安全门于是回答 `CheckFailed`。`run_child` 重新执行测试二进制时不指定 `current_dir`，
子进程于是继承了那个已经不存在的 cwd——把子进程钉到 `temp_dir()` 即修好，并加一条永久测试
`the_child_does_not_depend_on_the_cwd_it_inherits`。「删掉自己当前目录」这个动作放在一个只跑它自己的
子进程里：第一版写在父进程，它确实红了，但同时把同模块另外 32 条测试一起弄红（`17 passed; 33 failed`），
那是用制造缺陷的方式测试缺陷。两条变异（跑完 `cmp` 字节还原）：去掉钉住的那一行 → **逐字复现历史
偶发**（同测试名、同 verdict 对、同三层链）；让那个临时目录不被删除 → 红在「cwd 确实已被 unlink」
这条前置断言上，说明前提本身是活的。还原后 `git::safety` **50 passed / 0 failed / 3 ignored**，默认与
`--features metadata` 两种配置相同。本行交付的不是「不再红」：**谁删掉了那个目录**仍然匿名，另一种
「工作树路径自己消失」的形状（链里只有裸 `ENOENT`）本轮也没解释，两者都记在证据文件第 5.3 节作为待查，
不写成结论；该测试对 `CheckFailed` 的严格断言刻意不放宽（放宽会连带放过 filter 泄漏）。
钉住之后再跑同样的 120 轮整仓：**0 红**（`flaky10/`）；但同一份证据文件算过，即便修法完全无效，
120 轮全绿的概率也在 40% 上下，所以这个 0 只能读成「没有出现更糟的形状」，不能读成归零。
（2026-10-03；`crates/codegen/xai-fast-worktree/src/git/safety.rs`、`src/git/safety_tests/gate.rs`、
`docs/verification/fast-worktree-safety-gate-flake-2026-10-03.log`）

### 新增门禁：谁能改整个进程的当前目录，改完谁负责放回去——这条只能静态查

上一条那个偶发的形状决定了运行时抓不到它：报错的测试永远不是闯祸的测试，而单跑那条测试永远绿。
新增 `scripts/ci/cwd-change-census.py`，把「改进程级 cwd 的每一处 + 它的恢复动作」变成一张表加四条失败
形状：新调用点不在清单里、清单里的行漂了、**测试位置的 chdir 前面没绑 `CwdGuard`/`RestoreCwd`（结构规则，
不受清单支配）**、role 或 guard 列与实测不符／理由列是空的。实测 7 处（2 产品 5 测试）。角色判定不是数
属性：`git/safety_tests/gate.rs` 一个属性都没有，只有解析 `git/safety.rs` 里
`#[cfg(test)] #[path = "safety_tests.rs"]` 这条模块声明才能认出它只进测试构建——判错就会把那条故意删除
自己 cwd 的测试当成产品代码，而产品代码不要求恢复。**真实树 5 个变异逐个红、逐个还原**（抽掉 guard 绑定／
把危险站点声明成 product／新加一条无 guard 的测试 chdir／理由列写 `TODO`／反方向只写进注释必须不多报），
夹具 14 条全绿。夹具当场抓出扫描器自己的两个缺陷：普通字符串的终止符写成查找 `""`，第一个字符串起把整个
文件后半抹平（站点表变成空表）；`#[path = "..."]` 里的文件名被自家抹除器抹掉——真实仓库被「父目录名里带
tests」这条捷径救了一次，是把目录名捷径关掉只留模块图才暴露。扫描范围是仓库根全部 `.rs`，因为工作区成员
含 `crates/` 之外的 `prod/mc/cli-chat-proxy-types` 与四个 `third_party/*`。门禁只用标准库（3 秒），
接线由 `check-guard-wiring.py` 双向把关。它不查「谁删了目录」，也不证明那 7 处行为正确——那两条待查仍在
上一条评论里挂着。（2026-10-03；`scripts/ci/cwd-change-census.py`、`scripts/ci/cwd-change-baseline.tsv`、
`scripts/ci/test-cwd-change-census.py`、`.github/workflows/ci.yml`、`scripts/verify-in-docker.sh`、
`docs/verification/cwd-change-census-2026-10-03.log`）

### 修复：nfs「daemon 已死就本地拷贝兜底」那条测试每隔几十轮红一次——红因是夹具自己把判决时刻放进了 30 ms 的校时误差里

`nfs::client::tests::timeout_dead_daemon_unmounted_dest_is_fallback_without_second_create` 在整仓
lib 测试 120 轮里红 2 次（第 43、50 轮），两次逐字相同：

    called `Result::unwrap()` on an `Err` value: InFlight { phase: "unknown" }

产品没有判错，是**夹具让被测判决去赌调度器**。夹具 `lost_create_script()` 让 mock daemon 先睡
`create_hold = 300ms` 才 drop flock、unlink sock；客户端那侧的判决时刻 ≈ `create_timeout`(80ms) +
`query_phase` 的 socket 下限 `QUERY_PHASE_MIN_TIMEOUT`(250ms) ≈ 330ms。两者只差 30ms，满载机器上
线程从睡眠里醒过来的延迟轻易超过它，于是 `is_provably_dead()`（sock 不存在或 ping 不通 **且** flock
已释放）在判决那一刻看到的仍然是「锁被持有」，客户端回答 `InFlight`。改成 `create_hold:
Duration::ZERO`：mock 先 drop flock、先 unlink sock、再关掉连接，而客户端那次阻塞读**正是因为**连接
被关掉才返回——被探测的两个状态因此在探测开始之前就已成立，不再与墙钟赛跑。产品代码一行未动，
`die_after_create` 仍是无回包的丢失响应，被断言的仍是 `poll_after_lost_reply → deadline_decision`
这条真实判决路径。

**因果配对方**：只把这一行改回 `300ms`、其余一行不动，重编一个二进制，用同一条整仓命令再跑 120 轮
→ 第 79 轮红，报错与历史偶发**逐字相同**；含修复的那份在同样 120 轮里 `nfs::client` 0 红。红过 /
没红过是这一对的结论，1/120 与 0/120 的**速率**差不作为任何结论——按上面的算术这是「硬币贴着边立不
立得住」的问题，样本量撑不起更细的说法。测量诚实性也写在证据文件里：**单条测试单线程 300 次、整模块
24 个 CPU spinner 200 轮（而且是带着缺陷重编的对照二进制）都是 0 红**，所以这些形状不能用来证明修复，
只有整仓 `--lib` + `--test-threads=$(nproc)` 复现得起来。

**顺带补上两条此前无人看管的产品行为**：`deadline_decision` 一直会在 daemon 确定已死且 dest 不是
挂载点时清掉**空**的残留目录（daemon 写第一条 journal 之前就会 mkdir dest，而 `git worktree add`
拒绝已存在的目录，不清掉兜底直接失败），并且**拒绝**在**非空**残留上拷贝兜底（`InFlight { phase:
"dest-exists" }`——那可能是死掉 daemon 正在写的半成品投影，既不能删也不能盖）。改造前兜底那条测试
只看判决、压根不看 dest，两条都没有测试。现在加 `assert!(!dest.exists(), …)` 与新测试
`timeout_dead_daemon_nonempty_leftover_dest_refuses_fallback`（种 `dest/partial="not mine"`，断言
phase **等于** `dest-exists` 而不是泛化的 `unknown`、`creates == 1`、字节未被触碰）。**变异矩阵 3 行**
（跑完 `cmp` 字节还原）：N1 `is_provably_dead()` 取反 → 两条都红，且兜底那条的报错与历史偶发逐字相同
（这同时反证历史偶发走的就是这条路径）；N2 去掉空残留的 `remove_dir` → 只有 dest 断言红；N3 `if empty`
改成 `if true` → 只有拒绝那条红。
（2026-10-03；`crates/codegen/xai-fast-worktree/src/nfs/client.rs`、
`docs/verification/fast-worktree-nfs-dead-daemon-race-2026-10-03.log`）

### 修复：Windows 平台腿每次都死在自己的作业超时上，而 CI 给整条 run 的结论只写着 `cancelled`

`platform tests (windows-latest)` 在 `f585f59d` 上把 45 分钟预算全花在第 9 步
`cargo test (target-OS crates)`（前 8 步全绿，第 9 步在 45m15s 被取消），同一条 run 的 macOS 腿
31m17s 跑完，其余 8 个作业全绿。作业超时被 GitHub 记成该作业 `cancelled`，于是整条 run 的总结论也是
`cancelled`——看上去和被并发取消是同一个形状，实际不是。`timeout-minutes: 45` 是 `da5e1ee0` 写这条腿时
猜的，从来没跟一次真实运行对过；现在改成 75，两条实测数字写在设置上方。同一轮把两类静因分开记清：
main 上那五条 `cancelled` 里，`gh api .../runs/<id>/jobs` 返回的作业数是 `0`——那是并发组丢弃了**排在
队列里**的运行（`cancel-in-progress: false` 只保护已经在跑的运行），与超时无关，代价是那三个 commit
一条平台证据都没拿到，处置是把可交付切片攒成一批再推。证据（run／job／step 三个层级的 API 原文）见
`docs/verification/platform-ci-2026-10-02.log` 的 2026-10-03 一节。这条**不是**「Windows 测试通过」的
证据：它只说明这一步第一次拿到一个可能跑得完的预算，此前连续五次 head 的 Windows 失败都在我们的工具链
上而不是产品上。
（2026-10-03；`.github/workflows/ci.yml`、`docs/verification/platform-ci-2026-10-02.log`）

### 新增门禁：提交进仓库的文档不允许把证据放在只存在于一次会话里的目录

`docs/` 下的证据文档是给人复查的，但审计发现有 40 处指向只存在于一次会话里的目录：一类写「详见会话
scratch」，另一类直接给出会话 scratch 根目录下的绝对路径。那些目录随会话结束删除，读者照着找不到任何
东西，文档等于自己宣布不可复核。新增 `scripts/ci/check-evidence-paths.py`（+ 16 条自测）：`os.walk` 跳过
`.git`/`target`/`node_modules`/`dist`/`build`（首版用 `rglob("*")` 会走进 `target/`，一次扫描挂住不返回），
扫全部 `*.md` 加 `docs/` 下的 `*.tsv`——**生成的** tsv 也扫，`scripts/ci/*.tsv` 与
`docs/verification/*.log` 不扫（日志里本来就要抄命令）。四条模式逐条写在
`scripts/ci/check-evidence-paths.py` 的 `PATTERNS` 里（中英文两种「scratch 是证据所在」的说法、把
scratch 根目录写成绝对路径的前缀、以及两种把会话目录称作 scratch 的英文写法），这里**不复述字面量**：
本条改动第一次提交时，门禁就在这一段描述里红了三处，因为它对「描述规则」和「使用规则」不加区分——
这条自我命中本身就是它不是空转的最直接证据。今天 275 个文件 0 命中。唯一豁免是一条
**具名**条目（`xai-grok-shell` 里随产品发布的 prompt 模板，它教模型怎么用 scratch）；豁免条目一旦从
扫描集里消失，门禁自己红，防止有人删了规则却留着豁免。

40 处指针分两遍清：第一遍脚本替换留下的句子读不通、还漏了一处绝对路径，第二遍逐条手写。
变异矩阵 6 行里 **M5/M6 两行是绿的**——「把 `target/` 也走一遍」与「删掉中文那条模式」在今天的树上
都不改变结果，只有夹具能证明它们被覆盖，这两行如实记着，不写成红。门禁同时进
`.github/workflows/ci.yml` 与 `scripts/verify-in-docker.sh` 的 `gates=()`，由 `check-guard-wiring.py`
双向钉住（漏镜像或留悬空调用点都会红）。
（2026-10-03；`scripts/ci/check-evidence-paths.py`、`scripts/ci/test-check-evidence-paths.py`、
`.github/workflows/ci.yml`、`scripts/verify-in-docker.sh`）


### 新增：浏览器第一次真的把文件当附件上传进 workspace；顺带 e2e 抓出一处会吞掉错误的状态写法

协议与主机侧早就完备（`crates/codegen/chaos-engine/tests/attachment_protocol.rs`、Web 主机的策略测试），
缺的是浏览器这一侧**根本没有客户端**：协议类型还漏着 6 条消息，UI 里也没有任何入口。补上之后，一次上传
是 `validate_attachment` → `begin_attachment` → `attachment_chunk`×N → `finalize_attachment` → 审批
`workspace.attach_attachment` → `file_changed` + `attachment_completed`，写入这一步隔着一次真实审批。

**真正卡人的是帧上限，不是文件大小。** Web 主机每帧硬上限
`crates/codegen/xai-grok-web/src/lib.rs:25`（`MAX_REQUEST_BYTES = 64 * 1024`），超了回
`message_too_large`，而 base64 又把载荷放大 4/3，所以每片装多少字节是算出来的：
47 KiB 原文 → 64,172 个 base64 字符 + 144 字节 JSON 外框 = 64,316 < 65,536；48 KiB → 65,680 就超。
`UPLOAD_CHUNK_BYTES` 因此从**两侧**被测试夹住（写大了失败，写小了浪费带宽）。早期写的是 32 KiB，理由
「肯定装得下」——那次 `chunkFrameBytes(32768 + 1024)` 报 45,200，离上限还远，说明那条断言压根没证明上限
是约束所在。10 MiB 上限的整份文件切成 218 片后，同样断言每片都非空、每片都装得下、且切片覆盖每一字节
恰好一次；`MAX_ATTACHMENT_BYTES` 与 `UPLOAD_CHUNK_BYTES` 由测试**回读 Rust 源文件**核对，避免客户端以为的
上限与主机执行的上限各自漂移。

**e2e 抓到的缺陷不在上传里，在状态写入里。** 第 3 例红在状态行根本不存在，即 `session.upload` 是
`undefined`。根因：`ws.onmessage` 先推进 `sessionStateRef.current` 再 `setSession`，而点击上传用的是 React
的函数式更新——函数式更新只改 React 状态，那个 ref 要等下一次渲染后的 effect 才跟上。主机最早的回复
（`attachment_rejected`）正好落在这个窗口里，于是它被应用在一个**没有 `upload` 的旧状态**上，失败分支根本不
执行，而它算出的 `next` 又把排队的 `validating` 覆盖掉。用户看到的是徽标刷成一行通用错误、面板忘记这次
上传——一次静默失灵，而 `submit()` 那类「状态不完整就 early-return」的入口有同样的形状。修法不是给上传打
补丁，而是消掉两份真相：新增 `updateSession()`，一次调用里同时推进 ref 与 React 状态，19 处函数式写法 +
1 处 `(c) =>` 写法全部改过去，并留 `apps/chaos-ui/src/session-wiring.test.ts` 一条结构钉子（读 `main.tsx`
源码断言不再有任何 `setSession((`；把任意一处改回去它当场红，还原后 `cmp` 字节一致）。

**主机侧新测试**：`websocket_upload_lands_file_bytes_in_the_workspace_after_approval` 先发一片**故意**超过
64 KiB 的帧（原文 96,257 字节 → base64 128,344 字符）断言 `message_too_large`，再按 47 KiB 切三片
（48,128 + 48,128 + 3,744 = 100,000）让累计 `received` 等于 100,000，审批后 `fs::read` 逐字节比对、并断言
`.chaos-staging` 不残留分片——上限于是第一次被真实触发过，且证明主机拒的是**那一帧**不是整段上传。
`safe_web_mode_refuses_every_step_of_an_attachment_upload` 先证明安全模式下 `create_session` 仍成功（否则
「被拒」只是因为压根没有会话），再逐一断言五个步骤各回 `safe_web_mode_blocked`。同轮删掉
`safe_mode_allows` 里的 `ClientMessage::FinalizeAttachment`：它不是更宽的口子而是**永远走不到的死条目**
（安全模式下 `begin_attachment` 已被拒，客户端拿不到 `upload_id`），留着反而让人以为安全模式对上传开了洞。

**验证**：真实 Playwright 在 desktop 1440×1000 与 mobile 390×844 各跑 3 例共 6 条全绿（`setInputFiles` 真的
塞文件、审批点「允许」、状态行逐字等于 `附件已写入 nested/<name>.txt（122880 字节）`、磁盘逐字节比对、再换
workspace 内容搜索与编辑器这**第二条代码路径**复读一次；取消例等到「正在上传」才点取消并在 300 ms 后复断
文件仍不存在；`.exe` 例证明客户端不轻信自己的校验——本地不做扩展名白名单，主机的 `attachment_rejected`
被如实呈现）。plumbing 影响所有会话写入，故整套 e2e 重跑 **39 passed / 53.2s**（含真把 `chaos-web` 杀掉重启
的 `reconnect-snapshot`、第二个标签页不能替当前页审批的 `approval-competition`、两条 axe 无障碍用例），无一
转红；`npx vitest run` **56 passed**、`npm run typecheck` 干净、`cargo test -p xai-grok-web --test
local_policy_flow --test safe_mode_flow` 3 + 2 全绿。非空洞性：把 `pumpUpload` 改成只发第一片，第 1 例当场
转红（字节数停在第一片的累计值）而另两例仍绿，还原后 `cmp` 字节一致。全记录见
`docs/verification/web-attachment-upload-2026-10-03.log`。（2026-10-03；`apps/chaos-ui/src/attachments.ts`、
`apps/chaos-ui/src/attachments.test.ts`、`src/session.ts`、`src/session.test.ts`、`src/main.tsx`、
`src/session-wiring.test.ts`、`e2e/attachment-upload.pw.ts`、`playwright.config.ts`、
`crates/codegen/xai-grok-web/src/lib.rs`、`tests/local_policy_flow.rs`、`tests/safe_mode_flow.rs`、
`docs/verification/web-attachment-upload-2026-10-03.log`）

**提交前复跑抓到一条真偶发红，红的是那条断言自己的形状。** 上面「取消例等到『正在上传』才点取消」用的
是 `toContainText('正在上传')`，而 `正在上传` 是瞬态：取消例的文件只有 4 KiB，正好一片，`begin_attachment`
回来后一片就发完，状态行在下一次轮询之前已经走到 `附件已传完，等待审批写入：…`。`toContainText` 看的是
**当下**的 DOM，没有「出现过」这个概念——于是这条测试实际在测 loopback 有多快，实测 6 轮红 2 轮（两次都是
mobile 那一遍）。改法与 `reconnect-snapshot` 同一招：`addInitScript` 挂 MutationObserver 把状态行显示过的
每个文本记进 `window.__chaosUploadSeen`，断言改为 `expect.poll` 轮询**这份记录**；取消的点击仍排在这条等待
之后，所以「取消的是真传输」这个前提没被削弱。把观察者监听的 testid 改错，两个视口都红在
`上传状态里从未出现过「正在上传」`（跑完写回、`cmp` 字节一致）；记录形式连跑 5 轮 30 条全绿，轮询形式同机
连跑 5 轮红 1 轮。见同一日志第 9 节。

### 新增门禁：`check-gui-protocol.sh` 证明「文件重新生成过」，证明不了「镜像没漏写」——事实上门户漏了 6 条

浏览器那份协议类型来自 `crates/codegen/chaos-engine/src/protocol_schema.rs` 里手写的
`pub const TYPESCRIPT`，导出成 `apps/chaos-ui/src/generated/protocol.ts`。已有的
`scripts/ci/check-gui-protocol.sh` 把导出结果与那段字符串比对，两个输入是**同一段手写文本**，所以它证明的
仅是「文件被重新生成过」；镜像本身漏写了什么，它结构上无从得知。附件上传就是漏的——客户端要发
`begin_attachment`/`attachment_chunk`/`cancel_attachment`、主机要回 `attachment_started`/
`attachment_progress`/`attachment_cancelled`，浏览器类型里一条都没有，而引擎那一侧有一整组协议测试在发这些
消息。把镜像退回漏写前那 6 行再跑新守卫，得到 `ClientMessage: 29 of 32 / ServerMessage: 37 of 40` 加逐条
点名；**同一次运行里旧检查照样打印 `GUI protocol types are up to date`**。

`scripts/ci/check-protocol-mirror.py` 解析真的 Rust 枚举（花括号配平取 body、变体→字段名集合、复刻 heck 的
`rename_all = "snake_case"` 所以 `HTTPStatus` → `http_status`），与镜像里两个 union **双向**比对：缺 tag、多
tag、每条消息缺字段/多字段、重复 tag、带 tag 条目写在 union 之外、两个变体在 snake_case 下塌成同一个 tag、
union 整个缺失。字段**类型**刻意不在范围内——`UUID` 对 `Uuid` 是另一个量级的翻译问题，且类型不符会在
反序列化处当场失败，不会静默；会静默的只有「有这条消息」与「有这些字段」，范围就收在这里。

两处解析细节是写夹具时才逼出来的，都有测试钉住：字段名只认行首声明**且冒号不能是路径分隔符**（`(?!:)`），
因为 rustfmt 会把放不下的泛型换行，`serde_json::Value,` 也是「小写词 + 冒号」开头，早先的版本据此凭空造出
一个字段 `serde_json` 并报「镜像缺字段 serde_json」；`//` 也要跳过，设计稿里抄一份 `pub enum ClientMessage`
是常见写法，读注释那份会让守卫在真枚举缺消息时仍然绿。夹具 **26 条**，最后一组是关键——它**从真镜像里逐条
删掉那 6 条中的每一条**并要求守卫变红且点名那条，证明守卫读的是 CI 真正检查的文件而非自己的夹具。**17 个
变异体 16 个被抓**，唯一存活的 `field-anywhere-in-line`（`re.match` → `re.search`）是有据可查的等价变异：
在「守卫会看到的输入」这个集合上两者行为相同，而它第一轮同样存活时**没有** `(?!:)`，那时换行类型真的会造出
幽灵字段——那个真实缺陷现由 `test_a_wrapped_type_line_does_not_become_a_wire_field` 钉住。
`comments-not-stripped` 第一轮也存活，补了「注释里抄了一份枚举」这条双向夹具（镜像完好必须绿 + 真 tag 缺失
仍要点名）之后被抓。还原全部由 `cmp` 逐字节确认，pristine/after sha256 同为 `114f154f4930…f5af`。接线两个
入口都有：`ci.yml` GUI job 的「Protocol mirror coverage」步骤与 `scripts/verify-in-docker.sh` 同名 gate
（`check-guard-wiring.py` 会拒绝写了没人跑的守卫）。全记录含 17 行变异锚点表见
`docs/verification/protocol-mirror-coverage-2026-10-03.log`。（2026-10-03；
`crates/codegen/chaos-engine/src/protocol_schema.rs`、`apps/chaos-ui/src/generated/protocol.ts`、
`scripts/ci/check-protocol-mirror.py`、`scripts/ci/test-check-protocol-mirror.py`、`.github/workflows/ci.yml`、
`scripts/verify-in-docker.sh`、`docs/verification/protocol-mirror-coverage-2026-10-03.log`）

### 修复：`xai-fast-worktree` 的测试会扫描开发者真实的 grove 目录，`auto_gc` 因此随机红

`verify-in-docker.sh --full` 的 `cargo test` 门禁在容器里报
`auto_gc::tests::rebuild_same_pass_does_not_age_expire_new_registration` 失败于 `auto_gc.rs:1804`
（`503 passed; 1 failed`），本地测量复现率 3/20 与 5/34。单跑该模块全绿——把「不稳定的测试」当输入而不是
结论，才看见真正坏的是隔离：重建那一路会 union `nfs::candidate_data_dirs()`，而它包含主机真实的
`$HOME/.local/share/grove`。夹具自己只登记 1 个 worktree，开发机上多一个无关 grove 条目就把 `registered`
从 1 变成 2，断言于是取决于**跑测试的那台机器装了什么**。诊断打印出的 `candidates` 里同时列着
`/home/chaos/.local/share/grove` 与夹具目录，是这条链的直接证据。

修法沿用该 crate 已有的隔离机制而不是新造一个：`auto_gc.rs` 的 10 处夹具构造改走
`isolated_fixture()`，它调用 `GrokHomeFixture::isolate_xdg_grove_data()` 把 grove 查找限制在夹具内；该
方法先取 `nfs::GROVE_ENV_LOCK`——`GROVE_DATA_DIR` 是进程级环境变量，`nfs/remove.rs` 里三条测试也写它，
不共享一把锁的话两个测试会互相看到对方的设置（无嵌套获取，故不构成死锁）。**验证**：单跑
`cargo test -p xai-fast-worktree --offline --locked --features metadata --lib` **504 passed / 0 failed**，
随后按原复现命令连续 30 轮全绿（`RED 0 / 30`），与修复前同一命令的 `RED 3 / 20` 对照；逐轮日志与
环境变量快照见 `docs/verification/fast-worktree-grove-isolation-2026-10-03.log`。
补的两条测试不读开发者目录的内容，把「隔离」这件事钉在夹具上而不是钉在计数上：
`a_fixture_hides_any_grove_dir_outside_itself`（先断言候选列表里**有**一个落在夹具之内的目录——正向
锚点，防止「列表恰好为空」让后半句空转——再断言没有任何一个落在夹具之外）与
`a_fixture_clears_a_grove_data_dir_that_was_already_set`（夹具创建**之前**就种好 `GROVE_DATA_DIR`，
这正是测试进程会带上它的方式）。变异矩阵 5 行证明这个锚点不是装饰：只改产品代码不再认
`XDG_DATA_HOME`（M4）时新测试红，而**同时删掉正向锚点（M5）两条测试一起变绿**；原来的
`registered == 1` 计数断言在五组变异里**全程绿**，也就是它本来就看不见这个缺陷。

同一轮之后又按原复现命令跑了 120 轮整仓：`auto_gc` 这一类 **0 红**，但另有两类各红 2 次
（`nfs::client` 与 `git::safety`），是两条与本行无关的独立缺陷，各自有自己的证据文件——这张表因此
同时说明本轮**没有**修完该 crate 的偶发失败。全仓测试数从本行写下时的 504 涨到 508。
（2026-10-03；`crates/codegen/xai-fast-worktree/src/auto_gc.rs`、`src/db/mod.rs`、
`docs/verification/fast-worktree-grove-isolation-2026-10-03.log`）

### 证据复测：remote 签名「删掉那一行会红几条」被重新测了一遍，顺手挖出测量装置自己的洞

`docs/verification/remote-provenance-linux-2026-10-02.log` 写着「把 provenance 调用删掉，会有一串测试
转红」，却没写删的是**哪一行**。不能照着复现的证据不是证据，这一轮在 `ef33dfd9` 上重做：先把锚点、替换、
以及真正落盘的 diff 写进日志，再跑任何东西。被删的只有 `InstallLayout::commit` 里的一行
（`self.provenance.check_file(staged, signature)?;` → `let _ = (staged, signature);`），长度下限、摘要比对、
平台头读取、版本目录、原子 `rename`、发布后复读一概不动——这正是真实的回归形状：一次冲突解决把唯一那句
查签名策略的调用吞掉，而它周围每一样都看着仍被照顾。

**结果**：基线（动手之前先量）`remote::install` 20 绿、`remote::client` 30 绿、`remote::provenance` 15 绿；
删掉那一行之后 install 红 4、client 红 2、provenance **照绿 15**。六条红的 panic 载荷被逐字引进日志，因为它们
印出来的正是被破坏的断言本体——`CommitOutcome { version: "2.0.0", previous: Some("1.0.0"), .. }` 与
`InstallOutcome { version: "1.0.0", current: true, .. }`：一个无人能归属的产物成了主机要启动的那一个。

**`remote::provenance` 那 15 条删不动，是结果的一半，也是给读者的陷阱。** 它们直接调
`ProvenancePolicy::check`/`check_file`，测的是**策略**不是**接线**，删调用点不可能让它们红。所以「provenance
的测试通过」对「这台主机到底执行不执行」零信息量——执行活在 `install.rs`，由上面那六条守着。今后这一行任何
结论都必须引 install/client 的名字，不能引策略自己的测试。

**第一次尝试暴露的缺陷在测量装置身上，值得单独记。** 脚本原本先变异、跑测试、还原，最后才跑未变异基线，
于是它把同一次变异测了两遍、把第二遍当作对照组（基线报的正是那 4 + 2 条）。两个独立原因，每一个都足以凭空
造出「变异全被抓」的绿色结论：

- `shutil.copy2` 按设计连同备份的 **mtime** 一起还原，而那个时间戳比 cargo 用被变异源码产出的构建更旧。
  cargo 按 mtime 做指纹，看到「源比它自己的产物还老」便跳过重编，于是所谓基线跑的是被变异的二进制。
  修法：每次内容改动之后都 `os.utime(path, None)`，包括还原那一次；还原后再把三组过滤器跑一遍——
  字节相同的源码压在一个由变异源码构建的二进制之上，仍然是在对树撒谎。
- 顺序。放在实验之后的基线，说的是「已经被改过的树」。修法：基线先测，不绿即 `ABORT: the baseline is not
  green` 退出 4，不再产出一个谁都无法解读的数字。这条退出分支被第一次运行真实踩过。

还原证据：`sha256(pristine)=938a8130f2ef…4a25f1`，复制回来后 `sha256(after)` 同值、`cmp` 退出 0，三组过滤器
第三次跑仍是 20 / 30 / 15 全绿。日志里另给了**不依赖任何临时目录**的手工复现序列，其中 `touch` 是承重的
（原因见上）。

**同轮另一条复测**：`scripts/install-sh-in-docker.sh` 在容器存活上限改为 `CHAOS_LAB_KEEPALIVE`（6 小时，启动
时打印）之后重跑，19/19 全绿，`checksum OK 0ee7d6ee…`、`signature OK` 与 2026-10-02 逐项对上，摘要与
`docs/verification/release-signature-v0.4.2-2026-10-02.log` 记录的同一个值一致，说明发布产物没在这两份记录
底下被重传。被重跑的到底是哪个脚本也被钉住：`git log --oneline -1 -- scripts/install.sh` 仍指向 `88511413`，
`cmp` `1487aded` 与 `ef33dfd9` 两个提交里的 `scripts/install.sh` 无差异。green 本身不是重点——修复夹具之后
的 green 和修复之前的 green 说的是两件事，所以记录里写的是跑了哪个脚本，而不是默认沿用上一节。

### 修复：Web 主机重启后浏览器不能自愈——协议占位符被当成真工作区 id，丢失的会话无人接管

给「浏览器层的断线/重连/snapshot fallback」补第一条真实故障注入的 Playwright 用例时，页面在主机重启之后
停在「请求错误」再也不动。两个独立缺陷：

**一、UI 把协议里的占位符当成真 id 回传。** `ServerMessage::Workspaces.active_workspace_id` 不是
optional，主机在没有活动工作区时必须给个值，于是报 nil UUID。UI 照单全收存成 `activeWorkspaceId`，
重连时回 `create_session workspace_id="00000000-0000-0000-0000-000000000000"`；主机只能在登记表里找这个
id，找不到，回 `workspace_unavailable`。修法是把占位符显式命名为 `NIL_WORKSPACE_ID`，回传前经
`activeWorkspaceIdOrNull()` 过滤，`workspaceChanged` 的 `workspaceId` 改为可选、无工作区时把 `sessionId`
置空（原来保留的是上一个工作区的会话）。两侧都要动：`chaos-engine` 新增 `selected_workspace()`，把**入站**
nil 也读作「未选」，因为 `workspace_unavailable` 与 `workspace_session_mismatch` 两个错误的产生点都在
主机侧，只改 UI 会留下「别的客户端发同一字符串仍然红」。这不是防御性冗余。

**二、没有任何东西接管「主机不再认识我的会话」。** 会话活在进程内存里，主机重启即全丢。重连后 UI 发
`resume(旧 session_id)`，主机回 `session_not_found`，UI 只是把徽标刷成「请求错误」就停在那儿。而
`submit()` 在没有 session id 时直接 return——**输入框按发送键什么都不做，不是报错，是静默失灵**。
修法：`sessionLossRecoveryMessage()` 对 `session_not_found` 与 `workspace_session_mismatch` 主动申请一个
主机能答的新会话。凡 early-return 于「状态不完整」的入口，都值得问一句有没有东西会让状态**永久**不完整。

**根因不是读代码读出来的，是把每一帧打下来看到的**：`routeWebSocket` 两侧帧落盘，`addInitScript` 里用
MutationObserver 把状态徽标出现过的每个值记进 `window.__chaosStatusSeen`（恢复会在半秒内穿过
连接断开，正在重连 → 已连接 → 历史已恢复，轮询渲染文本会漏掉中间态——这个技巧保留在正式测试里）。
重启后那条腿的出站是 `resume(旧 id)` → `create_session(workspace_id=占位符)`，入站是
`session_not_found` → `workspace_unavailable`：第二个错误是 UI 自己造成的。

**修复前的症状在真实 shipped 路径上重放过**（把 UI 两处改回去、`dist` 重新 build）：
`Expected: "会话已创建" / Received: "请求错误"`，桌面与移动两个视口同时红；然后写回原文 `cmp` 校验字节一致。

**7 处变异，0 存活，0 处未还原**。其中一处值得记：把 `activeWorkspaceIdOrNull` 的过滤去掉，
「占位符视为无工作区」那条测试**仍然绿**——它测的是存储路径，变异动的是回传路径；红的是恢复测试里的
`workspace_id: null` 断言。两个函数必须分开、两侧各要有一条夹具，否则任意一侧被"简化"掉都还有测试绿着。
另一处被变异纠正的是我自己的断言：`expect(firstHost.exitCode).not.toBeNull()` 在 SIGKILL 之后必然红，
因为被信号停掉的进程 `exitCode` 本就是 `null`、`signalCode` 才有值。正式测试注释里那句「关服务器那一侧的
腿传不到页面」原本只是「我记得」，现在有记录：同一页面里先 `server.close()` 等 4 秒（腿数仍 1、徽标不变、
无新帧），再 `route.close()`（腿数立刻 2、出站 `list_workspaces, resume`）——关错一侧的话这 3 条会全部
「通过」而什么都没注入。

`npm test` 38 通过/5 文件（`session.test.ts` 16→18 例）、
`cargo test -p chaos-engine --test workspace_session` 4 通过（一条走 `ListWorkspaces`→断言占位符→
`CreateSession`→`Resume` 完整回环，另一条是同一段代码的反向夹具，防止「过滤」退化成「不校验」）、
Playwright 6 通过（桌面+移动 ×3 条，11.3s）。边界：Tauri 主机未被 e2e 覆盖；重连退避的次数没钉成常数；
恢复出来的是空会话而非旧 transcript（UI 变绿不等于用户回到原来的对话）。
见 `docs/verification/web-host-reconnect-2026-10-03.log`。

### 新增门禁：作业需要的工具，必须在触发它的那个步骤之前装好

CI 的 `docs localization` 作业每天都红，根因是它需要的工具那个 runner 上根本没有——
`scripts/l10n-guard.sh` 要 `rg`，而 `docs-l10n` 作业**从来不装 ripgrep**，前置检查命中后 `exit 64`、
17 例夹具全红。全仓扫一遍供给：`rust` 作业 apt 装了 `ripgrep`、`platform tests` 下载它的 release 包、
`gui`/`gui-browser-e2e`/`npm-scripts` 都有 `actions/setup-node`。只有 `docs-l10n` 什么都没装，而它需要的
恰好是别的作业为了完全不同的原因顺手装上的那两个之一——这个洞在任何一次「照着别的作业抄一段」里都不会
被发现。因果方向要写清：**这个红是上一轮 l10n fail-closed 改造的成果，不是新引入的坏**。改造之前 `rg`
缺失会让列清单的管道吐出空集，而空集在本守卫的语义里等于「没有中文丢失 = 通过」，那天的 CI 于是绿着、
什么也没测；改造之后它拒绝出结论，作业才红。红是对的，缺的是那个 apt 步骤（已加在 checkout 之后）。

新增 `scripts/ci/check-workflow-toolchain.py`，让这类事不再靠人记得。规则只读 `run:` 正文：整行注释与
`#` 尾注在匹配前一律去掉（`strip_trailing_comment()` 一个函数负责，它跳过引号内的 `#`，
`…/releases/#anchor` 这种 URL 不能被剪断）、步骤 `name:` 不算脚本、`node|npm|npx` 必须出现在行首或
`;`/`&`/`|` 之后、跨步骤比索引且同一脚本内比行号（`apt-get install ripgrep` 写在守卫之后仍然算
「跑不了守卫」）、`uses: actions/setup-node` 算 node 的供给、点名的 workflow 不存在或一个都没找到都直接
退出 1。工具表**故意只有两条**（ripgrep、node），每条都是已经咬过一口的，没有一条来自「看起来像需要」。
15 例夹具的第一条就是真实仓库，它在修 `ci.yml` 之前红而**只红这一条**——其余 14 例全绿说明红的是仓库
缺工具，不是新检查算错。**11 处变异，0 存活**；W6/W7/W9 顺带把真实仓库那条弄红，正是「放宽规则会误伤
真实 workflow」的直接证据。

两轮等价变异揪出守卫自己的两个缺陷。W3 关掉的注释过滤是**死代码**：`strip_trailing_comment()` 对整行
注释本来就先在 `#` 处截成空串，那层过滤永远轮不到起作用——一处永远不影响结果、读代码的人却以为它在起
作用的条件，处置是删掉而不是补测试。W5 不红是**夹具写错**：那条夹具的步骤名少了 `scripts/` 前缀，而触发
词是路径 `scripts/l10n-guard.sh`，所以它无论守卫怎么写都绿；补齐触发串后 W5 立刻转红——「步骤 `name:`
里的守卫名不算需求」这句话在写下的时候并没有被验证过。顺手记一条 CI 语义，免得「CI 全被取消」再被读成
CI 坏了：`cancel-in-progress: false` 只保证**正在跑**的作业不被掐掉，不保证排队中的作业活着，GitHub 在
同组排进新作业时会取消组里已在排队的旧 run。见 `docs/verification/workflow-toolchain-2026-10-03.log`。

### 修复：Windows 腿过了安装器那一步，却被本仓库自己的夹具绊住——一条 Linux-only 测试挂着跨平台的名字

run `37061975515` 的 `platform tests (windows-latest)` 终于走通了 `Parse the PowerShell
installers` 的解释器问题（`ok executing install.sh's detect_platform with
C:\\Program Files\\Git\\bin\\bash.exe`、`8 check(s), 0 failure(s)`），然后在紧接着跑的夹具上红了：

    AssertionError: "this host's own uname" not found in "...（前面几行略，全文见证据文件）"
         note  this host's uname is a Windows one, so detect_platform refuses, as designed

被打印出来的 actual 里那一行好端端地在那儿。install.sh 那一腿有**两句终点语，由主机决定走
哪句**：POSIX 主机 `ok this host's own uname picks '<asset>', which is published`，Windows 主机
`note this host's uname is a Windows one, …`（`install.sh` 对 Windows 用户的正确行为就是让他去跑
`install.ps1`）。夹具无条件断言 POSIX 那一句，于是它是一条**挂着跨平台名字的 Linux-only 测试**——
同批 13 条里另外 12 条在 Windows 上全过。讽刺的是它正是上一轮为了「别让 Windows 再被环境绊倒」
而加的；教训写成一句话：**在 Windows runner 上跑过之前，任何「Windows 已解锁」的说法都不成立**。

修法承认主机腿的结论不由测试决定：不再钉某一个措辞，而是要求**恰好报告了两句之一**（0 句或
2 句都判失败），并把两句话分别钉住——探针臂调 `run_detect_platform`、主机腿调 `run_shipped`，
只 monkeypatch 后者就能在**任意主机**上驱动目标分支，不惊动其它检查。证据不是推测：CI 抓到的
Windows 原文被直接喂给新断言，旧断言 `present? False`、新断言接受并选出 Windows 那句。**5 处
变异 0 存活**，其中 B1 钉错措辞、B2 让分支不可达、B3 改守卫侧措辞、B4 把 Windows 拒绝从 note
改成 failure 四条都让新夹具转红——它们才是「两侧任一改词都会在 Linux 上被抓到，而不是第五次
push 才在 Windows runner 上爆」的论据。本机 `Ran 14 tests … OK`。

**边界要说白**：`cargo test (target-OS crates)` 在 Windows 上仍然一次也没执行过（这一步在
`cargo test` 之前，step 9 本次依旧 `skipped`）。macOS 已有真实结论，Windows 没有。
见 `docs/verification/platform-ci-2026-10-02.log` 的 2026-10-03 一节。

### 修复：`GitGate` 解析不出仓库时那次 invalidate 是空转，在飞的 git walk 会被当成新读返回

`GitGate::invalidate(root)` 的契约是「工作区变了，作废缓存，之后的读必须是新 walk」。它解析不出
这是哪个仓库时走 `None` 分支，意图保守地把这个 gate 知道的一切作废：把 `state.epochs` 里每个 epoch +1。
但 `state.epochs` **只有 `invalidate` 自己会写**——`decide()` 读 epoch 用
`state.epochs.get(&key.root).copied().unwrap_or(0)`，从不插入。于是对一个从没被 invalidate 过的仓库，
这条分支遍历的是空 map，**一个字节都没改**：epoch 仍是 0，`decide()` 认为在飞的那条 walk epoch 匹配，
返回 `Decision::Join`，调用方拿到的是**改动之前**那次 walk 的结果。清快照救不了——快照本来就没写，
能被复用的是在飞的那一个。

**这条分支在生产里是常规路径，不是边角**：`ROOT_CACHE` 是进程级 `static`，`ROOT_CACHE_TTL` 生产取 30 s，
另有 `MAX_ROOT_CACHE = 1024` 满表时 `retain` 过期项、装不下就整表 `clear()`。会话开着仓库、读过一次 git
状态、三十秒后改了工作区再 `invalidate(git_root)`——`session/git.rs` 的四个调用点（`:111`、`:2773`、
`:3159`、`:3353`）走的正是这条路——缓存项早过期，invalidate 于是空转；此刻恰有一个 walk 在飞，它就拦不住。
**修法不引入新机制**，用的还是这个模块本来就有的 epoch 比较：`None` 分支在整体 +1 之前，先给每一个当前
有 slot 的 root `epochs.entry(root).or_insert(0)`，让这次 bump 落到真实存在的在飞请求上。`evict_slots`
本来就保留 `inflight.is_some()` 的 slot，补出来的条目不会被顺手清掉；已排队的 waiter 仍由 `finish_wait`
的 `epoch != current_epoch → WaitEnd::Retry` 处理。`Some(canon)` 分支不动，它自己已经 `or_insert(0)`。

**发现方式是本地 Docker 入口第一次跑 `--full`**，前 17 个门禁全绿，最后的 `cargo test` 报
`invalidate_during_inflight_does_not_return_stale_walk` 失败于 `timed out waiting for walk count 2
(have 1)`，`1933 passed; 1 failed`。`have 1` 说明第二次 `run` 没起新 walk，直接并进了 invalidate 之前
那条。**先排除环境因素**：`git_gate` 模块单跑 15 次全绿——不是这条测试自己的时序，是同进程里别的测试
把它依赖的状态弄坏了。把「不稳定的测试」当输入而不是结论，才让这一轮落到真 bug 上；如果当时选择重跑一次
看看，这个缺陷会继续留在发布分支上。

**回归测试 `invalidate_with_an_unresolved_root_still_supersedes_an_inflight_walk` 不靠 sleep 撞 TTL**——
那正是上一轮 l10n 事故里「坏仪器看起来像测出了结果」的同族写法。它拿一个本 gate **从没打开过**的第二个
临时仓库去 `invalidate`，使 `lookup_cached_root` 结构上必然 miss、`epochs` 结构上必然为空，从而必然走到
`None` 分支；断言第二次 `run` 起了新 walk，且交回的是新 walk 的结果（`2` 而非 `1`）。两个变异各写回原文并
`cmp` 确认字节一致：A 删掉补 epoch 的那段（即修复前发布的代码），B 算出列表但一条都不 apply，两者都让新
测试转红，失败信息与 `--full` 里那条**逐字相同**（`timed out waiting for walk count 2 (have 1)`）——这条
测试的论据不是「某个断言变红了」，而是把同一次故障在受控条件下重造了出来。格式检查第一次就抓出手写换行
不合 rustfmt；改后 `cargo fmt --all -- --check` 退出 0、`cargo clippy -p xai-grok-workspace --all-targets
--locked -- -D warnings` 无诊断、整包 `--lib` **连跑 8 次全绿**（`1935 passed; 0 failed` ×8），
`session::git_gate` 模块 16 例全绿。

边界：没有端到端驱动 `git.rs` 那四个生产调用点，本节证明的是 `GitGate` 单元在「root 解析不出来」时的
语义，不是「真实工作区改动后 GUI 看到的 git 状态一定新」（后者需要贯穿 Engine 的会话级回归）；
`MAX_ROOT_CACHE` 撑满触发的整表 `clear()` 与 `store_cached_root` 的 `retain` 分支仍无任何测试驱动——
新测试是**结构性**走到 `None` 分支，不是制造缓存压力；变异与 8 次连跑只在 Linux/x86_64 容器，macOS 上
`TMPDIR` 经 `/var`→`/private/var`，`dunce::canonicalize` 之后路径形状不同，那条路径走到 `None` 分支的
概率只高不低，但未实测。（2026-10-03；`crates/codegen/xai-grok-workspace/src/session/git_gate.rs`、
`crates/codegen/xai-grok-workspace/src/session/git_gate_tests.rs`、
`docs/verification/git-gate-invalidate-all-2026-10-03.log`）

### 改进：Docker 入口与 CI 的门禁镜像从「数量差不多」变成一条会红的规则

`scripts/verify-in-docker.sh` 的卖点是「在本地跑的就是 CI 跑的那串命令」，但这句话此前无人执行：
`scripts/ci/` 的 32 个守卫里有 16 个出现在 `gates` 数组，其余（版本 lockstep、panic-site 两条棘轮、
installer 签名策略、npm 侧、GUI 协议漂移）只在 `ci.yml` 跑。`check-guard-wiring.py` 上一轮接线时
只保证「有人跑」——它把同时被跑的数量打印出来，却不据此失败，于是这个差距可以任意扩大而无人报警。
差距真正的代价不是少跑了几个脚本，而是**每条只在 CI 跑的门禁都缺少"提交前本地能红"这一层**：
CI 红的时候人已经在写下一个改动了。

**规则而不是名单**：每个守卫必须恰好落进两类之一。**被镜像**——`gates` 数组在路径位置点名它，或入口
已经跑的脚本在路径位置点名它（与可达性同一条传递规则）；或者**登记为 CI-only**——
`scripts/ci/docker-entry-ci-only.tsv` 里一行 `<名>\t<理由>`，理由必须写清它需要的是干净容器里没有的
什么东西。两边都不在就报红并点名它，`--list-mirror` 打印整份分类供人核对。反向也查两条：清单里为已删除
的守卫保留的行、以及清单说 CI-only 但入口其实会跑的行，都是错误——前者防清单烂成退役守卫的墓地，
后者防"登记"变成绕过门禁的后门。**CONTRIBUTING.md 里明说：只加进 `ci.yml` 是这条检查唯一拒绝的选项。**

镜像项 16 → **26**。新增的都是本地无凭据就能跑、此前却只在 CI 跑的：version lockstep
（`check-versions.sh` + `check-version-lockstep.py`）、panic-site 的两条棘轮**连同它的自测夹具**、
`test-script-portability.py`（守卫早就在跑、它的自测不在——正是这条规则该抓的形状）、
installer 签名策略、以及 npm 侧 `node --check` ×3 + `test-publish-npm.sh`；为此
`docker/verify.Dockerfile` 装上 Debian 的 `nodejs`，Dockerfile 注释写明它只用于解析检查与假 npm 夹具，
不是「发布也跑在这个 Node 版本上」的主张（Debian 的比 CI runner 的老）。不镜像的只剩 5 条，理由同形：
`check-gui-protocol.sh` 要 dev profile 编译 `chaos-engine`（入口的 cargo 门有意止步于 check/clippy），
`check-powershell-syntax.py --require` 要真 PowerShell（bookworm-slim 引它得挂 Microsoft 源），
三条 release-integrity 实验室脚本要装配好的发布产物。

**两条新规则是被实测和变异逼出来的，不是设计出来的。** 其一，可达性一直按**裸文件名**子串匹配，
代价在真实树上看得见：本检查自己的 `EXEMPT` 那一行也算"点名"，于是上一轮报的「32/32 可达」里有 1 个是
它自己点名自己——同一棵树上分别跑新旧两套规则实测：32 → 31，丢掉的正是 `ignored-tests.sh`，而它本来就该
靠豁免而不是靠自点名。同一类误判这轮又出现在自己新写的真实树用例上：那条用例断言 `check-gui-protocol.sh`、
`check-powershell-syntax.py` 属于 CI-only，而那行断言本身把两者"点名"成了镜像。现在要求名字出现在
**路径位置**（前面必须有 `/`），`"$(dirname "$0")/helper.py"` 仍算调用；真实树随之变成 31 可达 + 1 豁免。
其二，`scripts/ci/` 下还有 `.tsv`/`.txt` 数据文件，把数据当调用者会让本检查新加的 ci-only 清单把自己豁免的
守卫标成"入口在跑"：2×2 实测（匹配规则 × 谁能当调用者）——两条都放宽时 5 条 CI-only 守卫**全部**误判、
镜像数从 26 虚涨到 32；只放宽一条也各误判 3 条；两条都收紧时为 0。检查差点把自己的清单咬了一遍。
现在只有 `.py`/`.sh`/`.mjs`/`.yml` 能点名别的脚本。
顺带修掉一个真实的镜像偏差：入口的 `workflow shells` 门禁原先只查 `ci.yml`，而 CI 那一步不带参数，
而这个检查存在的理由——Windows 矩阵——在 `release.yml` 里；现在两边一样。

**验证**：`test-check-guard-wiring.py` 由 9 例增至 **16 例**，其中一条直接断言真实树的分类与两份清单一致
（`publish-npm.sh` 等必须算镜像、`check-gui-protocol.sh` 等必须算 CI-only），另六条各造一个仓库：未镜像且
未登记被点名、登记已删除的守卫被点名、登记一个入口其实会跑的守卫被点名、缺理由的行点名行号并拒绝、
数据文件点名不算调用、经已镜像守卫传递到达算镜像。六个变异（不因镜像缺口失败 / 容忍已删除的行 /
容忍与入口冲突的行 / 跳过缺理由的行 / 把数据文件当调用者 / 退回裸名匹配）各让对应用例转红、
还原后 `cmp` 字节一致；"数据文件当调用者"那个变异第一次没红，补出"行里写路径 + CI 步骤 `cat` 这个数据文件"
两步之后才红——变异没红的原因本身也是一条关于夹具的信息。新镜像的每条门禁都在容器里真跑过（version
lockstep 7 check 0 failure、portability OK(23) + 5 tests、workflow shells 两份 workflow OK、installer
8 check + 2 OK、`node -v` v18 下 `node --check` ×3 与 `publish-npm guards: OK`、panic-site 97 crates
baseline holds + uncompiled 0 files），且 `test-publish-npm.sh` 跑完后 `git status --porcelain` 与跑前逐字节
相同——npm 夹具不写开发者的树，这是它能进绑挂载入口的前提。

边界：镜像说的是"入口能到达这个脚本"，不等于"入口把它的每条分支都跑一遍"。`publish-npm.sh`、
`local-publish-host.sh`、`stamp-npm-version.mjs` 是经 `test-publish-npm.sh` 的夹具到达的，其中后两条只在
缺二进制的早退分支和从未被传入的 `--version` 分支上被碰到，这一点写在清单的文件头注释里而不是藏在
"已镜像"三个字后面。容器只覆盖 Linux 门禁这一事实没有改变。

（2026-10-03；`scripts/ci/check-guard-wiring.py`、`scripts/ci/test-check-guard-wiring.py`、
`scripts/ci/docker-entry-ci-only.tsv`、`scripts/verify-in-docker.sh`、`docker/verify.Dockerfile`、
`CONTRIBUTING.md`、`TODO.md`、`docs/architecture/todo-open-item-classification.md`、
`docs/verification/todo-open-items-2026-10-03.tsv`、`docs/verification/docker-gate-mirror-2026-10-03.log`）

### 修复：中文化守卫在 Docker 入口里静默失败，四处 fail-open 一并改为 fail-closed

`scripts/verify-in-docker.sh --full` 有一轮整场只有 `docs localization` 一个门禁红，而那一节的输出止于
「report dir」——既没有报告，也没有报错；同一轮 `cargo test --workspace` 全绿、守卫自测 13/13 通过，
所以坏的不是判定逻辑，是守卫的**输入**。根因在两处叠在一起：`verify-in-docker.sh` 里每个用 git 的门禁
都带 `bootstrap` 前缀（`git config --global --add safe.directory /src`），唯独这条没有，而仓库是以宿主
uid 1003 bind-mount 进容器、容器内进程是 uid 0，git 于是拒绝打开仓库；旧守卫又恰好把列清单的 stderr 吞掉
且不检查退出码，两侧清单都成了空集。「仪器坏了」和「测出了不合格」在日志里长得一模一样（两行头 + 非零
退出）。CI 永远看不见这个洞——GitHub runner 的 checkout 归 runner 用户所有，宿主机上也不会抱怨。

接线层：`GIT_CONFIG_COUNT=1 / GIT_CONFIG_KEY_0=safe.directory / GIT_CONFIG_VALUE_0=/src` 提到 `run_args`，
所有门禁共用（故意不设 `safe.directory=*`，只信 `/src` 这一个明确路径）；「不一致的门禁清单」本身就是这次
事故的成因，所以 `docs localization` 也补上与其它 git 门禁相同的 `bootstrap` 前缀，即使全局 env 已经够用；
门禁循环之前加一条 preflight，
容器里的 git 读不出 `/src` 就整轮直接退出并说明要检查 bind mount 与 `GIT_CONFIG_*`，否则同类接线漂移的表现
依旧是「十几个门禁一起红，没人说为什么」。守卫层堵掉四处 fail-open：列清单走 `< <(list_rs_files …)` 进程替换，
退出码不进 `set -e`，git 一死该侧就是空集，而空集在本守卫的语义里等于「没有文件消失 = 通过」——改成先捕获
再判状态，失败即点名是哪一侧；空清单曾被当作合法输入，一个 395 个含汉字文件的仓库任一侧列出 0 个 `.rs`
只可能是仪器坏了，现在直接拒绝；计数用 `… | rg … | wc -l`，管道末端的 `wc` 把 rg 的死活（137）盖成 0，
而「文件从一侧消失」恰恰是本守卫要报的信号，坏仪器因此产出**看起来合理的错判决**——现在分别取 rg 的退出码，
>1 报错退出，1 保留为「没有匹配」这个真答案；`--before` 侧读不出文件内容时同样静默计 0，现在 `git show`
失败即报出路径并终止。另外加 `set -E` + ERR trap（死点常在子 shell 里，进程内变量传不回来，因此用一次性
marker 文件 + `trap … EXIT` 清理），任何非预期退出打印
`NO VERDICT -- died at line <N> (exit <rc>) while running: <命令>`；顺带删掉无调用者的死函数 `count_han_in()`，
它内部正是上面第三、四处那种写法，留着迟早被复用。

`scripts/l10n-guard-selftest.py` 由 13 例增至 17 例，四个新例各把一个 helper 换成敌意替身（PATH 前置 shim），
断言「非零退出 + 点名坏在哪 + 不产出报告」：`death_is_announced`（git 一律 exit 137，OOM 的形状）、
`killed_counter_is_announced`（rg 通过预检探针、对真实文件 exit 137）、`unreadable_at_ref_is_announced`
（只让 `git show` 失败）、`empty_listing_is_refused`（git 成功但列出空集，pathspec 或 checkout 位置不对）。
五处硬化逐一反向改回去：注释掉 ERR trap → 三例红（14/17）；忽略清单退出码 → `death_is_announced` 红；
删掉 rg 退出码检查 → `killed_counter_is_announced` 红；删掉 `git show` 检查 → `unreadable_at_ref_is_announced` 红；
接受空侧 → `empty_listing_is_refused` 红。每次写回原文并 `cmp` 确认字节一致。改造前后是同一容器、同一份仓库上
的两次真实运行：把 `HEAD:scripts/l10n-guard.sh` 抽出来挂进去原样跑，退出 1、只有两行头、报告目录 8 个产物
**全是 0 字节**（不是「测出 0 个问题」，是根本没测）；换改造后的守卫且仍不带 `GIT_CONFIG_*`，
`fatal: detected dubious ownership…` + `cannot list .rs files at HEAD` + `NO VERDICT -- died at line 322`
第一次被写进日志；补上 `GIT_CONFIG_*` 之后这条门禁在容器里第一次真跑出判定（before/after 各 395 个含汉字文件、
`0 broken link(s)`、`0 English prose line(s)`、gate exit=0）。

**验证**：`l10n-guard-selftest.py` **17/17**、`check-doc-l10n-selftest.py` 52/52、`check-script-portability.py`
OK(23)、`check-workflow-shells.py` OK、`bash -n` 两份脚本通过；真实树 `bash scripts/l10n-guard.sh`
（HEAD vs WORKTREE）与 `--before HEAD --after HEAD` 均 PASS 395/395。门禁接线没有新增——
`l10n-guard-selftest.py` 早已在 `ci.yml` 的 `docs-l10n` 作业与 Docker 入口的「localization guard self-tests」里跑。
边界：ERR trap 只负责把「没有结论」变成有位置、有命令名的一条错误，它不改变判定逻辑，也不改变通过条件，
真正的 fail-closed 是上面那四处。（2026-10-03；`scripts/l10n-guard.sh`、`scripts/l10n-guard-selftest.py`、
`scripts/verify-in-docker.sh`、`.agents/skills/chaos-upstream-sync/SKILL.md`、
`docs/verification/l10n-guard-fail-closed-2026-10-03.log`）

### 新增：`scripts/ci/` 里不允许再有没人调用的守卫；TODO 状态文档与 `TODO.md` 从此逐格对齐

`scripts/ci/test-classify-open-todos.py` 断言
`docs/architecture/todo-open-item-classification.md` 的分组计数等于 `TODO.md`。它被提交、被引用，
却没有任何地方跑过它，而且是红的——本轮是从别的方向撞上看见的，不是门禁报的。文档因此一路漂到
声称 51 unchecked / 98 partial / 149 行，表里写 `M0 0/13`、`M3 8/9`、`M4 24/3`、maintenance 6/14，
而**同一个文件的正文写的是 maintenance 5/13**；真实值是 `unchecked=23 partial=113 rows=136`，
`M4` 那一行相对它自己的正文还是转置的。它把三份未留存日志当证据路径，还沿用已作废的
429/218 ignore 数字（现场重测：428 个 `#[ignore]` 属性、0 条裸属性，`--require-reasons` 通过）。

`classify-open-todos.py` 改成三种模式。默认模式输出逐行清单（行号 / 状态 / 最近标题 / 原文 +
`TOTAL`）；`--groups` 按里程碑分组，**任何一行不属于任何组就直接失败并列出 `line N: <heading>`**，
新增 `### M6.` 段不会被静默排除在全部计数之外；`--check-doc` 把文档表格与 `TODO.md` 逐格比对，
差异按 `M4 unchecked: document says 3, TODO.md has 5` 这种可执行的形式打印，并附上重算命令。
文档表格、两处过时正文数字、以及指向会话私有临时目录日志的三处证据指针全部改掉，逐行导出改为随仓库提交的
`docs/verification/todo-open-items-2026-10-03.tsv`。

真正的新东西是第二个检查，因为「没人跑」这一类缺陷此前没有任何东西守着，本轮已连着撞上四个：
`classify-open-todos.py` 与它的 fixture、`test-brand-protocol.py`（brand 守卫的自测，CI 只跑被检对象）、
以及更早的 `check-doc-l10n-selftest.py`。`scripts/ci/check-guard-wiring.py` 的规则是可达性而不是名单：
根 = `.github/workflows/*.yml` + `scripts/verify-in-docker.sh`，再沿「已被可达文件点名的 `scripts/`
内文件」做不动点传播，于是 CI 跑 `install-integrity-in-docker.sh`、后者启动 `release-integrity-serve.py`
这条链算可达。**散文不算调用者**——`#` 行、行尾注释、Python docstring 三者在匹配前一律抹掉。
这条规则是被变异逼出来的：第一版只在非 Python 侧过滤 `#`，结果它自己的 docstring 里点了四个守卫的名字，
于是在 `test-brand-protocol.py` 确实没接线的真实仓库上报 OK。`docs/` 同样不扫——
`ignored-tests.sh` 被两份审计报告引用、无人执行，正是这条规则要拒绝的舒适区。反向也查：workflow 或
Docker 入口点名的 `scripts/...` 路径必须存在；白名单条目必须仍描述一个存在的文件，防止它烂成退役守卫的墓地。

测试：分类器 8 条（逐行输出与 `TOTAL` 精确匹配、`--groups` 精确匹配、无主标题两种模式都拒绝、
真实文档必须通过、某组两列对调必须失败并点名该组、缺行 / 重行 / 非数字各自拒绝），其中 TODO 侧的数字
**是现场跑 `--groups` 读出来的而不是抄文档**，否则两侧一起错也不会红；接线检查 9 条（真实仓库必须 OK、
未接线守卫被点名、经实验室脚本可达算数、`#` 注释不算、docstring 不算、`docs/` 提及不算、悬空调用点被点名、
为已删文件保留的豁免被拒、只接在 Docker 入口也算）。变异 14 个全部转红、还原后 `cmp` 字节一致：
分类器 7 个（`check_doc` 永不报漂移、静默丢弃无主行、容忍重复行、容忍缺行、非数字读成 0、真实文档改一个
数字、从 `GROUP_PREFIXES` 删掉 `## M3.3`），接线检查 7 个（把 `#` 注释当调用、去掉传递跳、容忍悬空调用点、
容忍豁免腐烂、把 `docs/` 当调用者、把 docstring 当代码，以及在真实仓库删掉 `test-brand-protocol.py`
的两处真实接线——最后这个就是该检查存在的理由本身）。证据日志第 5 节记的是本轮真实走过的顺序：
三条 MT-7 条目落进 `TODO.md` 后，文档表格没跟上，门禁立刻以
`Maintenance items / §8 unchecked: document says 2, TODO.md has 3` 报红。

接线：`ci.yml` docs-l10n 作业新增「TODO status document matches TODO.md」步骤，`workflows present`
作业新增 `test-check-guard-wiring.py` + `check-guard-wiring.py`，brand 守卫步骤补跑它自己的 fixture，
`scripts/verify-in-docker.sh` 增加 `TODO status doc` 与 `CI guard wiring` 两条门禁并把 brand fixture
并入 brand 门禁。当前覆盖：`scripts/ci/` 32 个文件全部可达，16 个同时被 Docker 入口跑，1 个豁免。
留下的口子记在 TODO：Docker 入口只镜像了一半守卫，`check-guard-wiring.py` 打印同时被跑的数量但不据此
失败——哪些必须留在 CI（npm 发布、目标 OS runner、外网）需要逐个定策。（2026-10-03；
`scripts/ci/check-guard-wiring.py`、`scripts/ci/test-check-guard-wiring.py`、
`scripts/ci/classify-open-todos.py`、`scripts/ci/test-classify-open-todos.py`、
`docs/architecture/todo-open-item-classification.md`、`docs/verification/todo-open-items-2026-10-03.tsv`、
`docs/verification/ci-guard-wiring-2026-10-03.log`、`.github/workflows/ci.yml`、`scripts/verify-in-docker.sh`）


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
