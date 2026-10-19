//! Real-handshake replay: every frame in `fixtures/handshake-capture.json` was
//! captured from the **actual official ZCode frontend** (packages/web + ui,
//! v3.14.3) talking to the official server through a logging proxy, in a real
//! Chromium session. The Rust bridge must understand every one of these bytes
//! before it can claim to host the official UI.
//!
//! The capture also fixes the boot contract for R3: which channels the shell
//! calls, in which order, and with which arguments.

use serde_json::Value;
use zcode_rpc_protocol::framing::ChunkStream;
use zcode_rpc_protocol::{BinaryKind, RpcValue, read_message, rpc_type};

const CAPTURE: &str = include_str!("fixtures/handshake-capture.json");

struct CapturedFrame {
    from_client: bool,
    bytes: Vec<u8>,
}

fn capture() -> Vec<CapturedFrame> {
    let frames: Value = serde_json::from_str(CAPTURE).expect("capture JSON parses");
    frames
        .as_array()
        .expect("capture is an array")
        .iter()
        .map(|entry| {
            let hex = entry["hex"].as_str().expect("hex string");
            CapturedFrame {
                from_client: entry["dir"].as_str() == Some("C->S"),
                bytes: (0..hex.len() / 2)
                    .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex byte"))
                    .collect(),
            }
        })
        .collect()
}

fn inner_payload(bytes: &[u8]) -> Vec<u8> {
    // 13-byte envelope: [type][id BE][ack BE][len BE].
    assert_eq!(bytes[0], 1, "every captured frame is Regular type");
    let len = u32::from_be_bytes(bytes[9..13].try_into().unwrap()) as usize;
    assert_eq!(bytes.len(), 13 + len, "envelope length is exact");
    bytes[13..].to_vec()
}

fn header_array(bytes: &[u8]) -> Vec<Value> {
    let (header, _) = read_message(&inner_payload(bytes)).expect("RPC message parses");
    match header {
        RpcValue::Json(Value::Array(items)) => items,
        other => panic!("header is an array, got {other:?}"),
    }
}

fn body_value(bytes: &[u8]) -> Value {
    let (_, body) = read_message(&inner_payload(bytes)).expect("RPC message parses");
    to_json(body)
}

fn to_json(value: RpcValue) -> Value {
    match value {
        RpcValue::Json(value) => value,
        RpcValue::Undefined => Value::Null,
        RpcValue::Binary(bytes, kind) => serde_json::json!({
            "binary": match kind { BinaryKind::Buffer => "buffer", BinaryKind::VsBuffer => "vsbuffer" },
            "base64-data": bytes,
        }),
    }
}

#[test]
fn server_initialize_is_the_first_frame_and_matches_our_encoding() {
    let frames = capture();
    let first = &frames[0];
    assert!(!first.from_client, "server speaks first");
    // header [200] + body undefined — byte-identical to ChannelServer::initialize_frame.
    let (header, body) = read_message(&inner_payload(&first.bytes)).unwrap();
    assert_eq!(
        header,
        RpcValue::Json(serde_json::json!([rpc_type::INITIALIZE]))
    );
    assert_eq!(body, RpcValue::Undefined);
    assert_eq!(
        inner_payload(&first.bytes),
        zcode_rpc_protocol::server::ChannelServer::initialize_frame(),
        "official Initialize bytes == our initialize_frame()"
    );
}

#[test]
fn boot_call_sequence_matches_the_recorded_contract() {
    let frames = capture();
    // Frame 1: EventListen #0 broadcast.onMessage.
    let h = header_array(&frames[1].bytes);
    assert!(frames[1].from_client);
    assert_eq!(h[0], serde_json::json!(rpc_type::EVENT_LISTEN));
    assert_eq!(h[1], serde_json::json!(0));
    assert_eq!(h[2], serde_json::json!("broadcast"));
    assert_eq!(h[3], serde_json::json!("onMessage"));
    assert_eq!(
        body_value(&frames[1].bytes),
        Value::Null,
        "listen arg is undefined"
    );

    // Frame 2: Promise #1 setting.get with an empty-array argument.
    let h = header_array(&frames[2].bytes);
    assert_eq!(h[0], serde_json::json!(rpc_type::PROMISE));
    assert_eq!(h[2], serde_json::json!("setting"));
    assert_eq!(h[3], serde_json::json!("get"));
    assert_eq!(body_value(&frames[2].bytes), Value::Array(vec![]));

    // Frames 3-4: onboarding-record.claimAnonymousRecord / getLatestEntry.
    let h = header_array(&frames[3].bytes);
    assert_eq!(h[2], serde_json::json!("onboarding-record"));
    assert_eq!(h[3], serde_json::json!("claimAnonymousRecord"));
    let h = header_array(&frames[4].bytes);
    assert_eq!(h[2], serde_json::json!("onboarding-record"));
    assert_eq!(h[3], serde_json::json!("getLatestEntry"));

    // Frame 6-7: model-selection.onDidChange listener + getView call whose arg
    // is `[undefined]` — the official client sends an array wrapping undefined.
    let h = header_array(&frames[6].bytes);
    assert_eq!(h[2], serde_json::json!("model-selection"));
    assert_eq!(h[3], serde_json::json!("onDidChange"));
    let h = header_array(&frames[7].bytes);
    assert_eq!(h[3], serde_json::json!("getView"));
    assert_eq!(
        body_value(&frames[7].bytes),
        Value::Array(vec![Value::Null])
    );

    // Frame 31: onboarding-record.shouldOnboard — the welcome-wall gate.
    let h = header_array(&frames[31].bytes);
    assert_eq!(h[2], serde_json::json!("onboarding-record"));
    assert_eq!(h[3], serde_json::json!("shouldOnboard"));
    assert_eq!(
        body_value(&frames[31].bytes),
        Value::Array(vec![Value::from("Linux x86_64|1440|900|24")])
    );
}

