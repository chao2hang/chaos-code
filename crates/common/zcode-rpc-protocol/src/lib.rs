//! Wire-format interop core for the `zai-org/ZCode` `@zcode/rpc` binary protocol.
//!
//! This crate is a clean-room Rust implementation of the protocol **shape** that
//! the official ZCode frontend (`packages/ui` + `packages/web`) speaks over its
//! `/ws` WebSocket, so the Chaos engine can terminate those connections natively
//! without a Node.js runtime. It implements only the transport format:
//!
//! - VQL (variable-length quantity) integers, 7 bits per byte, LSB-first,
//!   continuation bit in the high bit (`packages/rpc/src/serialization.ts`);
//! - the typed-chunk framing `[1 byte tag][VQL length][payload]` with the
//!   upstream data-type tags `Undefined=0, String=1, Buffer=2, VSBuffer=3,
//!   Array=4, Object=5 (JSON fallback), Int=6`;
//! - the RPC request/response event tags carried inside the header chunk
//!   (`Promise=100, PromiseCancel=101, EventListen=102, EventDispose=103` /
//!   `Initialize=200, PromiseSuccess=201, PromiseError=202, PromiseErrorObj=203,
//!   EventFire=204`).
//!
//! The upstream TypeScript source is Apache-2.0; this file reimplements the
//! format from its documented behaviour and is verified against frames produced
//! by the real serializer (see `tests/fixtures/frames.json`, generated with
//! `packages/rpc/src/serialization.ts` itself). No upstream business logic is
//! copied.
//!
//! One documented upstream asymmetry is mirrored on purpose: JS `serialize`
//! accepts any `(data | 0) === data` number as an `Int` chunk, and negative
//! values are encoded via the unsigned `>>>` shift, so `-1` round-trips through
//! the official `deserialize` as `4294967295`. Byte compatibility with the real
//! peer matters more than fixing that here, so the same encoding is used and
//! negative values decode to their unsigned 32-bit bit pattern.

use serde_json::Value;

pub mod framing;
pub mod server;

pub use framing::{ChunkStream, HEADER_SIZE, MESSAGE_TYPE_REGULAR, write_frame};
pub use server::{ChannelServer, DEFAULT_TIMEOUT, Incoming, Outbound};

/// Data-type tag that prefixes every serialized chunk.
pub mod tag {
    pub const UNDEFINED: u8 = 0;
    pub const STRING: u8 = 1;
    pub const BUFFER: u8 = 2;
    pub const VSBUFFER: u8 = 3;
    pub const ARRAY: u8 = 4;
    pub const OBJECT: u8 = 5;
    pub const INT: u8 = 6;
}

/// RPC message types carried as `header[0]` by the official client and server.
pub mod rpc_type {
    pub const PROMISE: u32 = 100;
    pub const PROMISE_CANCEL: u32 = 101;
    pub const EVENT_LISTEN: u32 = 102;
    pub const EVENT_DISPOSE: u32 = 103;
    pub const INITIALIZE: u32 = 200;
    pub const PROMISE_SUCCESS: u32 = 201;
    pub const PROMISE_ERROR: u32 = 202;
    pub const PROMISE_ERROR_OBJ: u32 = 203;
    pub const EVENT_FIRE: u32 = 204;
}

/// JSON key marker the official serializer uses to smuggle a nested
/// `Uint8Array` through the Object (JSON) branch.
pub const NESTED_UINT8_ARRAY_MARKER: &str = "__zcode_rpc_nested_uint8array_v1";
/// JSON key holding the base64 payload next to [`NESTED_UINT8_ARRAY_MARKER`].
pub const NESTED_UINT8_ARRAY_BASE64_KEY: &str = "base64";

/// A value at an RPC chunk boundary.
///
/// The official protocol distinguishes `undefined` (its own chunk tag) from
/// JSON `null` (emitted inside the Object branch), so this enum keeps them
/// apart instead of collapsing both into `serde_json::Value::Null`.
#[derive(Debug, Clone, PartialEq)]
pub enum RpcValue {
    /// Chunk tag `0` — the JS `undefined` value.
    Undefined,
    /// Any JSON-representable value (string, number, bool, null, array, object).
    Json(Value),
    /// Top-level binary chunk: upstream `Uint8Array` (tag `2`) or `VSBuffer`
    /// (tag `3`). Both are length-prefixed raw bytes on the wire.
    Binary(Vec<u8>, BinaryKind),
}

