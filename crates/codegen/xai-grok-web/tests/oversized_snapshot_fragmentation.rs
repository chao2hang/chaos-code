//! 回归：超过 1MiB 物理上限的会话锚点必须按 V4 合同分片（`kind:"fragment"`），
//! 且分片能被官方客户端语义（crc32 + 按 fragmentIndex 重装）完整还原。
//!
//! 背景（2026-10-19，会话 21afaba3-432a-44db-8691-539334ab227a）：桥此前永远发
//! 单片 `complete`，471 行 / 约 1.5MiB 的锚点被客户端装配器整帧拒收
//!（`proto.frameEnvelopeTooLarge`），initial 与 recovery 锚点双双被拒，
//! 恢复阶梯 fail-closed，会话面板永远打不开（fault.subscription.recovery*）。
//! 修复：`src/wire_fragmentation.rs` 按
//! `frontend/zcode/docs/v4-refactor/05-transport-and-backpressure.md` 的生产者
//! 合同实现分片编码，本文件从真实 router 全链路锁定该行为。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use chaos_engine::{ClientMessage, Engine, PromptAdapter, RunOptions, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xai_grok_web::router;
use zcode_rpc_protocol::{RpcValue, read_message, rpc_type, write_frame, write_message};

/// 单条回复约 1.6MiB：与真实案例同量级，确保锚点必然越过 1MiB 物理上限。
const HUGE_REPLY_BYTES: usize = 1_600_000;
/// 客户端 `PROTOCOL_V4_LIMITS.maxFrameBytes`。
const MAX_PHYSICAL_FRAME_BYTES: usize = 1024 * 1024;

struct HugeReplyAdapter;

impl PromptAdapter for HugeReplyAdapter {
    fn run_prompt(&self, prompt: &str, _options: &RunOptions) -> Result<Vec<String>, String> {
        Ok(vec![format!(
            "huge reply for {prompt}: {}",
            "x".repeat(HUGE_REPLY_BYTES)
        )])
    }
}

fn inner(bytes: &[u8]) -> Vec<u8> {
    let len = u32::from_be_bytes(bytes[9..13].try_into().unwrap()) as usize;
    bytes[13..13 + len].to_vec()
}

/// 分片归组表：route(topic\0sub\0ordinal) -> (fragmentCount, logicalBytes, checksum, 片)。
type PendingFragments = HashMap<String, (u64, u64, String, HashMap<u64, String>)>;

