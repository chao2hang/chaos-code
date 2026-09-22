# 钩子

钩子让你在 Grok 会话的关键时刻运行脚本或发送 HTTP 请求。可以用它来自动化任务、强制执行安全检查、记录活动、发送通知，以及集成你自己的工具。

---

## 什么是钩子？

钩子是一条 shell 命令或一个 HTTP 端点，在特定生命周期事件发生时由 Grok 调用。钩子可以：

- **拦截操作**：`PreToolUse` 钩子可以在危险命令运行之前拒绝它。
- **让 agent 保持工作**：`Stop` 钩子可以阻止 agent 结束当前回合，直到某个条件满足（例如测试套件通过），并把原因反馈给模型。
- **对事件作出反应**：`PostToolUse` 钩子可以把每次工具执行记录到文件。
- **在调用之后纠正它**：`PostToolUse` 钩子可以告诉模型某个工具结果意味着什么，或者替换模型读到的输出——抹掉密钥、裁剪一大段日志行——同时真实结果仍保留在记录里。
- **准备上下文**：`SessionStart` 钩子可以导出环境变量或运行初始化脚本。

---

## 常见用例

- **安全防护**：在 `rm -rf /` 之类的命令运行之前拦截它。
- **审计日志**：把工具使用与会话记录到文件或外部服务。
- **通知**：任务完成时发送消息。
- **自动格式化**：编辑之后运行 `cargo fmt` 或 `prettier`。
- **环境准备**：会话开始时导出变量。
- **自定义工作流**：在特定事件上触发构建、测试或部署。

---

## 快速上手

1. 创建钩子目录：

   ```sh
   mkdir -p ~/.chaos/hooks
   ```

2. 创建一个钩子文件，例如 `~/.chaos/hooks/session-start.json`：

   ```json
   {
     "hooks": {
       "SessionStart": [
         {
           "hooks": [
             { "type": "command", "command": "echo 'Grok session started in '$(pwd)" }
           ]
         }
       ]
     }
   }
   ```

3. 启动（或重启）Grok 会话。钩子会在 `SessionStart` 时自动运行。

4. 在非 VS Code 家族终端上按 `Ctrl+L`（或在任意位置运行 `/hooks`——VS Code 家族上推荐后者），检查 Hooks 标签页确认它已加载。

---

## 钩子位置

钩子从多个位置被发现（全部会合并）：

| 作用域 | 路径 | 是否信任？ | 说明 |
|-------|------|----------|-------|
| 全局 | `~/.grok/hooks/*.json` | 始终 | 个人钩子 |
| 全局 | `~/.claude/settings.json`（及 `settings.local.json`） | 始终 | Claude Code 兼容（可配置） |
| 全局 | `~/.cursor/hooks.json` | 始终 | Cursor 兼容（可配置） |
| 项目 | `<project>/.grok/hooks/*.json` | 需要信任 | 按仓库自动化 |
| 项目 | `<project>/.claude/settings.json`（及 `settings.local.json`） | 需要信任 | Claude 兼容（可配置） |
| 项目 | `<project>/.cursor/hooks.json` | 需要信任 | Cursor 兼容（可配置） |
| 配置 | `~/.grok/config.toml` | 始终 | 你的钩子与配置的其余部分放在一起 |
| 配置 | `managed_config.toml`（`$GROK_HOME` 与 `/etc/grok`） | 始终 | 组织分发的钩子（服务器同步与本地设备） |
| 配置 | `requirements.toml`（用户与系统） | 始终 | requirements 层中组织分发的钩子 |
| 插件 | 内置于已安装的插件中 | 按插件 | 团队共享钩子 |

