//! The mobile remote-control relay, self-hosted.
//!
//! The official phone client (`/remote/v4?sid=…&hash=…`) opens its cloud relay
//! with a small JSON envelope protocol (`auth_init` → `auth_challenge` →
//! `auth_response` → `auth_ack`, then `data` envelopes) and tunnels the exact
//! same binary `@zcode/rpc` frames the desktop bridge terminates inside
//! `rpc-frame` payloads (base64 + crc32 + seq/ack). The whole wire shape was
//! captured from a live session; `/tmp/pwtest/relay-capture.json` holds the
//! frozen transcript.
//!
//! This module plays the relay's role against the Chaos engine: it answers the
//! auth dance (any proof is accepted — the pairing URL is minted by this same
//! server), serves the phone's mobile-home bootstrap payloads from the engine's
//! session/workspace lists, and pipes `rpc-frame` traffic into the ordinary
//! bridge session through an in-memory duplex, so every desktop feature the
//! bridge already implements (approvals, terminal, git, attachments…) works on
//! the phone with zero bridge changes.

use std::collections::HashMap;
use std::path::PathBuf;

use axum::extract::ws::{Message, WebSocket};
use base64::Engine as _;
use futures_util::StreamExt;
use serde_json::{Value, json};

use crate::chaos_bridge;

/// How long one auth step may sit before the socket is dropped.
const AUTH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// `zcode-rpc` transport envelope type byte — `run_chaos_session`'s frames.
const MESSAGE_TYPE_REGULAR: u8 = zcode_rpc_protocol::MESSAGE_TYPE_REGULAR;

// ---------------------------------------------------------------------------
// In-memory duplex shaped like axum's WebSocket
// ---------------------------------------------------------------------------

/// One end of a connected pair. `run_chaos_session` treats it exactly like a
/// `WebSocket` (same `Message` items, same `axum::Error` error type); the relay
/// loop holds the other end and wraps/unwraps `rpc-frame` envelopes.
pub(crate) struct DuplexEnd {
    rx: tokio::sync::mpsc::UnboundedReceiver<Result<Message, axum::Error>>,
    tx: tokio::sync::mpsc::UnboundedSender<Result<Message, axum::Error>>,
}

/// A connected pair of ends: what one sends, the other receives.
pub(crate) fn duplex() -> (DuplexEnd, DuplexEnd) {
    let (a_tx, b_rx) = tokio::sync::mpsc::unbounded_channel::<Result<Message, axum::Error>>();
    let (b_tx, a_rx) = tokio::sync::mpsc::unbounded_channel::<Result<Message, axum::Error>>();
    (
        DuplexEnd { rx: a_rx, tx: a_tx },
        DuplexEnd { rx: b_rx, tx: b_tx },
    )
}

impl DuplexEnd {
    fn send(&mut self, msg: Message) -> Result<(), axum::Error> {
        self.tx.send(Ok(msg)).map_err(|_| broken_pipe())
    }
}

fn broken_pipe() -> axum::Error {
    axum::Error::new(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "relay duplex peer dropped",
    ))
}

impl futures_util::Stream for DuplexEnd {
    type Item = Result<Message, axum::Error>;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::pin::Pin::new(&mut self.rx).poll_recv(cx)
    }
}

