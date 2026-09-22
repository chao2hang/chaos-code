# 子代理与人设

子代理是并行的独立子会话：每个子代理都有自己的上下文窗口，所以主代理可以把研究、
实现、测试、代码评审这类工作委派出去，而不占用自己的上下文。子代理结束时会把一份
摘要回传给父会话。

子代理默认开启。

---

## 代理与人设

代理和人设都能定制行为，但作用的层级不同：

| | **代理** | **人设** |
|---|---|---|
| **配置什么** | 整个会话：模型、工具、提示模式、系统提示 | 叠加在子代理提示之上的一层行为设定 |
| **作用域** | 主会话或子代理 | 仅子代理 |
| **如何设置** | 启动时指定，或用代理定义（`.chaos/agents/` 或 `~/.chaos/agents/` 下的 `.md` 文件） | 写在 `config.toml`（`[subagents.personas]`）或 `.chaos/personas/` 下的 `.toml` 文件里；在子代理解析时生效 |
| **控制什么** | 模型、可用工具、提示正文、技能 | 语气、输出格式、任务重心，以及输入/输出契约 |
| **谁来编辑** | 你 —— 在代理模态框里创建、删除或开关，也可以直接改文件 | 你 —— 在配置或文件里定义自定义人设；打包自带的内置人设只读 |
| **示例** | `grok-build`, `explore`, `plan` | `researcher`, `concise` |

代理定义的是会话本身；人设塑造的是子代理在会话里的行为方式。子代理总是以某个代理
类型运行（例如 `general-purpose`），解析时可以在其上再叠一层人设。

两者都在代理模态框里管理：用 `/config-agents`（别名 `/agents`）打开，或用 `/personas`
直接打开**人设**标签页。模态框有两个标签页：**代理** 与 **人设**。

---

## 关闭子代理

用环境变量或配置文件关闭子代理：

```bash
export GROK_SUBAGENTS=0              # Environment variable
```

```toml
# ~/.chaos/config.toml
[subagents]
enabled = false
```

---

## 子代理如何工作

主代理发现可以委派的工作时，会调用 `spawn_subagent` 工具启动一个子会话。子会话带着这些条件运行：

- 自己的上下文窗口，与父会话相互独立
- 由它的代理类型和可选的能力模式决定的工具集
- 解析时可选叠加的人设指令

子代理结束后，父会话会收到它的输出 —— 通常是一份摘要。

---

## 内置代理类型

`spawn_subagent` 工具接受一个 `subagent_type` 参数，用来选择子会话的类型：

| 类型              | 说明                                          |
| ----------------- | ---------------------------------------------------- |
| `general-purpose` | 默认类型。全能力代理，什么任务都能做。    |
| `explore`         | 研究型代理。会搜索、读取、用 grep 找内容并执行 shell 命令，但不改文件。适合用来调查代码库。 |
| `plan`            | 规划型代理。探索代码库并产出结构化的实现计划；不改文件。 |

项目级或用户级定义的代理可以新增类型，也可以按名字遮蔽这些内置类型。

---

## 人设

人设是一层有名字的行为设定。它的指令以 `<system-reminder>` 的形式注入子代理的对话，
用来塑造语气、输出格式和任务重心，而不改变子代理的代理类型、模型或工具。

人设可以写在 `config.toml` 里，也可以写在 `.toml` 文件里：

```toml
[subagents.personas.researcher]
instructions = "You are a thorough researcher. Always cite specific file paths."
description = "Deep investigator."
```

Chaos 按下面的顺序（优先级从高到低）发现文件形式的人设：

- `.chaos/personas/*.toml`（项目）
- `~/.chaos/personas/*.toml`（用户）
- 随程序打包的人设目录（优先级最低）

每个文件定义一个人设，文件名（去掉扩展名）就是人设名。写在 `config.toml` 里的内联
人设优先于文件。只发现 `.toml` 文件。

