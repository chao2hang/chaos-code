# DSH (deepseek-harness) 特性移植方案 — 2026-08-14

> 调研 DSH（`@deepseek-ai/dsh` v0.1.0-rc.6，TypeScript/Node 插件式 agent harness）后，筛选出**真正值得移植到本项目 chaos-code（Rust TUI）的特性**。本方案基于对两侧源码的实测核对，所有文件路径/行号/结构体名均已验证。

## 0. 背景与结论

### 两项目对照

| | chaos-code（本项目） | deepseek-harness (DSH) |
|---|---|---|
| 语言/形态 | Rust，TUI 终端助手（`chaos`） | TypeScript/Node，Cordis 插件式 harness |
| 架构 | Grok Build fork，~60 crate | Cordis 插件框架 + profile/bundle 分层 |
| 工具链 | bash/fs/edit/grep/todo/plan/web/workflow/scheduler/subagent/goal/mcp/skill/memory 全有 | 同类工具链，以插件包形式存在 |

### 核心结论

**chaos-code 的工具链与 agent 能力已比 DSH 更全更成熟**（goal 带分类器、workflow 用 Rhai、subagent 带 Skeptic/Strategist 角色面板、FTS 会话搜索、OTEL、会话 fork……）。逐项核对 DSH ~150 个包后，**真正值得移植的只有三个**：

| 优先级 | 特性 | DSH 包 | chaos-code 现状 | 移植价值 |
|---|---|---|---|---|
| 🥇 P1 | **Code Mode**（程序化工具调用） | `dsh-agent-tool-presentation` + `dsh-code-runtime-worker-thread` | 完全没有 | 把 N 次工具往返压成 1 次，省 token/延迟 |
| 🥈 P2 | **Ralph 循环**（无种子 fresh-agent 迭代） | `dsh-tool-ralph` | 完全没有（0 处 `ralph`） | 长任务可重启迭代，避免上下文膨胀 |
| 🥉 P3 | **Agent Preset**（persona+工具集+prompt 可切换组合） | `dsh-agent-presets` | **已有"toolset preset"系统，但仅工具集，无 persona/prompt + 无用户切换** | 体验：一键切换精简/标准/创造模式 |

DSH 其余特性要么本项目已有（compaction、session-title、otel、permission-presets、jobs、schedule、web-search…），要么与"终端原生"定位冲突（Web GUI 几十个 React 组件、HMR/Cordis 运行时），**不建议移植**。

---

## 1. P1 — Code Mode（程序化工具调用）

### 1.1 DSH 机制（实测核对）

读 `dsh-agent-tool-presentation` / `dsh-code-runtime-worker-thread` / `dsh-tools` README：

- **ToolPresentationMode** = `native` | `code` | `both`。`native` = 每个工具一个 function schema；`code` = 只暴露 `run_code` 一个传输工具 + 一段生成的 TypeScript SDK；`both` = 两种并存。
- `code` 模式下，**executor 把模型直接调用任何非 `run_code` 工具解析为 `UNKNOWN_TOOL`**，强制模型走 `run_code` 一次批量编排。
- 运行时：每个程序在**一个全新 Node `worker_threads.Worker`** 里跑，TS 输入、host 侧 stripTypeScriptTypes、bindings 经 message port 桥接、`{ value, logs, error? }` 输出。
- 防护：`computeMs`（忙时预算）+ `maxWallMs`（墙钟上限）+ `maxOutputBytes`（64MiB）+ `maxOldGenerationSizeMb`（堆上限）+ `env:{}` 空环境。**Containment not security boundary**——信任等同 bash。
- 绑定：模型写的程序调用 `tools.xxx(...)`，经端口桥接到 host 的 ToolRegistry.execute，结果以完整 JSON 回传。

### 1.2 chaos-code 现状（实测核对）

