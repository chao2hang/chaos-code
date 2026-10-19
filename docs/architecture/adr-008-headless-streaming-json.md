# ADR-008: headless 轮次改走 `--output-format streaming-json`

- Status: proposed（设计结论已定，实现待下一批）
- Date: 2026-10-12
- 依据: `docs/verification/zcode-v4-contract-2026-10-11.md`、`crates/codegen/chaos-engine/src/chaos_store.rs` 的 `read_updates_transcript` 取证

## 背景

Web/ZCode 桥对镜像（CLI 拥有的）会话续聊时，`HeadlessProcessAdapter`（`crates/codegen/chaos-engine/src/lib.rs`）
以阻塞 `.output()` 跑 `--single --resume`，`--output-format json`（单个结果对象）。后果有四，
全部在本机实测确认：

1. **假流式**：整轮结束后才把文本切块「流」出（`bounded_text_chunks`），界面与真实生产脱节。
2. **工具不可见**：CLI 在轮内执行的工具（updates.jsonl 里 222 次 `tool_call`）在主机侧零事件，
   ZCode 面板的工具卡只能靠恢复时读 `updates.jsonl`（Phase 2.1 已接）补，live 轮次没有。
3. **停止无效**：`RunControl.cancelled` 只在块间检查，而阻塞路径没有块——停止要等整轮跑完才生效。
4. **用量缺失**：`TurnReport::default()`，会话内用量恒 0（Phase 2.1 用 `turn_completed` 的历史账
   兑现了恢复视图，live 轮次仍无账）。

## 结论

CLI 已内置流式输出：`--output-format streaming-json`（`crates/codegen/xai-grok-pager/src/headless/cli.rs`，
reducer `headless/reducer/acp.rs`），NDJSON 每行一个 ACP 会话更新，词表（实测 + reducer 源码）：

    {"type":"text","data":…}
    {"type":"thought","data":…}
    {"type":"tool_call","toolCallId":…,"title":…,"toolName":…,"rawInput":…,"status":…}
    {"type":"tool_call_update","toolCallId":…,"status":"completed|failed|pending|in_progress","content":[…],"rawOutput":…}
    {"type":"plan","entries":[…]}
    {"type":"usage",…,"usage":{"inputTokens":…,"outputTokens":…,"cachedReadTokens":…,"reasoningTokens":…}}
    {"type":"end","stopReason":…,"sessionId":…,"requestId":…}
    {"type":"error","message":…}

决定：`run_prompt_turn_progressing` 改传 `--output-format streaming-json`，stdout 逐行解析（NDJSON
本来就是行协议），stderr 转到助手线程照旧收 `api_retry` 与诊断。

### 映射

| 流式行 | 引擎 |
|---|---|
| `text` | `TurnOutput::Text`（现有路径不变：追加 assistant 消息 + `TextDelta`） |
| `thought` | `TurnOutput::Thought`（新增）→ `ReasoningDelta` 事件，不进引擎转录本（与 HTTP 路径现状一致；恢复视图读 updates.jsonl） |
| `tool_call` | `TurnOutput::Tool{Started}`（新增）→ `ToolStarted`，带 CLI 的真实 `toolCallId` |
| `tool_call_update` | `TurnOutput::Tool{Updated}` → `ToolProgress`/`ToolResult`（`completed/failed` 收行），`content[].content.text` 拼 output |
| `usage` / `end` | 累积进 `TurnReport`（真实 `input/output/reasoning/cached` tokens，camelCase 字段） |
| `error` / 非零退出 | `Err`（stderr 诊断兜底） |

配套改动：

- `ServerMessage::ToolStarted/ToolProgress/ToolResult` 增 `#[serde(default)] tool_call_id: Option<String>`，
  桥的 `map_engine_event` 有真 id 用真 id（与 `updates.jsonl` 同 id，恢复视图与 live 行不会重影），
  没有则沿用 `tool-{session}-{sequence}` 合成。
- **停止生效**：`deliver` 返回 `false`（宿主在取消后不再听）即杀子进程
  （`process_scope.kill_all()` + `child.kill()`），已交付文本留在会话里——阻塞路径做不到的
  「跑到一半真停」由此兑现。
- `run_prompt`（非流式、仅测试使用）保持 `--output-format json` 不动。

### 拒绝的替代方案

- **PTY 长驻会话**（`terminal_rpc` 式驱动交互模式）：能拿到与终端完全一致的行为，但要在桥里
  重建 CLI 的交互态机（会话恢复、权限问询、压缩边界），工程量与风险都数倍于流式解析；且
  `--single` 的一次性进程模型与 ProcessScope 的进程组纪律已被测试钉死，保留。
- **解析 `streaming-messages-json`**（Anthropic wire 格式）：为 `--include-partial-messages` 的
  消费者设计，行形状是 Messages API 帧，还要自己聚 tool_use 块；ACP 原生格式零转换。

## 实现清单（下一批）

1. `TurnOutput` 增 `Thought`/`Tool` 变体；`produce_run` 的 deliver 闭包补映射（`ReasoningDelta`、
   `Tool*` 事件 + 转录本 role="tool" 记录）。
2. 适配器重写为行解析 + 子进程取消；fixture 测试沿用现有假二进制模式
   （`streaming_headless_turn_reports_stderr_api_retry_and_still_delivers_text` 的写法）。
3. 桥侧真 `toolCallId` 贯通 + 更新 `map_engine_event` 测试。
4. 验收：fixture 轮次里 stop 真的中断子进程；`usage` 行进 `TurnReport` 后
   `app_usage_snapshot` 有真数；ZCode live 面板出现工具卡。

## 不支持的说法

- 不支持「CLI 的 streaming-json 词表永远不变」：reducer 源码（`acp.rs`）与本机 capture 是
  2026-10-12 的口径；实现时以 reducer 的 `AcpLine` 枚举为唯一合同来源。
