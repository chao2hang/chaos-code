# 用户指南中文化的进度与恢复点

更新于 2026-09-20，分支 `sync/curated-port-20260918`，基线 `a82a27ea`，
本文的提交清单与统计数字截至 `de2cba71`。

## 一、已完成

| 提交 | 内容 |
|---|---|
| `7d37319f` | 第 1 章 —— `20-background-tasks.md` |
| `88f55fb4` | 第 2 章 —— `19-plan-mode.md` |
| `f5e8c452` | 校验脚本补强 + 写作约定（`fork_normalize`、`--fork-names`、表格逐行） |
| `1194ddb5` | 第 3 章 —— `12-project-rules.md` |
| `61b3a023` | 更正 `02-authentication.md` 关于 `/login` `/logout` 的错误断言 |
| `4d68280b` | 核对结论与代码侧遗留归档（`sync/doc-claims-verification.md`） |
| `01029f2c` | 本文件：进度与恢复点 |
| `b99dca39` | `--cells` 表格校验 + `scripts/doc-cell-glossary.tsv` + 自测 33 例 |
| `a82a27ea` | 按词典机械替换 855 个表格短单元格 |
| `05a25b25` | 围栏内行尾注释的对齐按装饰处理 + 3 条自测 |
| `21ee6aa3` | `01-getting-started.md` 中文化 |
| `90d81e4e` | `06-theming.md` 中文化（Terminal 主题按 §4.3 删除） |
| `ddac8fc6` | 有心删除的行内代码字面量改为逐条声明（`scripts/doc-span-removals.tsv`） |
| `f945bd07` | 围栏内 ASCII 示意图的边框对齐按装饰处理 + 2 条自测 |
| `cea224fd` | `15-agent-mode.md` 中文化 |
| `9df3ea16` | `18-sandbox.md` 中文化 |
| `9f2c529c` | `08-skills.md` 中文化 |
| `6abb37a6` | `11-custom-models.md` 中文化（凭据解析一节按 BYOK 重写） |
| `cab54b74` | 句末的 `grok.` 也按改名归一化 + 3 条自测 |
| `37e5b55f` | `07-mcp-servers.md` 中文化 |
| `b9a0fcbc` | `25-status-line.md` 中文化 |
| `d4934338` | 点名上游的段落也算豁免 + `--fork-names` 自测 7 例 |
| `707899b4` | `19-plan-mode.md` 残留的 `~/.grok/sessions` 改回 `~/.chaos` |
| `6f92436a` | `13-memory.md` 中文化（四处错误断言按 §4.3 改写，`topics/` 声明删除） |
| `a660f513` | 刷新进度表，记下第 13 章的核对结论 |
| `35711ebb` | `04-slash-commands.md` 中文化（兼容桩与记忆闸按核对结论改写） |
| `de2cba71` | `17-sessions.md` 中文化（认证行按上游遗留处理，`refs/grok/…` 保持） |

已整章完成（散文行、表格单元格、上游旧名三项都归零）：`01`、`02`、`04`、
`06`、`07`、`08`、`11`、`12`、`13`、`15`、`17`、`18`、`19`、`20`、`25`。
本轮之前已提交的整章是 `12`、`19`、`20`；`02` 是更正而非翻译；`13`、`04`、
`17` 的核对结论分别见 `sync/doc-claims-verification.md` 第八、九、十节。
`26` 的首节散文已译，表格短单元格已全库替换；`README` 还剩链接表的 10 个单元格。

## 二、真实剩余工作量

`--english` 只扫散文行、不扫表格行，单看过它会把「散文已中文、表格全英文」
的章节误判成接近完成。下表是 `--english` 与 `--cells` 合起来的口径
（`--fork-names` 是同一批改动里顺带做的），统计于 `de2cba71`。

| 章节 | 行数 | 字符 | 散文行 | 表格单元格 | 上游旧名 |
|---|---:|---:|---:|---:|---:|
| `26-config-reference.md` | 685 | 58471 | 0 | 373 | 0 |
| `03-keyboard-shortcuts.md` | 477 | 28835 | 108 | 123 | 0 |
| `10-hooks.md` | 664 | 57204 | 177 | 52 | 27 |
| `14-headless-mode.md` | 686 | 41464 | 112 | 75 | 44 |
| `23-dashboard.md` | 311 | 14905 | 136 | 20 | 2 |
| `21-terminal-support.md` | 303 | 13414 | 150 | 0 | 12 |
| `24-monitoring-usage.md` | 379 | 20936 | 118 | 32 | 2 |
| `05-configuration.md` | 833 | 44495 | 71 | 75 | 40 |
| `22-permissions-and-safety.md` | 571 | 32964 | 123 | 20 | 28 |
| `16-subagents.md` | 400 | 20065 | 87 | 39 | 8 |
| `09-plugins.md` | 450 | 26801 | 82 | 28 | 44 |
| `README.md` | 60 | 2019 | 0 | 10 | 0 |
| 合计 | 5 819 | 361 573 | 1 164 | 847 | 207 |

