# 键盘快捷键

Chaos TUI 快捷键参考。绑定为内置，目前不可自定义重映射。

> 配置路径：`~/.chaos/config.toml` 或兼容的 `~/.grok/config.toml`（双读，见
> [配置](05-configuration.md) / [CHAOS.md](../../../../../CHAOS.md)）。

---

## 输入模式

Chaos 有两种输入模式，控制回滚区导航方式：

- **Simple mode**（简单模式，默认）：方向键导航，`Shift+Arrow` 按回合跳转，`Space` 聚焦提示框，字母键自动聚焦提示框。
- **Vim mode**（可选）：`j`/`k` 导航，`H`/`L` 按选中回合跳转，`J`/`K` 跳到下一/上一回合的视口顶部（与时间轴箭头一致），`h`/`l` 折叠，`e`/`E` 展开/收起，`i`/`Tab`/`Space` 聚焦提示框。

默认 Simple。切换到 Vim：在用户 `config.toml` 的 `[ui]` 下设 `vim_mode = true`，或运行时 `/vim-mode`。详见 [配置](05-configuration.md)。

下表同时记录两种模式。「键」为 Vim 绑定，「Alt 键」为 Simple 等价键。

> **需开启 Vim mode**：回滚区上下文中的单字母与 `Shift+字母` 绑定（`j/k`、
> `h/l`、`g/G`、`L/H`、`y/Y`、`o/O`、`r`、`x`、`e/E`、以及 `i` 插入模式）
> 要求 `[ui].vim_mode = true`（或 `/vim-mode`）。方向键、`Tab`、`Esc`、
> `Space`、`PageUp/Down` 与所有 `Ctrl+字母` 在两种模式下均可用。

---

## 导航（回滚区聚焦）

在回滚区窗格中移动对话条目。

| 键 | Alt 键 | 操作 |
|-----|---------|--------|
| `j` | `Down` | 选中下一条条目 |
| `k` | `Up` | 选中上一条条目 |
| `⇧L` | `Shift+Right` | 跳到下一回合（用户提示） |
| `⇧H` | `Shift+Left` | 跳到上一回合（用户提示） |
| `⇧J` | | 跳到视口顶部的下一回合 |
| `⇧K` | | 跳到视口顶部的上一回合 |
| `g` | | 回到回滚区顶部 |
| `⇧G` | | 跳到回滚区底部 |
| `Ctrl+K` | | 向上滚动一行（不改变选中） |
| `Ctrl+J` | | 向下滚动一行（不改变选中） |
| `PageUp` | | 向上滚动一页（选中移到视口顶部） |
| `PageDown` | | 向下滚动一页（选中移到视口底部） |
| `Ctrl+U` | | 向上滚动半页 |
| `Ctrl+D`（VSCode 里是 `Shift+D`） | | 向下滚动半页 |

普通提示框聚焦时，`PageUp` 和 `PageDown` 同样能滚动对话，既不移动焦点，也不改动草稿。若存在处于活动状态的提示历史、`@` 文件搜索、斜杠菜单或补全下拉，这些按键则归它自己的导航使用。

---

## 视图（回滚区聚焦）

控制条目在回滚区中的显示方式。

| 键 | Alt 键 | 操作 |
|-----|---------|--------|
| `h` | `Left` | 折叠选中的条目 |
| `l` | `Right` | 展开选中的条目 |
| `e` | | 切换选中条目的折叠状态 |
| `⇧E` | | 展开全部 / 折叠全部条目 |
| `Ctrl+E` | | 展开/折叠全部思考块 |
| `r` | | 切换选中条目的原始 markdown 显示 |

在 `pager.toml` 的 `[scrollback.scroll]` 下设 `respect_manual_folds = true`
（选配，默认关闭 —— 见[配置](05-configuration.md)）后，手动折叠的块会被
钉住：流式更新和结束事件（例如某个思考块结束）不再动它，而是保留原状；
自动滚动跟尾时展开某个块会停止跟随，好让你安心阅读；用 `⇧G`、在最后一条
条目上按 `j`、滚动越过底部或发送新提示即可恢复跟随。`⇧E` 清除全部钉住，
`Ctrl+E` 清除思考块上的钉住。

### 块内容