配置文件中的钩子位于你的组织已经掌控的那个 TOML 里；格式见[配置文件中的钩子](#配置文件中的钩子)。兼容厂商的钩子源默认会被扫描。要禁用对特定厂商的扫描，在 `~/.grok/config.toml` 里设 `[compat.<vendor>] hooks = false`，或设置对应的环境变量。详见[配置](05-configuration.md#厂商兼容性开关)。

**信任一个项目**：第一次打开一个带钩子的项目时，必须先信任它，它的项目钩子才会运行；在那之前它们会被静默跳过。运行 `/hooks-trust`（或以 `--trust` 启动）来授信；该决定记录在统一的文件夹信任存储（`~/.chaos/trusted_folders.toml`）里，与管理仓库本地 MCP/LSP 服务器的是同一道闸门。`~/.chaos/hooks/` 里的全局钩子始终被信任，无需条目。这可以防止不受信任的仓库运行任意代码。

由于钩子统一归入文件夹信任，一次 `--trust` / `/hooks-trust` 授权会为 **MCP、LSP、钩子、项目说明与项目技能** 一起信任整个文件夹，并覆盖同一仓库的子目录。该文件夹下嵌套的 git checkout 是一个独立的工作区，不被覆盖。反过来，禁用文件夹信任（`GROK_FOLDER_TRUST=0` 或 `[folder_trust] enabled = false`）会同时解除这些表面的门禁。

---

## 钩子事件

事件按三种节奏触发：每会话一次（`SessionStart`、`SessionEnd`）、每回合一次（`UserPromptSubmit`、`Stop`、`StopFailure`），以及回合内每次工具调用（`PreToolUse`、`PostToolUse`、`PostToolUseFailure`）。

| 事件 | 触发时机 | 是否阻塞？ |
|-------|---------------|-----------|
| `SessionStart` | 会话启动。子代理自身的会话不触发。 | 否 |
| `UserPromptSubmit` | 你提交一条提示。 | 是：可拦截提示 |
| `PreToolUse` | 某个工具即将运行。 | 是：可拒绝 |
| `PostToolUse` | 某个工具运行结束（包括非零 `run_terminal_command` 退出码这类内置逻辑错误；分发失败或 MCP 错误结果改为触发 `PostToolUseFailure`）。 | 否，但可以向模型反馈并替换模型看到的输出 |
| `PostToolUseFailure` | 工具分发失败，或 MCP 工具返回错误结果。 | 否，但可以向模型喂 `additionalContext` |
| `PermissionDenied` | 权限系统拒绝了一次工具调用。 | 否 |
| `Stop` | agent 回合以真正的完成收尾（中断则改触发 `StopCancelled`）。 | 是：可拦截停止 |
| `StopFailure` | 回合因 API 错误而结束。 | 否 |
| `StopCancelled` | 回合未完成即结束时运行，替代 `Stop`：用户中断（Ctrl+C / 客户端停止）、权限提示被拒绝、`--max-turns` 上限，或无进展退出。 | 否 |
| `Notification` | 需要用户注意的事件（`idle_prompt`、`permission_prompt`、`task_complete`、…）。 | 否 |
| `SubagentStart` | 子代理启动。 | 否 |
| `SubagentStop` | 子代理的回合结束（在子代理内触发一次，带停止决定控制）。 | 是：可拦截停止 |
| `PreCompact` | 对话压缩即将运行。 | 否 |
| `PostCompact` | 对话压缩完成。 | 否 |
| `SessionEnd` | 会话结束。子会话会携带 `subagentType`，宿主可以借此区分子会话的收尾与自身的收尾。 | 否 |

`SubagentEnd` 被接受为 `SubagentStop` 的别名。`PreToolUse` 可以拦截一次工具调用，`UserPromptSubmit` 可以拦截一条提示（见下文），`Stop`/`SubagentStop` 可以阻止 agent 停止（见[停止决定控制](#停止决定控制)）。`PostToolUse` 运行得太晚，无法拦截任何东西，但它的 stdout 会被读取：它可以向模型反馈并替换模型看到的工具输出（见 [PostToolUse 输出](#posttooluse-输出)）。其余事件都是被动的。

### UserPromptSubmit 决定控制

`UserPromptSubmit` 钩子可以拒绝一条提示：退出码 2 拦截（stderr 成为消息），stdout 上的 JSON `{"decision": "block", "reason": "..."}` 在任何退出码下同样拦截。原因会展示给你，绝不会加入模型的上下文。只有你亲手输入的提示才能被拦截：自动唤醒回合（任务与子代理完成、调度器触发）以及子代理会话只以观察模式运行该钩子。此事件的默认超时为 30 秒；超时或崩溃的钩子按失败放行处理，提示继续执行。

被拦截后，已排在被拦截提示后面的提示不会自动运行：队列保持等待，直到你采取行动（发送提示，或编辑 / 移除 / 重排 / 强制运行队列中的某一行）。被拦截的提示不会被记录：它既不进入模型在后续回合看到的对话历史，也不进入磁盘上的会话记录或会话摘要。它仍留在实时回滚区可见，并被保持在队列最前，供你编辑、重发或丢弃——但会话重启后，被拦截的气泡会从回滚区消失，正是因为什么都没有存储。一个刻意的例外：你的客户端本地提示历史（上箭头调出的那条记录）保留文本，在钩子运行之前的提交时刻就已记录，所以被丢弃的提示仍可找回。一个当前限制：放行钩子的 stdout 会被丢弃（没有 `additionalContext`）。

### Cursor 钩子兼容

Grok 接受 Cursor 的驼峰式钩子事件名，因此 `~/.cursor/hooks.json` 无需改动即可加载：

| Cursor 事件 | 对应 |
|---|---|
| `sessionStart`, `sessionEnd` | `SessionStart`, `SessionEnd` |
| `preToolUse`, `postToolUse`, `postToolUseFailure` | `PreToolUse`, `PostToolUse`, `PostToolUseFailure` |
| `beforeShellExecution`, `beforeMCPExecution`, `beforeReadFile` | `PreToolUse` |
| `afterShellExecution`, `afterMCPExecution`, `afterFileEdit` | `PostToolUse` |
| `afterAgentResponse`, `afterAgentThought` | `PostToolUse` |
| `beforeSubmitPrompt` | `UserPromptSubmit` |
| `subagentStart`, `subagentStop` | `SubagentStart`, `SubagentStop` |
| `preCompact`, `stop` | `PreCompact`, `Stop` |

Cursor 的按操作钩子（`beforeShellExecution`、`afterFileEdit` 等）映射到通用的 `PreToolUse`/`PostToolUse` 事件。钩子脚本在 JSON 输入中接收工具名，可据此过滤，或使用 `matcher` 字段。

---

## 钩子 JSON 格式

每个 `.json` 文件可以为多个事件定义钩子：

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          { "type": "command", "command": "bin/safety-check.sh", "timeout": 10 }
        ]
      }
    ],
    "PostToolUse": [
      {
        "hooks": [
          { "type": "command", "command": "bin/log-activity.sh" }
        ]
      }
    ]
  }
}
```

### 关键字段

- **事件名**（顶层键）：[钩子事件](#钩子事件)中列出的任意事件。Grok 会跳过无法识别的事件名，因此共享的 Claude 或 Cursor 设置文件仍能加载。
- **matcher**（可选）：一个正则表达式，选择哪些调用会触发钩子。它测试什么取决于事件：工具事件（`PreToolUse`、`PostToolUse`、`PostToolUseFailure`、`PermissionDenied`）上是工具名，`Notification` 上是通知类型，`SubagentStart`/`SubagentStop` 上是子代理类型（例如 `explore`），`SessionStart` 上是启动来源（`startup`、`resume`、…），`SessionEnd` 上是结束原因，`PreCompact`/`PostCompact` 上是压缩触发方式（`manual` 或 `auto`），`StopFailure` 上是错误类型（`rate_limit`、`authentication_failed`、`invalid_request`、`server_error`、`max_output_tokens` 或 `unknown`），`StopCancelled` 上是原因（`user_interrupt`、`permission_rejected`、`permission_cancelled`、`max_turns`、`no_progress` 或 `unknown`）。`Stop` 或 `UserPromptSubmit` 上的 matcher 会被忽略并给出警告（这两个事件总会触发）。空 matcher 或省略 matcher 匹配一切。思考结束的提示音应在 `Notification` 上把 `matcher` 设为 `idle_prompt`（任意回合结束，随后持续空闲）；`permission_prompt` 只在权限 UI 确实在等待时触发。matcher 测试的是真实工具名；经内部 `use_tool` 分发器路由的 MCP 调用会以带限定符的 `server__tool` 名称出现（例如 `linear__save_issue`），因此要匹配它而非分发器名。
- **type**：`"command"`（运行脚本或 shell 单行命令）或 `"http"`（把事件 POST 到某个 URL）。
- **command**：可执行文件的路径（相对于 JSON 文件）或内联 shell 命令。
- **timeout**：杀死钩子前的秒数（默认：5，`Stop`/`SubagentStop`/`PostToolUse` 闸门为 600）。所有钩子失败（超时、崩溃、输出格式错误、缺少必需的环境变量）都按失败放行处理：失败会记录到 UI 回滚区，但工具调用不会被拦截。只有钩子返回显式 `deny` 决定才会拦截一次工具调用。

### 工具名别名

在 `matcher` 中，Grok 会把 Claude 风格的工具名映射为自己的工具名，让从 Claude 迁移来的钩子正确触发。常见别名包括：

- `Bash` → `run_terminal_command`
- `Read` → `read_file`
- `Edit`, `Write`, and `MultiEdit` → `search_replace`
- `Grep` → `grep`
- `Glob` and `ListDir` → `list_dir`
- `WebSearch` → `web_search`
- `Task` → `spawn_subagent`

matcher 也保留原始名称，因此 `Bash` 同时匹配 `Bash` 和 `run_terminal_command`。

---

## 钩子如何解析

事件触发时，Grok 分四步解析它：

1. **选择匹配的组。** 对该事件，每个 `matcher` 与事件字段匹配的 matcher 组都会运行。matcher 在工具事件上测试工具名，在 `Notification` 上测试通知类型，等等（见[关键字段](#关键字段)）。空 matcher 或省略 matcher 匹配一切。
2. **按顺序运行处理器。** 被选中组里的处理器按配置顺序运行，各自通过 stdin 以 JSON 形式接收事件，直到某个处理器返回 `deny`（它会终止链条）。来自不同来源（全局、项目、插件、配置）的处理器会合并，相同的处理器会被去重。每个处理器看到的都是模型的原始工具输入；`PreToolUse` 的 `updatedInput` 只在所有处理器结束后应用，因此一个处理器看不到另一个处理器的改写（最后一次改写胜出）。
3. **应用决定。** 对 `PreToolUse` 闸门，第一个 `deny` 拦截调用并把原因展示给模型，`updatedInput` 改写工具输入，否则调用照常进行。对 `Stop` 与 `SubagentStop`，`block` 让 agent 继续工作。对 `PostToolUse`，工具已经运行，因此什么都不会被拦截，每个钩子都会运行：`block` 原因与任何 `additionalContext` 会随工具结果一起交付给模型，输出替换则改写模型那份结果。其余事件都是被动的：其输出会被记录，但不改变控制流。
4. **失败放行。** 超时、崩溃或输出格式错误的处理器会记录到回滚区，但绝不拦截操作。唯一的例外是 `PreToolUse` 的 `updatedInput` 未通过工具的 schema 校验：改写无法安全运行，因此调用被拦截并报告为无效输入错误。除此之外，只有显式 `deny` 才会拦截一次工具调用。

---

## 配置文件中的钩子

钩子也可以直接放在你的 Grok 配置里，团队就能随其余配置一起分发它们，而不必单独交付 JSON 文件。同一个 `hooks` 对象会从三个 TOML 文件读取：

| 文件 | 层级 | 谁设置它 |
|------|------|-------------|
| `~/.chaos/config.toml` | 用户 | 你 |
| `managed_config.toml` (`$GROK_HOME`, `/etc/grok`) | managed / 系统 | 你的组织 |
| `requirements.toml`（用户与系统） | requirements.toml 可否设置 | 你的组织 |

这个 TOML 在结构上与 JSON 钩子对象完全一致，因此既有钩子可以直接转写：

```toml
[[hooks.PreToolUse]]
matcher = "Bash|Write|Edit"
hooks = [
  { type = "command", command = "/opt/guard/pretooluse.sh", timeout = 10 },
]
```

每个 matcher 组是一个 `[[hooks.<Event>]]` 条目，带可选的 `matcher` 与内层 `hooks` 处理器数组。处理器字段（`type`、`command`、`url`、`timeout`、`env`）与事件名和 [JSON 格式](#钩子-json-格式)完全相同。

TOML 为内层处理器提供两种等价写法，二者解析出相同的结构。推荐上面展示的内联表数组形式：在常见的单处理器场景下最易读。嵌套的表数组形式同样被接受：

```toml
[[hooks.PreToolUse]]
matcher = "Bash|Write|Edit"
[[hooks.PreToolUse.hooks]]
type = "command"
command = "/opt/guard/pretooluse.sh"
timeout = 10
```

优先使用内联形式，避免为每个处理器重复 `[[hooks.<Event>.hooks]]` 表头。

- **跨层叠加。** 每一层的钩子都会运行；低优先级层会添加钩子，但从不替换另一层的块。在多个层中以相同方式定义的钩子会被去重，保留权威最高的那份。
- **来源标签。** 配置钩子出现在 `/hooks` 中并按来源打标签（`managed:`、`requirements/user:`、`user:` 等），你可以看到每个钩子来自哪一层。
- **读取时不展开。** `command` 或 `url` 中字面的 `${VAR}` 会原样到达钩子运行器，与 JSON 钩子文件的语义一致；由运行器执行这唯一一次展开。

---

## 编写钩子脚本

### 输入

事件以 JSON 形式通过 **stdin** 发送（例如一个 `PreToolUse` 事件；载荷还始终包含 `toolUseId` 与 `toolInputTruncated`）：

```json
{
  "hookEventName": "pre_tool_use",
  "hook_event_name": "PreToolUse",
  "sessionId": "abc-123",
  "cwd": "/Users/you/project",
  "workspaceRoot": "/Users/you/project",
  "permissionMode": "default",
  "toolName": "run_terminal_command",
  "toolInput": { "command": "npm test" },
  "timestamp": "2026-04-14T12:00:00Z"
}
```

每个事件都携带相同的公共字段：`hookEventName`、`sessionId`、`cwd`、`workspaceRoot`、`timestamp`、`permissionMode`（`default`、`auto`、`plan` 或 `bypassPermissions`）与 `promptId`（事件所属的回合；会话级事件没有该字段），再加上像上面 `toolName` 这样的事件特有字段。`hook_event_name`（snake_case 键）携带 Claude 的 PascalCase 值；`hookEventName`（camelCase 键）携带 Chaos 的 snake_case 值。

### 输出（拦截型钩子）

对 `PreToolUse` 钩子，向 **stdout** 写入 JSON：

- **允许**：`{"decision": "allow"}`
- **拒绝**：`{"decision": "deny", "reason": "Unsafe command detected"}`
- **询问用户**：`{"decision": "ask", "reason": "Confirm this deploy"}`
- **不表态**：`{"decision": "defer"}`
- **改写工具输入**：`{"hookSpecificOutput": {"hookEventName": "PreToolUse", "updatedInput": {"command": "npm test"}}}`
- **告诉模型一些事**：`{"hookSpecificOutput": {"hookEventName": "PreToolUse", "additionalContext": "This repo builds with xb, not cargo"}}`

决定可以写成顶层 `decision`，也可以写成 `hookSpecificOutput.permissionDecision`。两者都接受 `allow`、`deny`、`ask` 或 `defer`（旧式的 `approve` 与 `block` 拼法也可用），各有自己的原因字段——`reason` 与 `permissionDecisionReason`。规范的 `permissionDecision` 在存在时优先；顶层 `decision` 只在它缺席时生效。deny 或 ask 的消息在 `permissionDecisionReason` 存在时取它，否则取 `reason`。`allow` 只意味着"未被拦截"——它不会自动批准一个原本会询问用户的调用。集合之外的决定值视为钩子失败，按失败放行处理，除非钩子同时以退出码 2 退出，此时拒绝成立并把错误带在原因里。

`ask` 让调用到达权限提示：原本可以不经询问就放行它的东西——always-approve 模式、auto 模式、已保存的"始终允许"授权、安全命令——都不再适用，提示会点名你的钩子并展示你的原因。绝不会有第二次提示：在你本来就会被询问的地方，ask 只是把那一次重新标注。批准则运行；拒绝则像普通权限拒绝一样拦截。以完整 always-approve/YOLO 模式（自动应答每个提示）运行的客户端仍会自动批准该调用，与 Claude Code 的 `bypassPermissions` 一致：ask 覆盖的是管理器的 always-approve、auto、已保存授权与安全命令路径，而不是一个对所有提示一概批准的客户端。

`ask` 不能放宽任何东西，因此权限策略的拒绝、auto 模式的拦截或 plan 模式仍然决定该调用。在 auto 模式下，钩子的 `ask` 仍会在提示出现前运行分类器：分类器可以拒绝调用，但绝不能悄悄批准一个钩子要求询问的调用。`dontAsk` 模式会拒绝一切它需要提示的东西，因此在那里 ask 会把一个原本批准的调用变成拒绝。

只有配置在设置文件里的钩子（命令与 HTTP 钩子）才能 ask、defer 或发送 `additionalContext`：通过 grok-agent-sdk 注册的 `PreToolUse` 钩子可以 allow 或 deny，其余一律丢弃——那里的 `ask` 或 `defer` 会让调用走正常权限流程，并被记录为无法识别的决定，`additionalContext` 也不会到达模型。

`updatedInput` 在工具运行之前静默替换其输入：模型不会被告知，回滚区也不会写入任何内容，因此改写唯一的痕迹就是被改写的参数本身，用户只有在调用到达权限提示时才会看到。plan 模式闸门、权限提示、工具本身以及稍后的 `PostToolUse` 载荷看到的都是改写后的输入，因此钩子可以规范化或加固一次调用，而不只是允许或拒绝它。钩子在 plan 模式闸门之前运行，因此带副作用的钩子即使随后被 plan 模式拒绝也会触发。

该值必须是 JSON 对象；非对象会让钩子失败。如果改写后的输入未通过工具的 schema 校验，调用会作为钩子拒绝被拦截——回滚区注记会点名钩子——而不是回退到原始输入。改写可以改变调用的参数，但不能改变运行的是哪个工具，因此重定向 `use_tool` 调用的改写同样会被拦截。以非零退出的钩子保留其 `deny`，但丢失其 `updatedInput` 与 `additionalContext`。

`deny` 会丢弃任何 `updatedInput`；多个钩子都返回时，最后一个胜出。返回 `updatedInput` 而省略 `decision` 会允许调用并应用改写。

`defer` 既不拦截调用也不批准它：调用走正常权限流程，就像你的钩子没有应答一样，日志里会记录一条点名该钩子的警告。它对其余发送的内容也不生效——`defer` 旁边的 `updatedInput` 或 `additionalContext` 会被忽略并在日志中点名。在多个钩子之间 `defer` 排在 `ask` 之后，因此当你的一个钩子 defer、另一个 ask 时，Chaos 会发出提示。

`additionalContext` 是给模型的提示。它在调用运行之后到达——绝不在之前——随该调用所属批次的结果一起，包在你所用 harness 的提醒标签里（默认为 `<system-reminder>`）并点名写入它的钩子，模型因此能区分你的文本与用户的文本。每个发送它的钩子都会被送达，顺序与钩子运行顺序一致（与 `updatedInput` 不同，那里是最后写入者胜出）。`deny` 会丢弃全部内容，因为调用从未运行，并在日志中点名这次丢弃。超过 10,000 字符的文本会被截断，与 `Stop` 反馈的上限相同。

### PostToolUse 输出

`PostToolUse` 在工具结束后运行，因此什么也不拦截。它的 stdout 仍会被读取，因为它决定模型接下来看到什么。向 **stdout** 写入 JSON：

```json
{
  "decision": "block",
  "reason": "The diff still contains a debug print",
  "hookSpecificOutput": {
    "hookEventName": "PostToolUse",
    "additionalContext": "This file is generated; edit the template instead",
    "updatedToolOutput": { "type": "Bash", "command": "…", "exit_code": 0, "output_for_prompt": "[redacted]" }
  }
}
```

| 字段 | 效果 |
|-------|--------|
| `decision: "block"` + `reason` | 把 `reason` 送达模型，紧挨工具结果。工具自身的输出仍会到达；"block" 意为"告诉模型出了问题"，而非"停止调用"。 |
| `additionalContext` | 在工具结果旁为模型追加一条提示。 |
| `updatedToolOutput` | 替换模型那份结果。通用键；对每个工具都有效。 |
| `updatedMCPToolOutput` | `updatedToolOutput` 的 MCP 专用别名。在内置工具上被忽略。 |

- **送达。** block 原因与 `additionalContext` 在工具结果之后到达，包在你所用 harness 的提醒标签里并点名写入它们的钩子，模型因此能在同一回合内行动。每个钩子的 block 原因与 `additionalContext` 按钩子运行顺序送达，因此一个钩子的发现不会挤掉另一个的。只有替换是最后写入者胜出：两个钩子都返回时，最后一个存活，并在日志中点名被丢弃的那个。
- **构建 `updatedToolOutput`。** 对内置工具，它必须携带 Chaos 自身为刚运行的工具定义的输出形状，即一个带标签的对象，例如 `{"type": "Bash", …}`。拿事件交给你的 `toolResult`，编辑后再发回去——那正是它被校验时依据的形状。解析失败或解析成另一个工具输出的替换会被忽略，原件保留，但钩子的运行会被记录为 `Failed` 并注明原因，因此一个显示"failed"的退出码 0 钩子报告的是一次被丢弃的替换，而不是它从未运行。`decision` 拼写错误（只认 `"block"`）也以同样方式报告。先检查 `toolResultTruncated`：超大的载荷会以纯字符串到达钩子，无法原样回传。
- **MCP 工具。** 没有形状可强制，因此 `updatedToolOutput` 与 `updatedMCPToolOutput` 都不经校验直接通过——JSON 字符串逐字成为面向模型的文本，其他值则被序列化——两个键上最后写入的钩子胜出。
- **上限。** block 原因与 `additionalContext` 截断到 10,000 字符，与 `Stop` 反馈及 `PreToolUse` 上下文共享同一上限。替换获得 64 K 字符。上限按渲染后面向模型的文本度量，并在替换渲染完成后应用，因此一段很长的 `updatedToolOutput` 会像字符串一样被截断，而不是因超长被丢弃。结构化替换只有在与工具自身的输出形状不匹配时才会被丢弃。
- **坏掉的钩子。** 非零退出——包括退出码 2——保留 block 原因并丢弃其余一切：`additionalContext` 与替换被丢弃并在日志中点名，这正是 `PreToolUse` 应用于 `updatedInput` 的同一条规则。block 是失败安全的方向。
- **记录与模型。** 替换只改写模型那份。回滚区、转录与遥测保留原件，因此抹掉密钥只是对模型隐藏，而不是对你，被改写成成功的失败在记录上仍是失败。图片不会在替换下送达，因此被替换的截图或 PDF 读取到达模型的只有你的文本。钩子发送的一切（提示、block 原因、替换）都会被转义，无法闭合提醒标签来冒充 harness 或用户撰写的指令。
- **输出替换仅限设置文件。** 命令与 HTTP 钩子可以做所有这些。通过 grok-agent-sdk 注册的 `PostToolUse` 钩子可以贡献 `block` 原因与 `additionalContext`，但不能替换工具输出。
- **触发时机。** `PostToolUse` 对每个真正运行了的工具触发，包括结果是非零 `run_terminal_command` 退出码这类内置逻辑错误的工具。分发失败的工具，或返回错误结果的 MCP 工具，改为触发 `PostToolUseFailure`——只有上下文：它可以向模型喂 `additionalContext`，但不能拦截或替换输出。该钩子继承 600 秒的闸门默认值（它常用于运行 linter 或测试）；只有当检查需要更长或更短时间时才显式设置 `timeout`。超时的钩子被记录为失败，什么都不贡献。

### 退出码

| 退出码 | 含义 |
|-----------|---------|
| `0` | 成功 / 允许（对拦截型钩子而言） |
| `2` | 显式拒绝（`PreToolUse`）、以 stderr 作为反馈的拦截停止（`Stop`/`SubagentStop`），或给模型的反馈（`PostToolUse`）。对 `PreToolUse`，当 JSON 未携带原因时，第一行 stderr（有上限）成为拒绝原因；`Stop`/`SubagentStop` 与 `PostToolUse` 把完整 stderr 喂给模型，且 JSON `reason` 优先于它。 |
| 其他 | 失败放行——失败会被记录（形如 `exit code N: <first stderr line>`），但什么都不拦截。对 `PreToolUse`，stdout JSON 中的 `deny` 决定无论退出码如何都会被采纳。对 `Stop`/`SubagentStop`，stdout 上的有效决定 JSON 优先于退出码；只有 stdout 没有可用 JSON 时才由退出码决定，此时退出码 2 以 stderr 作为反馈拦截。对 `PostToolUse`，工具已经运行，因此无论哪种情况都不拦截；失败仍会记录，钩子保留其 block 原因，但丢失其 `additionalContext` 与输出替换。 |

**`PostToolUse` 退出码 2 是一处行为变更。** 它过去只是被记录的普通失败，什么也不改变；现在它会把钩子的 stderr 喂给模型。因此写成 `run_checker; exit $?` 的日志型钩子，会在检查器以退出码 2 退出时把检查器打印的一切交给模型——`mypy`、`grep`、`pytest` 与 `argparse` 都用退出码 2 表示"无匹配"或"用法错误"。让这样的钩子以显式 `exit 0` 结尾以保持沉默。

把人类可读的诊断写到 **stderr**：它是钩子的反馈通道。失败时，第一行 stderr 会出现在回滚区条目与日志中，取代光秃秃的退出码。

### 停止决定控制

`Stop` 与 `SubagentStop` 钩子在 agent 即将结束回合时运行，可以让它继续工作（与 Claude Code 兼容）。向 **stdout** 写入 JSON：

- **拦截停止**：`{"decision": "block", "reason": "The test suite hasn't been run yet"}`。原因作为用户消息反馈给模型，agent 在同一回合再跑一轮。
- **非错误反馈**：`{"hookSpecificOutput": {"hookEventName": "Stop", "additionalContext": "Run the linter before finishing"}}`。同样让 agent 继续工作，但以钩子反馈而非钩子错误的形式呈现。
- **强制停止**：`{"continue": false, "stopReason": "Budget exhausted"}`。结束回合，压过任何拦截。
- **允许停止**：以退出码 0 退出且无输出（或任何非 JSON 输出）。

以退出码 `2` 退出同样会拦截停止，并以 **stderr** 作为反馈。

钩子输入包含 `stopHookActive` 与 `lastAssistantMessage`。当 agent 已因本回合此前一次停止钩子拦截而继续时，`stopHookActive` 为 true；检查它或转录，避免对一个永远不会满足的条件反复拦截。`lastAssistantMessage` 携带 agent 本回合最终回复的文本，钩子无需解析转录即可据此行动。每个携带该字段的事件都把它截断到 32,768 字符，并使用与其他自由文本字段相同的 `… [+N chars]` 标记。它远比 `errorDetails` 等字段的 1,000 上限宽松，因为它承载的是完整回答而非标签，量级与工具载荷上限一致。同一回合内 **8 次延续**（拦截或非错误反馈）之后，闸门被压过、回合结束；那次最终的强制停止不会再询问钩子。计数按回合计：下一条用户提示重新开始，因此长期目标可以跨回合。钩子失败按失败放行：agent 正常停止。

`Stop`、`SubagentStop` 与 `PostToolUse` 钩子默认 600 秒超时，因为这些闸门常运行构建或测试套件，而超时的钩子按失败放行，检查反正不会拦截。其余事件保持 5 秒默认值。闸门需要更长时间时显式设置 `timeout`：`{ "type": "command", "command": "bin/verify.sh", "timeout": 1200 }`。

闸门只在真正的完成时运行。被中断（Ctrl+C）、被拒绝或在回合上限被截断的回合会跳过 Stop 闸门，不过落在已运行的 Stop 钩子上的一次 Ctrl+C 会将其中途杀掉（见下文）；API 错误的回合触发 `StopFailure`，被取消的回合触发 `StopCancelled`。`Esc` 绝不会取消一个运行中的回合。会话结束时还会单独触发一次 Stop（`reason: "channel_closed"` 或 `"shutdown"`）；其决定输出会被解析但被忽略，因为没有可继续的回合了。按 Stop 触发计数或设闸的脚本应检查 `reason == "end_turn"`，以免会话结束时那次触发扭曲统计。

`StopFailure` 只用于观察（用它记录失败或发送警报；输出与退出码都会被忽略）。其输入携带 `error`（matcher 测试的分类类型：`rate_limit`、`authentication_failed`、`invalid_request`、`server_error`、`max_output_tokens`，运行时无法区分的一切为 `unknown`；容量错误归类为 `rate_limit`）、`errorDetails`（原始错误细节，可用时截断到 1000 字符；拒绝时没有，因为其解释只经由 `lastAssistantMessage`）、`lastAssistantMessage`（会话中展示的渲染错误文本；对该事件而言它是错误字符串，而非助手输出）以及 `subagentType`（回合在子代理内运行时该子代理的类型）。

`StopCancelled` 同样只用于观察。**回合未完成即结束时，它替代 `Stop` 运行**，与 API 错误时 `StopFailure` 替代 `Stop` 的方式相同。

一个回合最多报告三者之一，但有一个下文提到的例外：运行到完成的 `Stop` 钩子之后仍可能跟一次 `StopCancelled`，如果用户在闸门期间中断，因为那时钩子已被告知回合结束。每个先运行模型然后结束、出错或被取消的回合都会报告一次，下列情况除外。

如果你的宿主绝不能错过一次空闲转换，还要监听 `idle_prompt` `Notification`。它覆盖会话仍存活的所有例外情况，只有一个缺口：一个只执行了一条运行完毕的 bash 模式命令的会话，既得不到报告也得不到 ping，但中断一条却能两者兼得。`SessionEnd` 覆盖收尾。`idle_prompt` ping 在会话安定后约一分钟触发，需要至少一个回合已结束，且若你先发送了其他消息则被取消。

被取消回合的报告从会话命令循环之外分发，因此中断绝不会因你的钩子而延迟。报告因而可能到达于下一回合 `UserPromptSubmit` 的**之后**，各路径之间的回合结束报告彼此不保序。

跟踪忙碌与空闲的脚本应以下述规则围绕 `promptId` 组织。Chaos 为每回合铸造一个，但自行在 `_meta` 中提供 id 的客户端拥有其唯一性，因此把 id 当作不透明值，并将会话作为其作用域。

每个回合结束报告都经过同一个 worker，因此慢钩子会推迟下一个报告，但绝不会推迟它所属的回合。观察型钩子的超时保持短小。

`Stop` 钩子运行期间的中断会将其中途杀掉，回合随后报告 `StopCancelled`：`Stop` 钩子已启动并不意味着回合已完成。`StopFailure` 钩子在回合之外运行，中断杀不掉它，而该回合已经报告过，因此不会再跟 `StopCancelled`。

`Stop` 是闸门，因此停止钩子拦截后每轮延续都会再次触发；只有放行回合结束的那次触发才是报告，被拦截的 `Stop` 之后以取消或失败收场的回合改为报告后者。被动观察者无法区分延续触发与最终触发（两者 `stopHookActive` 均为 true），因此仅依赖 `Stop` 设闸的 UI 从第一次延续触发起到用户下一次提示之间会显示错误的空闲，因为没有 `UserPromptSubmit` 标记延续轮。在同时运行拦截闸门时，把 `Stop` 从状态脚本中去掉，改用 `idle_prompt` `Notification`。

有些回合三者都不报告：

- 运行完毕的 bash 模式（`!`）与内置斜杠命令。中断其中之一仍会报告 `user_interrupt`，且前面没有 `UserPromptSubmit`。
- 取消并发送、回退，或运行前被移除的排队提示。
- 会话收尾，由会话结束的 `Stop` 与 `SessionEnd` 报告。
- 停止钩子让 agent 一直工作、直到每回合延续上限强制停止的回合。
- 没有任何停止钩子运行到完成的回合——它们全被禁用、不受信任或失败，或在子代理中因其 matcher 全都未命中。让该回合不被报告是刻意的：之后的取消或失败仍能报告它。
- 报告还在构建时就被下一回合取代的回合。
- 会话退出时仍在排队中的报告：收尾会等待排队中的回合结束钩子半秒，然后丢弃剩余并中止仍在运行的钩子。
- 已完成、已运行其 `Stop`、之后才写入磁盘失败的回合：它已报告 `Stop`，因此不再跟 `StopFailure`。失败仍会显示在会话中。

`StopCancelled`'s input carries:

- `reason`: the classified cause, and the value the matcher tests. `user_interrupt` (Ctrl+C, a client stop button, or a client `session/cancel`), `permission_rejected` (you declined a tool call), `permission_cancelled` (you dismissed the prompt), `max_turns`, `no_progress` (the agent bailed out after repeated no-op rounds), or `unknown` (a cancel the runtime could not classify, and the forward-compatible fallback). The matcher tests this field only, so a hook that wants every user-initiated stop matches the reasons it cares about and reads `cancelledBy` from the payload. New reasons may be added over time, so treat an unrecognized value the way you treat `unknown`.
- `cancelledBy`: `user` for an interrupt, a declined tool call, or a dismissed prompt; `runtime` for everything the agent decided itself, such as `max_turns` and `no_progress`; `unknown` when `reason` is `unknown`, because a cancel the runtime could not classify cannot claim the user was uninvolved. Derived from `reason`, so a new reason classifies automatically. Values may be added here too: treat one you do not recognize the way you treat `unknown`, rather than assuming anything that is not `user` was the runtime.
- `cancelTrigger`: the gesture, when the client named one, clipped at 64 characters, since a gesture name is a token. The bundled pager sends one of three: `ctrl_c`, `mouse` (the on-screen stop button), or `dashboard_stop`. It never sends `esc` (Esc does not cancel a turn). Another client may send any string, including `esc`, and it is passed through verbatim. Every value here classifies as `user_interrupt`, including one that happens to spell an internal name such as `shutdown`, because a client asking to cancel is the user asking; read `cancelledBy` from the payload rather than parsing this string. Omitted for a bare `session/cancel` and for every runtime-initiated reason.
- `reasonDetails`: the same kind of detail `StopFailure` puts in `errorDetails`, when the runtime has one. For a declined tool call it is `<tool>: <why>`. Clipped at 1000 characters, like `StopFailure`'s `errorDetails`.
- `lastAssistantMessage`: whatever the turn had committed to the conversation at the interrupt, if any. A Ctrl+C during the final answer leaves the last committed text, or nothing if the turn never committed any. Clipped like the same field on `Stop` and `StopFailure`.
- `subagentType`: the subagent's type when the turn ran inside one, so a hook can tell a nested agent's stop from the session's. Absent in the main session.

信封上的 `timestamp` 是在钩子分发时打上的，而不是回合结束的时刻。三个回合结束事件排在同一个 worker 后面，因此等待前方慢钩子的报告携带的时间戳晚于它所描述的时刻。用 `promptId` 关联，不要用时钟。

`StopCancelled` 不能拦截：回合已经结束，允许钩子重开一个用户刻意停止的回合将与用户对抗。想让 agent 继续工作就用 `Stop`。

"取消并发送"（回合运行时输入新消息）**不会**触发 `StopCancelled`，因为回合是被替换而非停止，agent 仍保持忙碌。在子代理内部，`user_interrupt` 也不触发：它跟随父级的取消，会话级信号才是有用的那个。子代理自身的 `max_turns`、`no_progress` 或被拒绝的权限则会触发。matcher 只测试 `reason`，因此报告会话是否空闲的脚本应在 `subagentType` 存在时提前退出。

完整的忙碌与空闲指示需要五个注册。`UserPromptSubmit` 把会话标记为忙碌；
`Stop`、`StopFailure` 与 `StopCancelled` 无论回合如何结束都把它落定；`idle_prompt`
`Notification` 是三者都不报告的那些回合的兜底。只注册
`StopCancelled` 会让宿主在每个正常回合之后保持忙碌。

```json
{
  "hooks": {
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "bin/turn-started.sh" }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "bin/turn-ended.sh", "timeout": 10 }] }],
    "StopFailure": [{ "hooks": [{ "type": "command", "command": "bin/turn-ended.sh" }] }],
    "StopCancelled": [{ "hooks": [{ "type": "command", "command": "bin/turn-ended.sh" }] }],
    "Notification": [
      { "matcher": "idle_prompt", "hooks": [{ "type": "command", "command": "bin/turn-ended.sh" }] }
    ]
  }
}
```

两个脚本必须处理好的事情：

- **跟踪最新的 `promptId`，忽略旧回合的报告。** 被取消回合的报告从命令循环之外
  分发，因此可能晚于下一回合的 `UserPromptSubmit` 到达。
- **没有 `promptId` 时无条件落定。** 那是 Chaos 在报告会话而非回合：`idle_prompt`
  ping 与会话结束的 `Stop`。正是它让兜底对回退或被取代的回合（它们什么都不报告）
  生效。
- **把一个从未见其开始的 `promptId` 视为空闲。** 被中断的 bash 模式回合没有前置的
  `UserPromptSubmit` 就会报告。
- **`subagentType` 存在时提前退出。** 子代理的停止不是会话的停止。
- **先把宿主落定，再把回合记为已处理，** 这样被中途杀掉的钩子仍让回合可被纠正。
  先重读那条记录，只清除你自己记录的回合。
- **只做本地写入。** 收尾给整队列的回合结束报告半秒，之后每个 `SessionEnd` 钩子再受
  自己的超时约束（默认 1.5s；设置 `GROK_SESSION_END_HOOKS_TIMEOUT_MS`（毫秒）可改
  该默认值，上限 60s）。

`Stop` 是闸门，因此该条目运行在回合的关键路径上：保持快速、给它 `timeout`、
并以 0 退出，因为退出码 2 会拦截停止并让 agent 继续工作。如果你同时运行拦截型
`Stop` 闸门，就把 `Stop` 从中去掉，因为一次延续触发会在 agent 仍在继续时把宿主
落定；改注册 `SessionEnd`，它是唯一能落定一个在 ping 之前就退出的会话的东西。

两个脚本也会在子代理自己的会话内运行。任一脚本读到的事件在那里都携带
`subagentType`，在主会话中则省略，因此 `[ -n "$subagentType" ] && exit 0`
能把子会话从两半中都过滤掉。这对后台子代理最重要：它比父回合活得久，否则会在
父级空闲后让宿主保持忙碌。

`Stop` 输入还携带 `backgroundTasks` 与 `sessionCrons`，钩子可以借此区分"会话已结束"与"会话暂停、等待后台工作把它唤醒"。没有在运行或计划中的东西时两个数组都为空。每个 `backgroundTasks` 条目描述一个运行中的任务：`id`、`type`（`shell`、`monitor` 或 `subagent`）、`status`，以及（取决于类型）`command`（仅 shell 任务）、`description`（监视器被监视的命令行，或子代理的任务描述）和 `agentType`（子代理）。每个 `sessionCrons` 条目描述一次计划中的唤醒（`scheduler_create` 或 `/loop`）：`id`、`schedule`、`recurring` 与 `prompt`。`schedule` 值是人类可读的间隔，例如 `every 5 minutes`；Chaos 的计划是间隔，不是 cron 表达式。自由文本条目字段截断到 1000 字符，并带字符串内的 `… [+N chars]` 标记。

在子代理内部，闸门以 `SubagentStop` 触发（agent frontmatter 的 `Stop` 钩子会被自动重映射）。`Stop` 钩子只为总 agent 设闸。

`SubagentStop` 每个子代理触发一次，在子代理自己的回合结束时，与 Claude Code 一致。其输入携带一个 `phase` 字段（当前恒为 `"gate"`），为前向兼容预留。

**移植 Claude Code 停止钩子**：输出词汇（`decision`、`reason`、`continue`、`stopReason`、`additionalContext`）无需改动即可使用。对照这份清单检查与 Claude 不一致之处：

- **camelCase 输入**：Chaos 的 stdin 信封通篇使用 camelCase 键，而 Claude 用 snake_case。读取 `.stop_hook_active` 或 `.background_tasks[].agent_type` 的脚本必须改读 `.stopHookActive` 与 `.backgroundTasks[].agentType`（`hook_event_name` snake_case 键携带 Claude 的 PascalCase 值，如 `"Stop"`；`hookEventName` camelCase 键携带 Chaos 的 snake_case 值，如 `"stop"`）。通过 grok-agent-sdk 注册的钩子会把顶层键与 `backgroundTasks`/`sessionCrons` 的条目键都转换为 snake_case，因此线上格式的 `.backgroundTasks[].agentType` 在 SDK 中读作 `.background_tasks[].agent_type`。
- **`toolResult` 字段**：`PostToolUse` 的工具输出是 `toolResult`（SDK：`tool_result`）；Chaos 还会发出一个复制 `toolResult` 的 `tool_response` snake 别名，因此读取 Claude 的 `.tool_response` 的钩子无需改动即可工作。
- **`updatedToolOutput` 在内置工具上携带 Chaos 自身的输出形状**：针对内置工具的 `PostToolUse` 替换会按 Chaos 序列化的工具输出校验——即该事件 `toolResult` 里的带标签对象——因此按另一个运行时的字段名编写的替换会解析成错误形状并被忽略。MCP 工具上没有形状可强制，因此 `updatedToolOutput` 与其 `updatedMCPToolOutput` 别名一样直接通过。见 [PostToolUse 输出](#posttooluse-输出)。
- **会话结束时触发**：会话结束时会额外触发一次仅观察的 Stop；用 `reason == "end_turn"` 过滤（见上文）。
- **间隔计划**：`sessionCrons[].schedule` 是人类可读的间隔，绝不是 cron 表达式。
- **任务类型**：`backgroundTasks[].type` 只有 `shell`、`monitor` 或 `subagent`；Claude 的其他标签（`workflow`、`teammate`、…）不会被发出。
- **StopFailure 分类**：Chaos 发出六种（`rate_limit`、`authentication_failed`、`invalid_request`、`server_error`、`max_output_tokens`、`unknown`）。容量错误（503/529）归类为 `rate_limit`。针对 Chaos 不会发出的错误类的 matcher 永不触发。
- **默认超时**：Chaos 默认观察钩子 5 秒，比多数运行时短。给一个做实际工作的导入钩子显式设置 `timeout`。
- **`UserPromptSubmit` 可拦截，但有一个缺口**：退出码 2 与 `decision: "block"` 像 Claude 一样拒绝提示，被拒绝的提示绝不进入对话历史——但放行钩子的 stdout / `additionalContext` 会被丢弃，而不是作为上下文加入。
- **`StopCancelled` 是 Chaos 特有的**：使用它的配置无法移植到没有中断钩子的运行时。
- **`idle_prompt` 在任意回合结束时触发**：Chaos 在被中断或出错的回合之后也会触发它，而不只是完成的回合，因为它报告的是状态而非结果。其 `message` 是展示文本，可能随版本变化，因此改匹配 `notificationType`。
- **子代理标识是 `subagentType`，不是 `agent_type`**：Chaos 把它放在能在子代理内触发的事件的载荷里，与自己的 `SubagentStart`/`SubagentStop` 一致，而不是放在公共字段里。
- **permission_mode 取值**：Chaos 发出 `default`、`auto`、`plan` 或 `bypassPermissions`。Claude 的 `acceptEdits`/`dontAsk` 在 Chaos 中没有对应（Chaos 的 `auto` 最接近），因此 `permission_mode === "acceptEdits"` 之类的检查永不匹配。
- **客户端（SDK）闸门超时**：SDK 的 `Stop`/`SubagentStop` 闸门与文件钩子一样默认 600 秒；`PreToolUse` 客户端闸门默认 30 秒（交互热路径）。两者都可在各 matcher 组上用 `timeoutS` 覆盖，上限 600。
- **`/goal`**：Chaos 的目标循环是另一个功能，在停止闸门之前运行；它不是提示类型的 Stop 钩子。

一个脚本搞定的完整"继续工作"策略：

```bash
#!/bin/bash
input=$(cat)
# Gate only genuine turn ends, not the session-end observe fire.
if [ "$(echo "$input" | jq -r '.reason')" != "end_turn" ]; then exit 0; fi
if ! bin/verify.sh >/dev/null 2>&1; then
  echo '{"decision": "block", "reason": "verify.sh failed; fix the failures before finishing"}'