/// Which binary chunk tag a [`RpcValue::Binary`] was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryKind {
    /// Tag `2` — the upstream `Buffer`/`Uint8Array` branch.
    Buffer,
    /// Tag `3` — the upstream `VSBuffer` branch.
    VsBuffer,
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("unexpected end of RPC frame at byte {0}")]
    UnexpectedEof(usize),
    #[error("VQL integer longer than 5 bytes at byte {0}")]
    VqlTooLong(usize),
    #[error("unknown RPC data-type tag {0:#04x} at byte {1}")]
    UnknownTag(u8, usize),
    #[error("chunk payload is not valid UTF-8 at byte {0}")]
    InvalidUtf8(usize),
    #[error("chunk JSON is not valid: {0}")]
    InvalidJson(#[source] serde_json::Error),
    #[error("frame has {0} trailing bytes after the parsed value")]
    TrailingBytes(usize),
    #[error("malformed RPC header: {0}")]
    MalformedHeader(&'static str),
    #[error("malformed RPC header: {0}")]
    MalformedHeaderBox(String),
}

/// Read one VQL unsigned integer, mirroring `readIntVQL` upstream.
pub fn read_vql(buf: &[u8], pos: &mut usize) -> Result<u32, ProtocolError> {
    let mut value: u32 = 0;
    let mut shift: u32 = 0;
    loop {
        let byte = *buf.get(*pos).ok_or(ProtocolError::UnexpectedEof(*pos))?;
        *pos += 1;
        if shift >= 32 {
            return Err(ProtocolError::VqlTooLong(*pos));
        }
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
    }
}

/// Write one VQL unsigned integer, mirroring `writeInt32VQL` upstream
/// (including its unsigned `>>>` treatment of negative int32 bit patterns).
pub fn write_vql(out: &mut Vec<u8>, value: u32) {
    if value == 0 {
        out.push(0);
        return;
    }
    let mut rest = value;
    while rest != 0 {
        let mut byte = (rest & 0x7f) as u8;
        rest >>= 7;
        if rest > 0 {
            byte |= 0x80;
        }
        out.push(byte);
    }
}

fn is_js_int32(n: &serde_json::Number) -> bool {
    if let Some(i) = n.as_i64() {
        return (i32::MIN as i64..=i32::MAX as i64).contains(&i);
    }
    // JS numbers are all doubles: `1.0` satisfies `(1.0 | 0) === 1.0`.
    match n.as_f64() {
        Some(f) => f.fract() == 0.0 && f >= i32::MIN as f64 && f <= i32::MAX as f64,
        None => false,
    }
}

/// Serialize one chunk, mirroring the upstream `serialize` dispatch order.
pub fn write_chunk(out: &mut Vec<u8>, value: &RpcValue) {
    match value {
        RpcValue::Undefined => out.push(tag::UNDEFINED),
        RpcValue::Binary(bytes, kind) => {
            out.push(match kind {
                BinaryKind::Buffer => tag::BUFFER,
                BinaryKind::VsBuffer => tag::VSBUFFER,
            });
            write_vql(out, bytes.len() as u32);
            out.extend_from_slice(bytes);
        }
        RpcValue::Json(Value::String(s)) => {
            out.push(tag::STRING);
            write_vql(out, s.len() as u32);
            out.extend_from_slice(s.as_bytes());
        }
        RpcValue::Json(Value::Array(items)) => {
            out.push(tag::ARRAY);
            write_vql(out, items.len() as u32);
            for item in items {
                write_chunk(out, &RpcValue::Json(item.clone()));
            }
        }
        RpcValue::Json(Value::Number(n)) if is_js_int32(n) => {
            out.push(tag::INT);
            let raw = n
                .as_i64()
                .map(|i| i as i32 as u32)
                .unwrap_or_else(|| n.as_f64().expect("int32 f64 checked") as i32 as u32);
            write_vql(out, raw);
        }
        RpcValue::Json(other) => {
            // Object branch: JSON fallback, preserving key insertion order
            // (`preserve_order`) so bytes match `JSON.stringify` output.
            let text =
                serde_json::to_string(other).expect("serde_json Value is always serializable");
            out.push(tag::OBJECT);
            write_vql(out, text.len() as u32);
            out.extend_from_slice(text.as_bytes());
        }
    }
}

/// Serialize one chunk into a fresh buffer.
pub fn serialize_chunk(value: &RpcValue) -> Vec<u8> {
    let mut out = Vec::new();
    write_chunk(&mut out, value);
    out
}

/// Read one chunk from `buf` starting at `pos`, mirroring upstream `deserialize`.
pub fn read_chunk(buf: &[u8], pos: &mut usize) -> Result<RpcValue, ProtocolError> {
    let tag_pos = *pos;
    let kind = *buf
        .get(tag_pos)
        .ok_or(ProtocolError::UnexpectedEof(tag_pos))?;
    *pos += 1;
    match kind {
        tag::UNDEFINED => Ok(RpcValue::Undefined),
        tag::STRING => {
            let len = read_vql(buf, pos)? as usize;
            let end = *pos + len;
            let bytes = buf
                .get(*pos..end)
                .ok_or(ProtocolError::UnexpectedEof(*pos))?;
            *pos = end;
            let text = std::str::from_utf8(bytes).map_err(|_| ProtocolError::InvalidUtf8(*pos))?;
            Ok(RpcValue::Json(Value::String(text.to_owned())))
        }
        tag::BUFFER | tag::VSBUFFER => {
            let kind = if kind == tag::BUFFER {
                BinaryKind::Buffer
            } else {
                BinaryKind::VsBuffer
            };
            let len = read_vql(buf, pos)? as usize;
            let end = *pos + len;
            let bytes = buf
                .get(*pos..end)
                .ok_or(ProtocolError::UnexpectedEof(*pos))?;
            *pos = end;
            Ok(RpcValue::Binary(bytes.to_vec(), kind))
        }
        tag::ARRAY => {
            let len = read_vql(buf, pos)? as usize;
            let mut items = Vec::with_capacity(len.min(1024));
            for _ in 0..len {
                items.push(read_chunk(buf, pos)?.into_json_else_undefined());
            }
            Ok(RpcValue::Json(Value::Array(items)))
        }
        tag::OBJECT => {
            let len = read_vql(buf, pos)? as usize;
            let end = *pos + len;
            let bytes = buf
                .get(*pos..end)
                .ok_or(ProtocolError::UnexpectedEof(*pos))?;
            *pos = end;
            let text = std::str::from_utf8(bytes).map_err(|_| ProtocolError::InvalidUtf8(*pos))?;
            let value = serde_json::from_str::<Value>(text).map_err(ProtocolError::InvalidJson)?;
            // The official deserializer rewrites nested-buffer marker objects
            // back into Uint8Array instances. This core keeps them verbatim so
            // no JSON information is lost; the bridge layer converts them when
            // a service actually needs the bytes.
            Ok(RpcValue::Json(value))
        }
        tag::INT => {
            let value = read_vql(buf, pos)?;
            Ok(RpcValue::Json(Value::Number(value.into())))
        }
        other => Err(ProtocolError::UnknownTag(other, tag_pos)),
    }
}

impl RpcValue {
    fn into_json_else_undefined(self) -> Value {
        match self {
            RpcValue::Json(value) => value,
            // Upstream arrays can hold `undefined` slots; JSON cannot, and the
            // official JS array simply keeps an `undefined` element there.
            // Represent it as `null` so the JSON round-trip stays lossless for
            // every payload a channel actually sends.
            RpcValue::Undefined => Value::Null,
            RpcValue::Binary(bytes, kind) => serde_json::json!({
                "__chaos_rpc_binary__": match kind {
                    BinaryKind::Buffer => "buffer",
                    BinaryKind::VsBuffer => "vsbuffer",
                },
                "base64-data": bytes,
            }),
        }
    }
}

/// Parse one full RPC message: `serialize(header)` immediately followed by
/// `serialize(body)`, exactly what `ChannelClient.onBuffer` reads.
pub fn read_message(frame: &[u8]) -> Result<(RpcValue, RpcValue), ProtocolError> {
    let mut pos = 0usize;
    let header = read_chunk(frame, &mut pos)?;
    let body = read_chunk(frame, &mut pos)?;
    if pos != frame.len() {
        return Err(ProtocolError::TrailingBytes(frame.len() - pos));
    }
    Ok((header, body))
}

/// Build one full RPC message from a header and a body chunk.
pub fn write_message(header: &RpcValue, body: &RpcValue) -> Vec<u8> {
    let mut out = serialize_chunk(header);
    out.extend_from_slice(&serialize_chunk(body));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn vql_matches_upstream_examples() {
        // 0 → [0x00], 127 → [0x7F], 128 → [0x80, 0x01] (doc comment upstream).
        let mut out = Vec::new();
        write_vql(&mut out, 0);
        assert_eq!(out, vec![0x00]);
        out.clear();
        write_vql(&mut out, 127);
        assert_eq!(out, vec![0x7f]);
        out.clear();
        write_vql(&mut out, 128);
        assert_eq!(out, vec![0x80, 0x01]);
        out.clear();
        write_vql(&mut out, 300);
        assert_eq!(out, vec![0xac, 0x02]);
    }

    #[test]
    fn vql_reads_multi_byte_values() {
        for value in [0u32, 1, 127, 128, 300, 2147483, u32::MAX] {
            let mut out = Vec::new();
            write_vql(&mut out, value);
            let mut pos = 0usize;
            assert_eq!(read_vql(&out, &mut pos).unwrap(), value);
            assert_eq!(pos, out.len());
        }
    }

    #[test]
    fn negative_int32_mirrors_upstream_unsigned_round_trip() {
        // Documented upstream asymmetry: -1 encodes via `>>>` and reads back as
        // its unsigned 32-bit bit pattern.
        let value = RpcValue::Json(json!(-1i64));
        let bytes = serialize_chunk(&value);
        assert_eq!(bytes, vec![tag::INT, 0xff, 0xff, 0xff, 0xff, 0x0f]);
        let mut pos = 0usize;
        let decoded = read_chunk(&bytes, &mut pos).unwrap();
        assert_eq!(decoded, RpcValue::Json(json!(4294967295u64)));
    }

    #[test]
    fn message_round_trip_header_and_arg() {
        let header = RpcValue::Json(json!([rpc_type::PROMISE, 1, "File", "readFile"]));
        let body = RpcValue::Json(json!({ "path": "src/main.rs" }));
        let frame = write_message(&header, &body);
        let (dh, db) = read_message(&frame).unwrap();
        assert_eq!(dh, header);
        assert_eq!(db, body);
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        let frame = write_message(
            &RpcValue::Json(json!([rpc_type::PROMISE, 1, "A", "b"])),
            &RpcValue::Undefined,
        );
        let mut padded = frame.clone();
        padded.push(0xaa);
        assert!(matches!(
            read_message(&padded),
            Err(ProtocolError::TrailingBytes(1))
        ));
    }

    #[test]
    fn string_length_counts_utf8_bytes_not_chars() {
        let value = RpcValue::Json(json!("你好"));
        let bytes = serialize_chunk(&value);
        // tag + 1-byte VQL length (6 UTF-8 bytes) + 6 bytes.
        assert_eq!(bytes.len(), 1 + 1 + 6);
        assert_eq!(bytes[1], 6, "VQL length counts UTF-8 bytes");
        assert_eq!(&bytes[2..5], &[0xe4, 0xbd, 0xa0], "payload starts with 你");
        let mut pos = 0usize;
        assert_eq!(read_chunk(&bytes, &mut pos).unwrap(), value);
    }

    #[test]
    fn undefined_stays_distinct_from_json_null() {
        let undefined = serialize_chunk(&RpcValue::Undefined);
        assert_eq!(undefined, vec![tag::UNDEFINED]);
        let null = serialize_chunk(&RpcValue::Json(Value::Null));
        assert_eq!(null[0], tag::OBJECT);
        assert_eq!(&null[2..], b"null");
    }
}
