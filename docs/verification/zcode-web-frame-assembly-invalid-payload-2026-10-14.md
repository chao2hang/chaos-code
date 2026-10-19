# ZCode Web 会话面板 frameAssemblyInvalidPayload（2026-10-14）

## 症状

浏览器控制台（部署 bundle `index-BC4rWqfM.js`，2026-10-12 部署）在每次打开
会话时打出上一轮加的诊断：

```
[chaos-diag] assemblyFault {"deliveryKind":"initial",
  "reasonCode":"proto.frameAssemblyInvalidPayload",
  "logicalFrameOrdinal":1,"topic":"conversation/<uuid>",...}
[chaos-diag] requestRecovery {"recoveryEvent":true,"contentFault":true,...}
```

每条会话一条 fault，全部发生在 `deliveryKind:"initial"`、`ordinal:1`
（订阅锚点）。因为 `isDeterministicContentFault("proto.frameAssemblyInvalidPayload")`
为真，恢复阶梯跳过 same-sub resync 直接 forceSnapshot——重投的是同一份内容，
再次被拒，终态 `fault.subscription.contentRejected`。会话面板打不开。

## 定性

`proto.frameAssemblyInvalidPayload` 由 `packages/shared/src/zcode-protocol-v4/
wire-reassembly.ts` 产生：帧字节过了 length/checksum/UTF-8/JSON 四道关，但没过
`conversationTopicFrameSchema` 的 zod 校验。发送端是 Rust 桥
（`crates/codegen/xai-grok-web/src/chaos_bridge.rs` 的
`chaos_conversation_snapshot`），接收端合同在部署 bundle 的
`zcode-protocol-v4/snapshot.ts`（从 sourcemap 逐字节取出核对）。

## 取证方法

1. 从部署 map（`~/.chaos/web/zcode-assets/assets/*.js.map`）抽出全部
   `shared/src` 冻结合同源码，与 `/tmp/zcode-upstream/packages/shared/src`
   逐字节核对一致（transport.ts md5 671716db）。
2. 新增 `#[ignore]` 诊断测试（诊断后已删）把 `chaos_conversation_snapshot_from_engine`
   对真实会话库副本（`~/.chaos/web/sessions.sqlite` 拷贝）产出的锚点帧原样落盘。
3. 用部署合同 + zod 4.6.5 跑 `conversationTopicFrameSchema.safeParse`。

## 根因（真实会话 37dc0764 / a618460b，各 4 条 issue）

| # | 路径 | 桥发的 | 合同要求 |
|---|------|--------|----------|
| 1 | `availability.compact.reasonCode` | `reason`（键名错） | `reasonCode: z.string()`（判别联合，缺键整帧拒） |
| 2 | `availability.setFollowupMode.reasonCode` | `reason` | 同上 |
| 3 | `inputRouting.mode` | `"queue"` | 枚举 `startNow/enqueue/guide/reject/choice`，没有 `queue` |
| 4 | `queue.pauseReason` | `null` | `z.enum(["stopped","manual","error"]).optional()`：`null` 非法，只能缺席 |

另有两处未在初始帧出现、但同一类会炸的：

- `queue_updated` 引擎事件 → `state.updated{queue}` 补丁同样带
  `pauseReason: null`（运行中入队后必炸，与快照同病）；
- 空面板锚点（会话答不上来时）`control.lastError.source: "host"` ——
  官方词表是 `provider/runtime/tool/network`，没有 `host`。
- 队列项本体：引擎 `QueuedInput` 原样序列化只有 4 个字段，过不了 strict 的
  `conversationInputIntentSchema`（缺 `clientId/kind/delivery/order/steer/
  dispatch/admittedAt`，且 `.strict()` 拒绝多余键）。一旦有人运行中入队，
  下一条队列补丁就把整条 deltas 帧拒收。

## 修复（全部在发送端，接收端合同是冻结的）

`crates/codegen/xai-grok-web/src/chaos_bridge.rs`

- `availability` 两个 `allowed:false` 项改发 `reasonCode`；
- `inputRouting.mode`：`is_running → "enqueue"`，空闲 → `"startNow"`（官方词表
  没有 `queue`；路由词说的是引擎接下来怎么处置下一条输入）；
- `queue.pauseReason`：只在真暂停时出现，值为 `"stopped"`（引擎的暂停唯一
  来源是用户按停止）；新 helper `queue_state_wire` 供快照与 `queue_updated`
  共用，杜绝两处分叉；
