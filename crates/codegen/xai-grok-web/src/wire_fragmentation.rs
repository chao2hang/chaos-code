//! V4 topic wire 的物理分片编码器（桥侧生产者实现）。
//!
//! 合同：`frontend/zcode/docs/v4-refactor/05-transport-and-backpressure.md`（物理分片）与
//! `04-sync-and-recovery.md`（deliveryKind 权威标记）。shared 侧参考实现是
//! `packages/shared/src/zcode-protocol-v4/wire-codec.ts` 的 `encodeTopicWireFrames`；
//! 本模块逐条镜像其计量与 fail-closed 行为，两边必须同步演化。
//!
//! 为什么必须有它：装配器对**每个** wire 帧（含 complete）计量
//! `measureTopicNotificationEnvelopeBytes`，超过 `PROTOCOL_V4_LIMITS.maxFrameBytes`
//!（1 MiB）整帧拒收并报 `proto.frameEnvelopeTooLarge`。桥此前永远发单片
//! complete，任何快照越过 1 MiB（真实案例：会话 21afaba3-432a-44db-8691-539334ab227a，
//! 471 行锚点约 1.5 MiB）就永不可装配——initial 与 recovery 锚点全部被拒，
//! 恢复阶梯 fail-closed，会话面板永远打不开（fault.subscription.recovery*）。

use base64::Engine as _;
use serde_json::{Value, json};

/// `PROTOCOL_V4_LIMITS.maxFrameBytes`：单个 wire 帧（fragment 或 complete）的
/// 信封字节上限。客户端按三层承载（CLI NDJSON / channel socket / mobile relay）
/// 的最大值计量，见下方 `measure_max_envelope_bytes`。
const MAX_PHYSICAL_FRAME_BYTES: usize = 1024 * 1024;
/// `PROTOCOL_V4_LIMITS.logicalFrameAssemblyMaxBytes`：一个 logical frame 的字节上限。
const MAX_ASSEMBLY_BYTES: usize = 16 * 1024 * 1024;
/// `PROTOCOL_V4_LIMITS.logicalFrameAssemblyMaxFragments`。
const MAX_FRAGMENTS: usize = 1024;
/// `wire-codec.ts` 的常量：EventFire(204) RPC header 在最坏 id 宽度下的字节数。
/// 推导：1(tag) + vql(2) + 1(204 vql) + [1 + vql(16) + 16](MAX_SAFE_INTEGER id) = 22。
const CHANNEL_HEADER_BYTES: usize = 22;
/// `wire-codec.ts`：SocketProtocol/PersistentProtocol 的 13 字节传输头。
const SOCKET_PROTOCOL_HEADER_BYTES: usize = 13;

/// 编码失败的原因码，与 shared 的 `TopicWireFrameEncodingError` 词表一致。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicWireEncodeError(pub &'static str);

fn vql_byte_length(value: usize) -> usize {
    // TS: for (let remaining = value >>> 7; remaining > 0; remaining >>>= 7) bytes += 1;
    let mut bytes = 1usize;
    let mut remaining = value >> 7;
    while remaining > 0 {
        bytes += 1;
        remaining >>= 7;
    }
    bytes
}

/// 与 `JSON.stringify` + `TextEncoder` 一致的 UTF-8 序列化字节数。
/// serde_json 默认不转义非 ASCII，输出即 UTF-8 原文，`String::len()` 就是字节数。
fn utf8_json_byte_length(value: &Value) -> usize {
    serde_json::to_string(value)
        .map(|text| text.len())
        .unwrap_or(usize::MAX)
}

fn mobile_relay_fixed_bytes() -> usize {
    // 与 wire-codec.ts 逐字段一致的最坏外层信封；只计量，不上线。
    let transport_id = "x".repeat(256);
    utf8_json_byte_length(&json!({
        "type": "data",
        "payload": {
            "zcode_type": "rpc-frame",
            "bridgeSessionId": transport_id,
            "bridgeGeneration": 9_007_199_254_740_991u64,
            "recoveryId": transport_id,
            "seq": 9_007_199_254_740_991u64,
            "dataBase64": "",
        },
        "client_ts": 9_007_199_254_740_991u64,
        "server_ts": 9_007_199_254_740_991u64,
    }))
}

/// 按客户端 `measureTopicNotificationEnvelopeBytes` 的口径计量一个 wire 帧：
/// 取 CLI NDJSON / channel socket / mobile relay 三层承载的最大值。
fn measure_max_envelope_bytes(wire: &Value) -> usize {
    let wire_json_bytes = utf8_json_byte_length(wire);
    if wire_json_bytes == usize::MAX {
        return usize::MAX;
    }
    let channel_payload =
        CHANNEL_HEADER_BYTES + 1 + vql_byte_length(wire_json_bytes) + wire_json_bytes;
    let cli_ndjson_bytes =
        utf8_json_byte_length(&json!({"method": "v4/conversation/frame", "params": wire})) + 1;
    let channel_socket_bytes = channel_payload + SOCKET_PROTOCOL_HEADER_BYTES;
    // mobile relay 把 channel binary 再 base64，膨胀 4/3。
    let mobile_relay_bytes = mobile_relay_fixed_bytes() + 4 * channel_payload.div_ceil(3);
    cli_ndjson_bytes
        .max(channel_socket_bytes)
        .max(mobile_relay_bytes)
}

