# ZCode V4 会话合同取证（2026-10-11）

本文件把 Chaos Web 桥（`crates/codegen/xai-grok-web/src/chaos_bridge.rs`）要对齐的
ZCode 官方前端 V4 会话合同逐字段钉下来，作为 Phase 1（stop、resolveInteraction、
switchModelConfig、工具卡行、pendingInteractions）的实现依据。证据来自本机正在服务的
官方前端 bundle（`~/.chaos/web/zcode-assets/assets/index-*.js`，`CHAOS_WEB_ASSETS_DIR`
指定，见 `~/.config/systemd/user/chaos-zcode-web.service`），按字符偏移逐段取回；
引擎侧现状对照 `crates/codegen/chaos-engine/src/lib.rs` 的 `ServerMessage`
（`:1207` 起）与 `crates/codegen/chaos-engine/src/usage.rs`。

## 1. 提交信封（sendConversationCommandV4）

信封构造（bundle 偏移 955 305，逐字段）：

    { commandId, clientId, sessionId, baseRevision?, baseLogEpoch?, type, payload, issuedAt }

`type` 的全集（zod `$ce` 表，偏移 143 510 起，`ele = Mi(Object.keys($ce))`）：

    createSession  createSelectionSideSession  sendText  sendGoalCommand  stop  compact
    forkAssistant  applyFileRewind  editUserQuery  retryTurn  setAssistantFeedback
    sendQueuedNow  editQueueItem  reorderQueueItem  deleteQueueItem  setAutoDrain
    resolveInteraction  respondWorkspaceHookReview  toggleWorkspaceHookReviewItem
    revokeWorkspaceHookTrust  requestWorkspaceHookReview  snoozeInteractionAutoResolution
    switchModelConfig  switchCollaborationMode  setFollowupMode  pauseGoal  resumeGoal
    cancelBackgroundWork  resumeWorkflowRun  startSavedWorkflow  amendWorkflowRunSettings
    renameSession  deleteSession  discardSharedContext

CAS 命令（必须携带 `baseRevision`，偏移 955 305 的 `ale` 集合）：
`applyFileRewind、forkAssistant、editUserQuery、retryTurn、setAssistantFeedback、
sendQueuedNow、editQueueItem、reorderQueueItem、deleteQueueItem、setAutoDrain、
switchModelConfig、switchCollaborationMode、setFollowupMode、pauseGoal、resumeGoal`。

`sendText` 的 payload（偏移 141 500 窗口）：

    { text, attachments?, requestedDelivery?: startNow|queue|guide,
      browserAmbientContext?, context_refs?, heldQueueDisposition?, expectedHeldQueue…? }

`createSession` 的 payload：`{ workspaceId, firstInput?: { text, attachments?,
modelSelection?, mode?, planEnabled? }, config?: { modelSelection?, provider?, model?,
thought?, followupMode?, mode?, planEnabled? }, … }`。

## 2. 命令应答（ack）

    { memoryEnabled?, ttftExcluded?, commandId, status: accepted|rejected|stale|duplicate|noop|failed,
      reasonCode?, message?, revisionAtDecision, result? }

`result.type` 的判别集（`sle`，偏移 147 248）：

    createSession|createSelectionSideSession|forkAssistant → { sessionId, input?: {delivery, inputId, messageId?} }
    resolveInteraction   → { resolvedBy: { clientId, optionId? } }
    inputAccepted        → { delivery: startNow|queue|guide, inputId, messageId? }
    inputDisposition     → { delivery }
    applyFileRewind / editUserQuery / startSavedWorkflow / amendWorkflowRunSettings → 各自形状

## 3. 会话快照与行

快照 schema `Bse`（偏移 112 792）的开放字段：`control、availability、inputRouting、meta、
config、modelTransition、usage、queue、pendingInteractions、pendingCommands、
backgroundWorks、subagents、workflowRuns、goal、plan、rows`。

行操作：`row.appended | row.upserted | row.removed | row.delta | state.updated |
workflowRun.updated | workflowRun.removed`。`row.delta` 的 path 与行种的对应（`Kse`，偏移 113 929）：

    text        → kind ∈ {assistantText, reasoning}
    inputText   → kind = toolCall
    output.text → kind = toolCall 且有 output
    summaryText → kind = subagent

toolCall 行的状态集（投影表 `KVt`，偏移 2875340，这里是**装配器发出的值**）：

    inputStreaming | pendingApproval | running | success | error | cancelled

