# ZCode 后台子代理可见性

## 目标

当 Chaos Web 的 ZCode 会话启动、更新或完成子代理时，Composer 后台工作徽标和会话状态面板应显示对应子线程；刷新/重连时应从同一后端运行态快照恢复。完成提醒仍由现有 shell 通知机制负责，本改动只补可观察性，不改变通知/自动唤醒策略。

## 状态所有者与边界

- Agent runtime 是子代理生命周期事件的权威来源（`SubagentSpawned`、`SubagentProgress`、`SubagentFinished`）。
- Headless `streaming-json` 仅把这些事件序列化给调用它的 Chaos Engine；不把通知文案解析成状态。
- Chaos Engine 持有进程内、按父 session 隔离的运行态投影；会话运行态不持久化，进程重启后不存在运行中的旧子线程。
- ZCode bridge 将 `SubagentsUpdated` 映射到 V4 `state.updated.subagents`，快照从 Engine 查询相同投影；前端现有面板消费 `subagents.running`。

## 事件顺序

```text
TaskTool
  → Shell emits SubagentSpawned(parent, child, type, description, model, background)
  → headless streaming-json emits structured subagent line
  → HeadlessProcessAdapter emits TurnOutput::Subagent
  → Engine updates parent-session projection, then publishes SubagentsUpdated
  → ZCode bridge emits V4 state.updated / recovery snapshot
  → ZCode Composer badge + status panel render running child
  → Shell emits SubagentFinished; same path removes running child and refreshes panel
```

Lifecycle updates are idempotent by child session id and attempt identity where available. Workflow-owned children remain excluded from the standalone subagent directory projection; workflow projection is a separate contract. Ended child history must not be fabricated: only runtime-known lifecycle records are included. Progress refreshes are additive and may be omitted when no new display data exists.

## Failure and compatibility

- Existing `streaming-json` lines and provider response frames keep their current shapes; only additive `type: subagent` lines are introduced for structured lifecycle notifications.
- Existing `ServerMessage::Subagents` stays an addressed query response. A distinct session event broadcasts projection updates to current followers.
- Older frontends ignore unknown event types; V4 snapshot fields follow the already-deployed `subagents` schema.
- If notification metadata is incomplete, the UI may still show the child using stable IDs and conservative labels; it must not infer completion from a missing progress event.

## Acceptance

1. A spawn produces `subagents.running` with child session id, role/type, description and running status.
2. A finish removes that child from running and increments ended count/revision.
3. A fresh snapshot during a running child returns the same state as live updates.
4. Unrelated sessions never receive or include the child.
5. Unit tests exercise event order, deduplication and projection wire shapes.