| 键 | 操作 |
|-----|--------|
| `y` | 把块内容复制到剪贴板 |
| `⇧Y` | 把块元数据（例如所执行的 shell 命令）复制到剪贴板 |
| `Enter` | 在全屏查看器中打开块内容 |
| `Ctrl+F` | 在全屏查看器中打开块内容（备用绑定） |

---

## 焦点

在提示输入框与回滚区窗格之间切换。

| 键 | Alt 键 | 上下文 | 操作 |
|-----|---------|---------|--------|
| `Tab` | `Space`（vim 模式下还有 `i`） | 回滚区聚焦 | 聚焦提示输入框 |
| `Tab` | | 提示框聚焦 | 聚焦回滚区（simple 与 vim 回滚模式都适用） |
| `Tab` | `Shift+Tab`（反向） | 某个阻塞式卡片正被聚焦（提问、权限提示、取消回全面板） | 逐行遍历该卡片，两端循环折返。焦点始终留在卡片内 |
| `Tab` | `Space`（vim 模式下还有 `i`） | 回滚区聚焦且有卡片停驻 | 把键盘交还给该卡片（栏上的焦点提示会点名它） |
| `Enter` | | 提示框聚焦 | 发送当前提示 |

**Esc 不是聚焦键。**它遵循下文的清空 / 回退语义，且从不取消正在运行的回合
（那是 `Ctrl+C` 的职责）。Esc 的任何行为都不依赖 `[ui].vim_mode`（回滚区
导航）或 `[ui].simple_mode`（提示编辑器）。覆盖层、模态框、斜杠/文件下拉、
语音、搜索和选区仍会优先截走 Esc。

## 阻塞式卡片

有四个界面会等待你的答复并阻塞代理，打开期间接管键盘：**提问卡片**
（`ask_user_question`）、**MCP 征询卡片**（`x.ai/mcp/elicit`）、**权限提示**
以及**取消回全面板**。同时打开多个时，键盘先给权限提示，再给取消回全面板，
然后是提问卡片，最后是征询卡片 —— 而快捷键栏始终显示当前正在接收按键者
的按键。

它们共用一套约定：

- `Tab` / `Shift+Tab` 逐行遍历该卡片并在两端循环。它们从不把焦点移出卡片，
  所以光标永远停在你看得见的地方。
- `Esc` 一级一级向后退：先清空该卡片上待处理的内容，等再没有东西可清了
  才离开。离开后落到哪里是各卡片唯一的不同 —— 提问卡片和权限提示会把
  键盘停驻在回滚区，让你上滚阅读它们背后的上下文（卡片仍留在屏幕上）；
  而取消回全面板的「keep running」会关闭面板，让回合（以及任何子代理）
  继续运行。`Enter` 或 `1`–`4` 仍可选定某个取消并指定子代理的选项。
- 键盘停驻期间，快捷键栏显示回滚区自己的按键，其焦点提示点名的是卡片而
  不是提示框：`Tab/Space: question`。该提示是钉住的，所以再窄的栏也绝不
  会裁掉这唯一的归途。
