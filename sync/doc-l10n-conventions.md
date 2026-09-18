# 用户指南中文化约定

本文件是 `crates/codegen/xai-grok-pager/docs/user-guide/*.md` 的翻译规范。
`docs.rs` 用 `include_str!` 把这 27 篇打进二进制，首次启动解包到
`~/.chaos/docs/user-guide/`（见 `pager/src/docs.rs` 的 `USER_GUIDE`）。
改动这里等于改动用户看到的 `/docs`。

## 目标

整本指南的正文是中文。术语、键名、命令、路径、代码块保持原样。
「该中文的地方就中文，该英文的地方就英文」：**散文**用中文，
**标识符与字面量**不动。

## 一、必须逐字保留的内容

以下内容不得翻译、不得改大小写、不得改标点：

- 围栏代码块 ```` ``` ```` 内的全部内容，包括 shell 注释
- 行内代码 `` `...` `` 里的标识符：配置键（`vim_mode`）、枚举值
  （`chat_completions`）、环境变量（`GROK_MEMORY`）、路径
  （`~/.chaos/config.toml`）、命令（`/model`、`chaos -p`）、工具名
  （`read_file`）、键位（`Ctrl+O`）、模型 id
- 表格表头中作为**字面量**出现的列名（`Key`、`Flag`、`Value`）
  ——注意是**反引号里的**才算字面量：表头单元格 `| Key |` 是散文，译
  `| 键 |`；`` 用 `Key` 列 `` 这种带反引号的提及才保留。
- Markdown 链接目标 `](...)`，包括锚点
- 转义与实体：`&lt;`、`\|`、行尾双空格

**行内代码里的散文也不要翻。** 例如 `# Attach a file` 这种代码块注释按
第一条处理（保留），但 `（可选）` 这类正文括号要翻译。

**一处例外**：本分叉把二进制改名为 `chaos`、原生配置目录改名为
`.chaos`，所以 `grok <cmd>`、`~/.grok`、`.grok/`、`$GROK_HOME` 要按
§4.1 的对照表改写（其余 `GROK_*`、`xai-grok-*`、`grok-<模型>`、
`/etc/grok` 仍然逐字保留）。也就是说「行内代码里的字面量不动」有个
明确的改名白名单，不在表里的照旧。

## 二、标题

`01`、`02`、`README` 用中文标题，其余章节历史上保留了英文标题。
本轮统一为**中文标题**：

- `## Key Concepts` → `## 核心概念`
- `### Slash Commands` → `### 斜杠命令`
- 专有名词不译：`## Project Rules (AGENTS.md)` → `## 项目规则 (AGENTS.md)`；
  `### MCP Servers` → `### MCP 服务器`
- 章节标题里的锚点依赖：**改标题会改锚点**，所以指向标题的链接
  （本文件内的 `](#...)` 与其它章节里的 `](NN-xxx.md#...)`）**不要手工改**，
  一律留给收尾时 `scripts/check-doc-l10n.py --fix-anchors` 机械重写（见 §六）。
  手工改锚点会让 `links` 不变量当轮就报漂移，而且和 `--fix-anchors` 的
  序号映射打架。

## 三、术语表

沿用 `01`、`02`、`07` 已确立的译法：

| 英文 | 中文 | 说明 |
| --- | --- | --- |
| scrollback | 回滚区 | 主显示区 |
| prompt / composer | 提示框 / 输入框 | `composer` 指输入编辑器 |
| turn | 回合 | 一次用户请求到回复结束 |
| session | 会话 | |
| subagent | 子代理 | |
| skill | 技能 | |
| plugin | 插件 | |
| hook | 钩子 | |
| marketplace | 插件市场 | |
| permission | 权限 | |
| sandbox | 沙箱 | |
| memory | 记忆 | |
| project rules | 项目规则 | |
| plan mode | 计划模式 | |
| background task | 后台任务 | |
| status line | 状态栏 | |
| dashboard | 看板 | `/dashboard` |
| workspace | 工作区 | |
| worktree | 工作树 | |
| token / context window | token / 上下文窗口 | `token` 不译 |
| streaming | 流式 | |
| tool call | 工具调用 | |
| headless mode | 无头模式 | |
| minimal mode | 最小模式 | `--minimal` |
| slash command | 斜杠命令 | |
| model picker | 模型选择器 | |
| session picker | 会话选择器 | 键位是 `F3` |
| always-approve (YOLO) | 始终批准 | |
| fold / expand | 折叠 / 展开 | |
| provider | Provider | 不译，见 `02` |
| BYOK | BYOK | 不译 |