fi
```

注册为 `{ "type": "command", "command": "bin/stop-gate.sh", "timeout": 300 }`，`timeout` 按验证步骤的需要设置。钩子在每次延续之后再次触发，内置上限在 8 次后结束回合；检查 `stopHookActive`，对 agent 显然无法据以行动的反馈尽早放弃。

### 被动钩子

对 `SessionStart` 或 `Notification` 这类事件，stdout 会被忽略。成功时以 0 退出即可。例外是 `PreToolUse`（见[输出（拦截型钩子）](#输出拦截型钩子)）、`Stop`/`SubagentStop`（见[停止决定控制](#停止决定控制)）与 `PostToolUse`——它虽然什么也不拦截，stdout 仍会被读取（见 [PostToolUse 输出](#posttooluse-输出)）。

### Environment Variables

Grok 会在每个钩子进程上设置若干环境变量。编写需要感知上下文或插件的钩子脚本时它们很有用。

#### 运行器注入的变量（始终可用）

钩子运行器为**每个**钩子设置这些变量：

| 变量              | 说明 |
|-----------------------|-------------|
| `GROK_HOOK_EVENT`     | 触发钩子的事件名（如 `pre_tool_use`、`session_start`、`post_tool_use`、`session_end`、`stop`、`notification`）。 |
| `GROK_HOOK_NAME`      | 该钩子的配置名称（插件提供的钩子会带上插件前缀）。 |
| `GROK_SESSION_ID`     | 当前 Grok 会话的唯一标识符。 |
| `GROK_WORKSPACE_ROOT` | 当前工作区根目录的绝对路径。 |
| `CLAUDE_PROJECT_DIR`  | 工作区根目录的绝对路径。`GROK_WORKSPACE_ROOT` 的 Claude Code 兼容别名，每个钩子都会设置。 |

这些变量是**保留的**。你试图通过钩子 JSON 的 `env` 字段为它们设置的任何值都会在加载时被剥除（并记录一条警告），运行器在启动时始终注入真实值。

#### 插件钩子变量

当钩子来自某个插件时，Grok 还会额外注入以下变量：

| 变量             | 说明 |
|----------------------|-------------|
| `GROK_PLUGIN_ROOT`   | 插件已安装目录的绝对路径。 |
| `GROK_PLUGIN_DATA`   | 插件可写数据目录的绝对路径（用于存储插件状态、缓存等）。 |

这些值由插件系统提供。对四个插件相关键（`GROK_PLUGIN_ROOT`、`GROK_PLUGIN_DATA` 及其 Claude 别名），插件适配器确保官方插件值始终压过用户在钩子 `env` 映射中声明的任何值。

#### 用户自定义环境变量

你可以用 `env` 字段为单个钩子处理器提供额外的环境变量：

```json
{
  "type": "command",
  "command": "bin/my-hook.sh",
  "env": {
    "MY_SECRET": "value",
    "LOG_LEVEL": "debug"
  }
}
```

这些变量会透传给钩子进程，但不能覆盖上面列出的保留运行器变量或插件变量。

#### 在 `command` 与 `url` 字段中使用变量

`command` 与 `url` 都支持 `${VAR}` 与 `$VAR` 展开。在 Windows PowerShell 上，已知的 `$VAR` 引用会被改写为 `$env:VAR`，以便读取子进程环境。关于加载时与运行时展开、`env` 映射的查找顺序以及参数展开修饰符（如 `${VAR:-default}`），见自定义钩子参考。

---

## HTTP 钩子

不调用本地脚本，改为调用远程端点：

```json
{ "type": "http", "url": "https://hooks.example.com/grok-event", "timeout": 15 }
```

完整的事件信封会以 JSON 形式 POST 出去。

---

## 在 TUI 中管理钩子

### Hooks 标签页

在非 VS Code 家族终端上按 `Ctrl+L` 打开扩展弹窗（Plugins 标签页），或运行 `/hooks`（任意终端；在 `Ctrl+L` 是 interject 的 VS Code 家族上必须用它）直接打开到 Hooks 标签页。在 **Hooks** 标签页：

| 键 | 操作 |
|-----|--------|
| `r` | 从磁盘重新加载全部钩子 |
| `a` | 按路径添加自定义钩子 |
| `x` | 移除选中的钩子源（会请求确认；按小写 `y` 确认） |
| `Space` | 启用或禁用选中的钩子 |
| `f` | 循环切换状态过滤器（全部 / 已启用 / 已禁用） |

钩子按来源分组：**全局**、**项目**、**插件**与**自定义**。

每个钩子显示：
- 触发它的**事件**
- 运行的**命令**或 **URL**
- **超时**时长
- **状态**：已启用或 `[disabled]`

### 斜杠命令

```
/hooks-list           # Show hooks loaded in this session
/hooks-trust          # Trust this project for hook execution
/hooks-add <path>     # Add a custom hook file or directory
/hooks-remove <path>  # Remove a custom hook
/hooks-untrust        # Revoke trust for this project
```

在 TUI 分页器中，单个 `/hooks-*` 命令不会出现在斜杠命令列表里。`/hooks` 弹窗覆盖钩子的列出、添加、移除与启用/禁用；项目信任通过 `/hooks-trust`（或弹窗的 Trust 操作）管理，它写入的正是上文描述的统一文件夹信任存储。

### 按钩子启用/禁用

在 Hooks 标签页按 `Space` 即可在运行时启用或禁用单个钩子。改动立即生效，无需重启会话。

### 会话中途重载

在 Hooks 标签页按 `r` 从磁盘重新加载全部钩子。Grok 会重新读取每个钩子源，因此你在会话期间对钩子文件所做的改动都会被拾取。

---

## 状态行与回滚区中的钩子

除非钩子拖住回合或改变其走向，否则它们是安静的：

- 当回合被一个钩子批次拦住（工具前的 `PreToolUse` 闸门、`UserPromptSubmit` 闸门、`Stop` 闸门）时，批次运行约 300 ms 后状态行会显示 `Running pre_tool_use hook…`（或 `Running 3 stop hooks…`）。计时器从批次启动时开始，因此慢钩子显示其全部等待时间；快的则根本不显示。
- 运行并放行的钩子不留痕迹。它的 stdout 不会被展示。
- 拒绝工具调用、拦截提示或停止/延续 agent 的钩子会得到一行带原因的注记。来自 `~/.chaos`、项目与插件文件的钩子会被点名；来自受管配置的钩子显示为"a managed policy hook"。
- 失败（非零退出、超时、崩溃、输出格式错误）的钩子得到一行：`<event> hook (<name>) failed, ignored: <reason>`，其中原因是退出码加第一行 stderr，或超时。"Ignored" 是字面意思：失败按失败放行处理，工具调用或回合就像钩子放行了它一样继续。

拒绝与失败的行带有与工具行相同的子弹符号，因此读起来像是其上方工具调用的一部分。

这些行只在插件 UI 启用（默认如此）时出现。

---

## 示例：安全 Shell 防护

拦截危险的 shell 命令：

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          { "type": "command", "command": "bin/safe-shell.sh", "timeout": 5 }
        ]
      }
    ]
  }
}
```

