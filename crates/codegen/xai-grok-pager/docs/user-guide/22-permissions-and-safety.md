# 权限与安全

控制 Chaos 可以访问什么、做什么：权限模式、allow/ask/deny 规则、钩子，以及可选的 OS 级沙箱。

- **模式（Modes）**决定 Chaos 多久请求一次批准（始终批准、auto、ask 等）。
- **规则（Rules）**决定在该基线之上，哪些工具被允许、被询问或被阻止。

---

## 权限模式

当 Chaos 编辑文件、运行命令或调用外部工具时，它可能会暂停以等待批准。权限模式控制这种情况发生的频率。

模式设定的是一条基线。allow、ask、deny [规则](#configuring-permissions)在任何模式之上仍然生效。

### 起点建议

| 场景 | 模式 |
| --------- | ---- |
| 交互式 TUI | Default（ask），或用 auto 减少弹窗并辅以后台检查 |
| 脚本、SDK、CI、代理服务器 | 始终批准；再加 [deny 规则](#configuring-permissions)或钩子做硬性限制 |

```bash
chaos -p "Run the tests" --always-approve
chaos agent --always-approve stdio
chaos agent --always-approve serve --bind 127.0.0.1:2419 --secret <token>
```

ACP 客户端可以在 `session/new` 上设置 `"_meta": { "yoloMode": true }`。见[代理模式](15-agent-mode.md#automation-and-sdks)。

### 可用模式

| 模式 | 不经询问即可运行的内容 | 最适合 |
| ---- | ------------------------ | -------- |
| `default`（**询问**） | 只读工具与内置只读 shell 命令 | 交互式日常使用 |
| `acceptEdits` | 文件编辑不弹窗 | 本地编码、事后审查 diff |
| `plan` | 为兼容而接受；门控式规划请用[计划模式](19-plan-mode.md) | Claude 兼容设置 |
| `auto` | 安全检查放行的工作；其余调用被阻止或上报 | 想减少弹窗的交互式会话 |
| `dontAsk` | 仅预先批准的工具与内置只读处理 | 严格的 CI 白名单 |
| `bypassPermissions`（**始终批准**） | 一般工具调用（`deny` 规则、钩子以及部分 shell `ask` 规则仍然生效） | 受信任的自动化与代理服务器 |

**始终批准（Always-approve）**是产品名称；配置与 Claude 兼容设置中可能以 `bypassPermissions` 指代同一模式。始终批准与 auto 互斥（同时请求时以始终批准为准）。

### 如何设置模式

**交互式 TUI：** `Shift+Tab` / `Ctrl+O`、`/always-approve` 或 `/auto`，或 `/settings`（[快捷键](03-keyboard-shortcuts.md)、[命令](04-slash-commands.md)）。

**命令行：**

```bash
chaos --always-approve -p "Run the test suite"
chaos --permission-mode auto
chaos agent --always-approve serve --bind 127.0.0.1:2419 --secret <token>
```

**配置：**

```toml
[ui]
permission_mode = "always-approve"   # or "auto", "ask", …
```

`.claude/settings.json` 中的 Claude 兼容 `defaultMode` 也受支持（见 [Claude 兼容设置](#3-claude-code-compatibility-claudesettingsjson)）。对当次进程而言，命令行覆盖配置。

### 始终批准

跳过普通权限弹窗，工具无需等待点击即可运行。`deny` 规则、钩子以及部分 shell `ask` 规则仍然生效。管理员可以锁定并禁用该模式（见下文）。

| 机制 | 示例 |
| --------- | ------- |
| 命令行 | `--always-approve`（别名 `--yolo`），或 `--permission-mode bypassPermissions` |
| 配置 | `[ui] permission_mode = "always-approve"` |
| 交互 | `/always-approve`, `Ctrl+O` |
| ACP | 在 `session/new` 上设 `_meta.yoloMode: true` |

#### 为始终批准加硬性限制

自动化场景继续使用始终批准，同时为绝不希望运行的路径或命令加上 deny 规则：

```toml
# project .chaos/config.toml
[ui]
permission_mode = "always-approve"

[permission]
deny = [
  "Bash(rm -rf *)",
  "MCPTool(sales__delete_*)",
]
```

```bash
chaos -p "Deploy the service" --always-approve --deny 'Bash(rm -rf *)'
```

Deny 永远压过 allow，也压过始终批准的正常放行。见[配置权限](#configuring-permissions)。

### Auto 模式

在许多工具调用运行之前先做检查，从而减少交互式弹窗。常规本地工作通常直接放行。分类器不愿自动放行的调用会弹出权限提示，由你允许或拒绝。在非交互式会话（`chaos -p`、未识别的 stdio）中，同一调用会失败并上报给模型（例如 `Auto mode blocked this action …`）。

对于必须不经交互批准就运行工具的自动化，请使用始终批准（需要硬性阻止时配合 deny 规则），而不是只用 auto。

### 禁用始终批准（管理员）

组织可以禁止通过命令行、TUI 或 `/always-approve` 启用始终批准。在 `requirements.toml` 中设置（用户级位于 `~/.chaos/`，或系统级位于 `/etc/grok/`，后者用户无法移除）：

```toml
[ui]
disable_bypass_permissions_mode = true
```

这把锁不要用 `permission_mode` 来做；那个键是一个可切换的默认值。`requirements.toml` 中的旧键 `[ui] yolo = false` 出于兼容同样会禁用始终批准。

Chaos 仍可从托管设置加载 Claude 风格的权限**规则**；始终批准则被上面展示的 `requirements.toml` 锁定。

---

## 一次工具调用如何被授权

当模型请求一个工具时，会按顺序进行下列检查：

1. **`PreToolUse` 钩子**。钩子可以在任何其他检查之前拒绝一次工具调用。允许调用的钩子并不会跳过下面的检查；它只是不拒绝而已。见 [10-hooks.md](10-hooks.md)。

2. **权限规则**（来自配置文件或 `--allow`/`--deny` 标志）
   - 命中的 `deny` 规则拒绝该调用。`deny` 压过其他所有规则。
   - 命中的 `ask` 规则会向你弹出提示，包括原本会被自动批准的文件读取、搜索与 shell 命令。
   - 命中的 `allow` 规则批准该调用。

3. **记住的授权**。你此前在弹窗中保存的按命令批准在这里生效，作用域为当前项目。已有的授权可以满足一条 `ask` 规则，而不必再次弹窗。[危险命令清单](#dangerous-commands)上的命令会重新弹窗，而不是套用记住的前缀。见[交互式批准](#interactive-approvals-and-where-they-persist)。

4. **内置自动批准**。只读工具和一组固定的只读 shell 命令不经提示直接运行（见下文）。

5. **提示策略**（由[权限模式](#permission-modes)设定）：向你提示、自动批准或自动拒绝该调用。

[始终批准](#always-approve)在第 2 步之后短路这条流水线：命中的 `deny` 规则、钩子以及匹配 shell 命令各片段的 `ask` 规则仍然生效，但不会查阅记住的授权（包括记住的「永不允许」条目），非 shell 工具上的 `ask` 规则也不再弹窗。

---

## 默认不弹窗的操作

下列操作在任何模式（包括 `dontAsk`）下都被视为只读、不经提示直接运行，除非命中的 `deny` 规则或钩子阻止了它们。`ask` 规则会强制为文件读取、搜索和 shell 命令弹出提示（见[一次工具调用如何被授权](#how-a-tool-call-is-authorized)）。

### 只读工具

- `read_file`
- `list_dir`
- `grep`（内容搜索）
- `web_search`
- `todo_write`
- `get_command_or_subagent_output` / `kill_command_or_subagent`（子代理控制）
- 调用技能

### 只读 shell 命令

拆分链式命令（按 `&&`、`||`、`;` 和管道）之后，下列命令作为主命令出现时会被识别为只读。这份清单按词边界匹配，因此 `ls` 不会匹配 `lsof` 或 `less`。（你自己的 `Bash(...)` 规则匹配方式不同；见[规则匹配参考](#rule-matching-reference)。）

**文件系统（只读查看）：**
- `ls`, `cat`, `pwd`, `date`, `whoami`, `hostname`, `uptime`, `ps`
- `head`, `tail`, `wc`, `sort`, `uniq`, `tr`, `cut`

**Git（只读）：**
- `git status`, `git branch`, `git log`, `git diff`, `git ls-files`, `git show`, `git rev-parse`
- `git blame`, `git describe`, `git merge-base`, `git shortlog`
- `git check-ignore`, `git check-attr`, `git cat-file`, `git ls-tree`, `git show-ref`, `git for-each-ref`, `git rev-list`, `git name-rev`, `git count-objects`

**搜索与检视：**
- `grep`, `rg`（不含 `rg --pre` / `rg --pre=…`，它们会为每个文件启动一个预处理器）

**Kubernetes（只读）：**
- `kubectl get`, `kubectl logs`, `kubectl describe`

> **注意：** `tee` 不在清单里，因为它可以把输入写入任意文件。`cargo check` 不在清单里，因为它会编译并运行仓库中的 `build.rs`、proc-macro 以及任何 `build.rustc-wrapper`（因此在 Ask 模式下会弹窗；Auto 模式仍可能把 `cargo` 启发式地当作项目代码运行器放行）。`sort --compress-program=…`（包括唯一长选项的缩写）、`git -c` / `--config-env` 覆盖，以及本地/worktree 配置安装了可执行钩子的 git 命令（`core.fsmonitor`、某个 `diff.*.command`/`textconv`/`external` 驱动，或 shell `alias.<safe-subcommand> = !…`），会把请求级别的下限抬高为弹窗而不是自动批准，除非用户已为那段完整脚本授权或启用了始终批准。

这些检查按片段逐一应用。在 `ls && rm -rf /` 这样的命令里，`ls` 片段被识别为只读，但 `rm` 片段不在清单上。在 `default` 模式下 `rm` 片段会弹窗；在 `dontAsk` 下则被拒绝。

---

---

## 配置权限

Chaos 从三种兼容来源读取权限规则。所有来源的规则合并为一个集合；一条规则的效果取决于它的动作（`deny` > `ask` > `allow`），与来自哪个文件无关。

### 权限规则放在哪里（作用域）

权限规则可以是全局的（所有项目）、项目级的（单个仓库），或项目中仅属于你个人的：

| 作用域 | 文件 | 是否与队友共享 |
|-------|------|-----------------------|
| 全局（所有项目） | `~/.chaos/config.toml` | 否 |
| 项目（已提交） | `<project>/.chaos/config.toml` | 是（提交进仓库） |
| 项目（个人） | `<project>/.claude/settings.local.json` | 否（gitignore 它） |
| 交互授权 | 由 Chaos 内部按项目存储 | 否 |

关于作用域的说明：

- Chaos 会从仓库根向下到你的工作目录，在每一级目录发现 `.chaos/config.toml`，因此子目录可以在仓库根的规则之上追加规则。
- 所有作用域的规则合并为一个规则集；`deny` > `ask` > `allow` 跨作用域生效，因此全局 `deny` 不能被项目 `allow` 覆盖。
- Chaos 没有原生的 `config.local.toml`。项目中个人的、未提交的规则请用 `.claude/settings.local.json`；Chaos 直接读取它（见 [Claude Code 兼容](#3-claude-code-compatibility-claudesettingsjson)）。
- 交互式的「始终允许」决定存储在仓库之外，作用域为该项目（见[交互式批准](#interactive-approvals-and-where-they-persist)）。

要在一个项目里免掉某条命令的弹窗，可在该项目的 `.chaos/config.toml`（或 `.claude/settings.json`）里加一条窄的 allow 规则：

```toml
[permission]
allow = ["Bash(cargo test *)", "Bash(npm run build)"]
```

这样只批准列出的命令。相比之下，始终批准模式会批准所有工具调用。

### 1. 命令行标志

```bash
chaos -p "Review the API changes" \
  --allow 'Bash(git *)' \
  --allow 'Bash(gh *)' \
  --allow 'Read' \
  --allow 'Grep' \
  --deny 'Bash(rm -rf *)'
```

`--allow RULE` 和 `--deny RULE` 可以重复给出，并且始终会被强制执行。

规则语法示例：
- `Bash(git *)` — 任何以 `git ` 开头的命令
- `Bash(npm run build)` — 精确命令（或前缀）
- `Bash(git commit:*)` — `cmd:*` 后缀形式，等价于对 `git commit` 做前缀匹配
- `Read(src/**)` — `src/` 之下的读取权限
- `Edit(**/*.rs)` — 编辑任何 Rust 文件
- `Grep` — 所有 grep 操作
- `MCPTool(my-server__*)` — 来自某个特定服务器的 MCP 工具

精确的匹配语义（包括链式命令与通配符如何求值）见[规则匹配参考](#rule-matching-reference)。

### 2. 原生配置（`~/.chaos/config.toml` 与 `.chaos/config.toml`）

```toml
[permission]
rules = [
  { action = "allow", tool = "bash", pattern = "git *" },
  { action = "allow", tool = "bash", pattern = "gh *" },
  { action = "allow", tool = "read" },
  { action = "allow", tool = "grep" },
  { action = "deny",  tool = "bash", pattern = "rm -rf *" },  # block a dangerous pattern
  { action = "ask",   tool = "edit" },
]
```

结构化的 `tool` 字段接受小写名称 `bash`、`read`、`edit`、`grep`、`mcp`、`webfetch`、`websearch`，对应[工具名称](#tool-names)里的工具类别。

由于 `deny` 永远胜出，你不能把这些 `allow` 规则与针对 `bash` 的一网打尽式 `deny` 组合成「只允许 git/gh」；一条 `deny tool = "bash"` 规则会把 `git` 和 `gh` 也一并阻止。要默认拒绝，请在 `.claude/settings.json` 里用 `defaultMode: "dontAsk"`，或使用 `PreToolUse` 钩子（见下文）。

全局 `~/.chaos/config.toml` 与每个项目 `.chaos/config.toml`（从仓库根到你的工作目录）的规则会连同任何 `.claude/settings.json` 规则一起合并为一个规则集。

组织部署的托管配置也会贡献 `[permission]` 规则：系统级 `/etc/grok/managed_config.toml`，以及 Chaos 自动维护在 `~/.chaos/managed_config.toml` 的用户级副本。托管规则像任何其他来源的规则一样合并，但托管 `allow` 规则有两个特有属性：你自己的 `deny` 和 `ask` 规则压过托管 `allow`（按严重度排序），并且在始终批准被锁定关闭时，一网打尽式的托管 `allow` 会被忽略。要写出用户改不掉的规则，请使用 root 拥有的系统级 `/etc/grok/requirements.toml`。

每个来源的权限规则在会话启动时读取一次。修改会在下一个会话生效。

原生 `[permission]` 段也接受紧凑的 `allow` / `deny` / `ask` 字符串数组形式，使用与 `--allow` / `--deny` 标志和 `.claude/settings.json` 相同的规则字符串：

```toml
[permission]
deny = [
  "Read(/Users/you/private/**)",
  "Edit(/Users/you/private/**)",
  "Bash(rm -rf *)",
]
allow = [
  "Bash(git *)",
  "Bash(gh *)",
]
```

`deny` 永远压过 `allow`（求值顺序为 `deny` > `ask` > `allow`），与书写顺序或来源无关。若还想在 OS 层面阻止读取项目之外的路径，可把 deny 规则与 `strict` 沙箱 profile 组合使用（见 [18-sandbox.md](18-sandbox.md)）。

### 3. Claude Code 兼容（`.claude/settings.json`）

Chaos 读取 `~/.claude/settings.json` 和 `~/.claude/settings.local.json`，以及项目级 `<project>/.claude/settings.json` 与 `settings.local.json`（向上查找到仓库根为止）。权限规则的原生 `.chaos` 来源是 `config.toml`，见上一节。

示例：

```json
{
  "permissions": {
    "defaultMode": "dontAsk",
    "allow": [
      "Read",
      "Grep",
      "Bash(git *)",
      "Bash(gh *)"
    ],
    "deny": [
      "Bash(rm -rf *)"
    ]
  }
}
```

支持的 `defaultMode` 取值包括 `default`、`auto`、`acceptEdits`、`bypassPermissions`、`dontAsk` 和 `plan`。Chaos 从 `permissions` 之下的规范位置读取 `defaultMode`；当嵌套键缺失时，也接受顶层的 `defaultMode`。

`permissions.allow`、`permissions.deny` 和 `permissions.ask` 条目会被翻译成原生规则，再按[规则匹配参考](#rule-matching-reference)的语义匹配。翻译说明：

- MCP 工具的规则既可以用 `.claude/settings.json` 文件里的 `mcp__server__tool` 形式，也可以用原生的 `MCPTool(server__tool)` 形式（见 [MCP 规则](#mcp-rules)）。
- 命名了无法识别的工具的规则，以及 `Agent(model:opus)` 这类参数规则，会被跳过并给出警告，而不是让加载失败。
- `permissions.additionalDirectories` 会被解析但不被支持。

你可以用 **Ctrl+I**（「导入 Claude 设置」）交互式导入现有 Claude 设置。

---

## 规则匹配参考

本节精确定义规则如何匹配。

### Bash 规则

一条 `Bash(...)` 模式按以下两种方式之一匹配一条命令（对 `allow` 规则而言是每个链式片段——见下文「链式命令」）：

- **前缀**：命令以模式文本开头，逐字符比较。没有词边界要求，因此 `Bash(git)` 既能匹配 `git status` 也能匹配 `gitleaks`。要让前缀必须是完整单词，请带上尾随空格和通配符（`Bash(git *)`）。
- **Glob**：模式作为 glob 匹配整条命令（或整个片段）。`*` 可以出现在任意位置并匹配任意字符，包括空格和斜杠，因此 `Bash(git * main)` 能匹配 `git checkout main`。也支持 `?` 和 `[...]`。

匹配区分大小写。命令的前导空白在匹配前会被去掉。对 `deny` 和 `ask` 规则而言，原始命令串此外不做归一化；片段级检查还会额外匹配归一化后的形式（见下文）。

Bash 规则上尾随的 `:*` 后缀会被剥成普通前缀：`Bash(git commit:*)` 变成前缀 `git commit`。由于前缀没有词边界，写成 `Bash(sed:*)` 的 `deny` 也会阻止 `sed-custom` 之类的命令。

**链式命令。** Chaos 像 shell 一样解析每条命令，并按 `&&`、`||`、`;`、`|` 和换行拆分。规则动作对各片段的处理不同：

- `deny` 和 `ask` 规则针对每个片段以及整串命令检查。任何一个片段被拒绝，整条命令即被拒绝。
- `allow` 规则是合取的：只有**每个**片段都独立命中某条 allow 规则时，命令才因规则被自动批准。`Bash(git *)` 批准 `git status && git diff`，但不批准 `git status && rm -rf /` —— `rm` 片段没有命中任何 allow 规则，于是命令落入该模式的正常处理（`default` 模式下弹窗；`auto` 模式下交给分类器，它仍可能批准或阻止；`dontAsk` 下拒绝）。因此单条 allow 规则永远无法批准一条夹带了无关命令的链。

> **allow 规则不是封闭的白名单。** 未命中任何 allow 规则的命令并不会因此被拒绝——它落入模式处理。在 `auto` 模式下，分类器可以批准你的规则从未提及的命令。要默认拒绝的策略，请用 `dontAsk`（或始终批准加 `deny` 规则做硬性阻止），如[配置权限](#configuring-permissions)所述。

无法拆分成简单片段的命令（子 shell、命令替换 `$(...)`、反引号、后台 `&`、控制流）在配置了 Bash 限制时作为一个整体弹窗。

每个片段在规则匹配前会被归一化。`RUST_LOG=debug` 这类前导环境变量赋值会被剥掉，一组固定的包装器（`timeout`、`nice`、`ionice`、`chrt`、`stdbuf`、`env`）会被剥离，使规则匹配内层命令：`Bash(npm test *)` 批准 `RUST_LOG=debug timeout 30 npm test --workers=4`。这适用于 `deny`、`ask`、`allow` 规则、记住的授权以及只读命令清单。

还有一些匹配细节：

- 规则同样适用于传给 `bash -c` 的字面脚本内部。对 `allow` 而言，该脚本内部的每条命令本身都必须被允许。
- 不在清单上的包装器（`sudo`、`xargs`、`nohup`、…）不会被剥离。请写出显式点名它们的规则。
- 当解析器无法安全剥离某种形式（例如 `env -S`）时，命令会弹窗，而不是匹配某条 `allow` 规则。
- 匹配看到的是解析后以单个空格连接的词，不含 shell 引号。请针对不带引号的命令书写模式。

### 危险命令

一份内置清单（`rm`、`chmod`、`chown`、`chgrp`、`chattr`、`pkill`、`kill`、`killall`、`git push`）上的命令即使片段已被记住的命令前缀或只读命令清单覆盖，仍会弹窗。配置中显式的 `allow` 规则确实可以批准它们，始终批准模式也会像对待其他命令一样自动批准；要无条件阻止它们，请用 `deny` 规则。把 `Bash(rm *)` 这类规则加入 allow 之前请仔细审查。

### Read、Edit 与 Grep 规则

路径模式是 glob，匹配经过词法归一化之后的工具路径（折叠 `.`/`..`；相对路径与会话工作目录拼接）。以 `~` 开头的工具路径按字面匹配——绝不与工作目录拼接——因为工具只在权限检查之后才把 `~` 展开为家目录：

- `*` 和 `?` 不跨越 `/`；`**` 可以。`Read(src/*)` 匹配 `src/main.rs` 但不匹配 `src/nested/mod.rs`；要覆盖整棵树请用 `Read(src/**)`。
- 裸文件名只匹配那个精确字符串。要匹配任意深度的 `.env` 请用 `**/.env`。
- 没有锚定前缀：模式里开头的 `//` 或 `~/` 会被当作字面 glob 文本。请改写绝对路径模式或 `**/` 模式。
- 由于 `.`/`..` 在匹配前被折叠，有根模式无法靠路径穿越绕开：`Read(./**)` 限定在工作目录内（`src/main.rs` 这类裸相对路径匹配；`./../../etc/passwd` 不匹配），`Read(src/**)` 保持在 `src/` 之下。无根模式（`*`，或以 `**` 开头如 `**/*.rs`）有意在任意深度、任意位置匹配。
- `Read` 规则同样管辖 `grep` 搜索；`Grep(...)` 规则只匹配 grep。
- 原生 Read/Edit/Grep 检查在 deny 与 ask 上会跟随解析目标内的路径内符号链接。只匹配解析目标的 allow 并不为工具参数授予 allow。
- 无法解析的路径内符号链接在任一 deny 或 ask 文件规则适用于该工具时弹窗。

`Read` 和 `Edit` 的 deny 规则还适用于 shell 命令触及的文件路径（例如对被拒绝路径上的 `cat` 或 `sed`），包括以 `-c` 传给 `bash`、`sh`、`dash`、`zsh` 或 `ksh` 的字面内联脚本。shell 级检查使用与上文直接 Read/Edit/Grep 工具相同的工作目录感知归一化以及 deny/ask 的符号链接跟随（工作目录之下的绝对操作数同样命中 `Read(src/**)` 这类有根规则）。要覆盖每个进程的 OS 级强制，请把 deny 规则与沙箱组合（[18-sandbox.md](18-sandbox.md)）。

### MCP 规则

`MCPTool(...)` 模式匹配 `server__tool` 形式的完整 Chaos 工具名，支持 glob：`MCPTool(linear__*)` 匹配 `linear` 服务器上的所有工具。Chaos 工具名不带 `mcp__` 前缀。

`.claude/settings.json` 文件里使用的 `mcp__` 规则拼写也被接受，并改写到同一匹配器：`mcp__linear`（`linear` 服务器上的所有工具）、`mcp__linear__get_issue`（单个工具）、`mcp__linear__*`（该服务器上的所有工具）和 `mcp__*`（所有 MCP 工具）。

### WebFetch 规则

- `WebFetch(domain:example.com)` 匹配该主机及所有子域（`api.example.com`），不区分大小写，并忽略开头的 `www.`。`domain:` 模式内部不支持通配符。
- 不带 `domain:` 前缀的模式对整个 URL 做 glob 匹配：`WebFetch(https://api.example.com/*)`。

### 工具名称

可识别的工具名称：`Bash`、`Read`、`Edit`（及 `Write`）、`Grep`（及 `Glob`）、`MCPTool`、`WebFetch`、`WebSearch`。裸 `*` 规则匹配所有工具。工具名位置不支持 glob。

命名了无法识别的工具的规则（例如 `Agent(model:opus)`）会被跳过并给出警告，而不是让加载失败。

### 求值顺序

每个来源的规则合并为一个集合，按严重度而非顺序求值：任何命中的 `deny` 拒绝；否则任何命中的 `ask` 弹窗；否则任何命中的 `allow` 批准。没有规则命中时，请求落入内置自动批准，再到提示策略，如[一次工具调用如何被授权](#how-a-tool-call-is-authorized)所述。

---

## 交互式批准及其持久化位置

当一次工具调用需要批准时，权限提示提供以下选项：

- **允许一次**：仅批准这一次调用。
- **拒绝一次**：拒绝它，可选择附带一条回给模型的消息。
- **启用始终批准模式**：批准此后所有工具调用，而不只是当前提示的这一条。
- **本次会话允许所有编辑**：仅对文件编辑显示。这份授权只保存在内存里，重启后不保留。

### 按命令「始终允许」

更窄的一组选项只记住当前提示的具体命令、MCP 工具或 web-fetch 域名，例如「始终允许 `cargo test`」。这些选项默认开启。可用下面方式关闭：

```toml
# ~/.chaos/config.toml
[ui]
remember_tool_approvals = false
```

组织可以通过 `requirements.toml` 或托管配置中的同一键禁用它们。开关开启（默认）时，提示会多出：

- **`Always allow: <command>`**，为该命令前缀持久化一条 allow。
- 配套的「永不允许」选项，以同样方式持久化一条 deny。
- MCP 工具与 web-fetch 域名的等价「始终允许」和「永不允许」选项。「永不允许」永远只记住被提示的那个精确工具（从不是整个服务器）或精确域名；记住的 deny 压过任何授权，被拒绝的域名也覆盖其子域。

被记住的前缀仅限命令的短形式：只读命令只保留其清单形式的前缀（例如 `git status`，而不是完整参数列表），其他命令保留一个较短的开头前缀。提示会在你确认之前显示将要记住的确切内容。

[危险命令清单](#dangerous-commands)上的命令（例如 `git push` 和 `rm`）从不认记住的*前缀*：只有针对整条命令的精确授权才算数，因此它们的「始终允许」选项默认作用于完整命令。批准它只会免除那次精确调用的弹窗；换任何不同参数都会再次弹窗。当没有任何可记住的授权能阻止一个脚本再次弹窗时——例如危险命令前加了 `env` 前缀，或链中其余步骤仍需批准——「始终允许」选项索性不予显示，而不是保存一条不会生效的规则。

### 持久化按项目生效

交互式授权存储在你家目录下 Chaos 自己的状态目录中，作用域为你启动 Chaos 的 git 仓库（其仓库根），因此在仓库根接受的授权同样适用于从同一仓库子目录启动的会话。在 git 仓库之外，授权的作用域是启动目录；每个 git worktree 保留自己的授权。一个项目里的授权绝不会在另一个项目生效；授权不会写进仓库，也不应手工编辑。

要检视或重置某个项目的授权，请打开 Chaos 主目录（你家目录下的 `.chaos` 目录，或 `$GROK_HOME`）的 `sessions` 子目录：其中每个项目目录（URL 编码的作用域根）持有一份 `permission.toml`（外加按客户端的 `permission_<client>.toml` 变体），列出被记住的命令前缀、glob、MCP 工具/服务器、web-fetch 域名和「永不允许」条目。删除该文件即重置该项目的授权；下一次匹配的工具调用会再次弹窗。请把它当作只读状态——要*新增*规则，请改用声明式的 `[permission]` 配置。

交互式授权是个人、按机器的状态。要得到能在代码评审中审查、能与队友共享的白名单，请改用项目 `.chaos/config.toml` 里的声明式规则。

---

## 用钩子把 Bash 限制到特定命令

一个 `PreToolUse` 钩子可以在 `Bash` 工具上强制一份允许清单，并在每种权限模式下都生效。钩子在权限系统之前求值；钩子 deny 会阻止调用，钩子 allow 则落入正常的权限检查（因此你的 `deny` 规则仍然生效）。

> **注意：** 钩子是失败放行的。如果钩子脚本崩溃、超时或缺失，工具调用会像钩子允许了它一样继续进行，失败会在 UI 中报告。把钩子用作安全边界时，它必须自行处理错误，并且必须考虑链式命令，如下例所示。见 [10-hooks.md](10-hooks.md)。

### 示例：只允许 `git` 和 `gh`

**`~/.chaos/hooks/git-gh-only.json`**

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "git-gh-only.sh",
            "timeout": 5
          }
        ]
      }
    ]
  }
}
```

**`~/.chaos/hooks/git-gh-only.sh`**

```bash
#!/bin/sh
# Allow only git and gh commands, including within chained commands.

set -eu

deny() {
  echo '{"decision": "deny", "reason": "'"$1"'"}'
  exit 2
}

INPUT=$(cat)
CMD=$(echo "$INPUT" | jq -r '.toolInput.command // empty')

[ -n "$CMD" ] || deny "Empty command is not allowed"

# Normalize '&&' and '||' to ';' so chains can be checked segment by
# segment, then reject constructs this script cannot inspect.
CMD=$(echo "$CMD" | sed 's/&&/;/g; s/||/;/g')
case "$CMD" in
  *'$('*|*'`'*|*'&'*|*'>'*|*'<'*) deny "Substitution, background, and redirection are not permitted" ;;
esac

# Split on the separators and require every segment to start with git or gh.
echo "$CMD" | tr ';|' '\n\n' | while IFS= read -r SEGMENT; do
  SEGMENT=$(echo "$SEGMENT" | sed 's/^[[:space:]]*//')
  [ -n "$SEGMENT" ] || continue
  case "$SEGMENT" in
    git\ *|git|gh\ *|gh) ;;
    *) deny "Only git and gh commands are permitted. Blocked segment: $SEGMENT" ;;
  esac
done
```

```bash
chmod +x ~/.chaos/hooks/git-gh-only.sh
```

这个钩子会拒绝所有 `Bash` 命令，除非每个链式片段以 `git` 或 `gh` 开头；它还因为无法核实命令替换、后台与重定向执行了什么，而把它们一概拒绝。它在每种权限模式下都有效。

钩子的安装、JSON 格式、项目钩子的信任模型以及其他事件，见 [10-hooks.md](10-hooks.md)，其中还有一个互补的「阻止危险模式」示例。

---

## 示例配置

### 只用 git 和 gh 的无头模式（CI 与自动化）

```bash
chaos -p "Implement the feature using only git and GitHub CLI" \
  --allow 'Read' \
  --allow 'Grep' \
  --allow 'Bash(git *)' \
  --allow 'Bash(gh *)'
```

安装上面的 `git-gh-only` 钩子，以拒绝其他所有 `Bash` 命令。要对所有工具默认拒绝，还要在 `.claude/settings.json` 里设置 `{"permissions": {"defaultMode": "dontAsk"}}`。

### 只读代码审查者

```toml
# .chaos/config.toml
[permission]
rules = [
  { action = "allow", tool = "read" },
  { action = "allow", tool = "grep" },
  { action = "deny",  tool = "edit" },
  { action = "deny",  tool = "bash" },
]
```

### 交互式开发

使用 `default` 模式，再为你最常运行的命令（`git`、`cargo test`、`rg` 等）加窄的 `Bash(...)` allow 规则。

---

## 与沙箱组合

权限控制的是**模型**被允许请求什么。OS 级沙箱（见 [18-sandbox.md](18-sandbox.md)）控制的是**进程**在命令被批准之后还能做什么。

针对不受信任代码的推荐组合：

1. `dontAsk` 加窄的 allow 规则，或一个限制性的钩子
2. `--sandbox strict` 或自定义 profile
3. 项目信任，外加对任何 `SessionStart` 钩子的审查

---

## 在 TUI 中管理权限

- 权限决定会出现在会话记录里。
- `/always-approve` 命令切换始终批准模式；其他模式通过 `defaultMode` 设置（见[如何设置模式](#how-to-set-the-mode)）。
- 权限提示包含按命令的「始终允许」选项，仅对当前项目持久化（默认开启；用 `[ui] remember_tool_approvals = false` 关闭）。见[交互式批准](#interactive-approvals-and-where-they-persist)。
- 要管理钩子与插件，请运行 `/hooks` 或 `/plugins`（在多数终端上，**Ctrl+L** 也会打开扩展面板；在 VS Code、Cursor、Windsurf 和 Zed 上，`Ctrl+L` 是回合中插话）。见 [10-hooks.md](10-hooks.md)。

---

## 最佳实践

1. **优先窄模式。** `Bash(git *)` 授予的权限比裸 `Bash` allow 规则小。
2. **组合多层。** `dontAsk`、窄 allow 规则、限制性钩子和沙箱各自独立设限。
3. **审查来自陌生来源的项目配置。** 文件夹信任为 `.chaos/config.toml` 与 `.claude/settings.json` 中的项目权限规则把关，也为项目指令与技能的启动加载把关。无头启动要使用这些来源需要 `--trust` 或既有授权。在信任一个陌生检出之前，请审查它们以及任何项目钩子（见 [10-hooks.md](10-hooks.md)）。
4. **测试你的策略。** 设置 `defaultMode: "dontAsk"`（或安装你的 `PreToolUse` 钩子）后，运行有代表性的命令，确认哪些被阻止。
5. **把只读命令清单当作便利，而不是安全边界。**

---

## 另见

- [钩子](10-hooks.md) — PreToolUse 及其他生命周期脚本
- [无头模式](14-headless-mode.md) — 一次性命令行与自动化标志
- [代理模式](15-agent-mode.md) — ACP、stdio 与代理服务器
- [沙箱](18-sandbox.md) — OS 级隔离 profile
- [配置](05-configuration.md) — 原生 `config.toml` 结构