- **没有** code-interpreter / run_code / 程序化多工具调用（grep `run_code|CodeInterpreter|tool_presentation` 仅命中 OpenAI Responses API，无关）。
- **但现有基建极适合承载**：
  - `xai-workflow` crate（`crates/codegen/xai-workflow/`）已是 **Rhai 脚本编排引擎**，workspace `rhai = { version = "1.25", features = ["serde"] }`。
  - 引擎核心 `engine.rs` / `host.rs`：`WorkflowHostRequest::SpawnAgent { opts, reply }` 已支持 `AgentOpts { output_schema, fork_context, ... }`。
  - `WorkflowTool`（`crates/codegen/xai-grok-tools/src/implementations/grok_build/workflow/mod.rs`）已是面向模型的工具，输入 `{ name|script|script_path, args, agent_budget }`，后台运行。
  - 工具运行时 `crates/common/xai-tool-runtime/` + 注册中心 `crates/codegen/xai-grok-tools/src/registry/types.rs`（`register_all` / `ToolServerConfig`）。
  - 工具实现分 variant：`grok_build` / `grok_build_concise` / `opencode` / `codex`（同构工具的不同 schema 风格）。

### 1.3 关键差异：Rust 没有"现成 TS 子线程"

DSH 用 Node worker_threads 跑 TS。chaos-code 是单二进制 Rust，**不能用 Node**。两条路线：

| 路线 | 机制 | 优点 | 缺点 |
|---|---|---|---|
| **A. Rhai 复用**（推荐） | 模型写 Rhai 脚本，调 `tools.read_file(...)`/`tools.bash(...)`，host 侧把每个 `tools.xxx` 桥接到现有 ToolRegistry | 复用 xai-workflow 引擎、沙箱成熟（Rhai 本就是沙箱语言）、无新依赖 | Rhai 语法非主流，模型需学习（但可写 SDK 文档注入 prompt） |
| B. 嵌入式 TS/JS 引擎 | 引入 `boa_engine`（纯 Rust JS）或 `deno_core` 跑模型写的 JS | 模型更熟 JS | 重依赖、boa 性能/兼容性弱、deno_core 带运行时过重 |

**推荐路线 A**：Code Mode = "受限 Rhai 子集 + 自动生成的 `tools.*` 绑定"，作为现有 WorkflowTool 的一个**演示层变体**，而非全新运行时。

### 1.4 移植设计（文件级）

**新增 crate**：`crates/codegen/xai-grok-code-mode/`（叶子 crate，依赖 `xai-workflow` + `xai-tool-runtime`）

| 文件 | 职责 |
|---|---|
| `src/lib.rs` | 模块入口、`CODE_MODE_TOOL_NAME = "run_code"` |
| `src/tool.rs` | `RunCodeTool`（实现 `xai_tool_runtime::Tool`），input `{ program, args? }`，同步执行、返回 `{ value, logs }` |
| `src/sdk.rs` | **核心**：从当前会话的 `ToolServerConfig` 生成 Rhai 绑定 —— 遍历 tools，为每个工具在 Rhai scope 里注册 `tools.<name> = |args...| { host_call(<name>, args) }` |
| `src/host_bridge.rs` | `tools.xxx` 调用 → 经 channel 调 `ToolRegistry.execute(name, args)` → 结果回传 Rhai（JSON↔Dynamic） |
| `src/budget.rs` | `compute_ms`/`max_wall_ms`/`max_output_bytes` 预算与超时（仿 DSH） |
| `src/prompt.rs` | 生成的 SDK 文档段 + "只可直接调 run_code" 规则，注入 system prompt |

**修改点**：

| 文件 | 改动 |
|---|---|
| `crates/codegen/xai-grok-agent/src/config.rs` | `native_toolset_presets()` 加 `("code-mode", code_mode_toolset())`；`RunCodeTool` 进 toolset |
| `crates/codegen/xai-grok-tools/src/registry/types.rs` | `register_all` 注册 `RunCodeTool`；加 `ToolPresentationMode { Native, Code, Both }` 枚举 + per-session override |
| `crates/codegen/xai-grok-agent/src/builder.rs` | 读 session 的 presentation mode；`Code` 模式下只把 `run_code` schema 暴露给模型，executor 把其他工具直接调用解析为 unknown（仿 DSH executor-collapse） |
| `crates/codegen/xai-grok-pager/src/settings/defs.rs` | 新增 `tool_presentation` 设置项（`native`/`code`/`both`，默认 `native`） |
| `crates/codegen/xai-grok-pager/src/slash/` | 新增 `/code-mode` slash 命令切换 |

