# 斜杠命令

在提示框里输入 `/` 打开命令菜单。它会随着你的输入做模糊匹配，选中一条就立即执行。

命令来自两处：**shell 内置命令**，由代理后端（xai-grok-shell）处理；**分页器内置命令**，由分页器前端（xai-grok-pager）处理。两者出现在同一个菜单里，任何带 `user-invocable: true` 的已启用技能也会出现在那里。如果一个技能重用了内置名（比如 `login`），内置命令保留 `/login`，技能则以 `/plugin-name:login` 的形式继续可用 —— 菜单会给两者都加上标记，让冲突看得见。

下面每条命令都列出了它的别名（如果有的话）。有少数命令只在某项功能或会话状态启用时才出现，这些情况会在正文里点明。菜单还会按渲染模式过滤 —— 见 [`/minimal` 和 `/fullscreen`](#minimal-与-fullscreen)。

---

## 会话管理

### `/new`

开一个新会话，并清空当前对话。别名：`/clear`。

### `/resume`

打开会话选择器，从磁盘重新载入此前的会话。

### `/dashboard`

打开[代理看板](23-dashboard.md)：本分页器里顶层会话的实时名册（查看、回复、派发、置顶、重命名、停止、接入）。别名：`/agents-dashboard`、`/sessions`。

它不是 `/config-agents`（别名 `/agents`）—— 后者管理代理*定义*与人设。最小模式下隐藏；用 `GROK_AGENT_DASHBOARD=0` 或 `[dashboard].enabled = false` 关闭。

### `/compact [context]`

压缩对话历史，回收上下文窗口的空间。可以带一段说明，告诉 Chaos 要保留什么：

```
/compact
/compact keep the auth implementation details
```

上下文窗口用到 85% 时 Chaos 也会自动压缩（用 `[session] auto_compact_threshold_percent` 调这个阈值）。

### `/context`

展示上下文窗口的使用情况：按类别拆分（系统提示、消息、推理与开销、剩余空间），外加若干信息行 —— 工具定义、技能清单，以及 MCP 服务器通告及其估算的 token 开销。

### `/session-info`

展示会话详情 —— 认证方式、模型、回合数、上下文用量。别名：`/status`、`/info`。点击某个值，或拖动选中后复制；`c` 复制会话 ID，`y` 复制整块内容。

### `/fork`

把当前会话分叉成一个新代理，保留到此为止的历史。

### `/rewind`（别名：`/undo`）

把对话回退到更早的回合，并丢弃其后的所有内容。`/undo` 是同一条命令。

### `/copy`

把最近一次回复的源 markdown 复制到剪贴板。传一个数字就复制倒数第 N 条回复；传一个文件路径就把文本写进文件而不是剪贴板（在 SSH 上很方便 —— 本地剪贴板往往够不着）。

```
/copy
/copy 2
/copy out.txt
/copy 2 ~/exports/last-reply.md
```

每一次复制还会写一份备份文件 —— 默认是 `~/.chaos/last-copy.txt`，设了 `GROK_COPY_FILE` 就用它。确认成功的复制会短暂提示一下（例如 `Copied!`）。未经确认的 OSC 52 投递，以及剪贴板够不着时的回退，都会给出备份路径，好让你把文本取回来。

### `/export`

把对话导出到文件或剪贴板。

### `/quit`

退出程序。别名：`/exit`。

### `/home`

离开当前会话，回到欢迎界面。别名：`/welcome`。

### `/delete`

删除当前会话的历史。会先要求确认。清除历史之前，会停掉正在运行的回合、后台任务与子代理。删完回到欢迎界面；如果你是从看板打开的这个会话，就回看板。

要删除一个你不在其中的会话，打开 `/resume` 或欢迎界面的会话列表，按 `d` 再按 `y`。在看板上，连按两次 `Ctrl+X`，或点击 `[✗]`。

### `/rename`

重命名当前会话。别名：`/title`。

```
/rename new session title
/rename --auto
```

`--auto` 解除手动标题的固定，让自动起名恢复工作。它只对 Build 会话有效 —— 聊天式对话没有本地自动起名器。它必须是唯一的参数（`/rename --auto Something` 会报错）。用这条命令没法把会话命名成 `--auto`；这种极端情况请用看板的重命名编辑器（`Ctrl+R`）。

---

## 模型与模式

### `/model <name>`

切换模型。接受模型 ID 或显示名（不区分大小写），还可以把推理等级作为第二个参数传进去。没有等级元数据的模型使用内置的回退等级。别名：`/m`。

```
/model grok-4.6
/model Grok 4.6
/model Reasoning X high
```

### `/effort <level>`（别名 `/think`）

给**当前**模型设置推理等级，而不重新选择模型。内置的回退等级是 `max`、`xhigh`、`high`、`medium`、`low`；服务商提供的 `reasoningEfforts` 列表优先。能力声明缺失时放行，而显式声明 `supportsReasoningEffort: false` 会禁用这条命令。

```
/effort high
```

### `/always-approve` 与 `/auto`

两者都是权限模式的真实开关：它们一直留在菜单里，运行你当前已经处于的那个模式就会把它关掉。

| 命令 | 关闭时 | 已经打开时 |
|---|---|---|
| `/always-approve` | 跳过所有权限提示 | 回到询问 |
| `/auto` | 分类器放行安全的工具（危险工具仍可能提示） | 回到询问 |

一个开着的时候运行另一个会切换模式 —— 例如在 always-approve 打开时运行 `/auto` 就切到 auto。`/auto` 只在自动权限模式这项功能启用时才出现。你也可以用 `Shift+Tab`（在 Normal / Plan / Auto（启用时）/ Always-approve 之间循环）、`Ctrl+O` 或 `/settings` 换模式。

### `/multiline`

切换多行输入。打开时 `Enter` 插入换行，`Shift+Enter`（或 `Alt+Enter`）发送消息。回合进行中，在空的输入框里光按一下 `Enter` 仍然会强制发送队列里最上面那条后续消息。别名：`/ml`。

### `/history`

打开提示词历史搜索：把本会话的提示词从新到旧模糊搜索，然后按 `Enter` 或 `Tab` 把命中的那条放回提示框。

想快速召回，也可以在空提示框上按 `↑`。如果有排队中的提示词，这么做会把焦点移进队列面板并高亮最后一行；否则面板打开时已经填好你最近的那条提示词，`↑`/`↓` 逐条翻阅（每按一次都会落进输入框），从最新那条再往下按 `↓` 就关闭面板，直接输入则是在原地编辑召回的那条提示词。

### `/compact-mode`

切换紧凑显示 —— 更少的留白、更紧的行距，输出更密集。

### `/vim-mode`

切换 vim 风格的回滚区按键（`j`/`k`、`h`/`l`、`g`/`G`、`y`/`Y` 等等）。关掉时（这也是默认值），在回滚区里光按一个字母或 `Shift+letter`，只会把焦点移到提示框并输入该字符。这项设置会持久化到 `[ui] vim_mode`。

### `/edit-prompt`

在任一渲染模式下为提示词打开外部编辑器。Chaos 依次解析 `$VISUAL`、`$EDITOR`、`vi`；命令值里可以带带引号的参数。保存会替换草稿但不发送，保存一个空文件则清空草稿。输入 `/edit-prompt` 必然替换输入框里的内容，所以编辑器是从空草稿开始的；要编辑**已有的**草稿，请从命令面板里选**在外部编辑器中编辑提示**（`/edit-prompt`；或在最小模式下按 `Ctrl+G`），它会保留文本，遇到粘贴内容、文件引用或图片块时直接拒绝，而不是把它们拍平。

```
/edit-prompt
```

### `/minimal` 与 `/fullscreen`

把当前会话就地切到另一种渲染模式。`/minimal`（在 fullscreen 下提供）切到实验性的回滚区原生模式；`/fullscreen`（在 minimal 下提供；别名 `/full`）切回标准全屏模式。切换发生在运行中的进程内部 —— 没有任何东西重启，所以正在跑的回合继续流式输出，你的输入框草稿、排队的提示词和权限模式都一并带过去；一个标记（最小模式下是提交进回滚区的一行，全屏下是一条提示）会提醒你怎么切回去。两者都是会话级的 —— 不写 `config.toml` —— `--minimal` / `--fullscreen` 这两个命令行开关同样是会话级的。要让裸跑的 `chaos` 默认以某个模式打开，用 `/settings` → **Default screen mode**，或设置 `[ui] screen_mode`。（如果这个就地切换在某个古怪的终端里出了问题，`GROK_SCREEN_MODE_SWITCH=exec` 可以恢复旧行为：把分页器重新拉起，挂到同一个会话上。）

有少数命令只能在两种模式中的一种里工作，因为它们驱动的界面在另一种模式里并不存在：`/find`、`/jump`、`/timeline`、`/theme`、`/tutorial` 和 `/dashboard` 是全屏专属，而 `/expand` 是最小模式专属。（`/workflow runs` 不一样：它在全屏下打开运行面板，在最小模式下退化成文本概览，而不是拒绝执行。）在它们跑不了的那种模式里，这些命令会从命令菜单和命令面板中隐藏。如果你还是把它们敲了出来，Chaos 会说明原因 —— 并指向真正有用的那个做法。当另一种模式是拿到它的唯一途径时，给出的就是模式切换：`/theme isn't available in minimal mode (minimal renders with your terminal's own palette). Run /fullscreen to switch this session.` 而当当前模式本来就有别的做法时，它点名那个做法：`/expand isn't available in fullscreen mode: press Tab to focus the scrollback, then → on the block.` 其余命令两种模式都能用。注意 `--no-alt-screen` 在这里仍然算全屏，所以它保留全屏专属的那些命令。

### `/plan`

进入计划模式。

```
/plan [description]
```

### `/view-plan`

打开当前已保存计划的预览。别名：`/show-plan`、`/plan-view`。

---

## 记忆

`/flush` 与 `/dream` 要求记忆已启用 —— 通过 `GROK_MEMORY=1`、`[memory] enabled = true`，或托管的远程设置。`/memory` 的要求不一样：只要记忆后端**已配置**就能用，哪怕记忆当前是关的，这样你关掉之后还能用 `/memory` 把它打开。`/remember` 始终可用。

### `/memory`

浏览、查看和管理已保存的记忆。传 `on` 或 `off` 可以打开或关闭记忆。别名：`/mem`。

```
/memory
/memory off
```

### `/flush`

立刻把当前会话的知识写进记忆，触发一次由模型归纳的「最重要内容」摘要。压缩之前值得用一次，任何时候想把上下文钉下来也可以用。回滚区里会给出一行提示，写明这次的触发原因与保存路径；headless 下则输出保存结果。

### `/dream`

执行记忆整理 —— 把工作区的会话日志交给模型归纳，折叠回工作区那份 `MEMORY.md`，然后删掉已经处理过的会话日志。成功时**没有**用户可见输出；只有被跳过时才会说明原因。

### `/remember`

立刻往记忆里存一条笔记，不必等自动摘要。

```
/remember the staging deploy uses the eu-west cluster
```

---

## 钩子与插件

`/hooks`、`/plugins`、`/marketplace`、`/skills` 和 `/workflows` 都打开同一个扩展模态，各自落在自己的标签页上。

### `/hooks`

在 Hooks 标签页上打开扩展模态，在那里可以查看已加载的钩子、增删自定义钩子，并逐个开关它们。这个模态**不会**授予项目信任 —— 信任模型见 [10-hooks.md](10-hooks.md)。

shell 另外还对外提供 `/hooks-list`、`/hooks-trust`、`/hooks-add`、`/hooks-remove`、`/hooks-untrust` 这几条单独的命令；在分页器里它们被折进 `/hooks` 模态。

### `/plugins`

在插件标签页上打开扩展模态，查看已安装的插件、从市场安装新的插件，并管理信任。

shell 另外还支持子命令（`/plugins list`、`/plugins install <source>`、`/plugins uninstall <name>`、`/plugins update`、`/plugins reload`）。在分页器里，模态用可视界面做同样的事。

### `/marketplace`

在市场标签页上打开扩展模态，浏览并安装插件。

### `/skills`

在 Skills 标签页上打开扩展模态，查看已安装的技能。

---

## 媒体生成

### `/imagine <description>`

根据文字描述生成一张图片。

```
/imagine a golden sunset over a calm ocean with silhouetted palm trees
```

### `/imagine-video <description>`

根据文字（或图片）描述生成一段视频。它会规划分镜、生成素材图，然后用 `image_to_video` 把它们动起来。

```
/imagine-video a cat playing piano in a jazz club
```

---

## 定时任务

### `/loop [interval] <prompt>`

让一条提示词按固定间隔重复运行。间隔写成 `30m`、`1 hour` 或 `every 2 days` 都可以；不写的话 Chaos 会问你。

```
/loop 30m check deploy status
/loop check deploy status every hour
```

间隔的写法是 `Ns`（秒，最小 60）、`Nm`（分钟）、`Nh`（小时）、`Nd`（天）；低于 60 秒的会被抬到最小值。重复任务 7 天后过期，取消时用 `scheduler_delete` 并传入创建这个循环时报告的作业 ID。

---

## 工作流与目标

### `/goal`

设置、管理或查看一个自主目标。Chaos 会跨多轮工作，只有在一次独立的证据复核确认了结论之后，才会把目标标记为完成；如果那次复核无法复现结果、或者拿不出可用证据，目标要么保持进行中，要么连同具体的缺口一起暂停下来。

```
/goal Migrate the auth module to the new API
/goal status
/goal pause
/goal resume
/goal clear
```

参数是 `<objective> [--budget <tokens>]`，或者 `status`、`pause`、`resume`、`clear` 之一。这里的 `--budget` 是这次目标运行用的 **token** 预算，与工作流用的代理个数预算无关。`/goal` 在会话启用目标模式时出现。由哪个驱动来跑它取决于后台工作流：开着的时候，宿主会评估每一个模型轮次，并对候选的完成结论做对抗式验证；关着的时候，走旧的、面向模型的 `update_goal` 路径来报告进度并触发验证。

### `/deep-research <query>`

启动一个后台研究工作流。它会规划一组有界的问题，带来源证据地收集结构化论断，把每条论断交给独立的验证分片交叉核对，最后只呈现活下来的论断及其已核验的来源定位符。失败的分片、被丢弃的论断、研究员的不确定之处都会作为覆盖度局限报出来；只要有这类局限残留，报告就会被标上 **Partial**。

```
/deep-research Compare the migration risks of PostgreSQL 17 and MySQL 9
```

命令会立刻返回 —— 进度去 `/workflow runs` 里看，最终报告会自己出现在对话中。

工作流对逻辑子代理调用设了一道绝对的累计 `agent_budget` 上限：每一次 `agent()` 调用、`parallel()` 面板里的每一个条目，都花掉一个名额，而模式纠正的重试不花。默认是 128，显式取值在 1–1,024 之间，如果一个面板会越过剩余预算，它会在任何子代理启动之前被拒绝。由模型发起的工作流，在 `workflow` 工具上设置 `agent_budget`；具名的斜杠启动则接受 `--agent-budget N`，或在其 JSON 参数里给一个 `agent_budget` 字段。具名启动还可以用 `--effort LEVEL` 或 JSON 里的 `effort` 设置子代理的推理等级，而不会改动当前会话的 `/effort`；子脚本自己的 `effort` 选项优先。另外，宿主配置的一道上限（默认 32）限制每次运行同时能跑多少个孩子；更大的面板会排队，并且仍然充当一道屏障。`budget()` 报告的字段是：上限为 `total`，已准入的调用为 `spent`，`reserved`（恒为 0），以及 `remaining`。

### `/workflow`

启动一个已保存的工作流，或者按会话内唯一的显示名管理正在运行的工作流。同一个工作流启动两次，显示名会带上编号（`review-changes`、`review-changes-2`）；你永远不需要内部的运行 ID。光敲 `/workflow` 会打印本会话各次运行的文本概览。

输入 `/workflow` 再打一个空格，就会自动补全已保存的工作流名（内置、项目、用户）以及管理动词 `runs`、`pause`、`resume`、`stop`、`save`。选中一个名字会把它填进去，并在你追加参数之前先给出启动开关；不按 Enter 就不会启动。而 `pause` / `resume` / `stop` / `save` 会接着列出本会话的运行句柄 —— 光敲 `/workflow stop` 不会替你选中哪一次运行。

```
/workflow review-changes --agent-budget 256 --effort high {"target":"origin/main...HEAD"}
/workflow review-changes {"target":"origin/main...HEAD","agent_budget":256,"effort":"high"}
/workflow runs
/workflow pause review-changes
/workflow resume review-changes
/workflow stop review-changes-2
/workflow save review-changes
```

`/workflow runs` 在全屏 TUI 里打开实时的 **Workflow Runs** 看板 —— 显示的是活跃与保留中的运行，不是已保存定义的目录。每一行给出这次运行的显示名、阶段、代理名册、进度和结果。在运行的详情视图里，`p` 暂停，`r` 恢复一次普通暂停，`x` 停止。受预算限制的运行无法用裸恢复命令继续：`r` 会把 shell 的拒绝原样返回（要用模型/工具发起的恢复请求、带上更高的 `agent_budget` 才能抬高上限），而 `x` 仍然能停。`s` 保存这次运行的脚本，但对已知的内置工作流与带编号的重复句柄是隐藏的 —— 遇到这些，请另选一个唯一的 `meta.name`，然后显式保存改过的脚本。在最小模式和非 TUI 客户端里，`/workflow runs` 打印的文本概览与光敲 `/workflow` 相同。

项目工作流放在 `.chaos/workflows/*.rhai`，用户工作流放在 `~/.chaos/workflows/*.rhai`。同进程的暂停/恢复会沿用最初那份不可变的脚本、参数与 `agent_budget` 上限，从已提交的宿主调用结果继续 —— 想迭代，就编辑返回给你的那份脚本副本，再把它当作新的一次运行启动。

受预算限制的运行不一样：它只能通过模型/工具发起的恢复请求继续，且该请求要给出高于已准入代理数的 `agent_budget`。光敲 `/workflow resume <name>` 抬不高上限，所以它拒绝恢复受预算限制的运行。因进程重启而中断的运行根本不会恢复，因为外部副作用没有稳定的跨进程身份。而且恢复并不是恰好一次：某个外部副作用如果在同进程暂停之前结果还没提交，就可能再跑一遍。

### `/workflows`

在工作流标签页上打开扩展模态 —— 这是一份只读目录，列出 Chaos 发现的已保存工作流（内置的、项目里的 `.chaos/workflows/`、用户目录的 `~/.chaos/workflows/`），每条给出它的来源、描述和路径。同一份目录也会列给模型看，放在会话前言里的技能清单下面。用 `/workflow <name>`（或它自己的斜杠命令）启动一个，然后到 `/workflow runs` 里观察它。

---

## 其他

### `/theme`

切换配色主题。别名：`/t`。

### `/feedback [message]`

报告问题或发送反馈。会打开一个报告面板：`Enter` 发送，`Esc` 丢弃。带的消息会预先填进面板，方便你在发送前修改。在 `--minimal` 下，带的消息仍然立即发送。

```
/feedback
/feedback Something isn't working correctly
```

### `/btw`

给代理发一句题外话，不打断当前任务。在最小模式（`--minimal`）下，回答会显示在提示框上方一个可关闭的面板里：`Esc` 关掉它，已经写好的回答会存进原生回滚区，而给一个已经关掉的面板送来的迟到回复会被丢弃。这条旁问和它的回答不属于主回合。

```
/btw also check the error handling
```

### `/mcps`

打开 MCP 服务器管理模态。

### `/doctor`

检查当前会话在终端、剪贴板、颜色、输入、通知和沙箱方面的问题。Doctor 会显示它发现了什么，以及每个问题该怎么解决。运行 `/doctor fix` 会列出可用的自动修复；其它发现会附上手工步骤。`/terminal-setup`、`/terminal-check`、`/terminal-info` 仍是别名。

### `/release-notes`

查看当前版本的发行说明。别名：`/changelog`。

### `/docs`

浏览内置的操作指南、打开线上 Build 文档，或者按标题直接跳到某篇指南。别名：`/howto`、`/guides`。

```
/docs
/docs web
/docs Getting Started
```

- 光敲 `/docs`（或 `/docs how-to`）会打开操作指南选择器。
- `/docs web` 在浏览器里打开 https://docs.x.ai/build/overview。
- `/docs <title>` 按不区分大小写的标题匹配打开某篇指南。

### `/tutorial`

打开上手教程：一份简短的话题清单（你的第一个提示词、附加上下文、导航、斜杠命令、工作树、计划模式、自定义、从别的代理工具迁移）—— 每篇大约读 30 秒，按 `→` 直接流到下一个话题。它不会自动弹出 —— 这条命令（或命令面板）就是入口。

```
/tutorial
```

别名：`/tour`、`/onboarding`

### `/import-claude`

打开 Claude 导入模态，把 `~/.claude` 的设置搬过来：权限、环境变量、MCP 服务器、钩子和路径。

---

## 代理与人设

### `/config-agents`

打开代理模态，查看和管理代理定义、设置默认项、切换当前生效的那个。别名：`/agents`。

它不是实时的多会话[代理看板](23-dashboard.md)（`/dashboard` / `Ctrl+\`）。

### `/personas`

创建、编辑和删除人设。子代理可以套用一个人设，来塑造自己的行为方式。

---

## 账号与数据

### `/login`

Chaos 不支持账号登录。这条命令**失败即关闭**：绝不启动浏览器授权流程，而是直接打开 `/provider` 面板，让你在那里配置 API Key。

### `/logout`

Chaos 没有需要退出的登录会话。这条命令只打印一条提示，让你去 `config.toml` 里改 provider 配置，或改用 `/provider`。

### `/usage`

查看本次会话的 token 用量和费用。别名：`/cost`。

```
/usage
/usage manage
```

它打开的是一个三标签的会话内模态：Context usage / Usage limit / Session info，默认停在 Usage limit。配置了外部认证提供方的安装里，这条命令会被隐藏。

要查看任何本地会话逐回合的 token 与费用累计，用 shell 里的 `chaos usage <session-id> [turn]`。见[会话管理](17-sessions.md#chaos-usage-子命令)。

### `/privacy`

在设置里打开 **Coding data, retention, and training**，在那里选
**Opt in** 或 **Opt out**。不带参数。

```
/privacy
```

这项设置不碰 `[features] telemetry`、`trace_upload`，也不碰你的外部 OTEL 设置 —— 见[用量监视](24-monitoring-usage.md#相关设置)。在团队账号上，只有团队管理员能改它；管理员还可以为团队打开或关闭 Zero Data Retention（[如何启用 ZDR](https://docs.x.ai/developers/faq/security#how-to-enable-zdr)）。当这不由你决定时，那一行会直接说明 —— `ZDR` 或 `· Admin Managed` —— 而不是打开选择器。ZDR 锁住的是编码数据共享；它不会屏蔽外部 OTEL，也不会屏蔽 `user.email` —— 见 [ZDR 与本数据流](24-monitoring-usage.md#zdr-与本数据流)。

---

## 配置与界面

### `/settings`

打开设置模态，以交互方式查看和修改配置。别名：`/config`、`/preferences`、`/prefs`。

### `/timestamps`

打开或关闭消息时间戳。

---

## 技能作为斜杠命令

任何已启用、且 SKILL.md frontmatter 里带 `user-invocable: true` 的技能，都会作为斜杠命令出现。（用 `/skills` 把技能关掉，它就不再被列出。）所以放在 `~/.chaos/skills/commit/SKILL.md` 的技能是这样运行的：

```
/commit fix typo in README
```

插件带来的技能同理。两个不同作用域的技能重名时，要加限定前缀：

```
/local:commit      # Project-scoped skill
/user:commit       # User-scoped skill
```

内置命令永远赢下裸名。把一个技能命名为 "compact"，`/compact` 跑的仍然是内置命令 —— 该技能以 `/local:compact` 继续可用（插件技能则是 `/acme:compact`）。两者都会出现在斜杠菜单里：内置的那条标为 `built-in`，技能那条标为 `skill · local` / `skill · acme`。

---

## 自动补全

菜单支持模糊搜索：在 `/` 后面开始输入即可过滤。每个条目显示命令名、描述、需要参数时的参数提示，以及它的来源（内置、技能作用域或插件名）。按 `Tab` 或 `Enter` 接受高亮的那条命令。