人设就在代理模态框的**人设**标签页里管理（`/personas`）。打包自带的内置人设只读；
你自己定义的人设可以编辑。

> **注意：** Chaos 是通过子代理解析与角色层来套用人设的，`spawn_subagent` 没有这个参数。
> 主代理派生子代理时不会传人设名。

### 人设字段

| 字段               | 说明                                                          |
| ------------------- | ------------------------------------------------------------------- |
| `instructions`      | 作为人设层套用的内联指令文本。               |
| `instructions_file` | 指令文件路径；派生时读取，合并到 `instructions` 之后。 |
| `description`       | 人设目录里显示的简短摘要。缺省时取 `instructions` 的第一段。 |
| `inputs` / `outputs`| 声明的输入与输出契约（见下）。                     |
| `model`             | 使用该人设时套用的模型覆盖。                    |
| `reasoning_effort`  | 使用该人设时套用的推理强度。                  |
| `default_isolation` | 默认隔离模式（`none` 或 `worktree`）。                      |

### 输入/输出契约

人设可以声明它期望的输入和它产出的输出。父代理读这些声明，就知道该提供什么上下文、
可以期待什么产物。这样就能把人设串起来：一个人设的输出文件就是下一个人设的输入：

```toml
[[subagents.personas.reviewer.inputs]]
name = "review_file"
io_type = "file"
required = true
description = "Path to the code under review"

[[subagents.personas.reviewer.outputs]]
name = "summary_file"
io_type = "file"
required = false
description = "Path to write review notes"
```

每个字段有 `name`、`io_type`（默认 `file`）、`required` 标志和 `description`。

### 人设解析

人设生效时，Chaos 按下面的顺序解析实际使用的模型与推理强度，优先级从高到低：

1. 派生时显式指定的覆盖值
2. 角色默认值
3. 人设默认值
4. 父会话

隔离模式前三条同理，但默认是 `none`（不建 worktree），而不是从父会话继承。

如果请求了某个人设却解析不出来 —— 找不到、没有指令、或 `instructions_file` 读不到 ——
派生就会失败。

---

## 派生子代理

主代理调用 `spawn_subagent` 工具。它的参数：

| 参数         | 说明                                                       |
| ----------------- | ---------------------------------------------------------------- |
| `prompt`          | 交给子代理的完整任务提示。                           |
| `description`     | 任务的简短标签（3-5 个词）。                          |
| `subagent_type`   | 要启动的代理类型。默认 `general-purpose`。         |
| `background`       | 让子代理在后台运行，立即返回一个子代理 ID。默认 `false`。 |
| `isolation`       | `none`（共享工作区，默认）或 `worktree`（隔离的 git worktree）。 |
| `resume_from`     | 继续某个已完成子代理的对话。传它的子代理 ID。 |
| `cwd`             | 子代理的工作目录。与 `isolation: worktree` 互斥；设了 `resume_from` 时忽略（续跑的子会话继承来源的目录）。 |

在后台运行子代理时，事后用 `get_command_or_subagent_output` 取它的结果。

### 给子代理发消息

`send_subagent_message` 工具默认关闭。用 `GROK_ACTIVE_AGENT_MESSAGES` 或
`[features] active_agent_messages` 打开。

根会话可以给自己拥有的子代理发后续消息。开关打开后，被授予该工具的子代理也会拿到它：

- `subagent_id: "parent"` 指向该子代理当前活跃的父级子代理。
- 一个持久化的代理 ID 指向另一个本地子代理。符合条件的已完成子代理会以同一个身份续跑。

父级是根会话的子代理不能给根会话发消息。精选的 harness 工具集永远拿不到这个工具。
排除了这一类的能力模式也会把它去掉。

子发送方有配额：每个发送方-目标对 4 条在途消息，每次发送尝试 32 条外发消息。
超限的发送返回 `QuotaExceeded`。

给不活跃的子代理发消息时，它总会以这条消息作为下一轮醒来。对于活跃的子代理，
可选的 `delivery` 参数决定消息怎么落地：

