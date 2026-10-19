//! Server-side RPC state machine, mirroring `packages/rpc/src/channelServer.ts`.
//!
//! The core is deliberately **pure and clock-injected**: it never spawns tasks
//! and never reads the wall clock. The host (the Chaos web backend's tokio
//! runtime) drives it with three calls:
//!
//! 1. [`ChannelServer::on_frame`] for every inbound WebSocket frame — returns
//!    [`Outbound`] commands the host must execute (spawn a channel call, start
//!    an event listener, cancel a request) **or** wire frames to send back;
//! 2. [`ChannelServer::register_channel`] when a channel becomes available —
//!    returns requests that were buffered for it (upstream flushes pending
//!    requests on `registerChannel`);
//! 3. [`ChannelServer::expire_pending`] on a timer tick — emits the official
//!    "Unknown channel" error frame for buffered requests whose 1000 ms default
//!    deadline passed (upstream uses a per-request `setTimeout`).

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use crate::{ProtocolError, RpcValue, read_message, write_message};
use serde_json::{Value, json};

pub mod response_type {
    pub const INITIALIZE: u32 = 200;
    pub const PROMISE_SUCCESS: u32 = 201;
    pub const PROMISE_ERROR: u32 = 202;
    pub const PROMISE_ERROR_OBJ: u32 = 203;
    pub const EVENT_FIRE: u32 = 204;
}

/// Upstream default `timeoutDelay` for buffered unknown-channel requests.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(1000);

/// One parsed inbound request that must be routed to a channel.
#[derive(Debug, Clone, PartialEq)]
pub struct Incoming {
    pub id: u32,
    pub channel: String,
    pub name: String,
    pub arg: RpcValue,
    /// `true` for `EventListen` (102): the host keeps the subscription until a
    /// cancel/dispose arrives. `false` for one-shot `Promise` (100) calls.
    pub is_event_listen: bool,
}

/// What the host must do after a frame was processed.
#[derive(Debug, Clone, PartialEq)]
pub enum Outbound {
    /// Invoke `channel.name(arg)` once, then report the outcome through
    /// [`promise_success_frame`] / [`promise_error_frame`].
    Call(Incoming),
    /// Subscribe to `channel.name(arg)`; every emission goes out through
    /// [`event_fire_frame`] until a cancel arrives.
    Listen(Incoming),
    /// `PromiseCancel` (101) or `EventDispose` (103) for a request id — the
    /// host must cancel its task/subscription. Unknown ids are ignored, exactly
    /// like upstream `disposeActiveRequest`.
    Cancel { id: u32 },
}

#[derive(Debug)]
struct Pending {
    request: Incoming,
    deadline: Instant,
}

/// Buffers requests for channels that are not registered yet, mirroring the
/// upstream `pendingRequests` map and its timeout error.
#[derive(Debug)]
pub struct ChannelServer {
    channels: BTreeSet<String>,
    pending: Vec<Pending>,
    timeout: Duration,
}

impl ChannelServer {
    pub fn new(timeout: Duration) -> Self {
        Self {
            channels: BTreeSet::new(),
            pending: Vec::new(),
            timeout,
        }
    }

    /// The frame the official server sends on construction: `Initialize` with
    /// an `undefined` body.
    pub fn initialize_frame() -> Vec<u8> {
        write_message(
            &RpcValue::Json(json!([response_type::INITIALIZE])),
            &RpcValue::Undefined,
        )
    }

    /// PromiseSuccess (201) response frame for a completed call.
    pub fn promise_success_frame(id: u32, data: &RpcValue) -> Vec<u8> {
        write_message(
            &RpcValue::Json(json!([response_type::PROMISE_SUCCESS, id])),
            data,
        )
    }

    /// PromiseError (202) frame for a failed call. Upstream serializes
    /// `{message, name, stack: undefined, ...}` through JSON, which drops the
    /// `undefined` stack — so this frame carries only `name` and `message`.
    pub fn promise_error_frame(id: u32, name: &str, message: &str) -> Vec<u8> {
        write_message(
            &RpcValue::Json(json!([response_type::PROMISE_ERROR, id])),
            &RpcValue::Json(json!({ "name": name, "message": message })),
        )
    }

    /// PromiseErrorObj (203) frame for a non-`Error` rejection value.
    pub fn promise_error_obj_frame(id: u32, data: &RpcValue) -> Vec<u8> {
        write_message(
            &RpcValue::Json(json!([response_type::PROMISE_ERROR_OBJ, id])),
            data,
        )
    }

