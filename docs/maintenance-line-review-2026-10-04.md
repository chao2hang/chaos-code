# 维护线（MT-1…MT-7）逐行复核 2026-10-04

`TODO.md` 第 7 章表头那句话（「核对于 2026-09-22，版本 `0.4.0`，分支
`sync/curated-port-20260918`」）已经过期 12 天、两个版本、一条分支。本文件是
`TODO.md` §8.2 那行 Q4 待办要求的「source-by-source MT-row review」：每条 MT 主张给出
**今天真正跑过的命令与输出**、判决、负责的角色、下次复核日。命令的原始输出、失败与
误记都记在 `docs/verification/maintenance-line-review-2026-10-04.log`（英文），本文件只
留结论与该看哪一节。

实测环境：Linux x86_64，HEAD `058726e5`，其上带着本轮尚未提交的 Windows LSP 修复；
发布链门禁的整轮结论取自 `058726e5` 的一次 `scripts/verify-in-docker.sh --full`
（33 个门禁全绿，`cargo test` 汇总 386 个 suite / 31,582 passed / 0 failed / 485 ignored）。
本轮全部改动落地之前又跑了一次主机侧门禁清扫 `bash scripts/verify-gates.sh`：27 段全绿、
4 段 build gate 未跑（由上面那次容器整轮覆盖），第一次跑时它真红过一次，原因与教训记在
证据日志 §14。凡本机跑不了的（npm 名字回收、Windows runner 上的 release dry-run、tag 动作、安全
签字），下面一律写「外部」，不改判为完成。

## 结论一览

| 编号 | 今天实测到的事实 | 判决 | Owner（角色） | 下次复核 |
|---|---|---|---|---|
| MT-1 | 仓库内 Cargo/npm 三处版本一致为 `0.4.2`；registry 上 `chaos-code` 仍是 `0.2.110`，`chaos-code-win32-x64@0.4.2` 直接 404 | 保持开放：仓库侧已闭环，发布侧仍被 npm 占位名阻塞 | 发布负责人 | 2027-01 |
| MT-2 | 仓库只有 `CHAOS_SIGNING_PRIVATE_KEY`（secret）与 `CHAOS_SIGNING_PUBLIC_KEY`（variable）两条名字；`v0.4.2` 有 6 产物 + 6 `.sig` + `SHA256SUMS`；安装器策略门 8 例绿；4 份 `.ps1` 由真 pwsh 解析通过 | 保持开放：只差带 Windows runner 的一次 dry-run 与 tag 动作 | 发布负责人 | 2027-01 |
| MT-3 | 工作树改动（复核时 18 条、本轮文档写完后 21 条）全部是本轮正在做的活；`TODO.md` 已被跟踪；`.chaos/uploads/` 由 `/.chaos/uploads/` 忽略 | **可关闭**：三个决定项都已落地 | 无需 owner（已闭环） | 不再复核 |
| MT-4 | `l10n-guard.sh --before HEAD --after HEAD`：395/395、四类全 0；`check-doc-l10n.py` `--english` 0 / `--links` 0 / `--fork-names` 0 / `--cells` 4 条 note；两份自测 17/17 与 52/52 | 保持开放：只剩代码侧短文案（弹窗底栏/toast/错误提示），需先建发射点清单 | 本地化维护者 | 2027-01 |
| MT-5 | 429 条 `#[ignore]` 全带 reason，baseline 双向对齐；平台门控台账 1,080 条 / 249 文件，`windows` 只编译 62 条，点名可拆 441 条，四个预算仍等于实测值 | 保持开放：441 条「看不出平台假设」的门要一条条拆或补理由 | CI 维护者 | 2027-01 |
| MT-6 | `panic-site-census.py` 两条棘轮都过（97 crate 生产位点不许升；无构建编译的文件为 0 个）；夹具全绿 | 保持开放：口径已固化，收敛本身是逐crate的长期活 | 各 crate 维护者 | 2027-01 |
| MT-7 | `SOURCE_REV=72a61251f` 仍是上游祖先，上游 `2bdd1d6a6` 领先 9 个提交（`ahead=9 behind=0`），分叉规模 `upstream/main..HEAD` = **996**（10-03 记录里的 969 已过期）；文档路径引用 275 份 / 47 悬空 / 25 记档；守卫接线 51 文件 50 可达 | 保持开放：窗口未关，且侦察脚本本轮才刚被证明不会覆盖记录 | 上游同步负责人 | 2026-11 |

`TODO.md` §8.2 那行 `[ ]`（Q4 maintenance cycle）由本文件与其证据日志结掉「MT-row
review」这一半；它自己写的另一半——release-gate 证据刷新——取上面那次 Docker `--full`
整轮。**这一行翻成 `[x]` 不代表外部平台门禁与安全签字被替代**，那一半在原行里已写明
不归本文件管。

## MT-1 发布链路正确性

主张（`sync/fork-layer-inventory.md` §6）：元包与六个平台包的版本会各自漂移，
`npm install chaos-code@<新>` 会去请求一个没发布过的平台包版本。