### 1.5 关键技术决策

1. **同步 vs 后台**：DSH 的 `run_code` 是**同步**（阻塞父 turn 直到完成），区别于 `workflow`（后台）。chaos-code 的 RunCodeTool 也应同步——它是"一次编排多次工具调用"的快速路径，不是长任务。
2. **沙箱边界**：Rhai 已是沙箱（无 I/O、无文件系统、无网络），比 DSH 的 Node worker 更安全。但 `tools.bash` 仍可执行任意命令——所以 Code Mode 的信任等同 bash（与 DSH 一致，README 明示 "bash-equivalent"）。
3. **输出裁剪**：`max_output_bytes`（默认 64KiB）+ `max_result_chars`（注入模型时的文本上限），超出则 `output-limit` 诊断，不截断原始值。
4. **绑定校验**：每个 `tools.xxx` 调用的 args 经 `serde_json` 校验（与正常工具调用同路径），非法 args 直接 Rhai 报错。

### 1.6 验收标准

- [ ] `run_code({ program: "let f = tools.read_file('README.md'); return f" })` 返回文件内容
- [ ] `run_code` 一次调用编排 ≥3 个工具（read+edit+bash），往返数 = 1
- [ ] Code 模式下模型直接调 `read_file` → `UNKNOWN_TOOL` 错误
- [ ] 死循环程序在 `compute_ms` 内被终止
- [ ] 输出超 `max_output_bytes` → `output-limit` 诊断
- [ ] `/code-mode` slash 切换 presentation，`/context` 显示当前模式

### 1.7 工作量估算

| 部分 | 估算 |
|---|---|
| 新 crate 骨架 + RunCodeTool | 1.5 天 |
| sdk.rs 绑定生成（遍历 ToolServerConfig → Rhai scope） | 2 天 |
| host_bridge（channel + JSON↔Dynamic） | 1.5 天 |
| budget/超时 | 0.5 天 |
| executor-collapse（Code 模式只许 run_code） | 1 天 |
| prompt 注入 + 设置/slash | 1 天 |
| 测试 | 2 天 |
| **合计** | **~9.5 天** |

---

## 2. P2 — Ralph 循环（无种子 fresh-agent 迭代）

### 2.1 DSH 机制（实测核对）

读 `dsh-tool-ralph` / `dsh-subagent` / `dsh-subagent-spawn-in-process` / `dsh-goal-round-driver` README：

- `ralph({ objective, maxRounds? })` 是面向模型的工具，**同步等待整个 run**。
- 每轮启动**一个 fresh child**，通过 `subagentProvider`（默认 `spawn`）；provider 必须 `inheritsParentContext: false`（即 spawn，非 fork）。
- **child 只收到**：immutable objective + 当前轮号/cap + "workspace 即权威"指令 + 上一轮的结构化 handoff。**不 seed 父对话历史**。
- handoff 格式：`{ status: continue|complete|blocked, summary, evidence, next_steps, blocker }`，受 `maxHandoffChars`（默认 16384）限制。
- 终止：worker 报 `complete`/`blocked`，或达 `maxRounds`（默认 256，部署上限）→ 返回 `{ runId, agentsStarted, result }`。
- child 普通失败→错误（点名失败轮 + 保留上一轮 handoff），**不重试**。
- Ralph **不碰同会话 goal 域**——它是独立的 fresh-agent 循环。
- round cap 同时作为 `WorkflowStartRequest.maxTotalAgents` 传给引擎，与引擎的总 child backstop 协调。

### 2.2 chaos-code 现状（实测核对）