- 在看板的会话覆盖层里还有一级：键盘停驻之后，下一次 `Esc` 返回看板，
  卡片保持待处理。（`Ctrl+\` 在任何状态下仍可直接离开。）

### MCP 征询卡片（`x.ai/mcp/elicit`）

当 MCP 服务器请求用户输入（表单字段或 URL 授权）时显示。标题始终包含
MCP 服务器名。

| 键 | 操作 |
|-----|--------|
| `↑` / `↓`, `j` / `k`, `Tab` | 在字段 / 操作之间移动 |
| 在字段上按 `Space` / `Enter` | 编辑文本，或切换布尔 / 枚举取值 |
| 操作项上的 `←` / `→` | 在 Accept 与 Decline 之间选择 |
| 在 Accept 上按 `Enter` | 提交（先校验表单；URL 会打开浏览器） |
| `d` / Decline | 拒绝该请求 |
| `Esc` | 后退一步：先退出文本编辑，再把焦点停驻到回滚区（`Tab` 返回）。只有等待已接受 URL 的结果时，它才会关掉卡片 |
| `Ctrl+C` | 取消该请求 |
| `o` | 等待完成期间重新打开 URL |

### 提问卡片（`ask_user_question`）

| 键 | 操作 |
|-----|--------|
| `↑` / `↓`, `j` / `k` | 在备选答案之间移动（两端封顶） |
| `Tab` / `Shift+Tab` | 循环遍历本题的答案 —— 从最后一个绕回第一个。绝不会把你带进另一道题 |
| `←` / `→`, `h` / `l`, `[` / `]` | 上一题 / 下一题 |
| `1`–`9`, `a`–`f` | 直接选定对应答案 |
| `z` | 跳到自由输入行并开始输入 |
| `Space` | 切换聚焦的答案（多选），或在自由输入行上开始输入 |
| `Enter` | 选定并前进，最后一题时提交，或编辑自由输入行 |
| `Esc` | 取消本题的已选答案；已无选中时，把焦点停驻到回滚区（`Tab` 返回）。在看板会话覆盖层的*第一*题上，它改为返回看板 —— 从后面的题目返回仍用 `←`，因此先停驻、下一次 `Esc` 再离开。快捷键栏会点名当前所在的那一级 |
| `y` | 复制聚焦的答案 |
| `Shift+X` | 关闭提问（代理在无答复的情况下继续） |
| `Ctrl+F` | 全屏显示该卡片 |

`/feedback` 面板是这张表唯一的例外：报告框没有可遍历的答案，`Enter`
直接发送报告，`Esc` 关闭面板。当可以上传 trace 时，在报告上按 `Enter`
会先弹出一个上传问题（`↑`/`↓` 选择，`Enter` 按你的选择发送，`Esc`
跳过上传但仍发送报告）。

在输入自由文本答案时，`Enter` 提交，`Esc` 返回答案行；其余按键都进入
文本框。

### 权限提示

| 键 | 操作 |
|-----|--------|
| `↑` / `↓`, `j` / `k` | 在选项之间移动（两端封顶） |
| `Tab` / `Shift+Tab` | 循环遍历选项 |
| `1`–`9` | 直接选择对应选项 |
| `Enter` | 选择聚焦的选项 |
| `←` / `→` | 放宽 / 收窄「always」答复所要记住的范围 |
| `e` | 手工编辑始终允许规则（bash 提示） |
| `Ctrl+F` | 展开 / 折叠完整参数 |
| `Ctrl+O` | 开启始终批准模式 |
| `Esc` | 把焦点停驻到回滚区（`Tab` 返回）。它从不作答复，也从不关闭请求 |
| `Ctrl+C` | 取消该请求 |

在「No」行上直接输入则会开始给代理写一条消息；`Enter` 发送，`Esc`
返回选项。

### 取消回全面板

| 键 | 操作 |
|-----|--------|
| `↑` / `↓`, `j` / `k`, `Tab` / `Shift+Tab` | 在各选项之间移动 |
| `1`–`4`, `Enter` | 确认该选项 |
| `Esc` | 一切照常运行。这会了结该面板，因此它永远不是死胡同，也永远不需要停驻 |

## Escape

| 状态 | 手势 | 效果 |
|--------|---------|--------|
| 回合运行中（任何模式与窗格） | `Esc` | **不**取消。显示「Press Ctrl+C to cancel the turn」提醒；草稿原封不动。请用 `Ctrl+C`（或命令面板 / 其他取消入口）。 |
| 回合取消中 | `Esc` | 被吞掉的无操作。此状态下 `Ctrl+C` 会逐步升级为退出。 |
| 空闲 + 提示框非空（文本或图片块），**提示框聚焦** | **800ms 内按两次 `Esc`** | 清空提示框；被清空的草稿会入档暂存（`Ctrl+S` 或 `Alt+S` 可恢复，图片也在内），其文本在 `↑` 历史浏览中排第一。第一次按时提示「press again to clear」。 |
| 空闲 + 提示框为空 + 已有对话消息，**提示框或回滚区聚焦** | **800ms 内按两次 `Esc`** | 打开回退选择器（同 `/rewind`）。第一次按是静默的（无提示）。 |
| 空闲 + 为空 + 无消息，**或回滚区聚焦且有草稿 / 处于 `!` `#` 带模式编辑器 / 有待处理的 needs-input 覆盖层 / 历史搜索已打开** | `Esc` | 被吞掉的无操作（不聚焦回滚区）。清空仅作用于提示窗格；回退要求 Normal 模式编辑器为空、无待处理覆盖层、无打开的历史搜索。浏览回滚区绝不会改动你的草稿、编辑器模式、等待答复的提问或进行中的搜索。 |