今天实测：

```
bash scripts/ci/check-versions.sh
  -> check-versions: OK — Cargo and all npm packages agree on 0.4.2
python3 -c "…npm/chaos/package.json…"
  -> meta version 0.4.2 / pins {'0.4.2'}
npm view chaos-code version          -> 0.2.110
npm view chaos-code-linux-x64 versions  -> 只有 0.2.110
npm view chaos-code-win32-x64 versions  -> 只有 0.0.1-security
npm view chaos-code-win32-x64@0.4.2     -> 404
```

判决：**仓库侧那一半已经真闭环**——三份版本不再是手写常量各写一处，`check-versions.sh`
在 CI 与 Docker 入口都跑，它把「元包 = 六个平台包 = 六条钉版 = Cargo」钉成一条命令。
**发布侧没有闭环**，而且不是版本号的闭环能解决的：registry 上最新仍是 `0.2.110`，
`0.4.2` 从未发布；两个 Windows 名字被 npm 的安全占位包占着，所以「`npm install` 成功但
`chaos` 跑不起来」这个形状今天在 Windows 上依旧成立（`release.yml` 有同名发布闸门拦着，
默认 `GitHub Release only`）。

外部动作（谁做）：包所有者向 npm 申请回收 `chaos-code-win32-{x64,arm64}`，或决定改名并
迁移 pins；之后由发布负责人决定要不要发 `0.4.3`。下次复核：2027-01，或 npm 名字有结论
时立刻。

## MT-2 自更新签名收尾

主张（`docs/audit-followup-report.md` §4）：签名接线已接通，真实密钥匹配与带 Windows
runner 的 release dry-run 未验证。

今天实测：`gh api repos/:owner/:repo/actions/secrets` 与 `…/actions/variables` 只回两条
名字（`CHAOS_SIGNING_PRIVATE_KEY`、`NPM_TOKEN` / `CHAOS_SIGNING_PUBLIC_KEY`），本轮**没有
读取任何值**；`gh release view v0.4.2 --json assets` 给出 13 个产物 = 六平台二进制 + 六个
`.sig` + `SHA256SUMS`；`test-installer-signature-policy.py` 8 例 OK；
`check-powershell-syntax.py` 用真 `/usr/local/bin/pwsh` 解析 4 份脚本全 OK。

判决：与 2026-10-02 的记录一致，未过期。剩下的是两件事，都不在这台机器上：一次带
Windows runner 的 release dry-run（`.exe` 至今只做到签名可验证、没执行过），以及 tag 的
创建与推送这类 owner 动作。下次复核：2027-01，或下一次 tag 之前必然要过。

## MT-3 工作树在途改动落地

主张：三个中文化文件未提交、`sync/` 与 `TODO.md` 是否入库、`.chaos/uploads/` 是否忽略。

今天实测：`git ls-files --error-unmatch TODO.md` 返回 `TODO.md`；
`git check-ignore -v .chaos/uploads/x.png` 命中 `.gitignore:17:/.chaos/uploads/`；
`git status --porcelain` 在复核那一刻是 18 条、本文件与本轮其余文档写完后是 21 条，两条都是当轮快照，内容全是本轮活（LSP 修复、门禁、文档），没有历史遗留。这个数会随每次改动变化，引用它必须带时刻。

判决：**关闭**。这一行的三个决定项都落地了，没有需要「下次再问一遍」的东西。

## MT-4 界面文案中文化（指南之外）

主张：指南已清零，剩下指南之外的面。

今天实测：`l10n-guard.sh --before HEAD --after HEAD` → before/after 各 395 个含汉字文件，
`moved 0 / removed-recorded 0 / regressed 0 / stale-allowlist 0 / shrunk 0 /
fortress-breach 0` → PASS；`check-doc-l10n.py` 四项 = 0 英文行 / 0 死链 / 0 上游名 / 4 条
有意保留的 note；`l10n-guard-selftest.py` 17/17，`check-doc-l10n-selftest.py` 52/52。

判决：保持开放，且**开放的理由不是没跑，是没做完**：`sync/2026-09-18-curated-port.md` §6
记的「弹窗底栏、toast、错误提示」这类短文案散落在大量文件里、且多为断言目标，需要先有
一张「用户可见发射点」清单才能动手；`check-doc-l10n.py --fork-names` 的 glob 到今天仍只
覆盖用户指南 `.md`，代码侧那条门禁缺口（TODO MT-4 里那条「门禁缺口」）没有新门禁补上。
下次复核：2027-01，或有人碰 `xai-grok-pager` 的文案发射点时。

## MT-5 测试债与 ignored 台账

主张：ignored 台账已建、季度 review 到期待做。

今天实测：`ignored-tests.py --require-reasons` → 429 条属性全带 reason；
`--check-baseline scripts/ci/ignored-tests-baseline.tsv` → baseline 双向对齐、429 条；
`platform-gated-tests.py` 在两个入口共同的预算（`--max-unreviewed 1106 /
--max-blind-windows 74 / --max-blind-macos 11 / --max-assumption-free 441`）下退出 0，
实测 1,080 条门控分布在 249 个文件、`windows` 只编译 62 条、点名「体内看不出平台假设」
441 条。

