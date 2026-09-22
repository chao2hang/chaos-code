# 无头模式与脚本

无头模式让 Chaos 在命令行里非交互地运行。它接收单个提示，以完整的工具访问权限执行，
并返回结果。可以用它自动化任务、编写工作流脚本、构建集成，以及以程序化方式解析输出。

---

## 基本用法

以非交互方式传入提示即触发无头模式。最常见的是 `-p` 标志（`--single` 的
简写）；`--prompt-json` 与 `--prompt-file` 同样会触发它：

```bash
chaos -p "Your prompt here"
```

Grok 会处理该提示，运行所有必要的工具，并把结果打印到 stdout。响应完成后进程即退出。

---

## 命令行选项

| 标志                    | 说明                                           |
| ----------------------- | ----------------------------------------------------- |
| `-p, --single <PROMPT>` | 要发送的提示（或改用 `--prompt-json` / `--prompt-file`） |
| `-m, --model <MODEL>`   | 要使用的模型（例如 `grok-4.6`）              |
| `-s, --session-id <ID>` | 以该 **UUID** 创建**新**会话（UUID 无效或在目标会话目录下已被占用时报错；不会恢复会话，恢复请用 `-r`/`-c`） |
| `--fork-session`        | 与 `-r`/`-c` 连用时，分叉到一个新的会话 ID，而不是追加到原会话 |
| `-r, --resume <ID_OR_TITLE>` | 按 ID 恢复既有会话，或按当前目录的标题恢复（忽略大小写；重名时唯一一个被手动改名的匹配胜出，其余重名项报错并列出其 ID；形如 UUID 的值总是走 ID 路径；脚本应优先使用 ID） |
| `-c, --continue`        | 继续当前目录中最近的一个会话  |
| `--cwd <PATH>`          | 设置工作目录                                 |
| `--output-format <FMT>` | 输出格式：`plain`、`json`、`streaming-json`、`streaming-messages-json` |
| `--include-partial-messages` | 发出原始的 `stream_event` 增量。仅对 `--output-format streaming-messages-json` 生效；其他格式会被忽略（并给出警告）。 |
| `--yolo`                | 自动批准所有工具执行                      |
| `--rules <TEXT>`        | 用于系统提示的自定义规则                    |
| `--tools <TOOLS>`       | 内置工具的白名单（逗号分隔）。除非被拒绝，MCP 元工具仍然可用。仅无头模式。 |
| `--disallowed-tools <TOOLS>` | 要移除的内置工具黑名单（逗号分隔）。支持 `Agent` 条目。仅无头模式。 |
| `--max-turns <N>`       | 停止前允许的最大智能体轮数。仅无头模式。 |
| `--reasoning-effort` / `--effort <LEVEL>` | 推理模型的推理投入程度。规范级别：`none`、`minimal`、`low`、`medium`、`high`、`xhigh`、`max`（每个都是独立档位；模型只接受其菜单中列出的级别）。也接受按模型的菜单选项 id（例如 `deep` → 映射的线上传输值），与 `/effort` 相同。在 TUI 和无头模式下均可用。 |
| `--permission-mode <MODE>` | 权限模式。`bypassPermissions` 启用始终批准（见[权限与安全](22-permissions-and-safety.md#权限模式)）；要默认拒绝，请在 `.claude/settings.json` 中设置 `defaultMode`。 |
| `--allow <RULE>`        | 带通配符模式的权限允许规则（可重复）。在 TUI 和无头模式下均可用。 |
| `--deny <RULE>`         | 带通配符模式的权限拒绝规则（可重复）。在 TUI 和无头模式下均可用。 |
| `--prompt-json <JSON>`  | 以 JSON 内容块表示的提示                         |
| `--prompt-file <PATH>`  | 从文件读取提示                                    |
| `--verbatim`            | 按原样发送提示                          |
| `--no-auto-update`      | 为本次会话禁用更新检查                |
| `--sandbox <PROFILE>`   | 用于文件系统/网络访问的沙箱配置         |

> **注意：** `--tools`、`--disallowed-tools`、`--max-turns` 和 `--agents` 是仅限无头模式的标志。如果在交互式 TUI 中使用，会打印警告并忽略该标志。`--reasoning-effort`/`--effort`、`--permission-mode`、`--allow` 和 `--deny` 在两种模式下都可用。更多标志（智能体与工作树）见[其他无头模式标志](#其他无头模式标志)。

### 工具过滤

使用 `--tools` 把智能体限制在一组显式工具（白名单），或用 `--disallowed-tools` 从默认集合中移除特定工具（黑名单）。两者都接受逗号分隔的工具名。

工具名是内部工具 ID（例如 shell 工具是 `run_terminal_cmd`，而不是 `bash`）。

```bash
# Only allow read-only tools
chaos -p "Explain this codebase" --tools "read_file,grep,list_dir"

# Remove web access and file editing
chaos -p "Review this code" --disallowed-tools "web_search,web_fetch,search_replace"

# Remove shell access
chaos -p "Review this code" --disallowed-tools "run_terminal_cmd"
```

`--disallowed-tools` 还支持特殊的 `Agent` 条目，用于控制子智能体的派生：

| 条目                  | 效果                                  |
| ---------------------- | --------------------------------------- |
| `Agent`                | 阻止所有子智能体派生             |
| `Agent(explore)`       | 仅阻止 `explore` 类型的子智能体  |
| `Agent(explore, plan)` | 阻止多个指定类型           |

```bash
# Prevent the agent from spawning any subagents
chaos -p "Fix this bug" --disallowed-tools "Agent"

# Block only the explore subagent
chaos -p "Refactor this module" --disallowed-tools "Agent(explore)"
```

`--tools` 会保留所选智能体配置的注入策略：出厂配置（stock profiles）会在应用白名单之前先注入已启用的可选工具，而精选配置（curated profiles）则保持严格。最终工具集会保留所请求的工具，加上始终开启的 MCP 元工具。两个标志同时出现时，`--disallowed-tools` 优先。

### 权限规则（`--allow` / `--deny`）

权限规则控制特定工具调用是被自动批准、拒绝，还是需要用户确认。与 `--disallowed-tools`（彻底移除工具）不同，权限规则保留工具可用，但对其执行加以门控。

规则使用 `ToolPrefix(glob_pattern)` 语法：

| 前缀        | 控制内容                   |
| ------------- | ---------------------------------- |
| `Bash(...)`   | Shell 命令执行            |
| `Edit(...)`   | 文件编辑（路径通配符）           |
| `Write(...)`  | 文件写入（路径通配符）           |
| `Read(...)`   | 文件读取（路径通配符）           |
| `Grep(...)`   | 搜索操作（路径通配符）      |
| `WebFetch(...)` | URL 抓取（通配符或 `domain:host`） |
| `MCPTool(...)` | MCP 工具调用              |

对于路径规则（`Read`、`Edit`、`Write`、`Grep`），`*` 是单层通配符，`**` 是递归通配符。对于 `Bash` 规则，`*` 匹配包括空格在内的任意字符。不带括号的裸前缀匹配该类型的所有调用，`Bash(cmd:*)` 等价于对 `cmd` 的前缀匹配。完整的匹配语义见 [22-permissions-and-safety.md](22-permissions-and-safety.md#规则匹配参考)。

```bash
# Deny shell commands matching "rm*"
chaos -p "Clean up this project" --deny "Bash(rm*)"

# Allow npm commands, deny sudo
chaos -p "Set up the project" --allow "Bash(npm*)" --deny "Bash(sudo*)"

# Allow all bash commands (auto-approve without prompting)
chaos -p "Build the project" --allow "Bash"
```

`--allow` 和 `--deny` 可以重复使用。拒绝规则优先于允许规则。

---

## 输出格式

无头模式支持四种输出格式，通过 `--output-format` 选择。

### plain（默认）

人类可读的文本，适合直接显示或通过管道传递：

```
Here's a summary of the codebase...
```

### json

响应完成后发出的单个 JSON 对象：响应文本、停止原因、会话 ID、请求 ID（存在推理时还有 `thought`）。
当提示已到达模型时，同一对象还会携带花费字段
（`usage`、`num_turns`、`modelUsage`、成本）。`stopReason` 是 snake_case 的
ACP/Messages 令牌（`end_turn`、`max_tokens`、…）。

```json
{
  "text": "Here's a summary of the codebase...",
  "stopReason": "end_turn",
  "sessionId": "abc123",
  "requestId": "xyz789",
  "num_turns": 7,
  "usage": {
    "input_tokens": 7210,
    "cache_read_input_tokens": 41000,
    "cache_creation_input_tokens": 0,
    "output_tokens": 1893,
    "reasoning_tokens": 412,
    "total_tokens": 50103
  },
  "modelUsage": {
    "grok-4.6": {
      "inputTokens": 7210,
      "outputTokens": 1893,
      "cacheReadInputTokens": 41000,
      "modelCalls": 7,
      "costUSD": 0.01268905
    }
  },
  "total_cost_usd": 0.01268905,
  "total_cost_usd_ticks": 126890500
}
```

用法说明：

- `usage` 汇总该提示的 token，包括在轮次结束前已完成的子智能体
  （它们也各自记在 `modelUsage` 键下）。压缩和其他侧模型调用不计入。
- **Token 字段策略（无头结果 / `end` / 错误花费）：**
  - `usage.input_tokens` 与 `modelUsage.*.inputTokens` **仅计未命中缓存的部分**。
  - `cache_read_input_tokens` / `cacheReadInputTokens` 是缓存命中。
  - `total_tokens` 是完整的输入 + 输出（包含两个缓存桶）：
    `total_tokens = input_tokens + cache_read_input_tokens + cache_creation_input_tokens + output_tokens`。
  - ACP 的 `_meta.usage.inputTokens`（PromptUsage）仍是**完整的**提示
    总和；只有无头投影器会扣除缓存。做花费自动化时优先使用无头字段。
- `num_turns` 统计记录在提示账本上的主智能体模型轮次
  （上报了用量的工具循环轮次）。子智能体的采样调用不会增加它。
  按模型的调用次数（包括子智能体）记在 `modelUsage.*.modelCalls` 上。
  它与 `--max-turns` 属于同一族计数器，但当某些轮次缺少用量或触发门限时，并不保证严格相等。
- `total_cost_usd` 只在服务端上报了**完整**花费时出现。
  缺失意味着未上报或不完整，绝不等于免费。目前只有 API key 流量会打上
  花费；pool/OAuth 路径常常不带，直到服务端补上。当部分调用缺少花费时，
  `cost_is_partial` 为 true，且**所有**花费浮点数都被省略
  （`total_cost_usd` 与每一个 `modelUsage.*.costUSD`），
  这样消费者就无法把各模型行加总成一张
  看起来完整的假账单。
- `total_cost_usd_ticks` 是同一个值的精确整数刻度表示
  （1 USD = 10^10 ticks），出现条件也相同。它最适合用来
  对账：把每次调用的刻度相加，能与服务端的用量导出精确吻合，
  这一点浮点美元无法保证。
- 当子智能体用量无法应用、嵌套的子智能体用量不完整，
  或成功路径的收尾等待超时（在回合任务上最长 120 秒）时，
  `usage_is_incomplete` 为 true，花费浮点数以同样方式省略
  （token 总数可能少算子智能体）。取消产生的快照不走那段长等待，
  并且在子智能体仍活跃时就标记为不完整。不完整且没有记录到 token 时，
  只发出 `usage_is_incomplete`（不带全零的 `usage` 对象）。
- 从未到达模型的提示会省略这些花费字段。

`sessionId` 字段可用于之后恢复该会话。

失败时，Chaos 会发出一个错误对象（进程以非零码退出）。提示层面的失败
在已记录用量时，也可能带上冻结的花费字段：

```json
{"type":"error","message":"Couldn't start session: ..."}
```

### streaming-json

换行分隔的 JSON，每行一个带 `type` 标签的对象，由智能体的 ACP 会话更新派生而来。叶子字段名（`toolCallId`、`kind`、`rawInput`、`rawOutput`）沿用 ACP；`toolName` 与 `usage` 行是 xAI 的增补。消费时按 `type` 分支即可。

```json
{"type":"thought","data":"Analyzing the directory structure..."}
{"type":"tool_call","toolCallId":"call_1","title":"Read","kind":"read","status":"in_progress","toolName":"read_file","rawInput":{"path":"src/main.rs"},"content":[],"locations":[]}
{"type":"tool_call_update","toolCallId":"call_1","status":"completed","content":[],"rawOutput":{"lines":42},"locations":[]}
{"type":"text","data":"Here's a summary"}
{"type":"usage","messageId":"resp_1","stopReason":"end_turn","usage":{"input_tokens":812,"output_tokens":45,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"reasoning_tokens":0},"signature":"..."}
{"type":"end","stopReason":"end_turn","sessionId":"abc123","requestId":"xyz789","usage":{...},"num_turns":7,"modelUsage":{...}}
```

事件类型：

| 类型               | 说明                                                                                  |
| ------------------ | ------------------------------------------------------------------------------------------- |
| `text`             | 智能体响应文本的一个片段                                                          |
| `thought`          | 内部推理（思考 token）                                                          |
| `tool_call`        | 智能体发起的一次工具调用（`toolCallId`、`toolName`、`kind`、`status`、`rawInput`、`content`、`locations`） |
| `tool_call_update` | 某次工具调用的进度或结果（`status`、`rawOutput`、`content`、`locations`）            |
| `usage`            | 单次模型响应的边界（`messageId`、`stopReason`、`usage`、`signature`），每个模型响应一条 |
| `plan`             | 智能体当前的计划（`entries`）                                                          |
| `available_commands` | 工具与斜杠命令清单（`tools`、`commands`）                                          |
| `end`              | 最终事件，带元数据和（有则带）花费字段                                    |
| `error`            | 出错了（带 `message`，有花费字段则一并带上）                               |

`end` 永远是最后一个事件。`end` 上的花费字段与 json 对象形状一致
（snake_case 的未命中缓存 `input_tokens`、安全的花费浮点数）。
`end.stopReason` 是回合停止原因，snake_case（`end_turn`、`max_tokens`、
`max_turn_requests`、`refusal`、`cancelled`）；每个响应的原始服务商原因
（如 `tool_use`、`pause_turn`）在 `usage` 行的 `stopReason` 上。
每个响应的 `message_id`/`stopReason`/`signature` 在 Messages API 后端
会被填充；其他后端只上报它们确实携带的内容。

Chaos 还可能发出 `max_turns_reached` 与 `auto_compact_*` 事件；请把上面的清单视为非穷尽，按 `type` 分支处理。

### streaming-messages-json

换行分隔的 JSON，采用 Messages API 的 `stream-json` 线上格式。承载数据的部分与 Messages 形状完全一致，包括 `assistant`/`user` 消息体、`usage`、`tool_use`/`tool_result`、内联网页搜索、`stop_reason`，以及 `--include-partial-messages` 的事件框架。用于重建消息、读取花费或检测错误的消费者无需改动即可工作。

`system`/`init` 行与结尾的 `result` 行承载元数据。Chaos 只发出它确实有数据的字段，填不上的纯占位字段一律省略，而不是补零。因此这两行可能通不过严格的 `init`/`result` schema 校验。各字段列在下面。把任何单个字段当作权威之前，请先读保真度说明。若想要一条干净、没有占位形状的 xAI 原生流，请用 `streaming-json`。

这条流以 `system`/`init` 行开头，随后是 `message.content[]` 里带 `text`、`thinking`、`tool_use` 块的 `assistant` 消息、带 `tool_result` 块的 `user` 消息，最后是一个结尾 `result`：

```json
{"type":"system","subtype":"init","session_id":"abc123","apiKeySource":"user","model":"grok-4.6","cwd":"/repo","permissionMode":"default","tools":["read_file","bash"],"slash_commands":["review"],"mcp_servers":[{"name":"linear","status":"connected"}],"skills":[],"uuid":"..."}
{"type":"assistant","message":{"id":"msg_0","type":"message","role":"assistant","model":"grok-4.6","content":[{"type":"text","text":"Let me read the file."},{"type":"tool_use","id":"call_1","name":"read_file","input":{"path":"src/main.rs"}}],"stop_reason":"tool_use","stop_sequence":null,"usage":{...}},"parent_tool_use_id":null,"session_id":"abc123","uuid":"..."}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call_1","content":"fn main() {}","is_error":false}]},"parent_tool_use_id":null,"session_id":"abc123","uuid":"..."}
{"type":"result","subtype":"success","is_error":false,"duration_ms":0,"duration_api_ms":0,"num_turns":7,"result":"Here's a summary...","stop_reason":"end_turn","total_cost_usd":0.0127,"usage":{"input_tokens":812,"output_tokens":210,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"server_tool_use":{"web_search_requests":0}},"modelUsage":{},"session_id":"abc123","uuid":"..."}
```

消息类型：

| 类型        | 说明                                                              |
| ----------- | ---------------------------------------------------------------------- |
| `system`    | 会话前导（`subtype: "init"`），含模型、cwd、权限模式、工具、斜杠命令与 MCP 服务器。`subtype: "compact_boundary"` 标记一次自动压缩 |
| `assistant` | 一条模型消息；`message.content[]` 里是 `text`/`thinking`/`tool_use`，内联后端网页搜索时还会有 `server_tool_use`/`web_search_tool_result` |
| `user`      | 工具结果，以 `tool_result` 块的形式放在 `message.content[]` 里         |
| `result`    | 结尾消息，含最终文本、停止原因与花费字段         |

`assistant` 与 `user` 消息带 `session_id`、`uuid` 和 `parent_tool_use_id`（主对话为 `null`）。`system`/`init` 行与结尾的 `result` 行带 `session_id` 和 `uuid`，但没有 `parent_tool_use_id`。

每一行的 `uuid` 都是该行发出时新生成的。它不是服务商 id、消息 id 或事件 id，也不是关联键。它与服务商的 `message.id` 不同（后者挂在 `assistant.message.id` 上）。它逐行唯一，即使多行描述同一条消息也各不相同，并且不携带跨行或跨运行的身份。不要用它做关联或去重。

文本与推理片段按每个模型响应归组成一条 assistant 消息。一个响应里并行的 `tool_result` 块归组成一条 `user` 消息。`result.result` 是最后一条 assistant 消息的文本。不产生任何内容块的模型响应在默认模式下不发 `assistant` 行；只有 `--include-partial-messages` 会把它露出来，形式是空的 `message_start` … `message_stop` 信封。

`init` 上的 `skills` 是实时的。它列出该会话可供用户调用的技能名，是 `slash_commands` 的子集，取自会话公布的命令；会话没有技能时是 `[]`。`init` 行只发一次，且推迟到第一条输出行，以便把会话公布的 `tools`、`slash_commands` 和 `skills` 一并收进来。Messages schema 没有定义第二个 `init`，所以流开始后才变化的命令列表不会再公布。

其余 `init` 字段都带真实数据：

- `apiKeySource` 在用 API key 认证时是 `user`，否则是 `oauth`。schema 里的 `project`、`org`、`temporary` 三种来源 Chaos 不作区分。
- `permissionMode` 是映射到 Messages 枚举后的实际无头模式：`--permission-mode` 的值，`--yolo` 下为 `bypassPermissions`，其余情况为 `default`。`auto` 这类 Chaos 独有的模式会塌缩成 `default`。
- `mcp_servers[].status` 是一次 `x.ai/mcp/list` 快照，只为 `streaming-messages-json` 发出：`connected`、`failed`、`needs-auth`、`pending` 或 `disabled`。仍在握手的服务器是 `pending`。`disabled` 只有在会话上报 `sessionMcpResolved` 之后才会打上；即使 `enabled` 仍为 false，未解析的清单行也是 `pending`。该快照不等 Blocking 启动宽限期，那段宽限期仍作用于提示的工具集。其他输出格式会省略该数组，也不会调用 `x.ai/mcp/list`。

schema 里纯占位、Chaos 又没有数据的 `init` 字段一律省略，不发占位值：`claude_code_version`、`output_style` 和 `plugins`。

`result` 包含 `duration_ms`、`duration_api_ms`、`num_turns`、`stop_reason`、`total_cost_usd`、`usage`（Messages API 的 `message.usage` 形状）以及 `modelUsage`。错误子类型上还会包含 `errors[]`。schema 里恒为空的 `permission_denials` 被省略，因为 Chaos 不收集权限拒绝记录。`structured_output`（配合 `--json-schema`）是 snake_case，与 schema 一致。

`model` 出现在 `init` 和每个 `assistant` 帧上。已知时是真实模型 id；只有在发出时还不知道模型的情况下才会是字面量 `"unknown"`。

assistant 帧的 `stop_sequence` 是端到端连通的。当模型停在某个已配置的停止序列上（`stop_reason: "stop_sequence"`）时，它携带服务商匹配到的那个序列；其他停止原因和其他后端上都是 `null`。在 `--include-partial-messages` 框架下，匹配到的序列同时出现在刷出的 `assistant` 帧和局部的 `message_delta.stop_sequence` 上，因此用局部流重建的结果与帧一致。只有局部的 `message_start.stop_sequence` 保持 `null`，因为消息打开时还不知道匹配到了哪个序列。

会发出的错误子类型是 `error_max_turns`、`error_during_execution` 和 `error_max_structured_output_retries`。schema 里的 `error_max_budget_usd` 子类型永远不会发出，因为 Chaos 没有预算功能。

`result.usage` 上报 Messages 的 `message.usage` 形状，三个 token 桶互不相交：`input_tokens`（未命中缓存）、`cache_read_input_tokens` 和 `cache_creation_input_tokens`。Chaos 从回合的汇总账本推导这些值，再重塑进对应的桶。子智能体的缓存创建计入 `cache_creation_input_tokens`；汇总账本把它当作独立的一桶，因此不再折进 `input_tokens`。

`result.usage` 总是发出数值桶，即使数据缺失。数据缺失发生在两种情形：该回合的用量账本不完整（与 `json` 格式里出现 `usage_is_incomplete` 的条件相同），或者根本没有汇总账本到达归约器。凡是 Chaos 算不出来的桶都退回 `0`，因为 Messages API 的 schema 没有标记「不完整」或「缺失」用量的方式。两种情形下归约器都会向 stderr 记一条警告。这里全零的 `usage` 要读作「未知」，而不是「免费」。

嵌套的 `server_tool_use` 计数器会被填充。`web_search_requests` 是本次运行发出的**成功**后端网页搜索次数。失败的搜索以及 open_page 这类非搜索的 `WebSearch` 动作不计入，与 Messages API 一致——它对报错的搜索不计费。失败的后端搜索仍会以错误形状发出一个 `web_search_tool_result`（`content.type: "web_search_tool_result_error"`），但不计数。它的 `error_code` 是固定的 `"unavailable"` 占位值，不是从后端透传的代码。没有 `web_fetch_requests` 键，因为 Chaos 没有服务端的 `web_fetch`，所以该占位项直接省略。

后端网页搜索是内联的，与前后文本折进同一个 `assistant` 帧。该帧带一个 `server_tool_use` 块（`name: "web_search"`、`input.query`），紧跟一个 `web_search_tool_result` 块。结果块的 `tool_use_id` 与 `server_tool_use.id` 相同，其 `content` 是 `{type, url, title}` 形式的 `web_search_result` 命中数组。这与 Messages API 的内联服务端工具形状一致，而不是把响应拆到多个帧里。

X 搜索与代码解释器是一处有记录的差异。它们保持通用形式，以一个客户端 `tool_use` 块加一条 `user` `tool_result` 呈现，因为 Messages API 没有为它们定义内联块类型。其余客户端工具同样保持 `tool_use`/`tool_result` 的拆分。

`--include-partial-messages` 发出原始事件框架，消费者可以用 Messages 的流式累加器重建每条消息。框架是 `message_start`、`content_block_start`/`content_block_delta`/`content_block_stop`、`message_delta` 和 `message_stop`，携带累加器所需的结构性事件。这些增量比 Messages API 的 token 级流式更粗：工具输入以一个 `input_json_delta` 到达，`citations_delta` 从不产生（见下）。结果是每条消息的忠实重建，而不是逐 token 回放。

在 Messages API 后端上，框架是忠实的。`message_start` 携带真实的 `message.id` 和输入侧的 `usage`。思考块按顺序在块的 `content_block_stop` 之前发出自己的 `signature_delta`。`message_start.usage` 的输入侧上报消息打开时已知的全部三个提示侧桶：`input_tokens`（未命中缓存的部分）、`cache_read_input_tokens` 和 `cache_creation_input_tokens`。因此缓存命中在 `message_start` 上就可见，而不是只在后面的 `message_delta`/`result` 上出现。`output_tokens` 在那里先置 `0`，在 `message_delta` 上定稿。已经开始但不产生任何内容的响应，仍会发出不带内容块的 `message_start` … `message_stop` 信封。

有些后端只在回合结束时才给出每个响应的元数据。这些后端会退回合成的 `message_start.id` 和置零的输入 `usage`，并把推理的 `signature` 推迟到最后的 `assistant` 行；在那种情况下以那一行为准。

工具调用的输入以一个 `input_json_delta` 发出，携带完整的参数 JSON，随后是 `content_block_stop`，而不是一串 token 级碎片。这是与 Messages API 增量式 `partial_json` 流的有意差异。Chaos 的 ACP 工具调用路径在参数完全解析后，把每次工具调用作为一个校验过的 JSON 对象交付，因此单个增量才是准确的表示。拼接 `partial_json` 的消费者两种情况下重组出的对象完全相同。后端网页搜索 `server_tool_use` 块的 `input.query` 也以同样方式发出，即一个 `input_json_delta`。

Messages API 的 `citations_delta` 携带被引用文本片段的内联引用，例如来自网页搜索的那些。这条流不产生它。Chaos 的 Messages 内容增量只限于文本、思考、签名和工具输入 JSON，因此没有引用数据可以以 `citations_delta` 呈现。后端网页搜索的来源 URL 改为在完成的 `web_search_tool_result` 块上内联上报（见上），而不是作为逐片段的文本引用。

少数几个字段有保真度上的注意事项。

`duration_ms` 是提示执行的墙上时钟时间。`duration_api_ms` 是各次模型调用**上报**耗时的总和。不上报自身耗时的模型调用贡献 `0`，所以 `duration_api_ms` 可能少算真实的 API 时间。

`num_turns` 和 `total_cost_usd` 在已知时是权威值。未知时，`num_turns` 退回本回合已完成的模型响应数，`total_cost_usd` 退回 `0`。已完成但不产生内容的响应不发 `assistant` 行，但仍算一个回合。花费从不多报。

`modelUsage` 携带 Chaos 所跟踪的按模型 token 与花费字段，以及归到当前活跃模型上的 `webSearchRequests`。归约器只跟踪一个全局网页搜索计数，而不是按模型分别计数，所以整个计数都落在当前或最后一个模型上，其他行保持 `0`。当某个模型的花费未知或被扣留时，该模型的 `modelUsage.*.costUSD` 为 `0`。这与顶层 `total_cost_usd` 一样是「失败即归零」的行为。`json` 格式在不完整时会整体省略花费浮点数，而这条流会保留该字段并置 `0`。`contextWindow` 是当前模型的真实总上下文窗口（Chaos 用于自动压缩的同一个值），只出现在当前模型那一行上。其他行省略它；窗口未知时当前行也省略。`maxOutputTokens` 没有对应的 Chaos 目录项，所以该键整体省略。没有按模型拆分数据时，`modelUsage` 为 `{}`。

与 `streaming-json` 一样，这条流是只读的。工具批准和其他双向流程走 ACP 接口（`chaos agent`）。

---

## 无头模式下的会话管理

默认情况下，每次 `chaos -p` 调用都会新建一个会话。要在多次调用间保持上下文，请使用会话标志。

### 具名会话（`-s`）

要在多次无头调用间延续上下文，请用 `-r/--resume` 或 `-c/--continue`。`-s/--session-id` 只用于以 **UUID** 创建**新**会话（不是 UUID，或在目标会话目录下已被占用时报错）。旧版隐藏的 `-s` upsert/resume 行为已经移除。要继续会话请用 `-r`/`-c`。与 `-r`/`-c` 连用时，`-s` 需要配 `--fork-session`：

```bash
# Start a headless session and capture its ID
chaos -p "Review the changes in this PR" --output-format json | jq -r '.sessionId'

# Continue in the same session
chaos -p "Now check for security issues" --resume "<id>"

# Optional: create with a client-chosen UUID (must not already exist)
chaos -p "hello" --session-id "$(uuidgen | tr '[:upper:]' '[:lower:]')" --output-format json
```

> **注意：** `-s/--session-id` 只创建新会话（UUID 需有效；已被占用时报错）。要恢复会话请用 `-r`。

### 恢复会话（`-r`）

`-r/--resume` 按 ID 恢复指定会话；当值不是 ID 时，按当前目录下的标题恢复，忽略大小写（重名时，唯一一个被手动改过名的匹配胜出，其余重名项报错并列出各自的 ID；形如 UUID 的值总走 ID 路径，因此脚本应优先用 ID）。会话不存在时报错：

```bash
# Get the session ID from a previous JSON response
chaos -p "Remember: the secret number is 42" --output-format json
# Output includes "sessionId": "abc123"

# Resume that exact session
chaos -p "What's the secret number?" --resume abc123
```

### 继续会话（`-c`）

`-c/--continue` 继续当前工作目录中最近的一个会话：

```bash
chaos -p "Continue where we left off" -c
```

### 取出会话 ID

用 `--output-format json`，然后解析 `sessionId` 字段：

```bash
chaos -p "Hello" --output-format json | jq -r '.sessionId'
```

---

## 管道输入与输出

无头模式与 Unix 管道和重定向天然契合。

### 标准输出

```bash
# Pipe output to a file
chaos -p "Generate a README" > README.md

# Parse JSON output with jq
chaos -p "List files" --output-format json | jq -r '.text'
```

### 标准输入

无头模式不会把管道进来的 stdin 读进提示。请通过命令替换或 `--prompt-file` 传入外部内容：

```bash
# Include git diff as context via command substitution
chaos -p "Write a concise commit message for these changes:

$(git diff --staged)"

# Or read the prompt from a file
chaos --prompt-file ./prompt.txt
```

---

## CI/CD 集成示例

### 自动化代码评审

```bash
chaos -p "Review changes for bugs and security issues." \
  --output-format json --yolo | jq -r '.text' > review.md
```

### 提交前钩子

```bash
chaos -p "Review staged changes for obvious bugs. Reply OK if fine, or list issues." \
  --yolo --output-format json | jq -r '.text' | grep -q "^OK" || exit 1
```

### 批量处理

```bash
for file in src/*.js; do
  chaos -p "Migrate $file from CommonJS to ES modules." --yolo
done
```

---

## 脚本范式

### Python 封装

Chaos 的无头模式可以封装成一个兼容 OpenAI 的 chat completion API：

```python
import asyncio
import json
import os

class GrokChat:
    """Simple OpenAI-compatible wrapper using headless mode."""

    def __init__(self, cwd="."):
        self.cwd = cwd
        self.env = {**os.environ}

    def _build_cmd(self, prompt, model, stream):
        return ["chaos", "-p", prompt, "-m", model, "--cwd", self.cwd,
                "--output-format", "streaming-json" if stream else "json",
                "--yolo"]

    async def create(self, messages, model="grok-4.6", stream=False):
        prompt = messages[-1]["content"] if len(messages) == 1 else "\n".join(
            f"{m['role']}: {m['content']}" for m in messages
        )
        cmd = self._build_cmd(prompt, model, stream)

        if stream:
            return self._stream(cmd)

        proc = await asyncio.create_subprocess_exec(
            *cmd, env=self.env, stdout=asyncio.subprocess.PIPE
        )
        stdout, _ = await proc.communicate()
        data = json.loads(stdout.decode()) if stdout else {"text": ""}
        return {
            "choices": [{
                "message": {"role": "assistant", "content": data.get("text", "")},
                "finish_reason": "stop"
            }]
        }

    async def _stream(self, cmd):
        proc = await asyncio.create_subprocess_exec(
            *cmd, env=self.env, stdout=asyncio.subprocess.PIPE
        )
        async for line in proc.stdout:
            if not line.strip():
                continue
            event = json.loads(line)
            if event.get("type") == "text":
                yield {"choices": [{"delta": {"content": event["data"]}}]}
            elif event.get("type") == "end":
                yield {"choices": [{"delta": {}, "finish_reason": "stop"}]}


async def main():
    client = GrokChat(cwd=".")
    response = await client.create(
        [{"role": "user", "content": "What files are here?"}]
    )
    print(response["choices"][0]["message"]["content"])

asyncio.run(main())
```

### Shell 脚本

```bash
#!/bin/bash
# Run a code review and exit with failure if issues are found

RESULT=$(chaos -p "Review this PR for bugs. Output JSON with 'issues' array." \
  --output-format json --yolo | jq -r '.text')

ISSUE_COUNT=$(echo "$RESULT" | jq '.issues | length' 2>/dev/null || echo "0")

if [ "$ISSUE_COUNT" -gt 0 ]; then
  echo "Found $ISSUE_COUNT issues"
  echo "$RESULT" | jq '.issues[]'
  exit 1
fi

echo "No issues found"
```

---

## 自动化场景下的始终批准

`--always-approve`（别名 `--yolo`，等价于 `--permission-mode bypassPermissions`）让工具调用不经交互式权限提示直接执行。拒绝规则、钩子和管理员锁定仍然生效（见[权限与安全](22-permissions-and-safety.md#权限模式)）。

```bash
chaos -p "Format all files" --always-approve
chaos -p "Run the tests and fix any failures" --cwd ~/projects/my-app --always-approve
```

智能体服务器与 SDK 见[智能体模式](15-agent-mode.md#自动化与-sdk)。
---

## 无头模式的环境变量

影响无头模式的关键环境变量：

| 变量                        | 说明                                                   |
| ------------------------------- | ------------------------------------------------------------- |
| `XAI_API_KEY`        | 用于认证的 API key（没有浏览器登录时必需）   |
| `CHAOS_HOME`                    | 覆盖配置目录（默认 `~/.chaos`）                |
| `GROK_LOG_FILE`                | 日志文件路径（按原样作为路径使用；无头模式与 TUI 均可用，遵循 `RUST_LOG`） |
| `RUST_LOG`                     | 日志级别过滤（如 `debug`）。无头模式把日志写到 stderr。     |

在没有浏览器访问的 CI 环境里，用来自 [console.x.ai](https://console.x.ai) 的 API key 设置 `XAI_API_KEY`：

```bash
export XAI_API_KEY="xai-..."
chaos -p "Run the test suite" --yolo
```

---

## 退出码

| 代码 | 含义                              |
| ---- | ------------------------------------ |
| `0`  | 成功。提示正常完成 |
| `1`  | 出错。认证失败、网络错误或运行时错误 |
| `130` | 被 SIGINT（Ctrl+C）中断                                   |
| `143` | 被 SIGTERM 终止                                            |

---

## 无头模式的认证

无头模式用以下方式之一认证：

- **`XAI_API_KEY`**：CI 里最简单的方式。见上面的[环境变量](#无头模式的环境变量)。
- 其他认证方式（自定义 Provider 密钥、OpenAI 兼容与 Anthropic 原生接口）见[认证](02-authentication.md#认证方式)。

此前登录过的话，缓存的凭据会被自动使用。

---

## 提示

- 无头模式**默认新建会话**。要用 `-r/--resume` 或 `-c/--continue` 才能在多次调用间保持上下文。
- `--output-format json` 的响应总带一个 `sessionId`，后续调用可以用它配 `--resume`。
- 把 `--yolo` 与 `--rules` 组合起来设护栏：`chaos -p "..." --yolo --rules "Never delete files"`。
- 调试时提高日志级别并捕获 stderr：`RUST_LOG=debug chaos -p "..." 2> debug.log`。

---

## 项目根目录的发现

Chaos 启动时，会从 `--cwd`（或当前目录）向上走，直到找到 `.git` 目录，
以此确定项目根。

注意：如果 `--cwd` 嵌在一个大型仓库（例如 monorepo）里，
Chaos 会把那个仓库当作项目根，并把发现范围（AGENTS.md、技能、git 历史）限定在其内，这可能拖慢
启动。把 `--cwd` 指向你要具体工作的子项目，可以让范围
保持得比较小。

---

## 文件位置

Chaos 把数据存放在 `~/.chaos`（兼容旧的 `~/.grok`；可用 `CHAOS_HOME` 覆盖；见[无头模式的环境变量](#无头模式的环境变量)）：

| 路径                     | 内容                              |
| ------------------------ | ------------------------------------- |
| `config.toml`            | 用户配置                    |
| `auth.json`              | 缓存的 OAuth2/API 凭据         |
| `version.json`           | 更新检查用的版本缓存       |
| `sessions/`              | 会话记录（SQLite）          |
| `memory/`                | 跨会话记忆存储            |
| `logs/`                  | 内部日志文件（例如 `unified.jsonl`） |
| `logs/mcp/`              | MCP 服务器日志                       |
| `skills/`                | 用户技能定义                |
| `personas/`              | 用户作用域的智能体人设            |
| `crash/`                 | 崩溃报告                         |
| `trace-exports/`         | 会话 trace 导出                 |
| `worktrees/`             | Git worktree 元数据                 |

### 只读的 `~/.chaos`

在容器或 CI 里，可以把 `~/.chaos` 以只读方式挂载：

- 预先放好 `auth.json`，或使用 `XAI_API_KEY`
- 会话持久化会静默失败（环境是临时的）
- 更新检查记一条警告后跳过

```bash
export XAI_API_KEY="xai-..."
export GROK_DISABLE_AUTOUPDATER=1
chaos -p "..." --no-auto-update
```

---

## 抑制更新检查

| 方式                          | 作用域     |
| ------------------------------- | --------- |
| `--no-auto-update`              | 会话   |
| `GROK_DISABLE_AUTOUPDATER=1`    | 进程   |
| 非 TTY 的 stderr（自动检测）  | 自动 |
| `[cli] auto_update = false`     | 持久|

`GROK_DISABLE_AUTOUPDATER` 设为假值（`0`、`false`、`off`、`no` 或空串，大小写不限）
等同于没设。智能体 SDK
为它派生的非主智能体注入 `GROK_DISABLE_AUTOUPDATER=1`（SDK 隔离环境里的假值
会让更新保持开启），而 stdio 智能体会跳过自己的后台更新检查，
除非它是从受管安装（`$CHAOS_HOME/bin/chaos`）运行的。

更新提示走 **stderr**，stdout 对 `--output-format json` 保持干净。另见[无头模式的环境变量](#无头模式的环境变量)。

---

## 其他无头模式标志

这些标志补充上面的[命令行选项](#命令行选项)表。已经在那里列出的标志（`--prompt-json`、`--prompt-file`、`--verbatim`、`--sandbox`、`--no-auto-update`）不再重复。

| 标志                          | 说明                                       |
| ----------------------------- | ------------------------------------------------- |
| `--agent <NAME>`              | 智能体名称或定义文件路径                |
| `--agents <JSON>`             | 以内联 JSON 给出的子智能体定义               |
| `--system-prompt-override`    | 覆盖该智能体的系统提示                |
| `--no-plan`                   | 禁用计划模式                                 |
| `--no-subagents`              | 禁用子智能体派生                         |
| `GROK_MEMORY=0`                | 为该进程禁用跨会话记忆      |
| `--disable-web-search`        | 禁用网页搜索与抓取工具                |
| `--no-alt-screen`             | 内联运行（不使用备用屏幕）                  |
| `--worktree [NAME]`           | 从当前检出（含未提交的改动）创建一个 git worktree 并在其中运行会话。从子目录启动会落在 worktree 的同一个子目录里。配 `-r` 时，会话会恢复进新 worktree。不可与 `--fork-session` 组合。 |
| `--ref <REF>` / `--worktree-ref <REF>` | worktree 所基于的分支/标签/提交（配合 `--worktree`）；要求检出干净，不带未提交改动 |

---

## 被中断的无头运行

遇到 SIGINT/SIGTERM 时：

- 会话状态保存到最后一个完成的工具调用为止
- 工具造成的文件改动**不会回滚**
- SIGINT 的退出码是 **130**（`128 + 2`），SIGTERM 是 **143**（`128 + 15`）；CI 流水线可以据此把它们与普通错误（退出码 `1`）区分开
- 恢复：`chaos -p "continue" --resume "<id>"` 或 `chaos -p "continue" --continue`

具名会话以及 `-s`/`-r`/`-c` 标志的细节，见[无头模式下的会话管理](#无头模式下的会话管理)。