- 新 helper `queue_item_wire`：引擎排队输入 → 完整官方 intent（`kind:
  "sendText"`、`delivery{requested,admitted:"queue"}`、`order{admissionSeq,
  queuePosition}`、`steer{state:"notRequested"}`、`dispatch{state:"queued"}`、
  `admittedAt`）；requested 词表外/旧值读作 `auto`，空 `sourceCommandId`
  兜底 uuid；
- 空面板锚点 `lastError.source` 改 `"runtime"`。

`crates/codegen/chaos-engine/src/lib.rs`

- `QueuedInput` 增 `requested_delivery` / `client_id`，均带 `#[serde(default)]`
  （旧持久化状态读作 `auto` / `unknown`，schema 兼容）；
- `ClientMessage::QueueInput` 增同名两字段，桥在入队时从命令信封取
  `requestedDelivery`（缺席 = `auto`）与 `clientId` 传入。

`crates/codegen/chaos-engine/tests/streaming_flow.rs`

- 该文件 6 例红与本修无关：2026-10-12 的队列合同给 `produce_run` 加了
  `TurnStarted` 回合开场公告，这批先于该公告写的测试没认得它。在
  `next_event` 读取点统一跳过 `TurnStarted`（文件内没有任何用例消费公告），
  11/11 回绿。

前端（`/tmp/zcode-upstream`）：只移除上一轮的 `CHAOS-INSTRUMENTATION`
（`[chaos-diag]` 两处 console.error，其注释本就写明「removed after
diagnosis」）并重新构建部署；`/ws/zcode` 连接修正与 E2E store 桥保留。

## 证据

1. 修复前：真实会话锚点帧 × 2 被 zod 拒，各 4 条 issue（本文件根因表即输出）。
2. 修复后：
   - 单元级：`/tmp/bridge-frame-*.json`（快照构造器原样输出）全部 `OK`；
   - 事件级：`queue_updated` → `state.updated` 增量帧、空面板锚点帧 `OK`；
   - 端到端：`cargo test -p xai-grok-web --test real_session_anchor_probe
     -- --ignored`（真实 sqlite 副本 → router → /ws/zcode 二进制帧 →
     JSON）`/tmp/e2e-anchor-*.json` 全部 `OK`；
   - Rust 合同回归：`queue_wire_contract_tests` 5 例（pauseReason 缺席/
     stopped、intent 全键、旧状态 serde 默认、QueueUpdated 映射）；
   - 全套：`cargo test -p chaos-engine`（258 lib + 11 streaming 等）与
     `cargo test -p xai-grok-web` 全绿。
3. 新 bundle `index-BbsfuIuk.js`：不含 `chaos-diag`，含 `/ws/zcode`，其
   `snapshot.ts` 与验证用合同逐字节一致；已 rsync 部署到
   `~/.chaos/web/zcode-assets/`，`chaos-zcode-web.service` 2026-10-14
   11:28 重启，`https://172.16.2.253:3082/` 200，`/api/auth/status` 正常。

## 后记：状态面板悬浮化（同日用户裁决）

会话面板修好后用户指出另一件事：右上角的「Git 工具」卡（ConversationStatusPanel）
虽然是 `absolute` 悬浮定位，但 `conversationLayout.ts` 让正文列在 ≥1280px 时
让出 `24rem` 并整体左移 `10.5rem`（且「无面板」档的宽度常量与「有面板」档
逐字相同，空会话也照让）——卡片实际上占着一整列宽度。

用户裁决：**面板应该悬浮，不占宽度**。`packages/ui/src/v4/conversationLayout.ts`
据此收敛：

- 正文列（消息列、headerSlot、pending guides、composer dock）在所有面板
  状态下统一为 `w-full @min-[864px]:max-w-4xl`，删除 ≥1280px 的
  `calc(100%_-_24rem)`/`max-w-6xl` 与 864–1280px 的 `calc(100%_-_6rem)`；
- `getConversationStatusPanelOffsetClassName` 改回 `undefined`（函数与签名
  保留，作为未来若恢复并排布局的接缝）；
- 面板开合不再引起内容列跳变；展开卡片盖在正文右缘，需要全宽读正文时
  收成 mini 胶囊。

新 bundle `index-BTDP1gYe.js`：`calc(100%_-_24rem)` 与 `translate-x-42`
清零，新宽度类在位；已部署并经 `https://172.16.2.253:3082/` 200 验证。