    /// EventFire (204) frame delivering one event emission.
    pub fn event_fire_frame(id: u32, data: &RpcValue) -> Vec<u8> {
        write_message(
            &RpcValue::Json(json!([response_type::EVENT_FIRE, id])),
            data,
        )
    }

    /// Register (or re-register) a channel name and return every buffered
    /// request that was waiting for it, in arrival order. The clock parameter
    /// exists for symmetry with the timeout API; flushing itself is timeless.
    pub fn register_channel(&mut self, channel: &str, _now: Instant) -> Vec<Incoming> {
        self.channels.insert(channel.to_owned());
        let mut flushed = Vec::new();
        self.pending.retain(|pending| {
            if pending.request.channel == channel {
                flushed.push(pending.request.clone());
                false
            } else {
                true
            }
        });
        flushed
    }

    pub fn knows_channel(&self, channel: &str) -> bool {
        self.channels.contains(channel)
    }

    /// Process one inbound RPC frame (header + body chunks) and return the
    /// host commands or immediate error frames it produced.
    pub fn on_frame(&mut self, frame: &[u8], now: Instant) -> Result<Vec<Outbound>, ProtocolError> {
        let (header, body) = read_message(frame)?;
        let RpcValue::Json(value) = &header else {
            return Err(ProtocolError::MalformedHeader("header must be an array"));
        };
        let Some(items) = value.as_array() else {
            return Err(ProtocolError::MalformedHeader("header must be an array"));
        };
        let kind_raw = items.first().and_then(Value::as_u64);
        const PROMISE: u64 = crate::rpc_type::PROMISE as u64;
        const PROMISE_CANCEL: u64 = crate::rpc_type::PROMISE_CANCEL as u64;
        const EVENT_LISTEN: u64 = crate::rpc_type::EVENT_LISTEN as u64;
        const EVENT_DISPOSE: u64 = crate::rpc_type::EVENT_DISPOSE as u64;
        let kind = match kind_raw {
            Some(PROMISE) => crate::rpc_type::PROMISE,
            Some(PROMISE_CANCEL) => crate::rpc_type::PROMISE_CANCEL,
            Some(EVENT_LISTEN) => crate::rpc_type::EVENT_LISTEN,
            Some(EVENT_DISPOSE) => crate::rpc_type::EVENT_DISPOSE,
            Some(other) => {
                return Err(ProtocolError::MalformedHeaderBox(format!(
                    "header[0] {other} is not a client request type"
                )));
            }
            None => {
                return Err(ProtocolError::MalformedHeader(
                    "header[0] must be a request type",
                ));
            }
        };

        match kind {
            crate::rpc_type::PROMISE | crate::rpc_type::EVENT_LISTEN => {
                if items.len() < 4 {
                    return Err(ProtocolError::MalformedHeader(
                        "request header needs [type, id, channelName, name]",
                    ));
                }
                let Some(id) = items[1].as_u64().map(|id| id as u32) else {
                    return Err(ProtocolError::MalformedHeader(
                        "header[1] must be a request id",
                    ));
                };
                let Some(channel) = items[2].as_str() else {
                    return Err(ProtocolError::MalformedHeader(
                        "header[2] must be a channel name",
                    ));
                };
                let Some(name) = items[3].as_str() else {
                    return Err(ProtocolError::MalformedHeader(
                        "header[3] must be a method name",
                    ));
                };
                let request = Incoming {
                    id,
                    channel: channel.to_owned(),
                    name: name.to_owned(),
                    arg: body,
                    is_event_listen: kind == crate::rpc_type::EVENT_LISTEN,
                };
                if self.channels.contains(channel) {
                    Ok(vec![if request.is_event_listen {
                        Outbound::Listen(request)
                    } else {
                        Outbound::Call(request)
                    }])
                } else {
                    // Upstream buffers the request and starts a timeout; on
                    // expiry it logs and — for Promise only — answers with the
                    // "Unknown channel" error. EventListen requests are dropped.
                    self.pending.push(Pending {
                        request: request.clone(),
                        deadline: now + self.timeout,
                    });
                    Ok(vec![])
                }
            }
            crate::rpc_type::PROMISE_CANCEL | crate::rpc_type::EVENT_DISPOSE => {
                let Some(id) = items.get(1).and_then(Value::as_u64).map(|id| id as u32) else {
                    return Err(ProtocolError::MalformedHeader(
                        "cancel header needs [type, id]",
                    ));
                };
                Ok(vec![Outbound::Cancel { id }])
            }
            _ => unreachable!("kind is narrowed to a client request type above"),
        }
    }

