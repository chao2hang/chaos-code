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
- Markdown 链接目标 `](...)`，包括锚点
- 转义与实体：`&lt;`、`\|`、行尾双空格

**行内代码里的散文也不要翻。** 例如 `# Attach a file` 这种代码块注释按
第一条处理（保留），但 `（可选）` 这类正文括号要翻译。

## 二、标题

`01`、`02`、`README` 用中文标题，其余章节历史上保留了英文标题。
本轮统一为**中文标题**：

- `## Key Concepts` → `## 核心概念`
- `### Slash Commands` → `### 斜杠命令`
- 专有名词不译：`## Project Rules (AGENTS.md)` → `## 项目规则 (AGENTS.md)`；
  `### MCP Servers` → `### MCP 服务器`
- 章节标题里的锚点依赖：**改标题会改锚点**，所以同一轮内必须同步修正
  指向它的链接（`docs.rs` 与其它章节的 `](#...)`）。改完用
  `rg -n '\]\(#?[a-z0-9-]*' ` 逐个核对。

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

1. **二进制名 `chaos`**。命令写 `chaos <cmd>`，不写 `grok <cmd>`。
   包名仍是上游的 `xai-grok-pager-bin`（从源码构建时用
   `cargo build -p xai-grok-pager-bin --release`）。
2. **配置根**。正文统一写 `~/.chaos`，并在每章首次出现处点明兼容读取：
   > 配置根按 `$CHAOS_HOME` → `$GROK_HOME` → 已有 `~/.chaos` → 已有
   > `~/.grok` → 默认 `~/.chaos` 的顺序解析；旧用户可继续使用
   > `~/.grok/config.toml`。项目级同样双读 `.chaos/` 与 `.grok/`，
   > 同名时 Chaos 优先。详见仓库根 [CHAOS.md](../../../../CHAOS.md)。

   权威表述在 `CHAOS.md`；不要在这里另创一套。
3. **删除本分叉不存在的功能**，不要留占位或「上游有而此处没有」的说明：
   - Grove / `grok clone` / `grok grove` / `[cli] grove` / `GROK_GROVE` /
     `GROK_CLONE` 相关全部段落
   - grok.com 浏览器登录、OIDC、企业 SSO、设备码流程、`/login`、`/logout`、
     `~/.grok/auth.json`、订阅门墙
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
- 表格单元格内是散文时翻译；单元格内是字面量时保留。
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

见 `sync/doc-l10n-check.md` 中的机器校验脚本。至少确认：

1. 围栏代码块数量成对，且数量与改前一致。
2. 每章行内代码里的标识符集合与改前一致（用脚本比对，不靠肉眼）。
3. 表格行列数与分隔行 `| --- |` 数量一致。
4. 所有 `](...)` 目标仍然可达（站内链接的文件名存在、锚点在同文件内存在）。
5. 标题层级不跳级。
6. `bash scripts/l10n-guard.sh --before main --after HEAD --report <dir>`
   在 `regressed.txt` / `shrunk.txt` / `fortress-breach.txt` 上都是 0。
7. `cargo test -p xai-grok-shell --lib --features config-docs config_docs`
   仍通过（`26-config-reference.md` 是它的输入，表格结构不能破坏）。
