# 自定义钩子指南

钩子让你在 Chaos 会话的关键时刻运行自定义脚本或 HTTP 请求，比如工具运行前后、会话开始或结束时，或代理发出通知时。

你可以用它做自动化、安全检查、日志记录、通知，以及与你自己的工具集成。

## 为什么用钩子？

常见用例：

- **安全防护**：在执行前拦截 `rm -rf /` 这类危险命令。
- **审计日志**：把每次工具使用或会话记录到文件或外部服务。
- **通知**：长任务结束时发一条 Slack/Discord 消息。
- **自动格式化**：编辑后自动运行 `cargo fmt` 或 `prettier`。
- **环境准备**：在会话开始时导出密钥或设置变量。
- **自定义工作流**：在特定事件上触发构建、测试或部署。

## 快速上手

1. 创建钩子目录：
   ```sh
   mkdir -p ~/.chaos/hooks
   ```

2. 创建一个简单的钩子文件，例如 `~/.chaos/hooks/session-start.json`：
   ```json
   {
     "hooks": {
       "SessionStart": [
         {
           "hooks": [
            { "type": "command", "command": "echo \"🚀 Grok session started in $(pwd)\"" }
           ]
         }
       ]
     }
   }
   ```

3. 启动（或重启）一个 Chaos 会话。该钩子会在 `SessionStart` 时自动运行。

   要确认它已加载，打开 Hooks 标签页：在 VS Code 系之外按 `Ctrl+L`，或在任何终端运行 `/hooks`（在 VS Code、Cursor、Windsurf 和 Zed 上推荐后者）。

## 钩子位置

钩子从多个位置被发现（全部会合并）：

| 作用域 | 路径 | 是否信任？ | 说明 |
|-----------|-----------------------------------|--------------|-------|
| 全局 | `~/.chaos/hooks/*.json` | 始终 | 个人钩子 |
| 全局 | `~/.claude/settings.json` | 始终 | Claude Code 兼容 |
| 项目 | `<project>/.chaos/hooks/*.json` | 需要信任 | 按仓库自动化 |
| 项目 | `<project>/.claude/settings.json` | 需要信任 | Claude 兼容 |
| 配置 | `config.toml`, `managed_config.toml`, `requirements.toml` | 始终 | 随你的（或你组织的）配置一起分发的钩子 |
| 插件 | 内置于已安装的插件中 | 按插件 | 团队共享钩子 |