- **0 处 `ralph`**（全仓库确认）。
- **但 fresh-child spawn 机制已存在**：
  - `AgentOpts.fork_context: bool`（`crates/codegen/xai-workflow/src/host.rs:30`）—— `false` = spawn（无父历史），`true` = fork（继承）。这就是 DSH 的 `inheritsParentContext`。
  - goal 子系统已用此机制：`goal_planner.rs` `fork_context: true`；`goal_classifier.rs` / `goal_strategist.rs` / `goal_summarizer.rs` 均 `fork_context: false`（**fresh child**）。
  - `SubagentBackend::spawn(request)`（`crates/codegen/xai-grok-tools/src/implementations/grok_build/task/backend.rs:38`）—— 抽象 spawn/fork。
  - workflow 引擎 `WorkflowHostRequest::SpawnAgent { opts, reply }` 已支持 `output_schema`（结构化输出 = handoff）。
  - `agent_budget`（`WorkflowToolInput::MAX_AGENT_BUDGET = 1024`）已是 child 总数 backstop。
- goal round-driver 机制：`UpdateGoalTool` + classifier（`ClassifierAchieved/NotAchieved/FailOpen`）+ `max_runs`，**但这是同会话延续**，不是 fresh-agent 循环——Ralph 是正交的。

### 2.3 移植设计（文件级）

**Ralph = 一个固定 Rhai workflow 脚本 + 一个面向模型的工具**，直接复用现有 workflow 引擎（与 DSH "an ordinary plugin over ctx.workflowEngine and ctx.subagents" 完全同构）。

**新增 crate**：`crates/codegen/xai-grok-ralph/`（叶子 crate，依赖 `xai-workflow` + `xai-grok-tools`）

| 文件 | 职责 |
|---|---|
| `src/lib.rs` | 模块入口、`RALPH_TOOL_NAME = "ralph"`、config 常量（`MAX_ROUNDS=256`、`MAX_HANDOFF_CHARS=16384`、`MAX_RESULT_CHARS=16384`） |
| `src/tool.rs` | `RalphTool`（实现 `Tool`），input `{ objective, max_rounds? }`，同步等待 run |
| `src/script.rs` | **固定 Rhai 脚本**（内嵌 const）：循环 spawn fresh child（`fork_context: false` + `output_schema` = handoff schema），传 objective + round/cap + 上轮 handoff，按 status 终止 |
| `src/handoff.rs` | handoff 结构体 `{ status, summary, evidence, next_steps, blocker }` + `maxHandoffChars` 校验（边界二次校验，仿 DSH） |
| `src/prompt.rs` | "仅当用户明确要 Ralph/fresh-agent 迭代时用；普通长任务用 goal；有界委派用 subagent/workflow" 系统提示段 |

**修改点**：

| 文件 | 改动 |
|---|---|
| `crates/codegen/xai-grok-agent/src/config.rs` | `RalphTool` 进默认 toolset（`default_grok_build_toolset()`） |
| `crates/codegen/xai-grok-tools/src/registry/types.rs` | `register_all` 注册 `RalphTool` |
| `crates/codegen/xai-grok-shell/src/` workflow 宿主 | Ralph 用 `WorkflowLaunchHandle` 启动固定脚本；需确保 `SpawnAgent` 的 `fork_context: false` + `output_schema` 路径走通 |
| `crates/codegen/xai-grok-pager/src/slash/` | （可选）`/ralph` 观察运行中循环 |

### 2.4 固定 Rhai 脚本草案（`src/script.rs`）

```rhai
// Ralph 固定编排：不可由模型改写路由
let meta = #{
    name: "ralph",
    description: "Fresh-agent Ralph loop",
    whenToUse: "Only when user explicitly asks for Ralph/fresh-agent iteration"
};

let round = 0;
let handoff = #{};  // 初始无 handoff

while round < args.max_rounds {
    round += 1;
    let opts = #{
        prompt: build_child_prompt(args.objective, round, args.max_rounds, handoff),
        fork_context: false,           // ← spawn，无父历史（核心）
        output_schema: HANDOFF_SCHEMA,  // ← 结构化 handoff
        label: "ralph-round-" + round,
    };
    let result = agent(opts);
    let report = validate_handoff(result.output, MAX_HANDOFF_CHARS);  // 边界校验

    if report.status == "complete" {
        return #{ status: "complete", summary: report.summary, rounds: round };
    }
    if report.status == "blocked" {
        return #{ status: "blocked", summary: report.summary, blocker: report.blocker, rounds: round };
    }
    // continue: 把 report 作为下轮 handoff
    handoff = report;
}

return #{ status: "budget-limited", rounds: round, last_handoff: handoff };
```

