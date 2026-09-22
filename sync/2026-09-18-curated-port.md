# 2026-09-18 精选移植记录（→ 0.4.0）

本轮不做架构对齐，只从上游 `xai-org/grok-build` **手工搬运外科手术式的修复**，
并把用户指南中文化收尾。`SOURCE_REV` 维持 `72a61251fcffb464bcc687aeb5a998e5a98ec0c9`
（上游 tip 为 `a28ee2b2`）——**本轮不虚报对齐点**，每处移植在提交信息里写明上游
提交号。

## 1. 移植清单

| 上游提交 | 内容 | 本地提交 |
|---|---|---|
| `48271133` | 压缩重试不再把 TPM 429 误判为上下文溢出 | `47460ae6` |
| `48271133` | 长任务计时显示进位到小时，不再停在分钟 | `1bb7a966` |
| `75810042` | CJK / 色深渲染测试固定色深机制 | `1d6d316e` |
| `75810042` / `48271133` | `quick-xml` 0.41、`tikv-jemalloc` 0.7 | `011346fc` |
| `75810042` | Esc 中段提示的三条 pty 用例改名并重写 | `c181fd64` |
| — | 分叉自持章节中与代码相符的上游增量 | `e6583a78` |
| — | 纯上游章节增量与配置参考缺行 | `ca7e2f1f` |

判定方式：先按 `sync/fork-layer-inventory.md` 划出 12 个分叉层文件，凡上游改动
落在分叉层内的**一律不搬**；落在分叉层外的，逐条读 diff 判断是否依赖架构变更，
依赖的一律不做（本轮明确不做 MCP admission 放宽、不做 oniguruma 2→3）。

## 2. 测试基线修复

基线本来就有 8 条 CJK 渲染测试失败，另有 32 条分叉侧残留失败。本轮一并处理：

- 8 条 CJK 渲染失败 → 采用上游 `75810042` 的色深固定机制（`1d6d316e`）。
- 32 条残留失败 → 清零，并修掉它们暴露的 6 处真实缺陷（`e9526bcc`）。
- 两个编译不过的测试目标（公共快照历史遗留）：
  - `registered_features_are_documented` 读 `docs/internal/25-enterprise.md`，
    而 `docs/internal/` **在本仓库和 `origin/main` 的整个历史里都不存在**；
    改为 `required-features = ["internal-docs"]` 门控，默认不编译。
  - `pty_e2e_scroll_selection` 用了未定义的 `gap_row`；2026-08-15 的那次同步
    （`5163763e` / `10764cac`）删了定义却留下了四处 `sgr_mouse` 使用。按
    `c68e39f6` 里存在过的原始块逐字恢复。

### 2.1 并发下的偶发失败（本轮清零）

清完上面这些之后，同一份测试二进制连跑 5 次仍有 2 次各挂 1–2 条，**且每次挂的
用例都不同**。引用栏那一组用 `--test-threads 1` 连跑 30 次全过、并发则每次挂
3–4 条，据此判定是竞争而不是用例本身写错。逐条钉死后，根因全部落在
**进程全局状态**上——每一条的判据都是「同一个全局在一次用例里被读了两次」：

| 全局 | 定义处 | 症状 | 处理 |
| --- | --- | --- | --- |
| 主题 `Theme::current()`（`AtomicU8`） | `xai-grok-pager-render/src/theme/cache.rs` | 产出时读一次、断言里再读一次，中间被别的用例改掉 | 用既有的 `pin_theme()` 守卫（顺带占住 `TEST_LOCK`，把 GrokNight 与 TrueColor 钉死） |
| 语音开关 `voice_mode_enabled()` | `app`（`set_voice_mode_enabled_for_test`） | 设置项行列表随开关增减，两个用例互相踩 | 两处加 `#[serial_test::serial(VOICE_GATE)]` |
| 模态嵌入（minimal）`modal_window::EMBEDDED` | `views/modal_window.rs` | 一次渲染落进别的用例把标志置真的窗口 | 比较两次渲染几何的用例补裸 `#[serial_test::serial]`（与置位用例同一把锁） |
| 项目选择器 `needs_project_picker()` | 分叉新增（`app_view.rs`） | `test_app()` 的 cwd 是 `/tmp`、不是项目目录，创建会话被改道到目录选择器，用例 `recv().await` **永久阻塞** | 用例里置 `project_picker_disabled = true`，沿用分叉既有先例 |

钉住的位置：主题 14 处（`scrollback/blocks/quote_bar.rs` 11 条引用栏选中用例、
`scrollback/blocks/thinking.rs` 1 条、`app/modals.rs` 命令面板光标 1 条、
`app/edit_highlight_worker.rs` 1 条）；语音 2 条；嵌入 4 条
（`views/settings_modal/tests.rs` 里比较两次渲染几何的四条）；选择器 2 条
（`app/event_loop.rs`）。

