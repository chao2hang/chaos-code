//! Golden-frame interop tests against the **real** `@zcode/rpc` serializer.
//!
//! `fixtures/frames.json` was produced by running the official
//! `packages/rpc/src/serialization.ts` `serialize` (via tsx) on representative
//! payloads — request headers, arguments, success/error responses, events and
//! edge cases. Every case must survive `read_chunk` → `write_chunk` byte-for-
//! byte, and the framing-sensitive ones are additionally asserted structurally,
//! so a framing regression cannot hide behind a symmetric re-encode.

use serde_json::Value;
use zcode_rpc_protocol::{
    BinaryKind, ProtocolError, RpcValue, read_chunk, rpc_type, tag, write_chunk,
};

const FIXTURES: &str = include_str!("fixtures/frames.json");

fn case(name: &str) -> (Vec<u8>, String) {
    let frames: Value = serde_json::from_str(FIXTURES).expect("fixture JSON parses");
    let hex = frames
        .get(name)
        .unwrap_or_else(|| panic!("missing fixture case {name}"))
        .as_str()
        .expect("fixture value is a hex string");
    let bytes = (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("fixture hex is valid"))
        .collect();
    (bytes, hex.to_owned())
}

fn round_trip(name: &str) -> RpcValue {
    let (bytes, hex) = case(name);
    let mut pos = 0usize;
    let decoded = read_chunk(&bytes, &mut pos).expect("fixture frame deserializes");
    assert_eq!(pos, bytes.len(), "{name}: no trailing bytes expected");
    let mut reencoded = Vec::new();
    write_chunk(&mut reencoded, &decoded);
    assert_eq!(
        reencoded, bytes,
        "{name}: re-serialization must be byte-identical"
    );
    assert_eq!(
        reencoded
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        hex,
        "{name}: hex identity"
    );
    decoded
}

#[test]
fn request_header_promise_is_int_string_array() {
    let value = round_trip("request_header_promise");
    let items = match &value {
        RpcValue::Json(Value::Array(items)) => items.clone(),
        other => panic!("expected array header, got {other:?}"),
    };
    assert_eq!(items.len(), 4);
    assert_eq!(items[0], Value::from(rpc_type::PROMISE));
    assert_eq!(items[1], Value::from(1));
    assert_eq!(items[2], Value::from("File"));
    assert_eq!(items[3], Value::from("readFile"));
    // Framing sanity: array tag, count 4, then Int tag with VQL(100).
    let (bytes, _) = case("request_header_promise");
    assert_eq!(bytes[0], tag::ARRAY);
    assert_eq!(bytes[1], 0x04);
    assert_eq!(&bytes[2..4], &[tag::INT, 0x64]);
}

#[test]
fn event_listen_header_uses_multibyte_vql_id() {
    let value = round_trip("request_header_event_listen");
    let RpcValue::Json(Value::Array(items)) = &value else {
        panic!("expected array header")
    };
    assert_eq!(items[0], Value::from(rpc_type::EVENT_LISTEN));
    assert_eq!(items[1], Value::from(2147483));
    assert_eq!(items[2], Value::from("ZcodeTask"));
    let (bytes, _) = case("request_header_event_listen");
    // 2147483 = 0x20C09B → 4-byte VQL [0x9b, 0x89, 0x83, 0x01] after the Int tag.
    assert_eq!(
        &bytes[2..4],
        &[tag::INT, 0x66],
        "first element is Int(EVENT_LISTEN)"
    );
    assert_eq!(&bytes[5..9], &[0x9b, 0x89, 0x83, 0x01], "id VQL is 4 bytes");
}

#[test]
fn request_arg_object_is_json_fallback_chunk() {
    let value = round_trip("request_arg_object");
    let RpcValue::Json(Value::Object(map)) = &value else {
        panic!("expected object argument")
    };
    assert_eq!(map.get("path"), Some(&Value::from("src/main.rs")));
    assert_eq!(map.get("encoding"), Some(&Value::from("utf-8")));
    let (bytes, _) = case("request_arg_object");
    assert_eq!(bytes[0], tag::OBJECT);
}

#[test]
fn request_arg_undefined_is_single_zero_byte() {
    let value = round_trip("request_arg_undefined");
    assert_eq!(value, RpcValue::Undefined);
    let (bytes, _) = case("request_arg_undefined");
    assert_eq!(bytes, vec![tag::UNDEFINED]);
}

#[test]
fn response_success_preserves_json_types_inside_object_branch() {
    let value = round_trip("response_success");
    let RpcValue::Json(Value::Array(items)) = &value else {
        panic!("expected array response")
    };
    assert_eq!(items[0], Value::from(rpc_type::PROMISE_SUCCESS));
    assert_eq!(items[1], Value::from(1));
    let payload = &items[2];
    assert_eq!(payload["ok"], Value::Bool(true));
    assert_eq!(payload["text"], Value::from("你好，世界"));
    assert_eq!(payload["count"], Value::from(300));
    // 0.5 is a float: it must stay inside the Object-branch JSON text, never
    // become an Int chunk.
    assert_eq!(
        payload["ratio"],
        serde_json::from_str::<Value>("0.5").unwrap()
    );
    assert_eq!(
        payload["flags"],
        Value::Array(vec![Value::Bool(true), Value::Null, Value::from("x")])
    );
    let (bytes, _) = case("response_success");
    // Outer array(3) → Int 201 (2-byte VQL) → Int 1 → Object chunk with payload.
    assert_eq!(bytes[0], tag::ARRAY);
    assert_eq!(bytes[1], 0x03);
    assert_eq!(&bytes[2..5], &[tag::INT, 0xc9, 0x01]);
    assert_eq!(bytes[5], tag::INT);
    assert_eq!(bytes[7], tag::OBJECT);
}