### 2.5 关键技术决策

1. **路由不可改**：DSH 把 `subagentProvider` 固定在 `WorkflowStartRequest`，模型无法 inspect/change。chaos-code 对应：固定脚本内嵌，`agent()` 调用不带 provider 选择（`AgentOpts` 无 provider 字段，由宿主决定）。
2. **fresh child 的"权威"**：child 收到 "workspace 即长期记忆，父对话与历史 child session 不 seed" 指令。chaos-code 的 `fork_context: false` spawn 已满足"不 seed 对话"。
3. **handoff 校验**：脚本内校验 + consumer 边界二次校验（`validate_handoff` 在 `RalphTool` 收到结果后再校验一次），仿 DSH "validated inside the fixed workflow and again at the consumer boundary"。
4. **不重试失败轮**：`agent()` 抛错→workflow error（点名失败轮，保留上一轮 handoff）。chaos-code 的 `WorkflowError::AGENT_START` 已支持。
5. **与 goal 正交**：Ralph 不调 `update_goal`，不碰 goal 域；goal 是同会话延续，Ralph 是跨轮 fresh-child 循环。文档明确二者区别。

### 2.6 验收标准

- [ ] `ralph({ objective: "实现 X" })` 启动循环，每轮 spawn fresh child（无父对话）
- [ ] child 报 `complete` → 循环结束，返回最终 summary + rounds
- [ ] child 报 `blocked` → 循环结束，返回 blocker + rounds
- [ ] 达 `max_rounds` → 返回 `budget-limited` + 最后 handoff
- [ ] child 普通 failure → error 点名失败轮，不重试
- [ ] handoff 超 `max_handoff_chars` → workflow 失败（非截断）
- [ ] `/ralph` 可观察运行中循环状态

### 2.7 工作量估算

| 部分 | 估算 |
|---|---|
| 新 crate 骨架 + RalphTool | 1 天 |
| 固定 Rhai 脚本 + child prompt 构建 | 1.5 天 |
| handoff 结构 + 双重校验 | 1 天 |
| 接 workflow 宿主（fork_context:false + output_schema 路径验证） | 2 天 |
| prompt 注入 + 注册 | 0.5 天 |
| 测试（含边界：超限/失败轮/达 cap） | 2.5 天 |
| **合计** | **~8.5 天** |

---

## 3. P3 — Agent Preset（persona+工具集+prompt 可切换组合）

### 3.1 DSH 机制（实测核对）

读 `dsh-agent-presets` / `dsh-agent-tool-presentation` README + `config/agent-presets/*/preset.yml`：

- preset = 一个目录，含 `agent.cordis.yml`（plugin row 列表）+ `preset.yml`（显示元数据 `name`/`description`/`order`）。
- 内置 4 个：`standard`（全编码 agent）、`code`（标准 + Code Mode 工具呈现）、`cordis`（标准 + 可改 harness 自身）、`minimal`（极简双工具）。
- roster 服务：`list()`/`resolve(id)`/`mount(agentCtx, id)`/`recompose(agentCtx, id)`（切换，仅 blank 会话）/`copy(from, id)`（复制即创作）/`remove(id)`。
- **切换仅限 blank 会话**（未产出任何内容的 agent）——换工具集会让历史 tool call 无法复现，product rule。
- preset 决定 model 看到的工具 schema + prompt 段 + persona，须从日志可重建（`agent-preset/selected` session event）。
- child 继承父 preset（`composeFrom` bind，非 re-mount by id，避免父已切换/已删导致不一致）。

### 3.2 chaos-code 现状（实测核对，关键发现）

**chaos-code 已有"toolset preset"系统**（`crates/codegen/xai-grok-agent/src/config.rs`）：

- `register_toolset_preset(name, builder)` / `register_internal_toolset_preset`（Public/Internal 可见性）
- `toolset_for_preset(preset)` / `preset_names()` / `all_toolset_presets()`
- 7 个内置 native preset：`grok-build` / `grok-build-concise` / `grok-build-plan` / `codex` / `explore` / `plan` / `grok-computer`
- 每个 preset = `ToolServerConfig { tools, behavior_preset }`（**仅工具集，无 persona/prompt**）
- 外部可 `register_toolset_preset` 注册 out-of-tree preset

