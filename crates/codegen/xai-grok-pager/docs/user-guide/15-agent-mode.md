# 代理模式（ACP）与编辑器集成

代理模式把 Chaos 作为长期运行的服务器启动，客户端通过 [ACP](https://agentclientprotocol.com)（JSON-RPC）与它通信。IDE、SDK、评测框架和自定义应用都能接入。若只要发一次提示、打印结果后退出，请改用 `chaos -p`（[无头模式](14-headless-mode.md)）。

---

## 自动化与 SDK

脚本、CI、评测和代理服务器请从始终批准模式起步，这样工具运行时不会弹出交互式权限提示。拒绝规则与钩子仍然生效。

```bash
# stdio (local process / many SDKs)
chaos agent --always-approve stdio

# WebSocket server
chaos agent --always-approve serve --bind 127.0.0.1:2419 --secret <token>
```

也可以在 `session/new` 上按会话开启始终批准：

```json
{
  "cwd": "/path/to/project",
  "mcpServers": [],
  "_meta": { "yoloMode": true }
}
```

交互式 TUI 用户通常保持默认的询问模式（或改用自动模式）。详见[权限与安全](22-permissions-and-safety.md)。

---

## 什么是 ACP？

[Agent Client Protocol（ACP）](https://agentclientprotocol.com)规定了客户端如何通过 JSON-RPC 与编码代理通信。Chaos 覆盖的部分包括：

- 会话（创建、加载、恢复）
- 提示与流式回复
- 工具调用更新
- 推理 / 思考流
- 会话未开启始终批准时的权限提示

---

## stdio 传输

stdio 是最常见的本地集成方式。代理在 stdin 与 stdout 上传输 JSON-RPC：

```bash
chaos agent --always-approve stdio
```

典型客户端是 IDE 扩展（Zed、Neovim、Emacs）、自定义工具和 ACP SDK。

### 选项

代理选项对每种传输都适用（`stdio`、`serve`、`headless`、`leader`）。它们写在 `agent` 之后、模式名之前。模式专属的标志写在模式之后（例如 `serve --bind`）。

```bash
chaos agent --always-approve --model grok-4.6 stdio
chaos agent --always-approve serve --bind 127.0.0.1:2419 --secret <token>
```

| 标志 | 说明 |
| ---- | ----------- |
| `-m, --model <MODEL>` | 模型 ID（例如 `grok-4.6`）。 |
| `--always-approve` | 运行时不再弹出交互式工具权限提示。别名：`--yolo`。 |
| `--reauth` | 在代理启动前完成认证。 |
| `--agent-profile <PATH>` | 从文件加载代理配置档。 |
| `--leader` / `--no-leader` | 连接共享的 leader 进程，或强制使用本地代理。请求非 `off` 的沙箱配置档时会拒绝 leader 模式，好让工具留在进程内（见[沙箱模式](18-sandbox.md)）。 |

---

## 服务器模式

```bash
chaos agent --always-approve serve --bind 127.0.0.1:2419 --secret <token>
```

客户端通过 WebSocket 连接，并用密钥 token 认证。省略 `--secret` 时，代理会在启动时打印一个生成的 token；也可以设置 `GROK_AGENT_SECRET`。进程在客户端重连之间保持状态。权限与其他入口一致，详见[权限与安全](22-permissions-and-safety.md)。

这个服务器由你自己运行——`chaos agent serve` 由你启动，不依赖托管服务。

---

## WebSocket 中继

要让代理可以从公网访问，请把代理接到中继上，并让浏览器指向同一个中继：

```bash
chaos agent --always-approve headless --grok-ws-url wss://your-relay.example.com/ws
```

---

## ACP 协议基础

通信遵循 JSON-RPC 2.0 格式。一次典型会话的生命周期：

1. **初始化** —— 客户端发送 `initialize`，携带自身能力
2. **创建会话** —— 客户端发送 `session/new`，携带工作目录
3. **发送提示** —— 客户端发送 `session/prompt`，携带用户消息
4. **接收更新** —— 代理发送 `session/update` 通知，携带流式内容
5. **处理权限** —— 代理可能请求工具执行授权（也可按权限模式直接允许或拒绝）

### 架构

```
+------------------------------------------+
|           ACP Client                     |
|  (IDE, Editor, Custom Application)       |
+-------------------+----------------------+
                    | JSON-RPC over stdio
+-------------------v----------------------+
|           chaos agent stdio              |
|                                          |
|  +---------+  +---------+  +---------+   |
|  | Session |  |  Tools  |  |   MCP   |   |
|  | Manager |  | Registry|  | Servers |   |
|  +---------+  +---------+  +---------+   |
+------------------------------------------+
```

---

## 流式更新

ACP 以流的方式发送结构化事件。每条 `session/update` 通知都带一个 `sessionUpdate` 字段，标明更新类型：

| `sessionUpdate` 的取值 | 说明                                            |
| --------------------- | ----------------------------------------------------- |
| `agent_message_chunk` | 代理回复文本中的一个片段。 |
| `agent_thought_chunk` | 代理内部推理中的一个片段。 |
| `tool_call`           | 一次新的工具调用（标题、类型、状态、输入）。 |
| `tool_call_update`    | 对进行中的工具调用的状态或结果更新。 |
| `plan`                | 代理的执行计划。 |

每条更新都自带类型，客户端可以据此为推理、工具调用和回复文本分别渲染不同的面板。

---

## 扩展方法

在基础 ACP 协议之外，Chaos 还在 `x.ai/` 前缀下定义了一批扩展方法，用于自身专有的功能。其中包括：

| 类别                   | 前缀               | 示例                                         |
| -------------------------- | -------------------- | ------------------------------------------------ |
| **文件系统**             | `x.ai/fs/*`          | `list`, `exists`, `read_file`, `write_file`      |
| **Git**                    | `x.ai/git/*`         | `status`, `stage`, `commit`, `diffs`, `discard`  |
| **Git 工作树**           | `x.ai/git/worktree/*`| `create`, `remove`, `apply`, `list`, `gc`        |
| **搜索**                 | `x.ai/search/*`      | `fuzzy/open`, `fuzzy/change`, `content`          |
| **终端**               | `x.ai/terminal/*`    | `create`, `kill`, `output`, `wait_for_exit`      |
| **会话管理**     | `x.ai/session/*`     | `fork`, `resolve_local_for_worktree_resume`      |
| **会话与历史** | `x.ai/*`             | `prompt_history`, `rewind/*`, `compact_conversation` |
| **认证**         | `x.ai/auth/*`        | `get_url`, `submit_code`                         |
| **反馈与遥测**   | `x.ai/*`             | `feedback`, `telemetry/*`                        |

下面的表只列出各类别中有代表性的方法。`x.ai/*` 这套方法是 Chaos 专有的，会随版本增加，所以不要当成完整清单；可用方法请从代理的 `initialize` 响应里发现。

### 通知（代理 → 客户端）

代理会向客户端推送通知，用于实时更新：

| 通知               | 说明                          |
| -------------------------- | ------------------------------------ |
| `x.ai/search/fuzzy/status` | 模糊搜索结果更新 |
| `x.ai/git/worktree/status` | 工作树创建进度 |
| `x.ai/fs_notify`           | 文件系统变更通知 |
| `x.ai/fs/index`            | 完整文件索引更新 |
| `x.ai/fs/index/delta`      | 增量文件索引更新 |
| `x.ai/session_notification`| 会话级更新（diff 审阅、重试状态、自动压缩） |
| `x.ai/session/update`      | 会话更新（工具调用、内容） |

---

## 会话配置选项

`session/new` 与 `session/load` 的响应里带一个带类型的 `configOptions` 列表（这是标准 ACP，不是 `x.ai/` 扩展）。改运行中的选项用 `session/set_config_option`。

| `configId` | 类别 | 效果 |
|------------|----------|--------|
| `model` | `model` | 切换会话所用的模型（`allowed_models`、chat gateway 路由）。值必须是字符串 id。 |
| `reasoning_effort` | `thought_level` | 在不换模型的前提下给当前模型施加推理强度（不重写提示、不过 `allowed_models` 门）。值必须是字符串 id（`minimal`、`low`、`medium`、`high`、`xhigh`）。当模型没有声明 `supportsReasoningEffort` 时，该值会被丢弃并给出警告。 |

```json
{
  "sessionId": "…",
  "configId": "reasoning_effort",
  "value": { "value": "high" }
}
```

响应是**完整且已更新**的选项列表。`config_option_update` 会话通知会把它同步给每个已订阅的客户端。leader 模式下，代理会旁听 `configId: model`，让各客户端的 `default_model` 保持同步。布尔值会被拒绝；暴露布尔型选项尚未实现。

---

## 会话 `_meta` 选项

`session/new` 上的可选字段：

| 字段 | 说明 |
| ----- | ----------- |
| `rules` | 追加到系统提示之后的额外规则。 |
| `systemPromptOverride` | 替换掉系统提示。 |
| `agentProfile` | 代理配置档的名称或 JSON 对象。 |
| `yoloMode` | 为 `true` 时，本会话开启始终批准。 |
| `autoMode` | 为 `true` 时，本会话使用自动权限模式。已开启始终批准时本项被取代。 |

```json
{
  "cwd": "/path/to/project",
  "mcpServers": [],
  "_meta": { "yoloMode": true }
}
```

---

## ACP SDK

官方为多种语言提供了 SDK 库：

| 语言   | 包                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------- |
| TypeScript | [`@agentclientprotocol/sdk`](https://www.npmjs.com/package/@agentclientprotocol/sdk)     |
| Rust       | [`agent-client-protocol`](https://crates.io/crates/agent-client-protocol)                |
| Python     | [`agent-client-protocol-python`](https://github.com/PsiACE/agent-client-protocol-python) |
| Go         | [`acp-go-sdk`](https://github.com/coder/acp-go-sdk)                                     |
| Kotlin     | [`acp`](https://github.com/agentclientprotocol/kotlin-sdk)                               |

---

## 兼容的客户端

| 客户端                                                   | 状态      |
| -------------------------------------------------------- | ----------- |
| [Zed](https://zed.dev/docs/ai/external-agents)           | 支持   |
| [Neovim](https://neovim.io)（插件 CodeCompanion、avante.nvim） | 支持   |
| [Emacs](https://github.com/xenodium/agent-shell)         | 支持   |
| [marimo notebook](https://github.com/marimo-team/marimo) | 支持   |
| JetBrains                                                | 即将支持 |

---

## 集成示例：一个 TypeScript ACP 客户端

```typescript
import { spawn, ChildProcess } from "child_process";
import * as readline from "readline";

class GrokACPChat {
  private proc!: ChildProcess;
  private sessionId!: string;
  private rl!: readline.Interface;

  constructor(private cwd = ".") {}

  async init() {
    this.proc = spawn("chaos", ["agent", "--always-approve", "stdio"]);
    this.rl = readline.createInterface({ input: this.proc.stdout! });

    await this.request("initialize", {
      protocolVersion: 1,
      clientCapabilities: {
        fs: { readTextFile: true, writeTextFile: true },
        terminal: true,
      },
    });

    const { sessionId } = await this.request("session/new", {
      cwd: this.cwd,
      mcpServers: [],
      _meta: { yoloMode: true },
    });
    this.sessionId = sessionId;
    return this;
  }

  private async request(method: string, params: any): Promise<any> {
    return new Promise((resolve) => {
      const msg = JSON.stringify({ jsonrpc: "2.0", id: 1, method, params });
      this.proc.stdin!.write(msg + "\n");

      this.rl.once("line", (line) => {
        resolve(JSON.parse(line).result || {});
      });
    });
  }

  async *streamPrompt(text: string) {
    const msg = JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      method: "session/prompt",
      params: {
        sessionId: this.sessionId,
        prompt: [{ type: "text", text }],
      },
    });
    this.proc.stdin!.write(msg + "\n");

    for await (const line of this.rl) {
      const data = JSON.parse(line);

      if (data.method === "session/update") {
        const update = data.params.update;
        yield update; // { sessionUpdate, content, title, ... }
      } else if (data.result) {
        break; // Final response
      }
    }
  }
}

// Usage
const client = await new GrokACPChat(".").init();

for await (const update of client.streamPrompt("List the files in this project")) {
  switch (update.sessionUpdate) {
    case "agent_message_chunk":
      process.stdout.write(update.content?.text || "");
      break;
    case "agent_thought_chunk":
      console.log(`\n[Thinking: ${update.content?.text}]`);
      break;
    case "tool_call":
      console.log(`\n[Tool: ${update.title}]`);
      break;
  }
}
```

---

## 相关资源

- [ACP Specification](https://agentclientprotocol.com/protocol/prompt-turn)
- [Protocol Introduction](https://agentclientprotocol.com/overview/introduction)