fn complete_wire(
    delivery_kind: &str,
    logical_frame_id: &str,
    logical_frame_ordinal: u64,
    topic: &str,
    subscription_id: &str,
    frame: &Value,
) -> Value {
    json!({
        "wireVersion": 3,
        "kind": "complete",
        "logicalFrameId": logical_frame_id,
        "logicalFrameOrdinal": logical_frame_ordinal,
        // deliveryKind is REQUIRED on every wire frame.
        "deliveryKind": delivery_kind,
        "topic": topic,
        "subscriptionId": subscription_id,
        "frame": frame,
    })
}

#[allow(clippy::too_many_arguments)]
fn fragment_wire(
    delivery_kind: &str,
    logical_frame_id: &str,
    logical_frame_ordinal: u64,
    topic: &str,
    subscription_id: &str,
    fragment_index: usize,
    fragment_count: usize,
    logical_bytes: usize,
    checksum: &str,
    data_base64: &str,
) -> Value {
    json!({
        "wireVersion": 3,
        "kind": "fragment",
        "logicalFrameId": logical_frame_id,
        "logicalFrameOrdinal": logical_frame_ordinal,
        "deliveryKind": delivery_kind,
        "topic": topic,
        "subscriptionId": subscription_id,
        "fragmentIndex": fragment_index,
        "fragmentCount": fragment_count,
        "logicalBytes": logical_bytes,
        "checksum": { "algorithm": "crc32", "value": checksum },
        "dataBase64": data_base64,
    })
}

/// 二分搜索最大的分片原始字节数，使「最坏形态」的 fragment wire 仍在物理上限内。
/// 与 `wire-codec.ts` 的 `findFragmentByteBudget` 一致：计量用最坏 index 位数与
/// 等长 base64 占位，真实 fragment 的信封只会更小。
#[allow(clippy::too_many_arguments)]
fn find_fragment_byte_budget(
    logical_bytes: usize,
    delivery_kind: &str,
    logical_frame_id: &str,
    logical_frame_ordinal: u64,
    topic: &str,
    subscription_id: &str,
    checksum: &str,
) -> usize {
    let mut low = 1usize;
    let mut high = logical_bytes.min(MAX_PHYSICAL_FRAME_BYTES);
    let mut best = 0usize;
    let worst_count = logical_bytes;
    while low <= high {
        let candidate = (low + high) / 2;
        let probe = fragment_wire(
            delivery_kind,
            logical_frame_id,
            logical_frame_ordinal,
            topic,
            subscription_id,
            worst_count.saturating_sub(1),
            worst_count,
            logical_bytes,
            checksum,
            &"A".repeat(4 * candidate.div_ceil(3)),
        );
        if measure_max_envelope_bytes(&probe) <= MAX_PHYSICAL_FRAME_BYTES {
            best = candidate;
            low = candidate + 1;
        } else if candidate == 0 {
            break;
        } else {
            high = candidate - 1;
        }
    }
    best
}