判决：保持开放。Q4 的 ignored 那一半已按 2026-10-02 结掉（全部有 reason、Owner=项目
负责人、复核日 2027-01）；新账是平台门控那一半——441 条要点名逐条判「该拆还是该补
assumption 标记」，441 与 1,106 两个预算只能降。**本轮实测把这条规则跑实了一次**：新加的
三条 LSP 回归测试第一版写成 `#[cfg(unix)]` / `#[cfg(windows)]`，门当场报「unlisted」并把
点名数从 441 顶到 442；处理方式不是抬预算，而是把三条改写成平台无关的写法（用真实的
绝对路径与 `MAIN_SEPARATOR`，Windows 的盘符情形交给同一条非门控测试在 Windows 上失败），
门回到 1,080 / 249 / 441 原值。下次复核：2027-01，每季一次。

## MT-6 unsafe / unwrap 收敛

主张：2026-08 的计数作废，需按当前源码重算并持续收敛。

今天实测：`panic-site-census.py --check-baseline scripts/ci/panic-site-baseline.tsv` →
`baseline holds: 97 crates, 0 fewer production sites than recorded`；
`--check-uncompiled scripts/ci/uncompiled-sources.txt` → `uncompiled set holds: 0 files`；
`test-panic-site-census.py` 夹具全绿。

判决：保持开放。口径（模块图可达 + cfg 区间判定）与两条棘轮都在 CI 与 Docker 入口里跑，
生产位点只许降；A/B 批的实际收敛仍是逐 crate 的慢活，`xai-grok-shell` 大批量 unwrap 那一
条尤其没动。下次复核：2027-01。

## MT-7 上游同步节奏与仓库卫生

主张：侦察节奏固定、`scripts/` 可移植、文档引用要能落地。

今天实测：`SOURCE_REV=72a61251f`，`git rev-parse upstream/main` → `2bdd1d6a`，
`git rev-list --count $(cat SOURCE_REV)..upstream/main` = **9**（上游领先），
`upstream/main..HEAD` = **996**（分叉规模）；`check-doc-path-refs.py` → 275 份文档 / 47 处
悬空提及 / 25 记档，链接全部可达；`check-guard-wiring.py` → 51 文件 / 50 可达 / 46 由
Docker 入口跑 / 4 记为 CI-only / 1 豁免；`check-evidence-paths.py` → 276 文件 0 命中。
（这三组数字是**含本文件与本轮改动**的终值，测量时刻在提交之前。）

判决：保持开放，窗口仍未关（3,325 files / +480025 / −173343，其中 290 个文件落在中文化
强保护路径），移植仍要 curated-port 评审。两处记录已在本轮就地更正：

1. `TODO.md` §MT-7 里第二次侦察那句写的是 `ahead=0 behind=9`，与脚本自己打印的
   `status=ahead ahead=9 behind=0`、以及 `sync/recon/2026-10-03-2bdd1d6a6.md` 表格里
   「上游领先 9 / 分叉领先 0」互相矛盾。9 在 **ahead** 一侧（GitHub compare 以
   `SOURCE_REV` 为 base、上游为 head），是那句话把两侧写反了。
2. 同一条记录里的分叉规模 969 是 2026-10-03 的数，今天同一命令是 996。分叉规模会随每次
   提交变化，引用它必须带日期——这类「会自己过期的数字」以后一律写明测量日。

顺带一条本轮的真实收获：`scripts/upstream-recon.sh` 第一次真跑（而不是夹具里跑）就在
这台机器上写出了 `sync/recon/2026-10-03-2bdd1d6a6-2.md`（未入库，测量完即删），因为
`sync/recon/2026-10-03-2bdd1d6a6.md` 已存在且内容不同——「侦察记录永不覆盖」这条设计在
真实仓库里兑现了。同一次真跑暴露记录落成 `0600`（`mktemp` 的模式被 `cp` 带到新文件），
已补 `chmod 644` 与一条夹具断言。补一句今天再跑一次的实测：同一个 UTC 日、同一个 tip，脚本
仍然确定性地写出同名那份记录，而它的权限已经是 `-rw-r--r--`（修复前是 `0600`），也就是这条修复
不只在夹具里成立；该记录同样不入库，跑完即删，因为同一天的上游状态已经由上面那份带评估内容的
记录承载。

下次复核：2026-11（月度节奏），或上游 tip 变化时。

## 本轮之外仍然开着的外部门禁

不计入本文件任何判决：npm 名字回收与改名决策、带 Windows runner 的 release dry-run、
tag/Release 的创建与推送、restricted-child fd 继承的独立安全评审、Tauri 打包与签名、
WSL/P9IO 崩溃报告外发。这些在本仓库里的表现是「一条会红的门 + 一句写明缺什么」，不是
绿灯。