**已有但缺的**：
- ✅ toolset preset 注册/解析/枚举
- ❌ preset **不携带 persona / prompt 段**（只有工具集）
- ❌ 无面向用户的 `--preset`/`/preset` 切换
- ❌ 无"blank-only 切换"约束（product rule）
- ❌ 无 `preset.yml` 显示元数据 + 文件目录式创作
- 另有 `[clients]`（claude-code/codex/grok-build/workbuddy）—— 但这是**请求身份层**（headers/UA），不碰工具/prompt，与 preset 正交。

### 3.3 移植设计（文件级）

**策略：扩展现有 toolset preset 系统为完整 preset（+persona+prompt），而非新建并行系统。**

| 文件 | 改动 |
|---|---|
| `crates/codegen/xai-grok-agent/src/config.rs` | `ToolsetPresetBuilder` 返回类型从 `fn() -> ToolServerConfig` 扩展为 `fn() -> AgentPresetConfig`（含 `toolset` + `persona: Option<String>` + `prompt_sections: Vec<PromptSection>`）。`native_toolset_presets()` 7 个补 persona/prompt |
| `crates/codegen/xai-grok-agent/src/prompt/` | 系统提示组装处读当前 preset 的 persona + prompt_sections，注入 |
| `crates/codegen/xai-grok-shell/src/agent/mvp_agent/` | session 记录当前 preset id（durable header），切换需 blank-check |
| `crates/codegen/xai-grok-pager/src/app/cli.rs` | 加 `--preset <name>` CLI 参数 |
| `crates/codegen/xai-grok-pager/src/slash/` | 新增 `/preset` slash 命令（列表/切换，仿现有 `/client`）|
| `crates/codegen/xai-grok-config-types/src/lib.rs` | `[agent] preset = "grok-build"` config 项（默认） |
| `crates/codegen/xai-grok-pager/src/app/agent_view/` | dashboard 显示当前 preset（仿 session title） |

**新增**（可选，文件目录式创作，P3.2 阶段）：

| 文件 | 职责 |
|---|---|
| `crates/codegen/xai-grok-preset-files/` | 从 `~/.chaos/presets/<id>/` + `.chaos/presets/<id>/` 发现 TOML preset（`preset.toml` = 显示元数据 + persona + toolset preset name 引用 + prompt 段） |

### 3.4 新增内置 preset 草案

| id | name | 工具集 | persona | 用途 |
|---|---|---|---|---|
| `standard`（=现 `grok-build`） | 标准模式 | 全工具 | 默认 | 日常编码 |
| `minimal` | 极简模式 | bash + str_replace_editor | 默认 | 快速、低 token |
| `code` | Code 模式 | standard + Code Mode 呈现 | 默认 + "优先用 run_code 批量编排" | 省往返（依赖 P1） |
| `explore` | 探索模式 | read + grep + search | "只读分析，不修改" | 代码审查 |
| `plan` | 计划模式 | read + grep + todo + plan | "先规划后执行" | 规划 |

（`explore`/`plan` 工具集已存在，只需补 persona。）

### 3.5 关键技术决策

1. **不复制 DSH 的 Cordis 插件模型**：DSH preset 是 plugin row 列表（TS 插件）；chaos-code 是 Rust 编译期 preset + 可选 TOML 文件 preset。内置 preset 走 `native_toolset_presets()`，用户自创走文件目录（P3.2）。
2. **blank-only 切换**：复用现有"session 已产出内容则不可切"的 product rule。切换写 `agent-preset/selected` session event（仿 DSH，保证日志可重建）。
3. **child 继承**：subagent spawn 时 child 的 preset = 父当前 preset（读 live scope，非 header，仿 DSH `composeFrom` 语义）。
4. **与 `[clients]` 正交**：preset = persona+工具+prompt；client = 请求身份。二者独立，可叠加（`--preset minimal --client codex`）。

### 3.6 验收标准