**主题那条**值得单说：`│` 引用栏的样式在解析时按 `Theme::current()` 上色，而
判断「这一行是不是引用栏」又在输出时再读同一个全局，两者不一致时整条引用前缀
就留在复制出来的文本里——症状是复制内容多出 `│ │`，看起来像引用栏的 bug，其实
是并发。`modal_window::EMBEDDED` 那条同样可以用算术钉死：100 列时 `list_area`
高度实测 14（正确值），150 列实测 22；后者只在嵌入模式下成立
（`inner.height` 29 而非 22），说明 150 列那次渲染撞上了标志为真的窗口。

### 2.2 偶发失败暴露的真实缺陷

- **会话候选枚举对并发删除不设防**。`xai-grok-shell` 的
  `session/storage/relocation/mod.rs::load_candidates` 对「会话根不存在」是容忍的
  （`NotFound` → 空表），对「cwd 桶在列举与遍历之间被删掉」却是硬报错
  （`read <桶路径>: No such file or directory`）。按标题恢复会话要列举所有 cwd 桶，
  于是桶一消失，恢复就整个失败——测试里表现为 `remote_miss_*` 三条随机挂掉，
  生产里则是别的 `chaos` 进程清理目录时把本次启动打掉。已把桶级与会话级的
  `NotFound` 按「没有内容可贡献」处理，其余 IO 错误仍旧 fail closed。

### 2.3 与实现长期不符的三条 Esc 用例

中段 Esc 的策略在本分叉落地于 `00323c34`（轮次运行时按 Esc 不再取消，只提示
Ctrl+C），但同在 `a5727c59` 进来的三条 pty 用例仍断言「Esc 取消轮次」。它们一直
`#[ignore]`，所以不进 CI，也就一直没人发现。按上游 `75810042` 改名并重写：
`esc_mid_turn_hints_ctrl_c_from_prompt_preserves_draft`、
`esc_mid_turn_hints_ctrl_c_from_scrollback`、
`minimal/minimal_esc_mid_turn_hints_ctrl_c`（`c181fd64`）。

断言改为「Esc 后出现提示、屏幕上没有取消标记、草稿仍在」，再用 Ctrl+C 完成取消，
与实现一致。提示文案 `Press Ctrl+c to cancel the turn` 本身仍是英文，属 §6
延后的代码侧英文界面面，故照上游原文断言，未夹带翻译。

## 3. 用户指南中文化

- 全书 27 篇正文、表格、目录条目均为中文；正文 H1、`docs.rs` 里的文档名与
  `README.md` 的目录条目统一为**同一个中文名**。
- 删除本分叉不存在的功能：Grove / `grok clone`、grok.com 浏览器登录与 OIDC、
  `docs/internal/*`、Terminal 主题（`theme = "terminal"`）。`/login`、`/logout`
  保留并按本分叉的**兼容桩**行为如实改写（源码模块注释说「未注册」是过期的，
  `slash/commands/mod.rs:151-152` 实际注册了它们）。
- 命令与路径按分叉实际改写：`chaos <cmd>`、`~/.chaos`；其余 `GROK_*`、
  `xai-grok-*`、`grok-<模型>`、`/etc/grok`、`grok.com` 保持原样。

## 4. 中文化机器校验

`scripts/check-doc-l10n.py`（1100 行）+ `scripts/check-doc-l10n-selftest.py`（51 条用例）
把下列内容做成不变量，翻译不可能悄悄改坏：

- `fences` 围栏代码块序列（`fork_normalize` 归一化改名，`fence_normalize`
  折叠行尾 `#` / `|` 前的对齐空白）
- `inline` 行内代码 span 多重集（**丢失**是漂移，除非在 `scripts/doc-span-removals.tsv`
  里声明，现 15 条；**新增**只提示）
- `tables` 每张表的声明列数与每个数据行的单元格数
- `links` `](目标)` 集合（**折算到译文 slug 后**比较，见下）、`headings` 每级标题数、
  `numbers` 正文数字多重集
- `--english`（散文行）与 `--cells`（表格单元格，`prose` / `short` / `mixed` 三档）
  合起来才是章节完整性的判据——`--english` 不扫表格行
- `--fork-names` 报告用上游旧名写的命令与配置路径，同时放行 `GROK_*`、
  `xai-grok-*`、`grok-<模型>`、`grok.com`、`/etc/grok`，并豁免含「兼容」/「上游」
  的整块（双读说明所在的段落与其正下方那张表）