## 四、分叉本地化（本仓库与上游的差异）

本仓库是 `xai-org/grok-build` 的中文化分叉。译文按**本分叉的实际行为**写：

1. **二进制名 `chaos`**。命令写 `chaos <cmd>`，不写 `grok <cmd>`；
   包名仍是上游的 `xai-grok-pager-bin`（从源码构建时用
   `cargo build -p xai-grok-pager-bin --release`）。

   **改与不改的清单**（依据 `xai-dirs/src/lib.rs`、
   `xai-grok-config/src/paths.rs`、`pager/src/app/cli.rs`）：

   | 上游写法 | 本分叉写法 | 说明 |
   |---|---|---|
   | `grok inspect`、`grok -c`、`grok wrap` | `chaos inspect`、`chaos -c`、`chaos wrap` | 命令字，改 |
   | 单独一个 `grok` 指本程序 | `chaos` | 产品名，改 |
   | `~/.grok/config.toml` | `~/.chaos/config.toml` | 新装默认根，改 |
   | `.grok/rules/`、`.grok/config.toml` | `.chaos/rules/`、`.chaos/config.toml` | 项目级原生目录，改 |
   | `$GROK_HOME` | `$CHAOS_HOME` | **唯一**有 Chaos 孪生的环境变量 |
   | `GROK_MEMORY`、`GROK_APPEARANCE`、`GROK_API_KEY` … | 原样 | 其余 `GROK_*` 没有孪生，**不许改** |
   | `/etc/grok/` | 原样 | 系统级目录没改名，`system_config_dir()` 仍是它 |
   | `xai-grok-pager`、`xai-grok-shell` | 原样 | 包名 |
   | `grok-4.5`、`grok-3-mini` | 原样 | 模型 id |
   | `grok.com`、`auth.x.ai` | 原样 | 上游托管服务 |

   现存命令（`chaos <cmd>`，别自己造）：`agent`、`inspect`、`doctor`、
   `leader`、`mcp`、`plugin`、`memory`、`models`、`clients`、`sessions`、
   `usage`、`setup`、`export`、`trace`、`update`、`version`、
   `completions`、`worktree`、`du`、`dashboard`、`wrap`、`share`、
   `workspace`。`login` / `logout` 虽仍在 clap 枚举里，但分叉没有派发
   实现，按 §4.3 处理（删除相关段落，不要写成本分叉的功能）。

   改完用 `python3 scripts/check-doc-l10n.py --fork-names` 清点剩余项：
   它会把该改而没改的列出来，同时放过上表里「原样」的那些。

   **判据是「兼容」二字**：正文里出现 `~/.grok`、`.grok/`、`$GROK_HOME`
   的行，必须是在说明兼容/历史遗留（因此句中含有「兼容」），否则就是漏改。
   所以「历史路径 `~/.grok/auth.json` 属上游遗留，请勿依赖」要写成
   「属上游**兼容**遗留」；双读顺序那段按 §4.2 的措辞写全。
2. **配置根**。正文统一写 `~/.chaos`，并在每章首次出现处点明兼容读取：
   > 配置根按 `$CHAOS_HOME` → `$GROK_HOME` → 已有 `~/.chaos` → 已有
   > `~/.grok` → 默认 `~/.chaos` 的顺序解析；旧用户可继续使用
   > `~/.grok/config.toml`。项目级同样双读 `.chaos/` 与 `.grok/`，
   > 同名时 Chaos 优先。详见仓库根 [CHAOS.md](../../../../CHAOS.md)。

   权威表述在 `CHAOS.md`；不要在这里另创一套。
3. **删除本分叉不存在的功能**，不要留占位或「上游有而此处没有」的说明：
   - Grove / `grok clone` / `grok grove` / `[cli] grove` / `GROK_GROVE` /
     `GROK_CLONE` 相关全部段落
   - grok.com 浏览器登录、OIDC、企业 SSO、设备码流程、`~/.grok/auth.json`、
     订阅门墙、xAI 计费/额度页

   但 **`/login` 与 `/logout` 要留下**，它们是本分叉注册的**兼容桩**，不是
   上游行为，也不是「未注册」。照实写：

   | 命令 | 本分叉行为 | 依据 |
   |---|---|---|
   | `/login` | 不启动浏览器 OIDC，直接打开 `/provider` 面板（fail-closed） | `slash/commands/login.rs` |
   | `/logout` | 只打印一条提示，让你去改 `config.toml` / 用 `/provider` | `slash/commands/logout.rs` |

   两处源码的模块注释写着「Not registered in `builtin_commands()`」，但
   `slash/commands/mod.rs:151-152` 确实注册了它们——**注释是过期的，别照抄**。
   照抄会写出「`/login` 未注册」这种与代码相反的话（`02-authentication.md`
   一度就是这么错的）。
   - 上游专有的托管服务（`grok.com` 远端设置、xAI 计费/额度页）
   - Terminal 主题（`theme = "terminal"`）、`GROK_TERMINAL_THEME`、
     `[features] terminal_theme`。本分叉可选主题只有 5 个 + `auto`
   - 本分叉 `26-config-reference.md` 里没有的配置键
   - `docs/internal/*`（该目录在本仓库不存在）