    /// Emit error frames for buffered requests whose deadline passed. Matches
    /// upstream: Promise requests get the "Unknown channel" PromiseError;
    /// EventListen requests are dropped silently.
    pub fn expire_pending(&mut self, now: Instant) -> Vec<Vec<u8>> {
        let mut expired = Vec::new();
        self.pending.retain(|pending| {
            if pending.deadline > now {
                return true;
            }
            if !pending.request.is_event_listen {
                expired.push(Self::promise_error_frame(
                    pending.request.id,
                    "Unknown channel",
                    &format!(
                        "Channel name '{}' timed out after {}ms",
                        pending.request.channel,
                        self.timeout.as_millis()
                    ),
                ));
            }
            false
        });
        expired
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{read_message, tag};
    use serde_json::Value;

    fn fixture(name: &str) -> Vec<u8> {
        let frames: Value =
            serde_json::from_str(include_str!("../tests/fixtures/frames.json")).unwrap();
        let hex = frames[name].as_str().unwrap();
        (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
            .collect()
    }

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn initialize_frame_matches_official_bytes() {
        assert_eq!(
            ChannelServer::initialize_frame(),
            fixture("server_initialize")
        );
        // Parse it back: header [200], body undefined.
        let (header, body) = read_message(&fixture("server_initialize")).unwrap();
        assert_eq!(header, RpcValue::Json(json!([response_type::INITIALIZE])));
        assert_eq!(body, RpcValue::Undefined);
    }

    #[test]
    fn promise_success_frame_matches_official_bytes() {
        let data = RpcValue::Json(json!({
            "ok": true, "text": "你好，Chaos 桥接", "ratio": 0.5,
            "nothing": null, "flags": [true, false, null], "count": 2147483,
        }));
        assert_eq!(
            ChannelServer::promise_success_frame(42, &data),
            fixture("server_promise_success")
        );
    }

    #[test]
    fn promise_error_frame_matches_official_bytes() {
        // stack: undefined is dropped by the official JSON stringify; the Rust
        // helper never emits it.
        assert_eq!(
            ChannelServer::promise_error_frame(
                5,
                "Unknown channel",
                "Channel name 'Foo' timed out after 1000ms"
            ),
            fixture("server_promise_error")
        );
    }

    #[test]
    fn event_fire_frame_matches_official_bytes() {
        let data = RpcValue::Json(json!({ "delta": "部分文本", "done": false }));
        assert_eq!(
            ChannelServer::event_fire_frame(7, &data),
            fixture("server_event_fire")
        );
    }

    #[test]
    fn promise_to_unknown_channel_buffers_then_errors_after_timeout() {
        let mut server = ChannelServer::new(DEFAULT_TIMEOUT);
        let now = t0();
        let frame = crate::write_message(
            &RpcValue::Json(json!([crate::rpc_type::PROMISE, 5, "Foo", "bar"])),
            &RpcValue::Undefined,
        );
        assert!(
            server.on_frame(&frame, now).unwrap().is_empty(),
            "buffered, no immediate output"
        );

        // Just before the deadline: nothing.
        assert!(
            server
                .expire_pending(now + DEFAULT_TIMEOUT - Duration::from_millis(1))
                .is_empty()
        );
        // At the deadline: the official Unknown-channel error frame.
        let expired = server.expire_pending(now + DEFAULT_TIMEOUT);
        assert_eq!(expired, vec![fixture("server_promise_error")]);
    }

    #[test]
    fn event_listen_to_unknown_channel_is_dropped_on_timeout() {
        let mut server = ChannelServer::new(DEFAULT_TIMEOUT);
        let now = t0();
        let frame = crate::write_message(
            &RpcValue::Json(json!([crate::rpc_type::EVENT_LISTEN, 9, "Foo", "onEvent"])),
            &RpcValue::Undefined,
        );
        assert!(server.on_frame(&frame, now).unwrap().is_empty());
        assert!(
            server.expire_pending(now + DEFAULT_TIMEOUT).is_empty(),
            "no error frame for listeners"
        );
    }

    #[test]
    fn registering_channel_flushes_buffered_requests() {
        let mut server = ChannelServer::new(DEFAULT_TIMEOUT);
        let now = t0();
        let frame = crate::write_message(
            &RpcValue::Json(json!([crate::rpc_type::PROMISE, 11, "File", "readFile"])),
            &RpcValue::Json(json!({ "path": "src/main.rs" })),
        );
        assert!(server.on_frame(&frame, now).unwrap().is_empty());

        let flushed = server.register_channel("File", now);
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].id, 11);
        assert_eq!(flushed[0].channel, "File");
        assert_eq!(flushed[0].name, "readFile");
        assert_eq!(
            flushed[0].arg,
            RpcValue::Json(json!({ "path": "src/main.rs" }))
        );
        // Deadline passed but the request was flushed — no timeout error.
        assert!(server.expire_pending(now + DEFAULT_TIMEOUT).is_empty());
    }