impl futures_util::Sink<Message> for DuplexEnd {
    type Error = axum::Error;
    fn poll_ready(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn start_send(self: std::pin::Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        self.get_mut().send(item)
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
}

// ---------------------------------------------------------------------------
// Bridge handle: one workspace bridge behind rpc-frames
// ---------------------------------------------------------------------------

struct BridgeHandle {
    /// Raw binary frames the bridge produced, ready to be rpc-frame wrapped.
    bridge_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    /// Binary frames from the phone, unwrapped, toward the bridge.
    bridge_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    /// Next `seq`/`messageSeq` for desktop→phone rpc-frames (1-based).
    out_seq: u64,
}

impl BridgeHandle {
    /// Run the ordinary bridge session against one duplex end; expose the
    /// binary frame streams of the other end as plain channels.
    fn spawn(engine: chaos_engine::Engine, file_root: PathBuf) -> Self {
        let (session_end, relay_end) = duplex();
        // One pump owns the relay end: phone→bridge frames in, bridge→phone
        // binary frames out, whichever direction stirs first.
        let (phone_tx, mut phone_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let mut pump_end = relay_end;
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    bytes = phone_rx.recv() => {
                        let Some(bytes) = bytes else { break };
                        if pump_end.send(Message::Binary(bytes.into())).is_err() { break; }
                    }
                    frame = pump_end.next() => {
                        let Some(Ok(Message::Binary(bytes))) = frame else { break };
                        if out_tx.send(bytes.to_vec()).is_err() { break; }
                    }
                }
            }
        });
        tokio::spawn(chaos_bridge::run_chaos_session(
            session_end,
            None,
            Some(engine),
            file_root,
        ));
        BridgeHandle {
            bridge_rx: out_rx,
            bridge_tx: phone_tx,
            out_seq: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Relay session
// ---------------------------------------------------------------------------

/// Entry from the `/ws` sniffer: the first frame was a relay `auth_init`.
pub async fn relay_session(
    socket: WebSocket,
    first: Option<Message>,
    engine: chaos_engine::Engine,
    file_root: PathBuf,
) {
    let mut socket = socket;
    // -- auth dance ---------------------------------------------------------
    // The sniffer already consumed `auth_init` and handed it over here, or
    // the route directly upgraded and we wait for the first frame.
    let auth_init = match first {
        Some(Message::Text(text)) => match serde_json::from_str::<Value>(&text) {
            Ok(v) => v,
            Err(_) => return,
        },
        Some(_) => return,
        None => match tokio::time::timeout(AUTH_TIMEOUT, socket.recv()).await {
            Ok(Some(Ok(Message::Text(text)))) => match serde_json::from_str::<Value>(&text) {
                Ok(v) => v,
                Err(_) => return,
            },
            _ => return,
        },
    };
    if auth_init.get("type").and_then(Value::as_str) != Some("auth_init") {
        return;
    }
    let device_sid = auth_init
        .get("device_sid")
        .and_then(Value::as_str)
        .unwrap_or("d_unknown")
        .to_owned();
    let nonce: String = uuid::Uuid::new_v4().simple().to_string()[..22].to_owned();
    if send_json(
        &mut socket,
        json!({"type":"auth_challenge","server_ts":now_s(),"nonce":nonce}),
    )
    .await
    .is_err()
    {
        return;
    }
    // auth_response — the pairing URL minted by this server is the only way to
    // reach it, and the phone computes the proof from the hash we handed out.
    let _auth_response = match tokio::time::timeout(AUTH_TIMEOUT, socket.recv()).await {
        Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str::<Value>(&text).ok(),
        _ => return,
    };
    let terminal_sid = format!("t_{}", uuid::Uuid::new_v4().simple());
    if send_json(
        &mut socket,
        json!({
            "type":"auth_ack",
            "server_ts":now_s(),
            "device_sid":device_sid,
            "terminal_sid":terminal_sid,
            "pair_status":"matched",
        }),
    )
    .await
    .is_err()
    {
        return;
    }
    eprintln!("[remote-relay] 手机已配对 sid={device_sid}");
    // -- data loop ----------------------------------------------------------
    let mut bridges: HashMap<String, BridgeHandle> = HashMap::new();
    loop {
        // Drain bridge output first: pending frames go out before we block on
        // the socket again.
        let ids: Vec<String> = bridges.keys().cloned().collect();
        for id in ids {
            let Some(handle) = bridges.get_mut(&id) else { continue };
            while let Ok(bytes) = handle.bridge_rx.try_recv() {
                // 桥输出是 13 字节信封包住的 RPC chunk 流；剥掉信封，rpc-frame
                // 里放裸载荷（官方手机端如此约定）。
                let payloads = bridge_output_payloads(&bytes);
                for raw in payloads {
                    handle.out_seq += 1;
                    let seq = handle.out_seq;
                    let frame = rpc_frame(&id, 1, seq, seq, &raw);
                    if send_json(&mut socket, data_envelope(frame)).await.is_err() {
                        return;
                    }
                }
            }
        }
        // A phone frame or new bridge output, whichever comes first. Bridge
        // output also breaks the wait via a wakeup channel; the simplest correct
        // structure is a short poll interval — the relay is one phone.
        tokio::select! {
            biased;
            env = socket.recv() => {
                let Some(Ok(msg)) = env else { break };
                let Some(text) = envelope_text(msg) else { continue };
                let Ok(env) = serde_json::from_str::<Value>(&text) else { continue };
                match env.get("type").and_then(Value::as_str) {
                    Some("pair_status_query") => {
                        if send_json(&mut socket, json!({
                            "type":"pair_status_ack",
                            "server_ts":now_s(),
                            "pair_status":"matched",
                            "terminal_sid":"",
                        })).await.is_err() { break; }
                        continue;
                    }
                    Some("data") => {
                        let Some(payload) = env.get("payload").cloned() else { continue };
                        if handle_data(&mut socket, &engine, &file_root, &device_sid, payload, &mut bridges)
                            .await
                            .is_err()
                        {
                            break;
                        }
                        continue;
                    }
                    _ => continue,
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(20)) => {
                // Bridge output may have arrived; loop back to the drain.
                continue;
            }
        }
    }
    eprintln!(
        "[remote-relay] 连接结束，关闭 {} 个工作区桥",
        bridges.len()
    );
}

fn data_envelope(payload: Value) -> Value {
    json!({"type":"data","server_ts":now_ms(),"payload":payload})
}

fn envelope_text(msg: Message) -> Option<String> {
    match msg {
        Message::Text(text) => Some(text.to_string()),
        _ => None,
    }
}

async fn send_json(socket: &mut WebSocket, value: Value) -> Result<(), axum::Error> {
    socket
        .send(Message::Text(value.to_string().into()))
        .await
}

/// One `data` payload from the phone.
#[allow(clippy::too_many_arguments)]
async fn handle_data(
    socket: &mut WebSocket,
    engine: &chaos_engine::Engine,
    file_root: &PathBuf,
    device_sid: &str,
    payload: Value,
    bridges: &mut HashMap<String, BridgeHandle>,
) -> Result<(), axum::Error> {
    let kind = payload
        .get("zcode_type")
        .and_then(Value::as_str)
        .unwrap_or("");
    match kind {
        "bootstrap-request" | "workspace-list-request" => {
            let request_id = payload
                .get("requestId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let result = bootstrap_result(engine, device_sid, file_root);
            let ztype = if kind == "bootstrap-request" {
                "bootstrap-response"
            } else {
                "workspace-list-response"
            };
            send_json(socket, data_envelope(json!({
                "zcode_type":ztype,
                "requestId":request_id,
                "success":true,
                "result":result,
            })))
            .await
        }
        "workspace-bridge-open" => {
            let request_id = payload
                .get("requestId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let bridge_session_id = payload
                .get("bridgeSessionId")
                .and_then(Value::as_str)
                .unwrap_or("bridge-unknown")
                .to_owned();
            let bridge_generation = payload
                .get("bridgeGeneration")
                .and_then(Value::as_u64)
                .unwrap_or(1);
            let workspace_key = payload
                .get("workspaceKey")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let handle = BridgeHandle::spawn(engine.clone(), file_root.clone());
            bridges.insert(bridge_session_id.clone(), handle);
            send_json(socket, data_envelope(json!({
                "zcode_type":"workspace-bridge-ready",
                "requestId":request_id,
                "bridgeSessionId":bridge_session_id,
                "bridgeGeneration":bridge_generation,
                "bridge":{
                    "bridgeGeneration":bridge_generation,
                    "bridgeSessionId":bridge_session_id,
                    "kind":"local",
                    "workspaceKey":workspace_key,
                    "workspacePath":workspace_key,
                },
            })))
            .await
        }
        "rpc-frame" => {
            let bridge_session_id = payload
                .get("bridgeSessionId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let bridge_generation = payload
                .get("bridgeGeneration")
                .and_then(Value::as_u64)
                .unwrap_or(1);
            let message_seq = payload
                .get("messageSeq")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            // Ack first: the phone retransmits when acks lag.
            send_json(socket, data_envelope(json!({
                "zcode_type":"rpc-frame-ack",
                "bridgeSessionId":bridge_session_id,
                "bridgeGeneration":bridge_generation,
                "ackMessageSeq":message_seq,
            })))
            .await?;
            let Some(handle) = bridges.get_mut(&bridge_session_id) else {
                return Ok(());
            };
            let bytes = match decode_rpc_frame(&payload) {
                Ok(b) => b,
                Err(reason) => {
                    eprintln!("[remote-relay] rpc-frame 丢弃: {reason}");
                    return Ok(());
                }
            };
            let _ = handle.bridge_tx.send(bytes);
            Ok(())
        }
        // Flow control only; frames are never dropped on a local socket.
        "rpc-frame-ack" => Ok(()),
        // Desktop-side display state; nothing to mirror.
        "mobile-view-state-update" => Ok(()),
        "mobile-diagnostic" => Ok(()),
        "platform-request" => {
            let request_id = payload
                .get("requestId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let method = payload
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let result = match method.as_str() {
                "loadMcpFromUserDirectory" => json!({"servers":[]}),
                _ => json!({}),
            };
            send_json(socket, data_envelope(json!({
                "zcode_type":"platform-response",
                "requestId":request_id,
                "method":method,
                "result":result,
                "success":true,
            })))
            .await
        }
        other => {
            eprintln!("[remote-relay] 未处理 payload 类型: {other}");
            Ok(())
        }
    }
}

/// Decode and verify one `rpc-frame` payload into raw bridge bytes.
fn decode_rpc_frame(payload: &Value) -> Result<Vec<u8>, String> {
    let data_b64 = payload
        .get("dataBase64")
        .and_then(Value::as_str)
        .ok_or("缺 dataBase64")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_b64)
        .map_err(|e| format!("base64 解码失败: {e}"))?;
    if let Some(checksum) = payload.get("checksum") {
        let algorithm = checksum
            .get("algorithm")
            .and_then(Value::as_str)
            .unwrap_or("");
        let expected = checksum.get("value").and_then(Value::as_str).unwrap_or("");
        if algorithm == "crc32" && !expected.is_empty() {
            let actual = format!("{:08x}", crc32fast::hash(&bytes));
            if actual != expected {
                return Err(format!("crc32 不符: 期望 {expected} 实得 {actual}"));
            }
        }
    }
    // 官方手机端的 rpc-frame 载荷是裸 RPC chunk 流（首字节 04=ARRAY），不带
    // 13 字节传输信封；chaos_bridge 的传输层要信封，这里补上。
    Ok(zcode_rpc_protocol::write_frame(&bytes))
}

/// Wrap raw bridge bytes into one (unfragmented) `rpc-frame` payload.
fn rpc_frame(
    bridge_session_id: &str,
    bridge_generation: u64,
    seq: u64,
    message_seq: u64,
    bytes: &[u8],
) -> Value {
    json!({
        "zcode_type":"rpc-frame",
        "bridgeSessionId":bridge_session_id,
        "bridgeGeneration":bridge_generation,
        "seq":seq,
        "messageSeq":message_seq,
        "fragmentIndex":0,
        "fragmentCount":1,
        "messageBytes":bytes.len(),
        "checksum":{"algorithm":"crc32","value":format!("{:08x}", crc32fast::hash(bytes))},
        "dataBase64":base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

/// Split one bridge transport write into裸 RPC payloads (envelope stripped).
fn bridge_output_payloads(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut stream = zcode_rpc_protocol::ChunkStream::new();
    match stream.accept(bytes) {
        Ok(frames) => frames.into_iter().map(|f| f.payload).collect(),
        Err(_) => Vec::new(),
    }
}

fn now_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The mobile home's data source: the engine's sessions dressed in the
/// official bootstrap shape (captured: `taskId` is `sess_<uuid>`,
/// `displayStatus` ∈ idle/running/completed/error). `windowControlSessionId`
/// is the pairing `sid` itself in the official capture — the desktop window
/// identity the phone echoes in view-state updates.
///
/// The phone refuses to finish booting without a workspace to sync (its own
/// diagnostic calls it `desktop-bootstrap-timeout`), so a host with no engine
/// workspace yet reports the shared file root as its one open window — the
/// same "one window per process" story the desktop bridge tells.
fn bootstrap_result(
    engine: &chaos_engine::Engine,
    window_control_session_id: &str,
    file_root: &PathBuf,
) -> Value {
    use chaos_engine::{ClientMessage, ServerMessage};
    // 与桌面桥同款前置：先折叠 CLI/sqlite 持久会话，再列表（否则重启后全空）。
    engine.refresh_chaos_sessions();
    let mut workspaces: Vec<chaos_engine::WorkspaceInfo> = Vec::new();
    let mut sessions: Vec<chaos_engine::SessionInfo> = Vec::new();
    let list_id = || format!("relay-list-{}", uuid::Uuid::new_v4());
    for message in engine.handle(ClientMessage::ListWorkspaces { client_msg_id: list_id() }) {
        if let ServerMessage::Workspaces { workspaces: list, .. } = message {
            workspaces = list;
        }
    }
    for message in engine.handle(ClientMessage::ListSessions {
        client_msg_id: list_id(),
        workspace_id: None,
    }) {
        if let ServerMessage::Sessions { sessions: list, .. } = message {
            sessions = list;
        }
    }
    let tasks: Vec<Value> = sessions
        .iter()
        .map(|s| {
            let ws = workspaces
                .iter()
                .find(|w| Some(w.id) == s.workspace_id);
            // 任务必须带工作区 key（官方手机端按 workspacePath 归组）；
            // 无工作区的会话归到兜底窗口（file_root），与上面的合成一致。
            let task_path = s
                .workspace_path
                .clone()
                .or_else(|| ws.and_then(|w| w.path.clone()))
                .unwrap_or_else(|| file_root.display().to_string());
            // 注意：Zod 的 .optional() 拒绝 null —— 未读为空时整键缺席。
            let mut task = json!({
                "taskId": format!("sess_{}", s.session_id),
                "title": s.title,
                "displayStatus": display_status(engine, s.session_id),
                "provider": "glm",
                "createdAt": s.created_at_ms,
                "updatedAt": s.updated_at_ms,
                "workspaceKind": "local",
                "workspaceLabel": ws.map(|w| w.name.clone()).unwrap_or_else(|| "Chaos".to_owned()),
                "workspacePath": task_path,
                "archived": s.archived,
            });
            if let Some(unread) = s.unread_at {
                task["unreadAt"] = json!(unread);
            }
            task
        })
        .collect();
    let root = file_root.display().to_string();
    // 引擎工作区的 path 可能为 None（默认工作区没有落盘目录）；官方手机端
    // 用 workspacePath 当主键归组任务，空/null 都会让整包"看着成功却不 sync"。
    let mut workspaces_json: Vec<Value> = workspaces
        .iter()
        .map(|w| {
            let path = w
                .path
                .clone()
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| root.clone());
            json!({
                "kind":"local",
                "label":w.name,
                "workspacePath":path,
                "workspacePurpose":"project",
            })
        })
        .collect();
    if workspaces_json.is_empty() {
        workspaces_json.push(json!({
            "kind":"local",
            "label":"Chaos",
            "workspacePath":root,
            "workspacePurpose":"project",
        }));
    }
    let active_workspace_key = workspaces_json[0]
        .get("workspacePath")
        .and_then(Value::as_str)
        .filter(|p| !p.is_empty())
        .unwrap_or(&root)
        .to_owned();
    json!({
        "desktopAppVersion": crate::host_version(),
        "mobileViewState": {
            "activeWorkspaceKey": active_workspace_key,
            "updatedAt": now_ms(),
        },
        "tasks": tasks,
        "windowControlSessionId": window_control_session_id,
        "workspaces": workspaces_json,
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn display_status(engine: &chaos_engine::Engine, session_id: uuid::Uuid) -> &'static str {
    if engine.session_is_running(session_id) {
        "running"
    } else {
        "completed"
    }
}