/// 与客户端装配器同口径：fragment 按 (topic, subId, ordinal) 归组，齐片重装；
/// complete 帧原样返回（包一层与 fragment 相同的「wire 视图」）。
/// 返回 (logical_frame_view, 本条 logical frame 实际占用的物理帧数)。
fn accept_wire(wire: &Value, pending: &mut PendingFragments) -> Option<(Value, usize)> {
    if wire["kind"].as_str() == Some("fragment") {
        let route = format!(
            "{}\u{0}{}\u{0}{}",
            wire["topic"].as_str().unwrap_or("?"),
            wire["subscriptionId"].as_str().unwrap_or("?"),
            wire["logicalFrameOrdinal"],
        );
        let entry = pending.entry(route).or_insert_with(|| {
            (
                wire["fragmentCount"].as_u64().unwrap_or(0),
                wire["logicalBytes"].as_u64().unwrap_or(0),
                wire["checksum"]["value"].as_str().unwrap_or("").to_owned(),
                HashMap::new(),
            )
        });
        entry.3.insert(
            wire["fragmentIndex"].as_u64().unwrap_or(0),
            wire["dataBase64"].as_str().unwrap_or("").to_owned(),
        );
        if entry.3.len() as u64 != entry.0 {
            return None;
        }
        let mut logical: Vec<u8> = Vec::new();
        for index in 0..entry.0 {
            let data = entry.3.get(&index).expect("collected fragment index");
            logical.extend(
                base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .expect("canonical base64"),
            );
        }
        assert_eq!(logical.len() as u64, entry.1, "logicalBytes 必须与分片一致");
        assert_eq!(
            format!("{:08x}", crc32fast::hash(&logical)),
            entry.2,
            "重装后的 crc32 必须与分片头一致"
        );
        let logical_frame: Value = serde_json::from_slice(&logical).expect("utf8 json");
        // 给测试一个统一的 wire 视图：分片重装后的 logical frame 挂回 frame 位。
        Some((
            json!({
                "wireVersion": wire["wireVersion"],
                "kind": "complete",
                "deliveryKind": wire["deliveryKind"],
                "logicalFrameId": wire["logicalFrameId"],
                "logicalFrameOrdinal": wire["logicalFrameOrdinal"],
                "topic": wire["topic"],
                "subscriptionId": wire["subscriptionId"],
                "frame": logical_frame,
            }),
            entry.0 as usize,
        ))
    } else {
        Some((wire.clone(), 1))
    }
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// 读下一条会话主题 logical frame（fragment 透明重装），附其物理帧数。
async fn next_conversation_logical_frame(
    socket: &mut Socket,
    pending: &mut PendingFragments,
) -> (Value, usize) {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(15), socket.next())
            .await
            .expect("帧 15s 内到达")
            .expect("socket alive")
            .expect("帧 ok");
        let Message::Binary(bytes) = message else {
            continue;
        };
        let (header, body) = match read_message(inner(bytes.as_ref()).as_slice()) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        let RpcValue::Json(Value::Array(items)) = header else {
            continue;
        };
        if items.first().and_then(Value::as_u64) != Some(u64::from(rpc_type::EVENT_FIRE)) {
            continue;
        }
        let RpcValue::Json(value) = body else {
            continue;
        };
        let is_conversation = value["frame"]["topic"]
            .as_str()
            .is_some_and(|topic| topic.starts_with("conversation/"))
            || value["kind"].as_str() == Some("fragment")
                && value["topic"]
                    .as_str()
                    .is_some_and(|topic| topic.starts_with("conversation/"));
        if !is_conversation {
            continue;
        }
        if let Some(accepted) = accept_wire(&value, pending) {
            return accepted;
        }
    }
}

async fn call(socket: &mut Socket, id: u32, channel: &str, method: &str, arg: Value) -> Value {
    let mut frame = zcode_rpc_protocol::serialize_chunk(&RpcValue::Json(json!([
        rpc_type::PROMISE,
        id,
        channel,
        method
    ])));
    frame.extend_from_slice(&zcode_rpc_protocol::serialize_chunk(&RpcValue::Json(arg)));
    socket
        .send(Message::Binary(write_frame(&frame).into()))
        .await
        .unwrap();
    loop {
        let message = tokio::time::timeout(Duration::from_secs(15), socket.next())
            .await
            .expect("应答 15s 内到达")
            .expect("socket alive")
            .expect("帧 ok");
        let Message::Binary(bytes) = message else {
            continue;
        };
        let (header, body) = read_message(inner(bytes.as_ref()).as_slice()).expect("rpc frame");
        let RpcValue::Json(Value::Array(items)) = header else {
            continue;
        };
        if items.first().and_then(Value::as_u64) != Some(u64::from(rpc_type::PROMISE_SUCCESS)) {
            continue;
        }
        let RpcValue::Json(value) = body else {
            continue;
        };
        return value;
    }
}

async fn listen(socket: &mut Socket, id: u32, channel: &str, method: &str) {
    let frame = write_message(
        &RpcValue::Json(json!([rpc_type::EVENT_LISTEN, id, channel, method])),
        &RpcValue::Undefined,
    );
    socket
        .send(Message::Binary(write_frame(&frame).into()))
        .await
        .unwrap();
}

