# 用户指南中文化的进度与恢复点

更新于 2026-09-18，分支 `sync/curated-port-20260918`，基线 `88f55fb4` 之后。

## 已完成的提交

| 提交 | 内容 |
|---|---|
| `7d37319f` | 第 1 章 —— `20-background-tasks.md` |
| `88f55fb4` | 第 2 章 —— `19-plan-mode.md` |
| `f5e8c452` | 校验脚本补强 + 写作约定（`fork_normalize`、`--fork-names`、表格逐行、自测 14 例） |
| `1194ddb5` | 第 3 章 —— `12-project-rules.md` |
| `61b3a023` | 更正 `02-authentication.md` 关于 `/login` `/logout` 的错误断言 |
| `4d68280b` | 核对结论与代码侧遗留归档（`sync/doc-claims-verification.md`） |

已完成章节：`20`、`19`、`12`（另 `02` 已更正、`README` 本来就是中文）。

## 剩余工作量（按英文残留降序）

| 文件 | 行数 | 英文残留 | 上游旧名 |
|---|---:|---:|---:|
| `10-hooks.md` | 665 | 177 | 27 |
| `21-terminal-support.md` | 304 | 150 | 12 |
| `23-dashboard.md` | 312 | 136 | 2 |
| `22-permissions-and-safety.md` | 572 | 123 | 28 |
| `24-monitoring-usage.md` | 380 | 118 | 2 |
| `14-headless-mode.md` | 687 | 112 | 44 |
| `03-keyboard-shortcuts.md` | 478 | 108 | 0 |
| `18-sandbox.md` | 306 | 101 | 24 |
| `13-memory.md` | 485 | 87 | 25 |
| `16-subagents.md` | 401 | 87 | 8 |
| `04-slash-commands.md` | 470 | 84 | 6 |
| `09-plugins.md` | 451 | 82 | 45 |
| `17-sessions.md` | 392 | 79 | 35 |
| `05-configuration.md` | 834 | 71 | 40 |
| `07-mcp-servers.md` | 394 | 67 | 38 |
| `08-skills.md` | 234 | 47 | 21 |
| `11-custom-models.md` | 400 | 38 | 9 |
| `06-theming.md` | 346 | 35 | 3 |
| `25-status-line.md` | 143 | 30 | 9 |
| `15-agent-mode.md` | 339 | 27 | 11 |
| `01-getting-started.md` | 229 | 16 | 20 |
| `26-config-reference.md` | 686 | 16 | 12 |
| 合计 | 10347 | **1791** | **424** |

## 每章的执行协议（实测唯一稳定的做法）

派发**一个**子代理翻译**一个**文件，提示词里给：

1. 让它先读 `sync/doc-l10n-conventions.md`（写作约定）与目标文件；
2. 给一份**有序的小节名清单**（工作清单），逐项 `search_replace`；
3. 硬性要求每次 `search_replace` 的原文**不超过 30 行**；
4. 只允许跑两条门禁命令，不要跑构建；
5. 结束时报告：调用次数、改动行数、两条门禁的 stdout。

原因：单条回复有约 8k token 的输出上限，整篇 `write` 必截断
（`max_tokens_truncation`）。并发派发时约有一半的 run 会在**第一次调用**
就被上游中止（signature：`modelCalls: 1`、`outputTokens: 48–91`、
`apiDurationMs: ~3400`、`decodeDurationMs: 0`），与内容无关；串行派发
（第 3 章 `12`）一次成功，935 秒、25 次工具调用、0 报错。失败就重试。

## 收尾清单（全部章节译完后）

1. `python3 scripts/check-doc-l10n.py --fix-anchors --before <译前基线>`
   机械重写入站锚点（标题顺序不变，按序号映射，不需要猜）。
2. `python3 scripts/check-doc-l10n.py --links` 归零。
   现存两条待处理：
   - `12-project-rules.md` 的自链 `#supported-file-names`（标题已中文化，
     由第 1 步修掉）；
   - `14-headless-mode.md` 的 `02-authentication.md#device-code-flow`
     —— 该锚点在分叉里不存在（设备码流程已删），随 14 的登录小节一起重写。
3. `python3 scripts/check-doc-l10n.py --fork-names --strict` 归零
   （只剩合法双读说明，其段落必须含「兼容」二字）。
4. `python3 scripts/check-doc-l10n.py --english` 归零。
5. `python3 scripts/check-doc-l10n.py --before main --after HEAD --strict-spans`
   全库 0 漂移。
6. `bash scripts/l10n-guard.sh --before main --after HEAD --report <dir>`
   三份报告都是 0。
7. `cargo test -p xai-grok-shell --lib --features config-docs config_docs`
   仍通过（`26-config-reference.md` 是它的输入）。
8. 对照 `~/.chaos/docs/user-guide/` 的解包结果与仓库副本一致。

## 各章特有的写作要求

除 `sync/doc-l10n-conventions.md` 的通用约定外：

- `04`：`/memory` 不以 `enabled` 为闸；`/flush`、`/dream` 不要引用不存在的
  状态串（`/dream` 成功时无用户可见输出）；记忆浏览模态没有绑定 `s`；
  `/login` `/logout` 照兼容桩译；`/usage` 是三标签模态；
  `Account and Billing` 一节在本分叉没有计费内容，按 §4.3 处理标题。
- `10`：不需要行为注记（`GROK_SESSION_ID` 仍在注入，原文即正确）。
- `14`：`Authentication for Headless Environments` 整节按 BYOK 重写，
  删掉 `grok login --device-auth`、`grok login` 两条，指向
  `02-authentication.md`。
- `23`、`25`：核对结论为真，可照写。
- `01`：有一处 `~/.grok/AGENTS.md` 应是 `~/.chaos`。
- `06`：Terminal 主题在本分叉不存在，按 §4.3 删掉相关段落与
  `GROK_TERMINAL_THEME`、`[features] terminal_theme`。权威清单是
  `settings/defs.rs:38-69` 的 `THEME_CHOICES`：`auto` + 5 个具体主题
  `groknight`、`grokday`、`tokyonight`、`rosepine-moon`、
  `oscura-midnight`；前两个的**显示名已是** `Chaos Night` / `Chaos Day`
  （不是 GrokNight/GrokDay），文档要跟着写。
