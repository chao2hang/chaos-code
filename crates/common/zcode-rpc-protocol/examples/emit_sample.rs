//! Emits one RPC message (header + body) as a single hex line on stdout so the
//! real `@zcode/rpc` JS deserializer can be run against it in a cross-language
//! interop check (`tests/interop-rust-to-js.mjs`).
//!
//! Payload mirrors what a Chaos bridge response looks like: a PromiseSuccess
//! header plus a JSON body covering strings, unicode, bool, null, a float and
//! an array — the value shapes the official `deserialize` must reproduce.

use serde_json::json;
use zcode_rpc_protocol::{RpcValue, serialize_chunk};

fn main() {
    let header = RpcValue::Json(json!([201u32, 42u32]));
    let body = RpcValue::Json(json!({
        "ok": true,
        "text": "你好，Chaos 桥接",
        "ratio": 0.5,
        "nothing": null,
        "flags": [true, false, null],
        "count": 2147483,
    }));
    let mut frame = serialize_chunk(&header);
    frame.extend_from_slice(&serialize_chunk(&body));
    let hex: String = frame.iter().map(|b| format!("{b:02x}")).collect();
    println!("{hex}");
}
