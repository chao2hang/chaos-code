# 用户指南中文化的进度与恢复点

更新于 2026-09-18，分支 `sync/curated-port-20260918`，基线 `a82a27ea`。

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

已整章完成：`12`、`19`、`20`（`02` 已更正，`README` 标题行待译）。
`26` 的首节散文已译，表格短单元格已全库替换。

## 二、真实剩余工作量

`--english` 只扫散文行、不扫表格行，单看过它会把「散文已中文、表格全英文」
的章节误判成接近完成。下表是 `--english` 与 `--cells` 合起来的口径
（`--fork-names` 是同一批改动里顺带做的）。

| 章节 | 行数 | 字符 | 散文行 | 表格单元格 | 上游旧名 |
|---|---:|---:|---:|---:|---:|
| `10-hooks.md` | 665 | 57 204 | 177 | 52 | 27 |
| `26-config-reference.md` | 686 | 58 471 | 0 | 373 | 0 |
| `03-keyboard-shortcuts.md` | 478 | 28 835 | 108 | 123 | 0 |
| `14-headless-mode.md` | 687 | 41 464 | 112 | 75 | 44 |
| `05-configuration.md` | 834 | 44 495 | 71 | 75 | 40 |
| `22-permissions-and-safety.md` | 572 | 32 964 | 123 | 20 | 28 |
| `24-monitoring-usage.md` | 380 | 20 936 | 118 | 32 | 2 |
| `23-dashboard.md` | 312 | 14 905 | 136 | 20 | 2 |
| `21-terminal-support.md` | 304 | 13 414 | 150 | 0 | 12 |
| `16-subagents.md` | 401 | 20 065 | 87 | 39 | 8 |
| `13-memory.md` | 485 | 18 651 | 87 | 40 | 25 |
| `18-sandbox.md` | 306 | 18 437 | 101 | 17 | 24 |
| `09-plugins.md` | 451 | 26 801 | 82 | 28 | 44 |
| `04-slash-commands.md` | 470 | 23 245 | 84 | 5 | 6 |
| `17-sessions.md` | 392 | 20 560 | 79 | 0 | 35 |
| `07-mcp-servers.md` | 394 | 17 172 | 67 | 5 | 38 |
| `08-skills.md` | 234 | 13 265 | 47 | 18 | 21 |
| `25-status-line.md` | 143 | 17 227 | 30 | 31 | 9 |
| `11-custom-models.md` | 400 | 12 628 | 38 | 4 | 9 |
| `06-theming.md` | 346 | 14 528 | 35 | 17 | 3 |
| `15-agent-mode.md` | 339 | 12 851 | 27 | 25 | 11 |
| `01-getting-started.md` | 229 | 7 185 | 16 | 15 | 20 |
| `README.md` | 61 | 2 019 | 0 | 10 | 2 |
| 合计 | 10 089 | 536 744 | 1 775 | 1 024 | 412 |

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
双读兼容的那一段。

## 五、各章特有的写作要求

除 `sync/doc-l10n-conventions.md` 的通用约定外：

- `01`：有一处 `~/.grok/AGENTS.md` 应是 `~/.chaos`。
- `04`：`/memory` 不以 `enabled` 为闸；`/flush`、`/dream` 不要引用不存在的
  状态串（`/dream` 成功时无用户可见输出）；记忆浏览模态没有绑定 `s`；
  `/login` `/logout` 照兼容桩译；`/usage` 是三标签模态；
  `Account and Billing` 一节在本分叉没有计费内容，按 §4.3 处理标题。
- `06`：Terminal 主题在本分叉不存在，按 §4.3 删掉相关段落与
  `GROK_TERMINAL_THEME`、`[features] terminal_theme`。权威清单是
  `settings/defs.rs:38-69` 的 `THEME_CHOICES`：`auto` + 5 个具体主题
  `groknight`、`grokday`、`tokyonight`、`rosepine-moon`、
  `oscura-midnight`；前两个的**显示名已是** `Chaos Night` / `Chaos Day`。
- `10`：不需要行为注记（`GROK_SESSION_ID` 仍在注入，原文即正确）。
- `14`：`Authentication for Headless Environments` 整节按 BYOK 重写，
  删掉 `grok login --device-auth`、`grok login` 两条，指向
  `02-authentication.md`；该章还带着一条死锚点
  `02-authentication.md#device-code-flow`，随这一节一起重写掉。
- `23`、`25`：核对结论为真，可照写。
- `26`：断言的核对结论见 `sync/doc-claims-verification.md`；表格的
  `Details` 列是散文（373 个单元格），`Key`/`Type / Values`/
  `Requirements`/`Managed` 四列是字面量。

## 六、收尾清单（全部章节译完后）

1. `python3 scripts/check-doc-l10n.py --fix-anchors --before <译前基线>`
   机械重写入站锚点（标题顺序不变，按序号映射，不需要猜）。
2. `python3 scripts/check-doc-l10n.py --links` 归零。
3. `python3 scripts/check-doc-l10n.py --fork-names --strict` 归零。
4. `python3 scripts/check-doc-l10n.py --english` 归零。
5. `python3 scripts/check-doc-l10n.py --cells --strict` 归零。
6. `python3 scripts/check-doc-l10n.py --before main --after HEAD --strict-spans`
   全库 0 漂移。
7. `bash scripts/l10n-guard.sh --before main --after HEAD --report <dir>`
   三份报告都是 0。
8. `cargo test -p xai-grok-shell --lib --features config-docs config_docs`
   仍通过（`26-config-reference.md` 是它的输入）。
9. 对照 `~/.chaos/docs/user-guide/` 的解包结果与仓库副本一致。