配置文件里的钩子用 TOML 形式表达同一套 schema；详见[钩子用户指南](user-guide/10-hooks.md#hooks-in-config-files)。

**信任项目**：第一次打开带钩子的项目时，打开钩子弹窗（在 VS Code 系之外按 `Ctrl+L`，或在任意终端运行 `/hooks`），或运行 `/hooks-trust`。这与 `--trust` 是同一道文件夹信任闸门，记录在 `~/.chaos/trusted_folders.toml`。信任可防止不受信任的仓库运行任意代码。

## 钩子 JSON 格式

每个 `.json` 文件可以定义多个钩子：

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

关键字段：

- **事件名**（顶层键）：`SessionStart`、`UserPromptSubmit`、`PreToolUse`、`PostToolUse`、`Stop`、`Notification`、`SessionEnd` 等。
- **matcher**（可选）：对事件的匹配值做正则测试：工具事件上是工具名，其它事件上则是各事件自己的取值（见用户指南的钩子章节）。为空则匹配一切。
- **type**：`"command"`（运行脚本或 shell 单行命令）或 `"http"`（把事件 POST 到某个 URL）。
- **command**：可执行文件的路径（相对于 JSON 文件）或内联 shell 命令。
- **timeout**：杀死钩子前的秒数（默认：5，`Stop`/`SubagentStop`/`PostToolUse` 闸门为 600）。钩子在超时时按失败放行。

**工具名别名**：`Bash`、`Edit`、`Read` 这类 Claude 风格的名字会自动匹配 Chaos 的内部名（`run_terminal_cmd`、`search_replace`、`read_file`）。

## 编写钩子脚本

### 输入
完整事件以 JSON 形式通过 **stdin** 发送。一个 `PreToolUse` 钩子的示例：

```json
{
  "hookEventName": "pre_tool_use",
  "hook_event_name": "PreToolUse",
  "sessionId": "abc-123",
  "cwd": "/Users/you/project",
  "workspaceRoot": "/Users/you/project",
  "toolName": "run_terminal_cmd",
  "toolInput": { "command": "npm test" },
  "timestamp": "2026-04-14T12:00:00Z"
}
```

`hook_event_name`（snake_case 键）携带 Claude 的 PascalCase 值；`hookEventName`（camelCase 键）携带 Chaos 的 snake_case 值。

### 输出（拦截型钩子）
向 **stdout** 写入 JSON：

- 允许：`{"decision": "allow"}`
- 拒绝：`{"decision": "deny", "reason": "Unsafe command detected"}`

**退出码**（行为随钩子类型而不同）：
- `0`：成功 / 允许（对拦截型钩子而言）。
- `2`：显式拒绝（`PreToolUse`）、以 stderr 作为反馈的拦截停止（`Stop`/`SubagentStop`；见用户指南的停止决定控制），或给模型的反馈（`PostToolUse`——尽管工具已经运行，其 stderr 仍会到达模型）。
- 其它任何取值（包括超时、崩溃或缺少环境变量）：**失败放行**。失败会被记录，并在回滚区留一行，但工具调用不会被拦截。要拦截一次工具调用，请在 stdout 返回 JSON `{"decision":"deny","reason":"..."}`。

### PostToolUse 输出
`PostToolUse` 在工具结束后运行，因此什么也不拦截，但它的 stdout 决定模型接下来看到什么。`{"decision":"block","reason":"..."}` 会把原因连同结果一起喂给模型。`hookSpecificOutput.additionalContext` 会追加一条注记。`hookSpecificOutput.updatedToolOutput`（内置工具；必须匹配工具自身的输出形状）或 `hookSpecificOutput.updatedMCPToolOutput`（MCP 工具；不校验形状）会替换模型读到的输出，而回滚区与遥测保留原件。每个钩子的 block 原因与上下文按调用顺序送达，各自点名其钩子。只有替换是最后写入者胜出，而以非零退出的钩子只保留其 block 原因。输出替换仅限设置文件：通过 SDK 注册的 `PostToolUse` 钩子可以贡献 `block` 原因与 `additionalContext`，但不能替换工具输出。见用户指南的 PostToolUse 输出。

### 被动钩子
对 `SessionStart` 或 `Notification` 这类事件，stdout 会被忽略。成功时以 0 退出即可。

### 常用环境变量

Chaos 会把以下变量注入每个钩子进程：

- `GROK_HOOK_EVENT`：事件名（例如 `pre_tool_use`、`session_start`、`post_tool_use`）。
- `GROK_HOOK_NAME`：该钩子配置的完整名称。
- `GROK_SESSION_ID`：当前会话的标识符。
- `GROK_WORKSPACE_ROOT`：工作区根目录的绝对路径。

对插件提供的钩子，还会额外设置以下变量：

- `GROK_PLUGIN_ROOT`：插件安装目录的绝对路径。
- `GROK_PLUGIN_DATA`：插件可写数据目录的绝对路径。

这些由运行器和插件注入的变量始终优先。试图通过 `env` 字段覆盖保留的运行器键，会在加载时被剥除（并记录一条警告）。对插件钩子，`GROK_PLUGIN_ROOT` 与 `GROK_PLUGIN_DATA` 同样会压过用户为这些键提供的任何值。

### 自定义环境变量（`env` 字段）

每个处理器都可以声明额外的环境变量，注入到子进程：

```json
{
  "type": "command",
  "command": "bin/check.sh",
  "env": {
    "MY_API_TOKEN": "secret-here",
    "LOG_LEVEL": "debug"
  }
}
```

值必须是**字符串**。JSON 数字与布尔值目前无法解析；需要时请用引号包起来。

对插件钩子，插件适配器还会额外注入
`GROK_PLUGIN_ROOT` 与 `GROK_PLUGIN_DATA`。这些键会压过用户为同名键声明的
任何值（插件契约不可协商）。

### 变量替换

`command` 与 `url` 字符串支持在配置加载时做 `$VAR` 与 `${VAR}` 替换：

```json
{
  "type": "command",
  "command": "${HOME}/.config/grok-hooks/check.sh"
}
```

每个引用的查找顺序：
1. 处理器自己的 `env` 映射。
2. 当前进程环境（Chaos 自己看到的环境）。

如果某个引用在两处都未设置，它会**原样保留**（例如 `${UNSET}`
仍是那个字面字符串）。运行器注入的名字（`CLAUDE_PROJECT_DIR`、
`GROK_WORKSPACE_ROOT`、`GROK_HOOK_EVENT`、`GROK_HOOK_NAME`、
`GROK_SESSION_ID`）在加载时不取自 Chaos 进程的环境。
Unix 的 `sh -c` 会从子进程环境里展开它们；Windows PowerShell 把 `$VAR`
改写成 `$env:VAR`。HTTP 的 `url` 在请求时替换它们。其余未解析的
命令引用会以 "required env var(s) not set" 拒绝。

具体到 HTTP 钩子，`url` 还会**在请求时**再次展开
（就在 SSRF 校验之前），因此 `${GROK_PLUGIN_ROOT}/check` 这类
由插件注入的变量会按插件的真实路径解析。

#### 参数展开修饰符

POSIX 的参数展开形式在加载时**绝不**展开。它们被原样留给运行时的
`sh -c` 分支处理：`${VAR:-default}`、`${VAR-default}`、`${VAR:=x}`、
`${VAR:?msg}`、`${VAR:+x}`、`${VAR%pat}`、`${VAR#pat}`、
`${VAR/pat/repl}`、`${VAR:N:M}`。这避免加载期展开器与 POSIX shell
语义之间出现细微分歧（尤其是 `:-` 的空串行为）。

如果你的钩子命令包含 shell 元字符（空格、管道、`&&`、重定向、`$`
等），运行器会把它交给 `sh -c`，你就能得到完整的 shell 展开语义。如果
命令是不含元字符的裸路径，运行器会直接 spawn 它。即便如此，路径里的
`$VAR` / `${VAR}` 引用仍会在加载时解析，因此像 `${HOME}/bin/check.sh`
这样的直接执行路径无需包进 `sh -c`。

#### 什么不会被展开

- **`matcher`** 是正则（`$` 是行尾的正则锚点），它从不做环境变量展开。
  替换 `$VAR` 会悄悄改变正则的语义，并很可能产出一个无效模式。如果你需要
  动态的 matcher，请在写入时生成 JSON 文件。
- **`timeout`** 是数字，没什么可展开的。
- **`env` 映射本身的值**：它们被原样存储并原样传给子进程，因此
  `"BAR": "${HOME}/x"` 会把字面字符串 `${HOME}/x` 注入子进程的环境。

## 在 TUI 中管理钩子

在 VS Code 系之外按 `Ctrl+L`（或在任何终端运行 `/hooks`）打开钩子与插件模态。

在 **Hooks** 标签页里，你可以：
- `l`：重新加载所有钩子。
- `a`：按路径添加自定义钩子（很适合做测试）。
- `e`：启用或禁用。
- `r`：移除。
- `Space`：展开分组。

来自 `~/.chaos/hooks/` 的钩子显示在**全局**下，项目的显示在**项目**下，等等。

## HTTP 钩子

不跑本地脚本，改为调用远端端点：

```json
{ "type": "http", "url": "https://hooks.example.com/grok-event", "timeout": 15 }
```

完整事件信封以 JSON POST 出去。适合 webhook、分析或 serverless 函数。

## 最佳实践

1. **保持钩子快速**：长时间运行的钩子会阻塞 UI。尽可能用后台 `&` 或异步方式。
2. **用显式 `deny` 拦截**：钩子在任何错误（超时、崩溃、缺少环境变量等）下都按失败放行，因此崩溃的钩子不会拦截工具调用。要强制执行策略，你的钩子必须运行到完成并在 stdout 上发出 `{"decision":"deny","reason":"..."}`。
3. **使用绝对路径或相对于钩子文件的路径**：JSON 旁 `bin/` 中的脚本便于移植。
4. **用 Hooks 标签页测试**：在 VS Code 系之外按 `Ctrl+L`，或运行 `/hooks`，在依赖钩子之前确认它们已加载并匹配。
5. **对项目钩子做版本控制**：提交 `.chaos/hooks/`（但绝不要提交密钥）。

## 安全注意事项

- 全局钩子（`~/.chaos/...`）以你的用户权限运行。把它们当作 shell 脚本对待。
- 项目钩子需要显式信任（运行 `/hooks-trust` 或使用模态），以防恶意仓库的供应链攻击。
- HTTP 钩子会发送会话数据。只使用可信端点。

## 故障排查

- **钩子没有运行？** 在 VS Code 系之外按 `Ctrl+L`（或在任何位置运行 `/hooks`），看它是否已加载并匹配。
- **项目钩子被忽略？** 先信任该项目。
- **找不到脚本？** 检查路径是否相对于 `.json` 文件且可执行（`chmod +x`）。
- **`The argument '/.claude/hooks/….ps1' to the -File parameter does not exist`？** PowerShell 把 `$CLAUDE_PROJECT_DIR` 当成了空。除非 `GROK_SHELL=cmd`，否则 Chaos 会把它改写成 `$env:CLAUDE_PROJECT_DIR`。
- **想看错误？** 查看 pager 日志（通常在 tracing 面板或 `~/.chaos/logs`）。

## 更多示例

参见 `xai-grok-hooks` crate 里内置的示例：

- [安全 Shell 防护](../../xai-grok-hooks/examples/hooks/safe-shell.json)
- [禁止递归 grep](../../xai-grok-hooks/examples/hooks/no-recursive-grep.json)：硬拦 `grep -r`/`grep -R`/`rgrep`（OOM 防护）
- [会话审计日志](../../xai-grok-hooks/examples/hooks/session-log.json)
- [工具活动日志](../../xai-grok-hooks/examples/hooks/tool-logger.json)

把它们复制到 `~/.chaos/hooks/` 并按需定制。

## 完整参考

完整事件列表、matcher 语义、信任模型与进阶细节，见[钩子用户指南](user-guide/10-hooks.md)。

---

*写钩子愉快！* 如果你做出了什么好东西，考虑把它作为插件分享出来。