**锚点比较按标题翻译归一化（本轮修）**：`links` 原先逐字比较 `](目标)` 集合。
可是标题一译成中文，GitHub 生成的 slug 就变了，指向它的入站锚点必然从
`#english-slug` 变成 `#中文-slug`——每条入站链接都会同时记成一次丢失加一次
新增。而 §七 收尾第 1 步恰恰是 `--fix-anchors`：**校验器规定的动作正是它自己
判为漂移的动作**，于是任何译过的章节永远「有漂移」，判据失去信号价值。

现在 `slug_mapping` 按**位置**建立单个文件的 旧 slug → 新 slug 表（翻译保持
标题数量与顺序），两侧目标都折算到译文 slug 后再比：跟着标题走的链接比较
相等，被改指、丢掉或指错文件的仍然报漂移；标题数量变了就退化为逐字比较，
并附一条 note 说明「这些文件的锚点是逐字比的，因为位置无法映射」，免得把
保守回退误读成内容损坏。`--fix-anchors` 与 `compare` 共用同一个 `slug_mapping`。

**`--fix-anchors` 多加右括号（本轮修）**：`LINK = r"\]\(([^)\s]+)"` 有意不含
右括号，替换串却又补了一个，15 篇 55 处被改成 `](a.md#锚点))`。`--links` 看不见
它（目标仍能解析，多出的括号只是链接之后的普通文本），只有渲染时看得出。
已修，并加了自测用例钉住。

**一个假通过的坑（已修）**：`--glob` 是**仓库根相对路径**，`expand()` 走
`Path().glob()`，所以 `--glob '10-hooks.md'` 一个文件也匹配不到。脚本原先对
「匹配到 0 个文件」静默返回空列表，于是所有计数都是空转的 0 —— 第 10 章的自检
就是这么被骗过去的（改用完整路径重扫，同一份文件立刻冒出 24 处上游旧名）。
现在 `expand()` 遇到空匹配**硬报错**，并给出该用的写法；约定 §七 也加了
「用已知非零的文件做正对照」的要求。

收尾顺序（**不可并发**，改标题会让别的文件里的入站锚点失效）：

1. `--fix-anchors --before <基线>` 按标题**位置**机械重写全部入站锚点；
2. `--links` 归零；
3. 全库 `--fork-names --strict` 归零（或只剩豁免的双读说明）；
4. `bash scripts/l10n-guard.sh --before main --after HEAD --report <dir>` → `regressed` /
   `shrunk` / `fortress-breach` 三个文件都是 0。

## 5. 全库漂移审计（本轮收尾）

§七 四道闸门全绿之后，还剩一笔「解释不了就不算完成」的账：`--before main
--after worktree` 扫全库 27 篇 + README，仍退出 1（19 篇有结构漂移、36 处可容忍的
新增）。这些究竟是翻译改坏的，还是上游本来就这么改的？判据是**第三个版本**——
`main` 落后上游 `a28ee2b2`，凡工作区与上游一致、而与 `main` 不一致的，都是上游
增量，不是本地损坏。

逐类归因（19 篇的每一条都有着落）：

| 判据 | 篇数 | 归因 |
| --- | --- | --- |
| 围栏代码块 | 8 | 7 篇与上游逐字节相同；01 那处是有意删掉的 `chaos clone` 段 |
| 行内 span 丢失 | 9 | 8 篇的工作区出现次数**等于上游**；01 的 2 处已声明 |
| 链接目标 | 12 | 锚点跟着标题译名走 / `CHAOS.md` 层级 / 24 修好 `main` 里本就断的锚点 |
| 表格形状 | 5 | 09 / 12 / 13 / 24 逐表与上游相同；26 少 `cli.grove` 一行 |
| 标题数 | 5 | 09 / 13 / 16 / 21 与上游相同；07 少 2 节，正是有意不移植的 MCP admission |
| 正文数字 | 6 | 新增的数字在上游各出现 1 次；13 的 `80`→`64` 也是上游改的 |

几条要单独记下的：

**行内 span 的判据是「次数」而不是「有无」**。一开始我只问「上游还有没有这个
span」，于是 13 的 `GROK_MEMORY` 这类都成了「上游仍有、分叉却丢了」。改成问「工作区
的出现次数是否等于上游」之后全部落地：13 的 `[memory]` 4→3、`false` 2→1、22 的
`allow` 19→18、26 的 `table` 17→16、14 的 `init` 11→10——工作区次数与上游**逐个
相等**，说明上游自己就少了一处，工作区是跟着上游走的。唯一不满足这条的是第 1 章的
`--full-history` 与 `[clone] enabled = true`（上游仍各 1 次，工作区 0 次）：它们随
「介绍 `chaos clone` 的整段」一起删除，属 §4.3 的有意删除，已补进
`doc-span-removals.tsv`（22 → 24 行、13 → 15 条声明）。