/// 把一个 logical topic frame 编码为一或多个物理 wire 帧。
///
/// - 信封不超限时保持单片 `complete`（线上既有形态，兼容既有测试与抓包）；
/// - 超限时在 UTF-8 byte 层切成 `fragment`：全部 fragment 共享同一
///   `logicalFrameId/logicalFrameOrdinal/topic/subscriptionId/fragmentCount/
///   logicalBytes/checksum`，`dataBase64` 是 canonical logical bytes 的当前分片，
///   客户端在 assembler 里按序重装、crc32 校验后整体 apply（`wire-assembler.ts`）。
pub fn encode_topic_wire_frames(
    frame: &Value,
    delivery_kind: &str,
    topic: &str,
    subscription_id: &str,
    logical_frame_ordinal: u64,
) -> Result<Vec<Value>, TopicWireEncodeError> {
    let logical_frame_id = format!("chaos-{}", uuid::Uuid::new_v4());
    let logical = serde_json::to_vec(frame)
        .map_err(|_| TopicWireEncodeError("proto.frameAssemblyInvalidJson"))?;
    if logical.len() > MAX_ASSEMBLY_BYTES {
        return Err(TopicWireEncodeError("proto.frameAssemblyTooLarge"));
    }

    let complete = complete_wire(
        delivery_kind,
        &logical_frame_id,
        logical_frame_ordinal,
        topic,
        subscription_id,
        frame,
    );
    if measure_max_envelope_bytes(&complete) <= MAX_PHYSICAL_FRAME_BYTES {
        return Ok(vec![complete]);
    }

    // crc32 与客户端 `crc32WireBytes` 同多项式（IEEE，zlib 口径），crc32fast 即该实现。
    let checksum = format!("{:08x}", crc32fast::hash(&logical));
    let chunk_bytes = find_fragment_byte_budget(
        logical.len(),
        delivery_kind,
        &logical_frame_id,
        logical_frame_ordinal,
        topic,
        subscription_id,
        &checksum,
    );
    if chunk_bytes < 1 {
        return Err(TopicWireEncodeError("proto.frameEnvelopeTooLarge"));
    }
    let fragment_count = logical.len().div_ceil(chunk_bytes);
    if fragment_count > MAX_FRAGMENTS {
        return Err(TopicWireEncodeError("proto.frameFragmentCountExceeded"));
    }

    let mut frames = Vec::with_capacity(fragment_count);
    for fragment_index in 0..fragment_count {
        let start = fragment_index * chunk_bytes;
        let end = (start + chunk_bytes).min(logical.len());
        let data_base64 = base64::engine::general_purpose::STANDARD.encode(&logical[start..end]);
        let wire = fragment_wire(
            delivery_kind,
            &logical_frame_id,
            logical_frame_ordinal,
            topic,
            subscription_id,
            fragment_index,
            fragment_count,
            logical.len(),
            &checksum,
            &data_base64,
        );
        // 物理上限包含真实外层信封；任何估算漂移都必须 fail closed，
        // 不能把超限帧交给下游再被装配器拒掉（那正是本 bug 的成因）。
        if measure_max_envelope_bytes(&wire) > MAX_PHYSICAL_FRAME_BYTES {
            return Err(TopicWireEncodeError("proto.frameEnvelopeTooLarge"));
        }
        frames.push(wire);
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn topic_frame(payload_bytes: usize) -> Value {
        // 一个合法形状的 conversation topic frame，rows 塞到指定字节量级。
        let filler = "x".repeat(payload_bytes);
        json!({
            "topic": "conversation/21afaba3-432a-44db-8691-539334ab227a",
            "subscriptionId": "chaos-test",
            "fromSeq": 0,
            "toSeq": 1,
            "sentAt": 1,
            "payload": { "kind": "deltas", "deltas": [{ "op": "row.delta", "rowId": 1, "text": filler }] },
        })
    }

    #[test]
    fn crc32_matches_the_frozen_wire_checksum_vector() {
        // zlib CRC-32 的公开 check 值：crc32(b"123456789") == 0xcbf43926。
        assert_eq!(format!("{:08x}", crc32fast::hash(b"123456789")), "cbf43926");
    }

    #[test]
    fn small_frames_stay_single_complete() {
        let frame = topic_frame(64);
        let wires = encode_topic_wire_frames(&frame, "online", "conversation/s", "chaos-test", 1)
            .expect("encode");
        assert_eq!(wires.len(), 1);
        assert_eq!(wires[0]["kind"], "complete");
        assert_eq!(wires[0]["deliveryKind"], "online");
    }

    #[test]
    fn oversized_frames_fragment_within_the_physical_cap() {
        let frame = topic_frame(1_600_000);
        let wires = encode_topic_wire_frames(&frame, "recovery", "conversation/s", "chaos-test", 2)
            .expect("encode");
        assert!(wires.len() > 1, "必须分片");
        let count = wires[0]["fragmentCount"].as_u64().unwrap();
        assert_eq!(count, wires.len() as u64);
        let checksum = wires[0]["checksum"]["value"].as_str().unwrap().to_owned();
        assert_eq!(checksum.len(), 8);

        let mut logical: Vec<u8> = Vec::new();
        for (index, wire) in wires.iter().enumerate() {
            assert_eq!(wire["kind"], "fragment");
            assert_eq!(wire["fragmentIndex"], index);
            assert_eq!(wire["fragmentCount"], count);
            assert_eq!(wire["deliveryKind"], "recovery");
            // 每片必须独立通过物理上限计量（客户端装配器逐帧检查的就是它）。
            assert!(measure_max_envelope_bytes(wire) <= MAX_PHYSICAL_FRAME_BYTES);
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(wire["dataBase64"].as_str().unwrap())
                .expect("canonical base64");
            logical.extend_from_slice(&decoded);
        }
        assert_eq!(
            logical.len(),
            wires[0]["logicalBytes"].as_u64().unwrap() as usize
        );
        assert_eq!(format!("{:08x}", crc32fast::hash(&logical)), checksum);
        let rebuilt: Value = serde_json::from_slice(&logical).expect("utf8 json");
        assert_eq!(rebuilt, frame, "重装结果必须逐字节还原 logical frame");
    }

    #[test]
    fn fragment_budget_never_produces_a_single_giant_fragment() {
        // 极端：logical 恰好等于 assembly 上界，也应切出 ≤1MiB 的片并在 1024 片内。
        let frame = topic_frame(MAX_ASSEMBLY_BYTES - 4096);
        let wires = encode_topic_wire_frames(&frame, "initial", "conversation/s", "chaos-test", 1)
            .expect("encode");
        assert!(wires.len() <= MAX_FRAGMENTS);
        for wire in &wires {
            assert!(measure_max_envelope_bytes(wire) <= MAX_PHYSICAL_FRAME_BYTES);
        }
    }
}