#[tokio::test]
async fn an_oversized_anchor_arrives_as_contract_fragments_and_reassembles() {
    // 1. 合成一个单回复 1.6MiB 的会话（sqlite 持久化，走正式引擎）。
    let directory = tempfile::tempdir().unwrap();
    let store = directory.path().join("sessions.sqlite");
    let session_id = {
        let engine =
            Engine::with_sqlite_store_and_adapter(&store, Some(Arc::new(HugeReplyAdapter)))
                .expect("engine from temp store");
        let created = engine.handle(ClientMessage::CreateSession {
            workspace_id: None,
            client_msg_id: "create".into(),
        });
        let session_id = match &created[0] {
            ServerMessage::SessionCreated { session_id, .. } => *session_id,
            other => panic!("unexpected {other:?}"),
        };
        engine.handle(ClientMessage::Submit {
            client_msg_id: "submit".into(),
            session_id,
            prompt: "produce huge tool output".into(),
        });
        session_id.to_string()
    };

    // 2. 正式 router + 官方启动序：会话主题监听先于一切注册。
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let engine = Engine::with_sqlite_store(&store).unwrap();
        let _ = axum::serve(listener, router(engine, ""))
            .with_graceful_shutdown(async { tokio::time::sleep(Duration::from_secs(120)).await })
            .await;
    });
    let (mut socket, _) = connect_async(format!("ws://{address}/ws/zcode"))
        .await
        .unwrap();
    listen(
        &mut socket,
        900,
        "zcode-agent",
        "onDynamicConversationFrame",
    )
    .await;

    // 3. 订阅会话主题：chaos 桥总以 snapshot 应答。
    let ack = call(
        &mut socket,
        901,
        "zcode-agent",
        "subscribeConversationV4",
        json!({ "sessionId": session_id }),
    )
    .await;
    let subscription_id = ack["ack"]["subscriptionId"]
        .as_str()
        .expect("ack subId")
        .to_owned();
    assert_eq!(ack["ack"]["mode"], "snapshot");

    // 4. 初始锚点：必须以多个 fragment 到达，每片独立通过 1MiB 物理上限。
    let mut pending: PendingFragments = HashMap::new();
    let (anchor, initial_physical_frames) = loop {
        let (logical, physical) = next_conversation_logical_frame(&mut socket, &mut pending).await;
        if logical["frame"]["payload"]["kind"] == "snapshot" {
            break (logical, physical);
        }
    };
    assert!(
        initial_physical_frames > 1,
        "锚点超过 1MiB 物理上限，必须分片（实际 {initial_physical_frames} 片）"
    );
    assert_eq!(anchor["deliveryKind"], "initial", "锚点以 initial 投递");
    assert_eq!(anchor["subscriptionId"], subscription_id);
    let payload = &anchor["frame"]["payload"];
    assert_eq!(payload["kind"], "snapshot");
    let logical_bytes = serde_json::to_vec(&anchor["frame"]).unwrap().len();
    assert!(
        logical_bytes > MAX_PHYSICAL_FRAME_BYTES,
        "合成会话的锚点必须真的超过 1MiB（实际 {logical_bytes}）"
    );
    let window = payload["snapshot"]["rows"]["window"]
        .as_array()
        .expect("rows");
    // 存档还原路径的 assistant 行词表：kind=assistantText + text（见 chaos_rows_from_stored）。
    assert!(
        window.iter().any(|row| row["kind"] == "assistantText"
            && row["text"]
                .as_str()
                .is_some_and(|text| text.len() > HUGE_REPLY_BYTES)),
        "锚点必须携带那行 1.6MiB 的完整回复"
    );

    // 5. 客户端恢复阶梯的 resync：recovery 锚点同样分片、重装后完整。
    let resync_ack = call(
        &mut socket,
        902,
        "zcode-agent",
        "resyncConversationV4",
        json!({ "sessionId": session_id, "subscriptionId": subscription_id, "forceSnapshot": true }),
    )
    .await;
    assert_eq!(resync_ack["ack"]["mode"], "snapshot");
    let (recovery_anchor, recovery_physical_frames) = loop {
        let (logical, physical) = next_conversation_logical_frame(&mut socket, &mut pending).await;
        if logical["frame"]["payload"]["kind"] == "snapshot" {
            break (logical, physical);
        }
    };
    assert!(recovery_physical_frames > 1, "recovery 锚点同样必须分片");
    assert_eq!(recovery_anchor["deliveryKind"], "recovery");
    assert_eq!(
        recovery_anchor["logicalFrameOrdinal"].as_u64(),
        anchor["logicalFrameOrdinal"]
            .as_u64()
            .map(|ordinal| ordinal + 1),
        "恢复锚点的 ordinal 严格递增"
    );
    assert_eq!(
        recovery_anchor["frame"]["payload"]["snapshot"]["rows"]["window"]
            .as_array()
            .expect("rows")
            .len(),
        window.len(),
        "恢复锚点与初始锚点行数一致"
    );
}