**回合中 Esc 的缓冲期：**回合中按下 Esc 之后约一秒内，空闲态的回退触发
仍被抑制 —— 对一个随后就结束的回合狂按 Esc，不可能悄悄打开回退选择器。
被按住的只有回退触发；Esc 的其余行为不受影响。

**截获 Esc（先于回合中提示和清空 / 回退执行）：**覆盖层、模态框、斜杠/
文件/补全下拉、历史搜索、回滚区搜索、文本选区、链接高亮、语音，以及提示框
为空时的 **Bash / Remember 模式退出**（Esc 会离开 `!` / `#` 模式回到普通
提示框，即使回合正在运行）。裸 `/feedback` 打开报告面板；Esc 将其关闭。

**Ctrl+C 与 Esc：**回合运行中草稿非空时，Ctrl+C 清空草稿并保留回合；提示
框为空时第二次 Ctrl+C 才取消。Esc 从不取消：回合中它只把你指向 Ctrl+C，
且不动草稿。空闲且非空时 Ctrl+C 一次即清空；Esc 需在 800ms 内按两次。
两种清空的残留不同：`Esc Esc` 会把草稿入档暂存，`Ctrl+S` 能取回，而
`Ctrl+C` 直接丢弃（其文本仍在 `↑` 历史里）。

---

## 代理级

影响代理会话的操作，在代理界面可用。

| 键 | 上下文 | 操作 |
|-----|---------|--------|
| `Ctrl+P` | 代理界面 | 打开命令面板 |
| `?` (Shift+/) | 代理界面 | 打开命令面板（备用绑定） |
| `Ctrl+M` | 代理界面 | 打开模型选择器 / 切换模型 |
| `Ctrl+M` | 提示框聚焦 | 切换多行输入模式 |
| `Ctrl+C` | 代理界面 | 取消当前回合（或先清空非空草稿；见 Escape 表） |
| `Ctrl+O` | 代理界面 | 切换始终批准（YOLO）模式 |
| `F3` | 代理界面 | 打开会话选择器（恢复先前的会话，同 `/resume`） |
| `Ctrl+;`（另一种：`Ctrl+'`） | 代理界面 | 切换提示队列面板（非空时）。仅限**本地 macOS** 的 VS Code 家族：主键为 **`Ctrl+4`**（`;` / `'` 仍是备用）。SSH 与非 Mac 保持 **`Ctrl+;`** / **`Ctrl+'`**。 |
| `Shift+Tab` | 提示框聚焦 | 循环切换模式（Normal → Plan → Auto（启用时）→ Always-approve） |
| `Ctrl+B` | 代理界面 | 把正在前台运行的命令转入后台 |
| `Ctrl+T` | 代理界面 | 切换任务面板 |
| `Ctrl+G` | 代理界面（完整 TUI） | 切换 tasks 面板 |
| `Ctrl+G` | 普通编辑器（最小模式） | 在外部编辑器中编辑当前草稿但不发送。若终端保留了该组合键，请从命令面板选「在外部编辑器中编辑提示」（`/edit-prompt`）。 |
| `Ctrl+L` | 代理界面 | 打开扩展模态框（**仅限非 VS Code 家族**；在 VS Code / Cursor / Windsurf / Zed 上，`Ctrl+L` 是回合中**插话**，扩展改经 `/plugins` / `/hooks` 打开） |
| `↑` | 提示框聚焦（提示框为空、普通输入模式） | 有排队的提示时，把焦点移入队列面板并高亮最后一行（`e` 编辑，`Enter` 立即发送）。否则打开历史面板并预填你最近的一条提示；`↑`/`↓` 逐条翻阅（每条都落入输入框），在最新一条上再按 `↓` 关闭面板，直接输入则就地编辑召回的提示。召回的 `!` shell 命令会重新进入 shell 模式。`↓` 从不打开历史。 |
| `Ctrl+S`（另一种：`Alt+S`） | 提示框聚焦 | 暂存 / 取回草稿，类似 `git stash`。编辑器里有文本或图片时：暂存并重新开始。编辑器为空时：恢复最近一次暂存（图片与 `!` shell 模式都在内）。组合键暂存的草稿还会在**你发送下一条提示后自动恢复**（双 Esc 清空的草稿保持暂存，因为那个手势是丢弃）。一次只有一份草稿：新的暂存会顶掉旧的，旧文本仍可在 `↑` 历史里找到；被暂存草稿的文本在那里排第一。 |
| `!` | 提示框聚焦 | 进入 shell 模式（在空提示框上输入 `!`） |
| `Ctrl+.`（另一种：`Ctrl+X`） | 代理界面 | 打开键盘快捷键帮助 |
| `F2`（另一种：`Ctrl+,` / `Cmd+,`） | 代理界面 | 打开设置模态框 |