- `steer`（默认）在下一个安全点并入当前回合。
- `queue` 不进入当前回合，而是作为受保护的后续回合排队等待。
- `interject` 是紧急投递：它排在其他待投递的 steer 之前，在最早的安全点送出，
  并且会打断正卡在后台工作上的子代理，让它立刻读到这条消息。只有等待调用提前结束，
  后台工作照旧运行。

如果子代理是活跃的但正处在两个回合之间，`steer` 和 `interject` 都会变成一个受保护的
排队回合，子代理从该回合开始。旧的 `queue: true` 标志仍然接受，含义是
`delivery: "queue"`；两者同时出现时以 `delivery` 为准。

会话记录把每次发送显示为一行 `Message` 行：先是表示结果的动词，然后是子代理的标签
（它的类型、人设或角色）和加弯引号的描述 —— 与它那行 `Subagent …: “…”` 滚动回溯
行引用的内容一致，截到第一行且最多 40 个字符。投递方式由动词体现，所以 steer 不带
标记：

- `Message sent to Explore “find callers”`（steer）
- `Message queued for Explore “find callers”` / `Message interjected to Explore “find callers”`
- `Message sending to …`，发送在途时配一个动画圆点
- 被拒绝的发送是 `Message rejected · Explore “find callers”`；shell 无法确认的是
  `Message unconfirmed · Explore “find callers”`
- 子代理给父会话发消息时是 `Message sent to parent`

折叠行不显示消息正文，也不显示原因。**Right**（vim 模式下 `l`/`e`）展开这一行，
显示请求的投递方式、完整的消息正文，以及被拒绝或无法确认的原因；**Left**（或 `h`）
再次折叠。**Enter**、**Ctrl+F** 或在行上双击会打开该子代理的视图，与在它的
`Subagent` 行上操作完全一致（Right/Left 仍然只做折叠）。如果这个子代理从未在本会话里
派生过（无头模式下的 `chaos export`，或来自别的会话的 ID），这一行会用它 ID 的最后
8 个字符命名为 `subagent …xxxxxxxx`，展开时显示原始的 `Subagent ID:`，并且无法打开。

---

## 能力模式

能力模式不是派生参数。子会话的工具来自它的**代理类型**和**角色/定义里的默认值**。
`general-purpose` 不受限制（`all`）。内置的 `explore` 和 `plan` 会读取、搜索并执行
shell 命令，但不能改文件。

| 模式         | 读 | 写 | 执行 | 说明                                  |
| ------------ | ---- | ----- | ------- | -------------------------------------------- |
| `read-only`  | 是  | 否    | 否      | 读取、搜索、检视（也包括网页搜索与 LSP）；不改文件，也不执行 shell。 |
| `read-write` | 是  | 是   | 否      | 读取，外加创建、修改、删除、移动文件。不执行 shell。 |
| `execute`    | 是  | 否    | 是     | 读取，外加执行 shell 命令和后台任务。不改文件。 |
| `all`        | 是  | 是   | 是     | 工具不受限制。`general-purpose` 的默认值。 |

---

## 上下文继承

### resume_from

`resume_from` 参数让新的子代理接着某个已完成子代理的位置继续，多阶段流程里很有用：

1. 派生一个研究子代理去调查问题。
2. 派生第二个子代理，把 `resume_from` 设为第一个子代理的 ID，它就能带着完整的研究
   上下文接手。

新子代理会继承来源的会话记录、工具状态和模型；它的系统提示和工具按当前的代理定义
重新渲染。来源必须处于已完成状态（不是在运行），属于当前会话，并且使用相同的代理类型。

### MCP 继承

子代理默认继承父会话**已经连上**的 MCP 服务器。这包括本地 stdio/HTTP 服务器和来自
插件的代理（例如 `my-plugin:reviewer`）。子会话发现和调用这些工具的方式与父会话相同，
都用 `search_tool` / `use_tool`。

继承行为由代理 frontmatter 里的 `mcpInheritance` 控制：