toolCall 行字段（`hkt` 事件行与 `JVt` 读取处）：`rowId、turnId、createdAt、createdAtSeq、
kind='toolCall'、toolCallId、toolName、status、input?、inputText、output?{text}、error?{message}`。
userInput 行字段（`yKt`，偏移 3048088）：`…、kind='userInput'、text、origin='realUser'、
sourceCommandId、clientId、attachments、productTurnId、entityId`。

## 4. 交互（pendingInteractions / resolveInteraction）

条目形状：`{ interactionId, payload, autoResolution? }`；`payload.kind` 全集：
`permission | userInput | workspaceHookReview`（`AVt`/`KZt` 读取处）。

permission 的 payload（`RZt`，偏移 3 209 376）：

    { kind:'permission', toolName, summary,
      options: [{ optionId, kind, label, response? }],
      fullAccessOption?, freeText?, origin?, display?, detail?, toolCallId? }

option.kind 经 `b$` 归一化（偏移 3 182 943）：含 `allow/approve` → allowOnce|allowAlways
（再加 `always`），含 `reject/deny` → rejectOnce|rejectAlways；`optionId === 'fullAccess'`
是全量授权项（`JXt` 排序处）。`response` 是客户端展示用的决定投影：
`{ decision: 'allow'|'deny', permissionUpdates?: [{ behavior:'allow', rules:[{toolName, ruleContent?}], type:'addRules' }] }`。

userInput 的 payload（`zZt`/`BZt`）：`{ kind:'userInput', prompt, freeText?, sensitive?,
options?: [{optionId, label}], toolName?, questions?: [{question, header, options:[{value,label,description}], multiSelect?}], schema? }`。

应答统一走 `resolveInteraction`：`{ interactionId, answer: { optionId?, freeText?,
action?: accept|decline|cancel, content? } }`（zod 偏移 145 130）。permission 的应答只带
`optionId`（拒绝理由走 `freeText`，`xZt` 的 `N` 回调）；userInput 的应答带 `optionId`
或 `freeText` 或 `action`+`content`。

## 5. 引擎侧对应物与落差

| V4 概念 | 引擎已有 | 落差 |
|---|---|---|
| stop | `ClientMessage::Cancel`（`:5400`，写审计、幂等、`RunControl.cancelled` 是唯一停止信号） | 桥未接线 |
| resolveInteraction(permission) | `Approve / ApproveRemembered / Reject{reason}` | 桥未接线，`pendingInteractions` 恒空 |
| resolveInteraction(userInput) | `RespondQuestion{answer}` | 同上 |
| switchModelConfig | `SetModel` + `run_options()` 每轮现读 | 桥未接该信封 |
| switchCollaborationMode(build/edit/plan/yolo) | `SetMode` / `ListModes` | 桥只 ack 不解析 |
| toolCall 行 | `ToolStarted/ToolProgress/ToolResult`（`:1332` 起） | `map_engine_event`（桥 `:2054`）只映射 4 种事件 |
| usage.delta | `Usage{input,output}` + `usage.rs` 账本 | 快照 usage 写死 0 |

## 5.1 plan 字段（2026-10-11 深挖补遗）

第一轮取证没拿到 plan 的形状（`oi` 随共享 chunk 压缩导出）。对懒加载 chunk 全量扫描后，
从状态面板渲染器（`Xgt`/`qgt`/`Vgt`/`Ugt`）读出了权威形状：

    plan: { totalCount, completedCount, displayItems: [{ id, content, status }] }

status 值域（`Vgt` 图标分支）：`completed` → 完成图标，`inProgress`（camelCase）→ 进行中，
其余一律按 pending 渲染；`Ugt` 的聚焦分组同样只找 `inProgress`。引擎 `PlanUpdated` 事件
（`PlanStep{id,title,status}`）已按此映射上桥（`plan_wire`），但引擎当前**没有任何路径发出
该事件**——live 的计划显示仍待 Phase 2 从 `updates.jsonl` 映射，本节只钉合同。

## 6. 不支持的说法

- 不支持「这就是官方前端的全部行为」：取证面只有 bundle 里的 zod 合同与调用点，
  没有对官方服务器抓帧；`expectedForegroundExecutionId`、`heldQueueDisposition`、
  `browserAmbientContext` 这些字段的运行时语义按名字与约束推断，未实测。
- 不支持把 bundle 里的实现当作 Chaos 该照抄的代码：本文件只取**合同**（字段名、
  判别集、状态集），实现全部在 `chaos_bridge.rs` 以 Rust 重建。
- 行 schema 的 zod 定义（`Pa`）随共享 chunk 压缩导出，本机未逐字段取回；toolCall/
  userInput 行的字段清单取自读取处的实际使用（`Kse`/`JVt`/`yKt`/`hkt`），可能有
  未被读取处触碰的可选字段。