4. **保留本分叉特有的行为**，不要为了贴近上游而改回：
   - 会话选择器是 `F3`（`/resume` 亦可），不是 `Ctrl+R`。`Ctrl+R` 在代理
     画面上是「重命名选中的代理」
   - 中局 `Esc` **不取消回合**，只提示改用 `Ctrl+C`；空闲时两次 `Esc`
     在 800ms 内清空提示框或打开回退选择器
   - 遥测默认关闭、内置模型目录为空、`remote_fetch` 默认 false
5. **不要引入登录/凭据依赖的表述**。认证一律指向
   `[Authentication](02-authentication.md)` 与 `CHAOS.md`。

6. **产品名是 Chaos**。描述本分叉自身行为时写 **Chaos**；只有明确对比上游时
   才写 Grok / Grok Build。原文里那个指代「本程序」的 "Grok"，译文一律作
   Chaos。既有先例：`02-authentication.md` 首行是 `# Authentication（Chaos）`，
   `03-keyboard-shortcuts.md` 开头是「Chaos TUI 快捷键参考」。

   但**字面量不跟着换**：`~/.grok/` 这种兼容读取的旧路径、`GROK_*` 环境变量、
   `GROK_HOME` 等按 §一 保留原样；而命令与二进制名按 §4.1 写成 `chaos`。
   一句话：**散文里的产品名是 Chaos，反引号里的字面量照旧。**

## 五、散文风格

- 第二人称「你」。`the agent` 译「代理」；句首不加「请注意」这类填充词。
- 保留原文的**信息密度**：不增删步骤、不改数字、不改默认值。
- 原文的破折号插入语、括号补充、`**粗体**` 用于强调时保留强调，
  但不要额外加粗。
- 长句拆成中文短句；不保留英文语序的定语从句。
- 表格单元格内是散文时翻译；单元格内是字面量时保留。判定与执行见 §八：
  「1–2 个单词」的单元格在词典里一次决定，「3 个及以上单词」的是散文，
  按章节就地翻译。
- 引用块（`>`）里的提示照译，但 `> **Note:**` 视作散文标题，译作
  `> **注意：**`。
- HTML 注释、`<!-- ... -->` 保留原样。
- 不改动行尾空格与围栏标记数量。

## 六、写文件的方式（执行约束）

模型单条回复有输出上限。实测约 8k token，**一次 `write` 覆盖整篇会截断，本次运行直接失败**
（`max_tokens_truncation`）。09-18 那次派发就是这么死的。

- 按小节翻译：每次 `search_replace` 只改一节（几十行），改完接着下一节；
- 仅当文件短于约 100 行时才允许整篇 `write`；
- 一旦察觉这条回复会很长，立刻收尾，把剩下的放到下一次调用。

同一条约束也是**跨文件**的：标题改中文会让别的文件里的入站锚点失效，
所以锚点修复不能和翻译并发做，必须留到最后由
`scripts/check-doc-l10n.py --fix-anchors` 统一机械重写。

## 七、每次提交前自检

机器校验脚本是 `scripts/check-doc-l10n.py`（它自己的测试在
`scripts/check-doc-l10n-selftest.py`，49 条用例覆盖每个不变量的
「该拦」和「该放」两个方向，改动脚本后先跑它）。翻译完一章，至少跑：

```sh
G=crates/codegen/xai-grok-pager/docs/user-guide
python3 scripts/check-doc-l10n.py --before HEAD --after WORKTREE --glob "$G/<本章>"
python3 scripts/check-doc-l10n.py --english --glob "$G/<本章>"
python3 scripts/check-doc-l10n.py --cells --strict --glob "$G/<本章>"
python3 scripts/check-doc-l10n.py --fork-names --glob "$G/<本章>"
```

三个都必须是 0（`--fork-names` 只报告、不失败；剩下的应当是上表里
「原样」的那些，或是明确讨论双读兼容的那一行）。此外：