| 取值 | 效果 |
| ----- | ------ |
| `all`（省略时的默认值） | 继承父会话连上的每一台 MCP 服务器 |
| `none` | 不继承任何父会话的 MCP 服务器 |
| `named: [server, …]` | 只继承列出的服务器名 |
| `except: [server, …]` | 继承父会话所有服务器，除了列出的名字 |

例子：

```yaml
---
name: research-only
description: Read MCP tools but not internal connectors
tools: search_tool, use_tool, Read
mcpInheritance:
  except:
    - internal-tools
---
```

**插件代理**以同样的方式继承父会话的 MCP。出于安全考虑，它们仍然不能：

- 在代理 frontmatter 里声明自己的 `mcpServers`（会被忽略并打一条警告）
- 在代理 frontmatter 里声明钩子
- 设置 `permissionMode: bypassPermissions`

插件随附的 MCP 服务器（插件的 `.mcp.json`）在插件被信任之后仍然挂在**父会话/会话**上
—— 它们不是子会话专属的 frontmatter 声明。见[插件](09-plugins.md)与
[MCP 服务器](07-mcp-servers.md)。

---

## 隔离：Worktree 模式

对于要改文件的任务，用 `isolation: worktree` 把子代理放进一个隔离的 git worktree 里跑。
这样它的改动就不会和父会话冲突：

- 子代理在自己的工作区副本里干活。
- 在你合并之前，它的改动与父会话隔离。
- 子代理的结果里会带上 worktree 路径。

Chaos 通过 `x.ai/git/worktree/*` 扩展方法管理 worktree，其中包含一个把改动合并回主
工作目录的 apply 操作。

---

## 配置

### 每种类型的开关与模型覆盖

可以关掉特定的代理类型，或把它们路由到别的模型：

```toml
[subagents.toggle]
explore = true                       # default -- omit to keep enabled
plan = false                         # disable the plan subagent

[subagents.models]
explore = "grok-4.6"                 # route explore to a specific model
```

每种类型的模型覆盖对任何父会话都生效；没有覆盖时，子代理继承父会话的模型。

### 自定义角色与人设

定义带自己的能力模式与模型默认值的自定义角色：

```toml
[subagents.roles.researcher]
description = "Deep research agent"
default_capability_mode = "read-only"
model = "grok-4.6"
prompt_file = ".chaos/prompts/researcher.md"
```

定义带行为指令的自定义人设：

```toml
[subagents.personas.concise]
instructions = "Be concise. No filler words."
# instructions_file = ".chaos/personas/concise.md"  # or load from a file
```

Chaos 还会从 `.chaos/roles/*.toml` 发现角色、从 `.chaos/personas/*.toml` 发现人设。
写在 `config.toml` 里的内联定义优先于文件。

---

## 任务面板（TUI）

Chaos 在代理界面的侧栏里显示正在运行和已完成的工作：

- 按 `Ctrl+G` 开关任务面板，它列出活跃与已完成的子代理和后台命令及其状态。
- 按 `Ctrl+T` 开关另一个待办面板。

要看有哪些可用的代理类型和人设，用 `Ctrl+P` 打开命令面板，选 **管理 Agent**
（`/config-agents`）。

子代理在任务面板顶部单独归入可折叠的「子代理」分组。

---

## 在 TUI 里查看子代理

子代理在交互式 TUI 里出现在好几处：

### 滚动回溯（父会话历史）

子代理被派生时，*父会话*的滚动回溯里会插入一个紧凑的生命周期块：

- `子代理运行中：“去做那件事” · 思考中 (Implementer · grok-4.6)`
- 后台子代理则是：`子代理已启动：“…”`

运行期间，这个块会显示一个来自子会话回合跟踪器的实时活动后缀（例如「运行: cargo test」、
「压缩中」、「重试中 (2/3)」）。圆点按状态做动画（或着色）。

在块上按 **Enter**（或 Ctrl-F）打开该子代理的完整会话记录。