## 三、执行顺序

一次派一个子代理译**一个**文件；不同文件可两路并发，同一文件必须串行。
顺序按「大而结构简单的先做」排，先建立译法与手感，后做术语密集的：

1. `08-skills`、`11-custom-models`（第一批，验证新的 `--cells` 闭环）
2. `06-theming`（要删 Terminal 主题）、`25-status-line`（断言的核对结论已归档）
3. `15-agent-mode`、`18-sandbox`
4. `07-mcp-servers`、`13-memory`
5. `04-slash-commands`、`17-sessions`
6. `01-getting-started`、`09-plugins`
7. `16-subagents`、`24-monitoring-usage`
8. `21-terminal-support`、`23-dashboard`
9. `05-configuration`、`22-permissions-and-safety`（大，可能要拆两次派发）
10. `03-keyboard-shortcuts`、`14-headless-mode`（大）
11. `10-hooks`、`26-config-reference`（最大，各拆 2–3 次派发，按小节区间）
12. `README`（等所有章节标题定稿后再译链接文字）

已做完：1（`08`、`11`）、2（`06`、`25`）、3（`15`、`18`）、4（`07`、`13`）、
5（`04`、`17`）、6 的 `01`。下一批是 `09-plugins`，再往后是 `16-subagents`
与 `24-monitoring-usage`。

## 四、每章的执行协议（实测唯一稳定的做法）

派发**一个**子代理翻译**一个**文件，提示词里给：

1. 让它先读 `sync/doc-l10n-conventions.md`（写作约定）与目标文件；
2. 给一份**有序的小节名清单**（工作清单），逐项 `search_replace`；
3. 硬性要求每次 `search_replace` 的原文**不超过 30 行**；
4. 只允许跑门禁命令，不要跑构建、不要跑 cargo；
5. 结束时报告：调用次数、改动行数、门禁命令的 stdout。

原因：单条回复有约 8k token 的输出上限，整篇 `write` 必截断
（`max_tokens_truncation`）。并发派发时约有一半的 run 会在**第一次调用**
就被上游中止（signature：`modelCalls: 1`、`outputTokens: 48–91`、
`apiDurationMs: ~3400`、`decodeDurationMs: 0`），与内容无关；串行派发
（第 3 章 `12`）一次成功。**模型必须显式指定 `BBLBB/glm-5.3`**：默认路由
会落到 `deepseek-flash`，那个账号返回 402 余额不足。

每章收尾跑这四条，然后才提交：

```sh
G=crates/codegen/xai-grok-pager/docs/user-guide
python3 scripts/check-doc-l10n.py --before HEAD --after WORKTREE --glob "$G/<本章>"
python3 scripts/check-doc-l10n.py --english --glob "$G/<本章>"
python3 scripts/check-doc-l10n.py --cells --strict --glob "$G/<本章>"
python3 scripts/check-doc-l10n.py --fork-names --glob "$G/<本章>"
```

前三条必须为 0；第四条剩下的必须是 §4.1 表里「原样」的那些，或明确讨论
双读兼容、或点名上游以说明「那不是我」的那一段。豁免按**段落**判定，认
「兼容」和「上游」两个词（脚本里的 `EXEMPT_LINE`）。

## 五、各章特有的写作要求

除 `sync/doc-l10n-conventions.md` 的通用约定外：

- `01`：有一处 `~/.grok/AGENTS.md` 应是 `~/.chaos`。
- `04`（已完成 2026-09-20）：`/memory` 不以 `enabled` 为闸，写的是「后端已
  配置即可用」；`/flush`、`/dream` 不引用不存在的状态串，且 `/dream` 写明成功
  时没有用户可见输出；记忆浏览模态的键位没写 `s`；`/login` `/logout` 照兼容
  桩译；`/usage` 写了三标签模态，并注明配置外部认证提供方时该命令会被隐藏；
  `Account and Billing` 改为 `## 账号与数据`。四条新结论见
  `sync/doc-claims-verification.md` 第九节。
- `06`：Terminal 主题在本分叉不存在，按 §4.3 删掉相关段落与
  `GROK_TERMINAL_THEME`、`[features] terminal_theme`。权威清单是
  `settings/defs.rs:38-69` 的 `THEME_CHOICES`：`auto` + 5 个具体主题
  `groknight`、`grokday`、`tokyonight`、`rosepine-moon`、
  `oscura-midnight`；前两个的**显示名已是** `Chaos Night` / `Chaos Day`。