**注：**当**子代理全屏视图**打开时，编辑器被隐藏。仅限根会话的组合键
（`Ctrl+P`、`Ctrl+M`、`F3`、`Ctrl+O`、`Ctrl+B`、设置、扩展、Shift+Tab）
都不起作用。`Ctrl+C` 取消的是**子代理**的回合。`q` / `Esc` 关闭该视图。
`Ctrl+Q` 仍可退出（VS Code 家族为 `Ctrl+D`）。见[在 TUI 中查看子代理](16-subagents.md#全屏框架视图子会话记录)。

**注：**`Ctrl+M` 依上下文而定。提示框聚焦时切换多行输入模式；否则打开
模型选择器。

**注：**草稿处于暂存期间，提示框的顶边框会显示 `Stashed`（如果你设过
`/rename` 标题，则紧挨着它）。最小模式不画边框，因此每次暂存或恢复都会在
回滚区打印一行。暂存只存在于内存中：退出即消失，也不会跟随恢复的会话。
新的暂存会顶掉旧的，只有旧草稿的**文本**会进 `↑` 历史，被顶掉草稿上的任何
图片都会丢失。

**注：**外部编辑在任何渲染模式下都可用：最小模式绑定 `Ctrl+G`，完整 TUI
用 `/edit-prompt` 或命令面板。Chaos 依次解析 `$VISUAL`、`$EDITOR`、`vi`。
取值可以带带引号的参数。保存只替换草稿（编辑器保存时追加的末尾换行会被
去掉）；空文件则清空草稿。带粘贴/文件/图片块的草稿必须在编辑器里编辑，
以免附件被拍平。

**注：**`Ctrl+'` 是 `Ctrl+;` 在 Windows 上的备用 —— 有些 Windows 终端会
丢掉标点键上的 `Ctrl` 修饰符。

**注：**`Ctrl+.` 需要 Kitty 键盘协议（或 tmux `extended-keys on` 让该协议
得以透传）。在 VS Code / Cursor / Windsurf / Zed 集成终端、VTE、Apple
Terminal、Windows Terminal、JetBrains、开了 `extended-keys off` 的 tmux、
screen 及类似不支持 KKP 的环境里，Chaos 会改以 **`Ctrl+X`** 作为快捷键
速查的主键。**`Ctrl+X` 始终可用**——它是经典控制字符，即使 `Ctrl+.`
不行它也能用。若 tmux 里修饰键失灵，运行 `/doctor`。

---

## 图片粘贴与拖放

| 操作 | macOS | Linux | Windows |
|---|---|---|---|
| 从文件管理器把图片拖进提示框 | Finder ✓ | Files / Dolphin ✓ | Explorer ✓ |
| 在文件管理器里复制文件后粘贴 | `Cmd+V` | `Ctrl+V` | `Ctrl+V` |
| 剪贴板里有截图或「Copy Image」后粘贴 | `Cmd+V` | `Ctrl+V` | **`Alt+V`** |

非图片文件会以文本形式插入其绝对路径，而不是一个块。

> **Windows 上的 `Alt+V`** 是 Chaos 特有的。Windows Terminal 默认的
> `Ctrl+V` 只粘贴纯文本，会静默丢弃图片剪贴板内容；`Alt+V` 绕过这层
> 拦截。若想让 `Ctrl+V` 也能粘贴图片，请在 Windows Terminal 的
> `settings.json` 中向 `actions` 添加 `{ "command": null, "keys": "ctrl+v" }`。

### Linux 的 PRIMARY 与 CLIPBOARD

Linux X11 有两套相互独立的文本选区：

- `Ctrl+V` 读取 **CLIPBOARD**，即显式复制/剪切的选区。它绝不会回退到
  PRIMARY。要用 `xclip` 把文本放进去，用
  `printf %s "text" | xclip -selection clipboard`。
- 在 Chaos 里不带修饰符地点击中键读取 **PRIMARY**（当前鼠标选区），仅在
  `DISPLAY` 非空时生效。纯 X11 可以用原生读取器回退；XWayland 要求 `PATH`
  上有 `xclip` 或 `xsel`，这样 Chaos 读到的才是 X11 选区而不是 Wayland 的
  PRIMARY。按下只处理一次；松开不会再次粘贴。
- `Shift+Insert` 是终端原生的粘贴选中文本方式。许多终端还支持
  `Shift+middle click` 绕过应用的鼠标上报。

通过 SSH 时，远端的 Chaos 进程通常够不到终端本地的 X11 选区。请用终端原生的
`Shift+Insert` 或 `Shift+middle click`，让本地终端把选中文本经 PTY 送过去。

---

## 回合进行期间（代理运行中）

代理生成期间：

- **普通 `Enter`**（编辑器里有文本时）会为稍后**排队**一条后续提示。默认
  （`[ui].follow_up_behavior = "queue"`）这些后续提示在当前回合结束后运行
  —— 而且当代理在等待后台任务或某个子代理而阻塞时，它们会刻意**按住不动**
  （会有提示解释这次按住，以及如何立即发送）。设为 `"steer"` 时，同一个
  Enter 仍会把该行显示在队列里，随后 shell 会在下一个工具或模型的安全间隙
  把它注入回合中（见[配置](05-configuration.md)）。
- **在已清空的编辑器上再按一次 `Enter`**（双 Enter）立即发送**最上面**那条
  排队的后续提示。
- **立即发送**组合键是**取消并发送**：它停止当前回合（后台任务、子代理和
  队列的其余部分继续运行），把你的消息作为下一个回合发送，因此它总是出现在
  记录的最底部：
  - **编辑器非空** → 取消并立即发送该文本。
  - **编辑器为空** + 有排队的后续提示 → 立即发送**最上面**那条排队提示
    （无需聚焦队列面板）。在队列面板上，同一组合键（或 **[Send now]**
    按钮）发送**选中**的那行。
  - **空闲**，或**编辑器为空且无排队** → 该键无操作。
- 当代理**阻塞等待**时（等待任务输出或某个子代理），带文本的普通 `Enter`
  也会立即送达 —— shell 会取消被阻塞的回合，接着运行你的消息。

| 终端 | 主要 | 备选键 | 操作 |
|----------|---------|------------|--------|
| 默认 | `Ctrl+Enter` | `Ctrl+I` | 立即发送（取消当前回合，接着运行你的消息） |
| Apple Terminal | `Ctrl+O` | `Ctrl+Enter`, `Ctrl+I` | 立即发送 |
| VS Code 家族（VS Code、Cursor、Windsurf、Zed） | **`Ctrl+L`** | *（无）* | 立即发送（不用 `Ctrl+I` —— Tab / 宿主聊天；插件经 `/plugins`） |

在 `/multiline` 模式下，`Shift+Enter`（或 `Alt+Enter`）发送，而普通
`Enter` 插入换行 —— 唯一的例外是回合中**空**编辑器且有排队后续提示时，
普通 `Enter` 仍会**立即发送**最上面那行（与普通模式一致）。（在非 VS Code
家族上绑定时，`Ctrl+Enter` 在回合中是立即发送；它不会提交新的空闲回合。）

立即发送有意做成打断式的 —— 它读作「停下手里的事，先处理这个」。想给代理
递一张**不打断**它的便条，请用普通 `Enter` 排队；代理会在下一个回合边界
取走它。

> **WezTerm**：这些带修饰符的 Enter 键需要在 WezTerm 配置里设
> `enable_kitty_keyboard = true`。完整步骤与一行 workaround 见
> [终端支持指南](21-terminal-support.md#ctrlenter-在-wezterm-中不触发插话)。

> **Windows（非 VS Code 家族）**：有些终端会丢掉 `Ctrl+Enter` 上的 `Ctrl`
> 修饰符（可能塌缩成裸 `Enter` 或 `Ctrl+J`）。改用 `Ctrl+I` 作备用 ——
> 字母键的 Ctrl 组合在哪里都稳定。在 VS Code 家族上用 **`Ctrl+L`**。

> **VS Code 家族的 `Ctrl+L`**：Chaos 把它用于插话，扩展快捷键保持未绑定
> （用 `/plugins` 或命令面板打开插件）。如果你的终端 profile 仍把
> **Clear**（或其他命令）映射到 `Ctrl+L`，宿主的绑定可能截走该组合键 ——
> 请重绑或移除它，让 PTY 收到 form feed（`\x0c`）。

---

## 全局

任何界面都可用的操作。

| 键 | Alt 键 | 操作 | 确认 |
|-----|---------|--------|-------------|
| `Ctrl+N` | | 新建会话（可选择在 git 工作树中） | 是（1000ms 内连按两次） |
| `Ctrl+\` | | 打开或切换 [代理看板](23-dashboard.md) | 否 |
| `Ctrl+Q` | `Ctrl+D` | 退出应用 | 是（1000ms 内连按两次） |

**VS Code 家族终端**（VS Code、Cursor、Windsurf、Zed 集成终端）：`Ctrl+Q`
会被宿主截走，因此 Chaos 让 **`Ctrl+D` 成为唯一的退出键**（`Ctrl+Q` 不
绑定）。半页下滚改绑为裸 **`Shift+D`**。回合中插话用 **`Ctrl+L`**（无
备用），因为 `Ctrl+Enter` / `Ctrl+I` 不一定能到达 PTY；扩展改经 `/plugins`
打开而不是 `Ctrl+L`。

> **返回欢迎界面没有按键绑定** —— 请在会话内使用 `/home` 斜杠命令（别名
> `/welcome`）。见[斜杠命令](04-slash-commands.md)。

### 破坏性操作确认

确认列标为「是」的操作要求在 1000ms 内连按两次。第一次按键会显示确认
提示，再按一次才执行。这样可以防止意外丢失会话。

---

## 欢迎界面

仅在欢迎界面上（尚未打开任何代理会话时）才触发的绑定。

| 键 | 操作 |
|-----|--------|
| `F3` | 恢复会话（打开会话选择器） |
| `Ctrl+W` | 打开新建工作树对话框（仅限 git 仓库内） |
| `Ctrl+I` | 导入 Claude 设置（可用时） |
| `Ctrl+Shift+I` | 关闭 Claude 导入行（可用时） |

`Ctrl+W`、`Ctrl+I` 和 `Ctrl+Shift+I` 只在欢迎界面上生效。`F3` 在欢迎界面
和代理会话内都会打开会话选择器（在会话内以模态覆盖层形式打开，同 `/resume`
命令）。`Ctrl+S` 是会话内的提示草稿暂存（在欢迎界面上无操作，那里没有可
搁置的草稿）。`Ctrl+Q` 就是上文记录的全局退出绑定，并非欢迎界面专用的
处理器。

---

## 代理看板

[代理看板](23-dashboard.md)聚焦时（`Ctrl+\` 或 `/dashboard`）的绑定。

| 键 | 操作 |
|-----|--------|
| `↑` / `↓`, `j` / `k` | 在代理行之间导航（选中某行即打开预览） |
| `Enter` | 打开选中的代理，或发送已输入的预览回复 / 调度提示 |
| `Ctrl+S` | 回复或调度**并**附着到该代理 |
| `Ctrl+/` | 切换搜索 / 过滤模式 |
| `Ctrl+R` | 重命名选中的代理 |
| `Ctrl+T` | 固定 / 取消固定 |
| `Ctrl+G` | 切换分组方式（状态 ↔ 工作目录） |
| `Ctrl+X` | 取消运行中的回合，或 2 秒内按两次以永久删除 |
| `Ctrl+O` | 对选中的代理切换始终批准 |
| `Tab` | 在列表与调度 / 预览输入框之间切换焦点 |
| `Esc` | 逐步后退（取消搜索 → 关闭预览 → 清除过滤 → 取消聚焦 → 取消选中 → 退出） |
| `Ctrl+\` | 退出看板（或从已附着的代理返回） |
| `Ctrl+.`（另一种：`?`） | 快捷键速查 |

详情（预览与调度、搜索前缀、持久化）：[代理看板](23-dashboard.md)。

---

## 命令面板

按 `Ctrl+P` 或 `?` 打开命令面板 —— 一个可搜索的常用操作列表，每条都显示
其按键绑定或斜杠命令。它包含会话操作、扩展模态框的各个标签页（Hooks、
Plugins、Marketplace、Skills、Workflows、MCP Servers）等。

输入即可过滤，然后按 `Enter` 执行选中的操作。

---

## 快捷键栏

TUI 底部会显示一个随上下文变化的快捷键栏，展示当前状态最相关的按键绑定。
提示内容依据以下因素变化：

- 当前聚焦的窗格（回滚区还是提示框）
- 代理是否正在运行
- 选中的条目类型

---

## 鼠标支持

TUI 支持鼠标交互：

- **点击**某个回滚区条目以选中它
- **滚轮**滚动回滚区
- **点击**提示区以聚焦它
- **悬停**在提示框上显示高亮（可经 `pager.toml` 配置）
- 在 Linux X11/XWayland 上**点击中键**粘贴 PRIMARY 选区

---

## 速查卡

### 回滚区聚焦时（Simple 模式 —— 默认）

```
Navigation:       Up/Down (prev/next entry)  Shift+Left/Right (prev/next turn)
Scrolling:        Ctrl+J/K (line)  PgUp/PgDn (page)  Ctrl+U/D (half page)
Focus prompt:     Space or any letter key (auto-focuses and types)
```

### 回滚区聚焦时（Vim 模式）

```
Navigation:       j/k (up/down)  H/L (prev/next turn)  K/J (viewport-top turn)  g/G (top/bottom)
Scrolling:        Ctrl+J/K (line)  Ctrl+U/D (half page; D=Shift+D in VSCode)  PgUp/PgDn (page)
Folding:          h/l (collapse/expand)  e (toggle)  E (all)
Content:          y (copy)  Y (copy cmd)  Enter (fullscreen)
View:             r (raw markdown)  Ctrl+E (thinking)
Focus prompt:     i, Tab, or Space
```

### 提示框聚焦时

```
Send:             Enter
Newline:          Shift+Enter or Alt+Enter
Multiline:        Ctrl+M (toggle)
Paste:            Ctrl+V (text, files, screenshots on macOS/Linux)
Selected text:    Middle click or Shift+Insert (Linux X11/XWayland PRIMARY)
Paste image:      Alt+V (Windows only — for screenshots / "Copy Image")
Select all:       Cmd+A (macOS, Ghostty only — see note below)
Select text:      Shift+←/→ (char) · Alt+Shift+←/→ (word) ·
                  Cmd+Shift+←/→ (visual row) · Shift+Home/End (logical line) ·
                  Shift+↑/↓ (row)
Copy / Cut:       Cmd+C / Cmd+X (with a selection; Kitty-protocol terminals)
Leave:            Tab (back to scrollback)
Cancel (running): Ctrl+C (empty prompt; non-empty draft clears first)
Clear (idle):     Esc Esc within 800ms (non-empty prompt)
Rewind (idle):    Esc Esc within 800ms (empty prompt + messages)
```

有活动选区时，输入 / `Enter` / 粘贴会替换它，删除与按词删除组合键只删除
选区内容，方向键把光标收拢到对应的边缘（按词/按行的移动从那条边缘继续），
而 `Esc` 或 `Tab` 会去掉高亮，同时照常执行它们原本的动作。注意
`Shift+←/→` 只在**提示框**聚焦时用于选择；回滚区聚焦时，同样的组合键在
回合之间跳转（见上文「导航」）。

> **Cmd+A 仅限 Ghostty。**Chaos 应用内的 `Cmd+A` 处理器只在检测到的终端
> 是 Ghostty 时才接通。其他终端要么在终端层吞掉 `Cmd+A`（Apple
> Terminal、默认的 iTerm2），要么执行各自终端内的「全选」行为（Kitty、
> WezTerm）。在非 Ghostty 终端上，该绑定不起作用，按键落回终端的原生
> 行为。
>
> 在 Ghostty 上，请把这一行解绑加进 `~/.config/ghostty/config`，好让
> 按键到达正在运行的 TUI：
>
> ```ini
> keybind = cmd+a=unbind
> ```
>
> Ghostty 重载配置后（它会监视配置文件），在提示框里按 `Cmd+A` 会选中
> 提示缓冲区中的每一个字符，包括粘贴的图片块。图片块永远不带路径
> （`[Image #N]`）；文件路径（如果已知）只出现在悬停或光标位于块上/紧随
> 其后时的图片预览覆盖层里。

### 始终可用

```
Command palette:  Ctrl+P or ?
Model picker:     Ctrl+M (from scrollback)
Cancel:           Ctrl+C (see Escape table)
Always-approve:   Ctrl+O (toggle YOLO)
New session:      Ctrl+N (press again, then choose normal/worktree)
Quit:             Ctrl+Q (or Ctrl+D in VSCode)
```