阻塞式子代理的这条记录会在子会话结束时改变圆点颜色。后台子代理则会再追加一条
`子代理已完成/失败/已取消（用时 Xs）：“…”` 块。

### 任务面板（Ctrl+G）

如前所述 —— 归在「子代理」分组下，带转圈动画、已用时间和快速终止或检视的入口。
按 `h` 在「隐藏已完成」与「显示全部」之间切换。

### 停靠栏（启用时）

提示框上方的停靠栏会列出子代理。停靠栏获得焦点时，`h` 在「隐藏已完成」与「显示全部」
之间切换（与任务面板同一个过滤器）。Left / Right 折叠与展开分组标题。

### 全屏框架视图（子会话记录）

打开某个子代理时（从滚动回溯块、任务面板或看板的某一行打开），父视图会被一个带边框
的框架替换，显示子会话的完整记录：

- 框架内的标题栏：状态图标（转圈 / ✓ / ✗）、标签 + 加粗描述 + 模型、可选的
  `resumed`/`forked` 徽章、实时活动 · 已用时间，以及 [✗] 关闭按钮。
- 子会话自己的滚动回溯、思考与工具调用都渲染在框架内。
- 查看期间，父会话的任务面板、待办面板、停靠栏和目录都会隐藏。

这个视图是只读观察用的。输入框被隐藏（占零行）。你不能聚焦它、输入提示、暂存草稿，
也不能从这里发后续消息。提示仍然归父会话所有。要给运行中的子代理下指令，请关闭该视图，
在父会话里用 `send_subagent_message`（见[给子代理发消息](#sending-messages-to-subagents)）。

**仍然可用的操作**

- 在子会话的记录里滚动、折叠、复制、打开链接，以及打开块查看器。
- `Ctrl+C` 取消**这个子会话**的回合。它不会取消父会话。
- `Ctrl+.` / `Ctrl+X` 打开子会话按键的快捷键速查表。
- 接管头部里的看板控件（`[Dashboard]`、`‹` / `›`）仍然作用于**父会话**。
- 在**块查看器**里空闲时按 `Enter` 会把选中的行引用到父会话的输入框并关闭视图。

**什么都不做的操作（fail closed）**

只属于根会话的组合键在这个界面上永远不会启动。它们不会在子会话上打开模态框，也不会
漏给父会话：

- 命令面板（`Ctrl+P`）、模型选择器（`Ctrl+M`）、会话选择器（`F3`）
- 设置、扩展、始终批准（`Ctrl+O`）、发送到后台（`Ctrl+B`）
- 外部提示编辑器、Shift+Tab 模式循环

被拒的操作只是静默重绘。没有 toast 提示。

如果出现提示队列浮层，那是**只读镜像**。你不能编辑、立即发送或删除行。队列 RPC 始终
作用于父会话。

**怎么退出**

- 在裸滚动回溯里按 `q` 或 `Esc`，或点击 [✗]。
- 如果滚动回溯搜索是打开的，`q` / `Esc` 先关闭搜索；再按一次才关闭视图。
- `Ctrl+Q` 始终退出 Chaos，在这里绝不会被吞掉。

关闭之后，父会话的滚动回溯仍然显示该子代理的状态。

---

## 嵌套深度上限

只有顶层会话能派生子代理。子代理不能再派生自己的子代理：最大嵌套深度是一。子代理
调用 `spawn_subagent` 时会因深度超限而失败。这样代理树保持扁平，也避免失控派生。

---

## 什么时候该用子代理

**适合的场景：**

- 父会话继续做别的事，同时研究代码库
- 父会话在改代码，同时并行跑测试
- 提交之前评审生成出来的改动
- 委派彼此独立、互不依赖的任务

**不适合的场景：**

- 父会话直接就能处理的小任务
- 需要与用户反复来回确认的任务 —— 子代理是自主运行的，不适合交互式交流
- 上下文准备成本高于并行收益的任务
