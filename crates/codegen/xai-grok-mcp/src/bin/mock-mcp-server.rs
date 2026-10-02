//! Minimal real MCP server over stdio, used as the peer for end-to-end tests
//! in `tests/real_mcp_server_e2e.rs`.
//!
//! It is deliberately independent of `rmcp`: the point of these tests is that
//! the shipped client performs the handshake and the tool round trip correctly,
//! so the peer only has to be correct on the wire. Framing matches
//! `ResilientRwTransport` in `servers.rs`: one JSON-RPC message per line.
//!
//! Deliberately blocking on `std` only — no tokio, no workspace deps beyond
//! `serde_json` — so building it cannot perturb the crate's feature graph.

use std::io::{self, BufRead, Write};

use serde_json::{Value, json};

/// Written into `serverInfo` so a test can prove it talked to this fixture.
const SERVER_NAME: &str = "grok-test-fixture";

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let Ok(line) = line else { return };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        // Responses and notifications sent to us get no reply.
        let Some(id) = msg.get("id").filter(|v| !v.is_null()).cloned() else {
            continue;
        };
        let Some(response) = handle(&msg) else {
            continue;
        };
        let mut envelope = json!({ "jsonrpc": "2.0", "id": id });
        match response {
            Ok(result) => envelope["result"] = result,
            Err((code, message)) => {
                envelope["error"] = json!({ "code": code, "message": message });
            }
        }
        if writeln!(out, "{envelope}").is_err() {
            return;
        }
        if out.flush().is_err() {
            return;
        }
    }
}

/// `Ok(result)` for a JSON-RPC result, `Err((code, message))` for a failure.
fn handle(msg: &Value) -> Option<Result<Value, (i64, String)>> {
    match msg["method"].as_str()? {
        "initialize" => Some(Ok(json!({
            // Echo the client's version so the handshake never depends on
            // which revision the pinned client happens to offer.
            "protocolVersion": msg["params"]["protocolVersion"],
            "capabilities": { "tools": {} },
            "serverInfo": { "name": SERVER_NAME, "version": "0.0.0" },
        }))),
        "ping" => Some(Ok(json!({}))),
        "tools/list" => Some(Ok(json!({
            "tools": [
                {
                    "name": "echo",
                    "description": "Echoes its text argument back.",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "text": { "type": "string" } },
                        "required": ["text"],
                    }
                },
                {
                    "name": "always_fails",
                    "description": "Returns an MCP tool error, not a protocol error.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    // Lets a test read the real PID of the process it spawned
                    // through the tool-call path itself, then check that the
                    // process is gone once the client is dropped.
                    "name": "server_pid",
                    "description": "Reports the PID of this server process.",
                    "inputSchema": { "type": "object", "properties": {} }
                }
            ]
        }))),
        "tools/call" => Some(tools_call(msg)),
        other => Some(Err((
            -32601,
            format!("mock MCP server: unexpected method {other}"),
        ))),
    }
}

fn tools_call(msg: &Value) -> Result<Value, (i64, String)> {
    let name = msg["params"]["name"].as_str().unwrap_or_default();
    match name {
        "echo" => {
            let text = msg["params"]["arguments"]["text"]
                .as_str()
                .unwrap_or_default();
            Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": false }))
        }
        "always_fails" => Ok(json!({
            "content": [{ "type": "text", "text": "fixture reports a tool failure" }],
            "isError": true,
        })),
        "server_pid" => Ok(json!({
            "content": [{ "type": "text", "text": std::process::id().to_string() }],
            "isError": false,
        })),
        other => Err((-32602, format!("unknown tool {other}"))),
    }
}