其中 `bin/safe-shell.sh`：

```bash
#!/bin/sh
INPUT=$(cat)
CMD=$(echo "$INPUT" | jq -r '.toolInput.command // empty')

# Block destructive patterns
if echo "$CMD" | grep -qE '(rm -rf /|mkfs|dd if=|:(){ :|& };:)'; then
  echo '{"decision": "deny", "reason": "Blocked potentially destructive command"}' 
  exit 2
fi

echo '{"decision": "allow"}'
```

---

## 安全注意事项

- 全局钩子（`~/.chaos/hooks/`）以你的用户权限运行；把它们当作 shell 脚本对待。
- 项目钩子需要文件夹信任（`/hooks-trust` 或 `--trust`，与仓库本地 MCP/LSP 相同的闸门），以防恶意仓库的供应链攻击。
- HTTP 钩子会发送会话数据；只使用可信端点。
- `PostToolUse` 钩子决定模型在该工具调用中读到什么——它可以添加指令或彻底替换输出——因此要像信任 `PreToolUse` 闸门一样信任它。回滚区与转录保留真实输出，因此替换对你始终可见。

---

## 最佳实践

1. **保持钩子快速**：长时间运行的钩子会阻塞 UI。尽可能使用后台进程（`&`）或异步方式。
2. **用显式 `deny` 拦截**：钩子在出错时按失败放行，因此崩溃的钩子不会拦截工具。要强制执行策略，你的钩子必须运行到完成并在 stdout 上发出 `{"decision":"deny","reason":"..."}`。务必在脚本内部处理错误，使其总能返回显式决定。
3. **使用绝对路径或相对于钩子文件的路径**：JSON 文件旁 `bin/` 中的脚本便于移植。
4. **用弹窗测试**：按 `Ctrl+L`（非 VS Code 家族）或运行 `/hooks`，在依赖钩子之前确认它们已加载并匹配。
5. **对项目钩子做版本控制**：提交 `.chaos/hooks/`（但绝不要提交密钥）。

---

## 故障排查

- **钩子没有运行？** 在非 VS Code 家族终端上按 `Ctrl+L`（或在任意位置运行 `/hooks`），看它是否已加载并匹配。
- **项目钩子被忽略？** 该文件夹可能不受信任。运行 `/hooks-trust`（或带 `--trust` 重新启动）。
- **找不到脚本？** 检查路径是否相对于 `.json` 文件且可执行（`chmod +x`）。
- **看到错误？** 以 `RUST_LOG=debug GROK_LOG_FILE=/tmp/grok.log chaos` 启动来捕获日志，然后检查 `/tmp/grok.log`。