#[test]
fn boot_channels_cover_the_expected_set() {
    let frames = capture();
    let mut channels: Vec<String> = frames
        .iter()
        .filter(|f| f.from_client)
        .map(|f| {
            let h = header_array(&f.bytes);
            h.get(2).and_then(Value::as_str).unwrap_or("").to_owned()
        })
        .filter(|c| !c.is_empty())
        .collect();
    channels.sort();
    channels.dedup();
    for expected in [
        "broadcast",
        "bots",
        "coding-plan-subscription",
        "credential",
        "model-selection",
        "oauth",
        "onboarding-record",
        "provider-settings",
        "setting",
        "zcode-agent",
    ] {
        assert!(
            channels.contains(&expected.to_owned()),
            "boot calls {expected}"
        );
    }
}

#[test]
fn server_answers_are_promise_success_frames() {
    let frames = capture();
    // Every server frame after Initialize is PromiseSuccess [201, id].
    for frame in frames.iter().skip(1).filter(|f| !f.from_client) {
        let h = header_array(&frame.bytes);
        assert_eq!(
            h[0],
            serde_json::json!(rpc_type::PROMISE_SUCCESS),
            "frame header {h:?}"
        );
    }
    // Spot-check the onboarding gate answer: shouldOnboard (#20) → false.
    // Skip the Initialize frame — its header is `[200]` and has no id slot.
    let answer = frames
        .iter()
        .skip(1)
        .find(|f| !f.from_client && header_array(&f.bytes)[1] == serde_json::json!(20))
        .expect("shouldOnboard answered");
    assert_eq!(body_value(&answer.bytes), serde_json::json!(false));
}

#[test]
fn every_captured_frame_survives_our_chunk_stream() {
    // Concatenate ALL captured wire bytes (client direction only, like a
    // socket would deliver) and confirm the chunk stream re-splits them into
    // exactly the captured frames with identical payloads.
    let frames = capture();
    let mut wire = Vec::new();
    let mut expected_payloads = Vec::new();
    for frame in frames.iter().filter(|f| f.from_client) {
        wire.extend_from_slice(&frame.bytes);
        expected_payloads.push(inner_payload(&frame.bytes));
    }

    // Real sockets split TCP however they like: feed with a mid-frame cut to
    // prove reassembly is position-independent.
    let mut stream = ChunkStream::new();
    let cut = wire.len() / 2 + 3; // inside the second frame's payload
    let mut got = stream.accept(&wire[..cut]).unwrap();
    got.extend(stream.accept(&wire[cut..]).unwrap());

    assert_eq!(
        got.len(),
        expected_payloads.len(),
        "no frames lost or split"
    );
    for (frame, expected) in got.iter().zip(&expected_payloads) {
        assert_eq!(&frame.payload, expected);
    }
    assert_eq!(stream.buffered_bytes(), 0);
}

#[test]
fn capture_statistics_are_sane() {
    let frames = capture();
    assert_eq!(frames.len(), 57, "the recorded session has 57 frames");
    let client = frames.iter().filter(|f| f.from_client).count();
    let server = frames.len() - client;
    assert_eq!(client, 32, "client frames");
    assert_eq!(
        server, 25,
        "server frames (1 Initialize + 24 PromiseSuccess)"
    );
}

#[test]
fn framing_and_protocol_errors_are_clean_on_capture_bytes() {
    let frames = capture();
    // A truncated envelope must not panic the chunk stream.
    let mut stream = ChunkStream::new();
    let short = &frames[1].bytes[..20]; // header says 29 payload bytes
    let got = stream.accept(short).unwrap();
    assert!(got.is_empty(), "incomplete frame stays buffered");
    assert_eq!(stream.buffered_bytes(), short.len());
}