1. 围栏代码块数量与改前一致，块内除命令名外逐字未动（改命令名会改名字长度，
   对齐用的行尾注释、ASCII 示意图的右边框都会跟着错位一列；这一列是装饰，
   脚本按归一化比较，`~/.grok` → `~/.chaos` 后请把块内的对齐重新调好）。
   归一化只认三种改名：独立成词的 `grok`（含句末的 `grok.`）、`~/.grok`、
   `.grok`。`grok.com`（上游服务）、`grok-cli` / `grok_com_figma`（协议里写死的
   字面量）、`grok-4.5`（模型名）、`GROK_*` 环境变量、以及路径段里的
   `grok`（`/etc/grok`、`/tmp/grok.log`）都不是本地二进制，别跟着改。
2. 行内代码里的标识符集合与改前一致（脚本比对，不靠肉眼）。
3. 表格声明列数与每个数据行的单元格数都不变。
4. 所有 `](...)` 目标仍然可达（文件名存在、锚点在同文件内存在）。
5. 标题层级不跳级。
6. `bash scripts/l10n-guard.sh --before main --after HEAD --report <dir>`
   在 `regressed.txt` / `shrunk.txt` / `fortress-breach.txt` 上都是 0。
7. `cargo test -p xai-grok-shell --lib --features config-docs config_docs`
   仍通过（`26-config-reference.md` 是它的输入，表格结构不能破坏）。
8. 行内代码 span **不许丢**。确需删除上游专有功能的表述（§4.3 的登录命令、
   OIDC/设备码标志）时，在 `scripts/doc-span-removals.tsv` 里加一行
   `span<TAB>理由` 声明它，校验会把这条丢失降级为「已声明的删除」note；
   没有声明的丢失仍是硬失败。**不要为了过门禁把字面量换写成另一个字面量**
   （那是「丢失 + 新增」，看起来像修好了，实际把标识符改错了）。

全部章节翻完后，再做一次收尾：`--fix-anchors` 机械重写入站锚点 →
`--links` 归零 → 全库 `--fork-names --strict`。

## 八、表格单元格与词典

`--english` **不扫表格行**（表格行是 `|` 开头，被判为结构化内容）。所以它
一个人报不出「散文全中文、表格全英文」的章节——`26-config-reference.md`
就是这样：`--english` 只剩 16 行，全文 57,850 字符里却只有 1.0% 是汉字。
表格那一半由 `--cells` 负责，两者合起来才是完整性判据。

`--cells` 按词数把单元格分成三档：

| 档 | 判据 | 处理方式 |
|---|---|---|
| `prose` | 去掉行内代码后 ≥ 3 个 ASCII 单词 | 就地翻译（子代理做） |
| `short` | 1–2 个 ASCII 单词 | 在 **`scripts/doc-cell-glossary.tsv`** 里决定一次 |
| `mixed` | 已含汉字但仍 ≥ 3 个英文词且有虚词 | 只提示，不算失败 |

`short` 档必须在词典里有条目，否则 `--cells --strict` 失败。词典一行一条
`英文单元格<TAB>中文`，值写 `=keep` 表示这是标识符/字面量/专名，保留英文：

```
array	=keep
Details	说明
`~/.chaos/config.toml` or `~/.grok/config.toml`	`~/.chaos/config.toml` 或 `~/.grok/config.toml`
```

规则：

- **整体匹配**：只有单元格文本与键完全相同时才替换，所以译文不会拼进更长
  的句子里；替换时保留单元格原有的前后空白，列宽不变。
- 词典条目自身也过机器校验（`--check-glossary`）：译文的行内代码集合、
  散文数字、链接目标必须与英文一致（命令改名按 §4.1 归一化后比较），
  不得含裸 `|` 或换行，必须含汉字。不满足的条目不会被采用。
- 因此写条目时不能凭空加反引号。要保留 `Key` 这样的字面量就写 `=keep`。
- 双语路径单元格（`.chaos/config.toml` or `.grok/config.toml`）只在**紧随
  其后的段落说明了兼容/双读**时才出现；`--fork-names` 按「块」豁免，一个块
  能继承它上面那个块的豁免，正是为这张表准备的。

```sh
python3 scripts/check-doc-l10n.py --check-glossary      # 校验词典
python3 scripts/check-doc-l10n.py --apply-cell-glossary  # 全库机械替换
python3 scripts/check-doc-l10n.py --cells --strict       # 清点未决
```

词典是**数据**，不是脚本：新增一个重复出现的短单元格，加一行即可，不要
在 27 个文件里各写一遍，也不要为此改脚本。