**链接的 12 篇分三类**。(1) 锚点跟着标题译名走——本该被新的归一化放行，没放行是因为
**目标文件自己**的标题数变了（07 / 09 / 21），位置映射不成立、退化为逐字比较，报告里
那句 `anchors ... compared verbatim` 的 note 就是它；(2) `CHAOS.md` 的相对层级由
`../../../../` 修成 `../../../../../`（01 / 02 / 05 / README），因为分叉的 `CHAOS.md`
挪了位置；(3) 24 把 `02-authentication.md#related-settings` 改成 `#相关文档`——
`main` 的 02 只有 7 个标题、**根本没有** `related-settings`，也就是说这条锚点在 `main`
里本来就是断的（上游的 02 有 27 个标题才有它），分叉顺手修好了。此外 01 删掉了指向
已删章节的 `27-grok-clone.md`。

**26 的 `[cli]` 表** 13 → 15 → 16 行：工作区补上了上游新增的 `cli.grove_worktree`、
`cli.nfs_worktree`，唯一没搬的上游行是 `cli.grove`——被删掉的那个功能。

**07 的标题数**：上游比 `main` 多 3 个三级标题，工作区只搬了 1 个
（`### 被组织策略拦截`），另外 2 个是 §4.3 明确不移植的 MCP admission 两节。

**结论**：19 篇无一例外可归因，没有一条是翻译改坏的。`--before main` 的漂移检查因此
**按设计仍然退出 1**——它只认 `main` 与工作区两个版本，不知道上游怎么改；§七 的闸门是
`--links` / `--fork-names` / `l10n-guard` 三项，不是「漂移归零」。

**没做但值得做**：给校验器加 `--upstream <ref>`，把「上游也这么改」的条目自动降级为
note（本轮 8 处 span 丢失与若干表格都属这一类），报告会更接近纯信号。本轮先把归因留档。

## 6. 遗留与不做的

- **不做**：`oniguruma` 2→3（延后到 Tier 2）、MCP admission 放宽（会改动
  架构语义）。
- **待用户拍板**：项目作用域 `.chaos` 不参与 agents/roles/personas/skills 解析
  （`xai-grok-config` §12.2 的漏网点）；`persona` 的译名在 `04`（角色）与
  `16`（人设）之间不统一。
- **未纳入本轮**：代码侧面向用户的英文界面文案。本轮清掉了快捷键详情页 45 条
  `long_help`（其中 3 条是 `stash` / `Stash / pop prompt draft` / `Shell` 这类短条目）、
  `app/modals.rs` 13 处与 `views/agents_modal.rs` 17 处弹窗底栏标签（`96224012`），
  加上此前的恢复/诊断类提示。**仍有残留**：欢迎页脚 chips（`views/welcome/mod.rs`
  的 `back` / `select` / `confirm delete` / `cancel` / `worktree` / `navigate` /
  `filter` / `delete`）、`views/memory_modal.rs`、`views/usage_modal.rs`、
  `views/tutorial.rs`、`views/import_claude_modal.rs`、`views/modal.rs`、
  `views/dashboard/render.rs`、`views/persona_detail.rs`、`views/workflows.rs`、
  `views/picker.rs`、`settings/defs.rs` 的部分标签（`'Confirm before rewind'`、
  `'Follow-up behavior'`、`'Copy and export'`、`'Voice shortcut'`）、会话创建问答的
  `'Yes'` / `'Delete'` / `'Cancel'` / `'Always worktree'`、`app/turn_completion.rs` 的
  `'Edit'` / `'Resend'` / `'Discard'`，以及 `diagnostics/fix.rs`、
  `views/extensions_modal.rs`、`memory_cmd.rs`，合计约 60–80 条、约 18 个模块。
  范围比指南正文大，建议单独一轮，且要先逐处判断哪些是**故意**留给英文的
  （键位名、命令名、`Execute` 这类术语）。
- **测试基建的已知隐患**（本轮只是消掉症状）：`xai_dirs::grok_home()` 是
  `OnceLock` 进程缓存，而 `GrokHomeFixture` 靠「设 `GROK_HOME` 环境变量」做隔离。
  缓存一旦被别的用例先播种（例如某个用例直接读真实家目录），夹具写的会话目录就
  落到真实的 `~/.chaos/sessions` 下，退出时又 `remove_dir_all` 删掉，于是与并发读
  的用例互相踩。§2.2 的生产侧容忍让读方不再因此报错，但夹具本身没改——真要根治
  得动 `xai-dirs` 的缓存机制（改成可重置），属架构层，本轮按约定不碰。