- `10`：不需要行为注记（`GROK_SESSION_ID` 仍在注入，原文即正确）。
- `13`：四处断言与实现不符，已按 §4.3 改写，重译时别再「还原」：
  「一主题一文件」的 `topics/`（实际是每作用域一份 `MEMORY.md`，会话日志在
  `sessions/` 下、由 `/dream` 折回）、`From earlier sessions` 迁移、
  `/memory` 开关「下一回合注入索引」（`context_injected` 不复位）、首回合
  注入的是检索结果而非生成索引。`topics/` 是已声明的删除项（见
  `scripts/doc-span-removals.tsv`）。模态分栏阈值散文的 `64` 与代码的 `80`
  不一致，§五禁止改数字、`numbers` 门禁也无声明通道，**保持 64 不动**。
- `17`（已完成 2026-09-20）：`/session-info` 的认证行是**源码里活着的上游
  遗留**，会原样打印「Run `grok login` to use your SuperGrok subscription
  instead.」，所以照引英文并在同句说明那是上游屏幕文本、凭据改用
  `/provider`（`effects/mod.rs:4984-4996`）。`refs/grok/reclaimed/…` 保持
  上游命名，那是代码里的 ref 名字空间，不是残留。按标题恢复的四条规则、
  `-s` 与 `--fork-session` 的关系、`chaos sessions list` 的列、
  `costUsdTicks` 的 1e10 刻度都核对为真，逐条记在
  `sync/doc-claims-verification.md` 第十节。
- `14`：`Authentication for Headless Environments` 整节按 BYOK 重写，
  删掉 `grok login --device-auth`、`grok login` 两条，指向
  `02-authentication.md`；该章还带着一条死锚点
  `02-authentication.md#device-code-flow`，随这一节一起重写掉。
- `23`、`25`：核对结论为真，可照写。
- `26`：断言的核对结论见 `sync/doc-claims-verification.md`；表格的
  `Details` 列是散文（373 个单元格），`Key`/`Type / Values`/
  `Requirements`/`Managed` 四列是字面量。
- `26` 第 101 行那个 `mixed` note 要顺手重写：它讲的是 `-w` 的 `Grove`
  模式，而 `Grove` 是本分叉没有的功能（`xai-grok-config`、`app/cli.rs`
  里都没有这个字符串），按 §4.3 删掉相关表述。

## 六、收尾清单（全部章节译完后）

1. `python3 scripts/check-doc-l10n.py --fix-anchors --before <译前基线>`
   机械重写入站锚点（标题顺序不变，按序号映射，不需要猜）。
2. `python3 scripts/check-doc-l10n.py --links` 归零。`de2cba71` 时全库还有
   16 条死锚点，都是改标题造成的，随第 1 步一起机械修掉：`04` 的
   `#minimal-and-fullscreen` 与 `17-sessions.md#the-grok-usage-subcommand`、
   `06` 的 `#auto-theme-system-appearance`、`07` 的 `#cli-management` /
   `#example-configurations` / `#project-scoped-mcp-servers`、`08` 的
   `#viewing-skill-details`、`12` 的 `#supported-file-names`、`14` 的
   `02-authentication.md#device-code-flow` 与 `15-agent-mode.md#automation-and-sdks`、
   `17` 的 `15-agent-mode.md#session-config-options`、`18` 的
   `#custom-profiles` / `#platform-support`、`22` 的
   `15-agent-mode.md#automation-and-sdks`、`25` 的 `#available-data` /
   `#refresh-runs`。
3. `python3 scripts/check-doc-l10n.py --fork-names --strict` 归零。
4. `python3 scripts/check-doc-l10n.py --english` 归零。
5. `python3 scripts/check-doc-l10n.py --cells --strict` 归零。
6. `python3 scripts/check-doc-l10n.py --before main --after HEAD`
   全库 0 问题（`problems` 为空），且每一条 `note:` 都要能逐条讲清来历
   （新增的行内字面量是有意补的，例如 `01` 的 `/provider`、`06` 的三个枚举
   名、`13` 的 `MEMORY.md`、`17` 的 `CHAOS_HOME` 与 `~/.chaos`），讲不清的
   按漂移处理，不算通过。这里**不能**加 `--strict-spans`：那个开关会把
   **有意补的**字面量也算成漂移，而补字面量恰好是本轮允许的动作
   （理由见 `sync/doc-claims-verification.md` 第 10.2 节）。
7. `bash scripts/l10n-guard.sh --before main --after HEAD --report <dir>`
   三份报告都是 0。
8. `cargo test -p xai-grok-shell --lib --features config-docs config_docs`
   仍通过（`26-config-reference.md` 是它的输入）。
9. 对照 `~/.chaos/docs/user-guide/` 的解包结果与仓库副本一致。