#[test]
fn response_error_keeps_error_object() {
    let value = round_trip("response_error");
    let RpcValue::Json(Value::Array(items)) = &value else {
        panic!("expected array response")
    };
    assert_eq!(items[0], Value::from(rpc_type::PROMISE_ERROR));
    assert_eq!(items[2]["message"], Value::from("文件未找到"));
    assert_eq!(items[2]["name"], Value::from("FileNotFound"));
}

#[test]
fn event_fire_is_plain_array_with_string_payload() {
    let value = round_trip("event_fire");
    let RpcValue::Json(Value::Array(items)) = &value else {
        panic!("expected array event")
    };
    assert_eq!(items[0], Value::from(rpc_type::EVENT_FIRE));
    assert_eq!(items[1], Value::from(7));
    assert_eq!(items[2], Value::from("delta-text"));
}

#[test]
fn edge_empty_values_round_trip() {
    let value = round_trip("edge_empty");
    let RpcValue::Json(Value::Array(items)) = &value else {
        panic!("expected array")
    };
    assert_eq!(items[0], Value::from(""));
    assert_eq!(items[1], Value::from(0));
    assert_eq!(items[2], Value::Array(vec![]));
    assert_eq!(items[3], Value::Object(Default::default()));
    let (bytes, _) = case("edge_empty");
    // {} serializes to the two-character JSON text `{}` in the Object branch.
    assert!(bytes.windows(2).any(|w| w == b"{}"));
}

#[test]
fn nested_uint8array_marker_survives_verbatim() {
    let value = round_trip("edge_nested_buffer");
    let RpcValue::Json(Value::Object(map)) = &value else {
        panic!("expected object with nested buffer marker")
    };
    let blob = map.get("blob").expect("blob key present");
    assert_eq!(
        blob.get(zcode_rpc_protocol::NESTED_UINT8_ARRAY_MARKER),
        Some(&Value::Bool(true)),
        "the official marker object must pass through unmodified"
    );
    assert_eq!(blob["base64"], Value::from("AQID+g=="));
}

#[test]
fn edge_deep_round_trips_long_strings_and_floats() {
    let value = round_trip("edge_deep");
    let RpcValue::Json(Value::Object(map)) = &value else {
        panic!("expected object")
    };
    assert_eq!(map["a"]["b"][0], Value::from(1));
    assert_eq!(map["a"]["b"][1]["c"], Value::Bool(false));
    assert_eq!(
        map["a"]["b"][1]["d"],
        serde_json::from_str::<Value>("3.25").unwrap()
    );
    assert_eq!(map["e"], Value::from("x".repeat(300)));
}

#[test]
fn full_message_parse_mirrors_channelclient_onbuffer() {
    // The real wire carries header chunk + body chunk back to back in one
    // WebSocket frame; two fixture cases concatenated must parse as (header,
    // body) with no leftovers.
    let (header, _) = case("request_header_promise");
    let (body, _) = case("request_arg_object");
    let mut frame = header.clone();
    frame.extend_from_slice(&body);
    let (dh, db) = zcode_rpc_protocol::read_message(&frame).expect("message parses");
    assert!(matches!(dh, RpcValue::Json(Value::Array(_))));
    assert!(matches!(db, RpcValue::Json(Value::Object(_))));
}

#[test]
fn unknown_tag_is_a_clean_error() {
    let bytes = [0x7f, 0x00];
    let mut pos = 0usize;
    assert!(matches!(
        read_chunk(&bytes, &mut pos),
        Err(ProtocolError::UnknownTag(0x7f, 0))
    ));
}

#[test]
fn truncated_vql_is_eof_not_panic() {
    let bytes = [tag::INT, 0x80]; // continuation bit set, then nothing
    let mut pos = 0usize;
    assert!(matches!(
        read_chunk(&bytes, &mut pos),
        Err(ProtocolError::UnexpectedEof(2))
    ));
}

#[test]
fn binary_chunks_keep_their_kind() {
    for (kind, expected_tag) in [
        (BinaryKind::Buffer, tag::BUFFER),
        (BinaryKind::VsBuffer, tag::VSBUFFER),
    ] {
        let value = RpcValue::Binary(vec![0xde, 0xad], kind);
        let bytes = serialize_chunk_for_test(&value);
        assert_eq!(bytes, vec![expected_tag, 0x02, 0xde, 0xad]);
        let mut pos = 0usize;
        let decoded = read_chunk(&bytes, &mut pos).unwrap();
        assert_eq!(decoded, value);
    }
}

fn serialize_chunk_for_test(value: &RpcValue) -> Vec<u8> {
    let mut out = Vec::new();
    write_chunk(&mut out, value);
    out
}