    #[test]
    fn promise_to_known_channel_dispatches_immediately() {
        let mut server = ChannelServer::new(DEFAULT_TIMEOUT);
        let now = t0();
        server.register_channel("File", now);
        let frame = crate::write_message(
            &RpcValue::Json(json!([crate::rpc_type::PROMISE, 3, "File", "readFile"])),
            &RpcValue::Json(json!({ "path": "a" })),
        );
        let out = server.on_frame(&frame, now).unwrap();
        assert_eq!(out.len(), 1);
        match &out[0] {
            Outbound::Call(request) => {
                assert_eq!(request.id, 3);
                assert!(!request.is_event_listen);
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn event_listen_and_dispose_pair_up() {
        let mut server = ChannelServer::new(DEFAULT_TIMEOUT);
        let now = t0();
        server.register_channel("Git", now);
        let listen = crate::write_message(
            &RpcValue::Json(json!([
                crate::rpc_type::EVENT_LISTEN,
                21,
                "Git",
                "onStatus"
            ])),
            &RpcValue::Undefined,
        );
        assert!(matches!(
            server.on_frame(&listen, now).unwrap()[0],
            Outbound::Listen(_)
        ));

        let dispose = crate::write_message(
            &RpcValue::Json(json!([crate::rpc_type::EVENT_DISPOSE, 21])),
            &RpcValue::Undefined,
        );
        assert_eq!(
            server.on_frame(&dispose, now).unwrap(),
            vec![Outbound::Cancel { id: 21 }]
        );

        let cancel = crate::write_message(
            &RpcValue::Json(json!([crate::rpc_type::PROMISE_CANCEL, 21])),
            &RpcValue::Undefined,
        );
        assert_eq!(
            server.on_frame(&cancel, now).unwrap(),
            vec![Outbound::Cancel { id: 21 }]
        );
    }

    #[test]
    fn malformed_headers_are_rejected() {
        let mut server = ChannelServer::new(DEFAULT_TIMEOUT);
        let now = t0();
        // Not an array.
        let bad = crate::write_message(&RpcValue::Json(json!("x")), &RpcValue::Undefined);
        assert!(matches!(
            server.on_frame(&bad, now),
            Err(ProtocolError::MalformedHeader(_))
        ));
        // Missing method name.
        let short = crate::write_message(
            &RpcValue::Json(json!([crate::rpc_type::PROMISE, 1, "File"])),
            &RpcValue::Undefined,
        );
        assert!(matches!(
            server.on_frame(&short, now),
            Err(ProtocolError::MalformedHeader(_))
        ));
        // Unknown request type.
        let unknown = crate::write_message(&RpcValue::Json(json!([999, 1])), &RpcValue::Undefined);
        assert!(matches!(
            server.on_frame(&unknown, now),
            Err(ProtocolError::MalformedHeaderBox(_))
        ));
    }

    #[test]
    fn error_object_frames_use_the_obj_tag() {
        // Non-Error rejection values travel as PromiseErrorObj with the value
        // verbatim (upstream `sendResponse` PromiseErrorObj branch).
        let frame = ChannelServer::promise_error_obj_frame(8, &RpcValue::Json(json!(7)));
        let (header, body) = read_message(&frame).unwrap();
        assert_eq!(
            header,
            RpcValue::Json(json!([response_type::PROMISE_ERROR_OBJ, 8]))
        );
        assert_eq!(body, RpcValue::Json(json!(7)));
        assert_eq!(frame[0], tag::ARRAY);
    }
}