- [ ] `--preset minimal` 启动会话，工具集为 bash+edit
- [ ] `/preset` 列出所有 preset，可切换
- [ ] 已产出内容的会话切换 preset → 拒绝（blank-only）
- [ ] preset 的 persona 注入系统提示
- [ ] subagent 继承父 preset
- [ ] session 恢复时按日志重建 preset（非 header 默认）
- [ ] dashboard 显示当前 preset

### 3.7 工作量估算

| 部分 | 估算 |
|---|---|
| `AgentPresetConfig` 扩展 + 7 个 preset 补 persona/prompt | 1.5 天 |
| 系统提示组装读 preset | 1 天 |
| `--preset` CLI + `/preset` slash + blank-check | 1.5 天 |
| session 记录 + 恢复重建 | 1.5 天 |
| dashboard 显示 | 0.5 天 |
| （可选 P3.2）文件目录式 preset 发现 | 2 天 |
| 测试 | 2 天 |
| **合计** | **~8 天**（P3.1）/ **+2 天**（P3.2） |

---

## 4. 实施顺序与依赖

```
P3.1 Preset 基础（扩展 toolset preset + persona/prompt + 切换）   [~8 天，无依赖]
   │
   ├─► P1 Code Mode（依赖 preset 的 "code" 变体作为展示层）       [~9.5 天，依赖 P3.1]
   │
   └─► P2 Ralph（独立，可并行，复用 workflow+spawn 基建）          [~8.5 天，无依赖]
```

**推荐顺序**：
1. **先 P3.1**（Preset 基础）——为 Code Mode 提供展示层变体宿主，且工作量小、用户立即可感知。
2. **P2 并行**（Ralph）——完全复用现有 workflow+spawn 基建，独立可做。
3. **后 P1**（Code Mode）——最大块，但依赖 P3.1 的 preset 展示层。

**总工作量**：P3.1(8) + P2(8.5) + P1(9.5) ≈ **26 天**（不含 P3.2 文件目录式创作的 +2 天）。

## 5. 不移植清单（已核对，避免浪费）

| DSH 特性 | 不移植原因 |
|---|---|
| Web GUI（`dsh-client-ui-*` 数十 React 组件） | 与终端原生定位冲突；已有 `serve`/WebSocket 远程模式 |
| HMR / Cordis 插件框架 | TS 运行时特性，Rust 编译产物不适用 |
| Profile/bundle/patch 分层 | TOML 配置 + CHAOS_HOME 解析已足够；仅可借鉴 `--dump-config` 可观测点 |
| repeat-tool-reminder | TodoWrite nudge 已部分覆盖 |
| output-retention | compaction 已覆盖 |
| message-feedback / anonymous-user-id | 遥测默认关，意义不大 |
| session-query-sqlite | 已有 FTS（`search_fts.rs`） |
| 其余工具（compaction/session-title/otel/permission/jobs/schedule/web-search/...） | 均已有等价或更优实现 |

## 6. 风险与缓解

| 风险 | 缓解 |
|---|---|
| Code Mode 的 Rhai 绑定生成复杂（每个工具 schema→Rhai 签名） | 先支持核心工具（bash/read/edit/grep/list），再扩展；schema 不匹配时退化为 `tools.call(name, json_args)` 通用入口 |
| 模型不熟 Rhai 语法 | 在 prompt 注入完整 SDK 文档 + 示例；Rhai 语法接近 Rust/JS，模型适应快 |
| Ralph fresh-child 的 `output_schema` 路径未充分验证 | 先写集成测试验证 `fork_context:false + output_schema` 的 spawn 端到端，再写 Ralph 脚本 |
| Preset 切换破坏历史工具调用 | 严格 blank-only 约束 + session event 日志可重建（仿 DSH） |
| 与上游 Grok Build 同步冲突 | 三个特性均为 Chaos 专属新增 crate，不污染上游 crate，同步时保留（仿现有 `clients`/BYOK 适配） |

## 7. 后续行动

1. 用户确认本方案后，按 §4 顺序逐个开 plan 文档（`docs/plan/code-mode-plan.md` / `ralph-plan.md` / `agent-preset-plan.md`）。
2. 每个特性先写集成测试骨架（验收标准 §1.6/§2.6/§3.6），再实现。
3. 每特性完成后移入 `docs/done/`。
